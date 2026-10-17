//! Geometry and interaction contracts for quiet, alternating panel rows.
use egui::{Color32, Event, Pos2, Rect};
use subcraft_app::{Command, NewLayer, Session};
use subcraft_ui::{panels, state::UiState, theme, widgets};

fn colors(output: &egui::FullOutput, palette: &[Color32]) -> Vec<(Rect, Color32)> {
    output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Rect(r) if palette.contains(&r.fill) => Some((r.rect, r.fill)),
            _ => None,
        })
        .collect()
}

#[test]
fn panel_headers_and_properties_alternate_independently_and_reset_each_frame() {
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    for _ in 0..3 {
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            widgets::panel(ui, |ui| {
                widgets::section(ui, "one", "One", true, |ui| {
                    widgets::prop_row(ui, "A", |ui| {
                        ui.label("A value");
                    });
                    ui.add_enabled_ui(false, |ui| {
                        widgets::prop_row(ui, "B", |ui| {
                            ui.label("B value");
                        })
                    });
                    widgets::prop_row(ui, "C", |ui| {
                        ui.label("C value");
                    });
                });
                widgets::section(ui, "two", "Two", false, |_| {});
                widgets::section(ui, "three", "Three", false, |_| {});
            });
        });
        out.textures_delta.clear();
        let heads = colors(&out, &theme::SECTION_BANDS);
        assert_eq!(
            heads.iter().map(|(_, c)| *c).collect::<Vec<_>>(),
            [
                theme::SECTION_BANDS[0],
                theme::SECTION_BANDS[1],
                theme::SECTION_BANDS[0]
            ]
        );
        assert!(heads.iter().all(|(r, _)| r.height() >= 32.0));
        // Disabled UI may fade its band, but it must still advance the alternating index.
        let props = colors(&out, &theme::ROW_BANDS);
        assert!(
            props
                .iter()
                .filter(|(_, c)| *c == theme::ROW_BANDS[0])
                .count()
                >= 2
        );
        assert!(props.iter().all(|(r, _)| r.height() >= 28.0));
    }
    for (a, b) in theme::ROW_BANDS[0]
        .to_array()
        .into_iter()
        .zip(theme::ROW_BANDS[1].to_array())
        .take(3)
    {
        assert!(
            (10..=14).contains(&a.abs_diff(b)),
            "visible but restrained band contrast"
        );
    }
}

#[test]
fn parts_history_and_layers_render_full_width_bands_without_document_edits() {
    for panel in [
        panels::parts::show,
        panels::history::show,
        panels::layers::show,
    ] {
        let mut state = UiState::new(
            Session::new(subcraft_mesh::primitives::sample_crate(), 64),
            None,
        );
        for _ in 0..3 {
            state
                .session
                .execute(Command::AddLayer(NewLayer::Paint))
                .unwrap();
        }
        let before = state.session.document().clone();
        let history = state.session.history().entries().len();
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let mut last = None;
        for _ in 0..3 {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(380.0, 650.0))),
                    ..Default::default()
                },
                |ui| panel(&mut state, ui),
            );
            out.textures_delta.clear();
            last = Some(out);
        }
        let out = last.unwrap();
        let bands = colors(&out, &theme::ROW_BANDS);
        assert!(bands.iter().any(|(_, c)| *c == theme::ROW_BANDS[0]));
        assert!(bands.iter().any(|(_, c)| *c == theme::ROW_BANDS[1]));
        assert!(
            bands
                .iter()
                .all(|(r, _)| r.width() > 300.0 && r.height() == theme::ROW_HEIGHT)
        );
        assert_eq!(state.session.document(), &before);
        assert_eq!(state.session.history().entries().len(), history);
    }
}

#[test]
fn hovering_either_band_brightens_it_and_selection_still_wins() {
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    let mut rows: Vec<Rect> = Vec::new();
    // Warm layout, then move over each alternating row and the selected row.
    for target in [None, None, Some(0), Some(1), Some(2)] {
        let events = target.map_or_else(Vec::new, |index| {
            vec![Event::PointerMoved(rows[index].center())]
        });
        rows.clear();
        let mut out = ctx.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                for index in 0..3 {
                    rows.push(
                        widgets::striped_list_row(
                            ui,
                            index,
                            index == 2,
                            egui::Sense::click(),
                            |ui| {
                                ui.label("Row");
                            },
                        )
                        .rect,
                    );
                }
            },
        );
        out.textures_delta.clear();
        let fills = colors(
            &out,
            &[
                theme::ROW_BANDS[0],
                theme::ROW_BANDS[1],
                theme::BG_3,
                theme::ACCENT_SOFT,
            ],
        );
        let fill = |index: usize| {
            fills
                .iter()
                .find(|(rect, _)| *rect == rows[index])
                .unwrap()
                .1
        };
        assert_eq!(fill(2), theme::ACCENT_SOFT);
        for index in 0..2 {
            let color = fill(index);
            if target == Some(index) {
                for (hover, base) in color
                    .to_array()
                    .into_iter()
                    .zip(theme::ROW_BANDS[index].to_array())
                    .take(3)
                {
                    assert!(hover > base, "hover must brighten either alternating band");
                }
            } else {
                assert_eq!(color, theme::ROW_BANDS[index]);
            }
        }
    }
}

#[test]
fn striped_row_selection_remains_prominent_and_child_buttons_take_the_click() {
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    let mut child = false;
    let mut parent = false;
    let mut at = Pos2::ZERO;
    for events in [
        vec![],
        vec![],
        vec![
            Event::PointerMoved(Pos2::new(38.0, 22.0)),
            Event::PointerButton {
                pos: Pos2::new(38.0, 22.0),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
        vec![Event::PointerButton {
            pos: Pos2::new(38.0, 22.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    ] {
        let mut out = ctx.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                let response = widgets::striped_list_row(ui, 1, true, egui::Sense::click(), |ui| {
                    let r = ui.button("Hide");
                    at = r.rect.center();
                    child |= r.clicked();
                });
                parent |= response.clicked();
            },
        );
        out.textures_delta.clear();
        assert!(!colors(&out, &[theme::ACCENT_SOFT]).is_empty());
    }
    assert!(at.distance(Pos2::new(38.0, 22.0)) < 20.0);
    assert!(
        child && !parent,
        "child controls retain priority over row selection"
    );
}
