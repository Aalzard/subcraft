//! Developer tool: print what the .spp reader recovers from a Painter project and optionally dump
//! the decoded images as PPM/PGM files. `cargo run --release -p subcraft-mesh --example spp_probe -- file.spp [out_dir]`

use std::path::Path;
use std::time::Instant;

use subcraft_mesh::spp::{BakedKind, LayerInfo, SppFile, channel_bits};

fn tree(layers: &[LayerInfo], depth: usize) {
    for l in layers.iter().rev() {
        let blends: Vec<String> = l.blends.iter().map(|(c, m, o)| format!("{m}@{o:.2}/{c:#x}")).collect();
        println!("{}{:?} '{}'{} mask={} strokes={} sources={} blends=[{}]", "  ".repeat(depth), l.kind, l.name, if l.visible { "" } else { " (hidden)" }, l.has_mask, l.strokes, l.sources.len(), blends.join(" "));
        if std::env::var("SPP_ACTIONS").is_ok() {
            for (label, list) in [("content", &l.content), ("mask", &l.mask)] {
                for a in list {
                    let what = match &a.kind {
                        subcraft_mesh::spp::document::ActionKind::Fill { sources, projection } => format!("fill proj {projection} [{}]", sources.iter().map(|s| match s {
                            subcraft_mesh::spp::SourceInfo::Uniform { channels, color } => format!("uniform {channels:#x} {color:.2?}"),
                            subcraft_mesh::spp::SourceInfo::Bitmap { channels, .. } => format!("bitmap {channels:#x}"),
                            subcraft_mesh::spp::SourceInfo::Procedural { channels, graph } => format!("{} {channels:#x} {:?}", graph.name(), graph.tweaks.iter().filter(|(n, _)| !n.starts_with('$')).take(4).collect::<Vec<_>>()),
                            subcraft_mesh::spp::SourceInfo::Reference { channels, anchor, .. } => format!("anchor {anchor} {channels:#x}"),
                        }).collect::<Vec<_>>().join("; ")),
                        k => format!("{k:?}").chars().take(160).collect(),
                    };
                    println!("{}    {label}: {what} blends={:?}", "  ".repeat(depth), a.blends);
                }
            }
        }
        tree(&l.children, depth + 1);
    }
}

fn dump(plane: &subcraft_mesh::spp::Plane, name: &Path, srgb: bool) {
    let _ = srgb;
    let (w, h, c) = (plane.width as usize, plane.height as usize, plane.components);
    let (magic, out_c) = if c == 1 { ("P5", 1) } else { ("P6", 3) };
    let mut out = format!("{magic}\n{w} {h}\n255\n").into_bytes();
    for i in 0..w * h {
        for k in 0..out_c {
            let v = plane.data[i * c + k.min(c - 1)];
            out.push((v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
        }
    }
    std::fs::write(name, out).unwrap();
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("spp file");
    let out_dir = args.next().filter(|d| d != "-");
    let max_size: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(4096);
    let mut f = SppFile::open(Path::new(&path)).expect("open");
    let t = Instant::now();
    let project = f.project().expect("document");
    println!("document parsed in {:?}", t.elapsed());
    for set in &project.texture_sets {
        println!("== texture set '{}' uid {} {}x{} normal_syscoord={}", set.name, set.uid, 1u32 << set.width_log2, 1u32 << set.height_log2, set.normal_syscoord);
        for c in &set.channels {
            println!("  channel {:#x} format {} '{}'", c.bits, c.format, c.name);
        }
        tree(&set.layers, 1);
        for (usage, url) in &set.mesh_maps {
            println!("  mesh map usage {usage}: {}", url.split('?').next().unwrap_or(url));
        }
        // Does the baked map's coverage (alpha) line up with the model's UV islands as imported?
        if let Ok(Some(p)) = f.baked_map(&set.name, BakedKind::Position, 512) {
            if let Ok(report) = subcraft_mesh::import_mesh(Path::new(&path)) {
                let n = p.width.min(p.height);
                let mut covered = vec![false; (n * n) as usize];
                for t in 0..report.mesh.triangle_count() {
                    subcraft_mesh::uv_raster::raster_triangle(report.mesh.tri_uvs(t), n, n, subcraft_core::IRect::from_size(n, n), 0.0, |h| covered[(h.y as u32 * n + h.x as u32) as usize] = true);
                }
                let (mut hit, mut hit_flipped, mut total) = (0usize, 0usize, 0usize);
                for y in 0..n {
                    for x in 0..n {
                        if covered[(y * n + x) as usize] {
                            total += 1;
                            hit += (p.pixel(x, y)[3] > 0.5) as usize;
                            hit_flipped += (p.pixel(x, n - 1 - y)[3] > 0.5) as usize;
                        }
                    }
                }
                println!("  UV coverage vs baked alpha: as imported {:.3}, v flipped {:.3} ({total} texels)", hit as f32 / total as f32, hit_flipped as f32 / total as f32);
            }
        }
        // Every procedural archive: does it unpack, and does it carry embedded images?
        let mut urls = std::collections::BTreeSet::new();
        subcraft_mesh::spp::document::walk_layers(&set.layers, &mut |l, _| {
            for src in &l.sources {
                if let subcraft_mesh::spp::SourceInfo::Procedural { graph, .. } = src {
                    urls.insert(graph.url.clone());
                }
            }
        }, 0);
        let (mut ok, mut failed, mut with_jpeg) = (0, 0, 0);
        for url in &urls {
            let t = Instant::now();
            match f.embedded_jpegs(url) {
                Ok(j) => {
                    ok += 1;
                    with_jpeg += (!j.is_empty()) as usize;
                    if !j.is_empty() && out_dir.is_some() {
                        let name = url.split('?').next().unwrap_or(url).rsplit('/').next().unwrap_or("x");
                        std::fs::write(Path::new(out_dir.as_ref().unwrap()).join(format!("{name}.jpg")), &j[0]).ok();
                    }
                    if t.elapsed().as_millis() > 500 {
                        println!("  slow archive {url}: {:?}", t.elapsed());
                    }
                }
                Err(e) => {
                    failed += 1;
                    println!("  archive {url}: ERROR {e}");
                }
            }
        }
        println!("  procedural archives: {} unpacked, {failed} failed, {with_jpeg} with embedded JPEG", ok);
        let procedural = subcraft_mesh::spp::document::procedural_resources(&set.layers);
        println!("  procedural resources: {}", procedural.len());
        for kind in f.baked_map_kinds(&set.name).unwrap() {
            let t = Instant::now();
            match f.baked_map(&set.name, kind, max_size) {
                Ok(Some(p)) => {
                    let mean: Vec<f32> = (0..p.components).map(|c| p.data.iter().skip(c).step_by(p.components).sum::<f32>() / (p.width * p.height) as f32).collect();
                    println!("  baked {kind:?}: {}x{}x{} mean {mean:?} in {:?}", p.width, p.height, p.components, t.elapsed());
                    if let Some(d) = &out_dir {
                        dump(&p, &Path::new(d).join(format!("{}_{kind:?}.ppm", set.name)), false);
                    }
                }
                Ok(None) => println!("  baked {kind:?}: missing"),
                Err(e) => println!("  baked {kind:?}: ERROR {e}"),
            }
        }
        for c in &set.channels {
            let t = Instant::now();
            match f.channel_texture(set, c.bits, max_size) {
                Ok(Some(img)) => {
                    let p = &img.plane;
                    let mean: Vec<f32> = (0..p.components).map(|k| p.data.iter().skip(k).step_by(p.components).sum::<f32>() / (p.width * p.height) as f32).collect();
                    println!("  cache {:#x} {:?}: {}x{} mean {mean:?} in {:?}", c.bits, img.format, p.width, p.height, t.elapsed());
                    if let Some(d) = &out_dir {
                        let name = match c.bits {
                            channel_bits::BASE_COLOR => "basecolor".to_string(),
                            b => format!("channel{b:x}"),
                        };
                        dump(p, &Path::new(d).join(format!("{}_cache_{name}.ppm", set.name)), false);
                    }
                }
                Ok(None) => println!("  cache {:#x}: not stored", c.bits),
                Err(e) => println!("  cache {:#x}: ERROR {e}", c.bits),
            }
        }
        let _ = BakedKind::Normal;
        // Is the Normal channel OpenGL (+Y) or DirectX (-Y)? Compare its green channel with the
        // height gradient along image rows (positive correlation = OpenGL).
        if let (Ok(Some(h)), Ok(Some(n))) = (f.channel_texture(set, channel_bits::HEIGHT, 1024), f.channel_texture(set, channel_bits::NORMAL, 1024)) {
            let (h, n) = (&h.plane, &n.plane);
            if h.width == n.width {
                let (mut dot, mut hh, mut gg) = (0.0f64, 0.0f64, 0.0f64);
                for y in 1..h.height - 1 {
                    for x in 0..h.width {
                        let dh = (h.pixel(x, y + 1)[0] - h.pixel(x, y - 1)[0]) as f64;
                        let g = (n.pixel(x, y)[1] - 0.5) as f64;
                        dot += dh * g;
                        hh += dh * dh;
                        gg += g * g;
                    }
                }
                println!("  normal-Y vs height row gradient correlation: {:.3} (positive = OpenGL)", dot / (hh.sqrt() * gg.sqrt() + 1e-12));
            }
        }
    }
}
