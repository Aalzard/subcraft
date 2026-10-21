//! Contact sheet of Edges wear on the baked default crate (CPU ray-cast, no window).
//! cargo run -p subcraft-app --example wear_sheet -- <out.png> <natural|natural-old|legacy> [variation] [size]
//! Columns: Level 0, .065, .25, .5, .75, 1. Rows: Spread 0, 3, 6.5, 12.5 (legacy: one row).
//! `natural-old` is Natural wear with the original "Level selects baked detail only" behavior.
use glam::{Vec2, Vec3};
use subcraft_app::{Command, NewLayer, Session};
use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake};
use subcraft_core::{ChannelKind, ChannelValue, MeshMapKind};
use subcraft_doc::{FillChannel, FillSource, Generator, GeneratorKind, MaskEffectKind};
use subcraft_image::{FileFormat, TiledImage, save_image};
use subcraft_mesh::{Bvh, primitives};

fn bilinear(img: &TiledImage, uv: Vec2) -> [f32; 4] {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let x = (uv.x * w - 0.5).clamp(0.0, w - 1.0); let y = (uv.y * h - 0.5).clamp(0.0, h - 1.0);
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(img.width() - 1), (y0 + 1).min(img.height() - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let [a, b, c, d] = [img.get(x0, y0), img.get(x1, y0), img.get(x0, y1), img.get(x1, y1)];
    std::array::from_fn(|i| (a[i] * (1.0 - fx) + b[i] * fx) * (1.0 - fy) + (c[i] * (1.0 - fx) + d[i] * fx) * fy)
}

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let out = args.first().ok_or("expected output .png")?;
    let mode = args.get(1).map(String::as_str).unwrap_or("natural");
    let variation: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.85);
    let size: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(512);
    let levels = [0.0f32, 0.065, 0.25, 0.5, 0.75, 1.0];
    let spreads: Vec<f32> = if mode == "legacy" { vec![0.0] } else { vec![0.0, 3.0, 6.5, 12.5] };
    let mesh = primitives::sample_crate(); let bvh = Bvh::build(&mesh);
    let mut s = Session::new(mesh.clone(), size);
    let sets: Vec<_> = s.document().texture_sets.iter().map(|t| (t.id, t.material_slot)).collect();
    let mut handles = Vec::new();
    for &(ts, slot) in &sets {
        s.execute(Command::SelectTextureSet(ts)).map_err(|e| format!("{e:?}"))?;
        let maps = bake(&BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: slot },
            &BakeSettings { size, maps: vec![MeshMapKind::Curvature], ..Default::default() }, &Progress::new()).map_err(|e| e.to_string())?;
        s.set_mesh_maps(ts, maps);
        for (color, wear) in [([0.045, 0.055, 0.065], false), ([0.9, 0.62, 0.2], true)] {
            s.execute(Command::AddLayer(NewLayer::Fill)).map_err(|e| format!("{e:?}"))?;
            let layer = s.selection().layer.unwrap();
            s.execute(Command::SetFillChannel { layer, channel: ChannelKind::BaseColor,
                fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(color[0], color[1], color[2]))) }).map_err(|e| format!("{e:?}"))?;
            if wear {
                s.execute(Command::AddMask { layer, white: false }).map_err(|e| format!("{e:?}"))?;
                s.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Generator(Generator::new(GeneratorKind::Edges)) }).map_err(|e| format!("{e:?}"))?;
                handles.push((ts, layer));
            }
        }
    }
    let (cw, ch) = (300u32, 200u32);
    let (w, h) = (cw * levels.len() as u32, ch * spreads.len() as u32);
    let mut px = vec![0.0f32; (w * h * 3) as usize];
    let light = Vec3::new(0.4, 0.8, 0.5).normalize();
    let (target, eye) = (Vec3::ZERO, Vec3::new(2.3, 1.6, 2.6));
    let forward = (target - eye).normalize(); let right = forward.cross(Vec3::Y).normalize(); let up = right.cross(forward);
    for (row, &spread) in spreads.iter().enumerate() {
        for (col, &level) in levels.iter().enumerate() {
            for &(ts, layer) in &handles {
                s.execute(Command::SelectTextureSet(ts)).map_err(|e| format!("{e:?}"))?;
                let mut effect = s.selected_texture_set().find(layer).unwrap().mask.as_ref().unwrap().effects[0].as_ref().clone();
                let mut g = Generator::new(GeneratorKind::Edges);
                g.level = level; g.noise_amount = 0.0;
                if mode == "legacy" { g.edge_spread = None; } else {
                    let e = g.edge_spread.as_mut().unwrap(); e.distance = spread; e.variation = variation;
                    e.level_reach = mode != "natural-old";
                }
                effect.kind = MaskEffectKind::Generator(g);
                s.execute(Command::SetMaskEffect { layer, effect }).map_err(|e| format!("{e:?}"))?;
            }
            s.update_composites();
            let albedo: Vec<TiledImage> = sets.iter().map(|&(ts, _)| s.composite(ts).unwrap().channel(ChannelKind::BaseColor).unwrap().clone()).collect();
            for y in 0..ch { for x in 0..cw {
                let ndc = Vec2::new((x as f32 + 0.5) / cw as f32 * 2.0 - 1.0, 1.0 - (y as f32 + 0.5) / ch as f32 * 2.0);
                let dir = (forward + right * ndc.x * 0.55 * cw as f32 / ch as f32 + up * ndc.y * 0.55).normalize();
                let color = match bvh.intersect(eye, dir, 0.0, 100.0) {
                    Some(hit) => {
                        let p = mesh.interpolate(hit.tri as usize, hit.u, hit.v);
                        let set = sets.iter().position(|&(_, m)| m == mesh.tri_material[hit.tri as usize]).unwrap();
                        let c = bilinear(&albedo[set], p.uv);
                        let shade = 0.35 + 0.65 * p.normal.dot(light).max(0.0);
                        [0, 1, 2].map(|i| (c[i] * shade).max(0.0).powf(1.0 / 2.2))
                    }
                    None => [0.16, 0.17, 0.19],
                };
                let o = (((row as u32 * ch + y) * w + col as u32 * cw + x) * 3) as usize;
                px[o..o + 3].copy_from_slice(&color);
            } }
            println!("{mode} spread {spread} level {level}");
        }
    }
    save_image(std::path::Path::new(out), w, h, 3, &px, FileFormat::Png8).map_err(|e| e.to_string())?;
    Ok(())
}
