use std::sync::Arc;

use glam::Vec2;
use subcraft_core::{ChannelValue, MeshMapKind, ResourceId};
use subcraft_doc::{FillSource, ProceduralKind, ProceduralParams, Projection, ProjectionMode, Resource, ResourceKind, ResourceStore};
use subcraft_image::{Bitmap, TILE_TEXELS, TiledImage};
use subcraft_mesh::primitives;

use super::*;
use crate::pattern;

fn ctx_parts() -> (ResourceStore, MeshMaps) {
    (ResourceStore::default(), MeshMaps::new())
}

/// Index of the tile containing the most covered texels whose normal is `n`.
fn tile_facing(geo: &TextureSetGeometry, n: [f32; 3]) -> Arc<TileGeometry> {
    (0..geo.tile_count())
        .map(|t| geo.tile(t))
        .max_by_key(|g| (0..TILE_TEXELS).filter(|&i| g.coverage(i) > 0.0 && g.normal(i) == n).count())
        .unwrap()
}

#[test]
fn uniform_is_exact() {
    let mesh = Arc::new(primitives::cube(2.0));
    let geo = TextureSetGeometry::new(mesh, 0, 256);
    let (res, maps) = ctx_parts();
    let ctx = SampleContext { resources: &res, mesh_maps: &maps };
    let v = ChannelValue::rgb(0.123, 0.456, 0.789);
    let mut out = vec![[0.0; 4]; TILE_TEXELS];
    for t in 0..geo.tile_count() {
        eval_fill_tile(&FillSource::uniform(v), true, &Projection::default(), &geo.tile(t), &ctx, &mut out);
        assert!(out.iter().all(|o| *o == [0.123, 0.456, 0.789, 1.0]));
    }
}

#[test]
fn uv_procedural_matches_pattern_at_transformed_uv() {
    let mesh = Arc::new(primitives::cube(2.0));
    let geo = TextureSetGeometry::new(mesh, 0, 256);
    let (res, maps) = ctx_parts();
    let ctx = SampleContext { resources: &res, mesh_maps: &maps };
    let params = ProceduralParams::new(ProceduralKind::Cells);
    let proj = Projection { scale: [2.0, 3.0], rotation_deg: 30.0, offset: [0.1, -0.2], ..Projection::default() };
    let (low, high) = (ChannelValue::scalar(0.2), ChannelValue::scalar(0.9));
    let src = FillSource::Procedural { params, low, high };
    let mut out = vec![[0.0; 4]; TILE_TEXELS];
    let g = geo.tile(5);
    eval_fill_tile(&src, false, &proj, &g, &ctx, &mut out);
    for (i, o) in out.iter().enumerate() {
        let p = proj.transform_2d(g.uv(i));
        let t = pattern::eval(&params, Vec2::new(p[0], p[1]));
        let expect = 0.2 + (0.9 - 0.2) * t;
        assert!((o[0] - expect).abs() < 1e-6, "texel {i}: {} vs {expect}", o[0]);
        assert_eq!(o[3], 1.0);
    }
}

#[test]
fn triplanar_on_cube_face_equals_planar_sample() {
    let mesh = Arc::new(primitives::cube(2.0));
    let geo = TextureSetGeometry::new(mesh, 0, 256);
    let (res, maps) = ctx_parts();
    let ctx = SampleContext { resources: &res, mesh_maps: &maps };
    let src = FillSource::Procedural {
        params: ProceduralParams::new(ProceduralKind::Clouds),
        low: ChannelValue::scalar(0.0),
        high: ChannelValue::scalar(1.0),
    };
    let tri = Projection { mode: ProjectionMode::Triplanar, scale: [3.0, 3.0], ..Projection::default() };
    for (axis, n) in [(0u8, [1.0, 0.0, 0.0]), (1, [0.0, 1.0, 0.0]), (2, [0.0, 0.0, 1.0])] {
        let planar = Projection { mode: ProjectionMode::Planar, planar_axis: axis, ..tri };
        let g = tile_facing(&geo, n);
        let mut a = vec![[0.0; 4]; TILE_TEXELS];
        let mut b = vec![[0.0; 4]; TILE_TEXELS];
        eval_fill_tile(&src, false, &tri, &g, &ctx, &mut a);
        eval_fill_tile(&src, false, &planar, &g, &ctx, &mut b);
        let mut checked = 0;
        for i in 0..TILE_TEXELS {
            if g.coverage(i) > 0.0 && g.normal(i) == n {
                assert!((a[i][0] - b[i][0]).abs() < 1e-6, "axis {axis} texel {i}");
                checked += 1;
            }
        }
        assert!(checked > 500, "axis {axis}: only {checked} texels on the face");
    }
}

#[test]
fn bitmap_linearizes_only_color_channels() {
    let mesh = Arc::new(primitives::plane(1.0, 1));
    let geo = TextureSetGeometry::new(mesh, 0, 128);
    let mut res = ResourceStore::default();
    let id = ResourceId::new();
    res.insert(Resource {
        id,
        name: "gray".into(),
        kind: ResourceKind::Texture,
        bitmap: Arc::new(Bitmap::new_u8(2, 2, 3, vec![128; 12])),
        source_path: None,
        encoded: None,
    });
    let maps = MeshMaps::new();
    let ctx = SampleContext { resources: &res, mesh_maps: &maps };
    let src = FillSource::Bitmap { resource: id };
    let mut color = vec![[0.0; 4]; TILE_TEXELS];
    let mut data = vec![[0.0; 4]; TILE_TEXELS];
    eval_fill_tile(&src, true, &Projection::default(), &geo.tile(0), &ctx, &mut color);
    eval_fill_tile(&src, false, &Projection::default(), &geo.tile(0), &ctx, &mut data);
    let enc = 128.0 / 255.0;
    assert!((data[0][0] - enc).abs() < 1e-6);
    assert!((color[0][0] - subcraft_core::color::srgb_to_linear(enc)).abs() < 1e-6);
    // Missing resource: transparent.
    eval_fill_tile(&FillSource::Bitmap { resource: ResourceId::new() }, true, &Projection::default(), &geo.tile(0), &ctx, &mut color);
    assert!(color.iter().all(|c| c[3] == 0.0));
}

#[test]
fn mesh_map_reads_caller_values_or_neutral() {
    let mesh = Arc::new(primitives::plane(1.0, 1));
    let geo = TextureSetGeometry::new(mesh, 0, 128);
    let res = ResourceStore::default();
    let mut maps = MeshMaps::new();
    let mut ao = TiledImage::new(128, 128, 1, [1.0; 4]);
    ao.set(3, 2, [0.25; 4]);
    maps.insert(MeshMapKind::AmbientOcclusion, ao);
    let ctx = SampleContext { resources: &res, mesh_maps: &maps };
    let mut out = vec![[0.0; 4]; TILE_TEXELS];
    eval_fill_tile(&FillSource::MeshMap { map: MeshMapKind::AmbientOcclusion }, false, &Projection::default(), &geo.tile(0), &ctx, &mut out);
    assert_eq!(out[2 * 64 + 3], [0.25, 0.25, 0.25, 1.0]);
    assert_eq!(out[0], [1.0, 1.0, 1.0, 1.0]);
    eval_fill_tile(&FillSource::MeshMap { map: MeshMapKind::Curvature }, false, &Projection::default(), &geo.tile(0), &ctx, &mut out);
    assert_eq!(out[0], [0.5, 0.5, 0.5, 1.0]);
}

#[test]
fn geometry_positions_are_normalized_and_cached() {
    let mesh = Arc::new(primitives::cube(2.0));
    let cache = GeometryCache::new(mesh);
    let a = cache.get(0, 256);
    assert!(Arc::ptr_eq(&a, &cache.get(0, 256)));
    assert!(!Arc::ptr_eq(&a, &cache.get(0, 512)));
    let mut covered = 0;
    for t in 0..a.tile_count() {
        let g = a.tile(t);
        assert!(Arc::ptr_eq(&g, &a.tile(t)));
        for i in 0..TILE_TEXELS {
            if g.coverage(i) > 0.0 {
                covered += 1;
                let p = g.position(i);
                assert!(p.iter().all(|&c| (-1e-5..=1.0 + 1e-5).contains(&c)), "{p:?}");
                // Cube: every surface point has one coordinate at 0 or 1.
                assert!(p.iter().any(|&c| c.abs() < 1e-4 || (c - 1.0).abs() < 1e-4));
            }
        }
    }
    assert!(covered > 256 * 256 / 2);
}

#[test]
fn procedural_fields_are_cached_exactly_per_parameter_set() {
    let geo = TextureSetGeometry::new(Arc::new(primitives::cube(2.0)), 0, 128);
    let g = geo.tile(1);
    let params = ProceduralParams::new(ProceduralKind::Clouds);
    let tri = Projection { mode: ProjectionMode::Triplanar, ..Projection::default() };
    let a = procedural_tile(&params, &tri, &g);
    // Exactly the per-texel evaluation.
    for i in 0..TILE_TEXELS {
        assert_eq!(a[i].to_bits(), eval_procedural(&params, &tri, &g, i).to_bits());
    }
    // Reused for the same parameters; any parameter bit gives another field.
    assert!(Arc::ptr_eq(&a, &procedural_tile(&params, &tri, &g)));
    let other = ProceduralParams { balance: 0.51, ..params };
    let b = procedural_tile(&other, &tri, &g);
    assert!(!Arc::ptr_eq(&a, &b));
    assert_ne!(&a[..], &b[..]);
    let rotated = Projection { rotation_deg: 10.0, ..tri };
    assert!(!Arc::ptr_eq(&a, &procedural_tile(&params, &rotated, &g)));
    // At most PER_TILE fields per tile; a field still in use is never pushed out by an extra one
    // (that one is computed uncached), but one unused for STALE_AFTER reads is replaced.
    let extra = |seed: u32| ProceduralParams { seed: 100 + seed, ..params };
    for seed in 0..field::PER_TILE as u32 + 3 {
        procedural_tile(&extra(seed), &tri, &g);
    }
    assert_eq!(g.fields.len(), field::PER_TILE);
    assert!(Arc::ptr_eq(&a, &procedural_tile(&params, &tri, &g)), "recently used: kept");
    let overflow = extra(field::PER_TILE as u32 + 2);
    assert!(!Arc::ptr_eq(&procedural_tile(&overflow, &tri, &g), &procedural_tile(&overflow, &tri, &g)), "no room: uncached");
    // Keep reading `a` only: every other field goes stale and the next new one takes a slot.
    for _ in 0..field::STALE_AFTER + 1 {
        procedural_tile(&params, &tri, &g);
    }
    let fresh = procedural_tile(&overflow, &tri, &g);
    assert!(Arc::ptr_eq(&fresh, &procedural_tile(&overflow, &tri, &g)), "replaced a stale field");
    assert!(Arc::ptr_eq(&a, &procedural_tile(&params, &tri, &g)));
    assert_eq!(g.fields.len(), field::PER_TILE);
}

/// A 40-layer stack of procedural fills reads about 80 fields per tile (base color and roughness
/// patterns per layer): all of them stay cached, so an edit below them re-blends without
/// recomputing any noise. With 24 fields per tile, 56 of the 80 were recomputed on every edit.
#[test]
fn a_forty_layer_stack_keeps_every_field_of_a_tile() {
    let geo = TextureSetGeometry::new(Arc::new(primitives::cube(2.0)), 0, 128);
    let g = geo.tile(2);
    let tri = Projection { mode: ProjectionMode::Triplanar, ..Projection::default() };
    let pass = || -> Vec<Arc<field::Field>> {
        (0..80u32).map(|s| procedural_tile(&ProceduralParams { seed: 1000 + s, scale: 2.0 + (s % 5) as f32, ..ProceduralParams::new(ProceduralKind::Clouds) }, &tri, &g)).collect()
    };
    let first = pass();
    let second = pass();
    assert!(first.iter().zip(&second).all(|(a, b)| Arc::ptr_eq(a, b)), "every field of the stack hits on the next pass");
    assert_eq!(g.fields.len(), 80);
}

/// The memory share for cached fields: a twelfth of RAM, never below the default, and enough on a
/// 64 GB machine for a 4096² 40-layer stack (about 5 GB) instead of stopping at 4 GiB.
#[test]
fn field_budget_follows_machine_memory() {
    let gb = |g: u64| g << 30;
    assert_eq!(field::budget_for_memory(gb(4)), field::BUDGET_BYTES, "small machines keep the default");
    assert_eq!(field::budget_for_memory(gb(16)), (gb(16) / 12) as usize);
    assert_eq!(field::budget_for_memory(gb(32)), (gb(32) / 12) as usize, "unchanged below 48 GB");
    assert!(field::budget_for_memory(gb(64)) > 5 << 30);
    assert_eq!(field::budget_for_memory(gb(256)), 8 << 30, "capped");
}

/// A stack needing more fields per tile than fit, read in the same order on every pass (one pass
/// per slider step), keeps hitting on the fields it holds instead of missing on every read.
#[test]
fn cyclic_field_reads_beyond_capacity_do_not_thrash() {
    let geo = TextureSetGeometry::new(Arc::new(primitives::cube(2.0)), 0, 128);
    let g = geo.tile(1);
    let params = ProceduralParams::new(ProceduralKind::Clouds);
    let tri = Projection { mode: ProjectionMode::Triplanar, ..Projection::default() };
    let n = field::PER_TILE as u32 + 5;
    let pass = || -> Vec<Arc<field::Field>> { (0..n).map(|s| procedural_tile(&ProceduralParams { seed: s, ..params }, &tri, &g)).collect() };
    let first = pass();
    let second = pass();
    let hits = first.iter().zip(&second).filter(|(a, b)| Arc::ptr_eq(a, b)).count();
    assert_eq!(hits, field::PER_TILE, "every cached field hits again");
    // Exactness does not depend on caching.
    for (a, b) in first.iter().zip(&second) {
        assert_eq!(&a[..], &b[..]);
    }
}

#[test]
fn a_stack_with_a_dozen_fields_per_tile_keeps_them_all() {
    // Five smart materials with triplanar fills use well over 8 fields per tile; every one must
    // still be cached on the next pass (an 8-entry LRU dropped each before its reuse).
    let geo = TextureSetGeometry::new(Arc::new(primitives::cube(2.0)), 0, 128);
    let g = geo.tile(2);
    let tri = Projection { mode: ProjectionMode::Triplanar, ..Projection::default() };
    let params: Vec<_> = (0..16u32).map(|seed| ProceduralParams { seed: 500 + seed, ..ProceduralParams::new(ProceduralKind::Grunge) }).collect();
    let first: Vec<_> = params.iter().map(|p| procedural_tile(p, &tri, &g)).collect();
    for (p, a) in params.iter().zip(&first) {
        let b = procedural_tile(p, &tri, &g);
        assert!(Arc::ptr_eq(a, &b), "field recomputed although it fits");
        for i in (0..TILE_TEXELS).step_by(97) {
            assert_eq!(b[i].to_bits(), eval_procedural(p, &tri, &g, i).to_bits());
        }
    }
}

#[test]
fn colored_procedural_fills_draw_each_component_from_its_own_pattern() {
    let geometry = TextureSetGeometry::new(Arc::new(subcraft_mesh::primitives::plane(2.0, 1)), 0, 64);
    let geo = geometry.tile(0);
    let ctx = SampleContext { resources: &ResourceStore::default(), mesh_maps: &MeshMaps::default() };
    let params = ProceduralParams { scale: 16.0, ..ProceduralParams::new(ProceduralKind::Clouds) };
    let (low, high) = (ChannelValue::rgb(0.25, 0.25, 0.25), ChannelValue::rgb(0.75, 0.75, 0.75));
    let mut gray = vec![[0.0f32; 4]; TILE_TEXELS];
    let mut colored = vec![[0.0f32; 4]; TILE_TEXELS];
    eval_fill_tile(&FillSource::Procedural { params, low, high }, false, &Projection::default(), &geo, &ctx, &mut gray);
    eval_fill_tile(&FillSource::Procedural { params: ProceduralParams { colored: true, ..params }, low, high }, false, &Projection::default(), &geo, &ctx, &mut colored);
    let covered: Vec<usize> = (0..TILE_TEXELS).filter(|&i| geo.triangle(i).is_some()).collect();
    assert!(covered.len() > 1000);
    // Grayscale: equal components. Colored: the red component is the grayscale pattern (same seed),
    // the others differ, all within low..high.
    assert!(covered.iter().all(|&i| gray[i][0] == gray[i][1] && gray[i][1] == gray[i][2]));
    assert!(covered.iter().all(|&i| colored[i][0] == gray[i][0]));
    let differ = covered.iter().filter(|&&i| (colored[i][1] - colored[i][0]).abs() > 0.02 && (colored[i][2] - colored[i][1]).abs() > 0.02).count();
    assert!(differ * 2 > covered.len(), "components should vary independently: {differ} of {}", covered.len());
    assert!(covered.iter().all(|&i| (0..3).all(|c| (0.25..=0.75).contains(&colored[i][c]))));
}


// ---- Fill projection options: Filtering, UV Wrap, Shape crop, tri-planar Hardness ----

fn store_with(bmp: Bitmap) -> (ResourceStore, ResourceId) {
    let mut res = ResourceStore::default();
    let id = ResourceId::new();
    res.insert(Resource { id, name: "t".into(), kind: ResourceKind::Texture, bitmap: Arc::new(bmp), source_path: None, encoded: None });
    (res, id)
}

/// A one-texel black / white checker.
fn checker(size: u32) -> Bitmap {
    Bitmap::new_u8(size, size, 1, (0..size * size).map(|i| if (i % size + i / size) % 2 == 0 { 0 } else { 255 }).collect())
}

fn eval_bitmap(res: &ResourceStore, id: ResourceId, proj: &Projection, g: &TileGeometry) -> Vec<[f32; 4]> {
    let maps = MeshMaps::new();
    let ctx = SampleContext { resources: res, mesh_maps: &maps };
    let mut out = vec![[0.0; 4]; TILE_TEXELS];
    eval_fill_tile(&FillSource::Bitmap { resource: id }, false, proj, g, &ctx, &mut out);
    out
}

#[test]
fn hardness_one_cuts_to_the_dominant_plane_and_lower_values_blend() {
    use subcraft_doc::TRIPLANAR_HARD;
    let n = [0.5f32, 0.6, 0.62449980];
    assert_eq!(triplanar_weights(n, TRIPLANAR_HARD), [0.0, 0.0, 1.0]);
    assert_eq!(triplanar_weights([0.0, -1.0, 0.0], TRIPLANAR_HARD), [0.0, 1.0, 0.0]);
    let d = 1.0 / 3f32.sqrt();
    assert_eq!(triplanar_weights([d, d, d], TRIPLANAR_HARD), [1.0 / 3.0; 3], "a tie shares the planes");
    assert_eq!(triplanar_weights([0.0; 3], TRIPLANAR_HARD), [1.0 / 3.0; 3], "gutter texels");
    // Hardness 0 is the exponent 1: weights proportional to |n|.
    let mut p = Projection::default();
    p.set_hardness(0.0);
    let w = triplanar_weights(n, p.triplanar_sharpness);
    let sum = n[0] + n[1] + n[2];
    for a in 0..3 {
        assert!((w[a] - n[a] / sum).abs() < 1e-6, "{w:?}");
    }
    // Hardness 0.5 is the exponent 2: weights are the squared components of a unit normal.
    p.set_hardness(0.5);
    let w = triplanar_weights(n, p.triplanar_sharpness);
    for a in 0..3 {
        assert!((w[a] - n[a] * n[a]).abs() < 1e-5, "{w:?}");
    }
    // Close to 1 the plain powers would underflow to 0 / 0; the weights stay finite, sum to 1
    // and approach the hard cut monotonically.
    let mut last = w[2];
    for h in [0.9, 0.99, 0.995, 0.999, 0.9999] {
        p.set_hardness(h);
        let w = triplanar_weights(n, p.triplanar_sharpness);
        assert!(w.iter().all(|x| x.is_finite()) && (w[0] + w[1] + w[2] - 1.0).abs() < 1e-5, "{h}: {w:?}");
        assert!(w[2] >= last, "{h}: {w:?}");
        last = w[2];
    }
    assert!(last > 0.999);
    // The exponents existing projects store (1..=32) keep their exact weights.
    let w8 = triplanar_weights(n, 8.0);
    let raw = n.map(|c| c.powf(8.0));
    let s = raw[0] + raw[1] + raw[2];
    assert_eq!(w8, raw.map(|c| c / s));
}

/// QA 2026-11: between exponent 32 and 64 (Hardness about 0.97-0.984) the plain powers of a
/// near-diagonal normal summed to under 1e-12 and the weights fell back to an even 1/3 split.
#[test]
fn high_hardness_keeps_the_dominant_plane_for_near_diagonal_normals() {
    let len = (0.6f32 * 0.6 + 0.57 * 0.57 + 0.565 * 0.565).sqrt();
    let n = [0.6 / len, 0.57 / len, 0.565 / len];
    for s in [40.0, 52.0, 60.0, 64.0] {
        let w = triplanar_weights(n, s);
        assert!(w[0] > w[1] && w[1] > w[2], "exponent {s}: {w:?}");
        assert!((w[0] + w[1] + w[2] - 1.0).abs() < 1e-5, "exponent {s}: {w:?}");
        assert!(w[0] > 0.8, "exponent {s}: the largest component dominates, not an even split: {w:?}");
    }
}

#[test]
fn mip_chain_averages_in_the_space_the_texture_is_used_in() {
    use crate::sample::MipChain;
    // 4x2 gray: columns 0, 255, 100, 200.
    let bmp = Bitmap::new_u8(4, 2, 1, vec![0, 255, 100, 200, 0, 255, 100, 200]);
    let raw = MipChain::build(&bmp, false);
    assert_eq!(raw.len(), 2, "4x2 -> 2x1 -> 1x1");
    assert_eq!((raw.level(1).width, raw.level(1).height), (2, 1));
    assert_eq!(raw.level(1).texel(0, 0, false)[0], 128.0 / 255.0, "(0 + 255) / 2 = 127.5 rounds up");
    assert_eq!(raw.level(1).texel(1, 0, false)[0], 150.0 / 255.0);
    assert_eq!((raw.level(2).width, raw.level(2).height), (1, 1));
    assert_eq!(raw.level(2).texel(0, 0, false)[0], 139.0 / 255.0, "mean of 0, 255, 100, 200 = 138.75");
    // Color: black + white average to linear 0.5, stored sRGB-encoded (188), not to sRGB 128.
    let lin = MipChain::build(&bmp, true);
    let v = lin.level(1).texel(0, 0, true)[0];
    assert!((v - 0.5).abs() < 4e-3, "{v}");
    assert_eq!(lin.level(1).texel(0, 0, false)[0], 188.0 / 255.0);
    // Alpha is averaged as stored, never color-decoded.
    let rgba = Bitmap::new_u8(2, 1, 4, vec![255, 0, 0, 0, 255, 0, 0, 255]);
    let t = MipChain::build(&rgba, true).level(1).texel(0, 0, true);
    assert_eq!(t[3], 128.0 / 255.0);
    assert!((t[0] - 1.0).abs() < 1e-6 && t[1] == 0.0);
    // Float data keeps its values.
    let f = Bitmap::new_f32(2, 2, 1, vec![0.0, 4.0, 8.0, 12.0]);
    assert_eq!(MipChain::build(&f, false).level(1).texel(0, 0, false)[0], 6.0);
    // The chain of a shared bitmap is built once.
    let shared = Arc::new(checker(8));
    assert!(Arc::ptr_eq(&crate::sample::mip_chain(&shared, false), &crate::sample::mip_chain(&shared, false)));
    assert!(!Arc::ptr_eq(&crate::sample::mip_chain(&shared, false), &crate::sample::mip_chain(&shared, true)));
}

#[test]
fn filtering_hq_averages_a_minified_texture_sharp_and_nearest_do_not() {
    use subcraft_doc::FillFiltering;
    let geo = TextureSetGeometry::new(Arc::new(primitives::plane(2.0, 1)), 0, 128);
    let g = geo.tile(0);
    let (res, id) = store_with(checker(64));
    // 64 px image tiled 8 times on 128 texels: 4 source texels per texel = mip level 2, where a
    // one-texel checker is uniform mid gray (127.5 stored as 128).
    let tiled = |filtering| Projection { scale: [8.0, 8.0], filtering, ..Projection::default() };
    let hq = eval_bitmap(&res, id, &tiled(FillFiltering::BilinearHq), &g);
    assert!(hq.iter().all(|o| (o[0] - 128.0 / 255.0).abs() < 1e-6 && o[3] == 1.0), "{:?}", &hq[..4]);
    // Sharp bilinear-samples the full-size image. Tiling 7 steps 3.5 source texels per texel, so
    // the sample phase alternates between 0.25 and 0.75 texel on both axes: bilinear mixes of
    // 0.375 and 0.625, never the 0.5 average. (Tiling 8 would land every sample halfway between
    // two rows, where even point sampling of a checker reads 0.5.)
    let shifted = |filtering| Projection { scale: [7.0, 7.0], filtering, ..Projection::default() };
    let sharp = eval_bitmap(&res, id, &shifted(FillFiltering::BilinearSharp), &g);
    let (lo, hi) = sharp.iter().fold((1.0f32, 0.0f32), |(lo, hi), o| (lo.min(o[0]), hi.max(o[0])));
    assert!(hi - lo > 0.2, "sharp keeps the checker contrast (aliasing): {lo}..{hi}");
    let hq_shifted = eval_bitmap(&res, id, &shifted(FillFiltering::BilinearHq), &g);
    assert!(hq_shifted.iter().all(|o| (o[0] - 128.0 / 255.0).abs() < 1e-6));
    // Nearest only ever returns texel values.
    let near = eval_bitmap(&res, id, &shifted(FillFiltering::Nearest), &g);
    assert!(near.iter().all(|o| o[0] == 0.0 || o[0] == 1.0));
    assert!(near.iter().any(|o| o[0] == 0.0) && near.iter().any(|o| o[0] == 1.0));
    // Level 1 exactly (2 source texels per texel): also uniform for this checker; and between
    // levels (tiling 6 = level 1.585) the trilinear mix of two uniform levels stays uniform.
    for tiling in [4.0, 6.0] {
        let p = Projection { scale: [tiling, tiling], ..Projection::default() };
        let out = eval_bitmap(&res, id, &p, &g);
        assert!(out.iter().all(|o| (o[0] - 128.0 / 255.0).abs() < 1e-6), "tiling {tiling}");
    }
    // Magnified (or 1:1) HQ is bit-identical to Sharp: mipmaps only matter when minifying.
    for tiling in [1.0, 2.0, 0.37] {
        let p = |filtering| Projection { scale: [tiling, tiling], rotation_deg: 20.0, offset: [0.13, 0.4], filtering, ..Projection::default() };
        assert_eq!(eval_bitmap(&res, id, &p(FillFiltering::BilinearHq), &g), eval_bitmap(&res, id, &p(FillFiltering::BilinearSharp), &g), "tiling {tiling}");
    }
}

#[test]
fn hq_filtering_follows_the_surface_scale_in_3d_projections() {
    use subcraft_doc::FillFiltering;
    // Cube faces span the whole normalized 0..1 range over roughly a quarter of the texture, so
    // a tri-planar 64 px checker tiled 32 times is far below one texel per source texel.
    let geo = TextureSetGeometry::new(Arc::new(primitives::cube(2.0)), 0, 128);
    let (res, id) = store_with(checker(64));
    let g = tile_facing(&geo, [0.0, 0.0, 1.0]);
    let tri = |filtering| Projection { mode: ProjectionMode::Triplanar, scale: [32.0, 32.0], filtering, ..Projection::default() };
    let hq = eval_bitmap(&res, id, &tri(FillFiltering::BilinearHq), &g);
    let sharp = eval_bitmap(&res, id, &tri(FillFiltering::BilinearSharp), &g);
    let face: Vec<usize> = (0..TILE_TEXELS).filter(|&i| g.coverage(i) > 0.0 && g.normal(i) == [0.0, 0.0, 1.0]).collect();
    assert!(face.len() > 500);
    let spread = |px: &[[f32; 4]]| {
        let (lo, hi) = face.iter().fold((1.0f32, 0.0f32), |(lo, hi), &i| (lo.min(px[i][0]), hi.max(px[i][0])));
        hi - lo
    };
    assert!(spread(&hq) < 0.02, "HQ averages: spread {}", spread(&hq));
    assert!(spread(&sharp) > 0.3, "Sharp aliases: spread {}", spread(&sharp));
}

#[test]
fn uv_wrap_limits_the_texture_to_its_square_on_axes_that_do_not_repeat() {
    use subcraft_doc::UvWrap;
    let geo = TextureSetGeometry::new(Arc::new(primitives::plane(2.0, 1)), 0, 128);
    let (res, id) = store_with(Bitmap::new_u8(2, 2, 4, vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255]));
    let maps = MeshMaps::new();
    let ctx = SampleContext { resources: &res, mesh_maps: &maps };
    // Tiling 2: the texture's own square is the top-left quarter of the texture set.
    for wrap in UvWrap::ALL {
        let proj = Projection { scale: [2.0, 2.0], uv_wrap: wrap, ..Projection::default() };
        let repeat = Projection { scale: [2.0, 2.0], ..Projection::default() };
        let params = ProceduralParams::new(ProceduralKind::Cells);
        let procedural = FillSource::Procedural { params, low: ChannelValue::scalar(0.0), high: ChannelValue::scalar(1.0) };
        for t in 0..geo.tile_count() {
            let g = geo.tile(t);
            let out = eval_bitmap(&res, id, &proj, &g);
            let full = eval_bitmap(&res, id, &repeat, &g);
            let mut pattern = vec![[0.0; 4]; TILE_TEXELS];
            let mut pattern_full = vec![[0.0; 4]; TILE_TEXELS];
            eval_fill_tile(&procedural, false, &proj, &g, &ctx, &mut pattern);
            eval_fill_tile(&procedural, false, &repeat, &g, &ctx, &mut pattern_full);
            for i in 0..TILE_TEXELS {
                let uv = g.uv(i);
                let [rx, ry] = wrap.repeats();
                let inside = (rx || uv[0] < 0.5) && (ry || uv[1] < 0.5);
                if !inside {
                    assert_eq!(out[i], [0.0; 4], "{wrap:?} tile {t} texel {i}: outside is transparent");
                    assert_eq!(pattern[i][3], 0.0, "{wrap:?}: a procedural is clipped the same way");
                    continue;
                }
                assert_eq!(out[i][3], 1.0);
                assert_eq!(pattern[i], pattern_full[i], "a covered procedural texel is unchanged");
                // Away from the square's border the sample is the repeating one; at the border a
                // non-repeating axis clamps to its edge texel instead of blending with the far side.
                let edge = |v: f32| (v * 2.0 - 0.5).abs() > 0.25;
                if (rx || !edge(uv[0])) && (ry || !edge(uv[1])) {
                    for c in 0..4 {
                        assert!((out[i][c] - full[i][c]).abs() < 1e-6, "{wrap:?} texel {i}: {:?} vs {:?}", out[i], full[i]);
                    }
                }
            }
        }
    }
    // The clamped border of UV Wrap None: the leftmost column is pure texel 0 (red), where
    // Repeat blends it half-way with the texture's right edge (green).
    let g = geo.tile(0);
    let none = eval_bitmap(&res, id, &Projection { uv_wrap: UvWrap::None, ..Projection::default() }, &g);
    let rep = eval_bitmap(&res, id, &Projection::default(), &g);
    assert_eq!(none[0], [1.0, 0.0, 0.0, 1.0]);
    assert!(rep[0][1] > 0.4, "{:?}", rep[0]);
    // Nearest honours the wrap too.
    let near = Projection { scale: [2.0, 2.0], uv_wrap: UvWrap::None, filtering: subcraft_doc::FillFiltering::Nearest, ..Projection::default() };
    let last = geo.tile(geo.tile_count() - 1);
    assert!(eval_bitmap(&res, id, &near, &last).iter().all(|o| *o == [0.0; 4]));
    assert_eq!(eval_bitmap(&res, id, &near, &g)[0], [1.0, 0.0, 0.0, 1.0]);
}

#[test]
fn shape_crop_drops_what_lies_outside_the_projection_box() {
    use subcraft_doc::{ShapeCrop, UvWrap};
    let sc = (0.0, 1.0);
    let inside = [0.5, 0.25, 1.0];
    let outside = [0.5, 1.2, 0.5];
    for mode in [ProjectionMode::Triplanar, ProjectionMode::Planar] {
        let crop = Projection { mode, shape_crop: ShapeCrop::Crop, ..Projection::default() };
        let extend = Projection { mode, ..Projection::default() };
        assert!(projection_covers(&crop, sc, [0.0; 2], inside), "{mode:?}");
        assert!(!projection_covers(&crop, sc, [0.0; 2], outside), "{mode:?}");
        assert!(projection_covers(&extend, sc, [0.0; 2], outside), "{mode:?}: the default extends");
    }
    // UV projection has no shape; tri-planar has no UV Wrap.
    assert!(projection_covers(&Projection { shape_crop: ShapeCrop::Crop, ..Projection::default() }, sc, [0.5; 2], outside));
    assert!(projection_covers(&Projection { mode: ProjectionMode::Triplanar, uv_wrap: UvWrap::None, scale: [9.0; 2], ..Projection::default() }, sc, [0.5; 2], inside));
    // Planar + UV Wrap None: only the texture's square on the projection plane (Z axis: x, 1 - y).
    let planar = Projection { mode: ProjectionMode::Planar, scale: [2.0, 2.0], uv_wrap: UvWrap::None, ..Projection::default() };
    assert!(projection_covers(&planar, sc, [0.0; 2], [0.25, 0.75, 0.3]));
    assert!(!projection_covers(&planar, sc, [0.0; 2], [0.75, 0.75, 0.3]));
    // A model always lies inside its own bounding cube: cropping to it changes no texel, and
    // nothing is clipped at the defaults (no coverage pass at all).
    let geo = TextureSetGeometry::new(Arc::new(primitives::cube(2.0)), 0, 128);
    let crop = Projection { mode: ProjectionMode::Triplanar, shape_crop: ShapeCrop::Crop, ..Projection::default() };
    for t in 0..geo.tile_count() {
        let g = geo.tile(t);
        assert!(coverage_tile(&Projection { mode: ProjectionMode::Triplanar, ..Projection::default() }, &g).is_none());
        assert!(coverage_tile(&crop, &g).unwrap().iter().all(|&c| c == 1.0));
    }
}

#[test]
fn spherical_cylindrical_and_symmetric_projections_sample_folded_coordinates() {
    use subcraft_doc::FillSymmetry;
    // The sampler returns the 2D coordinate it was asked for.
    let probe = |p: &Projection, pos: [f32; 3]| {
        let s = sample_projected(p, [0.0; 2], pos, [0.0, 1.0, 0.0], |q| [q[0], q[1], 0.0, 1.0]);
        [s[0], s[1]]
    };
    let near = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5;
    let sphere = Projection { mode: ProjectionMode::Spherical, planar_axis: 1, ..Projection::default() };
    assert!(near(probe(&sphere, [1.0, 0.5, 0.5]), [0.75, 0.5]));
    // Tiling scales the wrapped coordinate like any other projection.
    assert!(near(probe(&Projection { scale: [4.0, 2.0], ..sphere }, [1.0, 0.5, 0.5]), [3.0, 1.0]));
    let cylinder = Projection { mode: ProjectionMode::Cylindrical, planar_axis: 1, cylinder_angle: 180.0, ..Projection::default() };
    assert!(near(probe(&cylinder, [1.0, 0.25, 0.5]), [1.0, 0.75]));
    // Mirror symmetry: both sides of the plane sample the same coordinate.
    let mirrored = Projection { mode: ProjectionMode::Planar, planar_axis: 2, symmetry: FillSymmetry::mirror(), ..Projection::default() };
    assert!(near(probe(&mirrored, [0.2, 0.3, 0.5]), probe(&mirrored, [0.8, 0.3, 0.5])));
    let plain = Projection { symmetry: FillSymmetry::None, ..mirrored };
    assert!(!near(probe(&plain, [0.2, 0.3, 0.5]), probe(&plain, [0.8, 0.3, 0.5])));
    // On a UV projection symmetry is ignored.
    let uv = Projection { symmetry: FillSymmetry::mirror(), ..Projection::default() };
    assert!(near(sample_projected(&uv, [0.2, 0.7], [0.2, 0.3, 0.5], [0.0, 1.0, 0.0], |q| [q[0], q[1], 0.0, 1.0])[..2].try_into().unwrap(), [0.2, 0.7]));
    // UV Wrap None clips a half-perimeter cylinder to the half facing its seam's opposite side.
    let clipped = Projection { uv_wrap: subcraft_doc::UvWrap::None, ..cylinder };
    let sc = clipped.rotation_sin_cos();
    assert!(crate::sample::projection_covers(&clipped, sc, [0.0; 2], [0.5, 0.5, 1.0]));
    assert!(!crate::sample::projection_covers(&clipped, sc, [0.0; 2], [0.5, 0.5, 0.0]));
}

/// Warp projection: untouched it is Planar, it projects only inside its grid, and a moved point
/// changes what a texel reads.
#[test]
fn warp_projection_is_planar_until_bent_and_stops_at_its_grid() {
    let coords = |p: [f32; 2]| [p[0], p[1], 0.0, 1.0];
    for axis in 0..3u8 {
        let planar = Projection { mode: ProjectionMode::Planar, planar_axis: axis, scale: [1.0, 1.0], ..Projection::default() };
        let warp = Projection { mode: ProjectionMode::Warp, ..planar };
        for pos in [[0.2, 0.3, 0.4], [0.75, 0.1, 0.9], [0.5, 0.5, 0.5]] {
            let a = sample_projected(&planar, [0.0; 2], pos, [0.0, 0.0, 1.0], coords);
            let b = sample_projected(&warp, [0.0; 2], pos, [0.0, 0.0, 1.0], coords);
            assert!((a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5, "axis {axis} {pos:?}: {a:?} vs {b:?}");
        }
        assert!(warp.clips() && !planar.clips());
        let sc = warp.rotation_sin_cos();
        assert!(projection_covers(&warp, sc, [0.0; 2], [0.5, 0.5, 0.5]));
    }
    // Seen down Z, x = 1.2 is outside the grid: not covered.
    let warp = Projection { mode: ProjectionMode::Warp, planar_axis: 2, ..Projection::default() };
    assert!(!projection_covers(&warp, warp.rotation_sin_cos(), [0.0; 2], [1.2, 0.5, 0.5]));
    // Pull the point at grid (1/3, 1/3) to the right: the texel that sat at x = 0.4 reads further left.
    let mut grid = warp.warp_grid();
    let i = grid.index(1, 1);
    let p = grid.points()[i];
    assert!(grid.set_point(i, [p[0] + 0.15, p[1], p[2]]));
    let bent = Projection { warp: Some(grid), ..warp };
    let pos = [0.4, 1.0 - 1.0 / 3.0, 0.5];
    let before = sample_projected(&warp, [0.0; 2], pos, [0.0, 0.0, 1.0], coords);
    let after = sample_projected(&bent, [0.0; 2], pos, [0.0, 0.0, 1.0], coords);
    assert!((before[0] - 0.4).abs() < 1e-5 && after[0] < 0.35, "{before:?} -> {after:?}");
    assert!(projection_covers(&bent, bent.rotation_sin_cos(), [0.0; 2], pos));
}

/// QA 2026-11: the cache key of a procedural field lost most of the bits of the cylinder angle
/// and symmetry settings: angle 120 and 144 shared a key, so a fill at one angle was drawn with
/// the cached field of the other.
#[test]
fn field_keys_differ_for_every_cylinder_angle_and_mirror_position() {
    use subcraft_doc::FillSymmetry;
    let params = ProceduralParams::new(ProceduralKind::Clouds);
    let mut seen = std::collections::HashMap::new();
    for angle in 1..=359 {
        let p = Projection { mode: ProjectionMode::Cylindrical, cylinder_angle: angle as f32, ..Projection::default() };
        if let Some(other) = seen.insert(field::FieldKey::new(&params, &p), angle) {
            panic!("cylinder angles {other} and {angle} share a field key");
        }
    }
    let mirror = |position: f32| field::FieldKey::new(&params, &Projection { mode: ProjectionMode::Triplanar, symmetry: FillSymmetry::Mirror { axis: 0, position }, ..Projection::default() });
    assert_ne!(mirror(0.4), mirror(0.55));
    let mut positions = std::collections::HashSet::new();
    for i in 0..200 {
        assert!(positions.insert(mirror(i as f32 * 0.005)), "mirror position {} shares a key", i as f32 * 0.005);
    }
}

#[test]
fn projection_3d_transform_moves_the_sampled_coordinates_and_the_crop_box() {
    use subcraft_doc::{ProjectionTransform, ShapeCrop};
    let id = ProjectionTransform::default();
    let planar = |transform3, shape_crop| Projection { mode: ProjectionMode::Planar, transform3, shape_crop, ..Projection::default() };
    let coords = |p: &Projection, pos: [f32; 3]| super::projection::sample_projected(p, [0.0; 2], pos, [0.0, 0.0, 1.0], |q| [q[0], q[1], 0.0, 1.0]);
    // Planar along Z reads (x, 1 - y). Moved half a cube to the right, the box's left edge is
    // the cube's middle.
    let moved = planar(ProjectionTransform { offset: [0.5, 0.0, 0.0], ..id }, ShapeCrop::Crop);
    let q = coords(&moved, [0.75, 0.5, 0.5]);
    assert!((q[0] - 0.25).abs() < 1e-6 && (q[1] - 0.5).abs() < 1e-6, "{q:?}");
    let q = coords(&planar(id, ShapeCrop::Crop), [0.75, 0.5, 0.5]);
    assert!((q[0] - 0.75).abs() < 1e-6, "{q:?}");
    // The crop follows the box: the cube's left half is outside it now.
    let sc = moved.rotation_sin_cos();
    assert!(projection_covers(&moved, sc, [0.0; 2], [0.75, 0.5, 0.5]));
    assert!(!projection_covers(&moved, sc, [0.0; 2], [0.25, 0.5, 0.5]));
    assert!(projection_covers(&planar(ProjectionTransform { offset: [0.5, 0.0, 0.0], ..id }, ShapeCrop::default()), sc, [0.0; 2], [0.25, 0.5, 0.5]), "extends by default");
    // Scale: half as deep along Z (the planar depth), so the cube's front and back fall outside.
    let thin = planar(ProjectionTransform { scale: [1.0, 1.0, 0.5], ..id }, ShapeCrop::Crop);
    assert!(projection_covers(&thin, sc, [0.0; 2], [0.5, 0.5, 0.6]));
    assert!(!projection_covers(&thin, sc, [0.0; 2], [0.5, 0.5, 0.9]));
    // Scale 2 on X: the texture is twice as wide on the model.
    let wide = planar(ProjectionTransform { scale: [2.0, 1.0, 1.0], ..id }, ShapeCrop::default());
    let q = coords(&wide, [1.0, 0.5, 0.5]);
    assert!((q[0] - 0.75).abs() < 1e-6, "{q:?}");
    // Tri-planar follows the rotation: after a quarter turn about Z the model's +Y face reads
    // the box's X plane.
    let turned = Projection { mode: ProjectionMode::Triplanar, transform3: ProjectionTransform { rotation_deg: [0.0, 0.0, 90.0], ..id }, ..Projection::default() };
    let straight = Projection { mode: ProjectionMode::Triplanar, ..Projection::default() };
    let pos = [0.3, 1.0, 0.6];
    let a = super::projection::sample_projected(&turned, [0.0; 2], pos, [0.0, 1.0, 0.0], |q| [q[0], q[1], 0.0, 1.0]);
    let b = super::projection::sample_projected(&straight, [0.0; 2], [1.0, 0.7, 0.6], [1.0, 0.0, 0.0], |q| [q[0], q[1], 0.0, 1.0]);
    assert!((a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4, "{a:?} {b:?}");
}
