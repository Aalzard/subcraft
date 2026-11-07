//! Before/after sheet of the generator micro effect on SubCraft_base with Dusty Plastic.
//! cargo run --release -p subcraft-app --example micro_sheet -- <out_prefix> [size] [zoom]
//! Writes <prefix>_lit.png, <prefix>_height.png, <prefix>_normal.png: one column per setting
//! (dirt off / on, edge wear off / on), the camera framed on the model (zoom > 1 closes in).
//! Also prints composite timings with the micro effect off and on.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use glam::{Vec2, Vec3};
use subcraft_app::{Command, NewLayer, Session};
use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake};
use subcraft_core::{ChannelKind, ChannelValue, MeshMapKind};
use subcraft_doc::{FillChannel, FillSource, Generator, GeneratorKind, MaskEffectKind, MicroEffect};
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

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let prefix = args.first().ok_or("expected output prefix")?.clone();
    let size: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(2048);
    let zoom: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let mesh = subcraft_mesh::import_fbx_bytes(subcraft_assets::default_model::FBX, subcraft_assets::default_model::NAME).map_err(|e| format!("{e:?}"))?.mesh;
    let bvh = Bvh::build(&mesh);
    let mut s = Session::default_model();
    let sets: Vec<_> = s.document().texture_sets.iter().map(|t| (t.id, t.material_slot)).collect();
    if size != 2048 {
        for &(ts, _) in &sets {
            s.execute(Command::SetResolution { texture_set: Some(ts), resolution: size }).map_err(|e| format!("{e:?}"))?;
        }
    }
    let (mut dirt_layers, mut wear_layers) = (Vec::new(), Vec::new());
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
        // A surface layer that carries a (flat) normal map and a height map, under the generators.
        s.execute(Command::AddLayer(NewLayer::Fill)).map_err(|e| format!("{e:?}"))?;
        let surface = s.selection().layer.unwrap();
        s.execute(Command::SetFillChannel { layer: surface, channel: ChannelKind::BaseColor, fill: FillChannel { enabled: false, ..FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.5, 0.5, 0.5))) } }).map_err(|e| format!("{e:?}"))?;
        for (channel, value) in [(ChannelKind::Normal, ChannelValue::rgb(0.5, 0.5, 1.0)), (ChannelKind::Height, ChannelValue::scalar(0.0))] {
            s.execute(Command::SetFillChannel { layer: surface, channel, fill: FillChannel::new(FillSource::uniform(value)) }).map_err(|e| format!("{e:?}"))?;
        }
        for (color, kind, sink) in [([0.16, 0.10, 0.05], GeneratorKind::Cavity, &mut dirt_layers), ([0.95, 0.72, 0.30], GeneratorKind::Edges, &mut wear_layers)] {
            s.execute(Command::AddLayer(NewLayer::Fill)).map_err(|e| format!("{e:?}"))?;
            let layer = s.selection().layer.unwrap();
            s.execute(Command::SetFillChannel { layer, channel: ChannelKind::BaseColor, fill: FillChannel::new(FillSource::uniform(ChannelValue::rgb(color[0], color[1], color[2]))) })
                .map_err(|e| format!("{e:?}"))?;
            s.execute(Command::AddMask { layer, white: false }).map_err(|e| format!("{e:?}"))?;
            s.execute(Command::AddMaskEffect { layer, kind: MaskEffectKind::Generator(Generator::new(kind)) }).map_err(|e| format!("{e:?}"))?;
            sink.push((ts, layer));
        }
    }
    println!("bake {:.2}s", t0.elapsed().as_secs_f32());

    // (label, dirt micro, wear micro)
    let cols: [(&str, bool, bool); 4] = [("dirt off", false, false), ("dirt micro", true, false), ("wear off", false, false), ("wear micro", false, true)];
    let (cw, ch) = (560u32, 420u32);
    let w = cw * cols.len() as u32;
    let mut lit = vec![0.0f32; (w * ch * 3) as usize];
    let mut hgt = lit.clone();
    let mut nrm = lit.clone();
    let (lo, hi) = mesh.positions.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, b), p| (a.min(*p), b.max(*p)));
    let (target, radius) = ((lo + hi) * 0.5, (hi - lo).length() * 0.5);
    let eye = target + Vec3::new(0.9, 0.55, 1.0).normalize() * radius * 2.6 / zoom;
    let forward = (target - eye).normalize();
    let right = forward.cross(Vec3::Y).normalize();
    let up = right.cross(forward);
    let lights = [(Vec3::new(0.7, 0.25, 0.45).normalize(), 1.0f32), (Vec3::new(-0.5, 0.6, 0.3).normalize(), 0.35)];
    for (col, (label, dirt_on, wear_on)) in cols.iter().enumerate() {
        for (layers, on, kind) in [(&dirt_layers, *dirt_on, GeneratorKind::Cavity), (&wear_layers, *wear_on, GeneratorKind::Edges)] {
            for &(ts, layer) in layers {
                s.execute(Command::SelectTextureSet(ts)).map_err(|e| format!("{e:?}"))?;
                let mut effect = s.selected_texture_set().find(layer).unwrap().mask.as_ref().unwrap().effects[0].as_ref().clone();
                let mut g = Generator::new(kind);
                // "off" for the layer that is not under test: no coverage at all.
                if (kind == GeneratorKind::Cavity) != (col < 2) {
                    g = Generator::new(GeneratorKind::PositionGradient);
                    g.level = 5.0;
                    g.noise_amount = 0.0;
                }
                if on { g.micro = Some(MicroEffect::enabled_for(kind)); }
                effect.kind = MaskEffectKind::Generator(g);
                s.execute(Command::SetMaskEffect { layer, effect }).map_err(|e| format!("{e:?}"))?;
            }
        }
        let t = std::time::Instant::now();
        s.update_composites();
        println!("{label}: composite {:.2}s", t.elapsed().as_secs_f32());
        let get = |kind: ChannelKind| -> Vec<TiledImage> { sets.iter().map(|&(ts, _)| s.composite(ts).unwrap().channel(kind).unwrap().clone()).collect() };
        let (albedo, normal, height) = (get(ChannelKind::BaseColor), get(ChannelKind::Normal), get(ChannelKind::Height));
        for y in 0..ch {
            for x in 0..cw {
                let ndc = Vec2::new((x as f32 + 0.5) / cw as f32 * 2.0 - 1.0, 1.0 - (y as f32 + 0.5) / ch as f32 * 2.0);
                let dir = (forward + right * ndc.x * 0.5 * cw as f32 / ch as f32 + up * ndc.y * 0.5).normalize();
                let o = (((y * w) + col as u32 * cw + x) * 3) as usize;
                let bg = [0.16, 0.17, 0.19];
                let Some(hit) = bvh.intersect(eye, dir, 0.0, 1000.0) else {
                    for buf in [&mut lit, &mut hgt, &mut nrm] { buf[o..o + 3].copy_from_slice(&bg); }
                    continue;
                };
                let p = mesh.interpolate(hit.tri as usize, hit.u, hit.v);
                let set = sets.iter().position(|&(_, m)| m == mesh.tri_material[hit.tri as usize]).unwrap_or(0);
                let c = bilinear(&albedo[set], p.uv);
                let n = bilinear(&normal[set], p.uv);
                let h = bilinear(&height[set], p.uv)[0];
                let t = p.tangent.truncate();
                let bt = p.tangent.w * p.normal.cross(t);
                let (tx, ty, tz) = (n[0] * 2.0 - 1.0, n[1] * 2.0 - 1.0, n[2] * 2.0 - 1.0);
                let nw = (t * tx + bt * ty + p.normal * tz).normalize_or(p.normal);
                let view = -dir;
                let mut shade = 0.18;
                let mut spec = 0.0;
                for (l, k) in lights {
                    shade += 0.8 * k * nw.dot(l).max(0.0);
                    spec += k * nw.dot((l + view).normalize()).max(0.0).powf(40.0) * 0.5;
                }
                lit[o..o + 3].copy_from_slice(&[0, 1, 2].map(|i| (c[i] * shade + spec).max(0.0).powf(1.0 / 2.2)));
                let g = (0.5 + h * 5.0).clamp(0.0, 1.0);
                hgt[o..o + 3].copy_from_slice(&[g; 3]);
                nrm[o..o + 3].copy_from_slice(&[n[0], n[1], n[2]]);
            }
        }
    }
    // Timings: both generators live; alternate micro off / on and recomposite everything.
    for round in 0..3 {
        for on in if round % 2 == 0 { [false, true] } else { [true, false] } {
            for (layers, kind) in [(&dirt_layers, GeneratorKind::Cavity), (&wear_layers, GeneratorKind::Edges)] {
                for &(ts, layer) in layers {
                    s.execute(Command::SelectTextureSet(ts)).map_err(|e| format!("{e:?}"))?;
                    let mut effect = s.selected_texture_set().find(layer).unwrap().mask.as_ref().unwrap().effects[0].as_ref().clone();
                    let mut g = Generator::new(kind);
                    g.noise.seed = g.noise.seed.wrapping_add(round + 1); // a new mask each round; the second state of a round reuses its noise fields
                    if on { g.micro = Some(MicroEffect::enabled_for(kind)); }
                    effect.kind = MaskEffectKind::Generator(g);
                    s.execute(Command::SetMaskEffect { layer, effect }).map_err(|e| format!("{e:?}"))?;
                }
            }
            let t = std::time::Instant::now();
            s.update_composites();
            println!("timing round {round} micro {}: full recomposite {:.3}s", if on { "ON " } else { "OFF" }, t.elapsed().as_secs_f32());
        }
    }
    for (name, buf) in [("lit", &lit), ("height", &hgt), ("normal", &nrm)] {
        save_image(std::path::Path::new(&format!("{prefix}_{name}.png")), w, ch, 3, buf, FileFormat::Png8).map_err(|e| e.to_string())?;
    }
    Ok(())
}
