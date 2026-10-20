use crate::{fonts, view};
use eframe::egui::{self, ColorImage};
use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use whostty_core::{
    config::{Profile, Settings},
    session::{ExitState, Session, SessionEvent},
    terminal::Size,
    theme::Palette,
};

pub type Output = Arc<Mutex<Option<Value>>>;

const INPUT_PROMPT: &str = "whostty-native-check-input-ready";
const SUCCESS_MARKER: &str = "whostty-native-check-ready";
const CHECK_INPUT: &str = "whostty-input-한글-e\u{301}-😀";
const WARMUP_SAMPLES: usize = 20;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(15);
const SAMPLE_TIMEOUT: Duration = Duration::from_secs(5);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(600);
const MEASUREMENT_SCOPE: &str = "Application pipeline latency from immediately before Session::write through a disposable shell/PTY to GUI acknowledgement observation and GPU screenshot readback. Includes input queuing, shell execution, PTY output, terminal parsing, GUI scheduling, shaping/rendering and readback. Not physical keystroke-to-photon latency or pure GPU frame time.";

fn probe_token(index: usize) -> Vec<u8> {
    format!("whostty-request-{index:04}\r").into_bytes()
}

fn probe_ack(index: usize) -> String {
    format!("whostty-ack-{index:04}-ready")
}

fn probe_profile(requests: usize) -> Profile {
    #[cfg(windows)]
    {
        let script = format!(
            "$ErrorActionPreference = 'Stop'; \
             [Console]::InputEncoding = [Text.UTF8Encoding]::new($false); \
             [Console]::WriteLine('{INPUT_PROMPT}'); \
             $value = [Console]::ReadLine(); \
             if (-not [string]::Equals($value, '{CHECK_INPUT}', [StringComparison]::Ordinal)) {{ exit 9 }}; \
             [Console]::WriteLine(([string][char]27) + '[38;2;12;200;34m{SUCCESS_MARKER}' + [char]27 + '[0m'); \
             for ($i = 0; $i -lt {requests}; $i++) {{ \
                 $value = [Console]::ReadLine(); \
                 $number = $i.ToString('D4', [Globalization.CultureInfo]::InvariantCulture); \
                 if (-not [string]::Equals($value, ('whostty-request-' + $number), [StringComparison]::Ordinal)) {{ exit 10 }}; \
                 $color = if ($i % 2 -eq 0) {{ '12;200;34' }} else {{ '210;40;210' }}; \
                 [Console]::Write(([string][char]27) + '[2J' + [char]27 + '[H' + [char]27 + '[38;2;' + $color + 'mwhostty-ack-' + $number + '-ready' + [char]27 + '[0m' + [char]13 + [char]10) \
             }}; exit 0"
        );
        Profile::new(
            "Runtime check",
            "powershell.exe",
            &[
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &script,
            ],
        )
    }
    #[cfg(not(windows))]
    {
        let script = format!(
            "printf '%s\\n' '{INPUT_PROMPT}'; \
             IFS= read -r value; [ \"$value\" = '{CHECK_INPUT}' ] || exit 9; \
             printf '\\033[38;2;12;200;34m{SUCCESS_MARKER}\\033[0m\\r\\n'; \
             i=0; while [ \"$i\" -lt {requests} ]; do \
                 IFS= read -r value || exit 10; \
                 expected=$(printf 'whostty-request-%04d' \"$i\"); \
                 [ \"$value\" = \"$expected\" ] || exit 10; \
                 if [ $((i % 2)) -eq 0 ]; then color='12;200;34'; else color='210;40;210'; fi; \
                 printf '\\033[2J\\033[H\\033[38;2;%smwhostty-ack-%04d-ready\\033[0m\\r\\n' \"$color\" \"$i\"; \
                 i=$((i + 1)); \
             done; exit 0"
        );
        Profile::new("Runtime check", "/bin/sh", &["-c", &script])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScreenshotId {
    NativeCheck,
    Latency(usize),
}

fn screenshot_id(data: &egui::UserData) -> Option<ScreenshotId> {
    data.data.as_ref()?.downcast_ref::<ScreenshotId>().copied()
}

fn marker_pixels(image: &ColorImage, index: usize) -> usize {
    image
        .pixels
        .iter()
        .filter(|pixel| {
            if index % 2 == 0 {
                pixel.r() < 80 && pixel.g() > 130 && pixel.b() < 100
            } else {
                pixel.r() > 130 && pixel.g() < 100 && pixel.b() > 130
            }
        })
        .count()
}

fn latency_summary(values: &[f64]) -> Value {
    if values.is_empty() {
        return Value::Null;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable_by(f64::total_cmp);
    // Nearest-rank percentiles retain measured values rather than interpolate.
    let percentile = |percent: usize| sorted[(sorted.len() * percent).div_ceil(100) - 1];
    json!({
        "min":sorted[0],
        "p50":percentile(50),
        "p95":percentile(95),
        "p99":percentile(99),
        "max":sorted[sorted.len() - 1],
    })
}

struct PendingSample {
    index: usize,
    written: Instant,
    ack_observed: Option<Duration>,
    readback_requested: bool,
}

struct LatencyProbe {
    samples: usize,
    completed: usize,
    pending: Option<PendingSample>,
    ack_ms: Vec<f64>,
    gpu_ms: Vec<f64>,
    last_image: Option<Arc<ColorImage>>,
    finished_at: Option<Instant>,
    stale_screenshots: usize,
}

impl LatencyProbe {
    fn new(samples: usize) -> Self {
        Self {
            samples,
            completed: 0,
            pending: None,
            ack_ms: Vec::with_capacity(samples),
            gpu_ms: Vec::with_capacity(samples),
            last_image: None,
            finished_at: None,
            stale_screenshots: 0,
        }
    }

    fn total(&self) -> usize {
        WARMUP_SAMPLES + self.samples
    }

    fn observe_ack(&mut self, text: &str, observed: Instant) {
        if let Some(pending) = &mut self.pending {
            if pending.ack_observed.is_none() && text.contains(&probe_ack(pending.index)) {
                pending.ack_observed = Some(observed.duration_since(pending.written));
            }
        }
    }

    fn request_readback(&mut self) -> Option<egui::UserData> {
        let pending = self.pending.as_mut()?;
        if pending.ack_observed.is_none() || pending.readback_requested {
            return None;
        }
        pending.readback_requested = true;
        Some(egui::UserData::new(ScreenshotId::Latency(pending.index)))
    }

    fn accept_readback(
        &mut self,
        data: &egui::UserData,
        image: Arc<ColorImage>,
        observed: Instant,
    ) -> Result<bool, String> {
        let Some(pending) = &self.pending else {
            self.stale_screenshots += 1;
            return Ok(false);
        };
        if !pending.readback_requested
            || screenshot_id(data) != Some(ScreenshotId::Latency(pending.index))
        {
            self.stale_screenshots += 1;
            return Ok(false);
        }
        let colored = marker_pixels(&image, pending.index);
        let previous = marker_pixels(&image, pending.index + 1);
        if image.size[0] < 200 || image.size[1] < 100 || colored < 20 || previous >= 20 {
            return Err(format!(
                "Latency sample {} GPU readback did not contain the new marker: image {:?}, expected-color pixels {colored}, previous-color pixels {previous}",
                pending.index + 1, image.size,
            ));
        }
        let Some(ack) = pending.ack_observed else {
            return Err("GPU readback arrived before a latency acknowledgement".into());
        };
        if pending.index >= WARMUP_SAMPLES {
            self.ack_ms.push(ack.as_secs_f64() * 1000.0);
            self.gpu_ms
                .push(observed.duration_since(pending.written).as_secs_f64() * 1000.0);
        }
        self.completed += 1;
        self.pending = None;
        self.last_image = Some(image);
        if self.completed == self.total() {
            self.finished_at = Some(observed);
        }
        Ok(true)
    }

    fn timeout(&self, started: Instant, now: Instant, ready: bool) -> Option<String> {
        if now.duration_since(started) > TOTAL_TIMEOUT {
            return Some(format!(
                "Latency probe exceeded its {} s total timeout after {}/{} iterations",
                TOTAL_TIMEOUT.as_secs(),
                self.completed,
                self.total(),
            ));
        }
        if !ready && now.duration_since(started) > STARTUP_TIMEOUT {
            return Some("Timed out waiting for the Unicode PTY startup probe".into());
        }
        if let Some(pending) = &self.pending {
            if now.duration_since(pending.written) > SAMPLE_TIMEOUT {
                return Some(format!(
                    "Latency sample {}/{} timed out waiting for {}",
                    pending.index + 1,
                    self.total(),
                    if pending.ack_observed.is_none() {
                        "PTY acknowledgement"
                    } else {
                        "GPU readback"
                    },
                ));
            }
        }
        if self
            .finished_at
            .is_some_and(|finished| now.duration_since(finished) > SAMPLE_TIMEOUT)
        {
            return Some("Timed out waiting for the latency probe shell to exit 0".into());
        }
        None
    }

    fn report(&self) -> Value {
        json!({
            "measurement_scope":MEASUREMENT_SCOPE,
            "unit":"ms",
            "percentile_method":"nearest_rank",
            "requested_samples":self.samples,
            "samples":self.gpu_ms.len(),
            "warmup_samples":WARMUP_SAMPLES,
            "warmup_samples_completed":self.completed.min(WARMUP_SAMPLES),
            "stale_screenshots_ignored":self.stale_screenshots,
            "write_to_ack_observed":latency_summary(&self.ack_ms),
            "write_to_gpu_readback":latency_summary(&self.gpu_ms),
        })
    }
}

fn apply_pending_scale(ctx: &egui::Context, pending: &mut Option<f32>) -> bool {
    let Some(scale) = pending.take() else {
        return false;
    };
    // Native monitor scale is available only after the first RawInput. The zoom
    // change takes effect next pass, so defer all probe/render work until then.
    ctx.set_pixels_per_point(scale);
    ctx.request_repaint();
    true
}

pub struct CheckApp {
    session: Session,
    view: view::ViewState,
    palette: Palette,
    output: Output,
    info: Value,
    started: Instant,
    input_sent: bool,
    ready: bool,
    requested_scale: Option<f32>,
    pending_scale: Option<f32>,
    requested: bool,
    latency: Option<LatencyProbe>,
    done: bool,
}

impl CheckApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        output: Output,
        scale: Option<f32>,
        samples: Option<usize>,
    ) -> Result<Self, std::io::Error> {
        if samples.is_some_and(|samples| !(10..=2000).contains(&samples)) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Latency sample count must be between 10 and 2000",
            ));
        }
        // This isolated check never loads/saves user configuration or starts a user profile.
        let settings = Settings::default();
        let _ = fonts::configure_fonts(&cc.egui_ctx, &settings);
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let repaint = cc.egui_ctx.clone();
        let session = Session::spawn(
            &probe_profile(samples.map_or(0, |samples| WARMUP_SAMPLES + samples)),
            Size::new(80, 24),
            100,
            Arc::new(move || repaint.request_repaint()),
        )
        .map_err(|error| std::io::Error::other(format!("Runtime check PTY: {error:#}")))?;
        let info = if let Some(state) = &cc.wgpu_render_state {
            let adapter = state.adapter.get_info();
            json!({"adapter":adapter.name,"backend":format!("{:?}",adapter.backend),"device_type":format!("{:?}",adapter.device_type),"os":std::env::consts::OS,"arch":std::env::consts::ARCH})
        } else {
            json!({"os":std::env::consts::OS,"arch":std::env::consts::ARCH})
        };
        Ok(Self {
            session,
            view: Default::default(),
            palette: settings.theme.colors(),
            output,
            info,
            started: Instant::now(),
            input_sent: false,
            ready: false,
            requested_scale: scale,
            pending_scale: scale,
            requested: false,
            latency: samples.map(LatencyProbe::new),
            done: false,
        })
    }

    fn finish(
        &mut self,
        ctx: &egui::Context,
        pty: bool,
        image: Option<&ColorImage>,
        error: Option<&str>,
    ) {
        let marker_index = self
            .latency
            .as_ref()
            .and_then(|probe| probe.completed.checked_sub(1))
            .unwrap_or(0);
        let colored = image.map_or(0, |image| marker_pixels(image, marker_index));
        let rendered = image.is_some_and(|image| image.size[0] >= 200 && image.size[1] >= 100)
            && colored >= 20;
        let (glyphs, layouts, runs) = fonts::Typography::get(ctx)
            .shaping
            .as_ref()
            .map(|renderer| {
                renderer
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .diagnostics()
            })
            .unwrap_or_default();
        let shaped = glyphs > 0 && runs > 0;
        let roundtrip = pty && self.input_sent;
        let scale_matches = self
            .requested_scale
            .is_none_or(|scale| (ctx.pixels_per_point() - scale).abs() < 0.01);
        let error = error.or({
            if !pty {
                Some("PTY probe did not complete successfully")
            } else if !roundtrip {
                Some("Unicode PTY input was not verified")
            } else if !scale_matches {
                Some("Rendering scale did not match the requested scale")
            } else if !rendered {
                Some("GPU readback did not contain the colored PTY probe")
            } else if !shaped {
                Some("Shaped glyph renderer did not draw the PTY probe")
            } else {
                None
            }
        });
        let mut report = self.info.clone();
        report["ok"] = json!(pty && rendered && error.is_none());
        report["pty"] = json!(pty);
        report["input_roundtrip"] = json!(roundtrip);
        report["native_renderer"] = json!(rendered);
        report["text_shaping"] = json!(shaped);
        report["glyphs_cached"] = json!(glyphs);
        report["pixels_per_point"] = json!(ctx.pixels_per_point());
        report["native_pixels_per_point"] = json!(ctx.native_pixels_per_point());
        report["requested_scale"] = json!(self.requested_scale);
        report["layouts_cached"] = json!(layouts);
        report["image_size"] = json!(image.map(|image| image.size));
        report["colored_pixels"] = json!(colored);
        report["elapsed_ms"] = json!(self.started.elapsed().as_millis());
        report["error"] = json!(error);
        if let Some(probe) = &self.latency {
            report["latency"] = probe.report();
        }
        *self.output.lock().unwrap_or_else(|e| e.into_inner()) = Some(report);
        self.done = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn fail(&mut self, ctx: &egui::Context, error: &str) {
        let image = self
            .latency
            .as_ref()
            .and_then(|probe| probe.last_image.clone());
        let pty = self.ready && self.session.exit_state() == Some(ExitState::Exited(0));
        self.finish(ctx, pty, image.as_deref(), Some(error));
    }
}

impl eframe::App for CheckApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        view::color(self.palette.background).to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.done {
            return;
        }
        if apply_pending_scale(ctx, &mut self.pending_scale) {
            return;
        }
        // Observe readbacks before doing more frame work. Each request has a
        // typed owner, and a sample cannot advance until its readback is verified.
        let readbacks: Vec<_> = ctx.input(|input| {
            input
                .events
                .iter()
                .filter_map(|event| {
                    if let egui::Event::Screenshot {
                        user_data, image, ..
                    } = event
                    {
                        Some((user_data.clone(), image.clone(), Instant::now()))
                    } else {
                        None
                    }
                })
                .collect()
        });
        let session_error = self.session.events.try_iter().take(1024).find_map(|event| {
            if let SessionEvent::Error(error) = event {
                Some(error)
            } else {
                None
            }
        });
        if let Some(error) = session_error {
            self.fail(ctx, &format!("Runtime check PTY: {error}"));
            return;
        }
        let text = self
            .session
            .terminal
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .text();
        let text_observed = Instant::now();
        if !self.input_sent && text.contains(INPUT_PROMPT) {
            if let Err(error) = self.session.write(format!("{CHECK_INPUT}\r").into_bytes()) {
                self.fail(ctx, &format!("PTY input: {error:#}"));
                return;
            }
            self.input_sent = true;
        }
        self.ready |= self.input_sent && text.contains(SUCCESS_MARKER);
        let exit = self.session.exit_state();
        let pty = self.ready && exit == Some(ExitState::Exited(0));
        let exit_error = match &exit {
            Some(ExitState::Failed(error)) => Some(format!("PTY probe failed: {error}")),
            Some(ExitState::Exited(code)) if *code != 0 => {
                Some(format!("PTY probe exited with code {code}"))
            }
            _ => None,
        };
        if let Some(error) = exit_error {
            self.fail(ctx, &error);
            return;
        }
        if ctx.input(|input| input.viewport().close_requested()) {
            self.fail(ctx, "Check window was closed");
            return;
        }
        if let Some(probe) = &mut self.latency {
            let mut readback_error = None;
            for (data, image, observed) in &readbacks {
                if let Err(error) = probe.accept_readback(data, image.clone(), *observed) {
                    readback_error = Some(error);
                    break;
                }
            }
            if let Some(error) = readback_error {
                self.fail(ctx, &error);
                return;
            }
            if let Some(error) = probe.timeout(self.started, Instant::now(), self.ready) {
                self.fail(ctx, &error);
                return;
            }
            if probe.completed == probe.total() {
                if pty {
                    let image = probe.last_image.clone();
                    self.finish(ctx, true, image.as_deref(), None);
                    return;
                }
            } else {
                // Only the final request may be outstanding when the fixed
                // probe exits successfully; all earlier exits truncate the run.
                let final_request_sent = probe
                    .pending
                    .as_ref()
                    .is_some_and(|pending| pending.index + 1 == probe.total());
                if pty && !final_request_sent {
                    self.fail(
                        ctx,
                        "Latency probe shell exited before all requests were sent",
                    );
                    return;
                }
                if self.ready && probe.pending.is_none() {
                    let index = probe.completed;
                    let token = probe_token(index);
                    // Include Session's writer queue, but exclude token formatting.
                    let written = Instant::now();
                    if let Err(error) = self.session.write(token) {
                        self.fail(
                            ctx,
                            &format!("Latency sample {} PTY input: {error:#}", index + 1),
                        );
                        return;
                    }
                    probe.pending = Some(PendingSample {
                        index,
                        written,
                        ack_observed: None,
                        readback_requested: false,
                    });
                }
                probe.observe_ack(&text, text_observed);
            }
        }
        let mut render_error = None;
        egui::CentralPanel::default().show(ctx, |ui| {
            render_error = view::show(
                ui,
                &mut self.session,
                &mut self.view,
                view::Options {
                    font_size: 17.0,
                    composition: "",
                    focus: false,
                    theme: &self.palette,
                    search: None,
                },
            )
            .error;
        });
        if let Some(error) = render_error {
            self.fail(ctx, &format!("Runtime check terminal view: {error}"));
            return;
        }
        if let Some(probe) = &mut self.latency {
            // The shared terminal view has now painted the acknowledged marker.
            if let Some(data) = probe.request_readback() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(data));
            }
            if !readbacks.is_empty() {
                ctx.request_repaint();
            }
        } else if let Some((_, image, _)) = readbacks.iter().find(|(data, _, _)| {
            self.requested && screenshot_id(data) == Some(ScreenshotId::NativeCheck)
        }) {
            self.finish(ctx, pty, Some(image), None);
        } else if self.started.elapsed() > STARTUP_TIMEOUT {
            self.fail(ctx, "Timed out waiting for PTY or GPU readback");
        } else if pty && !self.requested {
            self.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                ScreenshotId::NativeCheck,
            )));
        }
        ctx.request_repaint_after(Duration::from_millis(30));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_until(mut predicate: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !predicate() {
            assert!(Instant::now() < deadline, "Runtime probe timed out");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn requested_scale_waits_for_native_monitor_input_and_the_next_pass() {
        for native in [1.0, 1.25, 1.5, 2.0] {
            for requested in [None, Some(1.0), Some(1.25), Some(1.5), Some(2.0)] {
                let ctx = egui::Context::default();
                let mut pending = requested;
                let mut input = egui::RawInput::default();
                input
                    .viewports
                    .get_mut(&egui::ViewportId::ROOT)
                    .unwrap()
                    .native_pixels_per_point = Some(native);
                assert_eq!(ctx.native_pixels_per_point(), None);
                let _ = ctx.run(input.clone(), |ctx| {
                    assert_eq!(apply_pending_scale(ctx, &mut pending), requested.is_some());
                });
                let _ = ctx.run(input, |ctx| {
                    assert!(!apply_pending_scale(ctx, &mut pending));
                    assert_eq!(ctx.native_pixels_per_point(), Some(native));
                    assert!((ctx.pixels_per_point() - requested.unwrap_or(native)).abs() < 0.01);
                });
            }
        }
    }

    #[test]
    fn unicode_probe_requires_exact_native_pty_input() {
        for (input, code) in [
            ("whostty-input-한글-e\u{301}-😀", 0),
            ("whostty-input-한글-é-😀", 9),
            ("incorrect-input", 9),
        ] {
            let profile = probe_profile(0);
            #[cfg(windows)]
            let profile = {
                let mut profile = profile;
                // CI commonly starts consoles with an OEM code page. The probe must
                // select UTF-8 itself rather than inherit this machine's locale.
                let script = profile.args.last_mut().unwrap();
                *script = format!(
                    "[Console]::InputEncoding = [Text.Encoding]::GetEncoding(437); {script}"
                );
                profile
            };
            let session =
                Session::spawn(&profile, Size::new(80, 24), 100, Arc::new(|| {})).unwrap();
            wait_until(|| {
                session
                    .terminal
                    .lock()
                    .unwrap()
                    .text()
                    .contains(INPUT_PROMPT)
            });
            session.write(format!("{input}\r").into_bytes()).unwrap();
            wait_until(|| session.exit_state().is_some());
            assert_eq!(session.exit_state(), Some(ExitState::Exited(code)));
            if code == 0 {
                wait_until(|| {
                    session
                        .terminal
                        .lock()
                        .unwrap()
                        .text()
                        .contains(SUCCESS_MARKER)
                });
            } else {
                assert!(!session
                    .terminal
                    .lock()
                    .unwrap()
                    .text()
                    .contains(SUCCESS_MARKER));
            }
        }
    }

    #[test]
    fn latency_percentiles_use_nearest_rank_and_keep_submillisecond_precision() {
        assert_eq!(latency_summary(&[]), Value::Null);
        let values = [0.75, 0.125, 3.0, 1.25, 0.5];
        assert_eq!(
            latency_summary(&values),
            json!({"min":0.125,"p50":0.75,"p95":3.0,"p99":3.0,"max":3.0})
        );
        let values: Vec<_> = (1..=100).rev().map(f64::from).collect();
        assert_eq!(
            latency_summary(&values),
            json!({"min":1.0,"p50":50.0,"p95":95.0,"p99":99.0,"max":100.0})
        );
        assert_eq!(
            latency_summary(&[0.25]),
            json!({"min":0.25,"p50":0.25,"p95":0.25,"p99":0.25,"max":0.25})
        );
    }

    #[test]
    fn latency_readback_rejects_stale_tags_and_the_previous_marker_color() {
        let written = Instant::now();
        let mut probe = LatencyProbe::new(10);
        probe.completed = WARMUP_SAMPLES + 1;
        probe.pending = Some(PendingSample {
            index: WARMUP_SAMPLES + 1,
            written,
            ack_observed: Some(Duration::from_micros(250)),
            readback_requested: true,
        });
        let image = Arc::new(ColorImage::new(
            [200, 100],
            egui::Color32::from_rgb(210, 40, 210),
        ));
        assert!(!probe
            .accept_readback(
                &egui::UserData::new(ScreenshotId::Latency(WARMUP_SAMPLES)),
                image.clone(),
                written + Duration::from_micros(1000),
            )
            .unwrap());
        assert!(probe.gpu_ms.is_empty());
        let current = egui::UserData::new(ScreenshotId::Latency(WARMUP_SAMPLES + 1));
        let previous = Arc::new(ColorImage::new(
            [200, 100],
            egui::Color32::from_rgb(12, 200, 34),
        ));
        assert!(probe
            .accept_readback(&current, previous, written + Duration::from_micros(1000))
            .is_err());
        assert!(probe.gpu_ms.is_empty());
        assert!(probe
            .accept_readback(&current, image, written + Duration::from_micros(1250))
            .unwrap());
        assert_eq!(probe.ack_ms, [0.25]);
        assert_eq!(probe.gpu_ms, [1.25]);
        assert!(probe.pending.is_none());
    }

    #[test]
    fn latency_warmups_are_verified_but_excluded_from_measurements() {
        let written = Instant::now();
        let mut probe = LatencyProbe::new(10);
        probe.completed = WARMUP_SAMPLES - 1;
        probe.pending = Some(PendingSample {
            index: WARMUP_SAMPLES - 1,
            written,
            ack_observed: Some(Duration::from_millis(1)),
            readback_requested: true,
        });
        let image = Arc::new(ColorImage::new(
            [200, 100],
            egui::Color32::from_rgb(210, 40, 210),
        ));
        assert!(probe
            .accept_readback(
                &egui::UserData::new(ScreenshotId::Latency(WARMUP_SAMPLES - 1)),
                image,
                written + Duration::from_millis(2),
            )
            .unwrap());
        assert_eq!(probe.completed, WARMUP_SAMPLES);
        assert!(probe.ack_ms.is_empty());
        assert!(probe.gpu_ms.is_empty());
        assert_eq!(probe.report()["samples"], 0);
    }

    #[test]
    fn latency_readback_waits_for_the_ack_and_is_requested_once() {
        let written = Instant::now();
        let mut probe = LatencyProbe::new(10);
        probe.pending = Some(PendingSample {
            index: 0,
            written,
            ack_observed: None,
            readback_requested: false,
        });
        assert!(probe.request_readback().is_none());
        probe.observe_ack("whostty-request-0000", written + Duration::from_millis(1));
        assert!(probe.request_readback().is_none());
        probe.observe_ack("whostty-ack-0001-ready", written + Duration::from_millis(2));
        assert!(probe.request_readback().is_none());
        probe.observe_ack("whostty-ack-0000-ready", written + Duration::from_millis(3));
        probe.observe_ack("whostty-ack-0000-ready", written + Duration::from_millis(8));
        let request = probe.request_readback().unwrap();
        assert_eq!(screenshot_id(&request), Some(ScreenshotId::Latency(0)));
        assert!(probe.request_readback().is_none());
        assert_eq!(
            probe.pending.unwrap().ack_observed,
            Some(Duration::from_millis(3))
        );
    }

    #[test]
    fn latency_timeouts_distinguish_startup_ack_readback_and_shell_exit() {
        let started = Instant::now();
        let mut probe = LatencyProbe::new(10);
        assert!(probe
            .timeout(started, started + Duration::from_secs(16), false)
            .unwrap()
            .contains("startup"));
        probe.pending = Some(PendingSample {
            index: 0,
            written: started,
            ack_observed: None,
            readback_requested: false,
        });
        assert!(probe
            .timeout(started, started + Duration::from_secs(6), true)
            .unwrap()
            .contains("acknowledgement"));
        probe.pending.as_mut().unwrap().ack_observed = Some(Duration::from_millis(1));
        assert!(probe
            .timeout(started, started + Duration::from_secs(6), true)
            .unwrap()
            .contains("readback"));
        probe.pending = None;
        probe.finished_at = Some(started);
        assert!(probe
            .timeout(started, started + Duration::from_secs(6), true)
            .unwrap()
            .contains("exit 0"));
        assert!(probe
            .timeout(started, started + Duration::from_secs(601), true)
            .unwrap()
            .contains("total timeout"));
    }

    #[test]
    fn latency_probe_acknowledges_exact_tokens_and_rejects_wrong_tokens() {
        for wrong_token in [false, true] {
            let session =
                Session::spawn(&probe_profile(3), Size::new(80, 24), 100, Arc::new(|| {})).unwrap();
            wait_until(|| {
                session
                    .terminal
                    .lock()
                    .unwrap()
                    .text()
                    .contains(INPUT_PROMPT)
            });
            session
                .write(format!("{CHECK_INPUT}\r").into_bytes())
                .unwrap();
            wait_until(|| {
                session
                    .terminal
                    .lock()
                    .unwrap()
                    .text()
                    .contains(SUCCESS_MARKER)
            });
            if wrong_token {
                // This resembles an acknowledgement but is never a valid request.
                session.write(b"whostty-ack-0000-ready\r".to_vec()).unwrap();
                wait_until(|| session.exit_state().is_some());
                assert_eq!(session.exit_state(), Some(ExitState::Exited(10)));
                continue;
            }
            for index in 0..3 {
                session.write(probe_token(index)).unwrap();
                let marker = probe_ack(index);
                wait_until(|| session.terminal.lock().unwrap().text().contains(&marker));
                if index > 0 {
                    assert!(!session
                        .terminal
                        .lock()
                        .unwrap()
                        .text()
                        .contains(&probe_ack(index - 1)));
                }
            }
            wait_until(|| session.exit_state().is_some());
            assert_eq!(session.exit_state(), Some(ExitState::Exited(0)));
        }
    }
}
