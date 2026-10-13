//! End-to-end: brush material channels painted through the Session reach the viewport render.

use glam::{Vec2, Vec3};
use subcraft_app::{Camera, Session};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_mesh::primitives;
use subcraft_paint::BrushMaterial;
use subcraft_render::{SHADED_CHANNELS, ViewParams, ViewportRenderer, headless};

const SIZE: u32 = 256;

fn render(session: &mut Session, renderer: &mut ViewportRenderer, dev: &subcraft_render::wgpu_api::Device, q: &subcraft_render::wgpu_api::Queue, cam: &Camera) -> Vec<u8> {
    session.update_composites();
    for ts in &session.document().texture_sets {
        let c = session.composite(ts.id).unwrap();
        for k in SHADED_CHANNELS {
            if let Some(img) = c.channel(k) {
                renderer.upload_channel(dev, q, ts.material_slot, k, img, None);
            }
        }
    }
    let p = ViewParams::new(cam.view, cam.proj);
    renderer.render(dev, q, [SIZE, SIZE], &p).read_rgba8(dev, q)
}

fn stroke(session: &mut Session, cam: &Camera, material: BrushMaterial) {
    session.brush.material = material;
    session.brush.settings.size = 25.0;
    session.begin_stroke().unwrap();
    let c = cam.project(Vec3::new(0.0, 0.0, 1.0)).unwrap().truncate();
    for k in -10..=10 {
        session.stroke_to(cam, c + Vec2::new(k as f32 * 4.0, 0.0), false).unwrap();
    }
    session.end_stroke();
}

fn changed(a: &[u8], b: &[u8]) -> usize {
    a.chunks_exact(4).zip(b.chunks_exact(4)).filter(|(x, y)| x.iter().zip(y.iter()).any(|(p, q)| p.abs_diff(*q) > 6)).count()
}

#[test]
fn metallic_roughness_and_height_strokes_change_the_viewport() {
    let Some((_, dev, q)) = headless::device() else {
        eprintln!("no wgpu adapter: skipping");
        return;
    };
    let cam = Camera::perspective(Vec3::new(0.6, 0.8, 4.0), Vec3::ZERO, Vec3::Y, 40.0, 0.1, 50.0, [SIZE, SIZE]);
    for (name, material) in [
        ("metallic", BrushMaterial::with(&[(ChannelKind::Metallic, ChannelValue::scalar(1.0)), (ChannelKind::Roughness, ChannelValue::scalar(0.1))])),
        ("roughness", BrushMaterial::with(&[(ChannelKind::Roughness, ChannelValue::scalar(0.05))])),
        ("height", BrushMaterial::with(&[(ChannelKind::Height, ChannelValue::scalar(0.5))])),
    ] {
        let mut s = Session::new(primitives::cube(2.0), 256);
        let mut r = ViewportRenderer::new(&dev, &q);
        r.set_mesh(&dev, &s.scene().mesh);
        let before = render(&mut s, &mut r, &dev, &q, &cam);
        stroke(&mut s, &cam, material);
        let after = render(&mut s, &mut r, &dev, &q, &cam);
        let body = s.document().texture_sets[0].id;
        let comp = s.composite(body).unwrap();
        let k = match name { "height" => ChannelKind::Height, "roughness" => ChannelKind::Roughness, _ => ChannelKind::Metallic };
        let img = comp.channel(k).unwrap();
        let maxv = img.to_dense().iter().cloned().fold(f32::MIN, f32::max);
        let minv = img.to_dense().iter().cloned().fold(f32::MAX, f32::min);
        let maxdiff = before.iter().zip(&after).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
        eprintln!("{name}: composite range {minv}..{maxv}, max pixel diff {maxdiff}");
        let n = changed(&before, &after);
        assert!(n > 150, "{name} stroke changed only {n} pixels");
        eprintln!("{name}: {n} pixels changed");
    }
}
