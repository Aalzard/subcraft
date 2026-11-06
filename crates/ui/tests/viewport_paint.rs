//! Painting in the 3D view through real egui input (issue #95 and the "brush does not draw"
//! report): a drag across the model must paint the layer, or the mask, with the whole brush,
//! at 100 % and at a scaled display.
use egui::{Event, Modifiers, PointerButton, Pos2, Rect};
use subcraft_app::{Command, LayerTarget, NewLayer, Session};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::LayerContent;
use subcraft_paint::BrushMaterial;
use subcraft_ui::{state::{Tool, UiState}, theme, viewport};

const SCREEN: egui::Vec2 = egui::vec2(1200.0, 800.0);

fn setup() -> (UiState, egui::Context) {
    let mut state = UiState::new(Session::new(subcraft_mesh::primitives::sample_crate(), 512), None);
    state.tool = Tool::Brush;
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    (state, ctx)
}

fn frame(state: &mut UiState, ctx: &egui::Context, events: Vec<Event>, ppp: f32) {
    let mut input = egui::RawInput { events, screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)), ..Default::default() };
    input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(ppp);
    let mut output = ctx.run_ui(input, |ui| viewport::show(state, ui));
    output.textures_delta.clear();
}

fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

/// A horizontal drag through the middle of the view, in small steps like a real mouse.
fn drag(state: &mut UiState, ctx: &egui::Context, ppp: f32) {
    for _ in 0..3 {
        frame(state, ctx, vec![], ppp);
    }
    let y = SCREEN.y * 0.55;
    let (from, to) = (Pos2::new(SCREEN.x * 0.35, y), Pos2::new(SCREEN.x * 0.65, y));
    frame(state, ctx, vec![Event::PointerMoved(from)], ppp);
    frame(state, ctx, vec![button(from, true)], ppp);
    for k in 1..=30 {
        frame(state, ctx, vec![Event::PointerMoved(from.lerp(to, k as f32 / 30.0))], ppp);
    }
    frame(state, ctx, vec![button(to, false)], ppp);
    frame(state, ctx, vec![], ppp);
}

fn paint_texels(state: &UiState) -> u32 {
    let LayerContent::Paint(p) = &state.session.selected_layer().unwrap().content else { panic!("a paint layer") };
    let Some(img) = p.channels.get(&ChannelKind::BaseColor) else { return 0 };
    (0..img.height()).map(|y| (0..img.width()).filter(|&x| img.get(x, y)[3] > 0.5).count() as u32).sum()
}

fn mask_texels(state: &UiState) -> u32 {
    let m = &state.session.selected_layer().unwrap().mask.as_ref().unwrap().base;
    (0..m.height()).map(|y| (0..m.width()).filter(|&x| m.get(x, y)[0] > 0.5).count() as u32).sum()
}

fn content_stroke(ppp: f32) -> u32 {
    let (mut state, ctx) = setup();
    state.session.brush.material = BrushMaterial::with(&[(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))]);
    drag(&mut state, &ctx, ppp);
    paint_texels(&state)
}

fn mask_stroke(ppp: f32) -> u32 {
    let (mut state, ctx) = setup();
    state.session.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = state.session.selection().layer.unwrap();
    state.session.execute(Command::AddMask { layer, white: false }).unwrap();
    state.session.execute(Command::SelectLayerTarget { layer, target: LayerTarget::Mask }).unwrap();
    state.session.brush.material.mask_value = 1.0;
    drag(&mut state, &ctx, ppp);
    mask_texels(&state)
}

#[test]
fn a_3d_drag_paints_the_layer_and_the_mask_alike_at_any_display_scale() {
    for ppp in [1.0, 1.5, 2.0] {
        let content = content_stroke(ppp);
        let mask = mask_stroke(ppp);
        println!("scale {ppp}: paint layer {content} texels, mask {mask} texels");
        assert!(content > 500, "scale {ppp}: the brush paints the model ({content} texels)");
        assert!(mask * 10 >= content * 9, "scale {ppp}: the mask stroke is as wide as the paint stroke ({mask} vs {content})");
    }
}
