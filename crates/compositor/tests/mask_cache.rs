//! The per-tile mask cache (`materials::sample::derived`) never serves a stale mask: after every
//! kind of change a mask reads (an effect edit, rebaked maps, a repainted base tile, a replaced
//! image resource, a layer edited below), a composite through the warm cache is bit-identical to
//! one evaluated with fresh, empty caches.

use std::sync::Arc;

use subcraft_compositor::{Composite, EvalContext, composite_full};
use subcraft_core::{ChannelKind, ChannelValue, MeshMapKind, ResourceId};
use subcraft_doc::{FillChannel, FillSource, Generator, GeneratorKind, Layer, LayerContent, Mask, MaskEffect, MaskEffectKind, ProceduralKind, ProceduralParams, Projection, Resource, ResourceKind, ResourceStore, TextureSet};
use subcraft_image::{Bitmap, TiledImage};
use subcraft_materials::{MeshMaps, TextureSetGeometry};
use subcraft_mesh::primitives;

const RES: u32 = 256;

fn geometry() -> TextureSetGeometry {
    TextureSetGeometry::new(Arc::new(primitives::plane(2.0, 2)), 0, RES)
}

fn map(phase: f32) -> TiledImage {
    let d: Vec<f32> = (0..RES * RES).map(|i| (0.5 + 0.45 * ((i % RES) as f32 * 0.07 + phase).sin() * ((i / RES) as f32 * 0.05).cos()).clamp(0.0, 1.0)).collect();
    TiledImage::from_dense(RES, RES, 1, [0.5; 4], &d)
}

fn image(res: &mut ResourceStore, id: ResourceId, shade: u8) {
    let data: Vec<u8> = (0..16 * 16).map(|i| shade.wrapping_add((i * 7) as u8)).collect();
    res.insert(Resource { id, name: "i.png".into(), kind: ResourceKind::Texture, bitmap: Arc::new(Bitmap::new_u8(16, 16, 1, data)), source_path: None, encoded: None });
}

fn fill(ts: &TextureSet, name: &str, roughness: f32) -> Layer {
    let mut l = Layer::new_fill(name, ts);
    if let LayerContent::Fill(f) = &mut l.content {
        f.channels.insert(ChannelKind::Roughness, FillChannel::new(FillSource::uniform(ChannelValue::scalar(roughness))));
    }
    l
}

/// Bottom fill, then a fill masked by an Edges generator, a procedural mask fill and an image.
fn stack(image_id: ResourceId) -> (TextureSet, subcraft_core::LayerId, subcraft_core::LayerId) {
    let mut ts = TextureSet::new("T", 0, RES);
    let bottom = fill(&ts, "Bottom", 0.2);
    let bottom_id = bottom.id;
    ts.insert_above(None, bottom);
    let mut top = fill(&ts, "Top", 0.9);
    let mut mask = Mask::new(RES, false);
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(Generator::new(GeneratorKind::Edges)))));
    let mut noise = MaskEffect::new(MaskEffectKind::Fill { value: 1.0, procedural: Some(ProceduralParams::new(ProceduralKind::Clouds)), projection: Projection::default() });
    noise.blend = subcraft_doc::BlendMode::Multiply;
    mask.effects.push(Arc::new(noise));
    let mut img = MaskEffect::new(MaskEffectKind::Image { resource: image_id, projection: Projection::default(), invert: false, balance: 0.5, contrast: 0.0 });
    img.opacity = 0.5;
    mask.effects.push(Arc::new(img));
    top.mask = Some(mask);
    let top_id = top.id;
    ts.insert_above(None, top);
    (ts, bottom_id, top_id)
}

fn fresh(ts: &TextureSet, res: &ResourceStore, maps: &MeshMaps) -> Composite {
    let geo = geometry();
    composite_full(ts, &EvalContext { geometry: &geo, resources: res, mesh_maps: maps })
}

fn edit_layer(ts: &mut TextureSet, id: subcraft_core::LayerId, f: impl FnOnce(&mut Layer)) {
    // `find_mut` is copy-on-write: a layer nothing else holds is edited in place.
    f(ts.find_mut(id).unwrap());
}

#[test]
fn warm_mask_cache_matches_fresh_evaluation_after_every_kind_of_change() {
    let geo = geometry();
    let mut res = ResourceStore::default();
    let image_id = ResourceId::new();
    image(&mut res, image_id, 10);
    let mut maps = MeshMaps::new();
    maps.insert(MeshMapKind::Curvature, map(0.0));
    let (mut ts, bottom, top) = stack(image_id);

    let warm = |ts: &TextureSet, res: &ResourceStore, maps: &MeshMaps| composite_full(ts, &EvalContext { geometry: &geo, resources: res, mesh_maps: maps });
    let check = |label: &str, ts: &TextureSet, res: &ResourceStore, maps: &MeshMaps| {
        let a = warm(ts, res, maps);
        assert_eq!(a, fresh(ts, res, maps), "{label}: warm cache differs from a fresh evaluation");
        a
    };

    let first = check("first", &ts, &res, &maps);
    assert!(subcraft_materials::sample::derived::cached_bytes() > 0, "masks are cached");
    assert_eq!(check("again", &ts, &res, &maps), first);

    // A layer below changes: the cached mask above is reused and the result is still exact.
    edit_layer(&mut ts, bottom, |l| l.blend.get_mut(&ChannelKind::Roughness).unwrap().opacity = 0.4);
    let below = check("edit below", &ts, &res, &maps);
    assert_ne!(below, first);

    // An effect edit (a new Arc for that effect).
    edit_layer(&mut ts, top, |l| {
        let m = l.mask.as_mut().unwrap();
        let mut e = m.effects[0].as_ref().clone();
        e.opacity = 0.35;
        m.effects[0] = Arc::new(e);
    });
    let level = check("generator opacity", &ts, &res, &maps);
    assert_ne!(level, below);

    // Rebaked curvature.
    maps.insert(MeshMapKind::Curvature, map(1.7));
    let rebaked = check("rebake", &ts, &res, &maps);
    assert_ne!(rebaked, level);

    // Paint on the mask's base raster (one tile).
    edit_layer(&mut ts, top, |l| {
        let base = &mut l.mask.as_mut().unwrap().base;
        for y in 70..90 {
            for x in 70..90 {
                base.set(x, y, [1.0, 0.0, 0.0, 0.0]);
            }
        }
    });
    let painted = check("painted base", &ts, &res, &maps);
    assert_ne!(painted, rebaked);

    // The image resource replaced under the same id.
    image(&mut res, image_id, 200);
    let replaced = check("replaced image", &ts, &res, &maps);
    assert_ne!(replaced, painted);

    // Disabled, then enabled again.
    edit_layer(&mut ts, top, |l| l.mask.as_mut().unwrap().enabled = false);
    check("disabled", &ts, &res, &maps);
    edit_layer(&mut ts, top, |l| l.mask.as_mut().unwrap().enabled = true);
    assert_eq!(check("enabled", &ts, &res, &maps), replaced);
}
