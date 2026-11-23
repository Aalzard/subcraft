//! A texture dropped on the model offers "Place as a decal here": one click makes a fill layer
//! that shows the image once at that spot and brings up the manipulator to slide it around.
use glam::Vec3;
use subcraft_app::Session;
use subcraft_app::decal::DecalSpot;
use subcraft_doc::{LayerContent, ProjectionMode, UvWrap};
use subcraft_ui::{projection_gizmo::GizmoMode, state::UiState, theme, viewport};

const DECAL: &str = "Place as a decal here";

fn frame(state: &mut UiState, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let input = egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))), ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| viewport::drop_channel_menu(state, ui, 0.0));
    out.textures_delta.clear();
    out
}

fn text_pos(out: &egui::FullOutput, text: &str) -> Option<egui::Pos2> {
    fn find(shape: &egui::epaint::Shape, text: &str) -> Option<egui::Pos2> {
        match shape {
            egui::epaint::Shape::Text(t) if t.galley.text() == text => Some(t.pos + t.galley.size() * 0.5),
            egui::epaint::Shape::Vec(v) => v.iter().find_map(|s| find(s, text)),
            _ => None,
        }
    }
    out.shapes.iter().find_map(|s| find(&s.shape, text))
}

fn click(at: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
}

fn image(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("subcraft-decal-drop-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("badge.png");
    subcraft_image::save_image(&file, 8, 4, 4, &[1.0; 8 * 4 * 4], subcraft_image::FileFormat::Png8).unwrap();
    file
}

#[test]
fn the_drop_menu_places_the_image_as_a_decal_and_arms_the_manipulator() {
    let mut state = UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    let layers = state.session.selected_texture_set().layers.len();
    let spot = DecalSpot { position: Vec3::Z, normal: Vec3::Z, up: Vec3::Y };
    state.drop_channel_menu = Some((egui::pos2(200.0, 150.0), image("place"), Some(spot)));
    state.gizmo.mode = GizmoMode::Rotate;
    let mut out = frame(&mut state, &ctx, vec![]);
    for _ in 0..2 {
        out = frame(&mut state, &ctx, vec![]);
    }
    let button = text_pos(&out, DECAL).expect("the decal entry is offered for a drop on the model");
    frame(&mut state, &ctx, vec![egui::Event::PointerMoved(button)]);
    frame(&mut state, &ctx, vec![click(button, true)]);
    frame(&mut state, &ctx, vec![click(button, false)]);
    frame(&mut state, &ctx, vec![]);

    assert!(state.drop_channel_menu.is_none(), "the menu closes");
    assert_eq!(state.session.selected_texture_set().layers.len(), layers + 1, "one new layer");
    let layer = state.session.selected_layer().expect("the decal is selected");
    assert_eq!(layer.name, "badge");
    let LayerContent::Fill(fill) = &layer.content else { panic!("a fill layer") };
    assert_eq!((fill.projection.mode, fill.projection.uv_wrap), (ProjectionMode::Planar, UvWrap::None));
    assert_eq!(fill.projection.transform3.scale, [0.25, 0.125, 0.5], "keeps the image's 2:1 shape");
    assert!(state.gizmo.visible && state.gizmo.mode == GizmoMode::Surface, "ready to slide over the model");
}

#[test]
fn a_drop_without_a_surface_point_offers_channels_only() {
    let mut state = UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    state.drop_channel_menu = Some((egui::pos2(200.0, 150.0), image("none"), None));
    let mut out = frame(&mut state, &ctx, vec![]);
    for _ in 0..2 {
        out = frame(&mut state, &ctx, vec![]);
    }
    assert!(text_pos(&out, DECAL).is_none());
    assert!(text_pos(&out, "Cancel").is_some(), "the channel menu itself is shown");
}
