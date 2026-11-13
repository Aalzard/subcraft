//! Cost of one texel of every procedural pattern and of the generator noises (single thread):
//! `cargo run --release -p subcraft-materials --example noise_cost`
use std::time::Instant;

use glam::Vec2;
use subcraft_doc::{ProceduralKind, ProceduralParams};
use subcraft_materials::pattern;

fn main() {
    let n = 1024usize; // a 1024² patch of a 4096² set
    let kinds = [
        ProceduralKind::Clouds, ProceduralKind::Cells, ProceduralKind::Grunge, ProceduralKind::Scratches, ProceduralKind::Scratch,
        ProceduralKind::WoodRings, ProceduralKind::Brushed, ProceduralKind::Checker, ProceduralKind::Dirt, ProceduralKind::Cracks,
        ProceduralKind::Streaks, ProceduralKind::Speckles, ProceduralKind::Pores,
    ];
    for scale in [2.0f32, 6.0, 14.0] {
        println!("scale {scale}:");
        for kind in kinds {
            let params = ProceduralParams { scale, ..ProceduralParams::new(kind) };
            let t = Instant::now();
            let mut sum = 0.0f64;
            for y in 0..n {
                for x in 0..n {
                    let p = Vec2::new((x as f32 + 0.5) / 4096.0, (y as f32 + 0.5) / 4096.0);
                    sum += pattern::eval(&params, p) as f64;
                }
            }
            let ns = t.elapsed().as_secs_f64() * 1e9 / (n * n) as f64;
            println!("  {kind:?}: {ns:.0} ns/texel (mean {:.3})", sum / (n * n) as f64);
        }
    }
}
