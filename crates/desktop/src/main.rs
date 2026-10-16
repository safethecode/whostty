#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
mod view;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Whostty")
            .with_inner_size([1120.0, 720.0])
            .with_min_inner_size([480.0, 300.0])
            .with_icon(icon()),
        ..Default::default()
    };
    eframe::run_native(
        "Whostty",
        options,
        Box::new(|cc| Ok(Box::new(app::Whostty::new(cc)))),
    )
}

fn icon() -> eframe::egui::IconData {
    let mut rgba = vec![0; 32 * 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            let mark = ((6..9).contains(&x) && (9..22).contains(&y))
                || ((23..26).contains(&x) && (9..22).contains(&y))
                || ((10..14).contains(&x) && (19..24).contains(&y))
                || ((18..22).contains(&x) && (19..24).contains(&y))
                || ((14..18).contains(&x) && (14..20).contains(&y));
            let color = if mark {
                [151, 219, 174, 255]
            } else {
                [19, 23, 28, 255]
            };
            rgba[(y * 32 + x) * 4..(y * 32 + x + 1) * 4].copy_from_slice(&color);
        }
    }
    eframe::egui::IconData {
        rgba,
        width: 32,
        height: 32,
    }
}
