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
    let t = std::time::Instant::now();
    let (session, notes) = subcraft_app::Session::open_mesh(Path::new(path), 1024).expect("open");
    for n in &notes {
        println!("{n}");
    }
    println!("opened in {:?}", t.elapsed());
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
            }
        }
    }
    write_f32(dir, "coverage", w, h, 1, &cov.iter().map(|c| *c as u8 as f32).collect::<Vec<_>>(), &mut index);
    write_f32(dir, "triangle", w, h, 1, &tri, &mut index);
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
        }
    }
    if args.get(3).is_some_and(|a| a == "probe") {
        let x: u32 = args[4].parse().unwrap();
        let y: u32 = args[5].parse().unwrap();
        let kind = match args.get(6).map(|s| s.as_str()) { Some("r") => ChannelKind::Roughness, Some("m") => ChannelKind::Metallic, _ => ChannelKind::BaseColor };
        probe(&session, &f, x, y, kind);
        return;
    }
    if args.get(3).is_some_and(|a| a == "masks") {
        // `masks [i [j ...]]`: the masks of the children of the folder at that path.
        let mut target = f.clone();
        for a in &args[4..] {
            let LayerContent::Group(g) = &target.content else { break };
            target = (*g.children[a.parse::<usize>().unwrap()]).clone();
        }
        dump_masks(&session, &target, dir, &mut index);
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
        for a in &l.mask {
            for (_, id, _) in &a.blends {
                *ids.entry(*id).or_default() += 1;
            }
        }
    }, 0);
    println!("ids: {ids:?}");
    let mut map: BTreeMap<i32, BlendMode> = BTreeMap::new();
    map.insert(25, BlendMode::Normal);
    let score = |map: &BTreeMap<i32, BlendMode>, spp: &mut subcraft_mesh::spp::SppFile<std::io::BufReader<std::fs::File>>| {
        let mut res = session.resources().clone();
        let maps: std::collections::BTreeMap<_, _> = session.mesh_maps(view.id).map(|m| m.iter().map(|(k, v)| (k, v.clone())).collect()).unwrap_or_default();
        let (folder, _, fitted) = subcraft_app::spp_import::rebuild_folder(spp, info, &view, &picker, &mut res, &maps, Some(map));
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
        None => ids.keys().copied().filter(|i| *i != 25).collect(),
    };
    for pass in 0..2 {
        for &id in &todo {
            let mut results = Vec::new();
            for mode in BlendMode::ALL {
                if matches!(mode, BlendMode::Passthrough | BlendMode::Disable) {
                    continue;
                }
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
