//! Heal: a clone stroke that copies the detail of the source but keeps the tone of the target.
//!
//! Each target image's pre-stroke snapshot is reduced to a coarse grid of alpha-weighted
//! averages ([`LowPass`]). A healed texel is the cloned value plus (target average - source
//! average): the source's fine detail lands on the target's colour, so the patch blends in
//! instead of carrying a different tone over.

use subcraft_core::ChannelKind;

use crate::target::TargetImage;

/// Coarse cells across the texture when the brush size is unknown.
pub const DEFAULT_CELLS: u32 = 64;

/// Cells across the texture for a brush of radius `radius_uv`: a cell is about one brush radius
/// wide, so detail smaller than the brush is carried over and tone changes larger than it are
/// not.
pub fn cells_for(radius_uv: f32) -> u32 {
    if !(radius_uv.is_finite() && radius_uv > 0.0) {
        return DEFAULT_CELLS;
    }
    (1.0 / radius_uv).round().clamp(4.0, 128.0) as u32
}

/// Tone of an image over a coarse grid (alpha-weighted medians).
#[derive(Clone, Debug)]
pub struct LowPass {
    cells: u32,
    /// Per cell: premultiplied value sums and the alpha sum, divided by the texel count.
    data: Vec<[f32; 4]>,
}

/// Histogram bins per channel and cell for the medians.
const BINS: usize = 64;

impl LowPass {
    /// Per cell: the alpha-weighted median of each value (the mean for height, whose range is
    /// open), so small bright or dark details inside a cell (the very detail heal carries over)
    /// do not shift the tone it measures; and the mean alpha.
    pub fn new(img: &TargetImage, cells: u32) -> Self {
        let (w, h) = (img.snapshot.width(), img.snapshot.height());
        let cells = cells.min(w).min(h).max(1);
        let n = img.value_comps.min(3);
        let median = img.key.channel() != Some(ChannelKind::Height);
        let count = (cells * cells) as usize;
        let mut sums = vec![[0.0f32; 4]; count];
        let mut texels = vec![0u32; count];
        // Per cell, channel and bin: the alpha weight and the weighted value sum.
        let mut hist = if median { vec![[0.0f32; 2]; count * n * BINS] } else { Vec::new() };
        for y in 0..h {
            let cy = (y as u64 * cells as u64 / h as u64) as u32;
            for x in 0..w {
                let cx = (x as u64 * cells as u64 / w as u64) as u32;
                let px = img.snapshot.get(x, y);
                let a = if img.has_alpha { px[img.value_comps] } else { 1.0 };
                let i = (cy * cells + cx) as usize;
                texels[i] += 1;
                sums[i][3] += a;
                if a <= 0.0 {
                    continue;
                }
                for j in 0..n {
                    sums[i][j] += px[j] * a;
                    if median {
                        let b = ((px[j].clamp(0.0, 1.0) * BINS as f32) as usize).min(BINS - 1);
                        let bin = &mut hist[(i * n + j) * BINS + b];
                        bin[0] += a;
                        bin[1] += px[j] * a;
                    }
                }
            }
        }
        let data = (0..count)
            .map(|i| {
                let t = texels[i].max(1) as f32;
                let (weight, alpha) = (sums[i][3], sums[i][3] / t);
                let mut d = [0.0f32; 4];
                d[3] = alpha;
                if weight <= 0.0 {
                    return d;
                }
                for j in 0..n {
                    let value = if median { median_of(&hist[(i * n + j) * BINS..][..BINS], weight) } else { sums[i][j] / weight };
                    // Stored premultiplied by the mean alpha, as `sample` blends them.
                    d[j] = value * alpha;
                }
                d
            })
            .collect();
        Self { cells, data }
    }

    /// Smooth average at `uv` (bilinear between cell centres), or `None` where the image holds
    /// almost no paint.
    pub fn sample(&self, uv: [f32; 2]) -> Option<[f32; 3]> {
        let c = self.cells as f32;
        let x = (uv[0] * c - 0.5).clamp(0.0, c - 1.0);
        let y = (uv[1] * c - 0.5).clamp(0.0, c - 1.0);
        let (x0, y0) = (x.floor() as u32, y.floor() as u32);
        let (x1, y1) = ((x0 + 1).min(self.cells - 1), (y0 + 1).min(self.cells - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let mut s = [0.0f32; 4];
        for (xx, yy, wt) in [(x0, y0, (1.0 - fx) * (1.0 - fy)), (x1, y0, fx * (1.0 - fy)), (x0, y1, (1.0 - fx) * fy), (x1, y1, fx * fy)] {
            let d = self.data[(yy * self.cells + xx) as usize];
            for j in 0..4 {
                s[j] += d[j] * wt;
            }
        }
        (s[3] > 0.05).then(|| [s[0] / s[3], s[1] / s[3], s[2] / s[3]])
    }
}

/// Weighted median of a 0..1 histogram: the mean value of the bin the middle weight falls in
/// (exact for a cell of one value).
fn median_of(bins: &[[f32; 2]], total: f32) -> f32 {
    let half = total * 0.5;
    let mut acc = 0.0;
    for &[w, sum] in bins {
        if w > 0.0 && acc + w >= half {
            return sum / w;
        }
        acc += w;
    }
    1.0
}

/// Whether heal shifts the values of `img`: not normals (a shifted normal is no longer a
/// direction; they are cloned as they are) and not stamp slopes.
pub fn heals(img: &TargetImage) -> bool {
    img.stamp.is_none() && img.key.channel() != Some(ChannelKind::Normal)
}

/// Heal one cloned tile in place: `source[i]` was sampled at `uvs[i]`; `target_uv(i)` is the
/// texel's own position.
pub fn heal_tile(img: &TargetImage, low: &LowPass, source: &mut [[f32; 4]], uvs: &[Option<[f32; 2]>], target_uv: impl Fn(usize) -> [f32; 2]) {
    let n = img.value_comps.min(3);
    let clamp = img.key.channel() != Some(ChannelKind::Height);
    for (i, (out, uv)) in source.iter_mut().zip(uvs).enumerate() {
        let Some(uv) = uv else { continue };
        if out[3] <= 0.0 {
            continue;
        }
        let (Some(from), Some(to)) = (low.sample(*uv), low.sample(target_uv(i))) else { continue };
        for j in 0..n {
            let v = out[j] + to[j] - from[j];
            out[j] = if clamp { v.clamp(0.0, 1.0) } else { v };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_median_ignores_sparse_detail_and_is_exact_for_one_value() {
        let mut bins = vec![[0.0f32; 2]; BINS];
        let put = |bins: &mut Vec<[f32; 2]>, v: f32, w: f32| {
            let b = ((v * BINS as f32) as usize).min(BINS - 1);
            bins[b][0] += w;
            bins[b][1] += v * w;
        };
        put(&mut bins, 0.3, 80.0);
        put(&mut bins, 1.0, 20.0);
        assert!((median_of(&bins, 100.0) - 0.3).abs() < 1e-6, "a fifth of white dots leave the tone at 0.3");
        assert_eq!(cells_for(0.1), 10);
        assert_eq!(cells_for(0.0001), 128);
        assert_eq!(cells_for(f32::NAN), DEFAULT_CELLS);
    }
}
