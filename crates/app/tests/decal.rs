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
