//! UV padding (dilation): extend island texels into the gutter so that filtering and mipmapping
//! never pull in undefined texels.

/// Fill every uncovered texel with the value of the nearest covered texel (jump flooding,
/// O(n log n)). `data` is row-major with `comps` interleaved components.
pub fn dilate_infinite(width: usize, height: usize, comps: usize, data: &mut [f32], covered: &[bool]) {
    let n = width * height;
    assert_eq!(covered.len(), n);
    assert_eq!(data.len(), n * comps);
    if !covered.iter().any(|&c| c) {
        return;
    }
    const NONE: u32 = u32::MAX;
    let mut seed: Vec<u32> = (0..n).map(|i| if covered[i] { i as u32 } else { NONE }).collect();
    let mut next = seed.clone();
    let dist2 = |a: usize, b: u32| -> u64 {
        let (ax, ay) = ((a % width) as i64, (a / width) as i64);
        let (bx, by) = ((b as usize % width) as i64, (b as usize / width) as i64);
        ((ax - bx).pow(2) + (ay - by).pow(2)) as u64
    };
    let mut step = (width.max(height).next_power_of_two() / 2).max(1);
    loop {
        for y in 0..height {
            for x in 0..width {
                let i = y * width + x;
                let mut best = seed[i];
                let mut best_d = if best == NONE { u64::MAX } else { dist2(i, best) };
                for dy in [-1i64, 0, 1] {
                    for dx in [-1i64, 0, 1] {
                        let nx = x as i64 + dx * step as i64;
                        let ny = y as i64 + dy * step as i64;
                        if nx < 0 || ny < 0 || nx >= width as i64 || ny >= height as i64 {
                            continue;
                        }
                        let s = seed[ny as usize * width + nx as usize];
                        if s != NONE {
                            let d = dist2(i, s);
                            if d < best_d {
                                best_d = d;
                                best = s;
                            }
                        }
                    }
                }
                next[i] = best;
            }
        }
        std::mem::swap(&mut seed, &mut next);
        if step == 1 {
            break;
        }
        step /= 2;
    }
    for i in 0..n {
        if !covered[i] && seed[i] != NONE {
            let s = seed[i] as usize;
            for c in 0..comps {
                data[i * comps + c] = data[s * comps + c];
            }
        }
    }
}

/// Grow covered regions by `width_px` texels; each new ring averages its covered neighbors.
/// Returns the grown coverage.
pub fn dilate_width(
    width: usize,
    height: usize,
    comps: usize,
    data: &mut [f32],
    covered: &[bool],
    width_px: u32,
) -> Vec<bool> {
    let mut cov = covered.to_vec();
    for _ in 0..width_px {
        let mut new_cov = cov.clone();
        let mut writes: Vec<(usize, [f32; 4])> = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let i = y * width + x;
                if cov[i] {
                    continue;
                }
                let mut acc = [0.0f32; 4];
                let mut count = 0;
                for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)] {
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                        continue;
                    }
                    let j = ny as usize * width + nx as usize;
                    if cov[j] {
                        for c in 0..comps {
                            acc[c] += data[j * comps + c];
                        }
                        count += 1;
                    }
                }
                if count > 0 {
                    for v in acc.iter_mut().take(comps) {
                        *v /= count as f32;
                    }
                    writes.push((i, acc));
                    new_cov[i] = true;
                }
            }
        }
        if writes.is_empty() {
            break;
        }
        for (i, v) in writes {
            data[i * comps..i * comps + comps].copy_from_slice(&v[..comps]);
        }
        cov = new_cov;
    }
    cov
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infinite_fills_from_nearest() {
        // 5x1: covered at 0 (value 1) and 4 (value 9).
        let mut data = vec![1.0, 0.0, 0.0, 0.0, 9.0];
        let cov = vec![true, false, false, false, true];
        dilate_infinite(5, 1, 1, &mut data, &cov);
        assert_eq!(data, vec![1.0, 1.0, 1.0, 9.0, 9.0]);
    }

    #[test]
    fn width_limited() {
        let mut data = vec![0.0, 0.0, 0.0, 5.0, 0.0, 0.0, 0.0];
        let cov = vec![false, false, false, true, false, false, false];
        let grown = dilate_width(7, 1, 1, &mut data, &cov, 2);
        assert_eq!(data, vec![0.0, 5.0, 5.0, 5.0, 5.0, 5.0, 0.0]);
        assert_eq!(grown, vec![false, true, true, true, true, true, false]);
    }
}
