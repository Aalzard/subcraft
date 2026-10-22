//! Reproducible CPU view of Edges wear on the actual default crate (no GPU/window needed).
//! cargo run -p subcraft-app --example wear_render -- <out.png> [level] [spread %] [variation] [breakup] [natural|legacy] [size] [zoom] [target x y z]
//! Bakes Curvature for Body/Trim, adds charcoal + gold Edges wear and ray-casts two 3/4 views.
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
    let out = args.first().ok_or("expected output .png path")?;
    let arg = |i: usize, d: f32| args.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    let (level, spread, variation, breakup) = (arg(1, 0.065), arg(2, 12.5), arg(3, 0.85), arg(4, 0.0));
    let natural = !args.get(5).is_some_and(|s| s == "legacy");
    let size = arg(6, 1024.0) as u32;
    let zoom = arg(7, 1.0);
    let target = Vec3::new(arg(8, 0.0), arg(9, 0.0), arg(10, 0.0));
    let mesh = primitives::sample_crate(); let bvh = Bvh::build(&mesh);
    let mut s = Session::new(mesh.clone(), size);
    let sets: Vec<_> = s.document().texture_sets.iter().map(|t| (t.id, t.material_slot)).collect();
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
                let mut g = Generator::new(GeneratorKind::Edges);
                g.level = level; g.noise_amount = breakup;
                if natural { let e = g.edge_spread.as_mut().unwrap(); e.distance = spread; e.variation = variation; } else { g.edge_spread = None; }
                s.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Generator(g) }).map_err(|e| format!("{e:?}"))?;
            }
        }
    }
    s.update_composites();
    let albedo: Vec<TiledImage> = sets.iter().map(|&(ts, _)| s.composite(ts).unwrap().channel(ChannelKind::BaseColor).unwrap().clone()).collect();
    let slot_index = |slot: u32| sets.iter().position(|&(_, m)| m == slot).unwrap();
    let (w, h) = (900u32, 600u32);
    let mut px = vec![0.0f32; (w * h * 3 * 2) as usize];
    let light = Vec3::new(0.4, 0.8, 0.5).normalize();
    for (view, eye) in [Vec3::new(2.3, 1.6, 2.6), Vec3::new(-2.6, 1.9, -2.0)].into_iter().enumerate() {
        let eye = eye + target; let forward = (target - eye).normalize(); let right = forward.cross(Vec3::Y).normalize(); let up = right.cross(forward);
        for y in 0..h { for x in 0..w {
            let ndc = Vec2::new((x as f32 + 0.5) / w as f32 * 2.0 - 1.0, 1.0 - (y as f32 + 0.5) / h as f32 * 2.0);
            let dir = (forward + right * ndc.x * 0.55 / zoom * w as f32 / h as f32 + up * ndc.y * 0.55 / zoom).normalize();
            let color = match bvh.intersect(eye, dir, 0.0, 100.0) {
                Some(hit) => {
                    let p = mesh.interpolate(hit.tri as usize, hit.u, hit.v);
                    let c = bilinear(&albedo[slot_index(mesh.tri_material[hit.tri as usize])], p.uv);
                    let shade = 0.35 + 0.65 * p.normal.dot(light).max(0.0);
                    // Composite is linear; encode approximately for display.
                    [0, 1, 2].map(|i| (c[i] * shade).max(0.0).powf(1.0 / 2.2))
                }
                None => [0.16, 0.17, 0.19],
            };
            let o = ((y * w * 2 + x + view as u32 * w) * 3) as usize;
            px[o..o + 3].copy_from_slice(&color);
        } }
    }
    save_image(std::path::Path::new(out), w * 2, h, 3, &px, FileFormat::Png8).map_err(|e| e.to_string())?;
    println!("level {level} spread {spread}% variation {variation} breakup {breakup} natural {natural} size {size} -> {out}");
    Ok(())
}
