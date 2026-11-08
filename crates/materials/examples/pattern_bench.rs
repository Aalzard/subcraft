//! Times procedural pattern evaluation over tile-shaped texel grids and prints a checksum of
//! every value bit, so a faster implementation can be checked for identical output.
//!
//! `cargo run --release -p subcraft-materials --example pattern_bench -- [tiles]`
use std::time::Instant;

use glam::Vec2;
use subcraft_doc::{ProceduralKind, ProceduralParams};
use subcraft_materials::pattern;

fn main() {
    let tiles: u32 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(64);
    let res = 2048.0f32;
    let mut total = 0.0;
    let mut all = 0x811c_9dc5u64;
    for kind in ProceduralKind::ALL {
        let params = ProceduralParams { scale: 4.0, ..ProceduralParams::new(kind) };
        let t = Instant::now();
        let mut h = 0x811c_9dc5u64;
        for tile in 0..tiles {
            let (tx, ty) = ((tile * 7) % 32, (tile * 13) % 32);
            for y in 0..64u32 {
                for x in 0..64u32 {
                    let p = Vec2::new(((tx * 64 + x) as f32 + 0.5) / res, ((ty * 64 + y) as f32 + 0.5) / res);
                    let v = pattern::eval(&params, p);
                    h = (h ^ v.to_bits() as u64).wrapping_mul(0x0100_0000_01b3);
                }
            }
        }
        let ms = t.elapsed().as_secs_f64() * 1e3;
        total += ms;
        all ^= h.rotate_left(kind as u32);
        println!("{kind:<12?} {:8.2} ms  ({:6.1} ns/texel)  hash {h:016x}", ms, ms * 1e6 / (tiles as f64 * 4096.0));
    }
    println!("total {total:.1} ms, combined hash {all:016x}");
}
