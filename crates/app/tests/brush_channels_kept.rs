//! Choosing a brush never switches on a brush channel the user switched off (#618).
//! A preset that carries a material used to replace the brush's channels wholesale, so a
//! disabled channel came back on and the next stroke painted it.

use glam::{Vec2, Vec3};
use subcraft_app::{Camera, Command, Session, assets::alphas, brush_preset::{BrushPreset, ShelfBrush, builtin}};
use subcraft_core::{ChannelKind as K, ChannelValue};
use subcraft_doc::LayerContent;
use subcraft_image::{Bitmap, TiledImage};
use subcraft_mesh::primitives;
use subcraft_paint::StampKind;

fn dirt_grunge() -> BrushPreset {
    let p = builtin().into_iter().find(|p| p.name == "Dirt Grunge").expect("built-in Dirt Grunge preset");
    assert!(p.material.is_enabled(K::Roughness) && p.material.is_enabled(K::BaseColor), "the preset paints colour and roughness");
    p
}

fn raster(s: &Session, k: K) -> Option<TiledImage> {
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!("paint layer") };
    p.channels.get(&k).cloned()
}

/// Texels holding any paint. Scalar channels keep their coverage in component 1, colour in 3,
/// so a texel counts when any component is non-zero (unpainted texels are all zero).
fn coverage(img: &TiledImage, res: u32) -> usize {
    (0..res).flat_map(|y| (0..res).map(move |x| (x, y))).filter(|&(x, y)| img.get(x, y).iter().any(|&v| v != 0.0)).count()
}

#[test]
fn selecting_a_material_brush_keeps_switched_off_channels_off() {
    let tips = alphas::builtin();
    let mut s = Session::new(primitives::cube(2.0), 128);
    s.execute(Command::ToggleBrushChannel { channel: K::Roughness, solo: false }).unwrap();
    assert!(!s.brush.material.is_enabled(K::Roughness));
    let p = dirt_grunge();
    s.select_shelf_brush(ShelfBrush::Preset(&p), &tips).unwrap();
    let m = &s.brush.material;
    assert!(!m.is_enabled(K::Roughness), "roughness stays off");
    assert!(m.is_enabled(K::BaseColor) && m.is_enabled(K::Metallic), "the preset's other channels are on");
    // The preset's value still loaded: switching the channel back on paints it.
    assert_eq!(m.channels[&K::Roughness].value, p.material.channels[&K::Roughness].value);
    assert_eq!(m.channels[&K::BaseColor].value, p.material.channels[&K::BaseColor].value);
    s.execute(Command::ToggleBrushChannel { channel: K::Roughness, solo: false }).unwrap();
    assert!(s.brush.material.is_enabled(K::Roughness));
}

#[test]
fn a_stroke_after_selecting_the_brush_leaves_the_switched_off_channel_unpainted() {
    const RES: u32 = 128;
    let tips = alphas::builtin();
    let mut s = Session::new(primitives::plane(2.0, 1), RES);
    s.execute(Command::ToggleBrushChannel { channel: K::Roughness, solo: false }).unwrap();
    s.select_shelf_brush(ShelfBrush::Preset(&dirt_grunge()), &tips).unwrap();
    s.brush.settings.size = 10.0;
    s.brush.settings.size_jitter = 0.0;
    s.brush.settings.position_jitter = 0.0;
    let cam = Camera::perspective(Vec3::new(0.0, 3.0, 0.0), Vec3::ZERO, Vec3::NEG_Z, 40.0, 0.1, 100.0, [512, 512]);
    let screen = |x: f32| -> Vec2 { cam.project(Vec3::new(x, 0.0, 0.0)).unwrap().truncate() };
    s.begin_stroke().unwrap();
    for i in 0..=24 {
        s.stroke_to(&cam, screen(-0.8 + 1.6 * i as f32 / 24.0), false).unwrap();
    }
    s.end_stroke();
    let base = raster(&s, K::BaseColor).expect("base colour painted");
    assert!(coverage(&base, RES) > 50, "the stroke painted base colour ({} texels)", coverage(&base, RES));
    let rough = raster(&s, K::Roughness).map_or(0, |r| coverage(&r, RES));
    assert_eq!(rough, 0, "no roughness texel painted");
}

#[test]
fn channels_switched_off_before_a_stamp_stay_off_when_a_material_brush_replaces_it() {
    let tips = alphas::builtin();
    let mut s = Session::new(primitives::cube(2.0), 128);
    s.execute(Command::ToggleBrushChannel { channel: K::Metallic, solo: false }).unwrap();
    // A normal stamp switches every other channel off for itself; Metallic was off before it.
    s.set_brush_stamp("Starter: Rivets", StampKind::Normal, Bitmap::new_u8(2, 2, 3, [204u8, 128, 230].repeat(4))).unwrap();
    s.select_shelf_brush(ShelfBrush::Preset(&dirt_grunge()), &tips).unwrap();
    let m = &s.brush.material;
    assert!(m.stamp.is_none(), "the stamp is replaced");
    assert!(!m.is_enabled(K::Metallic), "metallic, off before the stamp, stays off");
    assert!(m.is_enabled(K::BaseColor) && m.is_enabled(K::Roughness), "channels on before the stamp are on");
}

#[test]
fn a_brush_without_a_material_keeps_the_channel_state_as_before() {
    let tips = alphas::builtin();
    let mut s = Session::new(primitives::cube(2.0), 128);
    s.brush.material = subcraft_paint::BrushMaterial::with(&[(K::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0)), (K::Roughness, ChannelValue::scalar(0.4))]);
    s.brush.material.disable(K::Roughness);
    let before = s.brush.material.clone();
    let p = builtin().into_iter().find(|p| p.name == "Soft Airbrush").unwrap();
    s.select_shelf_brush(ShelfBrush::Preset(&p), &tips).unwrap();
    assert_eq!(s.brush.material, before);
}
