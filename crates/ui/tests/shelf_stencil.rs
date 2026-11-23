//! Shelf alphas and textures as the brush stencil: by "Use as stencil" (the function behind the
//! right-click entry) and by a drop on the Stencil block of the tool Properties.
use subcraft_app::Session;
use subcraft_ui::{
    panels::{brush, shelf::{self, ShelfDrag}},
    state::UiState,
};

fn state() -> UiState {
    UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None)
}

fn frame(state: &mut UiState, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let input = egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 3000.0))), ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| brush::show(state, ui));
    out.textures_delta.clear();
    out
}

fn text_pos(out: &egui::FullOutput, text: &str) -> Option<egui::Pos2> {
    fn find(shape: &egui::epaint::Shape, text: &str) -> Option<egui::Pos2> {
        match shape {
            egui::epaint::Shape::Text(t) if t.galley.text() == text || t.galley.text().ends_with(&format!("  {text}")) => Some(t.pos + t.galley.size() * 0.5),
            egui::epaint::Shape::Vec(v) => v.iter().find_map(|s| find(s, text)),
            _ => None,
        }
    }
    out.shapes.iter().find_map(|s| find(&s.shape, text))
}

fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
}

#[test]
fn a_shelf_alpha_or_texture_becomes_the_stencil() {
    let mut s = state();
    let (before, history) = (s.session.document().clone(), s.session.history().entries().len());
    // A built-in alpha with an image.
    let i = s.library.alphas.iter().position(|a| a.bitmap.is_some()).expect("a built-in alpha with an image");
    let (name, bitmap) = (s.library.alphas[i].name.clone(), s.library.alphas[i].bitmap.clone().unwrap());
    assert_eq!(shelf::use_as_stencil(&mut s, ShelfDrag::Alpha(i)).unwrap(), name);
    let st = s.session.brush.stencil.clone().expect("the stencil is set");
    assert!(std::sync::Arc::ptr_eq(&st.bitmap, &bitmap), "the alpha's own image, not a copy");
    assert!(!st.is_transformed(), "placed like a newly loaded stencil");

    // A texture from the shelf replaces it.
    let dir = std::env::temp_dir().join(format!("subcraft-shelf-stencil-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("grate.png");
    subcraft_image::save_image(&path, 8, 4, 1, &[1.0; 32], subcraft_image::FileFormat::Png8).unwrap();
    s.textures.add_files(None, &[path]);
    assert_eq!(shelf::use_as_stencil(&mut s, ShelfDrag::Texture(0)).unwrap(), s.textures.items[0].name);
    let st = s.session.brush.stencil.clone().unwrap();
    assert_eq!((st.bitmap.width, st.bitmap.height), (8, 4));

    // What cannot be a stencil leaves the current one alone.
    if let Some(round) = s.library.alphas.iter().position(|a| a.bitmap.is_none()) {
        assert!(shelf::use_as_stencil(&mut s, ShelfDrag::Alpha(round)).is_err());
    }
    assert!(shelf::use_as_stencil(&mut s, ShelfDrag::Alpha(usize::MAX)).is_err());
    assert!(shelf::use_as_stencil(&mut s, ShelfDrag::Material(0)).is_err());
    assert!(!shelf::stencil_accepts(ShelfDrag::Material(0)) && shelf::stencil_accepts(ShelfDrag::Alpha(0)) && shelf::stencil_accepts(ShelfDrag::Texture(0)));
    assert!(std::sync::Arc::ptr_eq(&s.session.brush.stencil.clone().unwrap(), &st));
    // A stencil is tool state: no document or history change.
    assert_eq!(s.session.document(), &before);
    assert_eq!(s.session.history().entries().len(), history);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn an_alpha_dropped_on_the_stencil_block_becomes_the_stencil() {
    let mut s = state();
    let ctx = egui::Context::default();
    subcraft_ui::theme::apply(&ctx);
    let mut out = frame(&mut s, &ctx, vec![]);
    for _ in 0..3 {
        out = frame(&mut s, &ctx, vec![]);
    }
    // Open the Stencil block when it starts closed.
    if text_pos(&out, "Choose image…").is_none() {
        let header = text_pos(&out, "Stencil").expect("the Stencil block header");
        frame(&mut s, &ctx, vec![egui::Event::PointerMoved(header), button(header, true)]);
        frame(&mut s, &ctx, vec![button(header, false)]);
        for _ in 0..30 {
            out = frame(&mut s, &ctx, vec![]);
        }
    }
    let slot = text_pos(&out, "Choose image…").expect("the Stencil block is open");
    let i = s.library.alphas.iter().position(|a| a.bitmap.is_some()).unwrap();

    // Something that is no image is not taken.
    egui::DragAndDrop::set_payload(&ctx, ShelfDrag::Material(0));
    frame(&mut s, &ctx, vec![egui::Event::PointerMoved(slot), button(slot, true)]);
    egui::DragAndDrop::set_payload(&ctx, ShelfDrag::Material(0));
    frame(&mut s, &ctx, vec![egui::Event::PointerMoved(slot), button(slot, false)]);
    assert!(s.session.brush.stencil.is_none());

    egui::DragAndDrop::set_payload(&ctx, ShelfDrag::Alpha(i));
    frame(&mut s, &ctx, vec![egui::Event::PointerMoved(slot), button(slot, true)]);
    egui::DragAndDrop::set_payload(&ctx, ShelfDrag::Alpha(i));
    frame(&mut s, &ctx, vec![egui::Event::PointerMoved(slot), button(slot, false)]);
    let st = s.session.brush.stencil.clone().expect("the dropped alpha is the stencil");
    assert!(std::sync::Arc::ptr_eq(&st.bitmap, s.library.alphas[i].bitmap.as_ref().unwrap()));
}
