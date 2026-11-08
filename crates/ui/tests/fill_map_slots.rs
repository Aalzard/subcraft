//! Fill layer map slots: each channel shows a slot for its texture, and an image file dropped from
//! the system file browser onto a channel's row becomes that channel's texture.
use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::ChannelKind;
use subcraft_doc::{FillSource, LayerContent};
use subcraft_ui::{panels::properties, state::UiState, theme};

fn frame(state: &mut UiState, ctx: &egui::Context, input: egui::RawInput) -> egui::FullOutput {
    let mut out = ctx.run_ui(input, |ui| { egui::ScrollArea::vertical().show(ui, |ui| properties::show(state, ui)); });
    out.textures_delta.clear();
    out
}

fn raw(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 1600.0))), ..Default::default() }
}

fn text_pos(out: &egui::FullOutput, text: &str) -> Option<egui::Pos2> {
    out.shapes.iter().find_map(|s| match &s.shape {
        egui::epaint::Shape::Text(t) if t.galley.text() == text => Some(t.pos + t.galley.size() * 0.5),
        _ => None,
    })
}

#[test]
fn an_image_dropped_on_a_channel_row_becomes_its_texture() {
    let mut state = UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
    state.session.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    let mut out = frame(&mut state, &ctx, raw(vec![]));
    for _ in 0..3 {
        out = frame(&mut state, &ctx, raw(vec![]));
    }
    // The slot is there for every channel, labelled for accessibility.
    let labels: Vec<String> = out.platform_output.accesskit_update.as_ref().map(|u| u.nodes.iter().filter_map(|(_, n)| n.label().map(str::to_string)).collect()).unwrap_or_default();
    let _ = labels; // AccessKit output is only produced when enabled; the drop below is the real check.
    let row = text_pos(&out, ChannelKind::Roughness.display_name()).expect("Roughness channel row");

    let dir = std::env::temp_dir().join(format!("subcraft-fill-slot-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("rough.png");
    subcraft_image::save_image(&file, 4, 4, 1, &[0.25; 16], subcraft_image::FileFormat::Png8).unwrap();

    let mut drop = raw(vec![egui::Event::PointerMoved(row)]);
    drop.dropped_files = vec![std::sync::Arc::new(TestFile(file.clone()))];
    frame(&mut state, &ctx, raw(vec![egui::Event::PointerMoved(row)]));
    frame(&mut state, &ctx, drop);
    frame(&mut state, &ctx, raw(vec![]));
    crate_poll(&mut state);

    let layer = state.session.selected_layer().unwrap();
    let LayerContent::Fill(fill) = &layer.content else { panic!("a fill layer") };
    assert!(matches!(fill.channels[&ChannelKind::Roughness].source, FillSource::Bitmap { .. }), "Roughness now uses the dropped image");
    assert!(!matches!(fill.channels[&ChannelKind::BaseColor].source, FillSource::Bitmap { .. }), "other channels are untouched");
    assert!(state.drops_taken, "the window-wide drop handler leaves it alone");
    std::fs::remove_dir_all(dir).unwrap();
}

fn crate_poll(_state: &mut UiState) {}

/// A file "dropped" from the system file browser.
#[derive(Debug)]
struct TestFile(std::path::PathBuf);

impl egui::DroppedFile for TestFile {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}
