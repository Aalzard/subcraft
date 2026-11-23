//! Picking and painting on the displacement preview: with it on, the cursor ray hits the raised
//! surface the 3D view draws, and the brush paints the modelled surface's texels under that point.

use glam::{Vec2, Vec3};
use subcraft_app::{Camera, Command, NewLayer, Session};
use subcraft_core::{ChannelKind as K, ChannelValue};
use subcraft_doc::{FillChannel, FillSource, LayerContent};
use subcraft_mesh::primitives;
use subcraft_paint::BrushMaterial;

const RES: u32 = 128;

/// A flat 2 x 2 plane facing +Y with Height = 1 everywhere (a fill layer).
fn raised() -> Session {
    let mut s = Session::new(primitives::plane(2.0, 1), RES);
    s.execute(Command::AddLayer(NewLayer::Fill)).unwrap();
    let layer = s.selection().layer.unwrap();
    s.execute(Command::SetFillChannel { layer, channel: K::Height, fill: FillChannel::new(FillSource::uniform(ChannelValue::scalar(1.0))) }).unwrap();
    s.update_composites();
    let ts = s.selected_texture_set().id;
    let h = s.composite(ts).unwrap().channels[&K::Height].get(RES / 2, RES / 2)[0];
    assert!((h - 1.0).abs() < 1e-3, "height composite {h}");
    s
}

/// Looking down at the plane at 45 degrees.
fn camera() -> Camera {
    Camera::perspective(Vec3::new(0.0, 3.0, 3.0), Vec3::ZERO, Vec3::Y, 40.0, 0.1, 100.0, [512, 512])
}

const SCALE: f32 = 0.4;
const CENTRE: Vec2 = Vec2::new(256.0, 256.0);

#[test]
fn the_cursor_hits_the_raised_surface_and_reports_the_point_under_it() {
    let mut s = raised();
    let cam = camera();
    let flat = s.pick(&cam, CENTRE).unwrap();
    assert!(flat.position.length() < 1e-3, "without the preview the centre ray hits the origin: {}", flat.position);
    s.set_displacement_preview(Some((2, SCALE)));
    assert!(s.displaced_surface().is_some());
    // The centre ray meets the raised plane (y = 0.4) at z = 0.4: the point under it is (0, 0, 0.4).
    let hit = s.pick(&cam, CENTRE).unwrap();
    assert!(hit.position.distance(Vec3::new(0.0, 0.0, SCALE)) < 1e-3, "{}", hit.position);
    assert!(hit.normal.y > 0.99);
    // Nearer than the flat surface by the 0.4 x sqrt(2) travelled less along the ray.
    assert!((flat.depth - hit.depth - SCALE * 2f32.sqrt()).abs() < 1e-3, "{} vs {}", flat.depth, hit.depth);
    // The scale follows the view; zero height scale is the flat surface again.
    s.set_displacement_preview(Some((2, 0.0)));
    assert!(s.pick(&cam, CENTRE).unwrap().position.length() < 1e-3);
    // Painting the height down moves the surface back with the next frame's update.
    s.set_displacement_preview(Some((2, SCALE)));
    let layer = s.selection().layer.unwrap();
    s.execute(Command::SetFillChannel { layer, channel: K::Height, fill: FillChannel::new(FillSource::uniform(ChannelValue::scalar(0.5))) }).unwrap();
    s.update_composites();
    s.set_displacement_preview(Some((2, SCALE)));
    let half = s.pick(&cam, CENTRE).unwrap();
    assert!(half.position.distance(Vec3::new(0.0, 0.0, 0.5 * SCALE)) < 1e-3, "{}", half.position);
    // Off: the modelled surface.
    s.set_displacement_preview(None);
    assert!(s.displaced_surface().is_none());
    assert!(s.pick(&cam, CENTRE).unwrap().position.length() < 1e-3);
}

/// Centre of what one dab at the screen centre paints, in UV.
fn dab_centre(s: &mut Session, cam: &Camera) -> Vec2 {
    s.execute(Command::AddLayer(NewLayer::Paint)).unwrap();
    s.brush.material = BrushMaterial::with(&[(K::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))]);
    s.brush.settings.size = 3.0;
    s.brush.settings.hardness = 1.0;
    s.begin_stroke().unwrap();
    s.stroke_to(cam, CENTRE, false).unwrap();
    s.end_stroke();
    let LayerContent::Paint(l) = &s.selected_layer().unwrap().content else { panic!() };
    let img = &l.channels[&K::BaseColor];
    let (mut sum, mut n) = (Vec2::ZERO, 0.0);
    for y in 0..RES {
        for x in 0..RES {
            let a = img.get(x, y)[3];
            sum += Vec2::new(x as f32 + 0.5, y as f32 + 0.5) / RES as f32 * a;
            n += a;
        }
    }
    assert!(n > 0.0, "the dab painted something");
    sum / n
}

#[test]
fn the_brush_paints_where_the_raised_surface_is_drawn() {
    let cam = camera();
    let mut flat = raised();
    let flat_uv = dab_centre(&mut flat, &cam);
    let mut shown = raised();
    shown.set_displacement_preview(Some((2, SCALE)));
    let shown_uv = dab_centre(&mut shown, &cam);
    // The UVs of the two points the picks report.
    let expected = |s: &Session, p: Vec3| {
        let mesh = &s.scene().mesh;
        (0..mesh.triangle_count())
            .find_map(|t| {
                let [a, b, c] = mesh.tri_positions(t);
                let (v0, v1, v2) = (b - a, c - a, p - a);
                let (d00, d01, d11, d20, d21) = (v0.dot(v0), v0.dot(v1), v1.dot(v1), v2.dot(v0), v2.dot(v1));
                let den = d00 * d11 - d01 * d01;
                let (v, w) = ((d11 * d20 - d01 * d21) / den, (d00 * d21 - d01 * d20) / den);
                (v >= -1e-4 && w >= -1e-4 && v + w <= 1.0 + 1e-4).then(|| mesh.interpolate(t, v, w).uv)
            })
            .unwrap()
    };
    let (want_flat, want_shown) = (expected(&flat, Vec3::ZERO), expected(&shown, Vec3::new(0.0, 0.0, SCALE)));
    eprintln!("dab centre: flat {flat_uv} (want {want_flat}), on the preview {shown_uv} (want {want_shown})");
    assert!(flat_uv.distance(want_flat) < 0.02);
    assert!(shown_uv.distance(want_shown) < 0.02, "the brush follows the preview: {shown_uv} vs {want_shown}");
    assert!(shown_uv.distance(flat_uv) > 0.15, "and not the flat surface");
}
