use eframe::egui::{self, Color32, Response, RichText, Ui};

pub fn show<R>(ctx: &egui::Context, contents: impl FnOnce(&mut Ui) -> R) {
    egui::TopBottomPanel::top("tabs")
        .exact_height(44.0)
        .frame(
            egui::Frame::new()
                .fill(Color32::from_rgb(25, 30, 36))
                .inner_margin(egui::Margin::symmetric(10, 7)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("W")
                        .color(crate::view::ACCENT)
                        .strong()
                        .size(18.0),
                );
                contents(ui)
            });
        });
}

pub fn title(ui: &mut Ui, selected: bool, title: &str) -> Response {
    ui.selectable_label(selected, title)
}

pub fn close(ui: &mut Ui) -> Response {
    ui.small_button("×")
}

pub fn button(ui: &mut Ui, text: &str) -> Response {
    ui.button(text)
}

pub fn menu<R>(ui: &mut Ui, contents: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<Option<R>> {
    ui.menu_button("v", contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolbar_controls_share_height_center_and_square_action_targets() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for dark in [false, true] {
                let ctx = egui::Context::default();
                ctx.set_pixels_per_point(scale);
                ctx.set_visuals(if dark {
                    egui::Visuals::dark()
                } else {
                    egui::Visuals::light()
                });
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(480.0, 300.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        show(ctx, |ui| {
                            let tab = title(ui, true, "python.exe").rect;
                            let controls = [
                                close(ui).rect,
                                button(ui, "+").rect,
                                menu(ui, |_| {}).response.rect,
                                button(ui, "...").rect,
                            ];
                            for rect in controls {
                                assert!(
                                    (rect.height() - tab.height()).abs() < 0.1,
                                    "Toolbar heights differ: tab={tab:?}, action={rect:?}"
                                );
                                assert!(
                                    (rect.width() - rect.height()).abs() < 0.1,
                                    "Non-square action target: {rect:?}"
                                );
                                assert!(
                                    (rect.center().y - tab.center().y).abs() < 0.1,
                                    "Toolbar centers differ"
                                );
                                assert!(
                                    rect.min.y >= 8.0 && rect.max.y <= 36.0,
                                    "Uneven toolbar vertical inset: {rect:?}"
                                );
                            }
                        });
                    },
                );
            }
        }
    }
}
