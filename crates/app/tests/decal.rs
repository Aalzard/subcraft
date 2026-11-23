//! Image decals through the session: one click makes a fill layer that shows the image once,
//! where the user pointed, upright, with its transparency, and nowhere else.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use subcraft_app::decal::DecalSpot;
use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{FillChannel, FillSource, LayerContent, ProjectionMode, UvWrap};
use subcraft_image::{FileFormat, save_image};
use subcraft_mesh::primitives;

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("subcraft-decal-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 64x32 RGBA image: red top half, green bottom half, with a fully transparent right quarter.
fn write_decal(path: &std::path::Path) {
    let mut data = Vec::new();
    for y in 0..32 {
        for x in 0..64 {
            let c = if y < 16 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
            data.extend_from_slice(&[c[0], c[1], c[2], if x < 48 { 1.0 } else { 0.0 }]);
        }
    }
    save_image(path, 64, 32, 4, &data, FileFormat::Png8).unwrap();
}

/// A cube of side 2 (bounds -1..1) under a blue fill; the tests use its +Z face.
fn session() -> Session {
    let mut s = Session::new(primitives::cube(2.0), 256);
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = s.selected_layer().unwrap().id;
    let fill = FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.0, 0.0, 1.0)));
    s.execute(Command::SetFillChannel { layer, channel: ChannelKind::BaseColor, fill }).unwrap();
    s
}

/// Base Color of the composite on the +Z face at `p` (its z is ignored).
fn color_at(s: &Session, p: Vec3) -> [f32; 3] {
    let mesh = &s.scene().mesh;
    let hit = s.scene().bvh.intersect(Vec3::new(p.x, p.y, 3.0), Vec3::NEG_Z, 0.0, f32::INFINITY).expect("point over the face");
    let uv: Vec2 = mesh.interpolate(hit.tri as usize, hit.u, hit.v).uv;
    let img = s.composite(s.selected_texture_set().id).unwrap().channel(ChannelKind::BaseColor).unwrap();
    let c = img.get((uv.x * img.width() as f32) as u32, (uv.y * img.height() as f32) as u32);
    [c[0], c[1], c[2]]
}

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.02)
}

#[test]
fn a_decal_shows_the_image_once_upright_with_its_transparency() {
    let dir = temp("place");
    let path = dir.join("logo.png");
    write_decal(&path);
    let mut s = session();
    let before = s.document().clone();
    let centre = Vec3::new(0.3, -0.2, 1.0);
    let name = s.add_decal_layer(&path, DecalSpot { position: centre, normal: Vec3::Z, up: Vec3::Y }).unwrap();
    assert_eq!(name, "logo");
    let layer = s.selected_layer().unwrap();
    let LayerContent::Fill(fill) = &layer.content else { panic!("a decal is a fill layer") };
    assert_eq!((fill.projection.mode, fill.projection.uv_wrap), (ProjectionMode::Planar, UvWrap::None));
    assert!(fill.channels.iter().all(|(k, c)| c.enabled == (*k == ChannelKind::BaseColor)), "only Base Color is on");
    s.update_composites();

    // The decal is 0.25 of the 2 unit extent wide (0.5) and half as tall (0.25), centred on
    // the spot. Its top is red, its bottom green, its right quarter transparent.
    let (red, green, blue) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    assert!(close(color_at(&s, centre + Vec3::new(-0.1, 0.06, 0.0)), red), "top: {:?}", color_at(&s, centre + Vec3::new(-0.1, 0.06, 0.0)));
    assert!(close(color_at(&s, centre + Vec3::new(-0.1, -0.06, 0.0)), green), "bottom");
    assert!(close(color_at(&s, centre + Vec3::new(-0.22, 0.06, 0.0)), red), "left edge is inside");
    assert!(close(color_at(&s, centre + Vec3::new(0.2, 0.06, 0.0)), blue), "the image's transparent part shows the layer below");
    // Nothing repeats outside the decal.
    for off in [Vec3::new(-0.32, 0.0, 0.0), Vec3::new(0.32, 0.0, 0.0), Vec3::new(0.0, 0.2, 0.0), Vec3::new(0.0, -0.2, 0.0), Vec3::new(-0.6, 0.5, 0.0)] {
        assert!(close(color_at(&s, centre + off), blue), "outside at {off:?}: {:?}", color_at(&s, centre + off));
    }

    // One undo step removes the whole decal.
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.document().texture_sets, before.texture_sets);
}

#[test]
fn the_wanted_up_direction_turns_the_decal() {
    let dir = temp("turn");
    let path = dir.join("logo.png");
    write_decal(&path);
    let mut s = session();
    // Top of the image towards +X: the red half is on the right, the decal is tall.
    s.add_decal_layer(&path, DecalSpot { position: Vec3::Z, normal: Vec3::Z, up: Vec3::X }).unwrap();
    s.update_composites();
    assert!(close(color_at(&s, Vec3::new(0.06, 0.1, 0.0)), [1.0, 0.0, 0.0]), "{:?}", color_at(&s, Vec3::new(0.06, 0.1, 0.0)));
    assert!(close(color_at(&s, Vec3::new(-0.06, 0.1, 0.0)), [0.0, 1.0, 0.0]));
    assert!(close(color_at(&s, Vec3::new(0.06, -0.2, 0.0)), [0.0, 0.0, 1.0]), "the image's transparent quarter is now at the bottom");
    assert!(close(color_at(&s, Vec3::new(0.2, 0.0, 0.0)), [0.0, 0.0, 1.0]), "outside the tall decal");
}

#[test]
fn a_decal_needs_a_readable_image_and_a_real_spot() {
    let dir = temp("errors");
    let mut s = session();
    let before = s.document().clone();
    let spot = DecalSpot { position: Vec3::ZERO, normal: Vec3::Z, up: Vec3::Y };
    assert!(s.add_decal_layer(&dir.join("missing.png"), spot).is_err());
    let path = dir.join("logo.png");
    write_decal(&path);
    assert!(s.add_decal_layer(&path, DecalSpot { position: Vec3::splat(f32::NAN), ..spot }).is_err());
    assert_eq!(s.document().texture_sets, before.texture_sets, "a failed decal adds no layer");
}

/// Object-space direction a composited tangent-space normal points to at `p` on the +Z face,
/// through the UV tangent frame of the triangle there (T toward +U, B toward V up).
fn world_normal_at(s: &Session, p: Vec3) -> Vec3 {
    let mesh = &s.scene().mesh;
    let hit = s.scene().bvh.intersect(Vec3::new(p.x, p.y, 3.0), Vec3::NEG_Z, 0.0, f32::INFINITY).expect("point over the face");
    let tri = hit.tri as usize;
    let sp = mesh.interpolate(tri, hit.u, hit.v);
    let pos = mesh.tri_positions(tri);
    let uv = mesh.tri_uvs(tri).map(|u| Vec2::new(u.x, 1.0 - u.y));
    let (e1, e2, d1, d2) = (pos[1] - pos[0], pos[2] - pos[0], uv[1] - uv[0], uv[2] - uv[0]);
    let det = d1.x * d2.y - d2.x * d1.y;
    let t = ((e1 * d2.y - e2 * d1.y) / det).normalize();
    let b = ((e2 * d1.x - e1 * d2.x) / det).normalize();
    let img = s.composite(s.selected_texture_set().id).unwrap().channel(ChannelKind::Normal).unwrap();
    let c = img.get((sp.uv.x * img.width() as f32) as u32, (sp.uv.y * img.height() as f32) as u32);
    let n = Vec3::new(c[0] * 2.0 - 1.0, c[1] * 2.0 - 1.0, c[2] * 2.0 - 1.0);
    (t * n.x + b * n.y + sp.normal * n.z).normalize()
}

/// A decal whose image is a normal map tilted towards the image's right and a little up.
fn normal_decal(s: &mut Session, dir: &std::path::Path, up: Vec3) {
    let path = dir.join("tilt.png");
    let n = Vec3::new(0.5, 0.2, 0.84).normalize();
    let px = [n.x * 0.5 + 0.5, n.y * 0.5 + 0.5, n.z * 0.5 + 0.5];
    let data: Vec<f32> = (0..32 * 32).flat_map(|_| px).collect();
    save_image(&path, 32, 32, 3, &data, FileFormat::Png16).unwrap();
    s.add_decal_layer(&path, DecalSpot { position: Vec3::Z, normal: Vec3::Z, up }).unwrap();
    let layer = s.selected_layer().unwrap().id;
    let LayerContent::Fill(fill) = &s.selected_layer().unwrap().content else { panic!() };
    let source = fill.channels[&ChannelKind::BaseColor].source.clone();
    s.execute(Command::SetFillChannel { layer, channel: ChannelKind::Normal, fill: FillChannel::new(source) }).unwrap();
    s.update_composites();
}

#[test]
fn a_normal_map_decal_tilts_towards_the_images_right_however_it_is_turned() {
    let dir = temp("normal");
    let n = Vec3::new(0.5, 0.2, 0.84).normalize();
    // Decal upright, on its side, upside down and at an odd angle: image right / up on the face.
    for up in [Vec3::Y, Vec3::X, Vec3::NEG_Y, Vec3::new(0.6, 0.8, 0.0)] {
        let mut s = session();
        normal_decal(&mut s, &dir, up);
        let right = up.cross(Vec3::Z);
        let want = (right * n.x + up * n.y + Vec3::Z * n.z).normalize();
        for p in [Vec3::ZERO, Vec3::new(0.05, -0.04, 0.0), Vec3::new(-0.06, 0.05, 0.0)] {
            let got = world_normal_at(&s, p);
            assert!((got - want).length() < 0.03, "up {up:?} at {p:?}: {got:?}, want {want:?}");
        }
        // Outside the decal the surface stays flat.
        let flat = world_normal_at(&s, Vec3::new(0.6, 0.6, 0.0));
        assert!((flat - Vec3::Z).length() < 0.02, "{flat:?}");
    }
}

#[test]
fn a_decal_fills_the_channel_its_file_name_names() {
    let dir = temp("channel");
    let path = dir.join("bolt_normal.png");
    save_image(&path, 8, 8, 3, &[0.5, 0.5, 1.0].repeat(64), FileFormat::Png8).unwrap();
    let mut s = session();
    s.add_decal_layer(&path, DecalSpot { position: Vec3::Z, normal: Vec3::Z, up: Vec3::Y }).unwrap();
    let LayerContent::Fill(fill) = &s.selected_layer().unwrap().content else { panic!("a fill layer") };
    assert!(fill.channels.iter().all(|(k, c)| c.enabled == (*k == ChannelKind::Normal)), "a normal map decal fills Normal only");
    assert!(matches!(fill.channels[&ChannelKind::Normal].source, FillSource::Bitmap { .. }));
    assert_eq!(fill.projection.mode, ProjectionMode::Planar);
    // The flat normal map leaves the colour below alone.
    s.update_composites();
    assert!(close(color_at(&s, Vec3::ZERO), [0.0, 0.0, 1.0]));
}
