use eframe::egui::{
    self,
    accesskit::{self, NodeId, Role, TextPosition, TextSelection},
    Id, Rect, Vec2,
};
use whostty_core::{
    alacritty_terminal::{
        grid::Indexed,
        index::Point,
        selection::SelectionRange,
        term::cell::{Cell, Flags},
    },
    terminal::Size,
};

#[derive(Default)]
struct Row {
    text: String,
    lengths: Vec<u8>,
    columns: Vec<usize>,
    widths: Vec<usize>,
    keep: usize,
}

impl Row {
    fn push(&mut self, text: &str, column: usize, width: usize) {
        let mut length = 0;
        let mut first = true;
        for c in text.chars() {
            if length + c.len_utf8() > u8::MAX as usize {
                self.lengths.push(length as u8);
                self.columns.push(column);
                self.widths.push(if first { width } else { 0 });
                first = false;
                length = 0;
            }
            self.text.push(c);
            length += c.len_utf8();
        }
        if length != 0 {
            self.lengths.push(length as u8);
            self.columns.push(column);
            self.widths.push(if first { width } else { 0 });
        }
    }

    fn trim(&mut self, newline: bool) {
        let count = self.columns.partition_point(|column| *column < self.keep);
        self.text.truncate(
            self.lengths[..count]
                .iter()
                .map(|length| *length as usize)
                .sum(),
        );
        self.lengths.truncate(count);
        self.columns.truncate(count);
        self.widths.truncate(count);
        if newline {
            self.push("\n", self.keep, 0);
        }
    }
}

fn rows(
    cells: &[Indexed<Cell>],
    offset: usize,
    size: Size,
    cursor: Point,
    selection: Option<SelectionRange>,
) -> Vec<Row> {
    let mut rows: Vec<Row> = (0..size.rows).map(|_| Row::default()).collect();
    if offset == 0 && cursor.line.0 >= 0 {
        if let Some(row) = rows.get_mut(cursor.line.0 as usize) {
            row.keep = cursor.column.0;
        }
    }
    if let Some(range) = selection.filter(|range| !range.is_block) {
        for (index, row) in rows.iter_mut().enumerate() {
            let line = index as i32 - offset as i32;
            if (range.start.line.0..=range.end.line.0).contains(&line) {
                let end = if line == range.end.line.0 {
                    (range.end.column.0 + 1).min(size.columns)
                } else {
                    size.columns
                };
                row.keep = row.keep.max(end);
            }
        }
    }
    for cell in cells {
        let row = cell.point.line.0 + offset as i32;
        if row < 0 {
            continue;
        }
        let Some(row) = rows.get_mut(row as usize) else {
            continue;
        };
        if cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        let column = cell.point.column.0;
        let width = 1 + usize::from(cell.flags.contains(Flags::WIDE_CHAR));
        if cell.flags.contains(Flags::HIDDEN) {
            row.push(&" ".repeat(width), column, width);
        } else {
            let mut text = cell.c.to_string();
            if let Some(extra) = cell.zerowidth() {
                text.extend(extra);
            }
            if !text.trim().is_empty() {
                row.keep = row.keep.max(column + width);
            }
            row.push(&text, column, width);
        }
    }
    for (index, row) in rows.iter_mut().enumerate() {
        row.trim(index + 1 < size.rows);
    }
    rows
}

pub fn update(
    ctx: &egui::Context,
    id: Id,
    rect: Rect,
    cell_size: Vec2,
    cells: &[Indexed<Cell>],
    display: Display,
) {
    let enabled = ctx
        .accesskit_node_builder(id, |node| {
            node.set_role(Role::Terminal);
            node.set_label("Terminal");
            node.set_read_only();
            node.set_live(accesskit::Live::Off);
            node.clear_children();
            node.clear_text_selection();
        })
        .is_some();
    if !enabled {
        return;
    }
    let rows = rows(
        cells,
        display.offset,
        display.size,
        display.cursor,
        display.selection,
    );
    let row_id = |index| id.with(("terminal-row", index));
    let position = |point: Point, after: bool| {
        let raw_row = point.line.0 + display.offset as i32;
        let row = raw_row.clamp(0, rows.len() as i32 - 1) as usize;
        let character_index = if raw_row < 0 {
            0
        } else if raw_row >= rows.len() as i32 {
            rows[row].lengths.len()
        } else {
            rows[row].columns.partition_point(|column| {
                if after {
                    *column <= point.column.0
                } else {
                    *column < point.column.0
                }
            })
        };
        TextPosition {
            node: NodeId(row_id(row).value()),
            character_index,
        }
    };
    let selection = if let Some(range) = display.selection.filter(|range| !range.is_block) {
        Some(TextSelection {
            anchor: position(range.start, false),
            focus: position(range.end, true),
        })
    } else if display.offset == 0 {
        let point = position(display.cursor, false);
        Some(TextSelection {
            anchor: point,
            focus: point,
        })
    } else {
        None
    };
    ctx.accesskit_node_builder(id, |node| {
        node.set_value(rows.iter().map(|row| row.text.as_str()).collect::<String>());
        if let Some(selection) = selection {
            node.set_text_selection(selection);
        }
    });
    ctx.with_accessibility_parent(id, || {
        for (index, row) in rows.into_iter().enumerate() {
            ctx.accesskit_node_builder(row_id(index), |node| {
                node.set_role(Role::TextRun);
                node.set_value(row.text);
                node.set_character_lengths(row.lengths);
                node.set_character_positions(
                    row.columns
                        .iter()
                        .map(|column| *column as f32 * cell_size.x)
                        .collect::<Vec<_>>(),
                );
                node.set_character_widths(
                    row.widths
                        .iter()
                        .map(|width| *width as f32 * cell_size.x)
                        .collect::<Vec<_>>(),
                );
                node.set_text_direction(accesskit::TextDirection::LeftToRight);
                let y = rect.top() + index as f32 * cell_size.y;
                node.set_bounds(accesskit::Rect {
                    x0: rect.left() as f64,
                    y0: y as f64,
                    x1: rect.right() as f64,
                    y1: (y + cell_size.y) as f64,
                });
            });
        }
    });
}

pub struct Display {
    pub offset: usize,
    pub size: Size,
    pub cursor: Point,
    pub selection: Option<SelectionRange>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use whostty_core::{
        alacritty_terminal::{
            event::VoidListener,
            grid::Dimensions,
            index::{Column, Line},
        },
        terminal::Terminal,
    };

    #[test]
    fn terminal_tree_exposes_unicode_cell_geometry_and_masks_concealed_output() {
        let mut terminal = Terminal::new(Size::new(20, 3), 100, VoidListener);
        terminal.feed("A한e\u{301}\x1b[8mSECRET\x1b[0m\r\nnext".as_bytes());
        let cells: Vec<_> = terminal
            .term
            .grid()
            .display_iter()
            .map(|cell| Indexed {
                point: cell.point,
                cell: cell.cell.clone(),
            })
            .collect();
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let id = Id::new("terminal-accessibility-test");
        let output = ctx.run(Default::default(), |ctx| {
            update(
                ctx,
                id,
                Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(200.0, 60.0)),
                Vec2::new(10.0, 20.0),
                &cells,
                Display {
                    offset: 0,
                    size: Size::new(20, 3),
                    cursor: Point::new(Line(1), Column(4)),
                    selection: None,
                },
            );
        });
        let tree = output.platform_output.accesskit_update.unwrap();
        let (_, root) = tree
            .nodes
            .iter()
            .find(|(node, _)| *node == NodeId(id.value()))
            .unwrap();
        assert_eq!(root.role(), Role::Terminal);
        assert_eq!(root.value(), Some("A한e\u{301}\nnext\n"));
        assert!(!root.value().unwrap().contains("SECRET"));
        assert_eq!(root.children().len(), 3);
        for child in root.children() {
            let (_, row) = tree.nodes.iter().find(|(node, _)| node == child).unwrap();
            assert_eq!(row.role(), Role::TextRun);
            assert_eq!(
                row.character_lengths()
                    .iter()
                    .map(|length| *length as usize)
                    .sum::<usize>(),
                row.value().unwrap().len()
            );
            assert_eq!(
                row.character_positions().unwrap().len(),
                row.character_lengths().len()
            );
        }
        let first = &tree
            .nodes
            .iter()
            .find(|(node, _)| *node == root.children()[0])
            .unwrap()
            .1;
        assert_eq!(first.character_lengths(), [1, 3, 3, 1]);
        assert_eq!(first.character_widths().unwrap(), [10.0, 20.0, 10.0, 0.0]);
        let caret = root.text_selection().unwrap();
        assert_eq!(caret.anchor, caret.focus);
        assert_eq!(caret.focus.node, root.children()[1]);
        assert_eq!(caret.focus.character_index, 4);
        assert_eq!(terminal.term.columns(), 20);
    }

    #[test]
    fn long_combining_clusters_keep_valid_accesskit_byte_lengths() {
        let mut row = Row::default();
        let text = format!("e{}", "\u{301}".repeat(300));
        row.push(&text, 0, 1);
        assert_eq!(row.text, text);
        let mut offset = 0;
        for length in &row.lengths {
            assert!(*length > 0);
            offset += *length as usize;
            assert!(row.text.is_char_boundary(offset));
        }
        assert_eq!(offset, text.len());
        assert_eq!(row.widths.iter().sum::<usize>(), 1);
    }
}
