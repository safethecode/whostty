//! Opt-in, one-frame visual QA of the real application (including its chrome).
use eframe::egui;
use std::{
    io::{BufWriter, Write},
    path::PathBuf,
    time::{Duration, Instant},
};

pub struct Snapshot {
    path: PathBuf,
    started: Instant,
    requested: bool,
}

impl Snapshot {
    pub fn from_environment() -> Option<Self> {
        std::env::var_os("WHOSTTY_UI_SNAPSHOT").map(|path| Self {
            path: path.into(),
            started: Instant::now(),
            requested: false,
        })
    }

    /// Returns true when the explicit snapshot run should exit.
    pub fn update(&mut self, ctx: &egui::Context) -> bool {
        let image = ctx.input(|input| {
            input.events.iter().find_map(|event| {
                if let egui::Event::Screenshot {
                    user_data, image, ..
                } = event
                {
                    if self.requested
                        && user_data
                            .data
                            .as_deref()
                            .and_then(|data| data.downcast_ref::<String>())
                            .is_some_and(|id| id == "whostty-ui-snapshot")
                    {
                        return Some(image.clone());
                    }
                }
                None
            })
        });
        if let Some(image) = image {
            if let Err(error) = self.save(&image) {
                eprintln!("UI snapshot: {error}");
            }
            return true;
        }
        if self.started.elapsed() > Duration::from_secs(15) {
            eprintln!("UI snapshot timed out");
            return true;
        }
        if !self.requested && self.started.elapsed() >= Duration::from_secs(1) {
            self.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                "whostty-ui-snapshot".to_owned(),
            )));
        }
        ctx.request_repaint_after(Duration::from_millis(50));
        false
    }

    fn save(&self, image: &egui::ColorImage) -> std::io::Result<()> {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.path)?;
        let mut file = BufWriter::new(file);
        write!(file, "P6\n{} {}\n255\n", image.size[0], image.size[1])?;
        for pixel in &image.pixels {
            file.write_all(&[pixel.r(), pixel.g(), pixel.b()])?;
        }
        file.flush()
    }
}
