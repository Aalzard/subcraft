//! Cost of one procedural field per 64² tile, per pattern kind: the per-texel path
//! (`eval_procedural` for every texel) against `procedural_tile` (batched where the kind has a
//! batched path), triplanar on a sphere.
//!
//! `cargo run --release -p subcraft-materials --example procedural_tile_bench`
use std::sync::Arc;
use std::time::Instant;

use subcraft_doc::{ProceduralKind, ProceduralParams, Projection, ProjectionMode};
use subcraft_image::TILE_TEXELS;
use subcraft_materials::sample::{TextureSetGeometry, eval_procedural, procedural_tile};

fn main() {
    let geo = TextureSetGeometry::new(Arc::new(subcraft_mesh::primitives::uv_sphere(1.0, 64, 32)), 0, 1024);
    let tiles: Vec<_> = (0..geo.tile_count()).map(|t| geo.tile(t)).collect();
    let proj = Projection { mode: ProjectionMode::Triplanar, scale: [3.0, 3.0], ..Projection::default() };
    for kind in [ProceduralKind::Clouds, ProceduralKind::Cells, ProceduralKind::Pores, ProceduralKind::Brushed, ProceduralKind::Speckles, ProceduralKind::Streaks] {
        let params = ProceduralParams { scale: 4.0, ..ProceduralParams::new(kind) };
        let t = Instant::now();
        let mut acc = 0.0;
        for g in &tiles {
            for i in 0..TILE_TEXELS {
                acc += eval_procedural(&params, &proj, g, i);
            }
        }
        let per_texel = t.elapsed().as_secs_f64() * 1e3;
        let t = Instant::now();
        for (k, g) in tiles.iter().enumerate() {
            // A new seed per pass so the field cache never hits.
            let p = ProceduralParams { seed: 1000 + k as u32, ..params };
            acc += procedural_tile(&p, &proj, g)[17];
        }
        let batched = t.elapsed().as_secs_f64() * 1e3;
        println!("{kind:?}: per texel {per_texel:.0} ms, procedural_tile {batched:.0} ms for {} tiles ({:.2}x) [{acc:.0}]", tiles.len(), per_texel / batched);
    }
}
