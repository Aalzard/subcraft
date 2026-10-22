//! Real egui events verify brush material widgets dispatch changes without document edits.
use subcraft_app::{Command, Session};
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
    if !state.session.brush.material.channels.get(&ChannelKind::Normal).is_some_and(|c| c.enabled) {
        click(&mut state, &ctx, "nrm");
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
    click(&mut state, &ctx, "nrm");
    let output = frame(&mut state, &ctx, vec![]);
    assert!(!output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "Normal blend")));
    assert_eq!(state.session.document(), &before);
    assert_eq!(state.session.history().entries().len(), history);
    std::fs::remove_dir_all(dir).unwrap();
}

fn dragged_drop_on(state: &mut UiState, ctx: &egui::Context, payload: ShelfDrag, row: &str) {
    egui::DragAndDrop::set_payload(ctx, payload);
    let output = frame(state, ctx, vec![]);
    let pos = position(&output, row);
    frame(state, ctx, vec![egui::Event::PointerMoved(pos), button(pos, true)]);
    egui::DragAndDrop::set_payload(ctx, payload);
    frame(state, ctx, vec![egui::Event::PointerMoved(pos), button(pos, false)]);
}

/// GitHub issue 58: a texture dragged from the Shelf onto a Brush channel slot sets that image,
/// including a channel that is switched off, and starter-pack textures work like files.
#[test]
fn shelf_textures_and_starter_pack_images_drop_onto_brush_channels_even_when_switched_off() {
    let dir = std::env::temp_dir().join(format!("subcraft-brush-drop-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rock_normal.png");
    let (bytes, _) = subcraft_image::encode_bitmap(&subcraft_image::Bitmap::new_u8(1, 1, 3, vec![128, 64, 255])).unwrap();
    std::fs::write(&path, bytes).unwrap();
    let mut state = UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 128), None);
    state.textures.add_files(None, &[path]);
    let (before, history) = (state.session.document().clone(), state.session.history().entries().len());
    let ctx = egui::Context::default();
    subcraft_ui::theme::apply(&ctx);
    for _ in 0..3 { frame(&mut state, &ctx, vec![]); }
    if state.session.brush.material.channels.get(&ChannelKind::Normal).is_some_and(|c| c.enabled) {
        click(&mut state, &ctx, "nrm");
    }
    assert!(!state.session.brush.material.channels[&ChannelKind::Normal].enabled);
    // Nothing is dragged: the switched-off channel offers no slot.
    let idle = frame(&mut state, &ctx, vec![]);
    assert!(!idle.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "Normal")));

    dragged_drop_on(&mut state, &ctx, ShelfDrag::Texture(0), "Normal");
    let material = &state.session.brush.material;
    assert!(material.channels[&ChannelKind::Normal].enabled, "the dropped image switches its channel on");
    assert!(material.textures.contains_key(&ChannelKind::Normal));
    let first = material.textures[&ChannelKind::Normal];

    // A starter-pack normal dropped on Roughness (any slot takes any image, like Image…).
    state.session.execute(Command::ToggleBrushChannel { channel: ChannelKind::Roughness, solo: false }).unwrap();
    if !state.session.brush.material.channels[&ChannelKind::Roughness].enabled {
        state.session.execute(Command::ToggleBrushChannel { channel: ChannelKind::Roughness, solo: false }).unwrap();
    }
    dragged_drop_on(&mut state, &ctx, ShelfDrag::Starter(0), "Roughness");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while !state.session.brush.material.textures.contains_key(&ChannelKind::Roughness) {
        assert!(std::time::Instant::now() < deadline, "starter image never reached the brush");
        subcraft_ui::asset_preview::show(&mut state, &ctx);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let second = state.session.brush.material.textures[&ChannelKind::Roughness];
    assert_ne!(first, second);
    assert_eq!(state.session.brush.material.textures[&ChannelKind::Normal], first, "other slots are untouched");
    assert!(state.session.resources().bitmap(second).is_some_and(|b| b.width == 1024));
    assert!(!state.asset_preview.is_applying());
    // Tool state only: no layer, history or document edit.
    assert_eq!(state.session.document(), &before);
    assert_eq!(state.session.history().entries().len(), history);
    std::fs::remove_dir_all(dir).unwrap();
}
