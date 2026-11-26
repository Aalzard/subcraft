//! Cost of one layer blend on a 64² tile (`composite_run`), Normal mode with a mask.
//!
//! `cargo run --release -p subcraft-compositor --example blend_bench`
use std::time::Instant;

use subcraft_compositor::blend::composite_run;
use subcraft_doc::BlendMode;
use subcraft_doc::blend_math::BlendTarget;

fn main() {
    let n = 4096;
    let src: Vec<[f32; 4]> = (0..n).map(|i| { let v = (i % 97) as f32 / 97.0; [v, 1.0 - v, 0.5 * v, if i % 5 == 0 { 0.0 } else { 0.8 }] }).collect();
    let cov: Vec<f32> = (0..n).map(|i| (i % 13) as f32 / 12.0).collect();
    let base: Vec<[f32; 4]> = (0..n).map(|i| { let v = (i % 31) as f32 / 31.0; [v, v, v, 1.0] }).collect();
    for mode in [BlendMode::Normal, BlendMode::Multiply, BlendMode::Overlay] {
        let reps = 3000;
        let mut dst = base.clone();
        let t = Instant::now();
        let mut acc = 0.0;
        for _ in 0..reps {
            dst.copy_from_slice(&base);
            composite_run(&mut dst, &src, mode, BlendTarget::for_channel(subcraft_core::ChannelKind::BaseColor), |i, s| s[3] * 0.9 * cov[i]);
            acc += dst[100][0];
        }
        let us = t.elapsed().as_secs_f64() * 1e6 / reps as f64;
        println!("{mode:?}: {us:.1} us per tile ({:.2} ns/texel) [{acc:.1}]", us * 1e3 / n as f64);
    }
}
