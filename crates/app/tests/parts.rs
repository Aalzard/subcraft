//! Part masks: a layer shown only on chosen mesh parts, picked in the viewport.

use glam::{Vec2, Vec3};
use subcraft_app::{Camera, Command, NewLayer, Session};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{FillChannel, FillSource, PartMask};
use subcraft_mesh::{Corner, Mesh, MeshBuilder};

/// Two separate quads in one material ("blade" at x < 0, "handle" at x > 0), each on its own
/// half of UV space.
fn two_pieces() -> Mesh {
    let mut b = MeshBuilder::new("Axe");
    let m = b.material("Axe");
    let c = |x: f32, z: f32, u: f32, v: f32| Corner { position: Vec3::new(x, 0.0, z), normal: Some(Vec3::Y), uv: Some(Vec2::new(u, v)) };
    let blade = [c(-1.0, -1.0, 0.05, 0.05), c(-0.2, -1.0, 0.45, 0.05), c(-0.2, 1.0, 0.45, 0.95), c(-1.0, 1.0, 0.05, 0.95)];
    let handle = [c(0.2, -1.0, 0.55, 0.05), c(1.0, -1.0, 0.95, 0.05), c(1.0, 1.0, 0.95, 0.95), c(0.2, 1.0, 0.55, 0.95)];
    for q in [blade, handle] {
        b.push_triangle(m, [q[0], q[2], q[1]]);
        b.push_triangle(m, [q[0], q[3], q[2]]);
    }
    b.build().unwrap()
}

fn top_camera() -> Camera {
    Camera::perspective(Vec3::new(0.0, 4.0, 0.01), Vec3::ZERO, Vec3::Y, 40.0, 0.05, 100.0, [800, 600])
}

fn screen_of(cam: &Camera, p: Vec3) -> Vec2 {
    cam.project(p).unwrap().truncate()
}

/// Session with a red fill layer on top (selected).
fn session() -> (Session, subcraft_core::LayerId) {
    let mut s = Session::new(two_pieces(), 256);
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = s.selected_layer().unwrap().id;
    let red = FillChannel::new(FillSource::uniform(ChannelValue::rgb(1.0, 0.0, 0.0)));
    s.execute(Command::SetFillChannel { layer, channel: ChannelKind::BaseColor, fill: red }).unwrap();
    s.update_composites();
    (s, layer)
}

fn red_at(s: &Session, u: f32, v: f32) -> f32 {
    let ts = s.selected_texture_set().id;
    let img = s.composite(ts).unwrap().channel(ChannelKind::BaseColor).unwrap();
    img.get((u * img.width() as f32) as u32, (v * img.height() as f32) as u32)[0]
}

#[test]
fn separate_pieces_are_separate_parts_and_clicks_find_them() {
    let (s, _) = session();
    let parts = s.mesh_parts();
    assert_eq!(parts.len(), 2);
    assert!(parts.iter().all(|p| p.triangles == 2));
    let cam = top_camera();
    let blade = s.part_at(&cam, screen_of(&cam, Vec3::new(-0.6, 0.0, 0.0))).unwrap();
    let handle = s.part_at(&cam, screen_of(&cam, Vec3::new(0.6, 0.0, 0.0))).unwrap();
    assert_ne!(blade, handle);
    assert_eq!(s.part_at(&cam, screen_of(&cam, Vec3::new(0.0, 0.0, 0.0))), None, "the gap is no part");
}

#[test]
fn layer_shows_only_on_picked_parts() {
    let (mut s, layer) = session();
    let cam = top_camera();
    assert!(red_at(&s, 0.25, 0.5) > 0.99 && red_at(&s, 0.75, 0.5) > 0.99, "everywhere without a part mask");

    let blade = s.part_at(&cam, screen_of(&cam, Vec3::new(-0.6, 0.0, 0.0))).unwrap();
    s.toggle_layer_part(layer, blade).unwrap();
    s.update_composites();
    assert!(red_at(&s, 0.25, 0.5) > 0.99, "blade keeps the layer");
    assert!(red_at(&s, 0.75, 0.5) < 0.9, "handle no longer shows it");

    // Clicking the handle adds it; clicking the blade again removes it.
    let handle = s.part_at(&cam, screen_of(&cam, Vec3::new(0.6, 0.0, 0.0))).unwrap();
    s.toggle_layer_part(layer, handle).unwrap();
    s.toggle_layer_part(layer, blade).unwrap();
    s.update_composites();
    assert!(red_at(&s, 0.25, 0.5) < 0.9 && red_at(&s, 0.75, 0.5) > 0.99);

    // Undo steps back one click at a time; disabling shows the layer everywhere again.
    s.execute(Command::Undo).unwrap();
    s.update_composites();
    assert!(red_at(&s, 0.25, 0.5) > 0.99 && red_at(&s, 0.75, 0.5) > 0.99, "blade back, handle still on");
    let mut p = s.selected_layer().unwrap().parts.clone().unwrap();
    p.enabled = false;
    s.execute(Command::SetParts { layer, parts: Some(p) }).unwrap();
    s.update_composites();
    assert!(red_at(&s, 0.25, 0.5) > 0.99 && red_at(&s, 0.75, 0.5) > 0.99);

    // An enabled part mask with no parts hides the layer.
    s.execute(Command::SetParts { layer, parts: Some(PartMask::new([])) }).unwrap();
    s.update_composites();
    assert!(red_at(&s, 0.25, 0.5) < 0.9 && red_at(&s, 0.75, 0.5) < 0.9);
}

#[test]
fn part_mask_survives_save_and_reopen() {
    let (mut s, layer) = session();
    let cam = top_camera();
    let blade = s.part_at(&cam, screen_of(&cam, Vec3::new(-0.6, 0.0, 0.0))).unwrap();
    s.toggle_layer_part(layer, blade).unwrap();
    let dir = std::env::temp_dir().join(format!("subcraft-parts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("axe.subcraft");
    s.save(Some(&path)).unwrap();

    let mut o = Session::open_project(&path).unwrap();
    let ts = o.selected_texture_set();
    let l = ts.find(layer).unwrap();
    assert_eq!(l.parts, Some(PartMask::new([blade])));
    o.update_composites();
    assert!(red_at(&o, 0.25, 0.5) > 0.99 && red_at(&o, 0.75, 0.5) < 0.9);
    std::fs::remove_dir_all(&dir).ok();
}
