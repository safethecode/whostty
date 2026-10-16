use eframe::egui::{self, Id, Pos2, Rect, Sense, Stroke};

pub type PaneId = u64;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Axis {
    Columns,
    Rows,
}

#[derive(Clone, Copy)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Debug)]
pub enum Layout {
    Pane(PaneId),
    Split {
        axis: Axis,
        ratio: f32,
        first: Box<Layout>,
        second: Box<Layout>,
    },
}

impl Layout {
    pub fn split(&mut self, target: PaneId, new: PaneId, axis: Axis) -> bool {
        match self {
            Self::Pane(id) if *id == target => {
                *self = Self::Split {
                    axis,
                    ratio: 0.5,
                    first: Box::new(Self::Pane(target)),
                    second: Box::new(Self::Pane(new)),
                };
                true
            }
            Self::Split { first, second, .. } => {
                first.split(target, new, axis) || second.split(target, new, axis)
            }
            _ => false,
        }
    }

    pub fn remove(&mut self, target: PaneId) -> bool {
        if let Self::Split { first, second, .. } = self {
            if matches!(**first, Self::Pane(id) if id == target) {
                *self = (**second).clone();
                return true;
            }
            if matches!(**second, Self::Pane(id) if id == target) {
                *self = (**first).clone();
                return true;
            }
            first.remove(target) || second.remove(target)
        } else {
            false
        }
    }

    pub fn ids(&self) -> Vec<PaneId> {
        match self {
            Self::Pane(id) => vec![*id],
            Self::Split { first, second, .. } => {
                let mut ids = first.ids();
                ids.extend(second.ids());
                ids
            }
        }
    }

    pub fn neighbor(&self, current: PaneId, direction: Direction) -> Option<PaneId> {
        let mut panes = Vec::new();
        self.rects(
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1000.0, 1000.0)),
            0.0,
            &mut panes,
        );
        let center = panes.iter().find(|(id, _)| *id == current)?.1.center();
        panes
            .into_iter()
            .filter(|(id, _)| *id != current)
            .filter_map(|(id, rect)| {
                let delta = rect.center() - center;
                let (along, across) = match direction {
                    Direction::Left => (-delta.x, delta.y.abs()),
                    Direction::Right => (delta.x, delta.y.abs()),
                    Direction::Up => (-delta.y, delta.x.abs()),
                    Direction::Down => (delta.y, delta.x.abs()),
                };
                (along > 0.1).then_some((id, along + across * 2.0))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    pub fn rects(&self, rect: Rect, gap: f32, output: &mut Vec<(PaneId, Rect)>) {
        match self {
            Self::Pane(id) => output.push((*id, rect)),
            Self::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let (left, _, right) = partition(rect, *axis, *ratio, gap);
                first.rects(left, gap, output);
                second.rects(right, gap, output);
            }
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        id: Id,
        output: &mut Vec<(PaneId, Rect)>,
    ) {
        match self {
            Self::Pane(pane) => output.push((*pane, rect)),
            Self::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let (_, handle, _) = partition(rect, *axis, *ratio, 6.0);
                let cursor = if *axis == Axis::Columns {
                    egui::CursorIcon::ResizeHorizontal
                } else {
                    egui::CursorIcon::ResizeVertical
                };
                let response = ui
                    .interact(handle, id, Sense::click_and_drag())
                    .on_hover_cursor(cursor);
                if response.double_clicked() {
                    *ratio = 0.5;
                }
                if response.dragged() {
                    if let Some(pointer) = response.interact_pointer_pos() {
                        let (position, length, minimum) = if *axis == Axis::Columns {
                            (pointer.x - rect.left(), rect.width(), 120.0)
                        } else {
                            (pointer.y - rect.top(), rect.height(), 90.0)
                        };
                        let limit = (minimum / length.max(1.0)).min(0.45);
                        *ratio = (position / length.max(1.0)).clamp(limit, 1.0 - limit);
                    }
                }
                let (a, handle, b) = partition(rect, *axis, *ratio, 6.0);
                let color = if response.hovered() || response.dragged() {
                    super::view::ACCENT
                } else {
                    egui::Color32::from_rgb(49, 58, 68)
                };
                let points = if *axis == Axis::Columns {
                    [handle.center_top(), handle.center_bottom()]
                } else {
                    [handle.left_center(), handle.right_center()]
                };
                ui.painter().line_segment(points, Stroke::new(1.0, color));
                first.show(ui, a, id.with("first"), output);
                second.show(ui, b, id.with("second"), output);
            }
        }
    }
}

fn partition(rect: Rect, axis: Axis, ratio: f32, gap: f32) -> (Rect, Rect, Rect) {
    let span = if axis == Axis::Columns {
        rect.width()
    } else {
        rect.height()
    };
    let gap = gap.min(span.max(0.0));
    let before = (span - gap).max(0.0) * ratio.clamp(0.05, 0.95);
    if axis == Axis::Columns {
        let cut = rect.left() + before;
        (
            Rect::from_min_max(rect.min, Pos2::new(cut, rect.bottom())),
            Rect::from_min_max(
                Pos2::new(cut, rect.top()),
                Pos2::new(cut + gap, rect.bottom()),
            ),
            Rect::from_min_max(Pos2::new(cut + gap, rect.top()), rect.max),
        )
    } else {
        let cut = rect.top() + before;
        (
            Rect::from_min_max(rect.min, Pos2::new(rect.right(), cut)),
            Rect::from_min_max(
                Pos2::new(rect.left(), cut),
                Pos2::new(rect.right(), cut + gap),
            ),
            Rect::from_min_max(Pos2::new(rect.left(), cut + gap), rect.max),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_close_preserves_siblings_and_spatial_focus() {
        let mut layout = Layout::Pane(1);
        assert!(layout.split(1, 2, Axis::Columns));
        assert!(layout.split(2, 3, Axis::Rows));
        assert_eq!(layout.neighbor(3, Direction::Up), Some(2));
        assert_eq!(layout.neighbor(1, Direction::Right), Some(2));
        assert!(layout.remove(2));
        assert_eq!(layout.ids(), vec![1, 3]);
        assert_eq!(layout.neighbor(1, Direction::Right), Some(3));
        assert!(!layout.remove(42));
        assert!(layout.remove(1));
        assert_eq!(layout.ids(), vec![3]);
        assert!(!layout.remove(3));
    }

    #[test]
    fn split_geometry_is_bounded_and_does_not_overlap() {
        let mut layout = Layout::Pane(1);
        layout.split(1, 2, Axis::Columns);
        layout.split(1, 3, Axis::Rows);
        for size in [egui::vec2(800.0, 600.0), egui::vec2(10.0, 8.0)] {
            let bounds = Rect::from_min_size(Pos2::ZERO, size);
            let mut panes = Vec::new();
            layout.rects(bounds, 6.0, &mut panes);
            for (_, rect) in &panes {
                assert!(bounds.contains_rect(*rect));
                assert!(rect.width() >= 0.0 && rect.height() >= 0.0);
            }
            for (i, (_, a)) in panes.iter().enumerate() {
                for (_, b) in &panes[i + 1..] {
                    assert!(!a.intersects(*b));
                }
            }
        }
    }

    #[test]
    fn pointer_drag_updates_the_split_ratio() {
        let ctx = egui::Context::default();
        let mut layout = Layout::Pane(1);
        layout.split(1, 2, Axis::Columns);
        let mut frame = |events: Vec<egui::Event>| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::new())
                        .show(ctx, |ui| {
                            let mut panes = Vec::new();
                            layout.show(
                                ui,
                                ui.available_rect_before_wrap(),
                                Id::new("test-split"),
                                &mut panes,
                            );
                            for (id, rect) in panes {
                                ui.scope_builder(
                                    egui::UiBuilder::new().max_rect(rect).id_salt(id),
                                    |ui| {
                                        ui.set_clip_rect(rect);
                                        ui.set_min_size(rect.size());
                                        ui.horizontal(|ui| {
                                            ui.selectable_label(false, "Shell").clicked();
                                            ui.small_button("x").clicked();
                                        });
                                        ui.allocate_exact_size(
                                            ui.available_size(),
                                            Sense::click_and_drag(),
                                        );
                                    },
                                );
                            }
                        });
                },
            );
        };
        frame(vec![]);
        frame(vec![
            egui::Event::PointerMoved(Pos2::new(400.0, 300.0)),
            egui::Event::PointerButton {
                pos: Pos2::new(400.0, 300.0),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ]);
        frame(vec![egui::Event::PointerMoved(Pos2::new(560.0, 300.0))]);
        frame(vec![egui::Event::PointerButton {
            pos: Pos2::new(560.0, 300.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]);
        let Layout::Split { ratio, .. } = layout else {
            panic!("Expected split")
        };
        assert!(ratio > 0.65, "Drag did not update ratio: {ratio}");
    }
}
