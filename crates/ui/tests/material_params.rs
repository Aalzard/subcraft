//! Properties > Material parameters (ADR-0047): a procedural material shows its sliders, and a
//! plain fill can have a setting exposed from the panel.
use egui::{Pos2, Rect};
use subcraft_app::{Command, NewLayer, Session};
use subcraft_ui::{panels, state::UiState, theme};

const HEIGHT: f32 = 2400.0;

fn frame(ctx: &egui::Context, state: &mut UiState, width: f32, events: Vec<egui::Event>) -> egui::FullOutput {
    let mut out = ctx.run_ui(
        egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(width, HEIGHT))), events, ..Default::default() },
        |ui| panels::properties::show(state, ui),
    );
    out.textures_delta.clear();
    out
}

fn texts(out: &egui::FullOutput) -> Vec<(String, Rect)> {
    out.shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) => Some((t.galley.text().to_string(), Rect::from_min_size(t.pos, t.galley.size()))),
            _ => None,
        })
        .collect()
}

fn find(out: &egui::FullOutput, needle: &str) -> Option<Rect> {
    texts(out).into_iter().find(|(t, _)| t.contains(needle)).map(|(_, r)| r)
}

fn click(ctx: &egui::Context, state: &mut UiState, width: f32, at: Pos2) -> egui::FullOutput {
    let press = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    frame(ctx, state, width, vec![egui::Event::PointerMoved(at)]);
    frame(ctx, state, width, vec![press(true)]);
    frame(ctx, state, width, vec![press(false)]);
    // A few more frames: menus open and lay out after the click.
    frame(ctx, state, width, vec![]);
    frame(ctx, state, width, vec![])
}

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    ctx
}

#[test]
fn a_procedural_material_shows_its_parameters_and_they_fit_the_panel() {
    let library = subcraft_assets::Library::builtin();
    let smart = library.smart_materials.iter().find(|m| m.name == "Procedural Painted Metal").unwrap();
    for width in [260.0_f32, 320.0, 420.0, 600.0] {
        let mut state = UiState::new(Session::new(subcraft_mesh::primitives::sample_crate(), 64), None);
        state.session.add_smart_material(smart).unwrap();
        let root = state.session.selected_layer().expect("the applied material is selected");
        let labels: Vec<String> = root.material.as_ref().expect("it carries parameters").params.iter().map(|p| p.label.clone()).collect();
        assert!(labels.len() >= 5);
        let ctx = context();
        let mut out = frame(&ctx, &mut state, width, vec![]);
        for _ in 0..2 {
            out = frame(&ctx, &mut state, width, vec![]);
        }
        let drawn = texts(&out);
        assert!(drawn.iter().any(|(t, _)| t.contains("MATERIAL PARAMETERS")), "{width}");
        for label in &labels {
            assert!(drawn.iter().any(|(t, _)| t == label), "parameter '{label}' has a row at width {width}");
        }
        assert!(drawn.iter().any(|(t, _)| t.contains("Add parameter")));
        for (t, r) in &drawn {
            assert!(r.right() <= width + 0.5 && r.left() >= -0.5, "text {t:?} spills out at width {width}: {r:?}");
        }
    }
}

#[test]
fn a_setting_of_a_fill_layer_is_exposed_from_the_panel_and_removed_again() {
    let width = 420.0;
    let mut state = UiState::new(Session::new(subcraft_mesh::primitives::sample_crate(), 64), None);
    state.session.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = state.session.selected_layer().unwrap().clone();
    assert!(layer.material.is_none());
    let ctx = context();
    let mut out = frame(&ctx, &mut state, width, vec![]);
    for _ in 0..2 {
        out = frame(&ctx, &mut state, width, vec![]);
    }
    // The section is closed on a layer without parameters: open it, then the menu.
    let header = find(&out, "MATERIAL PARAMETERS").expect("fill layers offer the section");
    let out = click(&ctx, &mut state, width, header.center());
    let add = find(&out, "Add parameter").expect("the open section has the Add parameter menu");
    let out = click(&ctx, &mut state, width, add.center());
    let submenu = texts(&out).into_iter().filter(|(t, r)| *t == layer.name && r.top() > add.top()).map(|(_, r)| r).last().expect("the layer is listed in the menu");
    let out = click(&ctx, &mut state, width, submenu.center());
    let entry = texts(&out).into_iter().filter(|(t, _)| t == "Roughness").map(|(_, r)| r).last().expect("its Roughness can be exposed");
    click(&ctx, &mut state, width, entry.center());

    let now = state.session.selected_layer().unwrap().clone();
    let m = now.material.as_ref().expect("the layer has parameters now");
    assert_eq!((m.params.len(), m.bindings.len()), (1, 1));
    assert_eq!(m.params[0].label, "Roughness");
    assert_eq!(now.content, layer.content, "exposing changes nothing by itself");
    assert_eq!(state.session.history().undo_label().unwrap(), "Material Parameters");

    // The slider drives the fill.
    let id = m.params[0].id.clone();
    state.session.execute(Command::SetMaterialParam { layer: now.id, param: id, value: subcraft_doc::ParamValue::Number(0.05) }).unwrap();
    assert_ne!(state.session.selected_layer().unwrap().content, layer.content);

    // Gear, then the bin on the row: back to a plain layer.
    let out = frame(&ctx, &mut state, width, vec![]);
    assert!(find(&out, "Roughness").is_some());
    let gear = find(&out, subcraft_ui::icons::GEAR).expect("a layer with parameters has the edit toggle");
    let out = click(&ctx, &mut state, width, gear.center());
    assert!(find(&out, "Use as defaults").is_some() && find(&out, "Remove all").is_some(), "edit mode shows its tools");
    let bin = find(&out, subcraft_ui::icons::TRASH).expect("each parameter can be removed");
    click(&ctx, &mut state, width, bin.center());
    assert!(state.session.selected_layer().unwrap().material.is_none());
}
