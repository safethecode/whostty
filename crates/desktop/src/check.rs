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

pub struct CheckApp {
    session: Session,
    view: view::ViewState,
    palette: Palette,
    output: Output,
    info: Value,
    started: Instant,
    requested: bool,
    done: bool,
}

impl CheckApp {
    pub fn new(cc: &eframe::CreationContext<'_>, output: Output) -> Result<Self, std::io::Error> {
        // This isolated check never loads/saves user configuration or starts a user profile.
        let settings = Settings::default();
        let _ = fonts::configure_fonts(&cc.egui_ctx, &settings);
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        #[cfg(windows)]
        let profile = Profile::new("Runtime check", "powershell.exe", &["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", "[Console]::WriteLine(([string][char]27) + '[38;2;12;200;34mwhostty-native-check-ready' + [char]27 + '[0m'); exit 0"]);
        #[cfg(not(windows))]
        let profile = Profile::new(
            "Runtime check",
            "/bin/sh",
            &[
                "-c",
                "printf '\\033[38;2;12;200;34mwhostty-native-check-ready\\033[0m\\r\\n'",
            ],
        );
        let repaint = cc.egui_ctx.clone();
        let session = Session::spawn(
            &profile,
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
        let error = error.or({
            if !pty {
                Some("PTY probe did not complete successfully")
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
        report["native_renderer"] = json!(rendered);
        report["text_shaping"] = json!(shaped);
        report["glyphs_cached"] = json!(glyphs);
        report["pixels_per_point"] = json!(ctx.pixels_per_point());
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
        let _ = self.session.events.try_iter().take(1024).count();
        let ready = self
            .session
            .terminal
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .text()
            .contains("whostty-native-check-ready");
        let pty = ready && self.session.exit_state() == Some(ExitState::Exited(0));
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
