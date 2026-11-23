//! Sheet of every stitch preset drawn along a pen path on a flat leather-coloured plane.
//! cargo run --release -p subcraft-app --example stitch_sheet -- <out_prefix> [size]
//!
//! One curved path per stitch, top to bottom in the order of `stitch::STITCHES`. Writes
//! <prefix>_lit.png (Base Color lit with the normal of the composited Height) and
//! <prefix>_height.png (Height, mid grey = 0).
use glam::{Vec2, Vec3};
use subcraft_app::{Command, NewLayer, Session, pen::PenEditor, stitch};
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::{FillChannel, FillSource};
use subcraft_image::{FileFormat, save_image};

fn err(e: impl std::fmt::Debug) -> String { format!("{e:?}") }

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let prefix = args.first().ok_or("expected output prefix")?.clone();
    let size: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1024);
    let mut s = Session::new(subcraft_mesh::primitives::plane(2.0, 1), size);
    let ts = s.selected_texture_set().id;
    if !s.channels().contains(&ChannelKind::Height) {
        s.execute(Command::AddChannel { texture_set: ts, kind: ChannelKind::Height }).map_err(err)?;
    }
    s.execute(Command::AddLayer(NewLayer::Fill)).map_err(err)?;
    let leather = s.selection().layer.unwrap();
    s.execute(Command::SetFillChannel { layer: leather, channel: ChannelKind::BaseColor, fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.18, 0.09, 0.05))) }).map_err(err)?;
    let tips = subcraft_app::assets::alphas::builtin();
    let rows = stitch::STITCHES.len();
    for (row, st) in stitch::STITCHES.iter().enumerate() {
        let y = (row as f32 + 0.5) / rows as f32;
        let mut pen = PenEditor::default();
        pen.style = Some(stitch::style(&mut s, st, &tips));
        // A shallow S curve: click, then click and drag for a smooth second point.
        pen.begin(&s, Vec2::new(0.08, y + 0.04), None).map_err(err)?;
        pen.release(&s).map_err(err)?;
        pen.begin(&s, Vec2::new(0.5, y - 0.03), None).map_err(err)?;
        pen.drag_to(Vec2::new(0.62, y - 0.03), false);
        pen.release(&s).map_err(err)?;
        pen.begin(&s, Vec2::new(0.92, y + 0.04), None).map_err(err)?;
        pen.release(&s).map_err(err)?;
        while pen.poll(&mut s, 1 << 20).map_err(err)? {}
        s.execute(Command::SelectLayer(Some(leather))).map_err(err)?;
    }
    s.update_composites();
    let composite = s.composite(ts).ok_or("no composite")?;
    let (albedo, height) = (composite.channel(ChannelKind::BaseColor).ok_or("no base color")?, composite.channel(ChannelKind::Height).ok_or("no height")?);
    let h = |x: i64, y: i64| height.get(x.clamp(0, size as i64 - 1) as u32, y.clamp(0, size as i64 - 1) as u32)[0];
    let light = Vec3::new(-0.5, -0.6, 0.62).normalize();
    let mut lit = vec![0.0f32; (size * size * 3) as usize];
    let mut hgt = lit.clone();
    // Bump strength scaled with the resolution so the relief looks the same at any size.
    let k = 12.0 * size as f32 / 1024.0;
    for y in 0..size as i64 {
        for x in 0..size as i64 {
            let n = Vec3::new(-(h(x + 1, y) - h(x - 1, y)) * k, -(h(x, y + 1) - h(x, y - 1)) * k, 1.0).normalize();
            let c = albedo.get(x as u32, y as u32);
            let shade = 0.25 + 0.9 * n.dot(light).max(0.0);
            let spec = n.dot((light + Vec3::Z).normalize()).max(0.0).powf(30.0) * 0.12;
            let o = ((y as u32 * size + x as u32) * 3) as usize;
            lit[o..o + 3].copy_from_slice(&[0, 1, 2].map(|i| (c[i] * shade + spec).max(0.0).powf(1.0 / 2.2)));
            hgt[o..o + 3].copy_from_slice(&[(0.5 + h(x, y) * 0.5).clamp(0.0, 1.0); 3]);
        }
    }
    for (name, buf) in [("lit", &lit), ("height", &hgt)] {
        save_image(std::path::Path::new(&format!("{prefix}_{name}.png")), size, size, 3, buf, FileFormat::Png8).map_err(|e| e.to_string())?;
    }
    Ok(())
}
