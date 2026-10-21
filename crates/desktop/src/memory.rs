//! Opt-in diagnostic accounting; GPU reservations and process private memory
//! are different accounting domains and must not be added together.
use eframe::{egui, wgpu};
use serde_json::{json, Value};
use std::{
    fs::File,
    io::Write,
    time::{Duration, Instant},
};

pub fn report(ctx: &egui::Context, device: Option<&wgpu::Device>) -> Value {
    let textures: Vec<_> = ctx.tex_manager().read().allocated().map(|(_, meta)| {
        json!({"name":meta.name, "size":meta.size, "bytes":meta.size[0] * meta.size[1] * meta.bytes_per_pixel})
    }).collect();
    let fonts = crate::fonts::Typography::get(ctx)
        .shaping
        .as_ref()
        .map(|renderer| {
            renderer
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .memory_report()
        });
    let gpu = device.and_then(wgpu::Device::generate_allocator_report).map(|report| {
        let allocations: Vec<_> = report.allocations.iter().map(|allocation| {
            json!({"name":allocation.name,"bytes":allocation.size})
        }).collect();
        json!({"allocated_bytes":report.total_allocated_bytes,"reserved_bytes":report.total_reserved_bytes,
               "blocks":report.blocks.iter().map(|block| block.size).collect::<Vec<_>>(),"allocations":allocations})
    });
    json!({"textures":textures,"shaping":fonts,"gpu_allocator":gpu,
           "scope":"Logical font/texture/cache sizes and backend GPU allocator report. Excludes egui font-byte clones, driver/system overhead and process heap accounting. GPU reservations are not OS process private bytes."})
}

pub struct Recorder {
    file: File,
    device: Option<wgpu::Device>,
    started: Instant,
    next: Instant,
}

impl Recorder {
    pub fn from_environment(cc: &eframe::CreationContext<'_>) -> Option<Self> {
        let path = std::env::var_os("WHOSTTY_MEMORY_LOG")?;
        // A diagnostic cannot truncate an unrelated pre-existing file.
        let file = match File::options().create_new(true).write(true).open(path) {
            Ok(file) => file,
            Err(error) => {
                eprintln!("Whostty memory diagnostic: {error}");
                return None;
            }
        };
        let now = Instant::now();
        Some(Self {
            file,
            device: cc
                .wgpu_render_state
                .as_ref()
                .map(|state| state.device.clone()),
            started: now,
            next: now,
        })
    }

    pub fn update(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        if now < self.next {
            return;
        }
        self.next = now + Duration::from_secs(1);
        ctx.request_repaint_after(Duration::from_secs(1));
        let mut value = report(ctx, self.device.as_ref());
        value["elapsed_seconds"] = json!(now.duration_since(self.started).as_secs_f64());
        if let Err(error) = writeln!(self.file, "{value}") {
            eprintln!("Whostty memory diagnostic: {error}");
        }
    }
}
