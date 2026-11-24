//! A MaterialCraft material imported as a graph layer: its exposed parameters are controls,
//! moving one re-cooks the graph, and the layer survives save and reopen.
//!
//! `fixtures/tinted_surface.mcmat` comes from `cargo run -p subcraft-materialcraft --example
//! mc_fixture`: MaterialCraft's "Material outputs" template with the base color exposed as
//! `tint` (linear 0.6, 0.2, 0.1) and the roughness as `rough` (0.5).

use std::path::Path;

use subcraft_app::Session;
use subcraft_app::graph_layer::{GraphLayers, engine_for_file, open_backend};
use subcraft_core::ChannelKind;
use subcraft_doc::{FillSource, GraphEngine, GraphParamKind, Layer, LayerContent};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tinted_surface.mcmat");

fn session() -> Session {
    Session::new(subcraft_mesh::primitives::cube(2.0), 128)
}

/// Linear value of `channel` in the layer's bitmap fill, at the first texel.
fn texel(s: &Session, layer: &Layer, channel: ChannelKind) -> [f32; 4] {
    let LayerContent::Fill(f) = &layer.content else { panic!("not a fill layer") };
    let c = &f.channels[&channel];
    assert!(c.enabled, "{channel:?} is on");
    let FillSource::Bitmap { resource } = c.source else { panic!("{channel:?} is not a bitmap") };
    s.resources().get(resource).expect("resource").bitmap.texel(0, 0, channel == ChannelKind::BaseColor)
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 2e-3
}

#[test]
fn materialcraft_files_pick_the_materialcraft_engine() {
    for name in ["a.mcmat", "B.MCRAFT", "c.sbs"] {
        assert_eq!(engine_for_file(name), Some(GraphEngine::MaterialCraft), "{name}");
    }
    assert_eq!(engine_for_file("d.sbsar"), Some(GraphEngine::Sbsar));
}

#[test]
fn exposed_parameters_become_controls() {
    let bytes = std::fs::read(FIXTURE).expect("fixture");
    let backend = open_backend(GraphEngine::MaterialCraft, &bytes, "tinted_surface.mcmat").expect("open");
    let graphs = backend.graphs();
    assert_eq!(graphs.len(), 1);
    let g = &graphs[0];
    assert_eq!(g.label, "Tinted_surface");
    for channel in [ChannelKind::BaseColor, ChannelKind::Normal, ChannelKind::Roughness, ChannelKind::Metallic, ChannelKind::Height, ChannelKind::AmbientOcclusion] {
        assert!(g.channels.contains(&channel), "{channel:?}");
    }
    let tint = g.params.iter().find(|p| p.id == "tint").expect("tint");
    assert_eq!(tint.label, "Tint");
    assert_eq!(tint.group, "Color");
    assert_eq!(tint.kind, GraphParamKind::Color { alpha: true, linear: true });
    assert!(close(tint.default[0] as f32, 0.6) && close(tint.default[2] as f32, 0.1));
    assert!(g.params.iter().any(|p| p.id == "rough"));
}

#[test]
fn bad_files_are_errors() {
    assert!(open_backend(GraphEngine::MaterialCraft, b"garbage", "x.mcmat").is_err());
    assert!(open_backend(GraphEngine::MaterialCraft, b"", "x.sbs").is_err());
}

#[test]
fn import_render_edit_save_reopen() {
    let mut s = session();
    let mut graphs = GraphLayers::new();
    let path = Path::new(FIXTURE);
    graphs.import_file(&mut s, path).expect("import");
    let events = graphs.finish(&mut s);
    assert!(events.iter().all(|e| matches!(e, subcraft_app::graph_layer::GraphEvent::Applied(_))), "{events:?}");

    let layer = s.selected_layer().expect("new layer").clone();
    let material = layer.graph.as_deref().expect("graph material").clone();
    assert_eq!(material.engine, GraphEngine::MaterialCraft);
    let base = texel(&s, &layer, ChannelKind::BaseColor);
    assert!(close(base[0], 0.6) && close(base[1], 0.2) && close(base[2], 0.1), "linear tint survives sRGB encoding: {base:?}");
    assert!(close(texel(&s, &layer, ChannelKind::Roughness)[0], 0.5));
    let normal = texel(&s, &layer, ChannelKind::Normal);
    assert!(close(normal[1], 0.5) && close(normal[2], 1.0), "{normal:?}");

    // Move the sliders: blue tint, rougher.
    let mut edited = material.clone();
    edited.values.insert("tint".into(), vec![0.1, 0.3, 0.9, 1.0]);
    edited.values.insert("rough".into(), vec![0.8, 0.8, 0.8, 1.0]);
    graphs.request(&s, layer.id, edited).expect("request");
    graphs.finish(&mut s);
    let layer = s.selected_layer().expect("layer").clone();
    let base = texel(&s, &layer, ChannelKind::BaseColor);
    assert!(close(base[0], 0.1) && close(base[2], 0.9), "{base:?}");
    assert!(close(texel(&s, &layer, ChannelKind::Roughness)[0], 0.8));

    // Undo brings the first render back.
    s.execute(subcraft_app::Command::Undo).expect("undo");
    let undone = s.selected_layer().expect("layer").clone();
    assert!(close(texel(&s, &undone, ChannelKind::BaseColor)[0], 0.6));
    s.execute(subcraft_app::Command::Redo).expect("redo");

    // Save and reopen: the package and the edited values come back and render again.
    let file = std::env::temp_dir().join(format!("materialcraft-{}.subcraft", subcraft_core::ObjectId::new()));
    s.save(Some(&file)).expect("save");
    let mut reopened = Session::open_project(&file).expect("reopen");
    std::fs::remove_file(&file).ok();
    let layer = reopened.document().texture_sets.iter().find_map(|ts| ts.find(layer.id)).expect("layer saved").clone();
    let saved = layer.graph.as_deref().expect("graph material saved").clone();
    assert_eq!(saved.values["tint"], vec![0.1, 0.3, 0.9, 1.0]);
    let mut again = GraphLayers::new();
    let mut half = saved.clone();
    half.values.insert("rough".into(), vec![0.25, 0.25, 0.25, 1.0]);
    again.request(&reopened, layer.id, half).expect("request after reopen");
    again.finish(&mut reopened);
    let layer = reopened.document().texture_sets.iter().find_map(|ts| ts.find(layer.id)).expect("layer").clone();
    assert!(close(texel(&reopened, &layer, ChannelKind::Roughness)[0], 0.25));
}
