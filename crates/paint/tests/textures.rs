//! Numeric acceptance for textured strokes, independent of the UI/GPU.
use glam::{Vec2, Vec3};
use std::sync::Arc;
use subcraft_core::{ChannelKind as K, ChannelValue, ResourceId, normal};
use subcraft_doc::{
    Layer, LayerContent, Projection, ProjectionMode, Resource, ResourceKind, ResourceStore,
    TextureSet,
};
use subcraft_image::{Bitmap, TILE_TEXELS, Wrap};
use subcraft_materials::{TextureSetGeometry, sample::sample_projected};
use subcraft_mesh::{Bvh, primitives};
use subcraft_paint::{
    BrushMaterial, BrushSettings, Camera, DepthCache, NormalPaintBlend, PaintContext, PaintTarget,
    Stroke, StrokeMode, StrokeOptions,
};

fn resources(bitmap: Bitmap) -> (ResourceStore, ResourceId) {
    let mut store = ResourceStore::default();
    let id = ResourceId::new();
    store.insert(Resource {
        id,
        name: "test.png".into(),
        kind: ResourceKind::Texture,
        bitmap: Arc::new(bitmap),
        source_path: None,
        encoded: None,
    });
    (store, id)
}

#[test]
fn projected_color_data_and_alpha_match_numeric_sampler_at_every_written_texel() {
    // A nonuniform RGBA source tests orientation, data/color interpretation and alpha together.
    let bmp = Bitmap::new_u8(
        2,
        2,
        4,
        vec![
            32, 80, 140, 64, 190, 40, 70, 128, 65, 220, 100, 192, 240, 160, 110, 255,
        ],
    );
    let (store, id) = resources(bmp.clone());
    let mesh = Arc::new(primitives::cube(2.0));
    let bvh = Bvh::build(&mesh);
    let geo = TextureSetGeometry::new(mesh.clone(), 0, 128);
    let depth = DepthCache::new();
    let camera = Camera::perspective(
        Vec3::new(0.0, 0.0, 5.0),
        Vec3::ZERO,
        Vec3::Y,
        40.0,
        0.1,
        100.0,
        [512, 512],
    );
    let ctx = PaintContext {
        mesh: &mesh,
        bvh: &bvh,
        geometry: &geo,
        camera: &camera,
        depth: &depth,
    };
    for (mode, filter) in [
        ProjectionMode::Uv,
        ProjectionMode::Planar,
        ProjectionMode::Triplanar,
    ].into_iter().flat_map(|mode| [subcraft_image::TextureFilter::Bilinear, subcraft_image::TextureFilter::Nearest].map(|filter| (mode, filter))) {
        let mut ts = TextureSet::new("T", 0, 128);
        ts.albedo_filter = filter;
        let layer = ts.insert_above(None, Layer::new_paint("Paint", &ts.clone()));
        let mut material = BrushMaterial::with(&[
            (K::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0)),
            (K::Roughness, ChannelValue::scalar(0.0)),
        ]);
        material
            .textures
            .extend([(K::BaseColor, id), (K::Roughness, id)]);
        material.projection = Projection {
            mode,
            scale: [2.3, 1.7],
            rotation_deg: 23.0,
            offset: [-0.1, 0.2],
            ..Default::default()
        };
        let mut stroke = Stroke::begin(
            &ts,
            layer,
            PaintTarget::Content,
            StrokeMode::Paint,
            BrushSettings {
                size: 45.0,
                stroke_opacity: 0.6,
                flow: 0.7,
                ..Default::default()
            },
            StrokeOptions::default(),
            &material,
        )
        .unwrap()
        .with_resources(&store);
        for x in [220.0, 260.0, 300.0, 260.0] {
            stroke
                .add_point(&mut ts, &ctx, Vec2::new(x, 256.0))
                .unwrap();
        }
        let LayerContent::Paint(paint) = &ts.find(layer).unwrap().content else {
            panic!()
        };
        assert_eq!(
            paint.channels.len(),
            2,
            "disabled channels must remain absent"
        );
        for kind in [K::BaseColor, K::Roughness] {
            let image = &paint.channels[&kind];
            let mut checked = 0;
            for tile in 0..geo.tile_count() {
                let Some(cov) = stroke.coverage(tile) else {
                    continue;
                };
                let g = geo.tile(tile);
                for (i, &c) in cov.iter().enumerate().filter(|(_, c)| **c > 0.01) {
                    let expected = sample_projected(
                        &material.projection,
                        g.uv(i),
                        g.position(i),
                        g.normal(i),
                        |uv| bmp.sample_filtered(uv[0], uv[1], Wrap::Repeat, kind.is_color(),
                            if kind == K::BaseColor { filter } else { subcraft_image::TextureFilter::Bilinear }),
                    );
                    let (x, y) = g.texel_xy(i);
                    let px = image.get(x, y);
                    for component in 0..kind.components() {
                        assert!(
                            (px[component] - expected[component]).abs() < 0.001,
                            "{mode:?} {kind:?} value"
                        );
                    }
                    assert!(
                        (px[kind.components()] - c * 0.6 * expected[3]).abs() < 0.001,
                        "{mode:?} alpha cap"
                    );
                    checked += 1;
                }
            }
            assert!(checked > 200, "test must write real texels");
        }
    }
}

#[test]
fn normal_replace_and_detail_are_unit_length_at_partial_opacity_without_intra_stroke_compounding() {
    use subcraft_paint::target::{ImageKey, TargetImage};
    let base = normal::encode(Vec3::new(0.6, 0.0, 0.8));
    let detail = normal::encode(Vec3::new(0.0, 0.6, 0.8));
    for blend in [NormalPaintBlend::Replace, NormalPaintBlend::Detail] {
        let img = TargetImage {
            key: ImageKey::Channel(K::Normal),
            snapshot: subcraft_image::TiledImage::new(64, 64, 4, [base[0], base[1], base[2], 1.0]),
            value: detail,
            value_comps: 3,
            has_alpha: true,
            texture: None,
            normal_blend: blend,
        };
        let out = img.compute_tile(0, &vec![1.0; TILE_TEXELS], 0.4, StrokeMode::Paint, None);
        let dest = if blend == NormalPaintBlend::Replace {
            detail
        } else {
            normal::reoriented(base, detail)
        };
        let expected = (normal::decode(base) * 0.6 + normal::decode(dest) * 0.4).normalize();
        for px in out.as_chunks::<4>().0 {
            let n = normal::decode([px[0], px[1], px[2]]);
            assert!((n - expected).length() < 0.002);
            assert!((n.length() - 1.0).abs() < 1e-6);
            assert_eq!(px[3], 1.0);
        }
        assert_eq!(
            out,
            img.compute_tile(0, &vec![1.0; TILE_TEXELS], 0.4, StrokeMode::Paint, None)
        );
        let mut transparent = img;
        transparent.snapshot = subcraft_image::TiledImage::new(64, 64, 4, [0.5, 0.5, 1.0, 0.0]);
        let out =
            transparent.compute_tile(0, &vec![1.0; TILE_TEXELS], 0.4, StrokeMode::Paint, None);
        assert!(
            (normal::decode([out[0], out[1], out[2]]) - normal::decode(detail)).length() < 1e-5
        );
        assert!((out[3] - 0.4).abs() < 1e-6);
    }
}

#[test]
fn missing_images_are_transparent_and_eraser_ignores_image_alpha() {
    use subcraft_paint::target::{ImageKey, TargetImage};
    let img = TargetImage {
        key: ImageKey::Channel(K::Roughness),
        snapshot: subcraft_image::TiledImage::new(64, 64, 2, [0.25, 0.75, 0.0, 0.0]),
        value: [1.0; 3],
        value_comps: 1,
        has_alpha: true,
        texture: Some(ResourceId::new()),
        normal_blend: NormalPaintBlend::Replace,
    };
    let cov = vec![1.0; TILE_TEXELS];
    let missing = vec![[0.0; 4]; TILE_TEXELS];
    let untouched = img.compute_tile(0, &cov, 0.5, StrokeMode::Paint, Some(&missing));
    assert_eq!(&untouched[..2], &[0.25, 0.75]);
    let erased = img.compute_tile(0, &cov, 0.5, StrokeMode::Erase, Some(&missing));
    assert_eq!(&erased[..2], &[0.25, 0.375]);
}

#[test]
fn non_tiling_image_outside_uv_domain_does_not_paint() {
    let mesh = Arc::new(primitives::cube(2.0));
    let bvh = Bvh::build(&mesh);
    let geo = TextureSetGeometry::new(mesh.clone(), 0, 64);
    let depth = DepthCache::new();
    let camera = Camera::perspective(
        Vec3::new(0.0, 0.0, 5.0),
        Vec3::ZERO,
        Vec3::Y,
        40.0,
        0.1,
        100.0,
        [512, 512],
    );
    let ctx = PaintContext {
        mesh: &mesh,
        bvh: &bvh,
        geometry: &geo,
        camera: &camera,
        depth: &depth,
    };
    let (store, id) = resources(Bitmap::new_u8(1, 1, 1, vec![255]));
    let mut material = BrushMaterial::with(&[(K::Roughness, ChannelValue::scalar(0.0))]);
    material.textures.insert(K::Roughness, id);
    material.projection.offset = [2.0, 2.0];
    let mut ts = TextureSet::new("T", 0, 64);
    let layer = ts.insert_above(None, Layer::new_paint("Paint", &ts.clone()));
    for tiling in [false, true] {
        material.texture_tiling = tiling;
        let mut stroke = Stroke::begin(
            &ts,
            layer,
            PaintTarget::Content,
            StrokeMode::Paint,
            BrushSettings {
                size: 30.0,
                ..Default::default()
            },
            StrokeOptions::default(),
            &material,
        )
        .unwrap()
        .with_resources(&store);
        stroke.add_point(&mut ts, &ctx, Vec2::splat(256.0)).unwrap();
        let LayerContent::Paint(paint) = &ts.find(layer).unwrap().content else {
            panic!()
        };
        let mass: f32 = (0..64)
            .flat_map(|y| (0..64).map(move |x| (x, y)))
            .map(|(x, y)| paint.channels[&K::Roughness].get(x, y)[1])
            .sum();
        assert_eq!(mass > 0.0, tiling);
    }
}
