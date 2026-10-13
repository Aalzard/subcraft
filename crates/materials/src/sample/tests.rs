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
