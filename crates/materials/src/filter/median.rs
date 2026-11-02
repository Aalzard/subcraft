//! Exact square-window median. Ranks are assigned once per rectangle, then a sliding histogram
//! over the ranks (coarse buckets of 256 ranks) finds the k-th element, so cost per pixel is
//! independent of the window area apart from the 2 * (2r + 1) histogram updates.

/// Whole-pixel radius used by the median for a (possibly resolution-scaled) radius setting.
pub fn window_radius(radius: f32) -> usize {
    if radius.is_finite() { radius.round().clamp(0.0, 64.0) as usize } else { 0 }
}

/// Median of `src` (a `width` x `height` rectangle, row-major) for the output region starting at
/// (`ox`, `oy`) of size `tw` x `th`. Window coordinates clamp to the rectangle, so a caller that
/// supplies `r` pixels of margin gets seamless tiles and a rectangle clipped at the image edge
/// gets edge replication. Each channel is independent.
#[allow(clippy::too_many_arguments)]
pub fn median_rect(src: &[[f32; 3]], width: usize, height: usize, ox: usize, oy: usize, tw: usize, th: usize, r: usize) -> Vec<[f32; 3]> {
    assert_eq!(src.len(), width * height);
    assert!(ox + tw <= width && oy + th <= height);
    let mut out = vec![[0.0; 3]; tw * th];
    if r == 0 {
        for ty in 0..th { for tx in 0..tw { out[ty * tw + tx] = src[(oy + ty) * width + ox + tx]; } }
        return out;
    }
    let gray = src.iter().all(|p| p[0] == p[1] && p[1] == p[2]);
    for c in 0..if gray { 1 } else { 3 } {
        let (ranks, values) = rank(src, c);
        let mut coarse = vec![0u32; (values.len() >> 8) + 1];
        let mut count = vec![0u16; values.len()];
        let k = ((2 * r + 1) * (2 * r + 1) / 2) as u32;
        let clamp = |v: isize, max: usize| v.clamp(0, max as isize - 1) as usize;
        for ty in 0..th {
            let y = (oy + ty) as isize;
            coarse.fill(0);
            count.fill(0);
            let column = |x: usize, add: bool, coarse: &mut [u32], count: &mut [u16]| {
                for yy in y - r as isize..=y + r as isize {
                    let rk = ranks[clamp(yy, height) * width + x] as usize;
                    if add { count[rk] += 1; coarse[rk >> 8] += 1; } else { count[rk] -= 1; coarse[rk >> 8] -= 1; }
                }
            };
            for xx in -(r as isize)..=r as isize { column(clamp(ox as isize + xx, width), true, &mut coarse, &mut count); }
            for tx in 0..tw {
                let (mut acc, mut b) = (0u32, 0usize);
                while acc + coarse[b] <= k { acc += coarse[b]; b += 1; }
                let mut rk = b << 8;
                while acc + count[rk] as u32 <= k { acc += count[rk] as u32; rk += 1; }
                let v = values[rk];
                if gray { out[ty * tw + tx] = [v; 3]; } else { out[ty * tw + tx][c] = v; }
                if tx + 1 < tw {
                    let x = (ox + tx) as isize;
                    column(clamp(x - r as isize, width), false, &mut coarse, &mut count);
                    column(clamp(x + r as isize + 1, width), true, &mut coarse, &mut count);
                }
            }
        }
    }
    out
}

/// Dense ranks (equal values share a rank) and the value of each rank.
fn rank(src: &[[f32; 3]], c: usize) -> (Vec<u32>, Vec<f32>) {
    let mut order: Vec<u32> = (0..src.len() as u32).collect();
    order.sort_unstable_by(|&a, &b| src[a as usize][c].total_cmp(&src[b as usize][c]));
    let mut ranks = vec![0u32; src.len()];
    let mut values: Vec<f32> = Vec::new();
    for &i in &order {
        let v = src[i as usize][c];
        if values.last().is_none_or(|&last| last != v || v.is_nan()) { values.push(v); }
        ranks[i as usize] = values.len() as u32 - 1;
    }
    (ranks, values)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> f32 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((self.0 >> 40) % 17) as f32 / 16.0 // few distinct values -> many ties
        }
    }

    fn brute(src: &[[f32; 3]], w: usize, h: usize, x: usize, y: usize, r: usize, c: usize) -> f32 {
        let mut v = Vec::new();
        for yy in -(r as isize)..=r as isize { for xx in -(r as isize)..=r as isize {
            let (px, py) = ((x as isize + xx).clamp(0, w as isize - 1) as usize, (y as isize + yy).clamp(0, h as isize - 1) as usize);
            v.push(src[py * w + px][c]);
        } }
        v.sort_by(f32::total_cmp);
        v[v.len() / 2]
    }

    #[test]
    fn matches_a_sorted_window_reference_with_ties_and_edge_clamp() {
        let (w, h) = (23, 17);
        let mut rng = Lcg(7);
        let src: Vec<[f32; 3]> = (0..w * h).map(|_| [rng.next(), rng.next(), rng.next()]).collect();
        for r in [1, 2, 5, 9] {
            let out = median_rect(&src, w, h, 0, 0, w, h, r);
            for y in 0..h { for x in 0..w { for c in 0..3 {
                assert_eq!(out[y * w + x][c], brute(&src, w, h, x, y, r, c), "r={r} at {x},{y} ch {c}");
            } } }
        }
        let gray: Vec<[f32; 3]> = src.iter().map(|p| [p[0]; 3]).collect();
        let out = median_rect(&gray, w, h, 0, 0, w, h, 3);
        for y in 0..h { for x in 0..w { assert_eq!(out[y * w + x], [brute(&gray, w, h, x, y, 3, 0); 3]); } }
    }

    #[test]
    fn removes_impulse_noise_keeps_steps_and_sub_regions_agree_with_the_whole() {
        let (w, h) = (20, 20);
        let mut src = vec![[0.25; 3]; w * h];
        src[10 * w + 10] = [1.0; 3]; // salt
        src[4 * w + 15] = [0.0; 3]; // pepper
        let out = median_rect(&src, w, h, 0, 0, w, h, 1);
        assert!(out.iter().all(|p| *p == [0.25; 3]));
        // A step edge stays exactly where it is.
        let step: Vec<[f32; 3]> = (0..w * h).map(|i| [if i % w < 8 { 0.0 } else { 1.0 }; 3]).collect();
        let out = median_rect(&step, w, h, 0, 0, w, h, 3);
        assert_eq!(out, step);
        // A sub-rectangle with enough margin equals the same pixels of the whole result.
        let mut rng = Lcg(3);
        let noisy: Vec<[f32; 3]> = (0..w * h).map(|_| [rng.next(), rng.next(), rng.next()]).collect();
        let whole = median_rect(&noisy, w, h, 0, 0, w, h, 2);
        let part = median_rect(&noisy, w, h, 5, 6, 8, 7, 2);
        for ty in 0..7 { for tx in 0..8 { assert_eq!(part[ty * 8 + tx], whole[(6 + ty) * w + 5 + tx]); } }
        assert_eq!(window_radius(2.4), 2); assert_eq!(window_radius(2.5), 3); assert_eq!(window_radius(f32::NAN), 0);
    }

    /// `cargo test --release -p subcraft-materials median_speed -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn median_speed_report_for_a_4k_texture_on_one_thread() {
        let mut rng = Lcg(1);
        for r in [2usize, 4, 8, 16] {
            let side = 64 + 2 * r;
            let rect: Vec<[f32; 3]> = (0..side * side).map(|_| [rng.next(), rng.next(), rng.next()]).collect();
            let start = std::time::Instant::now();
            std::hint::black_box(median_rect(&rect, side, side, r, r, 64, 64, r));
            let per_tile = start.elapsed();
            println!("r={r}: {per_tile:?} per 64x64 tile (colour), 4096x4096 = {:.1}s on one thread", per_tile.as_secs_f64() * 4096.0);
        }
    }
}
