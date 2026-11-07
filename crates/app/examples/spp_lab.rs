//! Developer tool for matching the rebuilt Painter stack against Painter's flattened result.
//!
//! `SUBCRAFT_SPP_MAX_RESOLUTION=256 cargo run --release -p subcraft-app --example spp_lab -- file.spp out_dir [prefix]`
//!
//! Opens the project like the app, then writes raw little-endian f32 images (`<name>.f32`, row-major,
//! components interleaved, size in `index.txt`) of Painter's flattened channels (`flat_*`), the rebuilt
//! stack composited alone (`rebuilt_*`), the baked maps (`map_*`) and the coverage (`coverage`), and
//! prints the mean absolute difference per channel. With `prefix` it also composites the rebuilt stack
//! with only its first N top-level entries (bottom first) and prints the difference after each one.
//! Nothing is written into the project; the output directory is for analysis only.

use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use subcraft_core::ChannelKind;
use subcraft_doc::{LayerContent, TextureSet};
use subcraft_image::TiledImage;

fn dense(img: &TiledImage, comps: usize) -> Vec<f32> {
    let (w, h) = (img.width(), img.height());
    let mut out = Vec::with_capacity((w * h) as usize * comps);
    for y in 0..h {
        for x in 0..w {
            let p = img.get(x, y);
            out.extend_from_slice(&p[..comps]);
        }
    }
    out
}

fn write_f32(dir: &Path, name: &str, w: u32, h: u32, comps: usize, data: &[f32], index: &mut std::fs::File) {
    let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_le_bytes()).collect();
    std::fs::write(dir.join(format!("{name}.f32")), bytes).unwrap();
    writeln!(index, "{name} {w} {h} {comps}").unwrap();
}

fn error(a: &[f32], b: &[f32], comps: usize, cov: &[bool]) -> f64 {
    let (mut s, mut n) = (0.0f64, 0usize);
    for (i, c) in cov.iter().enumerate() {
        if *c {
            for k in 0..comps {
                s += (a[i * comps + k] - b[i * comps + k]).abs() as f64;
                n += 1;
            }
        }
    }
    s / n.max(1) as f64
}

/// Texels not covered but within `r` texels (Chebyshev) of a covered one.
fn outside_band(cov: &[bool], w: u32, h: u32, r: i32) -> Vec<bool> {
    let (w, h) = (w as i32, h as i32);
    let mut out = vec![false; cov.len()];
    for y in 0..h {
        for x in 0..w {
            if cov[(y * w + x) as usize] {
                continue;
            }
            'n: for dy in -r..=r {
                for dx in -r..=r {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx >= 0 && ny >= 0 && nx < w && ny < h && cov[(ny * w + nx) as usize] {
                        out[(y * w + x) as usize] = true;
                        break 'n;
                    }
                }
            }
        }
    }
    out
}

fn composite(session: &subcraft_app::Session, ts: &TextureSet) -> subcraft_compositor::Composite {
    let geometry = session.scene().geometry.get(ts.material_slot, ts.resolution);
    let empty = subcraft_materials::MeshMaps::default();
    let maps = session.mesh_maps(ts.id).unwrap_or(&empty);
    subcraft_compositor::composite_full(ts, &subcraft_compositor::EvalContext { geometry: &geometry, resources: session.resources(), mesh_maps: maps })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("path to an .spp file");
    let dir = Path::new(args.get(2).expect("output directory"));
    let prefix = args.get(3).is_some_and(|a| a == "prefix");
    let decals = args.get(3).is_some_and(|a| a == "decals");
    let blends = args.get(3).is_some_and(|a| a == "blends");
    std::fs::create_dir_all(dir).unwrap();
    if args.get(3).is_some_and(|a| a == "sbsar") {
        sbsar_xml(path, &args[4]);
        return;
    }
    if args.get(3).is_some_and(|a| a == "sheets") {
        // `sheets file.sbsasm count`: the layout found for a dumped assembly (sizes before reduction).
        let asm = std::fs::read(&args[4]).unwrap();
        let t = std::time::Instant::now();
        let found = subcraft_mesh::spp::sheets::read_sheets(&asm, args[5].parse().unwrap());
        println!("{:?} in {:?}", found.map(|v| v.iter().map(|s| (s.width, s.height)).collect::<Vec<_>>()), t.elapsed());
        return;
    }
    if args.get(3).is_some_and(|a| a == "gens") {
        // `gens`: every mesh-map generator mask effect (graph name and its set tweaks), per layer.
        let mut spp = subcraft_mesh::spp::SppFile::open(Path::new(path)).unwrap();
        let project = spp.project().unwrap();
        for ts in &project.texture_sets {
            subcraft_mesh::spp::document::walk_layers(&ts.layers, &mut |l, d| {
                for a in &l.mask {
                    if let subcraft_mesh::spp::document::ActionKind::Fill { sources, .. } = &a.kind {
                        for src in sources {
                            if let subcraft_mesh::spp::document::SourceInfo::Procedural { graph, .. } = src {
                                if graph.name().starts_with("mg_") || graph.name().contains("wear") {
                                    let eff: Vec<String> = l.mask.iter().map(|a| match &a.kind {
                                subcraft_mesh::spp::document::ActionKind::Fill { sources, .. } => match sources.first() {
                                    Some(subcraft_mesh::spp::document::SourceInfo::Procedural { graph, .. }) => graph.name().to_string(),
                                    Some(subcraft_mesh::spp::document::SourceInfo::Uniform { color, .. }) => format!("uniform{:.2}", color[0]),
                                    _ => "fill".into(),
                                },
                                subcraft_mesh::spp::document::ActionKind::Filter { graph } => format!("filter:{}", graph.name()),
                                subcraft_mesh::spp::document::ActionKind::Levels(_) => "levels".into(),
                                subcraft_mesh::spp::document::ActionKind::Paint { .. } => "paint".into(),
                                subcraft_mesh::spp::document::ActionKind::Eraser { .. } => "erase".into(),
                                subcraft_mesh::spp::document::ActionKind::Decal { .. } => "decal".into(),
                                other => format!("{:?}", other).chars().take(10).collect(),
                            }).collect();
                            println!("{}{} | {} | {:?} | {}", " ".repeat(d), l.name, graph.name(), graph.tweaks, eff.join(">"));
                                }
                            }
                        }
                    }
                }
            }, 0);
        }
        return;
    }
    if args.get(3).is_some_and(|a| a == "stencils") {
        // `stencils`: every Substance-graph stencil of the strokes (url, stroke count) and what the
        // archive holds: file sizes and the image signatures found inside the compiled graph.
        let mut spp = subcraft_mesh::spp::SppFile::open(Path::new(path)).unwrap();
        let project = spp.project().unwrap();
        let mut urls: std::collections::BTreeMap<String, usize> = Default::default();
        for ts in &project.texture_sets {
            subcraft_mesh::spp::document::walk_layers(&ts.layers, &mut |l, _| {
                for a in l.content.iter().chain(&l.mask) {
                    if let subcraft_mesh::spp::document::ActionKind::Paint { brush: Some(b), strokes, .. } | subcraft_mesh::spp::document::ActionKind::Eraser { brush: Some(b), strokes, .. } = &a.kind {
                        if let Some((url, _)) = &b.stencil {
                            if url.contains(".sbsar") {
                                *urls.entry(url.clone()).or_default() += strokes.len();
                                if url.contains("your_assets") {
                                    println!("tweaks {} brushsize {:.3} points {} {:?}", url.rsplit('/').next().unwrap_or("").split('?').next().unwrap_or(""), b.size, strokes.iter().map(|s| s.points.len()).sum::<usize>(), b.stencil_tweaks);
                                }
                            }
                        }
                    }
                }
            }, 0);
        }
        let sigs: [(&str, &[u8]); 8] = [("png", &[0x89, 0x50, 0x4e, 0x47]), ("jpeg", &[0xff, 0xd8, 0xff]), ("dds", b"DDS "), ("zlib78", &[0x78, 0x9c]), ("zlib78da", &[0x78, 0xda]), ("lz4frame", &[0x04, 0x22, 0x4d, 0x18]), ("zstd", &[0x28, 0xb5, 0x2f, 0xfd]), ("ktx", &[0xab, 0x4b, 0x54, 0x58])];
        for (url, n) in urls {
            println!("{} strokes={n}", url.split('?').next().unwrap_or(""));
            let Ok(Some(files)) = spp.sbsar_files(&url) else { println!("  no archive"); continue };
            for f in files.iter().filter(|_| std::env::var_os("STENCIL_XML").is_some()) {
                if f.name.ends_with(".xml") && url.contains("your_assets") {
                    println!("{}", String::from_utf8_lossy(&f.data));
                }
            }
            if url.contains("your_assets") {
                let t = std::time::Instant::now();
                match spp.stencil_sheets(&url, 4096) {
                    Ok(Some(sheets)) => println!("  sheets: {:?} in {:?}", sheets.iter().map(|s| (s.width, s.height)).collect::<Vec<_>>(), t.elapsed()),
                    other => println!("  sheets: none ({:?})", other.map(|o| o.is_some())),
                }
            }
            if let Ok(dump) = std::env::var("STENCIL_DUMP") {
                for f in files.iter().filter(|f| f.name.ends_with(".sbsasm")) {
                    let key = url.split('?').next().unwrap_or("").rsplit('/').next().unwrap_or("x").to_string();
                    std::fs::write(Path::new(&dump).join(format!("{key}.sbsasm")), &f.data).unwrap();
                }
            }
            for f in &files {
                let counts: Vec<String> = sigs.iter().map(|(nm, sg)| format!("{nm}={}", f.data.windows(sg.len()).filter(|w| w == sg).count())).collect();
                let mut hist = [0usize; 256];
                for b in f.data.iter().step_by(97) { hist[*b as usize] += 1; }
                let tot: usize = hist.iter().sum();
                let ent: f64 = hist.iter().filter(|c| **c > 0).map(|c| { let p = *c as f64 / tot as f64; -p * p.log2() }).sum();
                println!("  {} {} bytes entropy {:.2} bits/byte; {}; head {:02x?}", f.name, f.data.len(), ent, counts.join(" "), &f.data[..f.data.len().min(48)]);
            }
        }
        return;
    }
    if args.get(3).is_some_and(|a| a == "jpegs") {
        // `jpegs`: the JPEGs embedded in each grunge/procedural archive the project uses, with sizes.
        let mut spp = subcraft_mesh::spp::SppFile::open(Path::new(path)).unwrap();
        let project = spp.project().unwrap();
        let mut urls = std::collections::BTreeSet::new();
        subcraft_mesh::spp::document::walk_layers(&project.texture_sets[0].layers, &mut |l, _| {
            for a in l.content.iter().chain(&l.mask) {
                if let subcraft_mesh::spp::document::ActionKind::Fill { sources, .. } = &a.kind {
                    for s in sources {
                        if let subcraft_mesh::spp::SourceInfo::Procedural { graph, .. } = s {
                            urls.insert(graph.url.clone());
                        }
                    }
                }
            }
        }, 0);
        // Extra URLs (one per line) from a file given after `jpegs`.
        if let Some(list) = args.get(4) {
            urls.extend(std::fs::read_to_string(list).unwrap_or_default().lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()));
        }
        for url in urls {
            let sizes: Vec<String> = spp.embedded_jpegs(&url).unwrap_or_default().iter().filter_map(|b| subcraft_image::io::decode_bitmap(b, Some("x.jpg")).ok()).map(|bm| format!("{}x{}", bm.width, bm.height)).collect();
            if !sizes.is_empty() {
                println!("{} {:?}", url.split('?').next().unwrap_or(""), sizes);
            } else if let Ok(Some(files)) = spp.sbsar_files(&url) {
                let list: Vec<String> = files.iter().map(|f| format!("{} ({} bytes, png {})", f.name, f.data.len(), f.data.windows(8).filter(|w| *w == [0x89u8, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]).count())).collect();
                println!("{} no JPEG; files {:?}", url.split('?').next().unwrap_or(""), list);
            }
        }
        return;
    }
    if args.get(3).is_some_and(|a| a == "images") {
        dump_images(path, dir);
        return;
    }
    let t = std::time::Instant::now();
    let (session, notes) = subcraft_app::Session::open_mesh(Path::new(path), 1024).expect("open");
    for n in &notes {
        println!("{n}");
    }
    let mesh = &session.scene().mesh;
    let objs: std::collections::BTreeSet<_> = mesh.tri_object.iter().collect();
    println!("opened in {:?}; uv per diagonal {:.4}; {} objects, {} distinct triangle objects, {} elements", t.elapsed(), uv_per_diagonal(mesh), mesh.objects.len(), objs.len(), subcraft_mesh::topology::mesh_elements(mesh).iter().max().map_or(0, |m| m + 1));
    if decals {
        decal_conventions(&session, path);
        return;
    }
    let ts = &session.document().texture_sets[0];
    let (w, h) = (ts.resolution, ts.resolution);
    let mut index = std::fs::File::create(dir.join("index.txt")).unwrap();
    let flat = match &ts.layers[0].content {
        LayerContent::Paint(p) => p.channels.clone(),
        _ => panic!("no flattened layer"),
    };
    let folder = ts.layers.iter().find(|l| matches!(l.content, LayerContent::Group(_))).expect("rebuilt folder").clone();
    let maps = session.mesh_maps(ts.id).cloned().unwrap_or_default();
    for (kind, img) in maps.iter() {
        let comps = kind.components();
        write_f32(dir, &format!("map_{kind:?}"), img.width(), img.height(), comps, &dense(img, comps), &mut index);
    }
    // Coverage: texels whose UV belongs to a triangle of the set.
    let geometry = session.scene().geometry.get(ts.material_slot, ts.resolution);
    let mut cov = vec![false; (w * h) as usize];
    let mut tri = vec![-1.0f32; (w * h) as usize];
    let mut pos = vec![0.0f32; (w * h) as usize * 3];
    for t in 0..geometry.tile_count() {
        let tile = geometry.tile(t);
        if tile.is_empty() {
            continue;
        }
        for i in 0..(subcraft_image::TILE_SIZE * subcraft_image::TILE_SIZE) as usize {
            let (x, y) = tile.texel_xy(i);
            if x < w && y < h {
                cov[(y * w + x) as usize] = tile.coverage(i) > 0.5;
                tri[(y * w + x) as usize] = tile.triangle(i).map_or(-1.0, |t| t as f32);
                let o = (y * w + x) as usize * 3;
                let p = glam::Vec3::from(tile.position(i)) * geometry.max_extent + geometry.bounds.min;
                pos[o..o + 3].copy_from_slice(&p.to_array());
            }
        }
    }
    write_f32(dir, "coverage", w, h, 1, &cov.iter().map(|c| *c as u8 as f32).collect::<Vec<_>>(), &mut index);
    write_f32(dir, "triangle", w, h, 1, &tri, &mut index);
    // Object-space position of every covered texel (for projecting texels into Painter's views).
    write_f32(dir, "position", w, h, 3, &pos, &mut index);
    // Regions: the mesh object and the connected element of every covered texel (-1 outside), with
    // the object names in `objects.txt`, for per-part errors.
    let elements = geometry.triangle_parts();
    let region = |f: &dyn Fn(usize) -> Option<u32>| tri.iter().map(|&t| if t < 0.0 { -1.0 } else { f(t as usize).map_or(-1.0, |v| v as f32) }).collect::<Vec<f32>>();
    write_f32(dir, "object", w, h, 1, &region(&|t| mesh.tri_object.get(t).copied().flatten()), &mut index);
    write_f32(dir, "element", w, h, 1, &region(&|t| elements.get(t).copied()), &mut index);
    std::fs::write(dir.join("objects.txt"), mesh.objects.iter().enumerate().map(|(i, o)| format!("{i} {}
", o.name)).collect::<String>()).unwrap();
    let mut alone = ts.clone();
    let mut f = (*folder).clone();
    f.visible = true;
    alone.layers = vec![Arc::new(f.clone())];
    let _ = subcraft_compositor::profile::take();
    let t_c = std::time::Instant::now();
    let rebuilt = composite(&session, &alone);
    println!("composite of the rebuilt stack: {:?}", t_c.elapsed());
    for (label, n, ms) in subcraft_compositor::profile::take().iter().take(12) {
        println!("  profile {label}: {n} evaluations, {ms:.0} ms");
    }
    for (kind, img) in &flat {
        let comps = kind.components();
        let a = dense(img, comps);
        write_f32(dir, &format!("flat_{kind:?}"), w, h, comps, &a, &mut index);
        if let Some(r) = rebuilt.channel(*kind) {
            let b = dense(r, comps);
            write_f32(dir, &format!("rebuilt_{kind:?}"), w, h, comps, &b, &mut index);
            println!("{kind:?}: mean abs difference {:.4}", error(&a, &b, comps, &cov));
            // The padding: texels 1..=4 outside the islands (Chebyshev), where Painter's textures are dilated.
            let band = outside_band(&cov, w, h, 4);
            println!("{kind:?}: outside band (4 texels) {:.4} over {} texels", error(&a, &b, comps, &band), band.iter().filter(|v| **v).count());
            // Per distance ring (1, 2..4 texels) and the means on both sides, to see what kind of gap it is.
            let mean = |v: &[f32]| {
                let (mut s, mut n) = (0.0f64, 0usize);
                for (i, c) in band.iter().enumerate() {
                    if *c {
                        s += v[i * comps] as f64;
                        n += 1;
                    }
                }
                s / n.max(1) as f64
            };
            {
                // The band split by distance to the islands: the rim the geometry keeps evaluated around
                // islands is a few texels wide, and beyond it procedural layers have no surface position.
                let mut inner = cov.clone();
                let mut rings = Vec::new();
                for d in 1..=4 {
                    let grown = outside_band(&cov, w, h, d);
                    let ring: Vec<bool> = grown.iter().zip(&inner).map(|(g, i)| *g && !*i).collect();
                    for (i, g) in grown.iter().enumerate() {
                        inner[i] = inner[i] || *g;
                    }
                    rings.push(format!("{d}: {:.4}", error(&a, &b, comps, &ring)));
                }
                println!("{kind:?}: band error by distance (texels) {}", rings.join(", "));
            }
            {
                // References: how far Painter's own band is from a nearest-texel padding of its inside, and
                // how far the rebuilt band is from a nearest-texel padding of the rebuilt inside.
                let nearest = subcraft_image::dilate::nearest_covered(w as usize, h as usize, &cov);
                let (mut pa, mut pb) = (a.clone(), b.clone());
                subcraft_image::dilate::apply_nearest(comps, &mut pa, &cov, &nearest);
                subcraft_image::dilate::apply_nearest(comps, &mut pb, &cov, &nearest);
                println!("{kind:?}: band, Painter vs its inside padded {:.4}; rebuilt vs rebuilt inside padded {:.4}; rebuilt padded vs Painter {:.4}", error(&a, &pa, comps, &band), error(&b, &pb, comps, &band), error(&a, &pb, comps, &band));
            }
            if std::env::var_os("SUBCRAFT_LAB_WORST").is_some() {
                // Where the band error sits: share of the band's error per 128x128 block, largest first.
                let bs = 128usize;
                let nb = (w as usize).div_ceil(bs);
                let mut blocks = vec![(0.0f64, 0usize); nb * nb];
                for (i, c) in band.iter().enumerate() {
                    if *c {
                        let (x, y) = (i % w as usize, i / w as usize);
                        let e = (0..comps).map(|k| (a[i * comps + k] - b[i * comps + k]).abs() as f64).sum::<f64>() / comps as f64;
                        let blk = &mut blocks[y / bs * nb + x / bs];
                        blk.0 += e;
                        blk.1 += 1;
                    }
                }
                let mut order: Vec<usize> = (0..blocks.len()).filter(|&k| blocks[k].1 > 0).collect();
                order.sort_by(|&p, &q| blocks[q].0.total_cmp(&blocks[p].0));
                for &k in order.iter().take(4) {
                    println!("  {kind:?} block x {} y {}: mean {:.3} over {} texels", k % nb * bs, k / nb * bs, blocks[k].0 / blocks[k].1 as f64, blocks[k].1);
                }
            }
            println!("{kind:?}: band mean flattened {:.4} rebuilt {:.4}", mean(&a), mean(&b));
        }
    }
    if args.get(3).is_some_and(|a| a == "probe") {
        let x: u32 = args[4].parse().unwrap();
        let y: u32 = args[5].parse().unwrap();
        let kind = match args.get(6).map(|s| s.as_str()) { Some("r") => ChannelKind::Roughness, Some("m") => ChannelKind::Metallic, _ => ChannelKind::BaseColor };
        probe(&session, &f, x, y, kind);
        return;
    }
    if args.get(3).is_some_and(|a| a == "ablate") {
        let kind = match args.get(4).map(|s| s.as_str()) { Some("r") => ChannelKind::Roughness, Some("m") => ChannelKind::Metallic, _ => ChannelKind::BaseColor };
        let flat_dense: Vec<(ChannelKind, Vec<f32>)> = flat.iter().map(|(k, i)| (*k, dense(i, k.components()))).collect();
        ablate(&session, &f, &flat_dense, &cov, kind);
        return;
    }
    if args.get(3).is_some_and(|a| a == "ablate_region") {
        // `ablate_region object|element N [b|r|m]`: ablation inside one region only.
        let by_object = args[4] == "object";
        let n: u32 = args[5].parse().unwrap();
        let kind = match args.get(6).map(|s| s.as_str()) { Some("r") => ChannelKind::Roughness, Some("m") => ChannelKind::Metallic, _ => ChannelKind::BaseColor };
        let elements = geometry.triangle_parts();
        let inside: Vec<bool> = tri.iter().zip(&cov).map(|(&t, &c)| c && t >= 0.0 && if by_object { mesh.tri_object.get(t as usize).copied().flatten() == Some(n) } else { elements.get(t as usize) == Some(&n) }).collect();
        println!("region: {} texels", inside.iter().filter(|b| **b).count());
        let flat_dense: Vec<(ChannelKind, Vec<f32>)> = flat.iter().map(|(k, i)| (*k, dense(i, k.components()))).collect();
        ablate(&session, &f, &flat_dense, &inside, kind);
        return;
    }
    if args.get(3).is_some_and(|a| a == "sources") {
        // `sources [r|m|b]`: what every leaf layer puts into one channel (kind of fill source).
        let kind = match args.get(4).map(|s| s.as_str()) { Some("r") => ChannelKind::Roughness, Some("m") => ChannelKind::Metallic, _ => ChannelKind::BaseColor };
        let mut list = Vec::new();
        leaves(&f, Vec::new(), &mut list);
        let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
        for (p, name) in &list {
            let mut l = &f;
            for &i in p {
                if let LayerContent::Group(g) = &l.content {
                    l = &g.children[i];
                }
            }
            if let LayerContent::Fill(fc) = &l.content {
                if let Some(c) = fc.channels.get(&kind) {
                    let d = format!("{:?}", c.source);
                    let d: String = d.chars().take(160).collect();
                    println!("{} | {name} | {d}", path_name(&f, p));
                    *counts.entry(d.split(|c: char| c == '{' || c == '(').next().unwrap_or("").to_string()).or_default() += 1;
                }
            }
        }
        println!("{counts:?}");
        return;
    }
    if args.get(3).is_some_and(|a| a == "edges") {
        // Layers whose mask holds an Edges generator (the rebuilt edge-wear / curvature masks).
        fn walk(l: &subcraft_doc::Layer, path: &mut Vec<String>, n: &mut usize) {
            path.push(l.name.clone());
            if let Some(m) = &l.mask {
                for e in &m.effects {
                    if let subcraft_doc::MaskEffectKind::Generator(g) = &e.kind {
                        if g.kind == subcraft_doc::GeneratorKind::Edges {
                            println!("{} | level {:.3} soft {:.3} invert {} noise {:.2} spread {:?}", path.join(" / "), g.level, g.softness, g.invert, g.noise_amount, g.edge_spread);
                            *n += 1;
                        }
                    }
                }
            }
            if let LayerContent::Group(g) = &l.content {
                for c in &g.children {
                    walk(c, path, n);
                }
            }
            path.pop();
        }
        let mut n = 0;
        walk(&f, &mut Vec::new(), &mut n);
        println!("{n} layers with Edges generators");
        return;
    }
    if args.get(3).is_some_and(|a| a == "trace") {
        // `trace x y [r|m]` or `trace x,y x,y ... [r|m]`.
        let kind = match args.last().map(|s| s.as_str()) { Some("r") => ChannelKind::Roughness, Some("m") => ChannelKind::Metallic, _ => ChannelKind::BaseColor };
        let points: Vec<(u32, u32)> = if args[4].contains(',') {
            args[4..].iter().filter_map(|a| a.split_once(',').map(|(x, y)| (x.parse().unwrap(), y.parse().unwrap()))).collect()
        } else {
            vec![(args[4].parse().unwrap(), args[5].parse().unwrap())]
        };
        for (x, y) in points {
            println!("== trace {x},{y} {kind:?}");
            trace(&session, &f, x, y, kind);
        }
        return;
    }
    if args.get(3).is_some_and(|a| a == "genfit") {
        // `genfit`: for every layer whose whole mask is one mesh-map generator, Painter's effective mask
        // (as `layer_alpha`) written as `pm_<k>.f32`, with `gens.txt` listing graph, tweaks and our mapped
        // generator, in the same order. Fit material for the generator models.
        let mut spp = subcraft_mesh::spp::SppFile::open(Path::new(path)).unwrap();
        let project = spp.project().unwrap();
        let mut theirs: Vec<(String, subcraft_mesh::spp::document::GraphRef)> = Vec::new();
        for pts in &project.texture_sets {
            subcraft_mesh::spp::document::walk_layers(&pts.layers, &mut |l, _| {
                if l.mask.len() == 1 {
                    if let subcraft_mesh::spp::document::ActionKind::Fill { sources, .. } = &l.mask[0].kind {
                        if let Some(subcraft_mesh::spp::document::SourceInfo::Procedural { graph, .. }) = sources.first() {
                            if matches!(graph.name().to_ascii_lowercase().as_str(), "mg_dirt" | "mg_curvature" | "mg_ambient_occlusion" | "mg_position") || graph.name().to_ascii_lowercase().contains("edge_wear") {
                                theirs.push((l.name.clone(), graph.clone()));
                            }
                        }
                    }
                }
            }, 0);
            break;
        }
        fn collect(l: &subcraft_doc::Layer, path: &mut Vec<usize>, out: &mut Vec<(Vec<usize>, String, subcraft_doc::Generator)>) {
            if let Some(m) = &l.mask {
                if m.effects.len() == 1 {
                    if let subcraft_doc::MaskEffectKind::Generator(g) = &m.effects[0].kind {
                        out.push((path.clone(), l.name.clone(), *g));
                    }
                }
            }
            if let LayerContent::Group(g) = &l.content {
                for (i, c) in g.children.iter().enumerate() {
                    path.push(i);
                    collect(c, path, out);
                    path.pop();
                }
            }
        }
        let mut ours = Vec::new();
        collect(&folder, &mut Vec::new(), &mut ours);
        println!("project {} clean generator layers, rebuilt {}", theirs.len(), ours.len());
        let flat_bc = dense(&flat[&ChannelKind::BaseColor], 3);
        let n = (w * h) as usize;
        let mut meta = std::fs::File::create(dir.join("gens.txt")).unwrap();
        for (k, ((tname, graph), (path, oname, g))) in theirs.iter().zip(ours.iter()).enumerate() {
            if tname.trim() != oname.trim() {
                println!("misaligned at {k}: {tname} vs {oname}");
                break;
            }
            fn mutate(l: &mut subcraft_doc::Layer, path: &[usize], how: &dyn Fn(&mut subcraft_doc::Layer)) {
                if path.is_empty() {
                    how(l);
                    return;
                }
                if let LayerContent::Group(g) = &mut l.content {
                    mutate(Arc::make_mut(&mut g.children[path[0]]), &path[1..], how);
                }
            }
            let mut without = (*folder).clone();
            mutate(&mut without, path, &|l| l.visible = false);
            let mut target = (*folder).clone();
            mutate(&mut target, path, &|l| l.mask = None);
            let below = stack_composite(&session, without);
            let full = stack_composite(&session, target);
            let (b, u) = (dense(below.channel(ChannelKind::BaseColor).unwrap(), 3), dense(full.channel(ChannelKind::BaseColor).unwrap(), 3));
            let mut pm = vec![-1.0f32; n];
            for i in 0..n {
                let (mut num, mut den) = (0.0f32, 0.0f32);
                for c in 0..3 {
                    let d = u[i * 3 + c] - b[i * 3 + c];
                    num += (flat_bc[i * 3 + c] - b[i * 3 + c]) * d;
                    den += d * d;
                }
                if den > 0.02 * 0.02 * 3.0 {
                    pm[i] = (num / den).clamp(-0.5, 1.5);
                }
            }
            {
                fn at(l: &subcraft_doc::Layer, path: &[usize]) -> subcraft_doc::Layer {
                    match (path.first(), &l.content) {
                        (Some(i), LayerContent::Group(g)) => at(&g.children[*i], &path[1..]),
                        _ => l.clone(),
                    }
                }
                let layer = at(&folder, path);
                if let Some(mask) = &layer.mask {
                    let empty = subcraft_materials::MeshMaps::default();
                    let img = subcraft_app::spp_stack::eval_mask_image(mask, ts, &geometry, session.resources(), session.mesh_maps(ts.id).unwrap_or(&empty));
                    std::fs::write(dir.join(format!("our_{k}.f32")), dense(&img, 1).iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>()).unwrap();
                }
            }
            let valid = pm.iter().filter(|v| **v >= 0.0).count();
            std::fs::write(dir.join(format!("pm_{k}.f32")), pm.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>()).unwrap();
            writeln!(meta, "{k}	{}	{}	{valid}	{:?}	{:?}", tname.trim(), graph.name(), graph.tweaks, g).unwrap();
            println!("{k} {tname} {} valid texels {valid}", graph.name());
        }
        let covf: Vec<f32> = cov.iter().map(|c| if *c { 1.0 } else { 0.0 }).collect();
        write_f32(dir, "coverage", w, h, 1, &covf, &mut index);
        return;
    }
    if args.get(3).is_some_and(|a| a == "layer_alpha") {
        // `layer_alpha folder ... layer`: Painter's effective mask of one layer, recovered from its
        // flattened Base Color between the stack without the layer and with its mask fully white
        // (`painter_mask`, -1 where the two do not differ), next to the rebuilt mask (`our_mask`).
        let names: Vec<&str> = args[4..].iter().map(|s| s.as_str()).collect();
        let edit = |f: &mut subcraft_doc::Layer, how: &dyn Fn(&mut subcraft_doc::Layer)| {
            fn walk(l: &mut subcraft_doc::Layer, names: &[&str], how: &dyn Fn(&mut subcraft_doc::Layer)) {
                if names.is_empty() {
                    how(l);
                    return;
                }
                if let LayerContent::Group(g) = &mut l.content {
                    if let Some(c) = g.children.iter_mut().find(|c| c.name.trim() == names[0].trim()) {
                        walk(Arc::make_mut(c), &names[1..], how);
                    }
                }
            }
            walk(f, &names, how);
        };
        let mut target = f.clone();
        let mut found: Option<subcraft_doc::Layer> = None;
        {
            fn get(l: &subcraft_doc::Layer, names: &[&str]) -> Option<subcraft_doc::Layer> {
                if names.is_empty() {
                    return Some(l.clone());
                }
                match &l.content {
                    LayerContent::Group(g) => g.children.iter().find(|c| c.name.trim() == names[0].trim()).and_then(|c| get(c, &names[1..])),
                    _ => None,
                }
            }
            found = found.or(get(&target, &names));
        }
        let layer = found.expect("layer path");
        let mut without = f.clone();
        edit(&mut without, &|l| l.visible = false);
        edit(&mut target, &|l| l.mask = None);
        let below = stack_composite(&session, without);
        let full = stack_composite(&session, target);
        let flat_bc = dense(&flat[&ChannelKind::BaseColor], 3);
        let (b, u) = (dense(below.channel(ChannelKind::BaseColor).unwrap(), 3), dense(full.channel(ChannelKind::BaseColor).unwrap(), 3));
        let n = (w * h) as usize;
        let mut pm = vec![-1.0f32; n];
        for i in 0..n {
            let (mut num, mut den) = (0.0f32, 0.0f32);
            for c in 0..3 {
                let d = u[i * 3 + c] - b[i * 3 + c];
                num += (flat_bc[i * 3 + c] - b[i * 3 + c]) * d;
                den += d * d;
            }
            if den > 0.02 * 0.02 * 3.0 {
                pm[i] = (num / den).clamp(-0.5, 1.5);
            }
        }
        write_f32(dir, "painter_mask", w, h, 1, &pm, &mut index);
        if let Some(mask) = &layer.mask {
            let empty = subcraft_materials::MeshMaps::default();
            let img = subcraft_app::spp_stack::eval_mask_image(mask, ts, &geometry, session.resources(), session.mesh_maps(ts.id).unwrap_or(&empty));
            write_f32(dir, "our_mask", w, h, 1, &dense(&img, 1), &mut index);
        }
        return;
    }
    if args.get(3).is_some_and(|a| a == "masks") {
        // `masks [i [j ...]]`: the masks of the children of the folder at that path (indices or names).
        let mut target = f.clone();
        for a in &args[4..] {
            let LayerContent::Group(g) = &target.content else { break };
            // An index, or a child's name.
            target = match a.parse::<usize>() {
                Ok(i) => (*g.children[i]).clone(),
                Err(_) => (**g.children.iter().find(|c| c.name.trim() == a.trim()).expect("no child of that name")).clone(),
            };
        }
        dump_masks(&session, &target, dir, &mut index);
    }
    if args.get(3).is_some_and(|a| a == "tune") {
        let flat_dense: Vec<(ChannelKind, Vec<f32>)> = flat.iter().map(|(k, i)| (*k, dense(i, k.components()))).collect();
        tune(&session, path, &flat_dense, &cov);
        return;
    }
    if blends {
        let flat_dense: Vec<(ChannelKind, Vec<f32>)> = flat.iter().map(|(k, i)| (*k, dense(i, k.components()))).collect();
        blend_search(&session, path, &flat_dense, &cov, args.get(4).map(|s| s.as_str()));
        return;
    }
    if prefix {
        let LayerContent::Group(g) = &f.content else { return };
        let flat_dense: Vec<(ChannelKind, Vec<f32>)> = flat.iter().map(|(k, i)| (*k, dense(i, k.components()))).collect();
        for n in 1..=g.children.len() {
            let mut part = f.clone();
            if let LayerContent::Group(pg) = &mut part.content {
                pg.children.truncate(n);
            }
            let mut set = ts.clone();
            set.layers = vec![Arc::new(part)];
            let c = composite(&session, &set);
            let errs: Vec<String> = flat_dense
                .iter()
                .filter_map(|(k, a)| Some(format!("{k:?} {:.4}", error(a, &dense(c.channel(*k)?, k.components()), k.components(), &cov))))
                .collect();
            println!("after {n:3} '{}' ({}): {}", g.children[n - 1].name, if g.children[n - 1].visible { "visible" } else { "hidden" }, errs.join(", "));
        }
    }
}

/// Experiment: cast the polygon-fill hits of the top-level folders' masks under several conventions.
#[allow(dead_code)]
pub fn decal_conventions(session: &subcraft_app::Session, path: &str) {
    use glam::{Mat4, Vec4};
    use subcraft_mesh::spp::document::ActionKind;
    let mut spp = subcraft_mesh::spp::SppFile::open(Path::new(path)).unwrap();
    let project = spp.project().unwrap();
    let scene = session.scene();
    let parts = subcraft_mesh::topology::mesh_elements(&scene.mesh);
    for l in project.texture_sets[0].layers.iter().rev() {
        let hits: Vec<_> = l.mask.iter().filter_map(|a| if let ActionKind::Decal { hits } = &a.kind { Some(hits.clone()) } else { None }).flatten().collect();
        if hits.is_empty() {
            continue;
        }
        let mut line = format!("{:16}", l.name);
        for conv in 0..8 {
            let mut n = 0;
            let mut ps = Vec::new();
            for h in &hits {
                let m = Mat4::from_cols_array(&h.proj_view_inv);
                let m = if conv & 1 == 1 { m.transpose() } else { m };
                let p = h.points[0];
                let ndc_y = if conv & 2 == 2 { 2.0 * p[1] - 1.0 } else { 1.0 - 2.0 * p[1] };
                let ndc_x = if conv & 4 == 4 { 1.0 - 2.0 * p[0] } else { 2.0 * p[0] - 1.0 };
                let un = |z: f32| {
                    let v = m * Vec4::new(ndc_x, ndc_y, z, 1.0);
                    v.truncate() / v.w
                };
                let (a, b) = (un(-1.0), un(1.0));
                let d = (b - a).normalize_or_zero();
                if let Some(hit) = scene.bvh.intersect(a, d, 0.0, f32::INFINITY) {
                    n += 1;
                    ps.push(parts[hit.tri as usize]);
                }
            }
            ps.sort();
            ps.dedup();
            line += &format!(" c{conv}:{n}/{} parts{:?}", hits.len(), &ps[..ps.len().min(4)]);
        }
        println!("{line}");
    }
}

/// The rebuilt stack (alone, with `folder` as given) composited.
pub fn stack_composite(session: &subcraft_app::Session, folder: subcraft_doc::Layer) -> subcraft_compositor::Composite {
    let mut set = session.document().texture_sets[0].clone();
    let mut f = folder;
    f.visible = true;
    set.layers = vec![Arc::new(f)];
    composite(session, &set)
}

/// Error of the rebuilt stack (alone) against the flattened channels, per channel.
pub fn stack_error(session: &subcraft_app::Session, folder: subcraft_doc::Layer, flat: &[(ChannelKind, Vec<f32>)], cov: &[bool]) -> Vec<(ChannelKind, f64)> {
    let mut set = session.document().texture_sets[0].clone();
    let mut f = folder;
    f.visible = true;
    set.layers = vec![Arc::new(f)];
    let c = composite(session, &set);
    flat.iter().filter_map(|(k, a)| Some((*k, error(a, &dense(c.channel(*k)?, k.components()), k.components(), cov)))).collect()
}

/// Experiment: find the SubCraft blend mode for each Painter blending id by coordinate descent on
/// the total difference to Painter's result.
pub fn blend_search(session: &subcraft_app::Session, path: &str, flat: &[(ChannelKind, Vec<f32>)], cov: &[bool], ids_arg: Option<&str>) {
    use std::collections::BTreeMap;
    use subcraft_doc::BlendMode;
    let mut spp = subcraft_mesh::spp::SppFile::open(Path::new(path)).unwrap();
    let project = spp.project().unwrap();
    let info = &project.texture_sets[0];
    let view = session.document().texture_sets[0].clone();
    let picker = subcraft_app::spp_select::Picker::new(session.scene().mesh.clone());
    let mut ids: BTreeMap<i32, usize> = BTreeMap::new();
    subcraft_mesh::spp::document::walk_layers(&info.layers, &mut |l, _| {
        for (_, id, _) in &l.blends {
            *ids.entry(*id).or_default() += 1;
        }
        for a in l.mask.iter().chain(&l.content) {
            for (_, id, _) in a.blends.iter().chain(&a.own_blends) {
                *ids.entry(*id).or_default() += 1;
            }
        }
    }, 0);
    println!("ids: {ids:?}");
    let mut map: BTreeMap<i32, BlendMode> = subcraft_app::spp_stack::blend_table();
    let cache = Arc::new(std::sync::Mutex::new(Vec::new()));
    let score = |map: &BTreeMap<i32, BlendMode>, spp: &mut subcraft_mesh::spp::SppFile<std::io::BufReader<std::fs::File>>| {
        let mut res = session.resources().clone();
        let maps: std::collections::BTreeMap<_, _> = session.mesh_maps(view.id).map(|m| m.iter().map(|(k, v)| (k, v.clone())).collect()).unwrap_or_default();
        let (folder, _, fitted) = subcraft_app::spp_import::rebuild_folder_cached(spp, info, &view, &picker, &mut res, &maps, Some(map), Some(cache.clone()));
        let mut set = view.clone();
        set.layers = vec![Arc::new(folder)];
        let flat_images: std::collections::BTreeMap<ChannelKind, TiledImage> = match &view.layers[0].content {
            LayerContent::Paint(p) => p.channels.clone(),
            _ => Default::default(),
        };
        let (_, c) = subcraft_app::spp_import::fit_and_composite(&mut set, &fitted, &session.scene().mesh, &res, &maps, &flat_images, None);
        let e: Vec<(ChannelKind, f64)> = flat.iter().filter_map(|(k, a)| Some((*k, error(a, &dense(c.channel(*k)?, k.components()), k.components(), cov)))).collect();
        (e.iter().map(|(_, v)| *v).sum::<f64>(), e)
    };
    let (base, e) = score(&map, &mut spp);
    println!("start {base:.4} {e:?}");
    let todo: Vec<i32> = match ids_arg {
        Some(list) => list.split(',').filter_map(|v| v.parse().ok()).collect(),
        None => ids.keys().copied().filter(|i| *i != 25 && *i != 1).collect(),
    };
    for pass in 0..1 {
        for &id in &todo {
            let mut results = Vec::new();
            use BlendMode::*;
            for mode in [Normal, Replace, Multiply, Divide, Darken, Lighten, LinearDodge, Subtract, Difference, SignedAddition, Overlay, Screen, LinearBurn, SoftLight, HardLight, LinearLight, Color, NormalMapDetail] {
                let mut m = map.clone();
                m.insert(id, mode);
                let (s, _) = score(&m, &mut spp);
                results.push((s, mode));
            }
            results.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            map.insert(id, results[0].1);
            let top: Vec<String> = results.iter().take(5).map(|(s, m)| format!("{m:?} {s:.4}")).collect();
            println!("pass {pass} id {id:3}: {}", top.join(", "));
        }
    }
    let (s, e) = score(&map, &mut spp);
    println!("final {s:.4} {e:?}\nmap {map:?}");
}

/// Leaves of a layer tree in compositing order (bottom first), as paths of child indices.
fn leaves(l: &subcraft_doc::Layer, path: Vec<usize>, out: &mut Vec<(Vec<usize>, String)>) {
    match &l.content {
        LayerContent::Group(g) if !g.children.is_empty() => {
            for (i, c) in g.children.iter().enumerate() {
                let mut p = path.clone();
                p.push(i);
                leaves(c, p, out);
            }
        }
        _ => out.push((path, l.name.clone())),
    }
}

/// "folder / sub / leaf" for an index path from [`leaves`].
fn path_name(folder: &subcraft_doc::Layer, path: &[usize]) -> String {
    let mut names = Vec::new();
    let mut l = folder;
    for &i in path {
        let LayerContent::Group(g) = &l.content else { break };
        l = &g.children[i];
        names.push(l.name.clone());
    }
    names.join(" / ")
}

/// Hide every leaf after the first `k` (in compositing order).
fn keep_first(l: &mut subcraft_doc::Layer, k: usize, seen: &mut usize) {
    match &mut l.content {
        LayerContent::Group(g) if !g.children.is_empty() => {
            for c in g.children.iter_mut() {
                keep_first(Arc::make_mut(c), k, seen);
            }
        }
        _ => {
            if *seen >= k {
                l.visible = false;
            }
            *seen += 1;
        }
    }
}

/// Experiment: which leaf layer last changes `channel` at texel (x, y)? Binary search over prefixes.
pub fn probe(session: &subcraft_app::Session, folder: &subcraft_doc::Layer, x: u32, y: u32, kind: ChannelKind) {
    let mut list = Vec::new();
    leaves(folder, Vec::new(), &mut list);
    let value = |k: usize| {
        let mut f = folder.clone();
        f.visible = true;
        keep_first(&mut f, k, &mut 0);
        let mut set = session.document().texture_sets[0].clone();
        set.layers = vec![Arc::new(f)];
        composite(session, &set).channel(kind).unwrap().get(x, y)
    };
    let full = value(list.len());
    println!("full {:?}", &full[..4]);
    let close = |a: [f32; 4]| (0..3).all(|c| (a[c] - full[c]).abs() < 1e-3);
    let (mut lo, mut hi) = (0, list.len());
    while lo < hi {
        let mid = (lo + hi) / 2;
        if close(value(mid)) { hi = mid } else { lo = mid + 1 }
    }
    for k in lo.saturating_sub(3)..(lo + 1).min(list.len() + 1) {
        let v = value(k);
        println!("after {k} leaves (last '{}'): {:?}", if k > 0 { &list[k - 1].1 } else { "-" }, &v[..4]);
    }
}

/// Experiment: each top-level folder's mask as an image (`mask_<n>`), and their names in `masks.txt`.
pub fn dump_masks(session: &subcraft_app::Session, folder: &subcraft_doc::Layer, dir: &Path, index: &mut std::fs::File) {
    let ts = &session.document().texture_sets[0];
    let geometry = session.scene().geometry.get(ts.material_slot, ts.resolution);
    let empty = subcraft_materials::MeshMaps::default();
    let maps = session.mesh_maps(ts.id).unwrap_or(&empty);
    let LayerContent::Group(g) = &folder.content else { return };
    let mut names = String::new();
    for (i, c) in g.children.iter().enumerate() {
        let Some(mask) = &c.mask else { continue };
        let img = subcraft_app::spp_stack::eval_mask_image(mask, ts, &geometry, session.resources(), maps);
        write_f32(dir, &format!("mask_{i}"), ts.resolution, ts.resolution, 1, &dense(&img, 1), index);
        names += &format!("{i} {}\n", c.name);
    }
    std::fs::write(dir.join("masks.txt"), names).unwrap();
}

/// Experiment: every bitmap source and stencil of the stack decoded to PNG (`image_<n>.png`).
pub fn dump_images(path: &str, dir: &Path) {
    let mut spp = subcraft_mesh::spp::SppFile::open(Path::new(path)).unwrap();
    let toc = std::fs::read(path).ok();
    drop(toc);
    let project = spp.project().unwrap();
    let mut urls = std::collections::BTreeSet::new();
    subcraft_mesh::spp::document::walk_layers(&project.texture_sets[0].layers, &mut |l, _| {
        for a in l.content.iter().chain(&l.mask) {
            if let subcraft_mesh::spp::document::ActionKind::Paint { brush: Some(b), .. } = &a.kind {
                if let Some((url, _)) = &b.stencil {
                    urls.insert(url.clone());
                }
            }
            if let subcraft_mesh::spp::document::ActionKind::Fill { sources, .. } = &a.kind {
                for s in sources {
                    if let subcraft_mesh::spp::SourceInfo::Bitmap { url, .. } = s {
                        urls.insert(url.clone());
                    }
                }
            }
        }
    }, 0);
    for (n, url) in urls.iter().enumerate() {
        let Some(blob) = spp.image_resource_blob(url).unwrap() else { continue };
        match subcraft_mesh::spp::images::decode_image_resource(&blob, 512) {
            Ok(p) => {
                let rgba: Vec<f32> = (0..(p.width * p.height) as usize).flat_map(|i| (0..4).map(move |c| if c < p.components { c } else if c == 3 { usize::MAX } else { 0 }).map(|c| if c == usize::MAX { 1.0 } else { p.data[i * p.components + c] }).collect::<Vec<_>>()).collect();
                subcraft_image::io::save_image(&dir.join(format!("image_{n}.png")), p.width, p.height, 4, &rgba, subcraft_image::io::FileFormat::Png8).unwrap();
                println!("image_{n}: {url} {}x{}x{}", p.width, p.height, p.components);
            }
            Err(e) => println!("image_{n}: {url} error {e}"),
        }
    }
}

/// Experiment: calibrate the generator mappings (`Tuning`) by coordinate descent on the total
/// difference to Painter's result, with the fitted fills refitted for every trial.
pub fn tune(session: &subcraft_app::Session, path: &str, flat: &[(ChannelKind, Vec<f32>)], cov: &[bool]) {
    use subcraft_app::spp_stack::{GeneratorMap, Tuning};
    let mut spp = subcraft_mesh::spp::SppFile::open(Path::new(path)).unwrap();
    let project = spp.project().unwrap();
    let info = &project.texture_sets[0];
    let view = session.document().texture_sets[0].clone();
    let picker = subcraft_app::spp_select::Picker::new(session.scene().mesh.clone());
    let cache = Arc::new(std::sync::Mutex::new(Vec::new()));
    let maps: std::collections::BTreeMap<_, _> = session.mesh_maps(view.id).map(|m| m.iter().map(|(k, v)| (k, v.clone())).collect()).unwrap_or_default();
    let flat_images: std::collections::BTreeMap<ChannelKind, TiledImage> = match &view.layers[0].content {
        LayerContent::Paint(p) => p.channels.clone(),
        _ => Default::default(),
    };
    let mut score = |t: Tuning| {
        let mut res = session.resources().clone();
        let (folder, _, fitted) = subcraft_app::spp_import::rebuild_folder_tuned(&mut spp, info, &view, &picker, &mut res, &maps, None, Some(cache.clone()), Some(t));
        let mut set = view.clone();
        set.layers = vec![Arc::new(folder)];
        let (_, c) = subcraft_app::spp_import::fit_and_composite(&mut set, &fitted, &session.scene().mesh, &res, &maps, &flat_images, None);
        let e: Vec<(ChannelKind, f64)> = flat.iter().filter_map(|(k, a)| Some((*k, error(a, &dense(c.channel(*k)?, k.components()), k.components(), cov)))).collect();
        (e.iter().map(|(_, v)| *v).sum::<f64>(), e)
    };
    let mut best = Tuning::default();
    let (mut best_s, e) = score(best);
    println!("start {best_s:.4} {e:?}");
    fn field(t: &mut Tuning, f: usize) -> &mut GeneratorMap {
        match f {
            0 => &mut t.dirt,
            1 => &mut t.edge_wear,
            2 => &mut t.curvature,
            3 => &mut t.ambient_occlusion,
            4 => &mut t.position,
            _ => &mut t.noise,
        }
    }
    let candidates: [&[f32]; 4] = [&[-0.5, -0.25, 0.0, 0.25, 0.5, 0.75, 1.0, 1.25], &[-1.5, -1.0, -0.5, 0.0, 0.5, 1.0, 1.5], &[0.01, 0.05, 0.1, 0.2, 0.35, 0.5], &[0.0, 0.2, 0.4, 0.6, 0.8, 1.0]];
    for pass in 0..1 {
        for f in [5, 0, 1, 2, 3, 4] {
            for p in 0..4 {
                let mut results = Vec::new();
                for &v in candidates[p] {
                    let mut t = best;
                    let m = field(&mut t, f);
                    match p {
                        0 => m.level[0] = v,
                        1 => m.level[1] = v,
                        2 => m.soft[0] = v,
                        _ => m.soft[1] = v,
                    }
                    let (s, _) = score(t);
                    results.push((s, v, t));
                }
                results.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                if results[0].0 < best_s {
                    best_s = results[0].0;
                    best = results[0].2;
                }
                println!("pass {pass} family {f} param {p}: best {:.4} at {} ({:?})", results[0].0, results[0].1, results.iter().take(3).map(|r| (r.1, (r.0 * 1e4).round() / 1e4)).collect::<Vec<_>>());
            }
        }
    }
    let (s, e) = score(best);
    println!("final {s:.4} {e:?}\ntuning {best:?}");
}

/// UV units per object unit over the whole mesh (sqrt of UV area / surface area) times the bbox
/// diagonal: the UV length of a brush whose size is 1 (100%) in object space.
pub fn uv_per_diagonal(mesh: &subcraft_mesh::Mesh) -> f32 {
    let (mut uv, mut world) = (0.0f64, 0.0f64);
    for t in 0..mesh.triangle_count() {
        let p = mesh.tri_positions(t);
        let u = mesh.tri_uvs(t);
        world += ((p[1] - p[0]).cross(p[2] - p[0]).length() * 0.5) as f64;
        uv += ((u[1] - u[0]).perp_dot(u[2] - u[0]).abs() * 0.5) as f64;
    }
    ((uv / world.max(1e-12)).sqrt() as f32) * mesh.bounds.diagonal()
}

/// Experiment: print the XML interface of the first `.sbsar` whose graph name contains `name`.
pub fn sbsar_xml(path: &str, name: &str) {
    let mut spp = subcraft_mesh::spp::SppFile::open(Path::new(path)).unwrap();
    let project = spp.project().unwrap();
    let mut url = None;
    subcraft_mesh::spp::document::walk_layers(&project.texture_sets[0].layers, &mut |l, _| {
        for a in l.content.iter().chain(&l.mask) {
            let graphs: Vec<&subcraft_mesh::spp::document::GraphRef> = match &a.kind {
                subcraft_mesh::spp::document::ActionKind::Fill { sources, .. } => sources.iter().filter_map(|s| if let subcraft_mesh::spp::SourceInfo::Procedural { graph, .. } = s { Some(graph) } else { None }).collect(),
                subcraft_mesh::spp::document::ActionKind::Filter { graph } => vec![graph],
                _ => Vec::new(),
            };
            for g in graphs {
                if url.is_none() && g.name().to_ascii_lowercase().contains(name) {
                    url = Some(g.url.clone());
                }
            }
        }
    }, 0);
    let Some(url) = url else { println!("no graph {name}"); return };
    println!("{url}");
    for f in spp.sbsar_files(&url).unwrap().unwrap_or_default() {
        if f.name.ends_with(".xml") {
            println!("{}", String::from_utf8_lossy(&f.data));
        } else {
            println!("{} ({} bytes)", f.name, f.data.len());
        }
    }
}

/// Experiment: the value of `kind` at texel (x, y) after each leaf layer (only that texel's tile is
/// composited), printing the leaves that change it.
pub fn trace(session: &subcraft_app::Session, folder: &subcraft_doc::Layer, x: u32, y: u32, kind: ChannelKind) {
    let mut list = Vec::new();
    leaves(folder, Vec::new(), &mut list);
    let ts = &session.document().texture_sets[0];
    let geometry = session.scene().geometry.get(ts.material_slot, ts.resolution);
    let empty = subcraft_materials::MeshMaps::default();
    let maps = session.mesh_maps(ts.id).unwrap_or(&empty);
    let ctx = subcraft_compositor::EvalContext { geometry: &geometry, resources: session.resources(), mesh_maps: maps };
    let tile = (y / subcraft_image::TILE_SIZE * ts.resolution.div_ceil(subcraft_image::TILE_SIZE) + x / subcraft_image::TILE_SIZE) as usize;
    let mut prev = [f32::NAN; 4];
    for k in 0..=list.len() {
        let mut f = folder.clone();
        f.visible = true;
        keep_first(&mut f, k, &mut 0);
        let mut set = ts.clone();
        set.layers = vec![Arc::new(f)];
        let mut c = subcraft_compositor::Composite::new(&set);
        c.evaluate_tiles(&set, &ctx, &[tile]);
        let v = c.channel(kind).unwrap().get(x, y);
        if (0..4).any(|i| (v[i] - prev[i]).abs() > 1e-3) {
            println!("after {k:3} '{}': {:?}", if k > 0 { path_name(folder, &list[k - 1].0) } else { "-".into() }, &v[..kind.components() + 1]);
        }
        prev = v;
    }
}

/// Hide the leaf at position `k` (compositing order).
fn hide_leaf(l: &mut subcraft_doc::Layer, k: usize, seen: &mut usize) {
    match &mut l.content {
        LayerContent::Group(g) if !g.children.is_empty() => {
            for c in g.children.iter_mut() {
                hide_leaf(Arc::make_mut(c), k, seen);
            }
        }
        _ => {
            if *seen == k {
                l.visible = false;
            }
            *seen += 1;
        }
    }
}

/// Experiment: the change of each channel's difference when one visible leaf layer is hidden
/// (negative = the stack is closer to Painter without it), the 25 largest gains by `kind`.
pub fn ablate(session: &subcraft_app::Session, folder: &subcraft_doc::Layer, flat: &[(ChannelKind, Vec<f32>)], cov: &[bool], kind: ChannelKind) {
    let mut list = Vec::new();
    leaves(folder, Vec::new(), &mut list);
    let visible: Vec<bool> = {
        let mut v = Vec::new();
        fn walk(l: &subcraft_doc::Layer, parent: bool, out: &mut Vec<bool>) {
            match &l.content {
                LayerContent::Group(g) if !g.children.is_empty() => g.children.iter().for_each(|c| walk(c, parent && l.visible, out)),
                _ => out.push(parent && l.visible),
            }
        }
        walk(folder, true, &mut v);
        v
    };
    let base = stack_error(session, folder.clone(), flat, cov);
    let of = |e: &[(ChannelKind, f64)], k: ChannelKind| e.iter().find(|(c, _)| *c == k).map_or(0.0, |(_, v)| *v);
    println!("base {base:?}");
    let mut rows = Vec::new();
    for k in 0..list.len() {
        if !visible[k] {
            continue;
        }
        let mut f = folder.clone();
        hide_leaf(&mut f, k, &mut 0);
        let e = stack_error(session, f, flat, cov);
        rows.push((of(&e, kind) - of(&base, kind), k, list[k].1.clone(), e));
    }
    rows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    for (d, k, name, e) in rows.iter().take(25) {
        let others: Vec<String> = e.iter().map(|(c, v)| format!("{c:?} {:+.4}", v - of(&base, *c))).collect();
        println!("{d:+.4} leaf {k:3} '{name}': {}", others.join(", "));
    }
}
