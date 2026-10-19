use crate::{fonts, view};
use eframe::egui::{self, ColorImage};
use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use whostty_core::{
    config::{Profile, Settings},
    session::{ExitState, Session},
    terminal::Size,
    theme::Palette,
};

pub type Output = Arc<Mutex<Option<Value>>>;

const INPUT_PROMPT: &str = "whostty-native-check-input-ready";
const SUCCESS_MARKER: &str = "whostty-native-check-ready";
const CHECK_INPUT: &str = "whostty-input-한글-e\u{301}-😀";

fn probe_profile() -> Profile {
    #[cfg(windows)]
    {
        let script = format!(
            "[Console]::WriteLine('{INPUT_PROMPT}'); \
             $value = [Console]::ReadLine(); \
             if (-not [string]::Equals($value, '{CHECK_INPUT}', [StringComparison]::Ordinal)) {{ exit 9 }}; \
             [Console]::WriteLine(([string][char]27) + '[38;2;12;200;34m{SUCCESS_MARKER}' + [char]27 + '[0m'); exit 0"
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
             printf '\\033[38;2;12;200;34m{SUCCESS_MARKER}\\033[0m\\r\\n'"
        );
        Profile::new("Runtime check", "/bin/sh", &["-c", &script])
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
    requested_scale: Option<f32>,
    pending_scale: Option<f32>,
    requested: bool,
    done: bool,
}

impl CheckApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        output: Output,
        scale: Option<f32>,
    ) -> Result<Self, std::io::Error> {
        // This isolated check never loads/saves user configuration or starts a user profile.
        let settings = Settings::default();
        let _ = fonts::configure_fonts(&cc.egui_ctx, &settings);
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let repaint = cc.egui_ctx.clone();
        let session = Session::spawn(
            &probe_profile(),
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
            requested_scale: scale,
            pending_scale: scale,
            requested: false,
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
        let colored = image.map_or(0, |image| {
            image
                .pixels
                .iter()
                .filter(|pixel| pixel.r() < 80 && pixel.g() > 130 && pixel.b() < 100)
                .count()
        });
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
        *self.output.lock().unwrap_or_else(|e| e.into_inner()) = Some(report);
        self.done = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
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
        let _ = self.session.events.try_iter().take(1024).count();
        let text = self
            .session
            .terminal
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .text();
        if !self.input_sent && text.contains(INPUT_PROMPT) {
            if let Err(error) = self.session.write(format!("{CHECK_INPUT}\r").into_bytes()) {
                self.finish(ctx, false, None, Some(&format!("PTY input: {error:#}")));
                return;
            }
            self.input_sent = true;
        }
        let ready = text.contains(SUCCESS_MARKER);
        let pty = ready && self.session.exit_state() == Some(ExitState::Exited(0));
        if matches!(self.session.exit_state(), Some(ExitState::Failed(_)))
            || matches!(self.session.exit_state(), Some(ExitState::Exited(code)) if code != 0)
        {
            self.finish(ctx, false, None, Some("Unicode PTY probe failed"));
            return;
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            let _ = view::show(
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
            );
        });
        let image = ctx.input(|input| {
            input.events.iter().find_map(|event| {
                if let egui::Event::Screenshot { image, .. } = event {
                    Some(image.clone())
                } else {
                    None
                }
            })
        });
        if let Some(image) = image {
            self.finish(ctx, pty, Some(&image), None);
        } else if ctx.input(|input| input.viewport().close_requested()) {
            self.finish(ctx, pty, None, Some("Check window was closed"));
        } else if self.started.elapsed() > Duration::from_secs(15) {
            self.finish(
                ctx,
                pty,
                None,
                Some("Timed out waiting for PTY or GPU readback"),
            );
        } else if pty && !self.requested {
            self.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
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
            let session =
                Session::spawn(&probe_profile(), Size::new(80, 24), 100, Arc::new(|| {})).unwrap();
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
}
