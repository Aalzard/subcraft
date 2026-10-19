//! Material previews and compact adjustments retain distinct, usable hit targets.
use egui::{Event, Pos2, Rect};
use subcraft_app::{Command, LayerTarget, NewLayer, Session};
use subcraft_doc::{Generator, GeneratorKind, Levels, MaskEffectKind};
use subcraft_ui::{panels, state::UiState, theme};

fn fixture() -> UiState {
    let mut s = Session::new(subcraft_mesh::primitives::cube(1.0), 64);
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = s.selection().layer.unwrap();
    s.execute(Command::AddMask { layer, white: false }).unwrap();
    s.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Generator(Generator::new(GeneratorKind::Edges)) }).unwrap();
    s.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Levels(Levels::default()) }).unwrap();
    s.execute(Command::SelectLayerTarget { layer, target: LayerTarget::Mask }).unwrap();
    UiState::new(s, None)
}

fn frame(ctx: &egui::Context, state: &mut UiState, width: f32, events: Vec<Event>) -> egui::FullOutput {
    let mut out = ctx.run_ui(egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(width, 650.0))),
        events, ..Default::default()
    }, |ui| panels::layers::show(state, ui));
    out.textures_delta.clear();
    out
}

fn text(out: &egui::FullOutput, name: &str) -> Option<Rect> {
    out.shapes.iter().find_map(|s| match &s.shape {
        egui::Shape::Text(t) if t.galley.text() == name => Some(Rect::from_min_size(t.pos, t.galley.size())),
        _ => None,
    })
}

#[test]
fn larger_previews_and_compact_effects_fit_sidebar_widths_without_mutating_layers() {
    for width in [280.0, 340.0, 480.0] {
        let ctx = egui::Context::default(); theme::apply(&ctx);
        let mut state = fixture();
        let before = state.session.document().clone();
        let history = state.session.history().entries().len();
        for _ in 0..3 {
            let out = frame(&ctx, &mut state, width, vec![]);
            let mut heights = Vec::new();
            let mut thumbs = Vec::new();
            for shape in &out.shapes {
                if let egui::Shape::Rect(r) = &shape.shape {
                    if theme::ROW_BANDS.contains(&r.fill) || r.fill == theme::ACCENT_SOFT {
                        heights.push(r.rect.height());
                    }
                    if r.stroke.width > 0.0 && (r.rect.width() - theme::LAYER_THUMB_SIZE).abs() < 0.01 && (r.rect.height() - theme::LAYER_THUMB_SIZE).abs() < 0.01 {
                        assert!(r.rect.left() >= 0.0 && r.rect.right() <= width);
                        thumbs.push(r.rect);
                    }
                }
                if let egui::Shape::Text(t) = &shape.shape {
                    assert!(t.pos.x + t.galley.size().x <= width + 1.0, "text overflow at {width}: {}", t.galley.text());
                }
            }
            assert!(heights.contains(&theme::LAYER_ROW_HEIGHT));
            assert_eq!(heights.iter().filter(|&&h| h == theme::EFFECT_ROW_HEIGHT).count(), 2);
            assert!(thumbs.len() >= 2);
            let header = text(&out, "Mask effects").unwrap();
            let levels = text(&out, "Levels").unwrap();
            assert!(levels.top() > header.bottom());
        }
        assert_eq!(state.session.document(), &before);
        assert_eq!(state.session.history().entries().len(), history);
    }
}

#[test]
fn collapsing_effects_keeps_selection_and_document_then_restores_rows() {
    let ctx = egui::Context::default(); theme::apply(&ctx);
    let mut state = fixture();
    let before = state.session.document().clone();
    let selected = state.session.selected_effect();
    let mut out = frame(&ctx, &mut state, 380.0, vec![]);
    for _ in 0..2 { out = frame(&ctx, &mut state, 380.0, vec![]); }
    for expanded in [false, true] {
        let pos = text(&out, "Mask effects").unwrap().center();
        for pressed in [true, false] {
            let _ = frame(&ctx, &mut state, 380.0, vec![Event::PointerMoved(pos), Event::PointerButton {
                pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default()
            }]);
        }
        out = frame(&ctx, &mut state, 380.0, vec![]);
        assert_eq!(text(&out, "Levels").is_some(), expanded);
        assert_eq!(state.session.selected_effect(), selected);
        assert_eq!(state.session.document(), &before);
    }
}
