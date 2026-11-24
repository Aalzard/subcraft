//! Graph layers through the session: importing a real `.sbsar` (see
//! `crates/sbsar/tests/data/README.md`), re-rendering with new values as one undo step, queued
//! requests, the composite following the images, and save / load.

use subcraft_app::graph_layer::{GraphEvent, GraphLayers, engine_for_file};
use subcraft_app::{Action, Session};
use subcraft_core::{ChannelKind, ResourceId};
use subcraft_doc::graph_material::current_images;
use subcraft_doc::{GraphEngine, Layer, ResourceKind};

const CARDBOARD: &[u8] = include_bytes!("../../sbsar/tests/data/CardBoard.sbsar");
const RES: u32 = 128;

fn graph_layer(s: &Session) -> Layer {
    s.document().texture_sets[0].layers.iter().find(|l| l.graph.is_some()).map(|l| (**l).clone()).expect("a graph layer")
}

fn imported() -> (Session, GraphLayers) {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), RES);
    let mut g = GraphLayers::new();
    let msg = g.import_bytes(&mut s, GraphEngine::Sbsar, CARDBOARD.to_vec(), "CardBoard.sbsar").unwrap();
    assert!(msg.contains("CardBoard"), "{msg}");
    assert!(g.busy());
    let events = g.finish(&mut s);
    assert_eq!(events, vec![GraphEvent::Applied("Added CardBoard".into())]);
    (s, g)
}

/// Mean base colour of the layer's rendered image (encoded values).
fn base_mean(s: &Session, layer: &Layer) -> [f32; 3] {
    let id = current_images(layer)[&ChannelKind::BaseColor];
    let b = &s.resources().get(id).unwrap().bitmap;
    let mut m = [0.0f32; 3];
    for y in 0..b.height {
        for x in 0..b.width {
            let t = b.texel(x, y, false);
            for c in 0..3 {
                m[c] += t[c];
            }
        }
    }
    m.map(|v| v / (b.width * b.height) as f32)
}

#[test]
fn files_are_recognized_by_extension() {
    assert_eq!(engine_for_file("a/B.SBSAR"), Some(GraphEngine::Sbsar));
    assert_eq!(engine_for_file("b.png"), None);
}

#[test]
fn an_archive_becomes_a_fill_layer_with_rendered_images() {
    let (s, _) = imported();
    let l = graph_layer(&s);
    assert_eq!(l.name, "CardBoard");
    let g = l.graph.as_deref().unwrap();
    assert_eq!(g.engine, GraphEngine::Sbsar);
    let package = s.resources().get(g.package).unwrap();
    assert_eq!(package.kind, ResourceKind::GraphPackage);
    assert_eq!(package.encoded.as_deref().map(|v| v.as_slice()), Some(CARDBOARD));
    let images = current_images(&l);
    for k in [ChannelKind::BaseColor, ChannelKind::Normal, ChannelKind::Roughness, ChannelKind::Metallic, ChannelKind::Height, ChannelKind::AmbientOcclusion] {
        assert!(images.contains_key(&k), "{k:?} filled");
    }
    // AO is not a default channel: it was added to the set with the layer.
    assert!(s.document().texture_sets[0].has_channel(ChannelKind::AmbientOcclusion));
    let b = &s.resources().get(images[&ChannelKind::BaseColor]).unwrap().bitmap;
    assert_eq!(b.width, 512, "the default render size");
    let m = base_mean(&s, &l);
    assert!((m[0] - 0.60).abs() < 0.05 && (m[2] - 0.34).abs() < 0.05, "brown cardboard: {m:?}");
}

#[test]
fn new_values_render_again_as_one_undo_step() {
    let (mut s, mut g) = imported();
    let l = graph_layer(&s);
    let before = current_images(&l);
    let mut m = l.graph.as_deref().unwrap().clone();
    m.values.insert("cardboard_color".into(), vec![0.2, 0.35, 0.6]);
    m.size_log2 = 6;
    g.request(&s, l.id, m.clone()).unwrap();
    assert!(g.is_rendering(l.id));
    assert_eq!(g.shown_material(&l).as_ref(), Some(&m), "the controls show the requested values at once");
    // A second change while the first renders waits; only the newest waiting one is rendered.
    let mut m2 = m.clone();
    m2.values.insert("tearing".into(), vec![1.0]);
    let mut m3 = m2.clone();
    m3.values.insert("tearing".into(), vec![0.5]);
    g.request(&s, l.id, m2).unwrap();
    g.request(&s, l.id, m3.clone()).unwrap();
    let events = g.finish(&mut s);
    assert_eq!(events.len(), 2, "first request and the newest waiting one: {events:?}");
    let after = graph_layer(&s);
    assert_eq!(after.id, l.id);
    assert_eq!(after.graph.as_deref(), Some(&m3));
    assert_ne!(current_images(&after), before);
    let mean = base_mean(&s, &after);
    assert!(mean[2] > mean[0] + 0.2, "blue now: {mean:?}");
    // Undo twice: back to the imported layer, images and values.
    s.perform(Action::Undo).unwrap();
    s.perform(Action::Undo).unwrap();
    let undone = graph_layer(&s);
    assert_eq!(current_images(&undone), before);
    assert!(undone.graph.as_deref().unwrap().values.is_empty());
}

#[test]
fn a_deleted_layer_drops_its_render() {
    let (mut s, mut g) = imported();
    let l = graph_layer(&s);
    let mut m = l.graph.as_deref().unwrap().clone();
    m.size_log2 = 5;
    g.request(&s, l.id, m).unwrap();
    s.execute(subcraft_app::Command::DeleteLayer(l.id)).unwrap();
    let events = g.finish(&mut s);
    assert!(matches!(&events[..], [GraphEvent::Failed(e)] if e.contains("deleted")), "{events:?}");
}

#[test]
fn graph_layers_survive_save_and_load_and_render_again() {
    let (mut s, _) = imported();
    let dir = std::env::temp_dir().join(format!("subcraft-graph-layer-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("graph.subcraft");
    s.save(Some(&path)).unwrap();
    let before = graph_layer(&s);
    let mut loaded = Session::open_project(&path).unwrap();
    let after = graph_layer(&loaded);
    assert_eq!(after.graph, before.graph);
    assert_eq!(after.content, before.content);
    let package: ResourceId = after.graph.as_deref().unwrap().package;
    let r = loaded.resources().get(package).unwrap();
    assert_eq!((r.kind, r.encoded.as_deref().map(|v| v.as_slice())), (ResourceKind::GraphPackage, Some(CARDBOARD)));
    // The images came back pixel for pixel (saved losslessly).
    for (k, id) in current_images(&before) {
        assert_eq!(*loaded.resources().get(id).unwrap().bitmap, *s.resources().get(id).unwrap().bitmap, "{k:?}");
    }
    // A fresh GraphLayers (a new app run) opens the saved package and renders again.
    let mut g = GraphLayers::new();
    let mut m = after.graph.as_deref().unwrap().clone();
    m.size_log2 = 5;
    g.request(&loaded, after.id, m).unwrap();
    assert!(matches!(&g.finish(&mut loaded)[..], [GraphEvent::Applied(_)]));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_that_is_not_an_archive_is_refused_without_adding_anything() {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), RES);
    let resources = s.resources().iter().count();
    let mut g = GraphLayers::new();
    let e = g.import_bytes(&mut s, GraphEngine::Sbsar, b"not an archive".to_vec(), "x.sbsar").unwrap_err();
    assert!(e.starts_with("x.sbsar:"), "{e}");
    assert_eq!(s.resources().iter().count(), resources);
    assert!(!g.busy());
}
