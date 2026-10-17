//! Reproducible paint-layer normal fixture and 4K long-stroke CPU timings.
//! cargo run -p subcraft-app --release --example brush_projection -- <scratch-directory>
use glam::{Vec2, Vec3};
use std::{path::PathBuf, time::Instant};
use subcraft_app::{Camera, Command, Session};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{LayerContent, ProjectionMode};
use subcraft_image::{Bitmap, encode_bitmap};
use subcraft_mesh::{import::write_obj, primitives};
use subcraft_paint::BrushMaterial;

fn camera() -> Camera {
    Camera::perspective(
        Vec3::new(0.0, 0.0, 5.0),
        Vec3::ZERO,
        Vec3::Y,
        40.0,
        0.1,
        100.0,
        [512, 512],
    )
}
fn main() {
    let dir = PathBuf::from(std::env::args_os().nth(1).expect("scratch directory"));
    std::fs::create_dir_all(&dir).unwrap();
    // Analytic embossed rings, encoded as a DirectX map to exercise the import conversion.
    let mut data = Vec::new();
    for y in 0..256 {
        for x in 0..256 {
            let p = Vec2::new(x as f32 / 256.0 - 0.5, y as f32 / 256.0 - 0.5);
            let slope = (p.length() * 60.0).cos() * 0.65;
            let d = p.normalize_or_zero() * slope;
            let n = Vec3::new(d.x, -d.y, 1.0).normalize();
            data.extend_from_slice(&[(n.x * 0.5 + 0.5), (n.y * 0.5 + 0.5), (n.z * 0.5 + 0.5)]);
        }
    }
    let bitmap = Bitmap::new_f32(256, 256, 3, data);
    let (bytes, ext) = encode_bitmap(&bitmap).unwrap();
    let normal = dir.join(format!("Emboss_normalDX.{ext}"));
    std::fs::write(&normal, bytes).unwrap();
    let model = dir.join("BrushCube.obj");
    write_obj(&primitives::cube(2.0), &model).unwrap();
    let (mut s, _) = Session::open_mesh(&model, 512).unwrap();
    s.execute(Command::Rename {
        layer: s.selection().layer.unwrap(),
        name: "Projected normal paint".into(),
    })
    .unwrap();
    s.save(Some(&dir.join("blank.subcraft"))).unwrap();
    s.import_brush_texture_set(std::slice::from_ref(&normal)).unwrap();
    s.brush.settings.size = 25.0;
    s.brush.material.projection.mode = ProjectionMode::Planar;
    s.brush.material.projection.scale = [3.0, 3.0];
    s.begin_stroke().unwrap();
    for i in 0..=80 {
        s.stroke_to(&camera(), Vec2::new(170.0 + i as f32 * 2.0, 256.0), false)
            .unwrap();
    }
    s.end_stroke();
    let LayerContent::Paint(p) = &s.selected_layer().unwrap().content else {
        panic!()
    };
    assert!(p.channels[&ChannelKind::Normal].allocated_tiles() > 0);
    s.save(Some(&dir.join("painted.subcraft"))).unwrap();
    for textured in [false, true] {
        let mut s = Session::new(primitives::cube(2.0), 4096);
        s.brush.material =
            BrushMaterial::with(&[(ChannelKind::Normal, ChannelValue::rgb(0.5, 0.5, 1.0))]);
        if textured {
            s.set_brush_channel_image(ChannelKind::Normal, &normal)
                .unwrap();
        }
        s.brush.settings.size = 5.0;
        let mut times = Vec::new();
        s.begin_stroke().unwrap();
        for i in 0..1000 {
            let t = Instant::now();
            let phase = i as f32 / 999.0;
            s.stroke_to(
                &camera(),
                Vec2::new(170.0 + phase * 170.0, 256.0 + (phase * 12.0).sin() * 30.0),
                false,
            )
            .unwrap();
            times.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        s.end_stroke();
        let first = times[0];
        times.sort_by(f64::total_cmp);
        println!(
            "4K normal textured={textured}: 1000 samples, cold={first:.2}ms, median={:.2}ms p95={:.2}ms max={:.2}ms total={:.1}ms",
            times[500],
            times[950],
            times[999],
            times.iter().sum::<f64>()
        );
    }
    println!("Fixture written to {}", dir.display());
}
