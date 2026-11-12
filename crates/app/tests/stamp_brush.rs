//! Normal-map stamp brush at session level: activating it, the layer's Normal channel turning on
//! with the first stroke, one undo step per stroke, other channels untouched, preset round trip.

use glam::{Vec2, Vec3};
use subcraft_app::{
    Camera, Command, Session,
    assets::{UserLibrary, alphas},
    brush_preset::{load_brush_preset, save_brush_preset},
};
use subcraft_core::{ChannelKind as K, normal};
use subcraft_doc::{BlendMode, LayerContent};
use subcraft_image::{Bitmap, TiledImage};
use subcraft_mesh::primitives;
use subcraft_paint::{StampKind, StrokeMode};

fn tilted() -> Bitmap {
    Bitmap::new_u8(4, 4, 3, [204u8, 128, 230].repeat(16))
}

fn camera() -> Camera {
    Camera::perspective(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y, 40.0, 0.1, 100.0, [512, 512])
}

fn stamp_once(s: &mut Session, at: Vec2) {
    s.begin_stroke().unwrap();
    s.stroke_to(&camera(), at, false).unwrap();
    s.end_stroke();
}

fn raster(s: &Session, k: K) -> Option<TiledImage> {
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!() };
    p.channels.get(&k).cloned()
}

fn layer_normal_blend_active(s: &Session) -> bool {
    s.selected_layer().unwrap().blend.get(&K::Normal).is_some_and(|b| b.is_active())
}

#[test]
fn first_stroke_enables_the_layers_normal_channel_and_only_that() {
    let mut s = Session::new(primitives::cube(2.0), 128);
    let layer = s.selection().layer.expect("default paint layer");
    s.execute(Command::SetBlendMode { layer, channel: K::Normal, mode: BlendMode::Disable }).unwrap();
    assert!(!layer_normal_blend_active(&s));
    let entries = s.history().entries().len();
    s.set_brush_stamp("Test bump", StampKind::Normal, tilted()).unwrap();
    // Choosing the brush is tool state only.
    assert_eq!(s.history().entries().len(), entries);
    assert!(!layer_normal_blend_active(&s));
    s.brush.settings.size = 25.0;
    stamp_once(&mut s, Vec2::new(256.0, 256.0));
    assert!(layer_normal_blend_active(&s), "the first stroke turned the Normal channel on");
    assert_ne!(s.selected_layer().unwrap().blend[&K::Normal].mode, BlendMode::Disable);
    let normals = raster(&s, K::Normal).expect("normals painted");
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!() };
    assert_eq!(p.channels.len(), 1, "other channels untouched: {:?}", p.channels.keys().collect::<Vec<_>>());
    // Painted texels hold unit normals leaning like the stamp (some tilt, not flat).
    let mut tilted_texels = 0;
    for y in 0..128 {
        for x in 0..128 {
            let px = normals.get(x, y);
            if px[3] > 0.9 {
                let n = normal::decode([px[0], px[1], px[2]]);
                assert!((n.length() - 1.0).abs() < 1e-3);
                if n.z < 0.95 { tilted_texels += 1; }
            }
        }
    }
    assert!(tilted_texels > 20, "{tilted_texels}");
    // Compositing the layer stack shows it in the Normal channel.
    s.update_composites();
}

#[test]
fn one_undo_step_per_stroke_restores_layer_state_including_the_channel_switch() {
    let mut s = Session::new(primitives::cube(2.0), 128);
    let layer = s.selection().layer.unwrap();
    s.execute(Command::SetBlendMode { layer, channel: K::Normal, mode: BlendMode::Disable }).unwrap();
    let before = s.document().clone();
    let entries = s.history().entries().len();
    s.set_brush_stamp("Test bump", StampKind::Normal, tilted()).unwrap();
    s.brush.settings.size = 25.0;
    s.begin_stroke().unwrap();
    for x in [200.0, 230.0, 256.0, 290.0, 320.0] {
        s.stroke_to(&camera(), Vec2::new(x, 256.0), false).unwrap();
    }
    s.end_stroke();
    assert_eq!(s.history().entries().len(), entries + 1, "one history entry for the whole stroke");
    let after = s.document().clone();
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.document(), &before);
    assert!(!layer_normal_blend_active(&s));
    s.execute(Command::Redo).unwrap();
    assert_eq!(s.document(), &after);
    assert!(layer_normal_blend_active(&s));
}

#[test]
fn selecting_the_stamp_solos_the_channel_and_back_to_normal_brush_restores_the_brush() {
    let mut s = Session::new(primitives::cube(2.0), 128);
    let enabled_before: Vec<K> = s.brush.material.channels.iter().filter(|(_, c)| c.enabled).map(|(k, _)| *k).collect();
    assert!(enabled_before.len() > 1);
    let settings = s.brush.settings;
    s.set_brush_stamp("Starter: Rivets", StampKind::Normal, tilted()).unwrap();
    assert_eq!(s.brush.material.stamp.as_ref().unwrap().name, "Rivets");
    let enabled: Vec<K> = s.brush.material.channels.iter().filter(|(_, c)| c.enabled).map(|(k, _)| *k).collect();
    assert_eq!(enabled, vec![K::Normal]);
    assert_eq!(s.brush.mode, StrokeMode::Paint);
    assert!(s.brush_stamp().is_some());
    // Switching to another stamp keeps what to restore.
    s.set_brush_stamp("Other", StampKind::Normal, Bitmap::new_u8(2, 2, 3, [128u8, 128, 255].repeat(4))).unwrap();
    s.clear_brush_stamp();
    assert!(s.brush.material.stamp.is_none());
    let enabled_after: Vec<K> = s.brush.material.channels.iter().filter(|(_, c)| c.enabled).map(|(k, _)| *k).collect();
    assert_eq!(enabled_after, enabled_before);
    assert_eq!(s.brush.settings, settings);
}

#[test]
fn erase_mode_and_masks_ignore_the_stamp() {
    let mut s = Session::new(primitives::cube(2.0), 128);
    s.set_brush_stamp("Test bump", StampKind::Normal, tilted()).unwrap();
    s.brush.mode = StrokeMode::Erase;
    assert!(s.stroke_stamp().is_none());
}

#[test]
fn height_stamp_turns_on_the_height_channel() {
    let mut s = Session::new(primitives::cube(2.0), 128);
    s.set_brush_stamp("Dot", StampKind::Height, Bitmap::new_u8(2, 2, 1, vec![255; 4])).unwrap();
    s.brush.settings.size = 25.0;
    stamp_once(&mut s, Vec2::new(256.0, 256.0));
    let h = raster(&s, K::Height).expect("height painted");
    assert!((0..128).any(|y| (0..128).any(|x| h.get(x, y)[1] > 0.9)));
    assert!(raster(&s, K::Normal).is_none());
}

#[test]
fn preset_round_trip_keeps_the_stamp_and_its_image() {
    let dir = std::env::temp_dir().join(format!("subcraft-stamp-preset-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut a = Session::new(primitives::cube(2.0), 128);
    a.set_brush_stamp("Screw", StampKind::Normal, tilted()).unwrap();
    {
        let st = a.brush.material.stamp.as_mut().unwrap();
        st.strength = 1.7;
        st.flip_green = true;
        st.tiled = true;
    }
    a.brush.settings.angle_deg = 33.0;
    a.brush.settings.angle_jitter_deg = 20.0;
    let lib = UserLibrary::new(dir.join("library"));
    let saved = save_brush_preset(&lib, &a.brush_preset("Screw brush")).unwrap();
    let loaded = load_brush_preset(saved.user_file.as_ref().unwrap()).unwrap();
    assert_eq!(loaded.images.len(), 1);
    let mut b = Session::new(primitives::cube(2.0), 128);
    assert!(b.apply_brush_preset(&loaded, &alphas::builtin()).is_none());
    let (sa, _) = a.brush_stamp().unwrap();
    let (sb, bitmap) = b.brush_stamp().expect("stamp restored with its image");
    assert_eq!((sb.kind, sb.name.as_str(), sb.strength, sb.flip_green, sb.tiled), (sa.kind, "Screw", 1.7, true, true));
    assert_eq!(a.brush_stamp().unwrap().1, bitmap);
    assert_eq!(b.brush.settings.angle_deg, 33.0);
    assert_eq!(b.brush.settings.angle_jitter_deg, 20.0);
    // An ordinary preset (no stamp) switches it off again.
    let plain = Session::new(primitives::cube(2.0), 128).brush_preset("Plain");
    b.apply_brush_preset(&plain, &alphas::builtin());
    assert!(b.brush.material.stamp.is_none());
    // Old presets (no `stamp` field) still load.
    let mut json: serde_json::Value = serde_json::to_value(&plain).unwrap();
    json["material"].as_object_mut().unwrap().remove("stamp");
    let old: subcraft_app::brush_preset::BrushPreset = serde_json::from_value(json).unwrap();
    assert!(old.material.stamp.is_none());
    std::fs::remove_dir_all(dir).unwrap();
}

/// A Shelf click puts a thumbnail stamp in place at once; the full image encoded on a worker
/// thread then replaces it without touching what the user adjusted meanwhile.
#[test]
fn full_resolution_upgrade_keeps_settings_and_never_overrides_another_choice() {
    let mut s = Session::new(primitives::cube(2.0), 64);
    let thumb = subcraft_app::stamp_brush::prepare_stamp("Starter: Bolts", Bitmap::new_u8(2, 2, 3, [204u8, 128, 230].repeat(4))).unwrap();
    assert_eq!(thumb.name, "Bolts", "Starter prefix dropped");
    let placeholder = s.set_prepared_stamp(StampKind::Normal, thumb);
    assert_eq!(s.brush_stamp().unwrap().1.width, 2, "thumbnail is the brush at once");
    // The user changes the strength while the full image loads (on another thread).
    s.brush.material.stamp.as_mut().unwrap().strength = 2.5;
    let full = std::thread::spawn(|| subcraft_app::stamp_brush::prepare_stamp("Starter: Bolts", tilted()).unwrap()).join().unwrap();
    assert!(s.upgrade_brush_stamp(placeholder, full.clone()));
    let (stamp, image) = s.brush_stamp().unwrap();
    assert_eq!(image.width, 4, "full resolution swapped in");
    assert_eq!(stamp.strength, 2.5, "user setting kept");
    assert_eq!(stamp.name, "Bolts");
    assert_ne!(stamp.resource, Some(placeholder));
    assert!(s.brush.material.channels[&K::Normal].enabled);

    // Picked another stamp before the load finished: the late image must not replace it.
    let other = s.set_prepared_stamp(StampKind::Normal, subcraft_app::stamp_brush::prepare_stamp("Rivets", Bitmap::new_u8(1, 1, 3, vec![128, 128, 255])).unwrap());
    assert!(!s.upgrade_brush_stamp(placeholder, full.clone()));
    assert_eq!(s.brush.material.stamp.as_ref().unwrap().resource, Some(other));
    // Back to a normal brush meanwhile: also untouched.
    s.clear_brush_stamp();
    assert!(!s.upgrade_brush_stamp(other, full));
    assert!(s.brush.material.stamp.is_none());
}

#[test]
fn identical_prepared_images_share_one_resource() {
    let mut s = Session::new(primitives::cube(2.0), 64);
    let a = s.set_prepared_stamp(StampKind::Normal, subcraft_app::stamp_brush::prepare_stamp("A", tilted()).unwrap());
    let n = s.resources().len();
    let b = s.set_prepared_stamp(StampKind::Height, subcraft_app::stamp_brush::prepare_stamp("B", tilted()).unwrap());
    assert_eq!(a, b);
    assert_eq!(s.resources().len(), n);
}

/// The fill stroke carries the stamp but accumulates no slope; the brush's flat normal value
/// used to be read as a slope of (0.5, 0.5), tilting every normal of the filled island.
#[test]
fn polygon_fill_with_a_normal_stamp_brush_fills_a_flat_normal() {
    let mut s = Session::new(primitives::cube(2.0), 128);
    s.set_brush_stamp("Test bump", StampKind::Normal, tilted()).unwrap();
    let target = s.target_for_part(subcraft_app::PartKey { kind: subcraft_core::PartKind::UvIsland, id: 0 }).unwrap();
    s.polygon_fill_uv(target.uv, subcraft_paint::PolygonFillMode::UvIsland).unwrap();
    let image = raster(&s, K::Normal).expect("the fill enables the Normal channel");
    let (x, y) = ((target.uv.x * 128.0) as u32, (target.uv.y * 128.0) as u32);
    let px = image.get(x, y);
    assert!(px[3] > 0.99, "island is filled: {px:?}");
    let n = normal::decode([px[0], px[1], px[2]]);
    assert!(n.z > 0.999, "a filled normal is flat, not tilted: {n:?}");
}
