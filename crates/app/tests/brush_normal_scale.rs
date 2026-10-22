//! The brush "Image projection" Scale control changes how often an imported brush normal repeats.
use glam::{Vec2, Vec3};
use subcraft_app::{Camera, Session};
use subcraft_core::{ChannelKind, ChannelValue, normal};
use subcraft_doc::{LayerContent, ProjectionMode};
use subcraft_image::{Bitmap, encode_bitmap};
use subcraft_mesh::primitives;
use subcraft_paint::BrushMaterial;

fn camera() -> Camera {
    Camera::perspective(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y, 40.0, 0.1, 100.0, [512, 512])
}

/// Vertical stripes: the normal tilts left/right with a period of 1/8 of the image.
fn stripes(path: &std::path::Path) {
    let mut data = Vec::new();
    for _y in 0..64 {
        for x in 0..64 {
            let tilt = (x as f32 / 64.0 * 8.0 * std::f32::consts::TAU).sin() * 0.6;
            let n = Vec3::new(tilt, 0.0, 1.0).normalize();
            data.extend_from_slice(&[n.x * 0.5 + 0.5, n.y * 0.5 + 0.5, n.z * 0.5 + 0.5]);
        }
    }
    let (bytes, _) = encode_bitmap(&Bitmap::new_f32(64, 64, 3, data)).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// Sign changes of the painted normal's X tilt along the busiest raster row.
fn crossings(scale: f32, texture: &std::path::Path) -> usize {
    let mut s = Session::new(primitives::cube(2.0), 512);
    s.brush.material = BrushMaterial::with(&[(ChannelKind::Normal, ChannelValue::rgb(0.5, 0.5, 1.0))]);
    s.set_brush_channel_image(ChannelKind::Normal, texture).unwrap();
    s.brush.material.projection.mode = ProjectionMode::Planar;
    s.brush.material.projection.scale = [scale, scale];
    s.brush.settings.size = 120.0;
    s.begin_stroke().unwrap();
    for i in 0..=60 {
        s.stroke_to(&camera(), Vec2::new(150.0 + i as f32 * 3.0, 256.0), false).unwrap();
    }
    s.end_stroke();
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else { panic!() };
    let raster = &p.channels[&ChannelKind::Normal];
    let mut best = 0;
    for y in 0..512 {
        let (mut last, mut n) = (0.0f32, 0);
        for x in 0..512 {
            let px = raster.get(x, y);
            if px[3] < 0.5 { continue; }
            let tx = normal::decode([px[0], px[1], px[2]]).x;
            if tx.abs() < 0.05 { continue; }
            if last != 0.0 && tx.signum() != last.signum() { n += 1; }
            last = tx;
        }
        best = best.max(n);
    }
    best
}

#[test]
fn projection_scale_sets_how_often_the_brush_normal_repeats() {
    let dir = std::env::temp_dir().join(format!("subcraft-normal-scale-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Stripes_normalGL.png");
    stripes(&path);
    let (half, one, four) = (crossings(0.5, &path), crossings(1.0, &path), crossings(4.0, &path));
    println!("sign changes along a row: scale 0.5 -> {half}, scale 1 -> {one}, scale 4 -> {four}");
    assert!(one >= 4, "scale 1 should show the stripes ({one})");
    // Scale is a repeat count: 4 shows the image four times as often, 0.5 half as often.
    assert!(four >= one * 3, "scale 4 must repeat the image ~4x ({one} vs {four})");
    assert!(half * 3 <= one * 2, "scale 0.5 must repeat the image less ({half} vs {one})");
}
