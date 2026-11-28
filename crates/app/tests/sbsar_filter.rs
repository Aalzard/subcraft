//! Substance filters (Painter's custom filters): an `.sbsar` whose image inputs read the channels
//! it outputs goes on top of a layer's content, renders from the layer stack below it (never from
//! the layers above), re-renders on demand and on new values, undoes in one step, and survives
//! save / load.

use std::sync::Arc;

use subcraft_app::graph_layer::{GraphEvent, GraphLayers, GraphTarget};
use subcraft_app::{Command, Session};
use subcraft_core::{ChannelKind, ChannelValue, LayerId};
use subcraft_core::color::{linear_to_srgb, srgb_to_linear};
use subcraft_doc::{ContentEffect, FillSource, GraphEngine, Layer, LayerContent};

/// Inverts base color (in its encoded values) and roughness; see `crates/sbsar/tests/data/README.md`.
const FILTER: &[u8] = include_bytes!("../../sbsar/tests/data/InvertFilter.sbsar");
const CARDBOARD: &[u8] = include_bytes!("../../sbsar/tests/data/CardBoard.sbsar");
const RES: u32 = 128;

/// Add a fill layer on top with uniform base color (linear) and roughness.
fn add_fill(s: &mut Session, name: &str, base: [f32; 3], rough: f32) -> LayerId {
    let ts = s.document().texture_sets[0].clone();
    let mut l = Layer::new_fill(name, &ts);
    if let LayerContent::Fill(f) = &mut l.content {
        for (k, c) in f.channels.iter_mut() {
            match k {
                ChannelKind::BaseColor => c.source = FillSource::uniform(ChannelValue::rgb(base[0], base[1], base[2])),
                ChannelKind::Roughness => c.source = FillSource::uniform(ChannelValue::scalar(rough)),
                _ => c.enabled = false,
            }
        }
    }
    let id = l.id;
    s.execute(Command::InsertLayer { layer: Box::new(l), label: "Add".into() }).unwrap();
    id
}

fn set_base(s: &mut Session, layer: LayerId, base: [f32; 3]) {
    let mut l = s.document().texture_sets[0].find(layer).unwrap().clone();
    if let LayerContent::Fill(f) = &mut l.content {
        f.channels.get_mut(&ChannelKind::BaseColor).unwrap().source = FillSource::uniform(ChannelValue::rgb(base[0], base[1], base[2]));
    }
    s.execute(Command::ReplaceLayer { layer, replacement: Box::new(l), label: "Color".into() }).unwrap();
}

fn filter(s: &Session, layer: LayerId) -> Option<Arc<ContentEffect>> {
    s.document().texture_sets[0].find(layer).unwrap().content_effects.iter().find(|e| e.graph.is_some()).cloned()
}

/// The value most texels of `kind` hold in the stack up to and including `layer` (its whole
/// content), rounded to 1/100.
fn common(s: &Session, layer: LayerId, kind: ChannelKind) -> [f32; 3] {
    let c = s.composite_below(layer, None).unwrap();
    let img = &c.channels[&kind];
    let mut counts = std::collections::HashMap::new();
    for y in 0..img.height() {
        for x in 0..img.width() {
            let v = img.get(x, y);
            *counts.entry([0, 1, 2].map(|i| (v[i] * 100.0).round() as i32)).or_insert(0) += 1;
        }
    }
    let best = counts.into_iter().max_by_key(|(_, n)| *n).unwrap().0;
    best.map(|v| v as f32 / 100.0)
}

/// What the filter makes of a linear base color: inverted in sRGB, back to linear.
fn inverted(c: f32) -> f32 {
    srgb_to_linear(1.0 - linear_to_srgb(c))
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= 0.02
}

#[test]
fn a_filter_renders_from_the_stack_below_and_not_from_above() {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), RES);
    let base = [0.5, 0.2, 0.05];
    let bottom = add_fill(&mut s, "Bottom", base, 0.25);
    let host = add_fill(&mut s, "Host", [0.0; 3], 0.0);
    // The host fill sets nothing, so the filter reads the bottom layer through it.
    let mut l = s.document().texture_sets[0].find(host).unwrap().clone();
    if let LayerContent::Fill(f) = &mut l.content {
        f.channels.values_mut().for_each(|c| c.enabled = false);
    }
    s.execute(Command::ReplaceLayer { layer: host, replacement: Box::new(l), label: "Empty".into() }).unwrap();
    // A layer above the host must not reach the filter.
    add_fill(&mut s, "Top", [1.0, 1.0, 1.0], 0.9);
    assert!(close(common(&s, host, ChannelKind::Roughness)[0], 0.25));

    let mut g = GraphLayers::new();
    let msg = g.import_filter_bytes(&mut s, host, GraphEngine::Sbsar, FILTER.to_vec(), "InvertFilter.sbsar").unwrap();
    assert!(msg.starts_with("Rendering filter"), "{msg}");
    let events = g.finish(&mut s);
    assert!(matches!(events.as_slice(), [GraphEvent::Applied(t)] if t.starts_with("Added filter")), "{events:?}");
    let e = filter(&s, host).expect("the filter is a content effect");
    assert!(matches!(e.kind, subcraft_doc::ContentEffectKind::Fill(_)));
    let rough = common(&s, host, ChannelKind::Roughness)[0];
    assert!(close(rough, 0.75), "roughness 0.25 inverted, the top layer's 0.9 unseen: {rough}");
    let color = common(&s, host, ChannelKind::BaseColor);
    for (got, c) in color.iter().zip(base) {
        assert!(close(*got, inverted(c)), "base color inverted in sRGB: {color:?} for {base:?}");
    }

    // Changing the layers below does nothing until the filter is updated from them.
    set_base(&mut s, bottom, [0.05, 0.2, 0.5]);
    assert!(close(common(&s, host, ChannelKind::BaseColor)[0], inverted(0.5)), "the filter keeps its render");
    let material = e.graph.as_deref().unwrap().clone();
    g.update_filter_input(&s, host, e.id, material.clone()).unwrap();
    assert!(g.is_rendering_target(GraphTarget::Content { layer: host, effect: e.id }));
    assert!(matches!(g.finish(&mut s).as_slice(), [GraphEvent::Applied(_)]));
    let color = common(&s, host, ChannelKind::BaseColor);
    assert!(close(color[0], inverted(0.05)) && close(color[2], inverted(0.5)), "follows the new color below: {color:?}");
    assert_eq!(filter(&s, host).unwrap().id, e.id, "the same effect is updated");

    // Undo brings the previous render back; undoing the add removes the filter.
    s.perform(subcraft_app::Action::Undo).unwrap();
    assert!(close(common(&s, host, ChannelKind::BaseColor)[0], inverted(0.5)));
    s.perform(subcraft_app::Action::Undo).unwrap(); // the color change
    s.perform(subcraft_app::Action::Undo).unwrap(); // the filter
    assert!(filter(&s, host).is_none());
}

#[test]
fn filters_survive_save_and_load_and_are_kept_out_of_materials() {
    let mut s = Session::new(subcraft_mesh::primitives::cube(2.0), RES);
    let layer = add_fill(&mut s, "Base", [0.3, 0.3, 0.3], 0.4);
    let mut g = GraphLayers::new();
    g.import_filter_bytes(&mut s, layer, GraphEngine::Sbsar, FILTER.to_vec(), "InvertFilter.sbsar").unwrap();
    g.finish(&mut s);
    let before = filter(&s, layer).unwrap();
    let dir = std::env::temp_dir().join(format!("subcraft-sbsar-filter-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("filter.subcraft");
    s.save(Some(&path)).unwrap();
    let mut loaded = Session::open_project(&path).unwrap();
    let after = filter(&loaded, layer).expect("the filter comes back");
    assert_eq!(after, before, "graph, images and blending come back");
    // A new app run renders it again from the saved package and the stack below.
    let mut g = GraphLayers::new();
    let mut m = after.graph.as_deref().unwrap().clone();
    m.values.insert("unused".into(), vec![1.0]);
    g.request_target(&loaded, GraphTarget::Content { layer, effect: after.id }, m.clone()).unwrap();
    assert!(matches!(g.finish(&mut loaded).as_slice(), [GraphEvent::Applied(_)]));
    assert_eq!(filter(&loaded, layer).unwrap().graph.as_deref(), Some(&m));
    assert!(close(common(&loaded, layer, ChannelKind::Roughness)[0], 0.6));
    let _ = std::fs::remove_dir_all(&dir);

    // A filter is not a material layer, and a material is not a filter.
    let err = g.import_bytes(&mut loaded, GraphEngine::Sbsar, FILTER.to_vec(), "InvertFilter.sbsar").unwrap_err();
    assert!(err.contains("filter"), "{err}");
    let err = g.import_filter_bytes(&mut loaded, layer, GraphEngine::Sbsar, CARDBOARD.to_vec(), "CardBoard.sbsar").unwrap_err();
    assert!(err.contains("not a filter"), "{err}");
}
