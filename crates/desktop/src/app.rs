use crate::fonts::configure_fonts;
use crate::view::{self, ACCENT, BACKGROUND, FOREGROUND};
use crate::{
    layout::{Axis, Direction, PaneId},
    workspace::{Pane, Tab},
};
use eframe::egui::{self, Color32, FontId, RichText, Vec2};
use std::{path::PathBuf, sync::Arc};
use whostty_core::{
    alacritty_terminal::{
        event::{Event, WindowSize},
        grid::Scroll,
        term::TermMode,
    },
    config::{Profile, Settings},
    input::{self, Key, Modifiers},
    session::{ExitState, Session, SessionEvent},
    terminal::Size,
};

#[derive(Clone, Copy)]
enum Close {
    Tab(usize),
    Pane(usize, PaneId),
    Restart(usize, PaneId),
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
    next_pane: PaneId,
    search_open: bool,
    search_focus: bool,
    reported_focus: Option<PaneId>,
}

impl Whostty {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        launch: crate::launch::Launch,
    ) -> Result<Self, std::io::Error> {
        let config_path = launch
            .config
            .clone()
            .or_else(|| std::env::var_os("WHOSTTY_CONFIG").map(PathBuf::from))
            .unwrap_or_else(Settings::path);
        let (settings, message, config_error) = match Settings::load(&config_path) {
            Ok(settings) => (settings, None, false),
            Err(error) => (
                Settings::default(),
                Some(format!("Configuration could not be loaded: {error:#}")),
                true,
            ),
        };
        let initial = launch
            .initial_profile(&settings)
            .map_err(std::io::Error::other)?;
        configure_style(&cc.egui_ctx);
        let font_error = configure_fonts(&cc.egui_ctx, &settings);
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
            next_pane: 1,
            search_open: false,
            search_focus: false,
            reported_focus: None,
        };
        if !app.config_path.exists() {
            if let Err(error) = app.settings.save(&app.config_path) {
                app.message = Some(format!("Create configuration: {error:#}"));
            }
        }
        app.open(initial, &cc.egui_ctx);
        Ok(app)
    }

    fn open_profile(&mut self, index: usize, ctx: &egui::Context) {
        if let Some(profile) = self.settings.profiles.get(index).cloned() {
            self.open(profile, ctx);
        }
    }

    fn new_tab(&mut self, ctx: &egui::Context) {
        let mut profile = self.settings.profiles[self.settings.default_profile].clone();
        // An explicit profile directory remains authoritative for a new tab.
        if profile.working_directory.is_none() {
            self.inherit_directory(&mut profile);
        }
        self.open(profile, ctx);
    }

    fn inherit_directory(&self, profile: &mut Profile) {
        if !self.settings.inherit_working_directory {
            return;
        }
        let Some(pane) = self.pane() else {
            return;
        };
        // Never translate a WSL/SSH/container directory into another process environment.
        if profile.program != pane.profile.program || profile.args != pane.profile.args {
            return;
        }
        if let Some(path) = pane
            .session
            .as_ref()
            .and_then(Session::working_directory)
            .filter(|path| path.is_dir())
        {
            profile.working_directory = Some(path);
        }
    }

    fn open(&mut self, profile: Profile, ctx: &egui::Context) {
        let pane = self.spawn_pane(profile, ctx);
        self.tabs.push(Tab::new(pane));
        self.active = self.tabs.len() - 1;
        self.focus_terminal = true;
        self.composition.clear();
    }

    fn spawn_pane(&mut self, profile: Profile, ctx: &egui::Context) -> Pane {
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
        let id = self.next_pane;
        self.next_pane += 1;
        Pane {
            id,
            title: profile.name.clone(),
            profile,
            session,
            error,
            exit: None,
            bell: false,
            view: Default::default(),
            search: Default::default(),
        }
    }

    fn pane(&self) -> Option<&Pane> {
        self.tabs.get(self.active).map(Tab::pane)
    }
    fn pane_mut(&mut self) -> Option<&mut Pane> {
        self.tabs.get_mut(self.active).map(Tab::pane_mut)
    }

    fn split(&mut self, axis: Axis, ctx: &egui::Context) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        if tab.panes.len() >= 8 {
            self.message = Some("A tab can contain up to 8 panes.".into());
            return;
        }
        if let Some(session) = &tab.pane().session {
            let size = session.size();
            if (axis == Axis::Columns && size.columns < 48)
                || (axis == Axis::Rows && size.rows < 12)
            {
                self.message = Some("Enlarge this pane before splitting it again.".into());
                return;
            }
        }
        let mut profile = tab.pane().profile.clone();
        self.inherit_directory(&mut profile);
        let pane = self.spawn_pane(profile, ctx);
        self.tabs[self.active].split(pane, axis);
        self.focus_terminal = true;
        self.composition.clear();
    }

    fn focus_neighbor(&mut self, direction: Direction) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            if let Some(id) = tab.layout.neighbor(tab.active, direction) {
                tab.active = id;
                self.focus_terminal = true;
                self.composition.clear();
            }
        }
    }

    fn close_pane(&mut self, tab: usize, pane: PaneId) {
        if self.tabs[tab].panes.len() == 1 {
            self.close_tab(tab);
        } else {
            self.tabs[tab].close_pane(pane);
            self.focus_terminal = true;
            self.composition.clear();
        }
    }

    fn request_close_pane(&mut self, tab: usize, id: PaneId) {
        if self.tabs[tab]
            .panes
            .iter()
            .any(|pane| pane.id == id && pane.running())
        {
            self.close = Some(Close::Pane(tab, id));
        } else {
            self.close_pane(tab, id);
        }
    }

    fn restart_pane(&mut self, ctx: &egui::Context) {
        let Some(old) = self.pane() else { return };
        let id = old.id;
        let profile = old.profile.clone();
        let mut pane = self.spawn_pane(profile, ctx);
        pane.id = id;
        *self.tabs[self.active].pane_mut() = pane;
        self.focus_terminal = true;
        self.composition.clear();
    }

    fn find(&mut self, ctx: &egui::Context, backwards: bool, reset: bool) {
        if let Some(pane) = self.pane_mut() {
            if let Some(session) = &pane.session {
                let repaint = ctx.clone();
                pane.search.start(
                    &session.terminal,
                    backwards,
                    reset,
                    Arc::new(move || repaint.request_repaint()),
                );
            }
        }
    }

    fn close_search(&mut self) {
        self.search_open = false;
        self.focus_terminal = true;
        for pane in self.tabs.iter_mut().flat_map(|tab| &mut tab.panes) {
            pane.search.cancel();
        }
    }

    fn find_bar(&mut self, ctx: &egui::Context) {
        let mut action = None;
        let mut close = false;
        let focus = std::mem::take(&mut self.search_focus);
        egui::TopBottomPanel::top("find-bar")
            .exact_height(38.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let Some(pane) = self.pane_mut() else {
                        close = true;
                        return;
                    };
                    ui.label("Find");
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut pane.search.query)
                            .desired_width((ui.available_width() - 290.0).max(80.0))
                            .hint_text("Search active pane")
                            .char_limit(256),
                    );
                    if focus {
                        edit.request_focus();
                    }
                    let mut changed = edit.changed();
                    changed |= ui
                        .checkbox(&mut pane.search.case_sensitive, "Aa")
                        .on_hover_text("Match case")
                        .changed();
                    changed |= ui.checkbox(&mut pane.search.regex, "Regex").changed();
                    if changed {
                        action = Some((false, true));
                    }
                    if edit.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                        action = Some((ui.input(|i| i.modifiers.shift), false));
                        edit.request_focus();
                    }
                    if ui
                        .button("<")
                        .on_hover_text("Previous · Shift+F3")
                        .clicked()
                    {
                        action = Some((true, false));
                    }
                    if ui.button(">").on_hover_text("Next · F3").clicked() {
                        action = Some((false, false));
                    }
                    if ui
                        .button("x")
                        .on_hover_text("Close search · Escape")
                        .clicked()
                    {
                        close = true;
                    }
                    if let Some(error) = &pane.search.error {
                        ui.colored_label(Color32::LIGHT_RED, error);
                    } else if let Some(session) = &pane.session {
                        let revision = session
                            .terminal
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .revision();
                        let status = if pane.search.query.is_empty() {
                            ""
                        } else if pane.search.busy() {
                            ui.spinner();
                            "Searching…"
                        } else if pane.search.stale(revision) {
                            "Press Enter to search"
                        } else if pane.search.result(revision).is_some() {
                            if pane.search.wrapped {
                                "Wrapped"
                            } else {
                                "Match found"
                            }
                        } else {
                            "No matches"
                        };
                        ui.label(RichText::new(status).small().weak());
                    }
                });
            });
        if let Some((backwards, reset)) = action {
            self.find(ctx, backwards, reset);
        }
        if close {
            self.close_search();
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        for tab in self.tabs.iter_mut().flat_map(|tab| tab.panes.iter_mut()) {
            let Some(session) = &tab.session else {
                continue;
            };
            {
                let mut terminal = session.terminal.lock().unwrap_or_else(|e| e.into_inner());
                let now = std::time::Instant::now();
                terminal.flush_sync(now);
                tab.search.poll(&mut terminal);
                if let Some(deadline) = terminal.sync_deadline() {
                    ctx.request_repaint_after(deadline.saturating_duration_since(now));
                }
            }
            match session.exit_state() {
                Some(ExitState::Exited(code)) => tab.exit = Some(code),
                Some(ExitState::Failed(error)) => tab.error = Some(error),
                None => {}
            }
            let events: Vec<_> = session.events.try_iter().take(512).collect();
            if events.len() == 512 {
                ctx.request_repaint();
            }
            for event in events {
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
                            self.settings
                                .theme
                                .colors()
                                .indexed(index, terminal.term.colors())
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
        let Some(tab) = self.pane_mut() else {
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
        self.pane()
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
            .pane()
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

    fn report_focus(&mut self, focused: bool) {
        let target = self.pane().filter(|_| focused).map(|pane| pane.id);
        if target == self.reported_focus {
            return;
        }
        for (id, gained) in [(self.reported_focus, false), (target, true)] {
            let Some(pane) = self
                .tabs
                .iter_mut()
                .flat_map(|tab| &mut tab.panes)
                .find(|pane| Some(pane.id) == id)
            else {
                continue;
            };
            let Some(session) = pane.session.as_ref().filter(|_| pane.exit.is_none()) else {
                continue;
            };
            let reporting = session
                .terminal
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .term
                .mode()
                .contains(TermMode::FOCUS_IN_OUT);
            if reporting {
                // Protocol messages must not clear the user's selection or scrollback.
                if let Err(error) =
                    session.write(if gained { b"\x1b[I" } else { b"\x1b[O" }.to_vec())
                {
                    pane.error = Some(format!("{error:#}"));
                }
            }
        }
        self.reported_focus = target;
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
        if self.tabs[index].running() {
            self.close = Some(Close::Tab(index));
        } else {
            self.close_tab(index);
        }
    }

    fn keyboard(&mut self, ctx: &egui::Context, events: &[egui::Event], focused: bool) {
        let mut text_modifiers = None;
        for event in events {
            if let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            {
                text_modifiers = Some(*modifiers);
                if modifiers.ctrl && *key == egui::Key::F {
                    self.search_open = true;
                    self.search_focus = true;
                    self.focus_terminal = false;
                    ctx.request_repaint();
                    continue;
                }
                if self.search_open && *key == egui::Key::Escape {
                    self.close_search();
                    continue;
                }
                if self.search_open && *key == egui::Key::F3 {
                    self.find(ctx, modifiers.shift, false);
                    continue;
                }
                if modifiers.ctrl && modifiers.shift && *key == egui::Key::T {
                    self.new_tab(ctx);
                    continue;
                }
                if modifiers.ctrl && modifiers.shift && *key == egui::Key::W {
                    if !self.tabs.is_empty() {
                        self.request_close_pane(self.active, self.tabs[self.active].active);
                    }
                    continue;
                }
                if modifiers.ctrl && modifiers.shift && matches!(key, egui::Key::D | egui::Key::E) {
                    self.split(
                        if *key == egui::Key::D {
                            Axis::Columns
                        } else {
                            Axis::Rows
                        },
                        ctx,
                    );
                    continue;
                }
                if modifiers.ctrl && modifiers.alt {
                    let direction = match key {
                        egui::Key::ArrowLeft => Some(Direction::Left),
                        egui::Key::ArrowRight => Some(Direction::Right),
                        egui::Key::ArrowUp => Some(Direction::Up),
                        egui::Key::ArrowDown => Some(Direction::Down),
                        _ => None,
                    };
                    if let Some(direction) = direction {
                        self.focus_neighbor(direction);
                        continue;
                    }
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
                    // The backend already filters command text. Its committed text event is
                    // authoritative, even when the frame ends with different modifier keys.
                    let mut bytes = text.as_bytes().to_vec();
                    if text_modifiers
                        .take()
                        .is_some_and(|modifiers| modifiers.alt && !modifiers.ctrl)
                    {
                        bytes.insert(0, 0x1b);
                    }
                    self.send(bytes);
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
                        if let Some(session) = self.pane().and_then(|tab| tab.session.as_ref()) {
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
                            self.mode(),
                        ) {
                            self.send(bytes);
                        }
                    }
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
            ui.horizontal(|ui| {
                use whostty_core::theme::{Theme, Preset};
                ui.label("Terminal colors");
                ui.selectable_value(&mut self.settings.theme, Theme::Preset(Preset::Dark), "Dark");
                ui.selectable_value(&mut self.settings.theme, Theme::Preset(Preset::Light), "Light");
                if matches!(self.settings.theme, Theme::Custom(_)) { ui.label("Custom (config)"); }
            });
            ui.add_space(12.0); ui.label("New tabs");
            ui.checkbox(&mut self.settings.inherit_working_directory, "Inherit the active terminal’s directory");
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
                        Ok(settings) => { self.message = configure_fonts(ctx, &settings); self.settings = settings; self.config_error = false; }
                        Err(error) => { self.message = Some(format!("Reload configuration: {error:#}")); self.config_error = true; }
                    }
                }
                if ui.add_enabled(!self.config_error, egui::Button::new("Save preferences")).clicked() {
                    self.message = match self.settings.save(&self.config_path) { Ok(()) => Some("Preferences saved.".into()), Err(error) => Some(format!("Save configuration: {error:#}")) };
                }
            });
            if self.config_error { ui.colored_label(Color32::LIGHT_RED, "Fix and reload the config before saving preferences."); }
            ui.add_space(12.0);
            ui.label(RichText::new("Ctrl+Shift+T  New tab\nCtrl+Tab  Switch tab\nCtrl+Shift+D / E  Split right / down\nCtrl+Alt+arrows  Focus pane\nCtrl+Shift+W  Close pane\nCtrl+F  Search active pane\nF3 / Shift+F3  Next / previous match\nCtrl+Shift+C / V  Copy / paste\nCtrl+plus / minus / 0  Text size\nShift+PageUp / PageDown  Scroll history\nShift+drag  Select while a TUI uses the mouse").small());
        });
        if self.show_settings && !open {
            self.focus_terminal = true;
        }
        self.show_settings = open;
    }
}

impl eframe::App for Whostty {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        view::color(self.settings.theme.colors().background).to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_close
            && self.tabs.iter().any(Tab::running)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close = Some(Close::Window);
        }
        let events = ctx.input(|i| i.events.clone());
        let mut new_profile = None;
        let mut close_tab = None;
        let mut split_axis = None;
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
                                    let title: String = tab.pane().title.chars().take(24).collect();
                                    let title = format!(
                                        "{}{}",
                                        if tab.bell() && index != self.active {
                                            "• "
                                        } else {
                                            ""
                                        },
                                        title
                                    );
                                    if ui
                                        .selectable_label(index == self.active, title)
                                        .on_hover_text(&tab.pane().title)
                                        .clicked()
                                    {
                                        self.active = index;
                                        self.focus_terminal = true;
                                        self.composition.clear();
                                    }
                                    if ui
                                        .small_button("×")
                                        .on_hover_text("Close tab and all its panes")
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
                        new_profile = Some((self.settings.default_profile, true));
                    }
                    ui.menu_button("v", |ui| {
                        for (index, profile) in self.settings.profiles.iter().enumerate() {
                            if ui.button(&profile.name).clicked() {
                                new_profile = Some((index, false));
                                ui.close_menu();
                            }
                        }
                        ui.separator();
                        if ui
                            .add_enabled(
                                !self.tabs.is_empty(),
                                egui::Button::new("Split right  Ctrl+Shift+D"),
                            )
                            .clicked()
                        {
                            split_axis = Some(Axis::Columns);
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                !self.tabs.is_empty(),
                                egui::Button::new("Split down  Ctrl+Shift+E"),
                            )
                            .clicked()
                        {
                            split_axis = Some(Axis::Rows);
                            ui.close_menu();
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
        if let Some((index, inherit)) = new_profile.take() {
            if inherit {
                self.new_tab(ctx);
            } else {
                self.open_profile(index, ctx);
            }
        }
        if let Some(axis) = split_axis {
            self.split(axis, ctx);
        }
        if let Some(index) = close_tab {
            self.request_close_tab(index);
        }
        if let Some(pane) = self.pane_mut() {
            pane.bell = false;
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
                    if let Some(tab) = self.pane() {
                        ui.label(RichText::new(&tab.profile.name).small().color(ACCENT));
                        if let Some(session) = &tab.session {
                            let size = session.size();
                            if let Some(directory) = session.working_directory() {
                                let label = directory
                                    .file_name()
                                    .unwrap_or(directory.as_os_str())
                                    .to_string_lossy();
                                let label: String =
                                    label.chars().filter(|c| !c.is_control()).take(28).collect();
                                ui.label(RichText::new(label).small().weak())
                                    .on_hover_text(directory.display().to_string());
                            }
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
                    if let Some(tab) = self.tabs.get(self.active).filter(|tab| tab.panes.len() > 1)
                    {
                        ui.label(
                            RichText::new(format!("{} panes", tab.panes.len()))
                                .small()
                                .weak(),
                        );
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
        if self.search_open {
            self.find_bar(ctx);
        }
        let blocked = self.show_settings || self.close.is_some();
        let mut close_pane = None;
        let mut restart_pane = false;
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(view::color(self.settings.theme.colors().background))
                    .inner_margin(12),
            )
            .show(ctx, |ui| {
                let Some(tab) = self.tabs.get_mut(self.active) else {
                    ui.centered_and_justified(|ui| {
                        if ui.button("Open a terminal · Ctrl+Shift+T").clicked() {
                            new_profile = Some((self.settings.default_profile, true));
                        }
                    });
                    return;
                };
                let mut rectangles = Vec::new();
                tab.layout.show(
                    ui,
                    ui.available_rect_before_wrap(),
                    egui::Id::new(("layout", self.active)),
                    &mut rectangles,
                );
                let multiple = tab.panes.len() > 1;
                let mut clicked = None;
                let mut focused_id = None;
                for (id, rect) in rectangles {
                    let pane = tab.panes.iter_mut().find(|pane| pane.id == id).unwrap();
                    let active = id == tab.active;
                    ui.scope_builder(egui::UiBuilder::new().max_rect(rect).id_salt(id), |ui| {
                        ui.set_clip_rect(rect);
                        ui.set_min_size(rect.size());
                        if multiple {
                            egui::Frame::new()
                                .fill(Color32::from_rgb(25, 30, 36))
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        if ui
                                            .selectable_label(active, &pane.profile.name)
                                            .on_hover_text(&pane.title)
                                            .clicked()
                                        {
                                            clicked = Some(id);
                                        }
                                        if ui
                                            .small_button("x")
                                            .on_hover_text("Close pane")
                                            .clicked()
                                        {
                                            close_pane = Some(id);
                                        }
                                    });
                                });
                        }
                        if let Some(error) = &pane.error {
                            ui.colored_label(
                                view::color(self.settings.theme.colors().ansi[1]),
                                error,
                            );
                        }
                        if pane.session.is_none() || pane.exit.is_some() || pane.error.is_some() {
                            let label = if pane.session.is_none() {
                                "Retry profile"
                            } else {
                                "Restart session"
                            };
                            if ui.button(label).clicked() {
                                clicked = Some(id);
                                restart_pane = true;
                            }
                        }
                        if let Some(session) = &mut pane.session {
                            let result = view::show(
                                ui,
                                session,
                                &mut pane.view,
                                view::Options {
                                    font_size: self.settings.font_size,
                                    composition: if active { &self.composition } else { "" },
                                    focus: self.focus_terminal && active && !blocked,
                                    theme: &self.settings.theme.colors(),
                                    search: self.search_open.then_some(&pane.search),
                                },
                            );
                            if result.response.clicked()
                                || result.response.drag_started()
                                || result.response.gained_focus()
                            {
                                clicked = Some(id);
                            }
                            if result.response.has_focus() {
                                focused_id = Some(id);
                            }
                            if let Some(error) = result.error {
                                pane.error = Some(error);
                            }
                            if !blocked {
                                if let Some(url) = result.open_url {
                                    ctx.open_url(egui::OpenUrl::new_tab(url));
                                }
                            }
                            if !blocked && pane.exit.is_none() {
                                for bytes in result.input {
                                    if let Err(error) = session.write(bytes) {
                                        pane.error = Some(format!("{error:#}"));
                                    }
                                }
                            }
                        }
                    });
                    if multiple {
                        ui.painter().rect_stroke(
                            rect,
                            0.0,
                            egui::Stroke::new(
                                1.0,
                                if active {
                                    view::color(self.settings.theme.colors().cursor)
                                        .gamma_multiply(0.75)
                                } else {
                                    Color32::TRANSPARENT
                                },
                            ),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
                if let Some(id) = clicked {
                    if tab.active != id {
                        self.composition.clear();
                    }
                    tab.active = id;
                    if focused_id != Some(id) {
                        self.focus_terminal = true;
                        ctx.request_repaint();
                    }
                }
                focused = focused_id == Some(tab.active) && !blocked;
            });
        self.focus_terminal = !focused && self.focus_terminal;
        if let Some(id) = close_pane {
            self.request_close_pane(self.active, id);
        }
        if restart_pane {
            if let Some(pane) = self.pane().filter(|pane| pane.running()) {
                self.close = Some(Close::Restart(self.active, pane.id));
            } else {
                self.restart_pane(ctx);
            }
        }
        if let Some((index, inherit)) = new_profile {
            if inherit {
                self.new_tab(ctx);
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
                    Close::Pane(_, _) => "Close this pane?",
                    Close::Restart(_, _) => "Restart this pane?",
                });
                ui.label(match close {
                    Close::Window => "Closing Whostty will end all running sessions.",
                    Close::Tab(_) => "Closing this tab will end all its running sessions.",
                    Close::Pane(_, _) => "Closing this pane will end its running session.",
                    Close::Restart(_, _) => {
                        "Restarting will end the current session and clear its output."
                    }
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Keep open").clicked() {
                        self.close = None;
                        self.focus_terminal = true;
                    }
                    if ui
                        .button(if matches!(close, Close::Restart(..)) {
                            "Restart session"
                        } else {
                            "End sessions and close"
                        })
                        .clicked()
                    {
                        match close {
                            Close::Tab(index) => self.close_tab(index),
                            Close::Pane(tab, id) => self.close_pane(tab, id),
                            Close::Restart(tab, id) => {
                                self.active = tab;
                                self.tabs[tab].active = id;
                                self.restart_pane(ctx);
                            }
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
                tab.pane().title
            )));
        }
        self.report_focus(
            focused
                && ctx.input(|input| input.focused)
                && !self.show_settings
                && self.close.is_none()
                && !self.search_focus,
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn keyboard_ignores_unfocused_text_and_ime_enter_before_commit() {
        #[cfg(windows)]
        let profile = Profile::new(
            "input",
            "powershell.exe",
            &[
                "-NoLogo",
                "-NoProfile",
                "-Command",
                "$v = [Console]::ReadLine(); [Console]::Write('received-' + $v)",
            ],
        );
        #[cfg(not(windows))]
        let profile = Profile::new(
            "input",
            "/bin/sh",
            &["-c", "read value; printf received-%s \"$value\""],
        );
        let ctx = egui::Context::default();
        let mut app = Whostty {
            settings: Settings {
                profiles: vec![profile],
                ..Settings::default()
            },
            config_path: PathBuf::new(),
            config_error: false,
            tabs: Vec::new(),
            active: 0,
            focus_terminal: true,
            composition: String::new(),
            show_settings: false,
            message: None,
            close: None,
            allow_close: false,
            next_pane: 1,
            search_open: false,
            search_focus: false,
            reported_focus: None,
        };
        app.open_profile(0, &ctx);
        let enter = egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        };
        app.keyboard(&ctx, &[egui::Event::Text("discarded".into())], false);
        app.composition = "preedit".into();
        app.keyboard(&ctx, &[enter.clone()], true);
        ctx.input_mut(|input| input.modifiers.alt = true);
        app.keyboard(
            &ctx,
            &[
                egui::Event::Ime(egui::ImeEvent::Commit("ime".into())),
                egui::Event::Text("-text".into()),
                enter,
            ],
            true,
        );
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let text = app.tabs[0]
                .pane()
                .session
                .as_ref()
                .unwrap()
                .terminal
                .lock()
                .unwrap()
                .text();
            if text.contains("received-ime-text") {
                break;
            }
            assert!(Instant::now() < deadline, "Unexpected shell input: {text}");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(app.composition.is_empty());
    }

    #[test]
    fn tabs_and_splits_inherit_only_a_compatible_local_directory() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().to_owned();
        let target = directory.join("한글 space;%");
        std::fs::create_dir(&target).unwrap();
        #[cfg(windows)]
        let profile = Profile::new(
            "cwd",
            "powershell.exe",
            &[
                "-NoLogo",
                "-NoProfile",
                "-Command",
                "[Console]::WriteLine('cwd=' + (Get-Location).ProviderPath); [Console]::ReadLine()",
            ],
        );
        #[cfg(not(windows))]
        let profile = Profile::new(
            "cwd",
            "/bin/sh",
            &["-c", "printf 'cwd=%s\\n' \"$PWD\"; read value"],
        );
        let ctx = egui::Context::default();
        let mut app = Whostty {
            settings: Settings {
                profiles: vec![profile],
                ..Settings::default()
            },
            config_path: PathBuf::new(),
            config_error: false,
            tabs: Vec::new(),
            active: 0,
            focus_terminal: true,
            composition: String::new(),
            show_settings: false,
            message: None,
            close: None,
            allow_close: false,
            next_pane: 1,
            search_open: false,
            search_focus: false,
            reported_focus: None,
        };
        app.open_profile(0, &ctx);
        let uri = url::Url::from_directory_path(&target).unwrap();
        app.pane()
            .unwrap()
            .session
            .as_ref()
            .unwrap()
            .terminal
            .lock()
            .unwrap()
            .feed(format!("\x1b]7;{uri}\x07").as_bytes());
        app.new_tab(&ctx);
        let inherited = app.pane().unwrap().session.as_ref().unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while !inherited
            .terminal
            .lock()
            .unwrap()
            .text()
            .contains("space;%")
        {
            assert!(
                Instant::now() < deadline,
                "New shell did not start in inherited directory"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(inherited.working_directory(), Some(target.clone()));
        app.split(Axis::Columns, &ctx);
        assert_eq!(
            app.pane()
                .unwrap()
                .session
                .as_ref()
                .unwrap()
                .working_directory(),
            Some(target.clone())
        );
        app.settings.inherit_working_directory = false;
        let mut candidate = app.settings.profiles[0].clone();
        app.inherit_directory(&mut candidate);
        assert!(candidate.working_directory.is_none());
        app.settings.inherit_working_directory = true;
        candidate.args.push("different-environment".into());
        app.inherit_directory(&mut candidate);
        assert!(candidate.working_directory.is_none());
        app.settings.profiles[0].working_directory = Some(directory.clone());
        app.new_tab(&ctx);
        assert_eq!(
            app.pane()
                .unwrap()
                .session
                .as_ref()
                .unwrap()
                .working_directory(),
            Some(directory.clone())
        );
        drop(app);
    }

    #[test]
    fn split_routes_input_to_one_session_and_close_preserves_sibling() {
        #[cfg(windows)]
        let profile = Profile::new(
            "pane",
            "powershell.exe",
            &[
                "-NoLogo",
                "-NoProfile",
                "-Command",
                "$v=[Console]::ReadLine(); [Console]::Write('received-'+$v)",
            ],
        );
        #[cfg(not(windows))]
        let profile = Profile::new(
            "pane",
            "/bin/sh",
            &["-c", "read value; printf received-%s \"$value\""],
        );
        let ctx = egui::Context::default();
        let mut app = Whostty {
            settings: Settings::default(),
            config_path: PathBuf::new(),
            config_error: false,
            tabs: Vec::new(),
            active: 0,
            focus_terminal: true,
            composition: String::new(),
            show_settings: false,
            message: None,
            close: None,
            allow_close: false,
            next_pane: 1,
            search_open: false,
            search_focus: false,
            reported_focus: None,
        };
        app.open(profile, &ctx);
        let first = app.tabs[0].active;
        app.split(Axis::Columns, &ctx);
        let second = app.tabs[0].active;
        assert_ne!(first, second);
        app.send(b"second\r".to_vec());
        app.tabs[0].active = first;
        app.send(b"first\r".to_vec());
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let first_text = app.tabs[0]
                .panes
                .iter()
                .find(|pane| pane.id == first)
                .unwrap()
                .session
                .as_ref()
                .unwrap()
                .terminal
                .lock()
                .unwrap()
                .text();
            let second_text = app.tabs[0]
                .panes
                .iter()
                .find(|pane| pane.id == second)
                .unwrap()
                .session
                .as_ref()
                .unwrap()
                .terminal
                .lock()
                .unwrap()
                .text();
            if first_text.contains("received-first") && second_text.contains("received-second") {
                assert!(!first_text.contains("received-second"));
                assert!(!second_text.contains("received-first"));
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Pane input did not reach the expected shell"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        app.close_pane(0, second);
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(app.tabs[0].layout.ids(), vec![first]);
        assert_eq!(app.tabs[0].pane().id, first);
        assert!(app.tabs[0].pane().session.is_some());
    }
}
