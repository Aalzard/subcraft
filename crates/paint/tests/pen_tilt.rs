//! Pen tilt and rotation: size, flow, roundness and angle of the stamps, and nothing for a mouse.

use std::sync::Arc;

use glam::Vec3;
use subcraft_core::{ChannelKind, ChannelValue, LayerId};
use subcraft_doc::{Layer, LayerContent, TextureSet};
use subcraft_materials::TextureSetGeometry;
use subcraft_mesh::uv_raster::rasterize_surface;
use subcraft_mesh::{Bvh, primitives};
use subcraft_paint::{BrushMaterial, BrushSettings, Camera, DepthCache, PaintContext, PaintTarget, PenPose, Stroke, StrokeMode, StrokeOptions};

const RES: u32 = 256;

/// One dab at the middle of the front face of a 2 m cube, seen straight on. Returns the painted
/// texel count, the peak alpha and the coverage-weighted spread along x and along y (meters²).
fn dab(settings: BrushSettings, pose: Option<PenPose>) -> (usize, f32, f32, f32) {
    let mesh = Arc::new(primitives::cube(2.0));
    let mut ts = TextureSet::new("T", 0, RES);
    let tc = ts.clone();
    let layer: LayerId = ts.insert_above(None, Layer::new_paint("Paint", &tc));
    let (bvh, geo, depth) = (Bvh::build(&mesh), TextureSetGeometry::new(mesh.clone(), 0, RES), DepthCache::new());
    let cam = Camera::perspective(Vec3::new(0.0, 0.0, 6.0), Vec3::ZERO, Vec3::Y, 40.0, 0.1, 100.0, [512, 512]);
    let ctx = PaintContext { mesh: &mesh, bvh: &bvh, geometry: &geo, camera: &cam, depth: &depth };
    let red = BrushMaterial::with(&[(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))]);
    let mut s = Stroke::begin(&ts, layer, PaintTarget::Content, StrokeMode::Paint, settings, StrokeOptions::default(), &red).unwrap();
    s.set_pen_pose(pose);
    let mut out = ts.clone();
    s.add_point(&mut out, &ctx, cam.project(Vec3::new(0.0, 0.0, 1.0)).unwrap().truncate()).unwrap();
    let LayerContent::Paint(p) = &out.find(layer).unwrap().content else { panic!() };
    let img = p.channels.get(&ChannelKind::BaseColor).expect("painted").clone();
    let surf = rasterize_surface(&mesh, 0..mesh.triangle_count(), RES, RES, 0.0);
    let (mut n, mut peak, mut w, mut sx, mut sy) = (0, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for i in 0..(RES * RES) as usize {
        let a = img.get(i as u32 % RES, i as u32 / RES)[img.comps() - 1];
        if a <= 0.01 || !surf.covered(i) {
            continue;
        }
        let q = surf.position[i];
        n += 1;
        peak = peak.max(a);
        w += a;
        sx += a * q[0] * q[0];
        sy += a * q[1] * q[1];
    }
    (n, peak, sx / w.max(1e-9), sy / w.max(1e-9))
}

fn base() -> BrushSettings {
    BrushSettings { size: 20.0, hardness: 1.0, pressure_size: 0.0, ..BrushSettings::default() }
}

fn tilted(x: f32, y: f32) -> Option<PenPose> {
    Some(PenPose { tilt: [x, y], rotation: None })
}

#[test]
fn a_mouse_and_an_upright_pen_are_unchanged_by_every_tilt_setting() {
    let plain = dab(base(), None);
    let all = BrushSettings { tilt_angle: true, tilt_size: 2.0, tilt_flow: 1.0, tilt_roundness: 1.0, rotation_angle: true, ..base() };
    assert_eq!(dab(all, None), plain, "a mouse (no pose)");
    assert_eq!(dab(all, tilted(0.0, 0.0)), plain, "an upright pen");
    // Tilt settings off: a tilted pen paints as before.
    assert_eq!(dab(base(), tilted(50.0, 20.0)), plain);
}

#[test]
fn tilt_widens_the_brush_and_lowers_the_flow() {
    let (n0, _, _, _) = dab(base(), None);
    let (n1, _, _, _) = dab(BrushSettings { tilt_size: 1.0, ..base() }, tilted(60.0, 0.0));
    let ratio = n1 as f32 / n0 as f32;
    assert!((3.4..4.6).contains(&ratio), "full tilt, Tilt size 1 = twice the radius = 4x the area, got {ratio} ({n0} -> {n1})");
    // Half tilt: 1.5x the radius.
    let (nh, _, _, _) = dab(BrushSettings { tilt_size: 1.0, ..base() }, tilted(0.0, 30.0));
    let ratio = nh as f32 / n0 as f32;
    assert!((1.9..2.6).contains(&ratio), "half tilt = 2.25x the area, got {ratio}");
    let (_, peak, _, _) = dab(BrushSettings { tilt_flow: 0.5, ..base() }, tilted(60.0, 0.0));
    assert!((peak - 0.51).abs() < 0.03, "Tilt flow 0.5 at full tilt halves the flow: {peak}");
}

#[test]
fn tilt_flattens_the_footprint_along_the_direction_the_pen_leans() {
    let s = BrushSettings { tilt_angle: true, tilt_roundness: 1.0, ..base() };
    // Upright: round.
    let (_, _, x0, y0) = dab(s, tilted(0.0, 0.0));
    assert!((x0 / y0 - 1.0).abs() < 0.1, "round: {x0} vs {y0}");
    // Leaning right (screen +X): long along x, flat across.
    let (_, _, xr, yr) = dab(s, tilted(60.0, 0.0));
    assert!(xr > 6.0 * yr, "leaning right: spread x {xr} vs y {yr}");
    // Leaning toward the user (screen +Y): long along the model's y.
    let (_, _, xd, yd) = dab(s, tilted(0.0, 60.0));
    assert!(yd > 6.0 * xd, "leaning down: spread x {xd} vs y {yd}");
    // Without Tilt angle the footprint keeps the brush angle (flattened along its own y).
    let (_, _, xf, yf) = dab(BrushSettings { tilt_angle: false, ..s }, tilted(0.0, 60.0));
    assert!(xf > 6.0 * yf, "fixed angle: spread x {xf} vs y {yf}");
}

#[test]
fn pen_rotation_turns_the_brush_when_enabled() {
    let flat = BrushSettings { roundness: 0.25, ..base() };
    let rotated = Some(PenPose { tilt: [0.0, 0.0], rotation: Some(90.0) });
    let (_, _, x0, y0) = dab(flat, rotated);
    assert!(x0 > 6.0 * y0, "rotation ignored by default: {x0} vs {y0}");
    let (_, _, x1, y1) = dab(BrushSettings { rotation_angle: true, ..flat }, rotated);
    assert!(y1 > 6.0 * x1, "rotation 90 turns the flat brush upright: {x1} vs {y1}");
}

#[test]
fn pose_math() {
    let p = PenPose { tilt: [30.0, 40.0], rotation: None };
    assert!((p.tilt_amount() - 50.0 / 60.0).abs() < 1e-6);
    assert!((p.tilt_direction().unwrap() - 40f32.atan2(30.0)).abs() < 1e-6);
    assert_eq!(PenPose { tilt: [80.0, 80.0], rotation: None }.tilt_amount(), 1.0);
    assert_eq!(PenPose { tilt: [f32::NAN, 0.0], rotation: None }.tilt_amount(), 0.0);
    assert_eq!(PenPose::default().tilt_direction(), None);
    // Settings saved before tilt existed load with every tilt option off.
    let old: BrushSettings = serde_json::from_str(r#"{"size": 3.0}"#).unwrap();
    assert!(!old.uses_pen_pose());
}
