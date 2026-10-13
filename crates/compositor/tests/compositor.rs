//! Compositor acceptance tests (layer-tree semantics, masks, dirty tracking, undo).

use std::sync::Arc;

use subcraft_compositor::{Composite, CompositeCache, EvalContext, composite_full};
use subcraft_core::{ChannelKind, ChannelValue, IRect, LayerId};
use subcraft_doc::{
    BlendMode, Document, FillChannel, FillSource, Generator, GeneratorKind, History, Layer, LayerContent, Levels, Mask,
    MaskEffect, MaskEffectKind, ResourceStore, TextureSet,
};
use subcraft_materials::{MeshMaps, TextureSetGeometry};
use subcraft_mesh::primitives;

const RES: u32 = 128;

struct Fixture {
    geo: TextureSetGeometry,
    res: ResourceStore,
    maps: MeshMaps,
}

impl Fixture {
    /// A plane whose single UV island covers the whole texture.
    fn plane() -> Self {
        Self::with_mesh(primitives::plane(2.0, 2))
    }

    fn with_mesh(mesh: subcraft_mesh::Mesh) -> Self {
        Self { geo: TextureSetGeometry::new(Arc::new(mesh), 0, RES), res: ResourceStore::default(), maps: MeshMaps::new() }
    }

    fn ctx(&self) -> EvalContext<'_> {
        EvalContext { geometry: &self.geo, resources: &self.res, mesh_maps: &self.maps }
    }
}

fn texture_set() -> TextureSet {
    TextureSet::new("T", 0, RES)
}

fn px(c: &Composite, kind: ChannelKind, x: u32, y: u32) -> [f32; 4] {
    c.channel(kind).unwrap().get(x, y)
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 2e-3
}

fn assert_rgb(c: &Composite, kind: ChannelKind, x: u32, y: u32, rgb: [f32; 3]) {
    let p = px(c, kind, x, y);
    assert!(close(p[0], rgb[0]) && close(p[1], rgb[1]) && close(p[2], rgb[2]), "{kind:?} at ({x},{y}): {p:?} != {rgb:?}");
}

fn assert_scalar(c: &Composite, kind: ChannelKind, x: u32, y: u32, v: f32) {
    let p = px(c, kind, x, y)[0];
    assert!(close(p, v), "{kind:?} at ({x},{y}): {p} != {v}");
}

/// Paint `value` with `alpha` into `rect` of a paint layer channel.
fn paint_rect(layer: &mut Layer, kind: ChannelKind, rect: IRect, value: [f32; 3], alpha: f32) {
    let LayerContent::Paint(p) = &mut layer.content else { panic!("not a paint layer") };
    let img = p.channel_mut(kind, RES);
    for y in rect.y0..rect.y1 {
        for x in rect.x0..rect.x1 {
            let v = if kind.components() == 1 { [value[0], alpha, 0.0, 0.0] } else { [value[0], value[1], value[2], alpha] };
            img.set(x as u32, y as u32, v);
        }
    }
}

fn fill_layer(ts: &TextureSet, values: &[(ChannelKind, ChannelValue)]) -> Layer {
    let mut l = Layer::new_fill("Fill", ts);
    let LayerContent::Fill(f) = &mut l.content else { unreachable!() };
    for (k, ch) in f.channels.iter_mut() {
        ch.enabled = false;
        if let Some((_, v)) = values.iter().find(|(kk, _)| kk == k) {
            *ch = FillChannel { enabled: true, source: FillSource::uniform(*v) };
        }
    }
    l
}

fn set_fill(ts: &mut TextureSet, id: LayerId, kind: ChannelKind, v: ChannelValue) {
    let LayerContent::Fill(f) = &mut ts.find_mut(id).unwrap().content else { panic!() };
    f.channels.insert(kind, FillChannel { enabled: true, source: FillSource::uniform(v) });
}

#[test]
fn empty_stack_is_channel_defaults() {
    let fx = Fixture::plane();
    let c = composite_full(&texture_set(), &fx.ctx());
    assert_rgb(&c, ChannelKind::BaseColor, 3, 3, [0.5; 3]);
    assert_rgb(&c, ChannelKind::Normal, 100, 3, [0.5, 0.5, 1.0]);
    assert_scalar(&c, ChannelKind::Roughness, 3, 3, 0.3);
    assert_eq!(c.channel(ChannelKind::BaseColor).unwrap().comps(), 4);
    assert_eq!(c.channel(ChannelKind::Roughness).unwrap().comps(), 1);
}

#[test]
fn reordering_two_paint_layers_changes_result_predictably() {
    let fx = Fixture::plane();
    let mut ts = texture_set();
    let tc = ts.clone();
    let mut a = Layer::new_paint("A", &tc);
    paint_rect(&mut a, ChannelKind::BaseColor, IRect::new(0, 0, 32, 32), [1.0, 0.0, 0.0], 1.0);
    let mut b = Layer::new_paint("B", &tc);
    paint_rect(&mut b, ChannelKind::BaseColor, IRect::new(16, 16, 48, 48), [0.0, 0.0, 1.0], 1.0);
    let a_id = ts.insert_above(None, a);
    ts.insert_above(None, b);

    let c = composite_full(&ts, &fx.ctx());
    assert_rgb(&c, ChannelKind::BaseColor, 20, 20, [0.0, 0.0, 1.0]);
    assert_rgb(&c, ChannelKind::BaseColor, 5, 5, [1.0, 0.0, 0.0]);
    assert_rgb(&c, ChannelKind::BaseColor, 40, 40, [0.0, 0.0, 1.0]);
    assert_rgb(&c, ChannelKind::BaseColor, 100, 100, [0.5; 3]);

    ts.move_to(a_id, None, 1).unwrap();
    let c = composite_full(&ts, &fx.ctx());
    assert_rgb(&c, ChannelKind::BaseColor, 20, 20, [1.0, 0.0, 0.0]);
    assert_rgb(&c, ChannelKind::BaseColor, 5, 5, [1.0, 0.0, 0.0]);
    assert_rgb(&c, ChannelKind::BaseColor, 40, 40, [0.0, 0.0, 1.0]);
}

#[test]
fn hiding_a_parent_folder_hides_its_children() {
    let fx = Fixture::plane();
    let mut ts = texture_set();
    let tc = ts.clone();
    let folder = ts.insert_above(None, Layer::new_folder("F", &tc));
    let inner = ts.insert(Some(folder), 0, Layer::new_folder("Inner", &tc)).unwrap();
    ts.insert(Some(inner), 0, fill_layer(&tc, &[(ChannelKind::BaseColor, ChannelValue::rgb(0.9, 0.1, 0.1))])).unwrap();
    let visible = composite_full(&ts, &fx.ctx());
    assert_rgb(&visible, ChannelKind::BaseColor, 64, 64, [0.9, 0.1, 0.1]);
    ts.find_mut(folder).unwrap().visible = false;
    let hidden = composite_full(&ts, &fx.ctx());
    assert_eq!(hidden, composite_full(&texture_set(), &fx.ctx()));
}

#[test]
fn channel_opacity_affects_only_that_channel() {
    let fx = Fixture::plane();
    let mut ts = texture_set();
    let tc = ts.clone();
    let id = ts.insert_above(
        None,
        fill_layer(&tc, &[(ChannelKind::BaseColor, ChannelValue::rgb(0.2, 0.4, 0.6)), (ChannelKind::Roughness, ChannelValue::scalar(0.9))]),
    );
    ts.find_mut(id).unwrap().blend.get_mut(&ChannelKind::Roughness).unwrap().opacity = 0.5;
    let c = composite_full(&ts, &fx.ctx());
    assert_rgb(&c, ChannelKind::BaseColor, 10, 70, [0.2, 0.4, 0.6]);
    assert_scalar(&c, ChannelKind::Roughness, 10, 70, 0.3 + (0.9 - 0.3) * 0.5);
    assert_scalar(&c, ChannelKind::Metallic, 10, 70, 0.0);
}

#[test]
fn black_mask_hides_and_white_paint_reveals_locally() {
    let fx = Fixture::plane();
    let mut ts = texture_set();
    let tc = ts.clone();
    let mut layer = fill_layer(&tc, &[(ChannelKind::Roughness, ChannelValue::scalar(0.9))]);
    layer.mask = Some(Mask::new(RES, false));
    let id = ts.insert_above(None, layer);

    let mut cache = CompositeCache::new();
    cache.update(&ts, &fx.ctx());
    let c = cache.composite().unwrap();
    for (x, y) in [(0, 0), (40, 40), (127, 127)] {
        assert_scalar(c, ChannelKind::Roughness, x, y, 0.3);
    }

    let mask = ts.find_mut(id).unwrap().mask.as_mut().unwrap();
    for y in 40..44 {
        for x in 40..44 {
            mask.base.set(x, y, [1.0; 4]);
        }
    }
    let dirty = cache.update(&ts, &fx.ctx());
    assert_eq!(dirty, vec![0], "mask paint dirties only its tile");
    let c = cache.composite().unwrap();
    assert_scalar(c, ChannelKind::Roughness, 41, 41, 0.9);
    assert_scalar(c, ChannelKind::Roughness, 39, 41, 0.3);
    assert_scalar(c, ChannelKind::Roughness, 100, 100, 0.3);
}

#[test]
fn fill_parameter_change_keeps_paint_above_intact() {
    let fx = Fixture::plane();
    let mut ts = texture_set();
    let tc = ts.clone();
    let fill = ts.insert_above(None, fill_layer(&tc, &[(ChannelKind::Roughness, ChannelValue::scalar(0.2))]));
    let mut paint = Layer::new_paint("P", &tc);
    paint_rect(&mut paint, ChannelKind::Roughness, IRect::new(10, 10, 20, 20), [0.9; 3], 1.0);
    ts.insert_above(None, paint);

    let mut cache = CompositeCache::new();
    cache.update(&ts, &fx.ctx());
    assert_scalar(cache.composite().unwrap(), ChannelKind::Roughness, 15, 15, 0.9);
    assert_scalar(cache.composite().unwrap(), ChannelKind::Roughness, 50, 50, 0.2);

    set_fill(&mut ts, fill, ChannelKind::Roughness, ChannelValue::scalar(0.7));
    let dirty = cache.update(&ts, &fx.ctx());
    assert_eq!(dirty.len(), 4, "parameter change re-evaluates every tile");
    let c = cache.composite().unwrap();
    assert_scalar(c, ChannelKind::Roughness, 15, 15, 0.9);
    assert_scalar(c, ChannelKind::Roughness, 50, 50, 0.7);
    assert_eq!(c, &composite_full(&ts, &fx.ctx()));
}

#[test]
fn passthrough_and_isolated_folders_differ_under_multiply() {
    let fx = Fixture::plane();
    let mut ts = texture_set();
    let tc = ts.clone();
    ts.insert_above(None, fill_layer(&tc, &[(ChannelKind::BaseColor, ChannelValue::scalar(0.5))]));
    let folder = ts.insert_above(None, Layer::new_folder("F", &tc));
    let mut mul = fill_layer(&tc, &[(ChannelKind::BaseColor, ChannelValue::scalar(0.5))]);
    mul.blend.get_mut(&ChannelKind::BaseColor).unwrap().mode = BlendMode::Multiply;
    ts.insert(Some(folder), 0, mul).unwrap();

    // Passthrough (folder default): the child multiplies the stack below.
    let c = composite_full(&ts, &fx.ctx());
    assert_rgb(&c, ChannelKind::BaseColor, 64, 64, [0.25; 3]);

    // Isolated: the child multiplies a transparent backdrop (passes through as 0.5), and the
    // folder result is then composited Normal over 0.5.
    ts.find_mut(folder).unwrap().blend.get_mut(&ChannelKind::BaseColor).unwrap().mode = BlendMode::Normal;
    let c = composite_full(&ts, &fx.ctx());
    assert_rgb(&c, ChannelKind::BaseColor, 64, 64, [0.5; 3]);

    // Isolated folder blended with Multiply itself reproduces the passthrough result.
    ts.find_mut(folder).unwrap().blend.get_mut(&ChannelKind::BaseColor).unwrap().mode = BlendMode::Multiply;
    let c = composite_full(&ts, &fx.ctx());
    assert_rgb(&c, ChannelKind::BaseColor, 64, 64, [0.25; 3]);

    // Passthrough at half opacity lerps between below (0.5) and the children's result (0.25).
    let f = ts.find_mut(folder).unwrap();
    let b = f.blend.get_mut(&ChannelKind::BaseColor).unwrap();
    b.mode = BlendMode::Passthrough;
    b.opacity = 0.5;
    let c = composite_full(&ts, &fx.ctx());
    assert_rgb(&c, ChannelKind::BaseColor, 64, 64, [0.375; 3]);
}

#[test]
fn paint_layers_contribute_only_channels_they_have() {
    let fx = Fixture::plane();
    let mut ts = texture_set();
    let tc = ts.clone();
    ts.insert_above(None, fill_layer(&tc, &[(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0)), (ChannelKind::Roughness, ChannelValue::scalar(0.8))]));
    let mut p = Layer::new_paint("P", &tc);
    for b in p.blend.values_mut() {
        b.mode = BlendMode::Replace;
    }
    paint_rect(&mut p, ChannelKind::BaseColor, IRect::new(0, 0, 4, 4), [0.0, 1.0, 0.0], 1.0);
    ts.insert_above(None, p);
    let c = composite_full(&ts, &fx.ctx());
    // No roughness image: roughness untouched even in Replace mode.
    assert_scalar(&c, ChannelKind::Roughness, 2, 2, 0.8);
    assert_rgb(&c, ChannelKind::BaseColor, 2, 2, [0.0, 1.0, 0.0]);
    // Replace ignores content alpha: unpainted texels replace with the image's fill value.
    assert_rgb(&c, ChannelKind::BaseColor, 50, 50, [0.5; 3]);
}

#[test]
fn single_tile_edit_reports_exactly_that_tile() {
    let fx = Fixture::plane();
    let mut ts = texture_set();
    let tc = ts.clone();
    ts.insert_above(None, fill_layer(&tc, &[(ChannelKind::BaseColor, ChannelValue::scalar(0.1))]));
    let mut p = Layer::new_paint("P", &tc);
    paint_rect(&mut p, ChannelKind::BaseColor, IRect::new(0, 0, 1, 1), [1.0; 3], 1.0);
    let pid = ts.insert_above(None, p);

    let mut cache = CompositeCache::new();
    assert_eq!(cache.update(&ts, &fx.ctx()), vec![0, 1, 2, 3]);
    assert!(cache.update(&ts, &fx.ctx()).is_empty(), "unchanged document: nothing dirty");

    paint_rect(ts.find_mut(pid).unwrap(), ChannelKind::BaseColor, IRect::new(70, 70, 72, 72), [0.0, 1.0, 0.0], 1.0);
    assert_eq!(cache.update(&ts, &fx.ctx()), vec![3]);
    assert_rgb(cache.composite().unwrap(), ChannelKind::BaseColor, 71, 71, [0.0, 1.0, 0.0]);

    // First stroke into a channel the layer did not have yet is still incremental.
    paint_rect(ts.find_mut(pid).unwrap(), ChannelKind::Roughness, IRect::new(65, 2, 66, 3), [0.9; 3], 1.0);
    assert_eq!(cache.update(&ts, &fx.ctx()), vec![1]);
    assert_scalar(cache.composite().unwrap(), ChannelKind::Roughness, 65, 2, 0.9);

    // Renaming changes no pixels.
    ts.find_mut(pid).unwrap().name = "Renamed".into();
    assert!(cache.update(&ts, &fx.ctx()).is_empty());
    assert_eq!(cache.composite().unwrap(), &composite_full(&ts, &fx.ctx()));
}

#[test]
fn undo_restores_the_earlier_composite_bit_exactly() {
    let fx = Fixture::plane();
    let mut ts = texture_set();
    let tc = ts.clone();
    let fill = ts.insert_above(None, fill_layer(&tc, &[(ChannelKind::BaseColor, ChannelValue::rgb(0.3, 0.5, 0.7)), (ChannelKind::Roughness, ChannelValue::scalar(0.6))]));
    let folder = ts.insert_above(None, Layer::new_folder("F", &tc));
    let mut p = Layer::new_paint("P", &tc);
    p.mask = Some(Mask::new(RES, true));
    paint_rect(&mut p, ChannelKind::BaseColor, IRect::new(5, 5, 90, 30), [0.9, 0.2, 0.1], 0.7);
    let pid = ts.insert(Some(folder), 0, p).unwrap();
    let doc1 = Document { texture_sets: vec![ts] };

    let mut history = History::new(doc1.clone(), "Open");
    let mut cache = CompositeCache::new();
    cache.update(&history.current().texture_sets[0], &fx.ctx());
    let first = cache.composite().unwrap().clone();

    // A stroke, then a parameter edit.
    let mut d2 = history.current().clone();
    paint_rect(d2.texture_sets[0].find_mut(pid).unwrap(), ChannelKind::BaseColor, IRect::new(100, 100, 110, 120), [0.0, 0.0, 1.0], 1.0);
    history.push("Stroke", d2);
    let dirty = cache.update(&history.current().texture_sets[0], &fx.ctx());
    assert_eq!(dirty, vec![3]);
    let mut d3 = history.current().clone();
    set_fill(&mut d3.texture_sets[0], fill, ChannelKind::Roughness, ChannelValue::scalar(0.1));
    history.push("Roughness", d3);
    cache.update(&history.current().texture_sets[0], &fx.ctx());
    assert_ne!(cache.composite().unwrap(), &first);

    history.undo();
    cache.update(&history.current().texture_sets[0], &fx.ctx());
    let second = cache.composite().unwrap().clone();
    assert_eq!(second, composite_full(&history.current().texture_sets[0], &fx.ctx()));
    history.undo();
    let dirty = cache.update(&history.current().texture_sets[0], &fx.ctx());
    assert_eq!(dirty, vec![3], "undoing the stroke re-evaluates only its tile");
    assert_eq!(cache.composite().unwrap(), &first, "bit-exact earlier composite");
    assert_eq!(cache.composite().unwrap(), &composite_full(&doc1.texture_sets[0], &fx.ctx()));
    history.redo();
    cache.update(&history.current().texture_sets[0], &fx.ctx());
    assert_eq!(cache.composite().unwrap(), &second);
}

#[test]
fn mask_effects_levels_invert_and_directional_generator() {
    let fx = Fixture::with_mesh(primitives::cube(2.0));
    let mut ts = texture_set();
    let tc = ts.clone();
    let mut layer = fill_layer(&tc, &[(ChannelKind::Roughness, ChannelValue::scalar(1.0))]);
    let mut mask = Mask::new(RES, false);
    let mut g = Generator::new(GeneratorKind::Directional);
    g.softness = 0.05;
    g.level = 0.75;
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(g))));
    layer.mask = Some(mask);
    let id = ts.insert_above(None, layer);

    // Find a texel on the +Y face (normal up) and one on the -Y face via the geometry.
    let geo = &fx.geo;
    let find = |n: [f32; 3]| {
        let mut hits = Vec::new();
        for t in 0..geo.tile_count() {
            let g = geo.tile(t);
            hits.extend((0..4096).filter(|&i| g.coverage(i) > 0.0 && g.normal(i) == n).map(|i| g.texel_xy(i)));
        }
        hits[hits.len() / 2]
    };
    let (up, down) = (find([0.0, 1.0, 0.0]), find([0.0, -1.0, 0.0]));
    let c = composite_full(&ts, &fx.ctx());
    assert_scalar(&c, ChannelKind::Roughness, up.0, up.1, 1.0);
    assert_scalar(&c, ChannelKind::Roughness, down.0, down.1, 0.3);

    // Invert on top flips it; Levels with out_high 0.5 halves the revealed amount.
    let m = ts.find_mut(id).unwrap().mask.as_mut().unwrap();
    m.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Invert)));
    m.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Levels(Levels { out_high: 0.5, ..Levels::default() }))));
    let c = composite_full(&ts, &fx.ctx());
    assert_scalar(&c, ChannelKind::Roughness, up.0, up.1, 0.3);
    assert_scalar(&c, ChannelKind::Roughness, down.0, down.1, 0.3 + 0.7 * 0.5);
}

#[test]
fn normal_channel_combines_with_whiteout() {
    let fx = Fixture::plane();
    let mut ts = texture_set();
    let tc = ts.clone();
    let tilt = |x: f32| {
        let n = glam::Vec3::new(x, 0.0, 1.0).normalize();
        ChannelValue::rgb(n.x * 0.5 + 0.5, n.y * 0.5 + 0.5, n.z * 0.5 + 0.5)
    };
    ts.insert_above(None, fill_layer(&tc, &[(ChannelKind::Normal, tilt(0.3))]));
    ts.insert_above(None, fill_layer(&tc, &[(ChannelKind::Normal, tilt(0.3))]));
    let c = composite_full(&ts, &fx.ctx());
    let expect = subcraft_compositor::blend::whiteout(tilt(0.3).0, tilt(0.3).0);
    // First layer combines with the flat default (identity), the second tilts further.
    assert_rgb(&c, ChannelKind::Normal, 64, 64, expect);
    assert!(px(&c, ChannelKind::Normal, 64, 64)[0] > tilt(0.3).0[0] + 0.02);
}
