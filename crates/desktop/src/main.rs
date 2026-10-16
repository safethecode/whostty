#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
mod icon;
mod view;

fn main() {
    if let Err(error) = run() {
        let message = format!("Whostty could not start.\n\n{error}");
        eprintln!("{message}");
        #[cfg(windows)]
        {
            let text: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
            let title: Vec<u16> = "Whostty".encode_utf16().chain(Some(0)).collect();
            // SAFETY: Both strings are NUL-terminated and live until the synchronous call returns.
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                    std::ptr::null_mut(),
                    text.as_ptr(),
                    title.as_ptr(),
                    0x10,
                );
            }
        }
        std::process::exit(1);
    }
}

fn run() -> eframe::Result {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Whostty")
            .with_inner_size([1120.0, 720.0])
            .with_min_inner_size([480.0, 300.0])
            .with_icon(eframe::egui::IconData {
                rgba: icon::rgba(),
                width: 32,
                height: 32,
            }),
        ..Default::default()
    };
    eframe::run_native(
        "Whostty",
        options,
        Box::new(|cc| Ok(Box::new(app::Whostty::new(cc)))),
    )
}
