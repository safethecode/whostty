use crate::view::{self, ACCENT, BACKGROUND, FOREGROUND};
use eframe::egui::{self, Color32, FontFamily, FontId, RichText, Vec2};
use std::{path::PathBuf, sync::Arc};
use whostty_core::{
    alacritty_terminal::{
        event::{Event, WindowSize},
        grid::Scroll,
        term::TermMode,
    },
    config::{Profile, Settings},
    input::{self, Key, Modifiers},
    session::{Session, SessionEvent},
    terminal::Size,
};

struct Tab {
    profile: Profile,
    title: String,
    session: Option<Session>,
    error: Option<String>,
    exit: Option<u32>,
    bell: bool,
    wheel: f32,
}
#[derive(Clone, Copy)]
enum Close {
    Tab(usize),
    Window,
}

pub struct Whostty {
    settings: Settings,
    config_path: PathBuf,
    config_error: bool,
    tabs: Vec<Tab>,
    active: usize,
    focus_terminal: bool,
    composition: String,
    show_settings: bool,
    message: Option<String>,
    close: Option<Close>,
    allow_close: bool,
}

impl Whostty {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let config_path = std::env::var_os("WHOSTTY_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(Settings::path);
        let (settings, message, config_error) = match Settings::load(&config_path) {
            Ok(settings) => (settings, None, false),
            Err(error) => (
                Settings::default(),
                Some(format!("Configuration could not be loaded: {error:#}")),
                true,
            ),
        };
        configure_style(&cc.egui_ctx);
        let font_error = configure_fonts(&cc.egui_ctx, settings.font_path.as_deref());
        let mut app = Self {
            settings,
            config_path,
            config_error,
            tabs: Vec::new(),
            active: 0,
            focus_terminal: true,
            composition: String::new(),
            show_settings: false,
            message: message.or(font_error),
            close: None,
            allow_close: false,
        };
        if !app.config_path.exists() {
            if let Err(error) = app.settings.save(&app.config_path) {
                app.message = Some(format!("Create configuration: {error:#}"));
            }
        }
        app.open_profile(app.settings.default_profile, &cc.egui_ctx);
        app
    }

    fn open_profile(&mut self, index: usize, ctx: &egui::Context) {
        if let Some(profile) = self.settings.profiles.get(index).cloned() {
            self.open(profile, ctx);
        }
    }

    fn open(&mut self, profile: Profile, ctx: &egui::Context) {
        let repaint = ctx.clone();
        let result = Session::spawn(
            &profile,
            Size::new(100, 30),
            self.settings.scrollback,
            Arc::new(move || repaint.request_repaint()),
        );
        let (session, error) = match result {
            Ok(session) => (Some(session), None),
            Err(error) => (None, Some(format!("{error:#}"))),
        };
        self.tabs.push(Tab {
            title: profile.name.clone(),
            profile,
            session,
            error,
            exit: None,
            bell: false,
            wheel: 0.0,
        });
        self.active = self.tabs.len() - 1;
        self.focus_terminal = true;
        self.composition.clear();
    }

    fn poll(&mut self, ctx: &egui::Context) {
        for tab in &mut self.tabs {
            let Some(session) = &tab.session else {
                continue;
            };
            for event in session.events.try_iter().take(512) {
                match event {
                    SessionEvent::Exited(code) => tab.exit = Some(code),
                    SessionEvent::Error(error) => tab.error = Some(error),
                    SessionEvent::Terminal(Event::Title(title)) => {
                        tab.title = title
                            .chars()
                            .filter(|c| !c.is_control())
                            .take(200)
                            .collect()
                    }
                    SessionEvent::Terminal(Event::ResetTitle) => {
                        tab.title.clone_from(&tab.profile.name)
                    }
                    SessionEvent::Terminal(Event::Bell) => tab.bell = true,
                    SessionEvent::Terminal(Event::ColorRequest(index, format)) => {
                        let color = {
                            let terminal =
                                session.terminal.lock().unwrap_or_else(|e| e.into_inner());
                            view::rgb(view::palette(index, terminal.term.colors()))
                        };
                        if let Err(error) = session.write(format(color).into_bytes()) {
                            tab.error = Some(format!("{error:#}"));
                        }
                    }
                    SessionEvent::Terminal(Event::TextAreaSizeRequest(format)) => {
                        let size = session.size();
                        let width = ctx.fonts(|f| {
                            f.glyph_width(&FontId::monospace(self.settings.font_size), 'M')
                        });
                        let response = format(WindowSize {
                            num_lines: size.rows as u16,
                            num_cols: size.columns as u16,
                            cell_width: width as u16,
                            cell_height: (self.settings.font_size * 1.4).ceil() as u16,
                        });
                        if let Err(error) = session.write(response.into_bytes()) {
                            tab.error = Some(format!("{error:#}"));
                        }
                    }
                    // OSC52 clipboard access is disabled; clipboard operations are user initiated.
                    _ => {}
                }
            }
        }
    }

    fn send(&mut self, bytes: Vec<u8>) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        if tab.exit.is_some() {
            return;
        }
        if let Some(session) = &tab.session {
            {
                let mut terminal = session.terminal.lock().unwrap_or_else(|e| e.into_inner());
                terminal.term.scroll_display(Scroll::Bottom);
                terminal.term.selection = None;
            }
            if let Err(error) = session.write(bytes) {
                tab.error = Some(format!("{error:#}"));
            }
        }
    }

    fn mode(&self) -> TermMode {
        self.tabs
            .get(self.active)
            .and_then(|tab| tab.session.as_ref())
            .map(|session| {
                *session
                    .terminal
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .term
                    .mode()
            })
            .unwrap_or_default()
    }

    fn copy(&self, ctx: &egui::Context) -> bool {
        let selected = self
            .tabs
            .get(self.active)
            .and_then(|tab| tab.session.as_ref())
            .and_then(|session| {
                session
                    .terminal
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .term
                    .selection_to_string()
            })
            .filter(|text| !text.is_empty());
        if let Some(text) = selected {
            ctx.copy_text(text);
            true
        } else {
            false
        }
    }

    fn close_tab(&mut self, index: usize) {
        self.tabs.remove(index);
        if index < self.active {
            self.active -= 1;
        }
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        self.focus_terminal = true;
        self.composition.clear();
    }

    fn request_close_tab(&mut self, index: usize) {
        if self.tabs[index].session.is_some() && self.tabs[index].exit.is_none() {
            self.close = Some(Close::Tab(index));
        } else {
            self.close_tab(index);
        }
    }

    fn keyboard(&mut self, ctx: &egui::Context, events: &[egui::Event], focused: bool) {
        for event in events {
            if let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            {
                if modifiers.ctrl && modifiers.shift && *key == egui::Key::T {
                    self.open_profile(self.settings.default_profile, ctx);
                    continue;
                }
                if modifiers.ctrl && modifiers.shift && *key == egui::Key::W {
                    if !self.tabs.is_empty() {
                        self.request_close_tab(self.active);
                    }
                    continue;
                }
                if modifiers.ctrl && *key == egui::Key::Tab && !self.tabs.is_empty() {
                    let count = self.tabs.len();
                    self.active =
                        (self.active + if modifiers.shift { count - 1 } else { 1 }) % count;
                    self.focus_terminal = true;
                    self.composition.clear();
                    continue;
                }
                if modifiers.ctrl && *key == egui::Key::Comma {
                    self.show_settings = true;
                    continue;
                }
                if modifiers.ctrl
                    && matches!(
                        key,
                        egui::Key::Plus | egui::Key::Equals | egui::Key::Minus | egui::Key::Num0
                    )
                {
                    self.settings.font_size = match key {
                        egui::Key::Minus => (self.settings.font_size - 1.0).max(8.0),
                        egui::Key::Num0 => 15.0,
                        _ => (self.settings.font_size + 1.0).min(40.0),
                    };
                    continue;
                }
            }
            if !focused || self.close.is_some() || self.show_settings {
                continue;
            }
            match event {
                egui::Event::Text(text) => {
                    let modifiers = ctx.input(|i| i.modifiers);
                    if !modifiers.ctrl || modifiers.alt {
                        let mut bytes = text.as_bytes().to_vec();
                        if modifiers.alt && !modifiers.ctrl {
                            bytes.insert(0, 0x1b);
                        }
                        self.send(bytes);
                    }
                }
                egui::Event::Ime(egui::ImeEvent::Preedit(text)) => {
                    self.composition.clone_from(text)
                }
                egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                    self.composition.clear();
                    self.send(text.as_bytes().to_vec());
                }
                egui::Event::Ime(egui::ImeEvent::Disabled) => self.composition.clear(),
                egui::Event::Paste(text) => self.send(input::encode_paste(
                    text,
                    self.mode().contains(TermMode::BRACKETED_PASTE),
                )),
                egui::Event::Copy => {
                    if !self.copy(ctx) && !ctx.input(|i| i.modifiers.shift) {
                        self.send(vec![3]);
                    }
                }
                egui::Event::Cut => self.send(vec![24]),
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if !self.composition.is_empty() {
                        continue;
                    }
                    if modifiers.ctrl && modifiers.shift && *key == egui::Key::C {
                        self.copy(ctx);
                        continue;
                    }
                    if modifiers.shift && matches!(key, egui::Key::PageUp | egui::Key::PageDown) {
                        if let Some(session) = self
                            .tabs
                            .get(self.active)
                            .and_then(|tab| tab.session.as_ref())
                        {
                            session
                                .terminal
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .term
                                .scroll_display(if *key == egui::Key::PageUp {
                                    Scroll::PageUp
                                } else {
                                    Scroll::PageDown
                                });
                        }
                        continue;
                    }
                    if modifiers.ctrl && modifiers.alt && key.name().len() == 1 {
                        continue;
                    }
                    if let Some(key) = terminal_key(*key) {
                        if let Some(bytes) = input::encode_key(
                            key,
                            Modifiers {
                                ctrl: modifiers.ctrl,
                                alt: modifiers.alt,
                                shift: modifiers.shift,
                            },
                            self.mode().contains(TermMode::APP_CURSOR),
                        ) {
                            self.send(bytes);
                        }
                    }
                }
                egui::Event::WindowFocused(focused)
                    if self.mode().contains(TermMode::FOCUS_IN_OUT) =>
                {
                    self.send(if *focused {
                        b"\x1b[I".to_vec()
                    } else {
                        b"\x1b[O".to_vec()
                    })
                }
                _ => {}
            }
        }
    }

    fn preferences(&mut self, ctx: &egui::Context) {
        let mut open = self.show_settings;
        egui::Window::new("Preferences").open(&mut open).collapsible(false).resizable(false).default_width(420.0).show(ctx, |ui| {
            ui.label("Text");
            ui.add(egui::Slider::new(&mut self.settings.font_size, 8.0..=40.0).text("Font size"));
            ui.add_space(12.0); ui.label("New tabs");
            let selected = self.settings.profiles[self.settings.default_profile].name.clone();
            egui::ComboBox::from_id_salt("default-profile").selected_text(selected).show_ui(ui, |ui| {
                for (index, profile) in self.settings.profiles.iter().enumerate() { ui.selectable_value(&mut self.settings.default_profile, index, &profile.name); }
            });
            ui.add_space(12.0); ui.label("Profiles, font file, and scrollback are configured in:");
            ui.add(egui::Label::new(RichText::new(self.config_path.display().to_string()).small()).wrap());
            ui.horizontal(|ui| {
                if ui.button("Open config").clicked() {
                    #[cfg(windows)]
                    let result = std::process::Command::new("notepad.exe").arg(&self.config_path).spawn();
                    #[cfg(target_os = "macos")]
                    let result = std::process::Command::new("open").arg("-t").arg(&self.config_path).spawn();
                    #[cfg(not(any(windows, target_os = "macos")))]
                    let result = std::process::Command::new("xdg-open").arg(&self.config_path).spawn();
                    if let Err(error) = result { self.message = Some(format!("Open configuration: {error}")); }
                }
                if ui.button("Reload config").clicked() {
                    match Settings::load(&self.config_path) {
                        Ok(settings) => { self.message = configure_fonts(ctx, settings.font_path.as_deref()); self.settings = settings; self.config_error = false; }
                        Err(error) => { self.message = Some(format!("Reload configuration: {error:#}")); self.config_error = true; }
                    }
                }
                if ui.add_enabled(!self.config_error, egui::Button::new("Save preferences")).clicked() {
                    self.message = match self.settings.save(&self.config_path) { Ok(()) => Some("Preferences saved.".into()), Err(error) => Some(format!("Save configuration: {error:#}")) };
                }
            });
            if self.config_error { ui.colored_label(Color32::LIGHT_RED, "Fix and reload the config before saving preferences."); }
            ui.add_space(12.0);
            ui.label(RichText::new("Ctrl+Shift+T  New tab\nCtrl+Tab  Switch tab\nCtrl+Shift+W  Close tab\nCtrl+Shift+C / V  Copy / paste\nCtrl+plus / minus / 0  Text size\nShift+PageUp / PageDown  Scroll history\nShift+drag  Select while a TUI uses the mouse").small());
        });
        if self.show_settings && !open {
            self.focus_terminal = true;
        }
        self.show_settings = open;
    }
}

impl eframe::App for Whostty {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        BACKGROUND.to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_close
            && self
                .tabs
                .iter()
                .any(|tab| tab.session.is_some() && tab.exit.is_none())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close = Some(Close::Window);
        }
        let events = ctx.input(|i| i.events.clone());
        let mut new_profile = None;
        let mut close_tab = None;
        egui::TopBottomPanel::top("tabs")
            .exact_height(44.0)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(25, 30, 36))
                    .inner_margin(egui::Margin::symmetric(10, 7)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("W").color(ACCENT).strong().size(18.0));
                    egui::ScrollArea::horizontal()
                        .max_width((ui.available_width() - 122.0).max(100.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for (index, tab) in self.tabs.iter().enumerate() {
                                    let title: String = tab.title.chars().take(24).collect();
                                    let title = format!(
                                        "{}{}",
                                        if tab.bell && index != self.active {
                                            "• "
                                        } else {
                                            ""
                                        },
                                        title
                                    );
                                    if ui
                                        .selectable_label(index == self.active, title)
                                        .on_hover_text(&tab.title)
                                        .clicked()
                                    {
                                        self.active = index;
                                        self.focus_terminal = true;
                                        self.composition.clear();
                                    }
                                    if ui
                                        .small_button("×")
                                        .on_hover_text("Close tab · Ctrl+Shift+W")
                                        .clicked()
                                    {
                                        close_tab = Some(index);
                                    }
                                    ui.add_space(5.0);
                                }
                            });
                        });
                    if ui
                        .button("+")
                        .on_hover_text("New tab · Ctrl+Shift+T")
                        .clicked()
                    {
                        new_profile = Some(self.settings.default_profile);
                    }
                    ui.menu_button("v", |ui| {
                        for (index, profile) in self.settings.profiles.iter().enumerate() {
                            if ui.button(&profile.name).clicked() {
                                new_profile = Some(index);
                                ui.close_menu();
                            }
                        }
                    });
                    if ui
                        .button("...")
                        .on_hover_text("Preferences · Ctrl+comma")
                        .clicked()
                    {
                        self.show_settings = true;
                    }
                });
            });
        if let Some(index) = new_profile.take() {
            self.open_profile(index, ctx);
        }
        if let Some(index) = close_tab {
            self.request_close_tab(index);
        }
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.bell = false;
        }
        egui::TopBottomPanel::bottom("status")
            .exact_height(26.0)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(25, 30, 36))
                    .inner_margin(egui::Margin::symmetric(12, 4)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some(tab) = self.tabs.get(self.active) {
                        ui.label(RichText::new(&tab.profile.name).small().color(ACCENT));
                        if let Some(session) = &tab.session {
                            let size = session.size();
                            ui.label(
                                RichText::new(format!("{} × {}", size.columns, size.rows))
                                    .small()
                                    .weak(),
                            );
                        }
                        if let Some(code) = tab.exit {
                            ui.label(RichText::new(format!("Exited · {code}")).small());
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{} pt", self.settings.font_size))
                                .small()
                                .weak(),
                        );
                    });
                });
            });
        if let Some(message) = self.message.clone() {
            egui::TopBottomPanel::top("message").show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(message);
                    if ui.small_button("Dismiss").clicked() {
                        self.message = None;
                    }
                });
            });
        }
        let mut focused = false;
        let blocked = self.show_settings || self.close.is_some();
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BACKGROUND).inner_margin(12))
            .show(ctx, |ui| {
                let Some(tab) = self.tabs.get_mut(self.active) else {
                    ui.centered_and_justified(|ui| {
                        if ui.button("Open a terminal · Ctrl+Shift+T").clicked() {
                            new_profile = Some(self.settings.default_profile);
                        }
                    });
                    return;
                };
                if let Some(error) = &tab.error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
                if tab.session.is_none() || tab.exit.is_some() {
                    let text = if tab.session.is_none() {
                        "Retry profile"
                    } else {
                        "Restart session"
                    };
                    if ui.button(text).clicked() {
                        new_profile = Some(usize::MAX);
                    }
                    ui.add_space(8.0);
                }
                if let Some(session) = &mut tab.session {
                    let result = view::show(
                        ui,
                        session,
                        self.settings.font_size,
                        &self.composition,
                        self.focus_terminal && !blocked,
                        &mut tab.wheel,
                    );
                    focused = result.response.has_focus() && !blocked;
                    if let Some(error) = result.error {
                        tab.error = Some(error);
                    }
                    if !blocked && tab.exit.is_none() {
                        for bytes in result.input {
                            if let Err(error) = session.write(bytes) {
                                tab.error = Some(format!("{error:#}"));
                            }
                        }
                    }
                }
            });
        self.focus_terminal = false;
        if let Some(index) = new_profile {
            if index == usize::MAX {
                let profile = self.tabs[self.active].profile.clone();
                let old = self.active;
                self.open(profile, ctx);
                self.close_tab(old);
                self.active = self.tabs.len() - 1;
            } else {
                self.open_profile(index, ctx);
            }
        }
        if !blocked {
            self.keyboard(ctx, &events, focused);
        }
        if self.show_settings {
            self.preferences(ctx);
        }
        if let Some(close) = self.close {
            egui::Modal::new(egui::Id::new("close-session")).show(ctx, |ui| {
                ui.set_width(340.0);
                ui.heading(match close {
                    Close::Window => "Close Whostty?",
                    Close::Tab(_) => "Close this terminal?",
                });
                ui.label("The shell is still running. Closing will end its session.");
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Keep open").clicked() {
                        self.close = None;
                        self.focus_terminal = true;
                    }
                    if ui.button("End session and close").clicked() {
                        match close {
                            Close::Tab(index) => self.close_tab(index),
                            Close::Window => {
                                self.allow_close = true;
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        }
                        self.close = None;
                    }
                });
            });
        }
        if let Some(tab) = self.tabs.get(self.active) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
                "{} — Whostty",
                tab.title
            )));
        }
    }
}

fn terminal_key(key: egui::Key) -> Option<Key> {
    Some(match key {
        egui::Key::Enter => Key::Enter,
        egui::Key::Tab => Key::Tab,
        egui::Key::Backspace => Key::Backspace,
        egui::Key::Escape => Key::Escape,
        egui::Key::ArrowUp => Key::Up,
        egui::Key::ArrowDown => Key::Down,
        egui::Key::ArrowLeft => Key::Left,
        egui::Key::ArrowRight => Key::Right,
        egui::Key::Home => Key::Home,
        egui::Key::End => Key::End,
        egui::Key::Insert => Key::Insert,
        egui::Key::Delete => Key::Delete,
        egui::Key::PageUp => Key::PageUp,
        egui::Key::PageDown => Key::PageDown,
        egui::Key::Space => Key::Character(' '),
        egui::Key::F1 => Key::Function(1),
        egui::Key::F2 => Key::Function(2),
        egui::Key::F3 => Key::Function(3),
        egui::Key::F4 => Key::Function(4),
        egui::Key::F5 => Key::Function(5),
        egui::Key::F6 => Key::Function(6),
        egui::Key::F7 => Key::Function(7),
        egui::Key::F8 => Key::Function(8),
        egui::Key::F9 => Key::Function(9),
        egui::Key::F10 => Key::Function(10),
        egui::Key::F11 => Key::Function(11),
        egui::Key::F12 => Key::Function(12),
        _ => {
            let name = key.name();
            if name.chars().count() == 1 {
                Key::Character(name.chars().next()?)
            } else {
                return None;
            }
        }
    })
}

fn configure_style(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::dark());
    ctx.style_mut(|style| {
        style.visuals.panel_fill = BACKGROUND;
        style.visuals.override_text_color = Some(FOREGROUND);
        style.visuals.selection.bg_fill = Color32::from_rgb(49, 72, 63);
        style.visuals.selection.stroke.color = ACCENT;
        style.spacing.button_padding = Vec2::new(8.0, 4.0);
        style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    });
}

fn configure_fonts(ctx: &egui::Context, custom: Option<&std::path::Path>) -> Option<String> {
    let mut fonts = egui::FontDefinitions::default();
    let mut error = None;
    let mut normal: Vec<PathBuf> = custom.into_iter().map(PathBuf::from).collect();
    #[cfg(windows)]
    {
        let root =
            PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into()))
                .join("Fonts");
        normal.extend([root.join("CascadiaMono.ttf"), root.join("consola.ttf")]);
    }
    #[cfg(target_os = "macos")]
    normal.push(PathBuf::from("/System/Library/Fonts/Menlo.ttc"));
    for path in normal {
        match std::fs::read(&path) {
            Ok(bytes) => {
                if ttf_parser::Face::parse(&bytes, 0).is_err() {
                    error = Some(format!("Unsupported font file: {}", path.display()));
                    continue;
                }
                fonts
                    .font_data
                    .insert("terminal".into(), egui::FontData::from_owned(bytes).into());
                fonts
                    .families
                    .get_mut(&FontFamily::Monospace)
                    .unwrap()
                    .insert(0, "terminal".into());
                break;
            }
            Err(failure) if custom == Some(path.as_path()) => {
                error = Some(format!("Load font {}: {failure}", path.display()))
            }
            Err(_) => {}
        }
    }
    let bold = fonts.families[&FontFamily::Monospace].clone();
    fonts
        .families
        .insert(FontFamily::Name("terminal-bold".into()), bold);
    #[cfg(windows)]
    {
        let root =
            PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into()))
                .join("Fonts");
        if custom.is_none() {
            if let Ok(bytes) = std::fs::read(root.join("consolab.ttf")) {
                fonts.font_data.insert(
                    "terminal-bold-data".into(),
                    egui::FontData::from_owned(bytes).into(),
                );
                fonts
                    .families
                    .get_mut(&FontFamily::Name("terminal-bold".into()))
                    .unwrap()
                    .insert(0, "terminal-bold-data".into());
            }
        }
        if let Ok(bytes) = std::fs::read(root.join("malgun.ttf")) {
            fonts
                .font_data
                .insert("cjk".into(), egui::FontData::from_owned(bytes).into());
            for family in fonts.families.values_mut() {
                family.push("cjk".into());
            }
        }
    }
    ctx.set_fonts(fonts);
    error
}
