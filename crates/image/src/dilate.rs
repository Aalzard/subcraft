//! UV padding (dilation): extend island texels into the gutter so that filtering and mipmapping
//! never pull in undefined texels.

/// For every texel, the index of the nearest covered texel (jump flooding, O(n log n), rows in
/// parallel); `u32::MAX` everywhere when nothing is covered. Use it to pad several images that share
/// one coverage with [`apply_nearest`].
pub fn nearest_covered(width: usize, height: usize, covered: &[bool]) -> Vec<u32> {
    use rayon::prelude::*;
    let n = width * height;
    assert_eq!(covered.len(), n);
    const NONE: u32 = u32::MAX;
    let mut seed: Vec<u32> = (0..n).map(|i| if covered[i] { i as u32 } else { NONE }).collect();
    if !covered.iter().any(|&c| c) {
        return seed;
    }
    let mut next = seed.clone();
    let dist2 = |a: usize, b: u32| -> u64 {
        let (ax, ay) = ((a % width) as i64, (a / width) as i64);
        let (bx, by) = ((b as usize % width) as i64, (b as usize / width) as i64);
        ((ax - bx).pow(2) + (ay - by).pow(2)) as u64
    };
    let mut step = (width.max(height).next_power_of_two() / 2).max(1);
    loop {
        let src = &seed;
        next.par_chunks_mut(width).enumerate().for_each(|(y, row)| {
            for (x, out) in row.iter_mut().enumerate() {
                let i = y * width + x;
                let mut best = src[i];
                let mut best_d = if best == NONE { u64::MAX } else { dist2(i, best) };
                for dy in [-1i64, 0, 1] {
                    for dx in [-1i64, 0, 1] {
                        let nx = x as i64 + dx * step as i64;
                        let ny = y as i64 + dy * step as i64;
                        if nx < 0 || ny < 0 || nx >= width as i64 || ny >= height as i64 {
                            continue;
                        }
                        let s = src[ny as usize * width + nx as usize];
                        if s != NONE {
                            let d = dist2(i, s);
                            if d < best_d {
                                best_d = d;
                                best = s;
                            }
                        }
                    }
                }
                *out = best;
            }
        });
        std::mem::swap(&mut seed, &mut next);
        if step == 1 {
            break;
        }
        step /= 2;
    }
    seed
}

/// Copy each uncovered texel's value from its nearest covered texel (see [`nearest_covered`]).
pub fn apply_nearest(comps: usize, data: &mut [f32], covered: &[bool], nearest: &[u32]) {
    use rayon::prelude::*;
    // Covered texels are never written, so reading their values from a snapshot is exact.
    let src = data.to_vec();
    data.par_chunks_mut(comps).enumerate().for_each(|(i, dst)| {
        if !covered[i] && nearest[i] != u32::MAX {
            let s = nearest[i] as usize;
            dst.copy_from_slice(&src[s * comps..(s + 1) * comps]);
        }
    });
}

/// Fill every uncovered texel with the value of the nearest covered texel (jump flooding,
/// O(n log n)). `data` is row-major with `comps` interleaved components.
pub fn dilate_infinite(width: usize, height: usize, comps: usize, data: &mut [f32], covered: &[bool]) {
    assert_eq!(data.len(), width * height * comps);
    let nearest = nearest_covered(width, height, covered);
    apply_nearest(comps, data, covered, &nearest);
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Padding {
    /// No padding: gutter texels keep their composite values.
    None,
    /// Grow islands by this many texels; the rest of the gutter keeps its composite values.
    Texels(u32),
    /// Fill the whole gutter with the nearest island texel (best for mipmaps).
    Infinite,
    /// Painter's "Dilation + default background color": grow islands by this many texels, then
    /// fill the rest of the gutter with each output's default (the value an empty texture set
    /// exports: flat normal, white occlusion, black base color...).
    Background(u32),
    /// Painter's "Dilation + diffusion": grow islands by this many texels, then fill the rest of
    /// the gutter with a smooth blend of the surrounding islands.
    Diffusion(u32),
    /// Painter's "Dilation + transparent": grow islands by this many texels; the rest of the
    /// gutter is transparent (alpha 0). Outputs without alpha get one; file types that store no
    /// alpha (JPEG, HDR, Netpbm) get dilation only.
    Transparent(u32),
}

impl Padding {
    /// Dilation distance of the modes that have one.
    pub fn distance(self) -> Option<u32> {
        match self {
            Padding::Texels(n) | Padding::Background(n) | Padding::Diffusion(n) | Padding::Transparent(n) => Some(n),
            Padding::None | Padding::Infinite => None,
        }
    }

    /// The same mode with another dilation distance (modes without one are unchanged).
    pub fn with_distance(self, n: u32) -> Padding {
        match self {
            Padding::Texels(_) => Padding::Texels(n),
            Padding::Background(_) => Padding::Background(n),
            Padding::Diffusion(_) => Padding::Diffusion(n),
            Padding::Transparent(_) => Padding::Transparent(n),
            other => other,
        }
    }
}

/// Fill every texel outside `known` with a smooth blend of the known texels around it
/// (push-pull: average the known values down a pyramid, then interpolate back up). Known texels
/// are unchanged. This is Painter's "diffusion" padding: gutters fade between neighbouring
/// islands instead of copying hard island edges outwards.
pub fn diffuse_fill(width: usize, height: usize, comps: usize, data: &mut [f32], known: &[bool]) {
    assert_eq!(data.len(), width * height * comps);
    assert_eq!(known.len(), width * height);
    if !known.iter().any(|&k| k) || known.iter().all(|&k| k) {
        return;
    }
    // levels[i] = (w, h, values, weights); weights in 0..1.
    let mut levels: Vec<(usize, usize, Vec<f32>, Vec<f32>)> =
        vec![(width, height, data.to_vec(), known.iter().map(|&k| if k { 1.0 } else { 0.0 }).collect())];
    while levels.last().is_some_and(|l| l.0 > 1 || l.1 > 1) {
        let (fw, fh, fv, fwt) = levels.last().expect("non-empty");
        let (cw, ch) = (fw.div_ceil(2), fh.div_ceil(2));
        let mut cv = vec![0.0f32; cw * ch * comps];
        let mut cwt = vec![0.0f32; cw * ch];
        for y in 0..ch {
            for x in 0..cw {
                let mut sw = 0.0;
                let mut sv = [0.0f32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (sx, sy) = (2 * x + dx, 2 * y + dy);
                    if sx >= *fw || sy >= *fh {
                        continue;
                    }
                    let j = sy * fw + sx;
                    let w = fwt[j];
                    sw += w;
                    for c in 0..comps {
                        sv[c] += fv[j * comps + c] * w;
                    }
                }
                let i = y * cw + x;
                if sw > 0.0 {
                    for c in 0..comps {
                        cv[i * comps + c] = sv[c] / sw;
                    }
                }
                cwt[i] = sw.min(1.0);
            }
        }
        levels.push((cw, ch, cv, cwt));
    }
    // Pull: from the coarsest level back down, blend each texel with the interpolated level above.
    for li in (0..levels.len() - 1).rev() {
        let (upper, lower) = levels.split_at_mut(li + 1);
        let (fw, fh, fv, fwt) = &mut upper[li];
        let (cw, ch, cv, _) = &lower[0];
        for y in 0..*fh {
            let cy = ((y as f32 + 0.5) * 0.5 - 0.5).clamp(0.0, (*ch - 1) as f32);
            let (y0, ty) = (cy.floor() as usize, cy.fract());
            let y1 = (y0 + 1).min(ch - 1);
            for x in 0..*fw {
                let i = y * *fw + x;
                let w = fwt[i];
                if w >= 1.0 {
                    continue;
                }
                let cx = ((x as f32 + 0.5) * 0.5 - 0.5).clamp(0.0, (*cw - 1) as f32);
                let (x0, tx) = (cx.floor() as usize, cx.fract());
                let x1 = (x0 + 1).min(cw - 1);
                for c in 0..comps {
                    let at = |xx: usize, yy: usize| cv[(yy * cw + xx) * comps + c];
                    let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx;
                    let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx;
                    let up = top + (bottom - top) * ty;
                    fv[i * comps + c] = fv[i * comps + c] * w + up * (1.0 - w);
                }
                fwt[i] = 1.0;
            }
        }
    }
    for (i, k) in known.iter().enumerate() {
        if !k {
            data[i * comps..(i + 1) * comps].copy_from_slice(&levels[0].2[i * comps..(i + 1) * comps]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diffusion_blends_between_islands_and_keeps_known_texels() {
        // 9x1: 0 on the left edge, 1 on the right edge; the gap fades monotonically.
        let mut data = vec![0.0f32; 9];
        data[8] = 1.0;
        let mut known = vec![false; 9];
        known[0] = true;
        known[8] = true;
        diffuse_fill(9, 1, 1, &mut data, &known);
        assert_eq!((data[0], data[8]), (0.0, 1.0), "known texels unchanged");
        for w in data.windows(2) {
            assert!(w[1] >= w[0] - 1e-6, "monotonic: {data:?}");
        }
        assert!(data[1] < 0.5 && data[7] > 0.5 && data[1] > 0.0 && data[7] < 1.0, "a gradient, not a copy: {data:?}");
        // 2D, 3 components: every gap texel stays within the range of the known values.
        let (w, h) = (16, 16);
        let mut img = vec![0.0f32; w * h * 3];
        let mut k = vec![false; w * h];
        for (i, v) in [(17usize, [1.0, 0.0, 0.0]), (230, [0.0, 0.0, 1.0])] {
            img[i * 3..i * 3 + 3].copy_from_slice(&v);
            k[i] = true;
        }
        diffuse_fill(w, h, 3, &mut img, &k);
        assert!(img.iter().all(|v| (-1e-6..=1.0 + 1e-6).contains(v)));
        assert!(img.chunks_exact(3).all(|p| (p[0] + p[2] - 1.0).abs() < 1e-4 && p[1].abs() < 1e-6), "a blend of the two");
        // Nothing known, or everything known: untouched.
        let mut same = vec![0.3f32; 4];
        diffuse_fill(2, 2, 1, &mut same, &[false; 4]);
        assert_eq!(same, vec![0.3; 4]);
    }

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
