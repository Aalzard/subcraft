//! Properties panel: context-sensitive brush settings and layout at narrow widths / UI scales.
use egui::{Pos2, Rect};
use subcraft_app::{Command, NewLayer, Session};
use subcraft_ui::{panels, state::UiState, theme};

fn state_with(fill: bool) -> UiState {
    let mut state = UiState::new(Session::new(subcraft_mesh::primitives::sample_crate(), 64), None);
    if fill {
        state.session.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    }
    state
}

#[test]
fn properties_follows_the_selected_layer_and_can_be_pinned_to_the_brush() {
    let mut paint = state_with(false);
    assert!(panels::properties::shows_brush(&paint), "a paint layer shows the brush settings");
    let mut fill = state_with(true);
    assert!(!panels::properties::shows_brush(&fill), "a fill layer shows its material/projection");
    fill.brush_pinned = true;
    assert!(panels::properties::shows_brush(&fill));
    paint.properties_hosts_brush = false;
    assert!(!panels::properties::shows_brush(&paint), "a docked legacy Brush tab owns the brush settings");
}

/// Every text in the panel must sit inside the panel width and nothing may need horizontal scrolling.
#[test]
fn rows_fit_the_panel_at_narrow_widths_and_both_scales() {
    for fill in [true, false] {
        for scale in [1.0_f32, 1.5] {
            for width in [260.0_f32, 320.0, 420.0, 600.0] {
                let mut state = state_with(fill);
                let ctx = egui::Context::default();
                theme::apply(&ctx);
                ctx.set_pixels_per_point(scale);
                let mut last = None;
                for _ in 0..3 {
                    let mut out = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(width, 1400.0))),
                            ..Default::default()
                        },
                        |ui| panels::properties::show(&mut state, ui),
                    );
                    out.textures_delta.clear();
                    last = Some(out);
                }
                let out = last.unwrap();
                let mut texts = 0;
                for s in &out.shapes {
                    if let egui::Shape::Text(t) = &s.shape {
                        texts += 1;
                        let r = Rect::from_min_size(t.pos, t.galley.size());
                        assert!(r.right() <= width + 0.5 && r.left() >= -0.5, "text {:?} spills out at width {width} scale {scale} fill {fill}: {r:?}", t.galley.text());
                    }
                }
                assert!(texts > 5, "panel drew content");
            }
        }
    }
}
