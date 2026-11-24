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

/// `fixtures/reads_mesh_maps.mcmat` (`mc_fixture -- <path> inputs`): roughness is the graph's
/// `curvature` input, base color its `ambientOcclusion` input.
const INPUT_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/reads_mesh_maps.mcmat");

fn import(s: &mut Session, graphs: &mut GraphLayers, path: &str) -> Layer {
    graphs.import_file(s, Path::new(path)).expect("import");
    let events = graphs.finish(s);
    assert!(events.iter().all(|e| matches!(e, subcraft_app::graph_layer::GraphEvent::Applied(_))), "{events:?}");
    s.selected_layer().expect("layer").clone()
}

fn bitmap_row(s: &Session, layer: &Layer, channel: ChannelKind) -> Vec<f32> {
    let LayerContent::Fill(f) = &layer.content else { panic!("not a fill layer") };
    let FillSource::Bitmap { resource } = f.channels[&channel].source else { panic!("{channel:?} is not a bitmap") };
    let bitmap = &s.resources().get(resource).expect("resource").bitmap;
    (0..bitmap.width).map(|x| bitmap.texel(x, 0, false)[0]).collect()
}

#[test]
fn baked_mesh_maps_feed_graph_inputs() {
    use subcraft_core::MeshMapKind;
    use subcraft_image::TiledImage;

    let mut s = session();
    let mut graphs = GraphLayers::new();
    let layer = import(&mut s, &mut graphs, INPUT_FIXTURE);
    // Before baking the inputs read each map's neutral value, like SubCraft's generators.
    let curvature = MeshMapKind::Curvature.neutral_value()[0];
    assert!(bitmap_row(&s, &layer, ChannelKind::Roughness).iter().all(|v| close(*v, curvature)), "unbaked curvature is neutral");
    assert!(close(texel(&s, &layer, ChannelKind::BaseColor)[0], MeshMapKind::AmbientOcclusion.neutral_value()[0]), "unbaked AO is neutral");

    // Bake: curvature is a left-to-right ramp, AO a flat 0.25.
    let ts = s.selection().texture_set;
    let res = 64u32;
    let mut curvature = TiledImage::new(res, res, 1, [0.0; 4]);
    for y in 0..res {
        for x in 0..res {
            curvature.set(x, y, [x as f32 / (res - 1) as f32, 0.0, 0.0, 0.0]);
        }
    }
    let maps = std::collections::BTreeMap::from([(MeshMapKind::Curvature, curvature), (MeshMapKind::AmbientOcclusion, TiledImage::new(res, res, 1, [0.25, 0.0, 0.0, 0.0]))]);
    s.set_mesh_maps(ts, maps);

    // Re-render the same layer (what a slider move does): it now reads the bakes.
    let material = layer.graph.as_deref().expect("graph material").clone();
    graphs.request(&s, layer.id, material).expect("request");
    graphs.finish(&mut s);
    let layer = s.selected_layer().expect("layer").clone();
    let row = bitmap_row(&s, &layer, ChannelKind::Roughness);
    let (first, last) = (row[0], row[row.len() - 1]);
    assert!(first < 0.05 && last > 0.95, "roughness follows the baked curvature: {first} .. {last}");
    let base = texel(&s, &layer, ChannelKind::BaseColor);
    assert!(close(base[0], 0.25) && close(base[1], 0.25), "AO feeds the color input as raw data: {base:?}");

    // A new import on the baked texture set reads them from the first render.
    let layer = import(&mut s, &mut graphs, INPUT_FIXTURE);
    assert!(bitmap_row(&s, &layer, ChannelKind::Roughness).last().copied().unwrap_or(0.0) > 0.95);
}

/// Two quads on UDIM tiles 1001 and 1002 of one material, "paint across UDIM tiles" on.
fn udim_strip() -> Session {
    use glam::{Vec2, Vec3};
    let mut b = subcraft_mesh::MeshBuilder::new("Strip");
    let m = b.material("Body");
    for (i, shift) in [Vec2::ZERO, Vec2::new(1.0, 0.0)].into_iter().enumerate() {
        let origin = Vec3::new(i as f32, 0.0, 0.0);
        let c = |x: f32, z: f32| subcraft_mesh::Corner { position: origin + Vec3::new(x, 0.0, z), normal: Some(Vec3::Y), uv: Some(shift + Vec2::new(x, z) * 0.9 + Vec2::splat(0.05)) };
        b.push_triangle(m, [c(0.0, 0.0), c(0.0, 1.0), c(1.0, 1.0)]);
        b.push_triangle(m, [c(0.0, 0.0), c(1.0, 1.0), c(1.0, 0.0)]);
    }
    let settings = subcraft_app::new_project::NewProjectSettings { resolution: 128, ..Default::default() };
    let mut s = Session::from_mesh_with(b.build().unwrap(), &settings).unwrap();
    let first = s.document().texture_sets[0].id;
    s.execute(subcraft_app::Command::SetPaintAcrossUdims { texture_set: first, on: true }).unwrap();
    s
}

/// Issue #581 for graph layers: the import lands on every UDIM tile, so moving a parameter must
/// re-render every tile's copy too, as one undo step (it only changed the selected tile).
#[test]
fn graph_parameters_drive_every_udim_tile() {
    let mut s = udim_strip();
    let mut graphs = GraphLayers::new();
    let layer = import(&mut s, &mut graphs, FIXTURE);
    let copies = |s: &Session| -> Vec<Layer> {
        s.document().texture_sets.iter().filter_map(|ts| ts.layers.iter().find(|l| l.graph.is_some()).map(|l| (**l).clone())).collect()
    };
    assert_eq!(copies(&s).len(), 2, "the import reached both tiles");
    let mut edited = layer.graph.as_deref().unwrap().clone();
    edited.values.insert("tint".into(), vec![0.1, 0.3, 0.9, 1.0]);
    graphs.request(&s, layer.id, edited.clone()).expect("request");
    graphs.finish(&mut s);
    for l in copies(&s) {
        let base = texel(&s, &l, ChannelKind::BaseColor);
        assert!(close(base[0], 0.1) && close(base[2], 0.9), "{}: {base:?}", l.name);
        assert_eq!(l.graph.as_deref().unwrap().values, edited.values, "the copy remembers the new values");
    }
    s.execute(subcraft_app::Command::Undo).expect("undo");
    for l in copies(&s) {
        assert!(close(texel(&s, &l, ChannelKind::BaseColor)[0], 0.6), "one undo step restores every tile");
    }
}
