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
