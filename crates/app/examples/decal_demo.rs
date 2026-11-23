//! Image decals on a curved and a flat model: three badges placed with `Session::add_decal_layer`
//! on a sphere (or a cube with `cube`), ray-cast to a picture.
//! cargo run --release -p subcraft-app --example decal_demo -- <out.png> [sphere|cube] [size]
use glam::{Vec2, Vec3};
use subcraft_app::decal::DecalSpot;
use subcraft_app::{Command, NewLayer, Session};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{FillChannel, FillSource};
use subcraft_image::{FileFormat, save_image};
use subcraft_mesh::Bvh;

fn err(e: impl std::fmt::Debug) -> String { format!("{e:?}") }

/// A 256x128 badge with a transparent surround: a rounded plate, a ring and a top stripe, so
/// its direction and edges are easy to read.
fn write_badge(path: &std::path::Path, color: [f32; 3]) -> Result<(), String> {
    let (w, h) = (256u32, 128u32);
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let p = Vec2::new(x as f32 + 0.5 - w as f32 / 2.0, y as f32 + 0.5 - h as f32 / 2.0);
            // Rounded rectangle 232x104 with corner radius 24.
            let q = p.abs() - Vec2::new(116.0 - 24.0, 52.0 - 24.0);
            let d = q.max(Vec2::ZERO).length() + q.x.max(q.y).min(0.0) - 24.0;
            let alpha = (0.5 - d).clamp(0.0, 1.0);
            let ring = (p.length() - 30.0).abs() < 5.0;
            let stripe = y > 14 && y < 26 && d < -8.0;
            let c = if ring || stripe { [0.95, 0.93, 0.85] } else if d > -6.0 { [0.05, 0.05, 0.06] } else { color };
            data.extend_from_slice(&[c[0], c[1], c[2], alpha]);
        }
    }
    save_image(path, w, h, 4, &data, FileFormat::Png8).map_err(|e| e.to_string())
}

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let out = std::path::PathBuf::from(args.first().ok_or("expected output file")?);
    let cube = args.get(1).is_some_and(|m| m == "cube");
    let size: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1024);
    let mesh = if cube { subcraft_mesh::primitives::cube(2.0) } else { subcraft_mesh::primitives::uv_sphere(1.0, 96, 48) };
    let bvh = Bvh::build(&mesh);
    let mut s = Session::new(mesh.clone(), size);
    let ts = s.selected_texture_set().id;
    s.execute(Command::AddLayer(NewLayer::Fill)).map_err(err)?;
    let base = s.selection().layer.unwrap();
    s.execute(Command::SetFillChannel { layer: base, channel: ChannelKind::BaseColor, fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.55, 0.57, 0.6))) }).map_err(err)?;

    let eye = Vec3::new(0.9, 0.7, 1.3).normalize() * 4.4;
    let forward = (-eye).normalize();
    let right = forward.cross(Vec3::Y).normalize();
    let up = right.cross(forward);
    let dir = std::env::temp_dir().join(format!("subcraft-decal-demo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(err)?;
    // Three drops: screen offsets from the view centre, each with its own "up" on screen.
    let drops = [(Vec2::new(0.085, 0.05), up, [0.75, 0.1, 0.08]), (Vec2::new(-0.2, -0.2), up + right, [0.08, 0.3, 0.7]), (Vec2::new(0.24, 0.17), up - right * 0.4, [0.1, 0.5, 0.2])];
    for (i, (at, image_up, color)) in drops.into_iter().enumerate() {
        let file = dir.join(format!("badge{i}.png"));
        write_badge(&file, color)?;
        let at = at * if cube { 1.0 } else { 0.7 };
        let ray = (forward + right * at.x + up * at.y).normalize();
        let hit = bvh.intersect(eye, ray, 0.0, 1000.0).ok_or("the drop missed the model")?;
        let p = mesh.interpolate(hit.tri as usize, hit.u, hit.v);
        s.add_decal_layer(&file, DecalSpot { position: p.position, normal: p.normal, up: image_up })?;
    }
    let _ = std::fs::remove_dir_all(&dir);
    s.update_composites();
    let albedo = s.composite(ts).ok_or("no composite")?.channel(ChannelKind::BaseColor).ok_or("no base color")?;
    let (w, h) = (900u32, 800u32);
    let light = Vec3::new(0.3, 0.8, 0.55).normalize();
    let mut img = vec![0.0f32; (w * h * 3) as usize];
    for y in 0..h {
        for x in 0..w {
            let ndc = Vec2::new((x as f32 + 0.5) / w as f32 * 2.0 - 1.0, 1.0 - (y as f32 + 0.5) / h as f32 * 2.0);
            let ray = (forward + right * ndc.x * 0.42 * w as f32 / h as f32 + up * ndc.y * 0.42).normalize();
            let o = ((y * w + x) * 3) as usize;
            let Some(hit) = bvh.intersect(eye, ray, 0.0, 1000.0) else {
                img[o..o + 3].copy_from_slice(&[0.16, 0.17, 0.19]);
                continue;
            };
            let p = mesh.interpolate(hit.tri as usize, hit.u, hit.v);
            let c = albedo.get(((p.uv.x * size as f32) as u32).min(size - 1), ((p.uv.y * size as f32) as u32).min(size - 1));
            let shade = 0.3 + 0.75 * p.normal.dot(light).max(0.0);
            img[o..o + 3].copy_from_slice(&[0, 1, 2].map(|i| (c[i] * shade).max(0.0).powf(1.0 / 2.2)));
        }
    }
    save_image(&out, w, h, 3, &img, FileFormat::Png8).map_err(|e| e.to_string())
}
