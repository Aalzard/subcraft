//! Real egui events verify brush material widgets dispatch changes without document edits.
use subcraft_app::Session;
use subcraft_core::ChannelKind;
use subcraft_doc::ProjectionMode;
use subcraft_paint::NormalPaintBlend;
use subcraft_ui::{
    panels::{brush_material, shelf::ShelfDrag},
    state::UiState,
};

fn frame(state: &mut UiState, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let mut output = ctx.run_ui(
        egui::RawInput {
            events,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(600.0, 1800.0),
            )),
            ..Default::default()
        },
        |ui| brush_material::show(state, ui),
    );
    output.textures_delta.clear();
    output
}
fn position(output: &egui::FullOutput, text: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::epaint::Shape::Text(t)
                if t.galley.text() == text || t.galley.text().ends_with(&format!("  {text}")) =>
            {
                Some(t.pos + t.galley.size() * 0.5)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no text: {text}"))
}
fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}
fn click(state: &mut UiState, ctx: &egui::Context, text: &str) {
    let out = frame(state, ctx, vec![]);
    let p = position(&out, text);
    frame(
        state,
        ctx,
        vec![egui::Event::PointerMoved(p), button(p, true)],
    );
    frame(state, ctx, vec![button(p, false)]);
}

#[test]
fn shelf_drop_projection_blend_and_clear_share_app_brush_state() {
    let dir = std::env::temp_dir().join(format!("subcraft-brush-ui-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test_normalDX.png");
    let (bytes, _) =
        subcraft_image::encode_bitmap(&subcraft_image::Bitmap::new_u8(1, 1, 3, vec![128, 64, 255]))
            .unwrap();
    std::fs::write(&path, bytes).unwrap();
    let mut state = UiState::new(
        Session::new(subcraft_mesh::primitives::cube(2.0), 128),
        None,
    );
    state.textures.add_files(None, &[path]);
    let before = state.session.document().clone();
    let history = state.session.history().entries().len();
    let ctx = egui::Context::default();
    subcraft_ui::theme::apply(&ctx);
    for _ in 0..3 {
        frame(&mut state, &ctx, vec![]);
    }
    let output = frame(&mut state, &ctx, vec![]);
    let pos = position(&output, "Normal");
    frame(
        &mut state,
        &ctx,
        vec![egui::Event::PointerMoved(pos), button(pos, true)],
    );
    egui::DragAndDrop::set_payload(&ctx, ShelfDrag::Texture(0));
    frame(
        &mut state,
        &ctx,
        vec![egui::Event::PointerMoved(pos), button(pos, false)],
    );
    let id = state.session.brush.material.textures[&ChannelKind::Normal];
    assert!(
        state
            .session
            .resources()
            .bitmap(id)
            .unwrap()
            .sample(0.5, 0.5, Default::default(), false)[1]
            > 0.74
    );
    click(&mut state, &ctx, "Planar");
    assert_eq!(
        state.session.brush.material.projection.mode,
        ProjectionMode::Planar
    );
    click(&mut state, &ctx, "Detail");
    assert_eq!(
        state.session.brush.material.normal_blend,
        NormalPaintBlend::Detail
    );
    click(&mut state, &ctx, "Clear image");
    assert!(state.session.brush.material.textures.is_empty());
    assert_eq!(state.session.document(), &before);
    assert_eq!(state.session.history().entries().len(), history);
    std::fs::remove_dir_all(dir).unwrap();
}
