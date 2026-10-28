//! Rules of the Painter stack rebuild on hand-made layer descriptions (no Painter file needed
//! beyond the synthetic container the rebuilder reads embedded images from).

#[path = "../../mesh/tests/support/spp_fixture.rs"]
mod fixture;

use std::io::Cursor;

use subcraft_app::spp_stack::Rebuilder;
use subcraft_core::ChannelKind;
use subcraft_doc::{BlendMode, ContentEffectKind, Filter, LayerContent, MaskEffectKind, ResourceStore, TextureSet};
use subcraft_mesh::spp::document::{ActionInfo, ActionKind, GraphRef, LayerInfo, LayerKind, SourceInfo};
use subcraft_mesh::spp::{SppFile, channel_bits};

fn action(kind: ActionKind, own: Vec<(u64, i32, f32)>) -> ActionInfo {
    ActionInfo { kind, enabled: true, label: String::new(), channels: u64::MAX, blends: vec![(u64::MAX, 25, 1.0)], group: 1, own_blends: own, uv: None }
}

fn layer(name: &str, content: Vec<ActionInfo>, mask: Option<(Vec<ActionInfo>, bool)>) -> LayerInfo {
    LayerInfo {
        name: name.into(),
        kind: LayerKind::Fill,
        visible: true,
        blends: Vec::new(),
        has_mask: mask.is_some(),
        sources: Vec::new(),
        strokes: 0,
        content,
        mask: mask.as_ref().map(|m| m.0.clone()).unwrap_or_default(),
        mask_enabled: true,
        mask_initial_white: mask.is_some_and(|m| m.1),
        children: Vec::new(),
    }
}

fn uniform(bits: u64, v: f32) -> ActionInfo {
    action(ActionKind::Fill { sources: vec![SourceInfo::Uniform { channels: bits, color: [v; 3] }], projection: 0 }, Vec::new())
}

fn graph(name: &str, tweaks: &[(&str, f32)]) -> GraphRef {
    GraphRef { url: format!("resource://starter_assets/{name}/{name}?version=0.sbsar"), tweaks: tweaks.iter().map(|(k, v)| (k.to_string(), vec![*v])).collect() }
}

#[test]
fn rebuild_rules() {
    let mut spp = SppFile::from_reader(Cursor::new(fixture::project_file())).unwrap();
    let ts = TextureSet::new("T", 0, 64);
    let mut resources = ResourceStore::default();
    let mut r = Rebuilder::new(&mut spp, &ts, &mut resources);
    let infos = vec![
        // An empty mask that starts black hides the layer (it is not "left off").
        layer("Hidden", vec![uniform(channel_bits::BASE_COLOR, 0.9)], Some((Vec::new(), false))),
        // A mask fill keeps its own blending (id 4 is Multiply).
        layer("Masked", vec![uniform(channel_bits::ROUGHNESS, 0.2)], Some((vec![uniform(0, 1.0), action(ActionKind::Fill { sources: vec![SourceInfo::Uniform { channels: 0, color: [0.5; 3] }], projection: 0 }, vec![(u64::MAX, 4, 0.8)])], false))),
        // A layer that only filters is an empty pass-through folder carrying the filter.
        layer("Sharpen", vec![action(ActionKind::Filter { graph: graph("sharpen", &[("intensity", 0.3)]) }, vec![(u64::MAX, 1, 1.0)])], None),
        // A procedural material fill becomes a uniform fill whose values are fitted later.
        layer("Plastic", vec![action(ActionKind::Fill { sources: vec![SourceInfo::Procedural { channels: channel_bits::BASE_COLOR | channel_bits::ROUGHNESS | channel_bits::NORMAL, graph: graph("Plastic", &[]) }], projection: 2 }, Vec::new())], None),
    ];
    let layers = r.rebuild(&infos);
    let fitted = r.fitted.clone();

    let hidden = layers[0].mask.as_ref().expect("the empty mask is kept");
    assert!(hidden.effects.is_empty() && hidden.base.get(3, 3)[0] == 0.0);

    let masked = layers[1].mask.as_ref().unwrap();
    assert_eq!(masked.effects.len(), 2);
    assert_eq!((masked.effects[0].blend, masked.effects[1].blend, masked.effects[1].opacity), (BlendMode::Normal, BlendMode::Multiply, 0.8));
    assert!(matches!(masked.effects[1].kind, MaskEffectKind::Fill { value, .. } if value == 0.5));

    let sharpen = &layers[2];
    assert!(matches!(sharpen.content, LayerContent::Group(ref g) if g.children.is_empty()));
    assert!(sharpen.blend.values().all(|b| b.mode == BlendMode::Passthrough));
    assert!(!sharpen.content_effects.is_empty());
    assert!(sharpen.content_effects.iter().all(|e| matches!(e.kind, ContentEffectKind::Filter { filter: Filter::Sharpen { .. }, channel } if channel != ChannelKind::Normal)));

    let plastic = &layers[3];
    let LayerContent::Fill(fill) = &plastic.content else { panic!("a fill layer") };
    assert!(fill.channels.contains_key(&ChannelKind::BaseColor) && fill.channels.contains_key(&ChannelKind::Roughness));
    assert!(!fill.channels.contains_key(&ChannelKind::Normal), "the material's normal detail is dropped");
    assert_eq!(fitted, vec![(plastic.id, vec![ChannelKind::BaseColor, ChannelKind::Roughness])]);
}
