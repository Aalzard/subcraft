//! The Auto Stitcher on the sample crate: stitches placed along its UV seams, and around a
//! disc painted into a layer's mask on a plane.
//! cargo run --release -p subcraft-app --example auto_stitch -- <out prefix> [size] [stitch or tile name] [distance in widths]
use glam::{Vec2, Vec3};
use subcraft_app::auto_stitch::{AutoStitch, StitchAlong};
use subcraft_app::{Command, NewLayer, Session, pen::PenEditor, stitch};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{FillChannel, FillSource, MaskEffectKind};
use subcraft_image::{FileFormat, TiledImage, save_image};
use subcraft_mesh::{Bvh, Mesh};

fn err(e: impl std::fmt::Debug) -> String { format!("{e:?}") }

fn session(mesh: &Mesh, size: u32, name: &str) -> Result<(Session, PenEditor), String> {
    let mut s = Session::new(mesh.clone(), size);
    let ts = s.selected_texture_set().id;
    if !s.channels().contains(&ChannelKind::Height) {
        s.execute(Command::AddChannel { texture_set: ts, kind: ChannelKind::Height }).map_err(err)?;
    }
    s.execute(Command::AddLayer(NewLayer::Fill)).map_err(err)?;
    let leather = s.selection().layer.unwrap();
    s.execute(Command::SetFillChannel { layer: leather, channel: ChannelKind::BaseColor, fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.18, 0.09, 0.05))) }).map_err(err)?;
    let mut pen = PenEditor::default();
    if let Some(tile) = subcraft_app::assets::stitch_tiles::builtin().iter().find(|t| t.name == name) {
        pen.tile = Some(stitch::sample_tile(&mut s, tile));
    } else {
        let tips = subcraft_app::assets::alphas::builtin();
        pen.style = Some(stitch::style(&mut s, stitch::find(name).ok_or("unknown stitch")?, &tips));
    }
    Ok((s, pen))
}

/// The model from `eye`, lit, with the height channel as a little shading.
fn render(s: &mut Session, mesh: &Mesh, eye: Vec3, out: &str) -> Result<(), String> {
    let bvh = Bvh::build(mesh);
    s.update_composites();
    let ts = s.selected_texture_set();
    let (id, size) = (ts.id, ts.resolution);
    let composite = s.composite(id).ok_or("no composite")?;
    let (albedo, height) = (composite.channel(ChannelKind::BaseColor).ok_or("no base color")?, composite.channel(ChannelKind::Height).ok_or("no height")?);
    let (w, h) = (900u32, 800u32);
    let forward = (-eye).normalize();
    let right = forward.cross(Vec3::Y).normalize();
    let up = right.cross(forward);
    let light = (eye.normalize() + Vec3::new(-0.3, 0.6, 0.1)).normalize();
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
            let uv = p.uv - p.uv.floor();
            let (tx, ty) = (((uv.x * size as f32) as u32).min(size - 1), ((uv.y * size as f32) as u32).min(size - 1));
            let c = albedo.get(tx, ty);
            let shade = 0.3 + 0.75 * p.normal.dot(light).abs() + height.get(tx, ty)[0] * 0.35;
            img[o..o + 3].copy_from_slice(&[0, 1, 2].map(|i| (c[i] * shade).max(0.0).powf(1.0 / 2.2)));
        }
    }
    save_image(std::path::Path::new(out), w, h, 3, &img, FileFormat::Png8).map_err(|e| e.to_string())
}

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let prefix = args.first().ok_or("expected output prefix")?.clone();
    let size: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1024);
    let name = args.get(2).cloned().unwrap_or_else(|| "Stitch: Saddle".into());
    let inset: f32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1.0);

    // Along the UV seams of a cube: every face is its own island.
    let cube = subcraft_mesh::primitives::cube(2.0);
    let (mut s, mut pen) = session(&cube, size, &name)?;
    let made = pen.auto_stitch(&mut s, AutoStitch { along: StitchAlong::Seams, inset, ..Default::default() }).map_err(err)?;
    println!("seams: {} paths, {} left out", made.paths, made.skipped);
    render(&mut s, &cube, Vec3::new(1.0, 0.85, 1.25).normalize() * 4.6, &format!("{prefix}_seams.png"))?;

    // One row right on every seam, with a stitch tile if one was asked for.
    let (mut s, mut pen) = session(&cube, size, &name)?;
    let made = pen.auto_stitch(&mut s, AutoStitch { along: StitchAlong::Seams, inset: 0.0, ..Default::default() }).map_err(err)?;
    println!("on the seams: {} paths, {} left out", made.paths, made.skipped);
    render(&mut s, &cube, Vec3::new(1.0, 0.85, 1.25).normalize() * 4.6, &format!("{prefix}_on_seams.png"))?;

    // Around a patch: a rounded shape painted into the mask of a fill layer on a plane.
    let plane = subcraft_mesh::primitives::plane(2.0, 8);
    let (mut s, mut pen) = session(&plane, size, &name)?;
    s.execute(Command::AddLayer(NewLayer::Fill)).map_err(err)?;
    let patch = s.selection().layer.unwrap();
    s.execute(Command::SetFillChannel { layer: patch, channel: ChannelKind::BaseColor, fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.5, 0.33, 0.12))) }).map_err(err)?;
    s.execute(Command::AddMask { layer: patch, white: false }).map_err(err)?;
    let shape: Vec<f32> = (0..size * size).flat_map(|i| {
        let p = Vec2::new((i % size) as f32 + 0.5, (i / size) as f32 + 0.5) / size as f32 - Vec2::splat(0.5);
        // A rounded star-like patch.
        let r = 0.27 + 0.05 * (p.y.atan2(p.x) * 5.0).cos();
        [1.0, f32::from(p.length() < r)]
    }).collect();
    s.execute(Command::AddMaskEffect { layer: patch, kind: MaskEffectKind::Paint { image: TiledImage::from_dense(size, size, 2, [0.0; 4], &shape) } }).map_err(err)?;
    s.execute(Command::SelectLayer(Some(patch))).map_err(err)?;
    let made = pen.auto_stitch(&mut s, AutoStitch { along: StitchAlong::MaskEdge, inset, ..Default::default() }).map_err(err)?;
    println!("mask edge: {} paths, {} left out", made.paths, made.skipped);
    render(&mut s, &plane, Vec3::new(0.0, 3.2, 0.9), &format!("{prefix}_mask.png"))
}
