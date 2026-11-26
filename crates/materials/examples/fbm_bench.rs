//! Fractal noise cost: `fbm` per point against `fbm_many` over the same points (a 64² tile's
//! worth of three triplanar planes).
//!
//! `cargo run --release -p subcraft-materials --example fbm_bench -- [reps]`
use std::time::Instant;

use glam::Vec2;
use subcraft_materials::noise::{fbm, fbm_many};

fn main() {
    let reps: usize = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(200);
    // A tile seen at scale 3: 64 texels span about 1/32 of the pattern.
    let points: Vec<Vec2> = (0..3 * 4096).map(|i| {
        let (x, y) = ((i % 64) as f32, ((i / 64) % 64) as f32);
        Vec2::new(0.37 + x / 2048.0, 0.71 + y / 2048.0) * 3.0
    }).collect();
    let mut out = vec![0.0f32; points.len()];
    let t = Instant::now();
    let mut acc = 0.0;
    for r in 0..reps {
        for (o, p) in out.iter_mut().zip(&points) {
            *o = fbm(*p, 3, 6, r as u32);
        }
        acc += out[17];
    }
    let scalar = t.elapsed().as_secs_f64() * 1e3 / reps as f64;
    let t = Instant::now();
    for r in 0..reps {
        fbm_many(&points, 3, 6, r as u32, &mut out);
        acc += out[17];
    }
    let many = t.elapsed().as_secs_f64() * 1e3 / reps as f64;
    println!("fbm per point {scalar:.3} ms, fbm_many {many:.3} ms per {} points, 6 octaves ({:.2}x) [{acc:.1}]", points.len(), scalar / many);
}
