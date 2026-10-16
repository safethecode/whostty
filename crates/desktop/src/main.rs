#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
mod fonts;
mod icon;
mod launch;
mod layout;
mod view;
mod workspace;

fn main() {
    let launch = match launch::parse(std::env::args_os().skip(1)) {
        Ok(launch::Action::Help) => {
            show_message(launch::HELP, false);
            return;
        }
        Ok(launch::Action::Version) => {
            show_message(concat!("Whostty ", env!("CARGO_PKG_VERSION")), false);
            return;
        }
        Ok(launch::Action::Run(launch)) => launch,
        Err(error) => {
            show_message(&error, true);
            std::process::exit(2);
        }
    };
    if let Err(error) = run(launch) {
        let message = format!("Whostty could not start.\n\n{error}");
        show_message(&message, true);
        std::process::exit(1);
    }
}

fn show_message(message: &str, error: bool) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::INVALID_HANDLE_VALUE,
            System::Console::{
                AttachConsole, GetConsoleProcessList, GetStdHandle, ATTACH_PARENT_PROCESS,
                STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
            },
        };
        // Preserve inherited redirection; attaching a console must not steal a pipe.
        let handle = unsafe {
            GetStdHandle(if error {
                STD_ERROR_HANDLE
            } else {
                STD_OUTPUT_HANDLE
            })
        };
        let redirected = !handle.is_null() && handle != INVALID_HANDLE_VALUE;
        let mut process = 0;
        let attached = unsafe { GetConsoleProcessList(&mut process, 1) } != 0;
        // SAFETY: Attaches only to the invoking console, never creates or changes its mode.
        if !redirected && !attached && unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } == 0 {
            let text: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
            let title: Vec<u16> = "Whostty".encode_utf16().chain(Some(0)).collect();
            // SAFETY: Strings are terminated and live through this synchronous call.
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                    std::ptr::null_mut(),
                    text.as_ptr(),
                    title.as_ptr(),
                    if error { 0x10 } else { 0x40 },
                );
            }
            return;
        }
    }
    if error {
        eprintln!("{message}");
    } else {
        println!("{message}");
    }
}

fn run(launch: launch::Launch) -> eframe::Result {
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
        Box::new(move |cc| Ok(Box::new(app::Whostty::new(cc, launch)?))),
    )
}
