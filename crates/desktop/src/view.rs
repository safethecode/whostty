use eframe::egui::{self, Color32, FontId, Pos2, Rect, Response, Sense, Stroke, Vec2};
use unicode_normalization::UnicodeNormalization;
use whostty_core::{
    alacritty_terminal::{
        event::EventListener,
        grid::{Dimensions, Indexed, Scroll},
        index::{Column, Point, Side},
        selection::{Selection, SelectionRange, SelectionType},
        term::{
            cell::{Cell, Flags},
            color::Colors,
            viewport_to_point, RenderableCursor, Term, TermMode,
        },
        vte::ansi::{CursorShape, NamedColor, Rgb},
    },
    session::Session,
    terminal::Size,
};

pub const BACKGROUND: Color32 = Color32::from_rgb(19, 23, 28);
pub const FOREGROUND: Color32 = Color32::from_rgb(219, 224, 231);
pub const ACCENT: Color32 = Color32::from_rgb(151, 219, 174);

#[derive(Default)]
pub struct ViewState {
    wheel: f32,
    wheel_context: Option<(TermMode, bool)>,
    cells: Vec<Indexed<Cell>>,
    mouse_buttons: u8,
    link_press: Option<(whostty_core::links::Link, Pos2)>,
}

struct Snapshot {
    cursor: RenderableCursor,
    offset: usize,
    colors: Colors,
    selection: Option<SelectionRange>,
    history: usize,
}

fn capture<T: EventListener>(term: &Term<T>, state: &mut ViewState) -> Snapshot {
    let content = term.renderable_content();
    state.cells.clear();
    state
        .cells
        .extend(content.display_iter.map(|indexed| Indexed {
            point: indexed.point,
            cell: indexed.cell.clone(),
        }));
    Snapshot {
        cursor: content.cursor,
        offset: content.display_offset,
        colors: *content.colors,
        selection: content.selection,
        history: term.grid().history_size(),
    }
}

pub struct View {
    pub response: Response,
    pub input: Vec<Vec<u8>>,
    pub error: Option<String>,
    pub open_url: Option<String>,
}

pub struct Options<'a> {
    pub font_size: f32,
    pub composition: &'a str,
    pub focus: bool,
    pub theme: &'a whostty_core::theme::Palette,
    pub search: Option<&'a whostty_core::search::Search>,
}

pub fn show(
    ui: &mut egui::Ui,
    session: &mut Session,
    state: &mut ViewState,
    options: Options<'_>,
) -> View {
    let Options {
        font_size,
        composition,
        focus,
        theme,
        search,
    } = options;
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let response = response.on_hover_cursor(egui::CursorIcon::Text);
    if focus || response.clicked() || response.drag_started() {
        response.request_focus();
    }
    if response.has_focus() {
        ui.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                response.id,
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            )
        });
    }
    let font = FontId::monospace(font_size);
    let cell = Vec2::new(
        ui.fonts(|fonts| fonts.glyph_width(&font, 'M')),
        (font_size * 1.4).ceil(),
    );
    let size = Size::new(
        (rect.width() / cell.x) as usize,
        (rect.height() / cell.y) as usize,
    );
    let error = session.resize(size).err().map(|error| format!("{error:#}"));
    let mut input = Vec::new();
    let mut open_url = None;
    let terminal_arc = session.terminal.clone();
    let mut terminal = terminal_arc
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let search_match = search.and_then(|search| search.result(terminal.revision()));
    let term = &mut terminal.term;
    let mode = *term.mode();
    let mouse = mode.intersects(TermMode::MOUSE_MODE);
    if !mouse {
        state.mouse_buttons = 0;
    }
    let wheel = &mut state.wheel;
    let events = ui.input(|input| input.events.clone());
    let shift = ui.input(|input| input.modifiers.shift);
    let pointer = ui.input(|input| input.pointer.hover_pos());
    let at = |pos: Pos2| -> (usize, usize, Side) {
        let column = ((pos.x - rect.left()).max(0.0) / cell.x).floor() as usize;
        let row = ((pos.y - rect.top()).max(0.0) / cell.y).floor() as usize;
        let side = if (pos.x - rect.left()).rem_euclid(cell.x) < cell.x / 2.0 {
            Side::Left
        } else {
            Side::Right
        };
        (column.min(size.columns - 1), row.min(size.rows - 1), side)
    };

    let link_at = |pos| {
        let (column, row, _) = at(pos);
        whostty_core::links::at(
            term,
            viewport_to_point(
                term.grid().display_offset(),
                Point::new(row, Column(column)),
            ),
        )
    };
    let hovered_link = pointer
        .filter(|pos| {
            rect.contains(*pos) && ui.input(|input| input.modifiers.ctrl && !input.modifiers.shift)
        })
        .and_then(link_at);
    let link_gesture = state.link_press.is_some() || events.iter().any(|event| {
        matches!(event, egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers } if modifiers.ctrl && !modifiers.shift && rect.contains(*pos) && link_at(*pos).is_some())
    });

    if (!mouse || shift) && !link_gesture {
        if let Some(pos) = response.interact_pointer_pos() {
            let (column, row, side) = at(pos);
            let point = viewport_to_point(
                term.grid().display_offset(),
                Point::new(row, Column(column)),
            );
            if response.triple_clicked() {
                term.selection = Some(Selection::new(SelectionType::Lines, point, side));
            } else if response.double_clicked() {
                term.selection = Some(Selection::new(SelectionType::Semantic, point, side));
            } else if response.drag_started() {
                let origin = ui
                    .input(|input| input.pointer.press_origin())
                    .unwrap_or(pos);
                let (column, row, side) = at(origin);
                let point = viewport_to_point(
                    term.grid().display_offset(),
                    Point::new(row, Column(column)),
                );
                term.selection = Some(Selection::new(SelectionType::Simple, point, side));
            } else if response.clicked() {
                term.selection = Some(Selection::new(SelectionType::Simple, point, side));
            }
            if response.dragged() {
                if let Some(selection) = &mut term.selection {
                    selection.update(point, side);
                }
            }
        }
    }

    for event in &events {
        if let egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        } = event
        {
            if *pressed && modifiers.ctrl && !modifiers.shift && rect.contains(*pos) {
                let (column, row, _) = at(*pos);
                let point = viewport_to_point(
                    term.grid().display_offset(),
                    Point::new(row, Column(column)),
                );
                if let Some(link) = whostty_core::links::at(term, point) {
                    state.link_press = Some((link, *pos));
                    continue;
                }
            } else if !pressed {
                if let Some((link, origin)) = state.link_press.take() {
                    let (column, row, _) = at(*pos);
                    let point = viewport_to_point(
                        term.grid().display_offset(),
                        Point::new(row, Column(column)),
                    );
                    if rect.contains(*pos)
                        && origin.distance(*pos) < 6.0
                        && whostty_core::links::at(term, point)
                            .is_some_and(|target| target.uri == link.uri)
                    {
                        open_url = Some(link.uri);
                    }
                    continue;
                }
            }
        }
        if state.link_press.is_some() && matches!(event, egui::Event::PointerMoved(_)) {
            continue;
        }
        match event {
            egui::Event::PointerButton {
                pos,
                button,
                pressed,
                modifiers,
            } if mouse => {
                let code = match button {
                    egui::PointerButton::Primary => 0,
                    egui::PointerButton::Middle => 1,
                    egui::PointerButton::Secondary => 2,
                    _ => continue,
                };
                let mask = 1 << code;
                if *pressed {
                    if modifiers.shift || !rect.contains(*pos) {
                        continue;
                    }
                    state.mouse_buttons |= mask;
                    response.request_focus();
                } else if state.mouse_buttons & mask != 0 {
                    // Release belongs to the pane that accepted the press, even outside it
                    // or after Shift was pressed during the drag.
                    state.mouse_buttons &= !mask;
                } else {
                    continue;
                }
                let (column, row, _) = at(*pos);
                if let Some(bytes) = mouse_report(code, column, row, *pressed, *modifiers, mode) {
                    input.push(bytes);
                }
            }
            egui::Event::PointerMoved(pos)
                if mouse && !shift && (rect.contains(*pos) || state.mouse_buttons != 0) =>
            {
                let down = state.mouse_buttons != 0;
                if mode.contains(TermMode::MOUSE_MOTION)
                    || (down && mode.contains(TermMode::MOUSE_DRAG))
                {
                    let (column, row, _) = at(*pos);
                    let code = if down {
                        32 + state.mouse_buttons.trailing_zeros() as u8
                    } else {
                        35
                    };
                    if let Some(bytes) =
                        mouse_report(code, column, row, true, ui.input(|i| i.modifiers), mode)
                    {
                        input.push(bytes);
                    }
                }
            }
            egui::Event::WindowFocused(false) => {
                *wheel = 0.0;
                state.wheel_context = None;
                state.link_press = None;
                let (column, row, _) = at(pointer.unwrap_or(rect.min));
                for code in 0..3 {
                    if state.mouse_buttons & (1 << code) != 0 {
                        if let Some(bytes) =
                            mouse_report(code, column, row, false, Default::default(), mode)
                        {
                            input.push(bytes);
                        }
                    }
                }
                state.mouse_buttons = 0;
            }
            egui::Event::MouseWheel {
                unit,
                delta,
                modifiers,
            } if response.hovered() => {
                let wheel_mode = mode
                    & (TermMode::ALT_SCREEN
                        | TermMode::ALTERNATE_SCROLL
                        | TermMode::MOUSE_MODE
                        | TermMode::SGR_MOUSE
                        | TermMode::UTF8_MOUSE);
                let context = (wheel_mode, modifiers.shift);
                if state.wheel_context != Some(context) {
                    *wheel = 0.0;
                    state.wheel_context = Some(context);
                }
                let lines = match unit {
                    egui::MouseWheelUnit::Point => delta.y / cell.y,
                    egui::MouseWheelUnit::Line => delta.y * 3.0,
                    egui::MouseWheelUnit::Page => delta.y * size.rows as f32,
                };
                *wheel += lines;
                let count = wheel.trunc() as i32;
                *wheel -= count as f32;
                if count == 0 {
                    continue;
                }
                if mouse && !modifiers.shift {
                    let (column, row, _) = at(pointer.unwrap_or(rect.min));
                    for _ in 0..count.unsigned_abs().min(100) {
                        if let Some(bytes) = mouse_report(
                            if count > 0 { 64 } else { 65 },
                            column,
                            row,
                            true,
                            *modifiers,
                            mode,
                        ) {
                            input.push(bytes);
                        }
                    }
                } else if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL)
                    && !modifiers.shift
                {
                    let sequence: &[u8] = if count > 0 {
                        if mode.contains(TermMode::APP_CURSOR) {
                            b"\x1bOA"
                        } else {
                            b"\x1b[A"
                        }
                    } else if mode.contains(TermMode::APP_CURSOR) {
                        b"\x1bOB"
                    } else {
                        b"\x1b[B"
                    };
                    input.push(sequence.repeat(count.unsigned_abs().min(100) as usize));
                } else {
                    term.scroll_display(Scroll::Delta(count));
                }
            }
            _ => {}
        }
    }

    let painter = ui.painter_at(rect);
    let Snapshot {
        cursor,
        offset,
        colors,
        selection,
        history,
    } = capture(term, state);
    // Text layout and GPU shape construction must not hold up PTY parsing.
    drop(terminal);
    let background = color(theme.indexed(NamedColor::Background as usize, &colors));
    let foreground = color(theme.indexed(NamedColor::Foreground as usize, &colors));
    let accent = color(theme.indexed(NamedColor::Cursor as usize, &colors));
    painter.rect_filled(rect, 0.0, background);
    crate::accessibility::update(
        ui.ctx(),
        response.id,
        rect,
        cell,
        &state.cells,
        crate::accessibility::Display {
            offset,
            size,
            cursor: cursor.point,
            selection,
        },
    );
    let response = if let Some(link) = &hovered_link {
        response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(format!("{}\nCtrl+click to open", link.uri))
    } else {
        response
    };
    let focused = response.has_focus() && ui.input(|i| i.focused);
    let typography = crate::fonts::Typography::get(ui.ctx());
    let cursor_wide = state
        .cells
        .iter()
        .any(|cell| cell.point == cursor.point && cell.flags.contains(Flags::WIDE_CHAR));
    let cursor_covers = |point: Point| {
        point == cursor.point
            || (cursor_wide
                && point.line == cursor.point.line
                && point.column.0 == cursor.point.column.0 + 1)
    };
    // Paint every background first: a wide character's spacer must not cover its glyph.
    for indexed in &state.cells {
        let row = indexed.point.line.0 + offset as i32;
        if row < 0 || row as usize >= size.rows {
            continue;
        }
        let pos = rect.min + Vec2::new(indexed.point.column.0 as f32 * cell.x, row as f32 * cell.y);
        let mut bg = color(theme.resolve(
            if indexed.flags.contains(Flags::INVERSE) {
                indexed.fg
            } else {
                indexed.bg
            },
            &colors,
        ));
        if search_match.is_some_and(|range| {
            range.contains(&indexed.point)
                || (indexed.flags.contains(Flags::WIDE_CHAR_SPACER)
                    && indexed.point.column.0 > 0
                    && range.contains(&Point::new(
                        indexed.point.line,
                        Column(indexed.point.column.0 - 1),
                    )))
        }) {
            bg = color(theme.search);
        }
        if selection.is_some_and(|range| range.contains(indexed.point)) {
            bg = color(theme.selection);
        }
        if offset == 0
            && cursor_covers(indexed.point)
            && mode.contains(TermMode::SHOW_CURSOR)
            && focused
            && cursor.shape == CursorShape::Block
        {
            bg = accent;
        }
        if bg != background {
            painter.rect_filled(Rect::from_min_size(pos, cell), 0.0, bg);
        }
    }
    for indexed in &state.cells {
        let row = indexed.point.line.0 + offset as i32;
        if row < 0 || row as usize >= size.rows {
            continue;
        }
        let pos = rect.min + Vec2::new(indexed.point.column.0 as f32 * cell.x, row as f32 * cell.y);
        let cell_rect = Rect::from_min_size(pos, cell);
        let flags = indexed.flags;
        let mut fg = color(theme.resolve(
            if flags.contains(Flags::INVERSE) {
                indexed.bg
            } else {
                indexed.fg
            },
            &colors,
        ));
        if flags.contains(Flags::DIM) {
            fg = fg.gamma_multiply(0.6);
        }
        let cursor_cell =
            offset == 0 && cursor_covers(indexed.point) && mode.contains(TermMode::SHOW_CURSOR);
        if cursor_cell && focused && cursor.shape == CursorShape::Block {
            fg = background;
        }
        if !flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER | Flags::HIDDEN)
            && indexed.c != ' '
        {
            let mut text = indexed.c.to_string();
            if let Some(zero_width) = indexed.zerowidth() {
                text.extend(zero_width);
            }
            let text = text.nfc().collect();
            let (format, synthetic_bold) = typography.format(flags, font_size, fg);
            let galley = ui.fonts(|fonts| {
                fonts.layout_job(egui::text::LayoutJob::single_section(text, format))
            });
            // Share the viewport clip so the GPU can batch glyphs into the same draw call.
            // Each character is positioned on the VT grid rather than using proportional advances.
            painter.galley(pos, galley.clone(), fg);
            if synthetic_bold {
                painter.galley(
                    pos + Vec2::new((font_size * 0.025).max(0.25), 0.0),
                    galley,
                    fg,
                );
            }
        }
        if !flags.contains(Flags::HIDDEN) {
            let underline = indexed
                .underline_color()
                .map(|underline| color(theme.resolve(underline, &colors)))
                .unwrap_or(fg);
            paint_underline(&painter, cell_rect, flags, underline);
            if hovered_link
                .as_ref()
                .is_some_and(|link| link.range.contains(&indexed.point))
            {
                painter.line_segment(
                    [
                        cell_rect.left_bottom() - Vec2::new(0.0, 1.0),
                        cell_rect.right_bottom() - Vec2::new(0.0, 1.0),
                    ],
                    Stroke::new(1.0, accent),
                );
            }
        }
        if flags.contains(Flags::STRIKEOUT) && !flags.contains(Flags::HIDDEN) {
            painter.line_segment(
                [cell_rect.left_center(), cell_rect.right_center()],
                Stroke::new(1.0, fg),
            );
        }
    }

    let cursor_row = cursor.point.line.0;
    let cursor_rect = Rect::from_min_size(
        rect.min
            + Vec2::new(
                cursor.point.column.0 as f32 * cell.x,
                cursor_row as f32 * cell.y,
            ),
        Vec2::new(cell.x * if cursor_wide { 2.0 } else { 1.0 }, cell.y),
    );
    if offset == 0 && cursor_row >= 0 && mode.contains(TermMode::SHOW_CURSOR) {
        if (!focused && cursor.shape != CursorShape::Hidden)
            || cursor.shape == CursorShape::HollowBlock
        {
            painter.rect_stroke(
                cursor_rect,
                0.0,
                Stroke::new(1.0, accent),
                egui::StrokeKind::Inside,
            );
        } else if cursor.shape == CursorShape::Beam {
            painter.line_segment(
                [cursor_rect.left_top(), cursor_rect.left_bottom()],
                Stroke::new(2.0, accent),
            );
        } else if cursor.shape == CursorShape::Underline {
            painter.line_segment(
                [cursor_rect.left_bottom(), cursor_rect.right_bottom()],
                Stroke::new(2.0, accent),
            );
        }
    }
    if response.has_focus() {
        ui.ctx()
            .output_mut(|output| output.ime = Some(egui::output::IMEOutput { rect, cursor_rect }));
        if !composition.is_empty() {
            let pos = cursor_rect.min;
            let galley = painter.layout_no_wrap(composition.into(), font, foreground);
            painter.rect_filled(Rect::from_min_size(pos, galley.size()), 0.0, background);
            painter.galley(pos, galley, foreground);
        }
    }

    if offset > 0 && history > 0 {
        let track = Rect::from_min_size(
            rect.right_top() - Vec2::new(4.0, 0.0),
            Vec2::new(3.0, rect.height()),
        );
        let thumb_height =
            (track.height() * size.rows as f32 / (history + size.rows) as f32).max(24.0);
        let progress = 1.0 - offset as f32 / history as f32;
        let thumb = Rect::from_min_size(
            track.min + Vec2::new(0.0, (track.height() - thumb_height) * progress),
            Vec2::new(3.0, thumb_height),
        );
        painter.rect_filled(thumb, 1.5, Color32::from_rgb(78, 91, 104));
    }
    View {
        response,
        input,
        error,
        open_url,
    }
}

fn paint_underline(painter: &egui::Painter, rect: Rect, flags: Flags, color: Color32) {
    if !flags.intersects(Flags::ALL_UNDERLINES) {
        return;
    }
    let y = rect.bottom() - 2.0;
    let stroke = Stroke::new(1.0, color);
    if flags.contains(Flags::UNDERCURL) {
        // Use absolute x phase so adjacent cells form a continuous wave.
        let points = (0..=rect.width().ceil() as usize)
            .map(|step| {
                let x = (rect.left() + step as f32).min(rect.right());
                Pos2::new(x, y - 1.0 + (x * std::f32::consts::TAU / 6.0).sin())
            })
            .collect();
        painter.add(egui::Shape::line(points, stroke));
    } else if flags.intersects(Flags::DOTTED_UNDERLINE | Flags::DASHED_UNDERLINE) {
        let (period, length) = if flags.contains(Flags::DOTTED_UNDERLINE) {
            (3.0, 1.0)
        } else {
            (7.0, 4.0)
        };
        let mut x = (rect.left() / period).floor() * period;
        while x < rect.right() {
            let start = x.max(rect.left());
            let end = (x + length).min(rect.right());
            if end > start {
                painter.line_segment([Pos2::new(start, y), Pos2::new(end, y)], stroke);
            }
            x += period;
        }
    } else {
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            stroke,
        );
        if flags.contains(Flags::DOUBLE_UNDERLINE) {
            painter.line_segment(
                [
                    Pos2::new(rect.left(), y - 3.0),
                    Pos2::new(rect.right(), y - 3.0),
                ],
                stroke,
            );
        }
    }
}

pub fn color(rgb: impl Into<Rgb>) -> Color32 {
    let rgb = rgb.into();
    Color32::from_rgb(rgb.r, rgb.g, rgb.b)
}

fn mouse_report(
    button: u8,
    column: usize,
    row: usize,
    pressed: bool,
    modifiers: egui::Modifiers,
    mode: TermMode,
) -> Option<Vec<u8>> {
    whostty_core::mouse::encode_mouse(
        button,
        column,
        row,
        pressed,
        whostty_core::input::Modifiers {
            ctrl: modifiers.ctrl,
            alt: modifiers.alt,
            shift: modifiers.shift,
        },
        mode,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use whostty_core::alacritty_terminal::vte::ansi::Color;

    #[test]
    fn xterm_palette_and_sgr_mouse_release() {
        let colors = Colors::default();
        let theme = whostty_core::theme::Palette::default();
        assert_eq!(color(theme.indexed(16, &colors)), Color32::BLACK);
        assert_eq!(color(theme.indexed(231, &colors)), Color32::WHITE);
        assert_eq!(color(theme.indexed(232, &colors)), Color32::from_gray(8));
        assert_eq!(
            mouse_report(
                0,
                4,
                8,
                false,
                egui::Modifiers::default(),
                TermMode::SGR_MOUSE
            )
            .unwrap(),
            b"\x1b[<0;5;9m"
        );
    }

    #[test]
    fn sgr_underlines_have_distinct_geometry_and_keep_their_color() {
        use whostty_core::{alacritty_terminal::event::VoidListener, terminal::Terminal};
        let ctx = egui::Context::default();
        for (sgr, expected) in [("4", 1), ("4:2", 2), ("4:3", 1), ("4:4", 30), ("4:5", 13)] {
            let mut terminal = Terminal::new(Size::new(40, 3), 10, VoidListener);
            terminal.feed(format!("\x1b[{sgr};58:2::255:0:0mx").as_bytes());
            let cell =
                &terminal.term.grid()[whostty_core::alacritty_terminal::index::Line(0)][Column(0)];
            assert_eq!(
                cell.underline_color(),
                Some(Color::Spec(Rgb { r: 255, g: 0, b: 0 }))
            );
            let output = ctx.run(Default::default(), |ctx| {
                let painter = ctx.layer_painter(egui::LayerId::background());
                paint_underline(
                    &painter,
                    Rect::from_min_size(Pos2::ZERO, Vec2::new(90.0, 24.0)),
                    cell.flags,
                    Color32::RED,
                );
            });
            assert_eq!(
                output.shapes.len(),
                expected,
                "Wrong geometry for SGR {sgr}"
            );
            if sgr == "4:3" {
                assert!(matches!(output.shapes[0].shape, egui::Shape::Path(_)));
            }
        }
    }

    #[test]
    fn renderer_snapshot_is_owned_and_reuses_its_cell_buffer() {
        use whostty_core::{
            alacritty_terminal::event::VoidListener,
            terminal::{Size, Terminal},
        };
        let mut terminal = Terminal::new(Size::new(80, 24), 100, VoidListener);
        terminal.feed(b"\x1b[31moriginal");
        let mut state = ViewState::default();
        let snapshot = capture(&terminal.term, &mut state);
        let capacity = state.cells.capacity();
        terminal.feed(b"\r\x1b[32mchanged");
        assert_eq!(state.cells[0].c, 'o');
        assert_eq!(state.cells[0].fg, Color::Named(NamedColor::Red));
        assert_eq!(snapshot.offset, 0);
        capture(&terminal.term, &mut state);
        assert_eq!(state.cells[0].c, 'c');
        assert_eq!(state.cells.capacity(), capacity);
    }

    #[test]
    fn tui_drag_captures_secondary_button_and_release_outside_the_pane() {
        use std::{
            sync::Arc,
            time::{Duration, Instant},
        };
        use whostty_core::config::Profile;
        #[cfg(windows)]
        let profile = Profile::new(
            "mouse",
            "powershell.exe",
            &[
                "-NoLogo",
                "-NoProfile",
                "-Command",
                "[Console]::Write('mouse-ready'); Start-Sleep 60",
            ],
        );
        #[cfg(not(windows))]
        let profile = Profile::new("mouse", "/bin/sh", &["-c", "printf mouse-ready; sleep 60"]);
        let mut session =
            Session::spawn(&profile, Size::new(40, 10), 100, Arc::new(|| {})).unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while !session
            .terminal
            .lock()
            .unwrap()
            .text()
            .contains("mouse-ready")
        {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        session
            .terminal
            .lock()
            .unwrap()
            .feed(b"\x1b[?1002h\x1b[?1006h");
        let terminal = session.terminal.clone();
        let ctx = egui::Context::default();
        let mut state = ViewState::default();
        let mut opened = Vec::new();
        let mut frame = |events| {
            let mut reports = Vec::new();
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 400.0))),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.scope_builder(
                            egui::UiBuilder::new()
                                .max_rect(Rect::from_min_size(Pos2::ZERO, Vec2::new(250.0, 180.0))),
                            |ui| {
                                let view = show(
                                    ui,
                                    &mut session,
                                    &mut state,
                                    Options {
                                        font_size: 15.0,
                                        composition: "",
                                        focus: false,
                                        theme: &whostty_core::theme::Palette::default(),
                                        search: None,
                                    },
                                );
                                reports.extend(view.input);
                                opened.extend(view.open_url);
                            },
                        );
                    });
                },
            );
            reports
        };
        frame(vec![]);
        let press = egui::Event::PointerButton {
            pos: Pos2::new(40.0, 40.0),
            button: egui::PointerButton::Secondary,
            pressed: true,
            modifiers: Default::default(),
        };
        assert!(frame(vec![press])
            .iter()
            .any(|bytes| bytes.starts_with(b"\x1b[<2;")));
        assert!(
            frame(vec![egui::Event::PointerMoved(Pos2::new(400.0, 40.0))])
                .iter()
                .any(|bytes| bytes.starts_with(b"\x1b[<34;"))
        );
        let release = egui::Event::PointerButton {
            pos: Pos2::new(400.0, 40.0),
            button: egui::PointerButton::Secondary,
            pressed: false,
            modifiers: egui::Modifiers::SHIFT,
        };
        assert!(frame(vec![release])
            .iter()
            .any(|bytes| bytes.starts_with(b"\x1b[<6;") && bytes.ends_with(b"m")));
        assert!(frame(vec![egui::Event::PointerMoved(Pos2::new(420.0, 40.0))]).is_empty());
        terminal
            .lock()
            .unwrap()
            .feed(b"\x1b[2;1H\x1b]8;;https://example.com\x1b\\linked label\x1b]8;;\x1b\\");
        for pressed in [true, false] {
            assert!(
                frame(vec![egui::Event::PointerButton {
                    pos: Pos2::new(40.0, 40.0),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::CTRL
                }])
                .is_empty(),
                "Link gesture leaked to the TUI"
            );
        }
        let scroll = |delta, modifiers, unit| {
            vec![
                egui::Event::PointerMoved(Pos2::new(40.0, 40.0)),
                egui::Event::MouseWheel {
                    unit,
                    delta: Vec2::new(0.0, delta),
                    modifiers,
                },
            ]
        };
        let line = egui::MouseWheelUnit::Line;
        let point = egui::MouseWheelUnit::Point;
        let reports = frame(scroll(1.0, egui::Modifiers::NONE, line));
        assert_eq!(reports.len(), 3);
        assert!(reports
            .iter()
            .all(|bytes| bytes.starts_with(b"\x1b[<64;") && bytes.ends_with(b"M")));
        terminal
            .lock()
            .unwrap()
            .feed(b"\x1b[?1002l\x1b[?1049h\x1b[?1007l");
        assert!(
            frame(scroll(1.0, egui::Modifiers::NONE, line)).is_empty(),
            "Disabled alternate scrolling must not send arrow keys"
        );
        terminal.lock().unwrap().feed(b"\x1b[?1007h");
        assert_eq!(
            frame(scroll(1.0, egui::Modifiers::NONE, line)),
            [b"\x1b[A".repeat(3)]
        );
        terminal.lock().unwrap().feed(b"\x1b[?1h");
        assert_eq!(
            frame(scroll(-1.0, egui::Modifiers::NONE, line)),
            [b"\x1bOB".repeat(3)]
        );
        assert!(frame(scroll(1.0, egui::Modifiers::SHIFT, line)).is_empty());
        assert!(frame(scroll(12.6, egui::Modifiers::NONE, point)).is_empty());
        // A fractional wheel delta from arrow mode must not become a TUI mouse event.
        terminal.lock().unwrap().feed(b"\x1b[?1002h");
        assert!(frame(scroll(12.6, egui::Modifiers::NONE, point)).is_empty());
        let reports = frame(scroll(12.6, egui::Modifiers::NONE, point));
        assert_eq!(reports.len(), 1);
        assert!(reports[0].starts_with(b"\x1b[<64;") && reports[0].ends_with(b"M"));
        assert_eq!(opened, ["https://example.com/"]);
    }
}
