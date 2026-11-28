//! Substance generators (Painter's custom generators): an `.sbsar` with a mask output becomes an
//! image mask effect that keeps its graph, renders from the baked mesh maps, re-renders on a new
//! bake and on new values (one undo step each), and survives save / load.

use std::collections::BTreeMap;

use subcraft_app::graph_layer::{GraphEvent, GraphLayers, GraphTarget};
use subcraft_app::Session;
use subcraft_core::{LayerId, MeshMapKind};
use subcraft_doc::{GraphEngine, MaskEffect, MaskEffectKind};
use subcraft_image::TiledImage;

const GENERATOR: &[u8] = include_bytes!("../../sbsar/tests/data/EdgeGenerator.sbsar");
const CARDBOARD: &[u8] = include_bytes!("../../sbsar/tests/data/CardBoard.sbsar");
const RES: u32 = 128;

fn first_layer(s: &Session) -> LayerId {
    s.document().texture_sets[0].layers[0].id
}

fn generator(s: &Session, layer: LayerId) -> MaskEffect {
    let l = s.document().texture_sets[0].find(layer).unwrap();
    (**l.mask.as_ref().expect("a mask").effects.iter().find(|e| e.graph.is_some()).expect("a generator")).clone()
}

/// The generator image's row through the middle (one channel).
fn mask_row(s: &Session, e: &MaskEffect) -> Vec<f32> {
    let MaskEffectKind::Image { resource, .. } = e.kind else { panic!("an image effect") };
    let b = &s.resources().get(resource).unwrap().bitmap;
    assert_eq!(b.channels, 1);
    (0..b.width).map(|x| b.texel(x, b.height / 2, false)[0]).collect()
}

fn curvature_ramp() -> TiledImage {
    let mut c = TiledImage::new(RES, RES, 1, [0.0; 4]);
    for y in 0..RES {
        for x in 0..RES {
            c.set(x, y, [x as f32 / (RES - 1) as f32, 0.0, 0.0, 0.0]);
        }
    }
    c
}

#[test]
fn a_generator_archive_becomes_a_mask_effect_that_follows_the_bake() {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), RES);
    let layer = first_layer(&s);
    let mut g = GraphLayers::new();
    assert_eq!(g.refresh_baked(&s), Ok(0));
    let msg = g.import_generator_bytes(&mut s, layer, GraphEngine::Sbsar, GENERATOR.to_vec(), "EdgeGenerator.sbsar").unwrap();
    assert!(msg.contains("Edge Generator"), "{msg}");
    assert_eq!(g.finish(&mut s), vec![GraphEvent::Applied("Added generator Edge Generator".into())]);
    let e = generator(&s, layer);
    assert_eq!(e.name, "Edge Generator");
    assert!(matches!(e.kind, MaskEffectKind::Image { .. }));
    // Not baked yet: flat curvature (0.5) is below the edge levels, so the mask is black.
    assert!(mask_row(&s, &e).iter().all(|v| *v < 0.05), "{:?}", mask_row(&s, &e));

    // A bake with curvature re-renders it with no control touched.
    let ts = s.selection().texture_set;
    s.set_mesh_maps(ts, BTreeMap::from([(MeshMapKind::Curvature, curvature_ramp())]));
    assert_eq!(g.refresh_baked(&s), Ok(1));
    assert!(matches!(g.finish(&mut s).as_slice(), [GraphEvent::Applied(_)]));
    let after = generator(&s, layer);
    assert_eq!((after.id, after.graph.clone()), (e.id, e.graph.clone()), "same effect, same values");
    let row = mask_row(&s, &after);
    assert!(row[0] < 0.05 && row[row.len() - 1] > 0.95, "edges are white after the bake: {row:?}");

    // New values: one render, one undo step that brings the old image back.
    let mut m = after.graph.as_deref().unwrap().clone();
    m.size_log2 = 5;
    let target = GraphTarget::Mask { layer, effect: e.id };
    g.request_target(&s, target, m.clone()).unwrap();
    assert!(g.is_rendering_target(target));
    assert_eq!(g.shown_for(target, after.graph.as_deref()), Some(m.clone()), "controls show the waiting values");
    assert!(matches!(g.finish(&mut s).as_slice(), [GraphEvent::Applied(_)]));
    assert_eq!(generator(&s, layer).graph.as_deref(), Some(&m));
    s.perform(subcraft_app::Action::Undo).unwrap();
    assert_eq!(generator(&s, layer).graph, after.graph);
}

#[test]
fn generators_survive_save_and_load() {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), RES);
    let layer = first_layer(&s);
    let mut g = GraphLayers::new();
    g.import_generator_bytes(&mut s, layer, GraphEngine::Sbsar, GENERATOR.to_vec(), "EdgeGenerator.sbsar").unwrap();
    g.finish(&mut s);
    let before = generator(&s, layer);
    let dir = std::env::temp_dir().join(format!("subcraft-sbsar-generator-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("gen.subcraft");
    s.save(Some(&path)).unwrap();
    let mut loaded = Session::open_project(&path).unwrap();
    let after = generator(&loaded, layer);
    assert_eq!(after, before, "graph, image and settings come back");
    let package = after.graph.as_deref().unwrap().package;
    assert_eq!(loaded.resources().get(package).unwrap().encoded.as_deref().map(|v| v.as_slice()), Some(GENERATOR));
    // A new app run renders it again from the saved package.
    let mut g = GraphLayers::new();
    let mut m = after.graph.as_deref().unwrap().clone();
    m.size_log2 = 6;
    g.request_target(&loaded, GraphTarget::Mask { layer, effect: after.id }, m).unwrap();
    assert!(matches!(g.finish(&mut loaded).as_slice(), [GraphEvent::Applied(_)]));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn materials_and_generators_go_where_they_belong() {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), RES);
    let layer = first_layer(&s);
    let mut g = GraphLayers::new();
    let e = g.import_generator_bytes(&mut s, layer, GraphEngine::Sbsar, CARDBOARD.to_vec(), "CardBoard.sbsar").unwrap_err();
    assert!(e.contains("mask output"), "{e}");
    let e = g.import_bytes(&mut s, GraphEngine::Sbsar, GENERATOR.to_vec(), "EdgeGenerator.sbsar").unwrap_err();
    assert!(e.contains("generator"), "{e}");
    assert!(!g.busy());
}

#[test]
fn generator_files_are_told_apart_from_materials() {
    let dir = std::env::temp_dir().join(format!("subcraft-sbsar-kind-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (generator, mat, junk) = (dir.join("g.sbsar"), dir.join("m.sbsar"), dir.join("j.sbsar"));
    std::fs::write(&generator, GENERATOR).unwrap();
    std::fs::write(&mat, CARDBOARD).unwrap();
    std::fs::write(&junk, b"nope").unwrap();
    assert!(subcraft_app::graph_layer::is_generator_file(&generator));
    assert!(!subcraft_app::graph_layer::is_generator_file(&mat));
    assert!(!subcraft_app::graph_layer::is_generator_file(&junk));
    assert!(!subcraft_app::graph_layer::is_generator_file(&dir.join("missing.sbsar")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_smart_mask_keeps_its_generator_live() {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), RES);
    let layer = first_layer(&s);
    let mut g = GraphLayers::new();
    g.import_generator_bytes(&mut s, layer, GraphEngine::Sbsar, GENERATOR.to_vec(), "EdgeGenerator.sbsar").unwrap();
    g.finish(&mut s);
    let before = generator(&s, layer);
    let dir = std::env::temp_dir().join(format!("subcraft-sbsar-smart-mask-test-{}", std::process::id()));
    let lib = subcraft_app::assets::UserLibrary::new(&dir);
    let saved = s.save_smart_mask(layer, &lib).unwrap();
    // Read back from the file: the graph, its name and its package come along.
    let mask = subcraft_app::library::load_smart_mask_file(saved.user_file.as_deref().unwrap()).unwrap();
    assert!(mask.has_graphs());
    let package = before.graph.as_deref().unwrap().package;
    assert!(mask.resources.iter().any(|r| r.id == package), "the package is in the smart mask file");

    // Applied to another layer of a new project: a live generator again.
    let mut other = Session::new(subcraft_mesh::primitives::cube(2.0), RES);
    let target_layer = first_layer(&other);
    other.execute(subcraft_app::Command::SelectLayer(Some(target_layer))).unwrap();
    other.apply_smart_mask(&mask).unwrap();
    let applied = generator(&other, target_layer);
    assert_eq!((applied.name.as_str(), applied.graph.clone()), ("Edge Generator", before.graph.clone()));
    // It re-renders after a bake, from the package the smart mask brought.
    let mut g = GraphLayers::new();
    assert_eq!(g.refresh_baked(&other), Ok(0));
    let ts = other.selection().texture_set;
    other.set_mesh_maps(ts, BTreeMap::from([(MeshMapKind::Curvature, curvature_ramp())]));
    assert_eq!(g.refresh_baked(&other), Ok(1));
    assert!(matches!(g.finish(&mut other).as_slice(), [GraphEvent::Applied(_)]));
    let row = mask_row(&other, &generator(&other, target_layer));
    assert!(row[row.len() - 1] > 0.95, "edges are white after the bake: {row:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Two quads on UDIM tiles 1001 and 1002 (two texture sets of one group).
fn udim_mesh() -> subcraft_mesh::Mesh {
    use glam::{Vec2, Vec3};
    use subcraft_mesh::{Corner, MeshBuilder};
    let mut b = MeshBuilder::new("Strip");
    let m = b.material("Body");
    for (i, shift) in [Vec2::ZERO, Vec2::new(1.0, 0.0)].into_iter().enumerate() {
        let origin = Vec3::new(i as f32, 0.0, 0.0);
        let c = |x: f32, z: f32| Corner { position: origin + Vec3::new(x, 0.0, z), normal: Some(Vec3::Y), uv: Some(shift + Vec2::new(x, z) * 0.9 + Vec2::splat(0.05)) };
        b.push_triangle(m, [c(0.0, 0.0), c(0.0, 1.0), c(1.0, 1.0)]);
        b.push_triangle(m, [c(0.0, 0.0), c(1.0, 1.0), c(1.0, 0.0)]);
    }
    b.build().unwrap()
}

#[test]
fn each_udim_tile_renders_its_own_generator() {
    use subcraft_app::new_project::NewProjectSettings;
    use subcraft_app::{Command, NewLayer};
    let mut s = Session::from_mesh_with(udim_mesh(), &NewProjectSettings { resolution: RES, ..Default::default() }).unwrap();
    let (t0, t1) = (s.document().texture_sets[0].id, s.document().texture_sets[1].id);
    s.execute(Command::SetPaintAcrossUdims { texture_set: t0, on: true }).unwrap();
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = s.selection().layer.unwrap();
    // Tile 1001: a curvature ramp (edges only on its right side); tile 1002: all edges.
    s.set_mesh_maps(t0, BTreeMap::from([(MeshMapKind::Curvature, curvature_ramp())]));
    s.set_mesh_maps(t1, BTreeMap::from([(MeshMapKind::Curvature, TiledImage::new(RES, RES, 1, [1.0, 0.0, 0.0, 0.0]))]));
    let partner = s.document().texture_sets[1].layers.last().unwrap().id;
    assert_ne!(partner, layer);

    let mut g = GraphLayers::new();
    g.refresh_baked(&s).unwrap();
    g.import_generator_bytes(&mut s, layer, GraphEngine::Sbsar, GENERATOR.to_vec(), "EdgeGenerator.sbsar").unwrap();
    g.finish(&mut s);
    let gen_of = |s: &Session, set: usize, l: LayerId| {
        let l = s.document().texture_sets[set].find(l).unwrap();
        (**l.mask.as_ref().unwrap().effects.iter().find(|e| e.graph.is_some()).expect("a generator on this tile")).clone()
    };
    let check = |s: &Session| {
        let (a, b) = (gen_of(s, 0, layer), gen_of(s, 1, partner));
        assert_eq!(a.graph, b.graph, "same settings on both tiles");
        let (ra, rb) = (mask_row(s, &a), mask_row(s, &b));
        assert!(ra[0] < 0.05, "tile 1001 is black where its curvature is flat: {ra:?}");
        assert!(rb[0] > 0.95, "tile 1002 renders from its own maps, not tile 1001's: {rb:?}");
    };
    check(&s);

    // New values on tile 1001: tile 1002 takes them and renders again from its own maps.
    let e = gen_of(&s, 0, layer);
    let mut m = e.graph.as_deref().unwrap().clone();
    m.size_log2 = 6;
    g.request_target(&s, GraphTarget::Mask { layer, effect: e.id }, m.clone()).unwrap();
    g.finish(&mut s);
    assert_eq!(gen_of(&s, 1, partner).graph.as_deref(), Some(&m));
    check(&s);
}
