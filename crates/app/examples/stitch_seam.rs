//! A stitched pen path that crosses UV seams: drawn over three faces of a cube (each face is
//! its own UV island), ray-cast from a corner.
//! cargo run -p subcraft-app --example stitch_seam -- <out.png> [size] [stitch or tile name]
use glam::{Vec2, Vec3};
use subcraft_app::{Command, NewLayer, Session, pen::PenEditor, stitch};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{FillChannel, FillSource};
use subcraft_image::{FileFormat, save_image};
use subcraft_mesh::Bvh;

fn err(e: impl std::fmt::Debug) -> String { format!("{e:?}") }

/// UV of the surface point hit from `eye` towards `target`.
fn uv_at(mesh: &subcraft_mesh::Mesh, bvh: &Bvh, eye: Vec3, target: Vec3) -> Option<Vec2> {
    let hit = bvh.intersect(eye, (target - eye).normalize(), 0.0, 1000.0)?;
    Some(mesh.interpolate(hit.tri as usize, hit.u, hit.v).uv)
}

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let out = args.first().ok_or("expected output file")?.clone();
    let size: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1024);
    let name = args.get(2).cloned().unwrap_or_else(|| "Stitch: Saddle".into());
    let mesh = subcraft_mesh::primitives::cube(2.0);
    let bvh = Bvh::build(&mesh);
    let mut s = Session::new(mesh.clone(), size);
    let ts = s.selected_texture_set().id;
    if !s.channels().contains(&ChannelKind::Height) {
        s.execute(Command::AddChannel { texture_set: ts, kind: ChannelKind::Height }).map_err(err)?;
    }
    s.execute(Command::AddLayer(NewLayer::Fill)).map_err(err)?;
    let leather = s.selection().layer.unwrap();
    s.execute(Command::SetFillChannel { layer: leather, channel: ChannelKind::BaseColor, fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.18, 0.09, 0.05))) }).map_err(err)?;
    let mut pen = PenEditor::default();
    // A stitch tile ("Tile: ...") or a brush-tip stitch ("Stitch: ...").
    if let Some(tile) = subcraft_app::assets::stitch_tiles::builtin().iter().find(|t| t.name == name) {
        pen.tile = Some(stitch::sample_tile(&mut s, tile));
    } else {
        let tips = subcraft_app::assets::alphas::builtin();
        pen.style = Some(stitch::style(&mut s, stitch::find(&name).ok_or("unknown stitch")?, &tips));
    }
    // From the front face over the top face to the right face.
    let far = 10.0;
    let clicks = [
        (Vec3::new(-0.6, -0.5, far), Vec3::new(-0.6, -0.5, 0.0)),
        (Vec3::new(-0.2, far, 0.3), Vec3::new(-0.2, 0.0, 0.3)),
        (Vec3::new(far, 0.4, -0.5), Vec3::new(0.0, 0.4, -0.5)),
        (Vec3::new(far, -0.6, 0.6), Vec3::new(0.0, -0.6, 0.6)),
    ];
    for (eye, target) in clicks {
        let uv = uv_at(&mesh, &bvh, eye, target).ok_or("click missed the cube")?;
        pen.begin(&s, uv, None).map_err(err)?;
        pen.release(&s).map_err(err)?;
    }
    while pen.poll(&mut s, 1 << 20).map_err(err)? {}
    s.update_composites();
    let composite = s.composite(ts).ok_or("no composite")?;
    let (albedo, height) = (composite.channel(ChannelKind::BaseColor).ok_or("no base color")?, composite.channel(ChannelKind::Height).ok_or("no height")?);
    let (w, h) = (900u32, 800u32);
    let eye = Vec3::new(1.0, 0.85, 1.25).normalize() * 4.6;
    let forward = (-eye).normalize();
    let right = forward.cross(Vec3::Y).normalize();
    let up = right.cross(forward);
    let light = Vec3::new(0.3, 0.8, 0.55).normalize();
    let mut img = vec![0.0f32; (w * h * 3) as usize];
    for y in 0..h {
        for x in 0..w {
            let ndc = Vec2::new((x as f32 + 0.5) / w as f32 * 2.0 - 1.0, 1.0 - (y as f32 + 0.5) / h as f32 * 2.0);
            let dir = (forward + right * ndc.x * 0.42 * w as f32 / h as f32 + up * ndc.y * 0.42).normalize();
            let o = ((y * w + x) * 3) as usize;
            let Some(hit) = bvh.intersect(eye, dir, 0.0, 1000.0) else {
                img[o..o + 3].copy_from_slice(&[0.16, 0.17, 0.19]);
                continue;
            };
            let p = mesh.interpolate(hit.tri as usize, hit.u, hit.v);
            let (tx, ty) = (((p.uv.x * size as f32) as u32).min(size - 1), ((p.uv.y * size as f32) as u32).min(size - 1));
            let c = albedo.get(tx, ty);
            let raised = height.get(tx, ty)[0];
            let shade = 0.3 + 0.75 * p.normal.dot(light).max(0.0) + raised * 0.35;
            img[o..o + 3].copy_from_slice(&[0, 1, 2].map(|i| (c[i] * shade).max(0.0).powf(1.0 / 2.2)));
        }
    }
    save_image(std::path::Path::new(&out), w, h, 3, &img, FileFormat::Png8).map_err(|e| e.to_string())
}
