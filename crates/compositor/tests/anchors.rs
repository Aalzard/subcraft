//! Anchor points (ADR-0045): captures of a layer's content or mask that other layers read.
//! Numeric checks of what references read, evaluation order, missing / misplaced references,
//! hidden anchored layers, folders, generator inputs and incremental refresh after an edit.

use std::collections::BTreeMap;
use std::sync::Arc;

use subcraft_compositor::{CompositeCache, EvalContext, composite_full};
use subcraft_core::{ChannelKind, ChannelValue, EffectId, LayerId, MeshMapKind};
use subcraft_doc::{
    AnchorInput, ContentEffect, ContentEffectKind, FillChannel, FillContent, FillSource, Filter, Generator, GeneratorKind, Layer,
    LayerContent, Levels, Mask, MaskEffect, MaskEffectKind, Projection, ResourceStore, TextureSet,
};
use subcraft_materials::{MeshMaps, TextureSetGeometry};
use subcraft_mesh::primitives;

const RES: u32 = 128;
const H: ChannelKind = ChannelKind::Height;
const R: ChannelKind = ChannelKind::Roughness;
const C: ChannelKind = ChannelKind::BaseColor;

struct Fixture {
    geo: TextureSetGeometry,
    res: ResourceStore,
    maps: MeshMaps,
}

impl Fixture {
    fn plane() -> Self {
        Self { geo: TextureSetGeometry::new(Arc::new(primitives::plane(2.0, 2)), 0, RES), res: ResourceStore::default(), maps: MeshMaps::new() }
    }
    fn ctx(&self) -> EvalContext<'_> {
        EvalContext { geometry: &self.geo, resources: &self.res, mesh_maps: &self.maps }
    }
}

fn get(ts: &TextureSet, fx: &Fixture, kind: ChannelKind, x: u32, y: u32) -> [f32; 4] {
    composite_full(ts, &fx.ctx()).channel(kind).unwrap().get(x, y)
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 2e-3
}

fn set() -> TextureSet {
    let mut ts = TextureSet::new("T", 0, RES);
    ts.channels.retain(|c| matches!(c.kind, ChannelKind::BaseColor | ChannelKind::Height | ChannelKind::Roughness));
    ts
}

/// A paint layer with height 0.8 on the left half (x < 64) and a content anchor on top.
fn bumps(ts: &TextureSet) -> (Layer, EffectId) {
    let mut l = Layer::new_paint("Bumps", ts);
    let LayerContent::Paint(p) = &mut l.content else { unreachable!() };
    let img = p.channel_mut(H, RES);
    for y in 0..RES {
        for x in 0..64 {
            img.set(x, y, [0.8, 1.0, 0.0, 0.0]);
        }
    }
    let mut a = ContentEffect::new(ContentEffectKind::Anchor, ts);
    a.name = "Bumps anchor".into();
    let id = a.id;
    l.content_effects.push(Arc::new(a));
    (l, id)
}

/// A red fill (roughness 0.9) behind a black mask that an Anchor Reference reveals.
fn dirt(ts: &TextureSet, input: AnchorInput) -> Layer {
    let mut l = Layer::new_fill("Dirt", ts);
    l.content = LayerContent::Fill(FillContent {
        projection: Projection::default(),
        channels: BTreeMap::from([
            (C, FillChannel::new(FillSource::uniform(ChannelValue::rgb(1.0, 0.0, 0.0)))),
            (R, FillChannel::new(FillSource::uniform(ChannelValue::scalar(0.9)))),
        ]),
    });
    let mut m = Mask::new(RES, false);
    m.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::AnchorRef { input, levels: None })));
    l.mask = Some(m);
    l
}

fn base_color_default() -> [f32; 3] {
    let d = C.default_value().0;
    [d[0], d[1], d[2]]
}

fn mixed(t: f32) -> [f32; 3] {
    let d = base_color_default();
    [d[0] + (1.0 - d[0]) * t, d[1] * (1.0 - t), d[2] * (1.0 - t)]
}

fn assert_color(p: [f32; 4], want: [f32; 3], what: &str) {
    assert!(close(p[0], want[0]) && close(p[1], want[1]) && close(p[2], want[2]), "{what}: {p:?} vs {want:?}");
}

/// The mask reads the anchored height (0.8 on the left, nothing on the right), even when the
/// anchored layer is hidden; without the reference the black mask hides the fill.
#[test]
fn mask_driven_by_an_anchor_reads_the_lower_layers_channel() {
    let fx = Fixture::plane();
    let mut ts = set();
    let tc = ts.clone();
    let (b, anchor) = bumps(&tc);
    let bumps_id = ts.insert_above(None, b);
    ts.insert_above(None, dirt(&tc, AnchorInput::new(anchor, H)));
    assert_color(get(&ts, &fx, C, 10, 10), mixed(0.8), "left: mask = anchored height 0.8");
    assert_color(get(&ts, &fx, C, 100, 10), base_color_default(), "right: unpainted height, mask 0");
    assert!(close(get(&ts, &fx, R, 10, 10)[0], R.default_value().0[0] * 0.2 + 0.9 * 0.8));
    assert!(close(get(&ts, &fx, H, 10, 10)[0], 0.8), "the anchored layer itself still shows");
    // Hidden anchored layer: no height in the composite, but the anchor still feeds the mask.
    ts.find_mut(bumps_id).unwrap().visible = false;
    assert!(close(get(&ts, &fx, H, 10, 10)[0], H.default_value().0[0]));
    assert_color(get(&ts, &fx, C, 10, 10), mixed(0.8), "hidden anchored layers still provide their anchor");
}

/// A fill channel reading an anchor takes its value and coverage; a color anchor read into a
/// scalar channel converts by luminance.
#[test]
fn fill_channels_read_anchors_with_coverage_and_conversion() {
    let fx = Fixture::plane();
    let mut ts = set();
    let tc = ts.clone();
    let (b, anchor) = bumps(&tc);
    ts.insert_above(None, b);
    let mut reader = Layer::new_fill("Reader", &tc);
    reader.content = LayerContent::Fill(FillContent {
        projection: Projection::default(),
        channels: BTreeMap::from([(R, FillChannel::new(FillSource::Anchor { input: AnchorInput::new(anchor, H) }))]),
    });
    for (k, b) in reader.blend.iter_mut() {
        if *k != R {
            b.mode = subcraft_doc::BlendMode::Disable;
        }
    }
    ts.insert_above(None, reader);
    assert!(close(get(&ts, &fx, R, 10, 10)[0], 0.8), "painted: the anchored value");
    assert!(close(get(&ts, &fx, R, 100, 10)[0], R.default_value().0[0]), "unpainted: transparent, the stack below shows");

    // Color anchor -> roughness: Rec. 709 luminance.
    let mut ts = set();
    let mut colored = Layer::new_fill("Colored", &tc);
    colored.content = LayerContent::Fill(FillContent {
        projection: Projection::default(),
        channels: BTreeMap::from([(C, FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.2, 0.6, 1.0))))]),
    });
    let a = ContentEffect::new(ContentEffectKind::Anchor, &tc);
    let color_anchor = a.id;
    colored.content_effects.push(Arc::new(a));
    ts.insert_above(None, colored);
    let mut reader = Layer::new_fill("Reader", &tc);
    reader.content = LayerContent::Fill(FillContent {
        projection: Projection::default(),
        channels: BTreeMap::from([(R, FillChannel::new(FillSource::Anchor { input: AnchorInput::new(color_anchor, C) }))]),
    });
    ts.insert_above(None, reader);
    let luma = 0.2126 * 0.2 + 0.7152 * 0.6 + 0.0722 * 1.0;
    assert!(close(get(&ts, &fx, R, 50, 50)[0], luma));
}

/// The anchor captures the content at its place in the stack: effects above it are not seen.
#[test]
fn anchors_capture_the_stack_at_their_position() {
    let fx = Fixture::plane();
    let mut ts = set();
    let tc = ts.clone();
    let (mut b, anchor) = bumps(&tc);
    // A Levels above the anchor raises the layer's own height; the anchor keeps 0.8.
    b.content_effects.push(Arc::new(ContentEffect::new(ContentEffectKind::Levels { channel: H, levels: Levels { out_low: 0.5, ..Levels::default() } }, &tc)));
    ts.insert_above(None, b);
    ts.insert_above(None, dirt(&tc, AnchorInput::new(anchor, H)));
    assert!(get(&ts, &fx, H, 10, 10)[0] > 0.85, "the layer shows its Levels");
    assert_color(get(&ts, &fx, C, 10, 10), mixed(0.8), "the anchor holds the value below the Levels");
}

/// References to anchors above, later in the stack, or missing change nothing (the black mask
/// stays black), and the doc index reports why.
#[test]
fn misplaced_and_missing_references_are_ignored() {
    let fx = Fixture::plane();
    let tc = set();
    let (b, anchor) = bumps(&tc);
    // Reader below the anchor.
    let mut ts = set();
    ts.insert_above(None, dirt(&tc, AnchorInput::new(anchor, H)));
    ts.insert_above(None, b.clone());
    assert_color(get(&ts, &fx, C, 10, 10), base_color_default(), "anchor above: ignored");
    let index = subcraft_doc::AnchorIndex::build(&ts.layers);
    assert_eq!(index.problems().next().unwrap().status, subcraft_doc::RefStatus::NotBelow);
    // Missing anchor.
    let mut ts = set();
    ts.insert_above(None, b);
    ts.insert_above(None, dirt(&tc, AnchorInput::new(EffectId::new(), H)));
    assert_color(get(&ts, &fx, C, 10, 10), base_color_default(), "missing anchor: ignored");
    assert_eq!(subcraft_doc::AnchorIndex::build(&ts.layers).problems().next().unwrap().status, subcraft_doc::RefStatus::Missing);
}

/// Mask anchors capture the mask at their place; a folder's content anchor captures its
/// children's composite.
#[test]
fn mask_anchors_and_folder_anchors() {
    let fx = Fixture::plane();
    let mut ts = set();
    let tc = ts.clone();
    // Layer with a 0.6 mask, an anchor, then an Invert: the anchor holds 0.6.
    let mut masked = Layer::new_fill("Masked", &tc);
    let mut m = Mask::new(RES, false);
    m.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Fill { value: 0.6, procedural: None, projection: Projection::default() })));
    let a = MaskEffect::new(MaskEffectKind::Anchor);
    let mask_anchor = a.id;
    m.effects.push(Arc::new(a));
    m.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Invert)));
    masked.mask = Some(m);
    for b in masked.blend.values_mut() {
        b.mode = subcraft_doc::BlendMode::Disable;
    }
    ts.insert_above(None, masked);
    ts.insert_above(None, dirt(&tc, AnchorInput::new(mask_anchor, H)));
    assert_color(get(&ts, &fx, C, 70, 70), mixed(0.6), "mask anchor");

    // Folder anchor over a child with height 0.8 on the left.
    let mut ts = set();
    let folder = ts.insert_above(None, Layer::new_folder("Folder", &tc));
    let (mut child, _) = bumps(&tc);
    child.content_effects.clear();
    ts.insert(Some(folder), 0, child).unwrap();
    let a = ContentEffect::new(ContentEffectKind::Anchor, &tc);
    let folder_anchor = a.id;
    ts.find_mut(folder).unwrap().content_effects.push(Arc::new(a));
    ts.insert_above(None, dirt(&tc, AnchorInput::new(folder_anchor, H)));
    assert_color(get(&ts, &fx, C, 10, 10), mixed(0.8), "folder anchor: the children's composite");
    assert_color(get(&ts, &fx, C, 100, 10), base_color_default(), "folder anchor outside the child");
}

/// A generator's input image: the anchored height replaces the baked map it thresholds.
#[test]
fn generator_reads_an_anchor_instead_of_the_baked_map() {
    let fx = Fixture::plane();
    let mut ts = set();
    let tc = ts.clone();
    let (b, anchor) = bumps(&tc);
    ts.insert_above(None, b);
    let mut l = dirt(&tc, AnchorInput::new(anchor, H));
    let mut g = Generator::new(GeneratorKind::MeshMap(MeshMapKind::Curvature));
    g.level = 0.4;
    g.softness = 0.05;
    g.input = Some(AnchorInput::new(anchor, H));
    l.mask.as_mut().unwrap().effects = vec![Arc::new(MaskEffect::new(MaskEffectKind::Generator(g)))];
    ts.insert_above(None, l);
    assert_color(get(&ts, &fx, C, 10, 10), mixed(1.0), "0.8 is above the 0.4 threshold");
    assert_color(get(&ts, &fx, C, 100, 10), base_color_default(), "0 is below it");
}

/// Painting the anchored layer refreshes the readers' tiles: the incremental composite equals a
/// full one, also when a blur on the reader's mask spreads the change.
#[test]
fn editing_the_anchored_layer_refreshes_its_readers() {
    let fx = Fixture::plane();
    let mut ts = set();
    let tc = ts.clone();
    let (b, anchor) = bumps(&tc);
    let bumps_id = ts.insert_above(None, b);
    let mut reader = dirt(&tc, AnchorInput::new(anchor, H));
    reader.mask.as_mut().unwrap().effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Filter(Filter::Blur { radius: 6.0 }))));
    ts.insert_above(None, reader);
    let mut cache = CompositeCache::new();
    cache.update(&ts, &fx.ctx());
    assert_eq!(cache.composite().unwrap(), &composite_full(&ts, &fx.ctx()));
    // Paint height on the right half, just above a tile edge (x 96..100, y 58..64): the blur
    // carries it into the tile below.
    let l = ts.find_mut(bumps_id).unwrap();
    let LayerContent::Paint(p) = &mut l.content else { unreachable!() };
    let img = p.channels.get_mut(&H).unwrap();
    for y in 58..64 {
        for x in 96..100 {
            img.set(x, y, [0.7, 1.0, 0.0, 0.0]);
        }
    }
    let dirty = cache.update(&ts, &fx.ctx());
    assert!(dirty.len() < 16, "a raster edit refreshes tiles, not the whole set: {dirty:?}");
    let full = composite_full(&ts, &fx.ctx());
    assert_eq!(cache.composite().unwrap(), &full, "incremental == full after editing the anchored layer");
    assert!(full.channel(C).unwrap().get(97, 61)[1] < base_color_default()[1] - 0.05, "the reader shows the new paint");
}

/// Sets without references do not change: the same composite whether or not a dangling anchor
/// point exists (anchors alone are inert).
#[test]
fn anchor_points_alone_change_nothing() {
    let fx = Fixture::plane();
    let mut ts = set();
    let tc = ts.clone();
    let (mut b, _) = bumps(&tc);
    b.content_effects.clear();
    let id: LayerId = ts.insert_above(None, b);
    let before = composite_full(&ts, &fx.ctx());
    ts.find_mut(id).unwrap().content_effects.push(Arc::new(ContentEffect::new(ContentEffectKind::Anchor, &tc)));
    let mut m = Mask::new(RES, true);
    m.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Anchor)));
    ts.find_mut(id).unwrap().mask = Some(m.clone());
    let after = composite_full(&ts, &fx.ctx());
    ts.find_mut(id).unwrap().mask.as_mut().unwrap().effects.clear();
    let without = composite_full(&ts, &fx.ctx());
    assert_eq!(before, without);
    assert_eq!(after, without, "anchor points are identity effects");
}
