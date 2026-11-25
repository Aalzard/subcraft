//! Mesh Map generator channel selection (issue #732): which R, G, B channels of a color mesh map
//! the mask reads.

use std::sync::Arc;

use subcraft_compositor::{EvalContext, composite_full};
use subcraft_core::{ChannelKind, ChannelValue, MeshMapKind};
use subcraft_doc::{FillChannel, FillSource, Generator, GeneratorKind, Layer, LayerContent, Mask, MaskEffect, MaskEffectKind, ResourceStore, TextureSet};
use subcraft_image::TiledImage;
use subcraft_materials::{MeshMaps, TextureSetGeometry};
use subcraft_mesh::primitives;

const RES: u32 = 64;

/// ID map: left half pure red, right half pure blue.
fn id_map() -> TiledImage {
    let mut img = TiledImage::new(RES, RES, 3, [0.0; 4]);
    for y in 0..RES {
        for x in 0..RES {
            img.set(x, y, if x < RES / 2 { [1.0, 0.0, 0.0, 1.0] } else { [0.0, 0.0, 1.0, 1.0] });
        }
    }
    img
}

/// Mask values (left, right) of a Roughness fill masked by the generator alone.
fn mask_sides(g: Generator, maps: &MeshMaps) -> (f32, f32) {
    let geo = TextureSetGeometry::new(Arc::new(primitives::plane(2.0, 2)), 0, RES);
    let res = ResourceStore::default();
    let mut ts = TextureSet::new("T", 0, RES);
    let tc = ts.clone();
    let mut layer = Layer::new_fill("Fill", &tc);
    let LayerContent::Fill(f) = &mut layer.content else { unreachable!() };
    for (k, ch) in f.channels.iter_mut() {
        ch.enabled = *k == ChannelKind::Roughness;
        if *k == ChannelKind::Roughness {
            *ch = FillChannel::new(FillSource::uniform(ChannelValue::scalar(1.0)));
        }
    }
    let mut mask = Mask::new(RES, false);
    mask.effects.push(Arc::new(MaskEffect::new(MaskEffectKind::Generator(g))));
    layer.mask = Some(mask);
    ts.insert_above(None, layer);
    let ctx = EvalContext { geometry: &geo, resources: &res, mesh_maps: maps };
    let c = composite_full(&ts, &ctx);
    let r = c.channel(ChannelKind::Roughness).unwrap();
    // Roughness = 0.3 (stack default) + 0.7 * mask.
    let m = |x: u32| (r.get(x, RES / 2)[0] - 0.3) / 0.7;
    (m(RES / 4), m(3 * RES / 4))
}

fn generator(kind: MeshMapKind, channels: Option<[bool; 3]>) -> Generator {
    let mut g = Generator::new(GeneratorKind::MeshMap(kind));
    g.level = 0.5;
    g.softness = 0.02;
    g.noise_amount = 0.0;
    g.map_channels = channels;
    g
}

#[test]
fn checked_channels_select_the_id_colors() {
    let mut maps = MeshMaps::new();
    maps.insert(MeshMapKind::Id, id_map());
    let close = |(a, b): (f32, f32), (ea, eb): (f32, f32)| (a - ea).abs() < 1e-3 && (b - eb).abs() < 1e-3;
    // Absent (every older project): red only, as before.
    let sides = mask_sides(generator(MeshMapKind::Id, None), &maps);
    assert!(close(sides, (1.0, 0.0)), "red only by default: {sides:?}");
    let sides = mask_sides(generator(MeshMapKind::Id, Some([false, false, true])), &maps);
    assert!(close(sides, (0.0, 1.0)), "blue only: {sides:?}");
    let sides = mask_sides(generator(MeshMapKind::Id, Some([true, false, true])), &maps);
    assert!(close(sides, (1.0, 1.0)), "red or blue: {sides:?}");
    let sides = mask_sides(generator(MeshMapKind::Id, Some([false, true, false])), &maps);
    assert!(close(sides, (0.0, 0.0)), "green is in neither: {sides:?}");
    let sides = mask_sides(generator(MeshMapKind::Id, Some([false; 3])), &maps);
    assert!(close(sides, (0.0, 0.0)), "nothing checked, nothing selected: {sides:?}");
}

#[test]
fn grayscale_maps_ignore_the_channel_selection() {
    let mut maps = MeshMaps::new();
    maps.insert(MeshMapKind::Thickness, TiledImage::new(RES, RES, 1, [0.8; 4]));
    let plain = mask_sides(generator(MeshMapKind::Thickness, None), &maps);
    let blue = mask_sides(generator(MeshMapKind::Thickness, Some([false, false, true])), &maps);
    assert_eq!(plain, blue);
    assert!((plain.0 - 1.0).abs() < 1e-3, "thickness 0.8 is above the level: {plain:?}");
}
