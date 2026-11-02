//! ID Color Mask Properties (ADR-0041): missing-map hint, swatches, eyedropper toggle.

use std::collections::BTreeMap;

use egui::{Pos2, Rect};
use subcraft_app::{Command, MeshMapKind, NewLayer, Session};
use subcraft_doc::MaskEffectKind;
use subcraft_image::TiledImage;
use subcraft_ui::{panels::effects, state::UiState, theme};

fn labels(out: &egui::FullOutput) -> Vec<String> {
    out.shapes.iter().filter_map(|s| match &s.shape { egui::Shape::Text(t) => Some(t.galley.text().to_string()), _ => None }).collect()
}

fn find(out: &egui::FullOutput, needle: &str) -> Option<Pos2> {
    out.shapes.iter().find_map(|s| match &s.shape {
        egui::Shape::Text(t) if t.galley.text().contains(needle) => Some(t.pos + t.galley.size() * 0.5),
        _ => None,
    })
}

#[test]
fn id_color_properties_show_hint_swatches_and_toggle_the_eyedropper() {
    let mut session = Session::new(subcraft_mesh::primitives::plane(2.0, 2), 64);
    session.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = session.selection().layer.unwrap();
    session.execute(Command::AddMask { layer, white: false }).unwrap();
    session.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::IdColor(Default::default()) }).unwrap();
    let effect = session.selected_layer().unwrap().mask.as_ref().unwrap().effects[0].id;
    let mut state = UiState::new(session, None);
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(480.0, 1200.0))), ..Default::default() };
    let frame = |state: &mut UiState, input: egui::RawInput| {
        let mut out = ctx.run_ui(input, |ui| { assert!(effects::properties(state, ui)); });
        out.textures_delta.clear();
        out
    };

    let out = frame(&mut state, input.clone());
    let text = labels(&out);
    assert!(text.iter().any(|s| s.contains("No ID map baked")), "{text:?}");
    assert!(text.iter().any(|s| s == "Tolerance") && text.iter().any(|s| s == "Softness"), "{text:?}");
    assert!(text.iter().any(|s| s == "No colors picked"));

    // Clicking "Pick from model" arms the 3D-view eyedropper for this effect.
    let pos = find(&out, "Pick from model").expect("pick button");
    let button = |pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    let mut press = input.clone();
    press.events = vec![egui::Event::PointerMoved(pos), button(true)];
    frame(&mut state, press);
    let mut release = input.clone();
    release.events = vec![button(false)];
    frame(&mut state, release);
    assert_eq!(state.picking_id_color, Some((layer, effect)));
    let out = frame(&mut state, input.clone());
    assert!(find(&out, "Picking: click the model").is_some());

    // With a baked map and picked colors: no hint, one swatch per color.
    let ts = state.session.selection().texture_set;
    state.session.set_mesh_maps(ts, BTreeMap::from([(MeshMapKind::Id, TiledImage::new(64, 64, 3, [0.5, 0.2, 0.2, 1.0]))]));
    state.session.execute(Command::SetIdColors { layer, effect, colors: vec![[0.5, 0.2, 0.2], [0.2, 0.2, 0.5]] }).unwrap();
    let out = frame(&mut state, input);
    let text = labels(&out);
    assert!(!text.iter().any(|s| s.contains("No ID map baked")), "{text:?}");
    assert!(!text.iter().any(|s| s == "No colors picked"));
}
