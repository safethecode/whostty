use eframe::egui::{self, Color32, FontId, Pos2, Rect, Response, Sense, Stroke, Vec2};
use whostty_core::{
    alacritty_terminal::{
        grid::{Dimensions, Scroll},
        index::{Column, Point, Side},
        selection::{Selection, SelectionType},
        term::{cell::Flags, color::Colors, viewport_to_point, TermMode},
        vte::ansi::{Color, CursorShape, NamedColor, Rgb},
    },
    session::Session,
    terminal::Size,
};

pub const BACKGROUND: Color32 = Color32::from_rgb(19, 23, 28);
pub const FOREGROUND: Color32 = Color32::from_rgb(219, 224, 231);
pub const ACCENT: Color32 = Color32::from_rgb(151, 219, 174);

pub struct View {
    pub response: Response,
    pub input: Vec<Vec<u8>>,
    pub error: Option<String>,
}

pub fn show(
    ui: &mut egui::Ui,
    session: &mut Session,
    font_size: f32,
    composition: &str,
    focus: bool,
    wheel: &mut f32,
) -> View {
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
    let terminal_arc = session.terminal.clone();
    let mut terminal = terminal_arc
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let term = &mut terminal.term;
    let mode = *term.mode();
    let mouse = mode.intersects(TermMode::MOUSE_MODE);
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

    if !mouse || shift {
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
        match event {
            egui::Event::PointerButton {
                pos,
                button,
                pressed,
                modifiers,
            } if mouse && !modifiers.shift && rect.contains(*pos) => {
                let code = match button {
                    egui::PointerButton::Primary => 0,
                    egui::PointerButton::Middle => 1,
                    egui::PointerButton::Secondary => 2,
                    _ => continue,
                };
                let (column, row, _) = at(*pos);
                if let Some(bytes) = mouse_report(code, column, row, *pressed, *modifiers, mode) {
                    input.push(bytes);
                }
            }
            egui::Event::PointerMoved(pos) if mouse && !shift && rect.contains(*pos) => {
                let down = ui.input(|input| input.pointer.primary_down());
                if mode.contains(TermMode::MOUSE_MOTION)
                    || (down && mode.contains(TermMode::MOUSE_DRAG))
                {
                    let (column, row, _) = at(*pos);
                    let code = if down { 32 } else { 35 };
                    if let Some(bytes) =
                        mouse_report(code, column, row, true, ui.input(|i| i.modifiers), mode)
                    {
                        input.push(bytes);
                    }
                }
            }
            egui::Event::MouseWheel {
                unit,
                delta,
                modifiers,
            } if response.hovered() => {
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
                } else if mode.contains(TermMode::ALT_SCREEN) && !modifiers.shift {
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
    painter.rect_filled(rect, 0.0, BACKGROUND);
    let content = term.renderable_content();
    let cursor = content.cursor;
    let offset = content.display_offset;
    let colors = content.colors;
    let selection = content.selection;
    let focused = response.has_focus() && ui.input(|i| i.focused);
    let cells: Vec<_> = content.display_iter.collect();
    // Paint every background first: a wide character's spacer must not cover its glyph.
    for indexed in &cells {
        let row = indexed.point.line.0 + offset as i32;
        if row < 0 || row as usize >= size.rows {
            continue;
        }
        let pos = rect.min + Vec2::new(indexed.point.column.0 as f32 * cell.x, row as f32 * cell.y);
        let mut bg = resolve_color(
            if indexed.flags.contains(Flags::INVERSE) {
                indexed.fg
            } else {
                indexed.bg
            },
            colors,
        );
        if selection.is_some_and(|range| range.contains(indexed.point)) {
            bg = Color32::from_rgb(51, 77, 70);
        }
        if offset == 0
            && cursor.point == indexed.point
            && mode.contains(TermMode::SHOW_CURSOR)
            && focused
            && cursor.shape == CursorShape::Block
        {
            bg = ACCENT;
        }
        if bg != BACKGROUND {
            painter.rect_filled(Rect::from_min_size(pos, cell), 0.0, bg);
        }
    }
    for indexed in cells {
        let row = indexed.point.line.0 + offset as i32;
        if row < 0 || row as usize >= size.rows {
            continue;
        }
        let pos = rect.min + Vec2::new(indexed.point.column.0 as f32 * cell.x, row as f32 * cell.y);
        let cell_rect = Rect::from_min_size(pos, cell);
        let flags = indexed.flags;
        let mut fg = resolve_color(
            if flags.contains(Flags::INVERSE) {
                indexed.bg
            } else {
                indexed.fg
            },
            colors,
        );
        if flags.contains(Flags::DIM) {
            fg = fg.gamma_multiply(0.6);
        }
        let cursor_cell =
            offset == 0 && cursor.point == indexed.point && mode.contains(TermMode::SHOW_CURSOR);
        if cursor_cell && focused && cursor.shape == CursorShape::Block {
            fg = BACKGROUND;
        }
        if !flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER | Flags::HIDDEN)
            && indexed.c != ' '
        {
            let mut text = indexed.c.to_string();
            if let Some(zero_width) = indexed.zerowidth() {
                text.extend(zero_width);
            }
            let family = if flags.contains(Flags::BOLD) {
                egui::FontFamily::Name("terminal-bold".into())
            } else {
                egui::FontFamily::Monospace
            };
            // Share the viewport clip so the GPU can batch glyphs into the same draw call.
            // Each character is positioned on the VT grid rather than using proportional advances.
            painter.text(
                pos,
                egui::Align2::LEFT_TOP,
                text,
                FontId::new(font_size, family),
                fg,
            );
        }
        if flags.intersects(Flags::ALL_UNDERLINES) {
            painter.line_segment(
                [
                    cell_rect.left_bottom() - Vec2::new(0.0, 2.0),
                    cell_rect.right_bottom() - Vec2::new(0.0, 2.0),
                ],
                Stroke::new(1.0, fg),
            );
        }
        if flags.contains(Flags::STRIKEOUT) {
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
        cell,
    );
    if offset == 0 && cursor_row >= 0 && mode.contains(TermMode::SHOW_CURSOR) {
        if (!focused && cursor.shape != CursorShape::Hidden)
            || cursor.shape == CursorShape::HollowBlock
        {
            painter.rect_stroke(
                cursor_rect,
                0.0,
                Stroke::new(1.0, ACCENT),
                egui::StrokeKind::Inside,
            );
        } else if cursor.shape == CursorShape::Beam {
            painter.line_segment(
                [cursor_rect.left_top(), cursor_rect.left_bottom()],
                Stroke::new(2.0, ACCENT),
            );
        } else if cursor.shape == CursorShape::Underline {
            painter.line_segment(
                [cursor_rect.left_bottom(), cursor_rect.right_bottom()],
                Stroke::new(2.0, ACCENT),
            );
        }
    }
    if response.has_focus() {
        ui.ctx()
            .output_mut(|output| output.ime = Some(egui::output::IMEOutput { rect, cursor_rect }));
        if !composition.is_empty() {
            let pos = cursor_rect.min;
            let galley = painter.layout_no_wrap(composition.into(), font, FOREGROUND);
            painter.rect_filled(Rect::from_min_size(pos, galley.size()), 0.0, BACKGROUND);
            painter.galley(pos, galley, FOREGROUND);
        }
    }

    let history = term.grid().history_size();
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
    }
}

pub fn resolve_color(color: Color, overrides: &Colors) -> Color32 {
    match color {
        Color::Spec(rgb) => Color32::from_rgb(rgb.r, rgb.g, rgb.b),
        Color::Named(named) => palette(named as usize, overrides),
        Color::Indexed(index) => palette(index as usize, overrides),
    }
}

pub fn palette(index: usize, overrides: &Colors) -> Color32 {
    if let Some(rgb) = overrides[index] {
        return Color32::from_rgb(rgb.r, rgb.g, rgb.b);
    }
    const ANSI: [[u8; 3]; 16] = [
        [35, 40, 48],
        [232, 122, 130],
        [151, 219, 174],
        [231, 199, 133],
        [133, 178, 231],
        [189, 160, 225],
        [128, 206, 211],
        [219, 224, 231],
        [104, 116, 131],
        [247, 148, 154],
        [176, 234, 192],
        [246, 217, 159],
        [163, 199, 244],
        [209, 184, 241],
        [158, 226, 230],
        [247, 249, 252],
    ];
    match index {
        0..=15 => {
            let [r, g, b] = ANSI[index];
            Color32::from_rgb(r, g, b)
        }
        16..=231 => {
            let value = index - 16;
            let channel = |v| if v == 0 { 0 } else { (55 + v * 40) as u8 };
            Color32::from_rgb(
                channel(value / 36),
                channel((value / 6) % 6),
                channel(value % 6),
            )
        }
        232..=255 => {
            let value = (8 + (index - 232) * 10) as u8;
            Color32::from_gray(value)
        }
        i if i == NamedColor::Background as usize => BACKGROUND,
        i if i == NamedColor::Cursor as usize => ACCENT,
        259..=266 => palette(index - 259, overrides).gamma_multiply(0.6),
        _ => FOREGROUND,
    }
}

pub fn rgb(color: Color32) -> Rgb {
    Rgb {
        r: color.r(),
        g: color.g(),
        b: color.b(),
    }
}

fn mouse_report(
    button: u8,
    column: usize,
    row: usize,
    pressed: bool,
    modifiers: egui::Modifiers,
    mode: TermMode,
) -> Option<Vec<u8>> {
    let modifiers =
        u8::from(modifiers.shift) * 4 + u8::from(modifiers.alt) * 8 + u8::from(modifiers.ctrl) * 16;
    if mode.contains(TermMode::SGR_MOUSE) {
        Some(
            format!(
                "\x1b[<{};{};{}{}",
                button + modifiers,
                column + 1,
                row + 1,
                if pressed { 'M' } else { 'm' }
            )
            .into_bytes(),
        )
    } else if column < 223 && row < 223 {
        Some(vec![
            27,
            b'[',
            b'M',
            32 + if pressed {
                button + modifiers
            } else {
                3 + modifiers
            },
            (column + 33) as u8,
            (row + 33) as u8,
        ])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xterm_palette_and_sgr_mouse_release() {
        let colors = Colors::default();
        assert_eq!(palette(16, &colors), Color32::BLACK);
        assert_eq!(palette(231, &colors), Color32::WHITE);
        assert_eq!(palette(232, &colors), Color32::from_gray(8));
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
}
