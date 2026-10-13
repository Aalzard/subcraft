//! Developer preview: renders engine-level results to PNG files so they can be inspected
//! before the application UI exists.
//!
//! Usage: `cargo run -p subcraft-preview --release -- [output_dir]` (default: `preview_out`).

use std::path::{Path, PathBuf};

use glam::Vec2;
use rayon::prelude::*;
use subcraft_core::color::{hsv_to_rgb, linear_to_srgb};
use subcraft_doc::{ProceduralKind, ProceduralParams};
use subcraft_image::{FileFormat, save_image};
use subcraft_materials::pattern;
use subcraft_mesh::{Mesh, primitives, topology, uv_raster};

fn main() {
    let out: PathBuf = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("preview_out"));
    std::fs::create_dir_all(&out).expect("create output directory");

    patterns(&out);
    let mesh = primitives::sample_crate();
    for slot in 0..mesh.materials.len() as u32 {
        uv_layout(&mesh, slot, &out);
        world_normals(&mesh, slot, &out);
        baked_maps(&mesh, slot, &out);
    }
    println!("wrote previews to {}", out.canonicalize().unwrap_or(out).display());
}

/// One image per procedural pattern, a 3x3 contact sheet, and a 2x2 tiling check.
fn patterns(out: &Path) {
    const S: usize = 256;
    let mut sheet = vec![0.0f32; (S * 3) * (S * 3) * 3];
    for (i, kind) in ProceduralKind::ALL.into_iter().enumerate() {
        let params = ProceduralParams::new(kind);
        let img = render_gray(S, S, |u, v| pattern::eval(&params, Vec2::new(u, v)));
        let name = format!("pattern_{}.png", kind.label().to_lowercase().replace(' ', "_"));
        save_image(&out.join(name), S as u32, S as u32, 1, &img, FileFormat::Png8).expect("write pattern");
        let (cx, cy) = (i % 3, i / 3);
        for y in 0..S {
            for x in 0..S {
                let v = img[y * S + x];
                let o = ((cy * S + y) * S * 3 + cx * S + x) * 3;
                sheet[o..o + 3].copy_from_slice(&[v, v, v]);
            }
        }
    }
    draw_grid(&mut sheet, S * 3, S);
    save_image(&out.join("patterns_sheet.png"), (S * 3) as u32, (S * 3) as u32, 3, &sheet, FileFormat::Png8)
        .expect("write sheet");

    // Tiling check: the clouds pattern repeated 2x2 must show no seams.
    let params = ProceduralParams::new(ProceduralKind::Clouds);
    let tiled = render_gray(512, 512, |u, v| pattern::eval(&params, Vec2::new(u * 2.0, v * 2.0)));
    save_image(&out.join("tiling_check_clouds_2x2.png"), 512, 512, 1, &tiled, FileFormat::Png8).expect("write tiling");
}

fn draw_grid(rgb: &mut [f32], size: usize, cell: usize) {
    for k in 0..size {
        for line in (cell..size).step_by(cell) {
            for (x, y) in [(line, k), (k, line)] {
                let o = (y * size + x) * 3;
                rgb[o..o + 3].copy_from_slice(&[1.0, 0.55, 0.1]);
            }
        }
    }
}

fn render_gray(w: usize, h: usize, f: impl Fn(f32, f32) -> f32 + Sync) -> Vec<f32> {
    let mut img = vec![0.0f32; w * h];
    img.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, v) in row.iter_mut().enumerate() {
            *v = f((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
        }
    });
    img
}

/// UV islands of one texture set, each in its own color, with the UV wireframe on top.
fn uv_layout(mesh: &Mesh, slot: u32, out: &Path) {
    const S: u32 = 1024;
    let tris = mesh.material_triangles(slot);
    let surface = uv_raster::rasterize_surface(mesh, tris.clone(), S, S, 0.0);
    let islands = topology::uv_islands(mesh);
    let mut rgb = vec![0.08f32; (S * S * 3) as usize];
    for i in 0..(S * S) as usize {
        if surface.covered(i) {
            let island = islands[surface.triangle[i] as usize];
            let hue = (island as f32 * 0.618_034).fract();
            let c = hsv_to_rgb([hue, 0.45, 0.85]);
            rgb[i * 3..i * 3 + 3].copy_from_slice(&c);
        }
    }
    for [a, b] in topology::uv_edges(mesh, tris) {
        draw_line(&mut rgb, S as usize, a * S as f32, b * S as f32, [0.05, 0.05, 0.05]);
    }
    let name = format!("uv_layout_{}.png", mesh.materials[slot as usize].name.to_lowercase());
    save_image(&out.join(name), S, S, 3, &rgb, FileFormat::Png8).expect("write uv layout");
}

/// World-space normals rasterized in UV space (a low-poly "world normal" bake preview).
fn world_normals(mesh: &Mesh, slot: u32, out: &Path) {
    const S: u32 = 1024;
    let surface = uv_raster::rasterize_surface(mesh, mesh.material_triangles(slot), S, S, 1.0);
    let mut rgb = vec![0.0f32; (S * S * 3) as usize];
    for i in 0..(S * S) as usize {
        if surface.covered(i) {
            let n = surface.normal[i];
            for c in 0..3 {
                rgb[i * 3 + c] = n[c] * 0.5 + 0.5;
            }
        } else {
            rgb[i * 3..i * 3 + 3].copy_from_slice(&[linear_to_srgb(0.02); 3]);
        }
    }
    let name = format!("world_normal_{}.png", mesh.materials[slot as usize].name.to_lowercase());
    save_image(&out.join(name), S, S, 3, &rgb, FileFormat::Png8).expect("write normals");
}

fn draw_line(rgb: &mut [f32], size: usize, a: Vec2, b: Vec2, color: [f32; 3]) {
    let steps = (b - a).abs().max_element().ceil().max(1.0) as usize;
    for s in 0..=steps {
        let p = a + (b - a) * (s as f32 / steps as f32);
        let (x, y) = (p.x as isize, p.y as isize);
        if x >= 0 && y >= 0 && (x as usize) < size && (y as usize) < size {
            let o = (y as usize * size + x as usize) * 3;
            rgb[o..o + 3].copy_from_slice(&color);
        }
    }
}

/// Bake every mesh map of a texture set (low on low) and write them side by side.
fn baked_maps(mesh: &Mesh, slot: u32, out: &Path) {
    use subcraft_bake::{BakeInput, BakeSettings, Geometry, Progress, bake};
    use subcraft_core::MeshMapKind;
    const S: u32 = 512;
    let bvh = subcraft_mesh::Bvh::build(mesh);
    let input = BakeInput { low: Geometry { mesh, bvh: &bvh }, high: None, material_slot: slot };
    let settings = BakeSettings { size: S, ..BakeSettings::default() };
    let t = std::time::Instant::now();
    let maps = bake(&input, &settings, &Progress::new()).expect("bake");
    println!("baked {} maps for slot {slot} at {S}² in {:?}", maps.len(), t.elapsed());
    let kinds = [MeshMapKind::AmbientOcclusion, MeshMapKind::Curvature, MeshMapKind::Thickness, MeshMapKind::WorldSpaceNormal, MeshMapKind::Id, MeshMapKind::Position];
    let w = S as usize * kinds.len();
    let mut rgb = vec![0.0f32; w * S as usize * 3];
    for (k, kind) in kinds.iter().enumerate() {
        let img = &maps[kind];
        let dense = img.to_dense();
        let c = img.comps();
        for y in 0..S as usize {
            for x in 0..S as usize {
                let src = &dense[(y * S as usize + x) * c..][..c];
                let px = if c == 1 { [src[0]; 3] } else { [src[0], src[1], src[2]] };
                let o = (y * w + k * S as usize + x) * 3;
                rgb[o..o + 3].copy_from_slice(&px);
            }
        }
    }
    let name = format!("baked_{}.png", mesh.materials[slot as usize].name.to_lowercase());
    save_image(&out.join(name), w as u32, S, 3, &rgb, FileFormat::Png8).expect("write baked maps");
}
