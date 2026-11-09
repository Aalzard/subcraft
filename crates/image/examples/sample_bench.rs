//! Bilinear bitmap sampling cost over tile-shaped texel runs (ns per sample) plus a checksum.
//! `cargo run --release -p subcraft-image --example sample_bench`
use std::time::Instant;

use subcraft_image::{Bitmap, Wrap};

fn main() {
    let n = 2048u32;
    let data: Vec<u8> = (0..n * n * 4).map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8).collect();
    let b = Bitmap::new_u8(n, n, 4, data);
    let gray = Bitmap::new_u8(n, n, 1, (0..n * n).map(|i| (i.wrapping_mul(40_503) >> 8) as u8).collect());
    for (label, bmp, lin) in [("rgba8 srgb", &b, true), ("rgba8 linear", &b, false), ("gray8", &gray, false)] {
        let t = Instant::now();
        let mut h = 0u64;
        for tile in 0..256u32 {
            let (tx, ty) = (tile % 16, tile / 16);
            for i in 0..4096u32 {
                let u = ((tx * 64 + i % 64) as f32 + 0.5) / 1024.0;
                let v = ((ty * 64 + i / 64) as f32 + 0.5) / 1024.0;
                let s = bmp.sample(u * 2.0, v * 2.0, Wrap::Repeat, lin);
                h = h.wrapping_mul(31).wrapping_add(s[0].to_bits() as u64 ^ s[3].to_bits() as u64);
            }
        }
        let ns = t.elapsed().as_secs_f64() * 1e9 / (256.0 * 4096.0);
        println!("{label:<14} {ns:6.1} ns/sample  hash {h:016x}");
    }
}
