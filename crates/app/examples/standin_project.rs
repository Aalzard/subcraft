//! Contributed by MrHallM in issue #628 (memory profiling of large multi-set projects).
//! A synthetic stand-in for a large imported avatar project: 11 texture sets at 4096², each with
//! a dense "flattened import" paint layer (most tiles one flat value, as Painter imports are),
//! a uniform fill layer and a partly painted layer. Geometry and textures are generated, so it
//! can be shared freely.
//! `cargo run --release -p subcraft-app --example standin_project -- out.subcraft [resolution]`
//! Then: `cargo run --release -p subcraft-app --example mem_profile -- out.subcraft`

use rayon::prelude::*;
use subcraft_core::ChannelKind;
use subcraft_doc::{Document, Layer, LayerContent, TextureSet};
use subcraft_image::{TILE_SIZE, TILE_TEXELS, TiledImage};
use subcraft_mesh::{Mesh, primitives};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Share of tiles of the flattened layer that hold one flat value (measured on a real import: 65%).
const FLAT_SHARE: f32 = 0.65;

fn mesh() -> Mesh {
    let mut parts = Vec::new();
    for i in 0..11u32 {
        let base = match i % 3 {
            0 => primitives::uv_sphere(0.5, 96, 48),
            1 => primitives::torus(0.45, 0.15, 96, 32),
            _ => primitives::atlas_box(glam::Vec3::splat(0.8), 8, "box"),
        };
        let at = glam::Mat4::from_translation(glam::vec3((i % 4) as f32 * 1.4, (i / 4) as f32 * 1.4, 0.0));
        let mut part = base.transformed(&at);
        for slot in &mut part.materials {
            slot.name = format!("Set {:02}", i + 1);
        }
        parts.push(part);
    }
    let refs: Vec<&Mesh> = parts.iter().collect();
    Mesh::merged(&refs)
}

/// Deterministic hash in 0..1.
fn hash(a: u32, b: u32) -> f32 {
    let mut h = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h & 0xFFFF) as f32 / 65535.0
}

/// A dense image like an imported flattened layer: every tile allocated, most of them one flat
/// value that is NOT the image's fill value, the rest a smooth pattern.
fn flattened(kind: ChannelKind, res: u32, set: u32) -> TiledImage {
    let mut img = subcraft_doc::layer::empty_paint_image(kind, res);
    let comps = img.comps();
    let flat: Vec<f32> = (0..comps).map(|c| if c + 1 == comps { 1.0 } else { 0.2 + 0.6 * hash(set, c as u32 + kind as u32 * 7) }).collect();
    let mut buf = vec![0.0f32; TILE_TEXELS * comps];
    for t in 0..img.tile_count() {
        if hash(set * 1000 + kind as u32, t as u32) < FLAT_SHARE {
            for px in buf.chunks_exact_mut(comps) {
                px.copy_from_slice(&flat);
            }
        } else {
            let (tx, ty) = img.tile_coords(t);
            for (i, px) in buf.chunks_exact_mut(comps).enumerate() {
                let x = (tx * TILE_SIZE + i as u32 % TILE_SIZE) as f32 / res as f32;
                let y = (ty * TILE_SIZE + i as u32 / TILE_SIZE) as f32 / res as f32;
                for (c, v) in px.iter_mut().enumerate() {
                    *v = if c + 1 == comps { 1.0 } else { 0.5 + 0.4 * ((x * 37.0 + c as f32).sin() * (y * 23.0).cos()) };
                }
            }
        }
        img.write_tile(t, &buf);
    }
    img
}

/// A painted layer: a band of allocated tiles (strokes), the rest left sparse.
fn painted(kind: ChannelKind, res: u32, set: u32) -> TiledImage {
    let mut img = subcraft_doc::layer::empty_paint_image(kind, res);
    let comps = img.comps();
    let tiles = res / TILE_SIZE;
    let mut buf = vec![0.0f32; TILE_TEXELS * comps];
    for ty in tiles / 3..tiles / 2 {
        for tx in 0..tiles {
            let t = img.tile_index(tx, ty);
            for (i, px) in buf.chunks_exact_mut(comps).enumerate() {
                let a = hash(set + t as u32, i as u32 / 97);
                for (c, v) in px.iter_mut().enumerate() {
                    *v = if c + 1 == comps { a } else { 0.3 + 0.1 * c as f32 };
                }
            }
            img.write_tile(t, &buf);
        }
    }
    img
}

fn paint_layer(name: &str, ts: &TextureSet, image: impl Fn(ChannelKind) -> TiledImage) -> Layer {
    let mut layer = Layer::new_paint(name, ts);
    if let LayerContent::Paint(p) = &mut layer.content {
        for c in &ts.channels {
            p.channels.insert(c.kind, image(c.kind));
        }
    }
    layer
}

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).expect("output .subcraft path"));
    let res: u32 = std::env::args().nth(2).and_then(|r| r.parse().ok()).unwrap_or(4096);
    let mesh = mesh();
    let names: Vec<String> = mesh.materials.iter().map(|m| m.name.clone()).collect();
    let mut doc = Document::from_material_slots(&names, res);
    doc.texture_sets.par_iter_mut().enumerate().for_each(|(i, ts)| {
        let set = i as u32;
        let flat = paint_layer("Flattened import", ts, |k| flattened(k, res, set));
        let fill = Layer::new_fill("Base fill", ts);
        let paint = paint_layer("Painted", ts, |k| painted(k, res, set));
        ts.layers = vec![std::sync::Arc::new(flat), std::sync::Arc::new(fill), std::sync::Arc::new(paint)];
    });
    let mut session = subcraft_app::Session::from_parts(mesh, doc, Default::default(), "Stand-in project");
    let path = session.save(Some(&out)).expect("save");
    println!("wrote {} ({} texture sets at {res}², 3 layers each)", path.display(), names.len());
}
