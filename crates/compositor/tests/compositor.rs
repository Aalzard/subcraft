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
            *ch = FillChannel::new(FillSource::uniform(*v));
        }
    }
    l
}

fn set_fill(ts: &mut TextureSet, id: LayerId, kind: ChannelKind, v: ChannelValue) {
    let LayerContent::Fill(f) = &mut ts.find_mut(id).unwrap().content else { panic!() };
    f.channels.insert(kind, FillChannel::new(FillSource::uniform(v)));
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

#[test]
fn generator_noise_breakup_is_continuous_across_uv_seams() {
    // A flat 2×2 quad split at x = 0 into two UV islands that sit far apart (the right one is
    // also mirrored), so the seam is not a geometric edge.
    use glam::{Vec2, Vec3};
    use subcraft_mesh::{Corner, MeshBuilder};
    let mut b = MeshBuilder::new("SplitPlane");
    let m = b.material("SplitPlane");
    let c = |x: f32, z: f32, u: f32, v: f32| Corner { position: Vec3::new(x, 0.0, z), normal: Some(Vec3::Y), uv: Some(Vec2::new(u, v)) };
    let (l, r) = ([c(-1.0, -1.0, 0.05, 0.05), c(0.0, -1.0, 0.45, 0.05), c(0.0, 1.0, 0.45, 0.95), c(-1.0, 1.0, 0.05, 0.95)],
        [c(0.0, -1.0, 0.55, 0.95), c(1.0, -1.0, 0.95, 0.95), c(1.0, 1.0, 0.95, 0.05), c(0.0, 1.0, 0.55, 0.05)]);
    for q in [l, r] {
        b.push_triangle(m, [q[0], q[2], q[1]]);
        b.push_triangle(m, [q[0], q[3], q[2]]);
    }
    let fx = Fixture::with_mesh(b.build().unwrap());

    // Directional along X reads 0.5 everywhere on this plane, so the noise alone decides the mask.
    let mut ts = texture_set();
    let tc = ts.clone();
    let mut layer = fill_layer(&tc, &[(ChannelKind::Roughness, ChannelValue::scalar(1.0))]);
    let mut mask = Mask::new(RES, false);
    let mut g = Generator::new(GeneratorKind::Directional);
    g.direction = [1.0, 0.0, 0.0];
    g.level = 0.5;
    g.softness = 0.1;
    g.noise_amount = 0.8;
    g.organic_breakup = true;
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(g))));
    layer.mask = Some(mask);
    ts.insert_above(None, layer);
    let comp = composite_full(&ts, &fx.ctx());

    // Fully covered texels hugging the seam on each island (positions are normalized to 0..1, so the seam is x = 0.5 and one
    // texel is ~0.01 wide), keyed by their 3D z.
    let geo = &fx.geo;
    let (mut left, mut right) = (Vec::new(), Vec::new());
    for t in 0..geo.tile_count() {
        let tile = geo.tile(t);
        for i in 0..4096 {
            let [x, _, z] = tile.position(i);
            if tile.coverage(i) < 1.0 || (x - 0.5).abs() > 0.015 {
                continue;
            }
            let (tx, ty) = tile.texel_xy(i);
            let v = px(&comp, ChannelKind::Roughness, tx, ty)[0];
            if tx < RES / 2 { left.push((z, v)) } else { right.push((z, v)) }
        }
    }
    assert!(left.len() > 20 && right.len() > 20);
    let mut diff = 0.0;
    for &(z, v) in &left {
        let nearest = right.iter().min_by(|a, b| (a.0 - z).abs().total_cmp(&(b.0 - z).abs())).unwrap();
        diff += (nearest.1 - v).abs();
    }
    let diff = diff / left.len() as f32;
    assert!(diff < 0.05, "mask jumps across the UV seam by {diff} on average");
}

#[test]
fn organic_edge_breakup_matches_numeric_coverage_and_seed_size_invalidate_cached_fields() {
    use subcraft_core::MeshMapKind;
    let mut fx = Fixture::plane();
    fx.maps.insert(MeshMapKind::Curvature, subcraft_image::TiledImage::new(RES, RES, 1, [0.6; 4]));
    let mut ts = texture_set(); let tc = ts.clone();
    let mut layer = fill_layer(&tc, &[(ChannelKind::Roughness, ChannelValue::scalar(1.))]);
    let mut mask = Mask::new(RES, false);
    let mut generator = Generator::new(GeneratorKind::Edges); generator.level = 0.6; generator.edge_spread = None;
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(generator))));
    layer.mask = Some(mask); let id = ts.insert_above(None, layer);
    let mut cache = CompositeCache::new(); cache.update(&ts, &fx.ctx());
    let first = cache.composite().unwrap().clone();
    for index in 0..fx.geo.tile_count() {
        let geo = fx.geo.tile(index);
        for i in (0..4096).step_by(117) {
            if geo.coverage(i) < 1. { continue; }
            let noise = subcraft_materials::organic::eval(&generator.noise, geo.position(i));
            let level = generator.level + (noise - 0.5) * generator.noise_amount;
            let t = ((0.60009766 - (level - generator.softness * 0.5)) / generator.softness).clamp(0., 1.);
            let expected = 0.3 + 0.7 * t * t * (3. - 2. * t);
            let (x, y) = geo.texel_xy(i); assert_scalar(&first, ChannelKind::Roughness, x, y, expected);
        }
    }
    for (size, seed) in [(25., 1), (25., 8)] {
        generator.set_pattern_size(size); generator.noise.seed = seed;
        let effect = Arc::make_mut(&mut ts.find_mut(id).unwrap().mask.as_mut().unwrap().effects[0]);
        effect.kind = MaskEffectKind::Generator(generator);
        cache.update(&ts, &fx.ctx());
        assert_eq!(cache.composite().unwrap(), &composite_full(&ts, &fx.ctx()));
        let changes = first.channel(ChannelKind::Roughness).unwrap().to_dense().iter()
            .zip(cache.composite().unwrap().channel(ChannelKind::Roughness).unwrap().to_dense()).filter(|(a, b)| (**a - b).abs() > 0.02).count();
        assert!(changes > 1000, "size/seed should change the visible breakup: {changes}");
    }
}

#[test]
fn natural_spread_matches_surface_distance_golden_and_varies_at_zero_breakup() {
    let fx = Fixture::with_mesh(primitives::cube(2.0));
    let mut ts = texture_set(); let tc = ts.clone();
    let mut layer = fill_layer(&tc, &[(ChannelKind::Roughness, ChannelValue::scalar(1.0))]);
    let mut mask = Mask::new(RES, false);
    let mut g = Generator::new(GeneratorKind::Edges); g.noise_amount = 0.0; g.softness = 0.25;
    g.edge_spread = Some(subcraft_doc::EdgeSpread { distance: 8.0, variation: 0.9 });
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(g)))); layer.mask = Some(mask);
    let id = ts.insert_above(None, layer);
    let mut cache = CompositeCache::new(); cache.update(&ts, &fx.ctx());
    let first = cache.composite().unwrap().clone();
    let mut checked = 0;
    for t in 0..fx.geo.tile_count() {
        let geo = fx.geo.tile(t);
        for i in (0..4096).step_by(17) {
            if geo.coverage(i) < 1.0 { continue; }
            let p = geo.position(i); let n = geo.normal(i);
            let distance = [0, 1, 2].into_iter().filter(|&axis| n[axis].abs() < 0.5)
                .map(|axis| p[axis].min(1.0 - p[axis])).fold(f32::INFINITY, f32::min).max(0.0);
            let noise = subcraft_materials::organic::eval(&g.noise, p);
            // Boundary chips: a second seeded field four times finer than Pattern size, at higher
            // contrast, blended 50/50 with the Pattern-size field.
            let mut chips = g.noise; chips.scale = g.noise.scale * 4.0; chips.seed = chips.seed.wrapping_add(0x5bd1);
            let chips = subcraft_materials::organic::eval(&chips, p);
            let boundary = ((noise - 0.5) * 3.0).clamp(-0.5, 0.5) * 0.5 + ((chips - 0.5) * 4.0).clamp(-0.5, 0.5) * 0.5;
            let reach = 0.08 * (1.0 + 0.9 * boundary * 1.8);
            let t = ((distance - reach * 0.75) / (reach * 0.25)).clamp(0.0, 1.0);
            let expected = 0.3 + 0.7 * (1.0 - t * t * (3.0 - 2.0 * t));
            let (x, y) = geo.texel_xy(i); assert_scalar(&first, ChannelKind::Roughness, x, y, expected); checked += 1;
        }
    }
    assert!(checked > 400, "exercise all tiles and separate cube UV islands");
    let geo = fx.geo.tile(0); let distance_field = fx.geo.edge_distance_tile(&geo);
    for (variation, seed, size) in [(0.0, 1, 12.5), (0.9, 51, 12.5), (0.9, 1, 35.0)] {
        let mut edited = g; edited.edge_spread.as_mut().unwrap().variation = variation;
        edited.noise.seed = seed; edited.set_pattern_size(size);
        Arc::make_mut(&mut ts.find_mut(id).unwrap().mask.as_mut().unwrap().effects[0]).kind = MaskEffectKind::Generator(edited);
        cache.update(&ts, &fx.ctx());
        let current = cache.composite().unwrap(); assert_eq!(current, &composite_full(&ts, &fx.ctx()));
        let changes = current.channel(ChannelKind::Roughness).unwrap().to_dense().iter()
            .zip(first.channel(ChannelKind::Roughness).unwrap().to_dense()).filter(|(a, b)| (**a - *b).abs() > 0.02).count();
        assert!(changes > 30, "variation/seed/size must affect the boundary at Breakup 0: {changes}");
        assert!(Arc::ptr_eq(&distance_field, &fx.geo.edge_distance_tile(&geo)), "slider edits must reuse geometry distances");
    }
}

#[test]
fn spread_reaches_independently_at_low_level_without_turning_flat_faces_white() {
    let fx = Fixture::with_mesh(primitives::cube(2.0));
    let mut ts = texture_set(); let tc = ts.clone();
    let mut layer = fill_layer(&tc, &[(ChannelKind::Roughness, ChannelValue::scalar(1.0))]);
    let mut mask = Mask::new(RES, false);
    let mut g = Generator::new(GeneratorKind::Edges); g.noise_amount = 0.0;
    g.edge_spread.as_mut().unwrap().distance = 2.0;
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(g)))); layer.mask = Some(mask);
    let id = ts.insert_above(None, layer);
    let small = composite_full(&ts, &fx.ctx());
    for (spread, level) in [(8.0, 0.5), (8.0, 0.9), (12.5, 0.065)] {
        g.edge_spread.as_mut().unwrap().distance = spread; g.level = level;
        Arc::make_mut(&mut ts.find_mut(id).unwrap().mask.as_mut().unwrap().effects[0]).kind = MaskEffectKind::Generator(g);
        let large = composite_full(&ts, &fx.ctx());
        let (mut grew, mut far) = (0, 0);
        for t in 0..fx.geo.tile_count() { let geo = fx.geo.tile(t); for i in 0..4096 {
            if geo.coverage(i) < 1.0 { continue; }
            let (x, y) = geo.texel_xy(i);
            let before = px(&small, ChannelKind::Roughness, x, y)[0]; let after = px(&large, ChannelKind::Roughness, x, y)[0];
            assert!(after >= before - 0.001); if after > before + 0.1 { grew += 1; }
            let p = geo.position(i); let n = geo.normal(i);
            if [0, 1, 2].into_iter().filter(|&a| n[a].abs() < 0.5).all(|a| p[a] > 0.3 && p[a] < 0.7) {
                assert!((after - 0.3).abs() < 0.001); far += 1;
            }
        } }
        assert!(grew > 100 && far > 100);
        if level == 0.065 {
            // No baked map in this fixture: Level cannot secretly shrink geometric reach.
            let at_low_level = large;
            g.level = 0.5;
            Arc::make_mut(&mut ts.find_mut(id).unwrap().mask.as_mut().unwrap().effects[0]).kind = MaskEffectKind::Generator(g);
            assert_eq!(at_low_level, composite_full(&ts, &fx.ctx()));
        }
    }
}

#[test]
fn narrow_spread_boundary_is_chipped_at_its_own_scale_not_a_straight_stripe() {
    // Owner: boundaries looked unnaturally straight at Breakup 0. Measure the band's reach in
    // narrow columns along one cube edge; nearby columns must differ, not only slowly drift.
    const HI: u32 = 1024;
    let fx = Fixture { geo: TextureSetGeometry::new(Arc::new(primitives::cube(2.0)), 0, HI), res: ResourceStore::default(), maps: MeshMaps::new() };
    let mut ratios = Vec::new();
    for seed in [1, 7, 23] {
    let mut ts = TextureSet::new("T", 0, HI); let tc = ts.clone();
    let mut layer = fill_layer(&tc, &[(ChannelKind::Roughness, ChannelValue::scalar(1.0))]);
    let mut g = Generator::new(GeneratorKind::Edges); g.noise_amount = 0.0; g.softness = 0.02; g.noise.seed = seed;
    g.edge_spread = Some(subcraft_doc::EdgeSpread { distance: 3.0, variation: 0.7 });
    let mut mask = Mask::new(HI, false);
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(g)))); layer.mask = Some(mask);
    ts.insert_above(None, layer);
    let image = composite_full(&ts, &fx.ctx());
    // Top face (+Y): reach from the z=1 edge, per 1% x column (normalized frame, cube = 0..1).
    let mut columns = std::collections::BTreeMap::<i32, f32>::new();
    for t in 0..fx.geo.tile_count() { let geo = fx.geo.tile(t); for i in 0..4096 {
        if geo.coverage(i) < 1.0 || geo.normal(i)[1] < 0.9 { continue; }
        let p = geo.position(i);
        if p[0] < 0.1 || p[0] > 0.9 || p[2] < 0.85 { continue; }
        let (x, y) = geo.texel_xy(i);
        if px(&image, ChannelKind::Roughness, x, y)[0] > 0.65 {
            let reach = columns.entry((p[0] * 100.0) as i32).or_insert(0.0); *reach = reach.max(1.0 - p[2]);
        }
    } }
    let reach: Vec<f32> = columns.values().copied().collect();
    assert!(reach.len() > 70, "{} columns", reach.len());
    let mean = reach.iter().sum::<f32>() / reach.len() as f32;
    // Detail at the band's own scale: deviation from a 9-column (~9%) moving average. A slow
    // Pattern-size wobble alone leaves a smooth stripe with almost no residual.
    let residual = (4..reach.len() - 4).map(|i| {
        let average = reach[i - 4..=i + 4].iter().sum::<f32>() / 9.0; (reach[i] - average).abs()
    }).sum::<f32>() / (reach.len() - 8) as f32;
    ratios.push(residual / mean);
    }
    // Measured before the chip field: 0.097/0.085/0.048 (mean 0.077); with it 0.107/0.104/0.072.
    let average = ratios.iter().sum::<f32>() / ratios.len() as f32;
    assert!(average > 0.088 && ratios.iter().all(|r| *r > 0.06), "boundary too straight: residual/reach {ratios:?}");
}

#[test]
fn maximum_spread_at_level_point_065_has_the_requested_numeric_reach() {
    let fx = Fixture::with_mesh(primitives::cube(2.0));
    let mut ts = texture_set(); let tc = ts.clone();
    let mut layer = fill_layer(&tc, &[(ChannelKind::Roughness, ChannelValue::scalar(1.0))]);
    let mut g = Generator::new(GeneratorKind::Edges);
    g.level = 0.065; g.noise_amount = 0.0; g.softness = 0.25;
    g.edge_spread = Some(subcraft_doc::EdgeSpread { distance: 12.5, variation: 0.0 });
    let mut mask = Mask::new(RES, false);
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(g)))); layer.mask = Some(mask);
    ts.insert_above(None, layer);
    let image = composite_full(&ts, &fx.ctx()); let mut wide = 0;
    for t in 0..fx.geo.tile_count() { let geo = fx.geo.tile(t); for i in 0..4096 {
        if geo.coverage(i) < 1.0 { continue; }
        let p = geo.position(i); let n = geo.normal(i);
        let distance = [0, 1, 2].into_iter().filter(|&a| n[a].abs() < 0.5)
            .map(|a| p[a].min(1.0 - p[a])).fold(f32::INFINITY, f32::min).max(0.0);
        let fade = ((distance - 0.125 * 0.75) / (0.125 * 0.25)).clamp(0.0, 1.0);
        let expected = 0.3 + 0.7 * (1.0 - fade * fade * (3.0 - 2.0 * fade));
        let (x, y) = geo.texel_xy(i); assert_scalar(&image, ChannelKind::Roughness, x, y, expected);
        if distance > 0.05 && distance < 0.09 { wide += 1; }
    } }
    assert!(wide > 100, "exercise the area lost by the old Level multiplier");
}

#[test]
fn image_mask_uses_alpha_or_raw_brightness() {
    use subcraft_core::ResourceId;
    use subcraft_doc::{Projection, Resource, ResourceKind};
    use subcraft_image::Bitmap;

    let mut fx = Fixture::plane();
    let add = |fx: &mut Fixture, b: Bitmap| {
        let id = ResourceId::new();
        fx.res.insert(Resource { id, name: "m.png".into(), kind: ResourceKind::Texture, bitmap: Arc::new(b), source_path: None, encoded: None });
        id
    };
    // 8x1 images: left half / right half. Sampled near u = 0.25 / 0.75, far from the edge.
    let halves = |a: [u8; 4], b: [u8; 4], ch: usize| -> Vec<u8> { (0..8).flat_map(|x| if x < 4 { a } else { b }[..ch].to_vec()).collect() };
    let logo = add(&mut fx, Bitmap::new_u8(8, 1, 4, halves([9, 9, 9, 0], [9, 9, 9, 255], 4)));
    let gray = add(&mut fx, Bitmap::new_u8(8, 1, 1, halves([64; 4], [192; 4], 1)));
    let (left, right) = (RES / 4, 3 * RES / 4);

    let eval = |fx: &Fixture, resource: ResourceId, invert: bool| {
        let mut ts = texture_set();
        let mut l = fill_layer(&ts, &[(ChannelKind::Roughness, ChannelValue::scalar(1.0))]);
        let mut mask = Mask::new(RES, false);
        mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Image { resource, projection: Projection::default(), invert })));
        l.mask = Some(mask);
        ts.insert_above(None, l);
        composite_full(&ts, &fx.ctx())
    };

    // Alpha image: transparent part leaves the black mask, opaque part reveals the layer.
    let c = eval(&fx, logo, false);
    assert_scalar(&c, ChannelKind::Roughness, left, RES / 2, 0.3);
    assert_scalar(&c, ChannelKind::Roughness, right, RES / 2, 1.0);
    // Inverted alpha image stamps black: on a black mask nothing shows.
    let c = eval(&fx, logo, true);
    assert_scalar(&c, ChannelKind::Roughness, right, RES / 2, 0.3);

    // Grayscale image: raw values (no sRGB decode) become the mask.
    let c = eval(&fx, gray, false);
    assert_scalar(&c, ChannelKind::Roughness, left, RES / 2, 0.3 + 0.7 * 64.0 / 255.0);
    assert_scalar(&c, ChannelKind::Roughness, right, RES / 2, 0.3 + 0.7 * 192.0 / 255.0);
    let c = eval(&fx, gray, true);
    assert_scalar(&c, ChannelKind::Roughness, left, RES / 2, 0.3 + 0.7 * (1.0 - 64.0 / 255.0));

    // A resource missing from the store contributes nothing.
    let c = eval(&fx, ResourceId::new(), false);
    assert_scalar(&c, ChannelKind::Roughness, right, RES / 2, 0.3);
}

#[test]
fn fill_channel_strength_reaches_the_composite() {
    let fx = Fixture::plane();
    let mut ts = texture_set();
    let mut l = fill_layer(&ts, &[(ChannelKind::Roughness, ChannelValue::scalar(0.5)), (ChannelKind::Height, ChannelValue::scalar(0.2))]);
    if let LayerContent::Fill(f) = &mut l.content {
        f.channels.get_mut(&ChannelKind::Roughness).unwrap().strength = 1.5;
        f.channels.get_mut(&ChannelKind::Height).unwrap().strength = 3.0;
    }
    ts.insert_above(None, l);
    let c = composite_full(&ts, &fx.ctx());
    assert_scalar(&c, ChannelKind::Roughness, 10, 10, 0.75);
    assert_scalar(&c, ChannelKind::Height, 10, 10, 0.6);
}
