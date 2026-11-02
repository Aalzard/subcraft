//! ID Color Mask workflow (ADR-0041): eyedropper on the model, undo, removal, missing ID map,
//! "Mask by ID color", save/reopen.

use std::collections::BTreeMap;

use glam::{Vec2, Vec3};
use subcraft_app::part_workflow::LayerAction;
use subcraft_app::{Camera, Command, MeshMapKind, NewLayer, Session};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{FillChannel, FillSource, MaskEffectKind};
use subcraft_image::TiledImage;
use subcraft_mesh::primitives;

const RES: u32 = 128;
const RED: [f32; 3] = [0.9, 0.3, 0.3];
const BLUE: [f32; 3] = [0.3, 0.3, 0.9];

fn camera() -> Camera {
    Camera::perspective(Vec3::new(0.0, 3.0, 0.0), Vec3::ZERO, Vec3::Z, 40.0, 0.1, 100.0, [512, 512])
}

fn screen(cam: &Camera, x: f32, z: f32) -> Vec2 {
    cam.project(Vec3::new(x, 0.0, z)).unwrap().truncate()
}

/// Two ID regions split at u = 0.5.
fn id_map() -> TiledImage {
    let mut img = TiledImage::new(RES, RES, 3, [0.0; 4]);
    for y in 0..RES {
        for x in 0..RES {
            let c = if x < 64 { RED } else { BLUE };
            img.set(x, y, [c[0], c[1], c[2], 1.0]);
        }
    }
    img
}

/// Plane with a pure green fill layer carrying an empty ID Color Mask (black mask).
fn setup() -> (Session, subcraft_core::LayerId, subcraft_core::EffectId) {
    let mut s = Session::new(primitives::plane(2.0, 2), RES);
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = s.selection().layer.unwrap();
    s.execute(Command::SetFillChannel { layer, channel: ChannelKind::BaseColor, fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.0, 1.0, 0.0))) })
        .unwrap();
    s.layer_action(layer, LayerAction::IdColorMask).unwrap();
    let mask = s.selected_layer().unwrap().mask.as_ref().unwrap();
    assert!(mask.base.get(0, 0)[0] == 0.0, "the menu adds a black mask");
    let effect = mask.effects[0].id;
    assert!(matches!(&mask.effects[0].kind, MaskEffectKind::IdColor(id) if id.colors.is_empty()));
    (s, layer, effect)
}

/// Texels where the green fill shows through the mask.
fn green_texels(s: &mut Session) -> usize {
    s.update_composites();
    let img = s.composite(s.selection().texture_set).unwrap().channel(ChannelKind::BaseColor).unwrap();
    let mut n = 0;
    for y in 0..RES {
        for x in 0..RES {
            let p = img.get(x, y);
            if p[1] > 0.999 && p[0] < 1e-3 {
                n += 1;
            } else {
                assert!(p[1] < 1.0 - 1e-3, "({x},{y}) partially selected: {p:?}");
            }
        }
    }
    n
}

fn colors(s: &Session, layer: subcraft_core::LayerId) -> Vec<[f32; 3]> {
    let l = s.selected_texture_set().find(layer).unwrap();
    match &l.mask.as_ref().unwrap().effects[0].kind {
        MaskEffectKind::IdColor(id) => id.colors.clone(),
        _ => unreachable!(),
    }
}

#[test]
fn eyedropper_picks_id_colors_with_one_undo_step_each() {
    let (mut s, layer, effect) = setup();
    let ts = s.selection().texture_set;
    let cam = camera();
    let (left, right) = (screen(&cam, -0.5, 0.0), screen(&cam, 0.5, 0.0));
    assert_eq!(green_texels(&mut s), 0, "empty selection: black mask");

    // No ID map baked: nothing picked, no history entry.
    assert!(!s.has_id_map(ts));
    let entries = s.history().entries().len();
    assert_eq!(s.pick_id_color(layer, effect, &cam, left).unwrap(), None);
    assert_eq!(s.history().entries().len(), entries);

    s.set_mesh_maps(ts, BTreeMap::from([(MeshMapKind::Id, id_map())]));
    assert!(s.has_id_map(ts));
    let first = s.pick_id_color(layer, effect, &cam, left).unwrap().expect("hit the plane");
    assert_eq!(s.history().entries().len(), entries + 1);
    assert_eq!(green_texels(&mut s), 64 * 128, "exactly one ID region");
    // The same color again changes nothing.
    assert_eq!(s.pick_id_color(layer, effect, &cam, left).unwrap(), None);
    assert_eq!(s.history().entries().len(), entries + 1);
    // A miss (sky) changes nothing either.
    assert_eq!(s.pick_id_color(layer, effect, &cam, Vec2::new(1.0, 1.0)).unwrap(), None);

    let second = s.pick_id_color(layer, effect, &cam, right).unwrap().unwrap();
    assert_ne!(first, second, "the two halves have different IDs");
    assert_eq!(colors(&s, layer), vec![first, second]);
    assert_eq!(green_texels(&mut s), 128 * 128);

    s.execute(Command::Undo).unwrap();
    assert_eq!(colors(&s, layer), vec![first]);
    assert_eq!(green_texels(&mut s), 64 * 128);
    s.execute(Command::Undo).unwrap();
    assert!(colors(&s, layer).is_empty());
    assert_eq!(green_texels(&mut s), 0);
    s.execute(Command::Redo).unwrap();
    s.execute(Command::Redo).unwrap();
    assert_eq!(colors(&s, layer), vec![first, second]);

    // Removing a swatch is its own step.
    s.remove_id_color(layer, effect, 0).unwrap();
    assert_eq!(colors(&s, layer), vec![second]);
    assert_eq!(green_texels(&mut s), 64 * 128);
    s.execute(Command::Undo).unwrap();
    assert_eq!(colors(&s, layer), vec![first, second]);
    assert!(s.remove_id_color(layer, effect, 5).is_err());

    // Invalid colors are rejected atomically.
    let before = s.document().clone();
    assert!(s.execute(Command::SetIdColors { layer, effect, colors: vec![[f32::NAN, 0.0, 0.0]] }).is_err());
    assert_eq!(s.document(), &before);
}

#[test]
fn mask_by_id_color_shortcut_and_project_roundtrip() {
    let mut s = Session::new(primitives::plane(2.0, 2), RES);
    let ts = s.selection().texture_set;
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = s.selection().layer.unwrap();
    s.execute(Command::SetFillChannel { layer, channel: ChannelKind::BaseColor, fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.0, 1.0, 0.0))) })
        .unwrap();
    let cam = camera();
    let pick = s.pick(&cam, screen(&cam, 0.5, 0.0)).unwrap();
    assert!(s.mask_selected_by_id_color(ts, pick.uv).is_err(), "no ID map yet");
    s.set_mesh_maps(ts, BTreeMap::from([(MeshMapKind::Id, id_map())]));
    let entries = s.history().entries().len();
    s.mask_selected_by_id_color(ts, pick.uv).unwrap();
    assert_eq!(s.history().entries().len(), entries + 1, "one undo step");
    assert_eq!(green_texels(&mut s), 64 * 128);
    let picked = s.id_map_color(ts, pick.uv).unwrap();
    let near = |c: [f32; 3]| (0..3).all(|i| (picked[i] - c[i]).abs() < 1e-3); // stored as f16
    assert!(near(RED) || near(BLUE), "{picked:?}");

    let dir = std::env::temp_dir().join(format!("subcraft-id-color-{}", subcraft_core::LayerId::new()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("id.subcraft");
    s.save(Some(&path)).unwrap();
    let mut reopened = Session::open_project(&path).unwrap();
    assert_eq!(reopened.document(), s.document());
    assert!(reopened.has_id_map(ts), "the baked ID map is saved with the project");
    assert_eq!(green_texels(&mut reopened), 64 * 128);
    std::fs::remove_dir_all(&dir).unwrap();

    s.execute(Command::Undo).unwrap();
    assert!(s.selected_texture_set().find(layer).unwrap().mask.is_none(), "undo removes the whole shortcut edit");
}
