//! Edge wear sheet on SubCraft_base: one column per Wear Level / Spread pair, default settings
//! otherwise, on a plain painted surface.
//! cargo run --release -p subcraft-app --example edge_wear_sheet -- <out_prefix> <size> <zoom> <level:spread>...
//! Writes <prefix>_lit.png (ray-cast and lit) and <prefix>_mask.png (white = worn).
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use glam::{Vec2, Vec3};
use subcraft_app::{Command, NewLayer, Session};
use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake};
use subcraft_core::{ChannelKind, ChannelValue, MeshMapKind};
use subcraft_doc::{FillChannel, FillSource, Generator, GeneratorKind, MaskEffectKind};
use subcraft_image::{FileFormat, TiledImage, save_image};
use subcraft_mesh::Bvh;

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

const SURFACE: [f32; 3] = [0.22, 0.30, 0.36];
const DIRT: [f32; 3] = [0.10, 0.07, 0.04];
const WEAR: [f32; 3] = [0.80, 0.78, 0.74];

fn err(e: impl std::fmt::Debug) -> String { format!("{e:?}") }

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let prefix = args.first().ok_or("expected output prefix")?.clone();
    let size: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(2048);
    let zoom: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let pairs: Vec<(f32, f32)> = args[3..].iter().map(|a| { let (l, s) = a.split_once(':').unwrap(); (l.parse().unwrap(), s.parse().unwrap()) }).collect();
    let mesh = subcraft_mesh::import_fbx_bytes(subcraft_assets::default_model::FBX, subcraft_assets::default_model::NAME).map_err(err)?.mesh;
    let bvh = Bvh::build(&mesh);
    let mut s = Session::default_model();
    let sets: Vec<_> = s.document().texture_sets.iter().map(|t| (t.id, t.material_slot)).collect();
    if size != 2048 {
        for &(ts, _) in &sets {
            s.execute(Command::SetResolution { texture_set: Some(ts), resolution: size }).map_err(err)?;
        }
    }
    let (mut dirt_layers, mut wear_layers) = (Vec::new(), Vec::new());
    let t0 = std::time::Instant::now();
    for &(ts, slot) in &sets {
        s.execute(Command::SelectTextureSet(ts)).map_err(err)?;
        let maps = bake(
            &BakeInput { low: Geometry { mesh: &mesh, bvh: &bvh }, high: None, material_slot: slot },
            &BakeSettings { size, maps: vec![MeshMapKind::Curvature, MeshMapKind::AmbientOcclusion], use_gpu: false, ..Default::default() },
            &Progress::new(),
        )
        .map_err(|e| e.to_string())?;
        s.set_mesh_maps(ts, maps);
        // Surface: a painted-metal grey with a procedural Height of raised plates and grooves
        // between them (tri-planar, so it is not in the baked maps at all).
        s.execute(Command::AddLayer(NewLayer::Fill)).map_err(err)?;
        let surface = s.selection().layer.unwrap();
        s.execute(Command::SetFillChannel { layer: surface, channel: ChannelKind::BaseColor, fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(SURFACE[0], SURFACE[1], SURFACE[2]))) }).map_err(err)?;
        s.execute(Command::SetFillChannel { layer: surface, channel: ChannelKind::Normal, fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.5, 0.5, 1.0))) }).map_err(err)?;
        for (color, kind, sink) in [(DIRT, GeneratorKind::Cavity, &mut dirt_layers), (WEAR, GeneratorKind::Edges, &mut wear_layers)] {
            s.execute(Command::AddLayer(NewLayer::Fill)).map_err(err)?;
            let layer = s.selection().layer.unwrap();
            s.execute(Command::SetFillChannel { layer, channel: ChannelKind::BaseColor, fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(color[0], color[1], color[2]))) }).map_err(err)?;
            s.execute(Command::AddMask { layer, white: false }).map_err(err)?;
            s.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Generator(Generator::new(kind)) }).map_err(err)?;
            sink.push((ts, layer));
        }
    }
    println!("bake {:.2}s", t0.elapsed().as_secs_f32());

    let set_generators = |s: &mut Session, under_test: GeneratorKind, level: f32, spread: f32| -> Result<(), String> {
        for (layers, kind) in [(&dirt_layers, GeneratorKind::Cavity), (&wear_layers, GeneratorKind::Edges)] {
            for &(ts, layer) in layers {
                s.execute(Command::SelectTextureSet(ts)).map_err(err)?;
                let mut effect = s.selected_texture_set().find(layer).unwrap().mask.as_ref().unwrap().effects[0].as_ref().clone();
                let mut g = Generator::new(kind);
                if kind != under_test {
                    // The layer not under test covers nothing.
                    g = Generator::new(GeneratorKind::PositionGradient);
                    g.level = 5.0;
                } else {
                    g.level = level;
                    if let Some(e) = &mut g.edge_spread { e.distance = spread; }
                }
                effect.kind = MaskEffectKind::Generator(g);
                s.execute(Command::SetMaskEffect { layer, effect }).map_err(err)?;
            }
        }
        Ok(())
    };

    let cols: Vec<(String, GeneratorKind, (f32, f32))> = pairs.iter().map(|&(l, sp)| (format!("wear {l} spread {sp}"), GeneratorKind::Edges, (l, sp))).collect();
    let (cw, ch) = (560u32, 420u32);
    let w = cw * cols.len() as u32;
    let mut lit = vec![0.0f32; (w * ch * 3) as usize];
    let mut msk = lit.clone();
    let (lo, hi) = mesh.positions.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, b), p| (a.min(*p), b.max(*p)));
    let (target, radius) = ((lo + hi) * 0.5, (hi - lo).length() * 0.5);
    println!("bounds {lo} {hi}");
    let aim: Vec3 = std::env::var("AIM").ok().map(|v| { let p: Vec<f32> = v.split(',').map(|x| x.parse().unwrap()).collect(); Vec3::new(p[0], p[1], p[2]) }).unwrap_or(target);
    let target = aim;
    let eye = target + Vec3::new(0.9, 0.55, 1.0).normalize() * radius * 2.6 / zoom;
    let forward = (target - eye).normalize();
    let right = forward.cross(Vec3::Y).normalize();
    let up = right.cross(forward);
    let lights = [(Vec3::new(0.7, 0.25, 0.45).normalize(), 1.0f32), (Vec3::new(-0.5, 0.6, 0.3).normalize(), 0.35)];
    for (col, (label, kind, (level, spread))) in cols.iter().enumerate() {
        set_generators(&mut s, *kind, *level, *spread)?;
        let t = std::time::Instant::now();
        s.update_composites();
        println!("{label}: composite {:.2}s", t.elapsed().as_secs_f32());
        let get = |kind: ChannelKind| -> Vec<TiledImage> { sets.iter().map(|&(ts, _)| s.composite(ts).unwrap().channel(kind).unwrap().clone()).collect() };
        let (albedo, normal) = (get(ChannelKind::BaseColor), get(ChannelKind::Normal));
        // The mask under test, recovered from the base colour: the surface is uniform and only the
        // generator layer under test covers anything.
        let paint = if *kind == GeneratorKind::Cavity { DIRT } else { WEAR };
        let span: Vec<f32> = (0..3).map(|i| paint[i] - SURFACE[i]).collect();
        let norm = span.iter().map(|v| v * v).sum::<f32>();
        for y in 0..ch {
            for x in 0..cw {
                let ndc = Vec2::new((x as f32 + 0.5) / cw as f32 * 2.0 - 1.0, 1.0 - (y as f32 + 0.5) / ch as f32 * 2.0);
                let dir = (forward + right * ndc.x * 0.5 * cw as f32 / ch as f32 + up * ndc.y * 0.5).normalize();
                let o = (((y * w) + col as u32 * cw + x) * 3) as usize;
                let bg = [0.16, 0.17, 0.19];
                let Some(hit) = bvh.intersect(eye, dir, 0.0, 1000.0) else {
                    for buf in [&mut lit, &mut msk] {
                        buf[o..o + 3].copy_from_slice(&bg);
                    }
                    continue;
                };
                let p = mesh.interpolate(hit.tri as usize, hit.u, hit.v);
                let set = sets.iter().position(|&(_, m)| m == mesh.tri_material[hit.tri as usize]).unwrap_or(0);
                let c = bilinear(&albedo[set], p.uv);
                let n = bilinear(&normal[set], p.uv);
                let m = (0..3).map(|i| (c[i] - SURFACE[i]) * span[i]).sum::<f32>() / norm;
                let t = p.tangent.truncate();
                let bt = p.tangent.w * p.normal.cross(t);
                let (tx, ty, tz) = (n[0] * 2.0 - 1.0, n[1] * 2.0 - 1.0, n[2] * 2.0 - 1.0);
                let nw = (t * tx + bt * ty + p.normal * tz).normalize_or(p.normal);
                let view = -dir;
                let mut shade = 0.18;
                let mut spec = 0.0;
                for (l, k) in lights {
                    shade += 0.8 * k * nw.dot(l).max(0.0);
                    spec += k * nw.dot((l + view).normalize()).max(0.0).powf(40.0) * 0.4;
                }
                lit[o..o + 3].copy_from_slice(&[0, 1, 2].map(|i| (c[i] * shade + spec).max(0.0).powf(1.0 / 2.2)));
                // Mask: white where covered, lightly shaded so the shape still reads.
                let g = (0.12 + 0.88 * m.clamp(0.0, 1.0)) * (0.55 + 0.45 * shade.min(1.0));
                msk[o..o + 3].copy_from_slice(&[g.powf(1.0 / 2.2); 3]);
            }
        }
    }
    for (name, buf) in [("lit", &lit), ("mask", &msk)] {
        save_image(std::path::Path::new(&format!("{prefix}_{name}.png")), w, ch, 3, buf, FileFormat::Png8).map_err(|e| e.to_string())?;
    }
    Ok(())
}
