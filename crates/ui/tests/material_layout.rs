//! Material controls must stay calm, readable and below their owning section header.
use egui::{Pos2, Rect};
use subcraft_app::Session;
use subcraft_ui::{panels::brush, state::UiState, theme, widgets};

#[test]
fn expanded_material_action_never_overlaps_its_header_at_sidebar_widths() {
    for width in [280.0, 340.0, 420.0] {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let mut state = UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
        let mut last = None;
        for _ in 0..3 {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(width, 2400.0))),
                    ..Default::default()
                },
                |ui| brush::show(&mut state, ui),
            );
            out.textures_delta.clear();
            last = Some(out);
        }
        let out = last.unwrap();
        let texts = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t),
                _ => None,
            })
            .collect::<Vec<_>>();
        let heading = texts
            .iter()
            .find(|t| t.galley.text() == "MATERIAL")
            .unwrap();
        let action = texts
            .iter()
            .find(|t| t.galley.text().ends_with("Paint whole material…"))
            .unwrap();
        assert!(
            action.pos.y >= heading.pos.y + heading.galley.size().y + 14.0,
            "body overlaps header at {width}"
        );
        assert!(action.pos.x + action.galley.size().x <= width - 12.0);
        for text in texts.iter().filter(|t| t.galley.text().ends_with("Image…")) {
            assert!(
                text.pos.x + text.galley.size().x <= width - 12.0,
                "image button overflow at {width}"
            );
        }
    }
}

#[test]
fn quiet_actions_have_soft_corners_muted_text_and_a_readable_hit_target() {
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    let mut bounds = Rect::NOTHING;
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        bounds = widgets::quiet_button(ui, subcraft_ui::icons::IMAGE, "Image…").rect;
    });
    out.textures_delta.clear();
    assert!(bounds.height() >= 28.0);
    assert!(theme::TEXT.r() <= 210 && theme::TEXT.b() <= 220);
    let background = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Rect(r) if r.rect == bounds => Some(r),
            _ => None,
        })
        .unwrap();
    assert_eq!(background.fill, theme::BG_2);
    assert_eq!(background.stroke.width, 0.0);
    assert!(background.corner_radius.nw >= 4);
    assert!(out.shapes.iter().any(|s| {
        match &s.shape {
            egui::Shape::Text(t) => t
                .galley
                .job
                .sections
                .iter()
                .all(|section| section.format.color == theme::TEXT_DIM),
            _ => false,
        }
    }));
}
