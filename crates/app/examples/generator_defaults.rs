//! Contact sheet of freshly added generators (their defaults) on baked shapes.
//! cargo run -p subcraft-app --example generator_defaults -- <out_prefix> <crate|cube|torus|sphere|slab> [size] [tag]
//! Writes <prefix>_<shape>_3d.png (ray-cast render, one column per setting) and
//! <prefix>_<shape>_uv.png (composited base color of texture set 0 for the same settings).
// The app uses mimalloc; the system heap serializes per-tile buffer allocation across cores.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use glam::{Vec2, Vec3};
use subcraft_app::{Command, NewLayer, Session};
use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake};
use subcraft_core::{ChannelKind, ChannelValue, MeshMapKind};
use subcraft_doc::{FillChannel, FillSource, Generator, GeneratorKind, MaskEffectKind};
use subcraft_image::{FileFormat, TiledImage, save_image};
use subcraft_mesh::{Bvh, primitives};

fn bilinear(img: &TiledImage, uv: Vec2) -> [f32; 4] {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let x = (uv.x * w - 0.5).clamp(0.0, w - 1.0);
    let y = (uv.y * h - 0.5).clamp(0.0, h - 1.0);
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(img.width() - 1), (y0 + 1).min(img.height() - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let [a, b, c, d] = [img.get(x0, y0), img.get(x1, y0), img.get(x0, y1), img.get(x1, y1)];
    std::array::from_fn(|i| (a[i] * (1.0 - fx) + b[i] * fx) * (1.0 - fy) + (c[i] * (1.0 - fx) + d[i] * fx) * fy)
}

/// (label, dirt-slot generator, wear-slot generator) per column: every generator exactly as a
/// fresh "add generator" creates it, so the sheet shows the defaults users start from.
fn settings() -> Vec<(&'static str, Option<Generator>, Option<Generator>)> {
    let ao = serde_json::from_str::<GeneratorKind>("\"ambient_occlusion\"").unwrap_or(GeneratorKind::MeshMap(MeshMapKind::AmbientOcclusion));
    vec![
        ("clean", None, None),
        ("dirt", Some(Generator::new(GeneratorKind::Cavity)), None),
        ("edge wear", None, Some(Generator::new(GeneratorKind::Edges))),
        ("ambient occlusion", Some(Generator::new(ao)), None),
        ("curvature", Some(Generator::new(GeneratorKind::MeshMap(MeshMapKind::Curvature))), None),
        ("dirt + wear", Some(Generator::new(GeneratorKind::Cavity)), Some(Generator::new(GeneratorKind::Edges))),
        ("light (directional)", Some(Generator::new(GeneratorKind::Directional)), None),
        ("position", Some(Generator::new(GeneratorKind::PositionGradient)), None),
    ]
}

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let prefix = args.first().ok_or("expected output prefix")?.clone();
    let shape = args.get(1).map(String::as_str).unwrap_or("crate");
    let size: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(512);
    let mesh = match shape {
        "cube" => primitives::cube(1.6),
        "torus" => primitives::torus(0.8, 0.35, 48, 24),
        "sphere" => primitives::uv_sphere(1.0, 64, 32),
        "slab" => primitives::thin_slab(2.2, 1.4, 0.25),
        _ => primitives::sample_crate(),
    };
    let bvh = Bvh::build(&mesh);
    let mut s = Session::new(mesh.clone(), size);
    let sets: Vec<_> = s.document().texture_sets.iter().map(|t| (t.id, t.material_slot)).collect();
    // layers per set: (dirt layer, wear layer)
    let mut handles = Vec::new();
    let t0 = std::time::Instant::now();
    for &(ts, slot) in &sets {
        s.execute(Command::SelectTextureSet(ts)).map_err(|e| format!("{e:?}"))?;
        let maps = bake(
            &BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: slot },
            &BakeSettings { size, maps: vec![MeshMapKind::Curvature, MeshMapKind::AmbientOcclusion], use_gpu: false, ..Default::default() },
            &Progress::new(),
        )
        .map_err(|e| e.to_string())?;
        s.set_mesh_maps(ts, maps);
        let mut pair = Vec::new();
        for color in [[0.55, 0.56, 0.58], [0.16, 0.10, 0.05], [0.95, 0.72, 0.30]] {
            s.execute(Command::AddLayer(NewLayer::Fill)).map_err(|e| format!("{e:?}"))?;
            let layer = s.selection().layer.unwrap();
            s.execute(Command::SetFillChannel {
                layer,
                channel: ChannelKind::BaseColor,
                fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(color[0], color[1], color[2]))),
            })
            .map_err(|e| format!("{e:?}"))?;
            if color[0] < 0.5 || color[0] > 0.9 {
                s.execute(Command::AddMask { layer, white: false }).map_err(|e| format!("{e:?}"))?;
                s.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Generator(Generator::new(GeneratorKind::Edges)) })
                    .map_err(|e| format!("{e:?}"))?;
                pair.push(layer);
            }
        }
        handles.push((ts, pair[0], pair[1]));
    }
    println!("bake {:.2}s", t0.elapsed().as_secs_f32());
    let mut cols = settings();
    // `legacy` renders the pre-grain model (what older projects keep): no painter controls, old Spread defaults.
    if args.get(3).map(String::as_str) == Some("legacy") {
        for (_, d, w) in cols.iter_mut() {
            for g in [d, w].into_iter().flatten() {
                g.painter = None;
                if let Some(e) = g.edge_spread.as_mut() { e.distance = e.distance.min(1.5); e.variation = 0.7; }
            }
        }
    }
    let (cw, ch) = (720u32, 540u32);
    let per_row = 4u32;
    let rows = (cols.len() as u32).div_ceil(per_row);
    let w = cw * per_row;
    let mut px = vec![0.0f32; (w * ch * rows * 3) as usize];
    let mut uv_px = vec![0.0f32; (size * per_row * size * rows * 3) as usize];
    let light = Vec3::new(0.4, 0.8, 0.5).normalize();
    let (target, eye) = (Vec3::ZERO, Vec3::new(1.38, 1.06, 1.62));
    let forward = (target - eye).normalize();
    let right = forward.cross(Vec3::Y).normalize();
    let up = right.cross(forward);
    for (col, (label, dirt, wear)) in cols.iter().enumerate() {
        let t = std::time::Instant::now();
        for &(ts, dirt_layer, wear_layer) in &handles {
            s.execute(Command::SelectTextureSet(ts)).map_err(|e| format!("{e:?}"))?;
            for (layer, g) in [(dirt_layer, dirt), (wear_layer, wear)] {
                let mut effect = s.selected_texture_set().find(layer).unwrap().mask.as_ref().unwrap().effects[0].as_ref().clone();
                // "off" is a generator whose level can never be reached.
                let g = g.unwrap_or_else(|| {
                    let mut z = Generator::new(GeneratorKind::PositionGradient);
                    z.level = 5.0;
                    z.noise_amount = 0.0;
                    z
                });
                effect.kind = MaskEffectKind::Generator(g);
                s.execute(Command::SetMaskEffect { layer, effect }).map_err(|e| format!("{e:?}"))?;
            }
        }
        s.update_composites();
        println!("{label}: composite {:.2}s", t.elapsed().as_secs_f32());
        let albedo: Vec<TiledImage> =
            sets.iter().map(|&(ts, _)| s.composite(ts).unwrap().channel(ChannelKind::BaseColor).unwrap().clone()).collect();
        for y in 0..ch {
            for x in 0..cw {
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
                let (cx, cy) = (col as u32 % per_row, col as u32 / per_row);
                let o = (((cy * ch + y) * w + cx * cw + x) * 3) as usize;
                px[o..o + 3].copy_from_slice(&color);
            }
        }
        let a0 = &albedo[0];
        let uw = size * per_row;
        let (ux, uy) = (col as u32 % per_row, col as u32 / per_row);
        for y in 0..size {
            for x in 0..size {
                let c = a0.get(x, y);
                let o = (((uy * size + y) * uw + ux * size + x) * 3) as usize;
                for i in 0..3 {
                    uv_px[o + i] = c[i].max(0.0).powf(1.0 / 2.2);
                }
            }
        }
    }
    save_image(std::path::Path::new(&format!("{prefix}_{shape}_3d.png")), w, ch * rows, 3, &px, FileFormat::Png8).map_err(|e| e.to_string())?;
    save_image(std::path::Path::new(&format!("{prefix}_{shape}_uv.png")), size * per_row, size * rows, 3, &uv_px, FileFormat::Png8)
        .map_err(|e| e.to_string())?;
    Ok(())
}
