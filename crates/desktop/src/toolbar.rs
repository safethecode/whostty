use eframe::egui::{self, Color32, Response, RichText, Ui};

pub const CONTROL: f32 = 28.0;

pub fn show<R>(ctx: &egui::Context, contents: impl FnOnce(&mut Ui) -> R) {
    egui::TopBottomPanel::top("tabs")
        .exact_height(44.0)
        .frame(
            egui::Frame::new()
                .fill(Color32::from_rgb(25, 30, 36))
                .inner_margin(egui::Margin::symmetric(10, 8)),
        )
        .show(ctx, |ui| {
            // The header stays dark even when eframe follows a light host theme.
            *ui.visuals_mut() = egui::Visuals::dark();
            ui.visuals_mut().override_text_color = Some(crate::view::FOREGROUND);
            ui.visuals_mut().selection.bg_fill = Color32::from_rgb(49, 72, 63);
            ui.visuals_mut().selection.stroke.color = crate::view::ACCENT;
            ui.spacing_mut().interact_size = egui::vec2(CONTROL, CONTROL);
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 0.0);
            ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);
            ui.horizontal_centered(|ui| {
                ui.add_sized(
                    [20.0, CONTROL],
                    egui::Label::new(
                        RichText::new("W")
                            .color(crate::view::ACCENT)
                            .strong()
                            .size(18.0),
                    ),
                );
                ui.add_space(4.0);
                contents(ui)
            });
        });
}

pub fn title(ui: &mut Ui, selected: bool, title: &str) -> Response {
    ui.selectable_label(selected, RichText::new(title).size(14.0))
}

pub fn tabs<R>(ui: &mut Ui, contents: impl FnOnce(&mut Ui) -> R) -> R {
    egui::ScrollArea::horizontal()
        .max_width((ui.available_width() - 104.0).max(100.0))
        .show(ui, |ui| ui.horizontal(contents).inner)
        .inner
}

pub fn close(ui: &mut Ui) -> Response {
    button(ui, "×")
}

pub fn button(ui: &mut Ui, text: &str) -> Response {
    ui.scope(|ui| {
        ui.spacing_mut().button_padding = egui::Vec2::ZERO;
        ui.add_sized(
            [CONTROL, CONTROL],
            egui::Button::new(RichText::new(text).size(14.0)),
        )
    })
    .inner
}

pub fn menu<R>(ui: &mut Ui, contents: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<Option<R>> {
    ui.scope(|ui| {
        ui.spacing_mut().button_padding = egui::Vec2::ZERO;
        let mut response = egui::menu::menu_custom_button(
            ui,
            egui::Button::new("").min_size(egui::vec2(CONTROL, CONTROL)),
            |ui| {
                ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);
                contents(ui)
            },
        );
        // A geometric chevron avoids missing-glyph boxes in the default UI font.
        let center = response.response.rect.center();
        ui.painter().add(egui::Shape::line(
            vec![
                center + egui::vec2(-3.5, -2.0),
                center + egui::vec2(0.0, 2.0),
                center + egui::vec2(3.5, -2.0),
            ],
            egui::Stroke::new(1.5, ui.visuals().text_color()),
        ));
        response.response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                ui.is_enabled(),
                "Profiles and split actions",
            )
        });
        response.response = response
            .response
            .on_hover_text("Profiles and split actions");
        response
    })
    .inner
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
                            let mut tab = egui::Rect::NOTHING;
                            let mut tab_close = egui::Rect::NOTHING;
                            tabs(ui, |ui| {
                                tab = title(ui, true, "python.exe").rect;
                                tab_close = close(ui).rect;
                                ui.add_space(4.0);
                            });
                            let controls = [
                                tab_close,
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

    #[test]
    fn overflowing_tabs_leave_all_toolbar_actions_inside_the_header() {
        for width in [480.0, 800.0, 1120.0] {
            let ctx = egui::Context::default();
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 300.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    show(ctx, |ui| {
                        tabs(ui, |ui| {
                            for index in 0..12 {
                                title(ui, index == 0, "한글 very long terminal tab title");
                                close(ui);
                                ui.add_space(4.0);
                            }
                        });
                        for rect in [
                            button(ui, "+").rect,
                            menu(ui, |_| {}).response.rect,
                            button(ui, "…").rect,
                        ] {
                            assert!(
                                rect.min.x >= 10.0 && rect.max.x <= width - 10.0 + 0.1,
                                "Action clipped at {width}: {rect:?}"
                            );
                            assert!(
                                (rect.min.y - 8.0).abs() < 0.1
                                    && (rect.height() - CONTROL).abs() < 0.1,
                                "Overflow changed header alignment: {rect:?}"
                            );
                        }
                    });
                },
            );
        }
    }

    #[test]
    fn sized_profile_button_still_opens_its_menu_on_click() {
        let ctx = egui::Context::default();
        let mut point = egui::Pos2::ZERO;
        let mut opened = false;
        for pressed in [None, Some(true), Some(false)] {
            let events = pressed.map_or_else(Vec::new, |pressed| {
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ]
            });
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(480.0, 300.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    show(ctx, |ui| {
                        let response = menu(ui, |ui| {
                            opened = true;
                            let _ = ui.button("PowerShell");
                        });
                        point = response.response.rect.center();
                    });
                },
            );
        }
        assert!(opened, "Sized profile button did not open its menu");
    }
}
