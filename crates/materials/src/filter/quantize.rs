//! Quantize: reduce an image to a palette of a few colours picked from the image itself (median
//! cut refined by k-means), with optional ordered or noise dithering. The palette is computed once
//! per texture from [`palette`] and every texel is then mapped with [`map`].
use subcraft_core::ChannelKind;
use subcraft_doc::DitherPattern;

/// Texels read at most to build the palette (a regular grid over the texture).
pub const MAX_PALETTE_SAMPLES: usize = 1 << 16;
/// k-means passes after the median cut.
const REFINE_PASSES: usize = 6;

/// Settings for mapping texels onto a palette.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quantize {
    /// 0 off .. 1: dither offsets up to the typical spacing between palette colours.
    pub dithering: f32,
    pub pattern: DitherPattern,
    /// Nearest colour by CIELAB distance (colour) rather than linear RGB distance (data).
    pub lab: bool,
    pub apply_to_alpha: bool,
    pub alpha_threshold: f32,
}

/// A palette: the colours in linear RGB plus their coordinates in the distance space.
#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub colors: Vec<[f32; 3]>,
    space: Vec<[f32; 3]>,
    lab: bool,
    /// Mean per-component step (largest component difference, linear RGB) from each colour to its
    /// nearest neighbour: the dither scale.
    spread: f32,
}

/// Linear RGB (Rec.709 primaries, D65) to CIELAB, with L scaled to 0..1 like the other axes.
pub fn linear_to_lab(c: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = c.map(|v| v.max(0.0));
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.950_47;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.088_83;
    let f = |t: f32| if t > 0.008_856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [1.16 * fy - 0.16, 5.0 * (fx - fy), 2.0 * (fy - fz)]
}

fn to_space(c: [f32; 3], lab: bool) -> [f32; 3] {
    if lab { linear_to_lab(c) } else { c }
}

fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

impl Palette {
    /// Index of the colour nearest `c` (linear RGB).
    pub fn nearest(&self, c: [f32; 3]) -> usize {
        let p = to_space(c, self.lab);
        let mut best = (0, f32::INFINITY);
        for (i, s) in self.space.iter().enumerate() {
            let d = dist2(p, *s);
            if d < best.1 {
                best = (i, d);
            }
        }
        best.0
    }
    pub fn spread(&self) -> f32 {
        self.spread
    }
}

/// Up to `count` colours that represent `samples` (linear RGB): a median cut in the distance
/// space, then k-means passes. Deterministic for the same samples. Fewer colours come back when
/// the samples have fewer distinct values.
pub fn palette(samples: &[[f32; 3]], count: usize, lab: bool) -> Palette {
    let count = count.max(1);
    let points: Vec<([f32; 3], [f32; 3])> = samples.iter().map(|&c| (c, to_space(c, lab))).collect();
    if points.is_empty() {
        return Palette { colors: Vec::new(), space: Vec::new(), lab, spread: 0.0 };
    }
    // Median cut: split the box with the widest axis (weighted by its size) at its median.
    let mut boxes: Vec<Vec<usize>> = vec![(0..points.len()).collect()];
    while boxes.len() < count {
        let mut pick = None;
        for (bi, b) in boxes.iter().enumerate() {
            if b.len() < 2 {
                continue;
            }
            for axis in 0..3 {
                let (lo, hi) = b.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &i| (lo.min(points[i].1[axis]), hi.max(points[i].1[axis])));
                let score = (hi - lo) * (b.len() as f32).sqrt();
                if hi - lo > 1e-6 && pick.is_none_or(|(_, _, s)| score > s) {
                    pick = Some((bi, axis, score));
                }
            }
        }
        let Some((bi, axis, _)) = pick else { break };
        let mut b = std::mem::take(&mut boxes[bi]);
        b.sort_by(|&x, &y| points[x].1[axis].total_cmp(&points[y].1[axis]).then(x.cmp(&y)));
        // Split at the median, moved off a run of equal values so both halves differ.
        let mut mid = b.len() / 2;
        let at = |k: usize| points[b[k]].1[axis];
        while mid > 0 && at(mid) == at(mid - 1) {
            mid -= 1;
        }
        if mid == 0 {
            mid = b.len() / 2;
            while mid < b.len() && at(mid) == at(mid - 1) {
                mid += 1;
            }
        }
        let rest = b.split_off(mid);
        boxes[bi] = b;
        boxes.push(rest);
    }
    let mean = |members: &mut dyn Iterator<Item = usize>| {
        let (mut s, mut n) = ([0.0f64; 3], 0usize);
        for i in members {
            for c in 0..3 {
                s[c] += points[i].0[c] as f64;
            }
            n += 1;
        }
        (n > 0).then(|| s.map(|v| (v / n as f64) as f32))
    };
    let mut colors: Vec<[f32; 3]> = boxes.iter().filter_map(|b| mean(&mut b.iter().copied())).collect();
    // k-means: assign in the distance space, average in linear RGB.
    for _ in 0..REFINE_PASSES {
        let space: Vec<[f32; 3]> = colors.iter().map(|&c| to_space(c, lab)).collect();
        let mut sums = vec![([0.0f64; 3], 0usize); colors.len()];
        for (rgb, p) in &points {
            let k = space.iter().enumerate().map(|(i, s)| (i, dist2(*p, *s))).min_by(|a, b| a.1.total_cmp(&b.1)).unwrap().0;
            for c in 0..3 {
                sums[k].0[c] += rgb[c] as f64;
            }
            sums[k].1 += 1;
        }
        let next: Vec<[f32; 3]> = sums.iter().zip(&colors).filter(|(s, _)| s.1 > 0).map(|(s, _)| s.0.map(|v| (v / s.1 as f64) as f32)).collect();
        if next == colors {
            break;
        }
        colors = next;
    }
    // Duplicates add nothing.
    let mut unique: Vec<[f32; 3]> = Vec::with_capacity(colors.len());
    for c in colors {
        if !unique.iter().any(|u| dist2(*u, c) < 1e-12) {
            unique.push(c);
        }
    }
    unique.sort_by(|a, b| a.iter().zip(b).map(|(x, y)| x.total_cmp(y)).find(|o| o.is_ne()).unwrap_or(std::cmp::Ordering::Equal));
    let spread = if unique.len() < 2 {
        0.0
    } else {
        let step = |a: &[f32; 3], b: &[f32; 3]| (0..3).map(|c| (a[c] - b[c]).abs()).fold(0.0f32, f32::max);
        let total: f32 = unique.iter().map(|a| unique.iter().filter(|b| *b != a).map(|b| step(a, b)).fold(f32::INFINITY, f32::min)).sum();
        total / unique.len() as f32
    };
    let space = unique.iter().map(|&c| to_space(c, lab)).collect();
    Palette { colors: unique, space, lab, spread }
}

const BAYER4: [u8; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];

/// Dither threshold in 0..1 at texel (x, y).
pub fn dither_threshold(x: u32, y: u32, pattern: DitherPattern) -> f32 {
    match pattern {
        DitherPattern::Bayer4 => (BAYER4[((y % 4) * 4 + x % 4) as usize] as f32 + 0.5) / 16.0,
        DitherPattern::Bayer8 => {
            // 8 x 8 Bayer from the 4 x 4 one: 4 * B4(x/2, y/2) + B2(x, y).
            let b2 = [0u32, 2, 3, 1][((y % 2) * 2 + x % 2) as usize];
            let b4 = BAYER4[(((y / 2) % 4) * 4 + (x / 2) % 4) as usize] as u32;
            ((4 * b4 + b2) as f32 + 0.5) / 64.0
        }
        DitherPattern::Noise => {
            let mut h = x.wrapping_mul(0x8da6_b343) ^ y.wrapping_mul(0xd816_3841) ^ 0x9e37_79b9;
            h ^= h >> 15;
            h = h.wrapping_mul(0x2c1b_3c6d);
            h ^= h >> 12;
            h = h.wrapping_mul(0x297a_2d39);
            h ^= h >> 15;
            (h >> 8) as f32 / (1u32 << 24) as f32
        }
    }
}

/// Texel `p` (straight colour, coverage) mapped onto `palette`.
pub fn map(p: [f32; 4], x: u32, y: u32, palette: &Palette, q: Quantize) -> [f32; 4] {
    let alpha = if q.apply_to_alpha { if p[3] >= q.alpha_threshold { 1.0 } else { 0.0 } } else { p[3] };
    if palette.colors.is_empty() {
        return [p[0], p[1], p[2], alpha];
    }
    let amount = q.dithering.clamp(0.0, 1.0);
    let c = if amount > 0.0 {
        let o = (dither_threshold(x, y, q.pattern) - 0.5) * amount * palette.spread();
        [p[0] + o, p[1] + o, p[2] + o]
    } else {
        [p[0], p[1], p[2]]
    };
    let [r, g, b] = palette.colors[palette.nearest(c)];
    [r, g, b, alpha]
}

/// Whether nearest colours use CIELAB: only for colour channels when asked; data channels are
/// always compared in their own values.
pub fn uses_lab(lab: bool, kind: ChannelKind) -> bool {
    lab && kind.is_color()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(dithering: f32) -> Quantize {
        Quantize { dithering, pattern: DitherPattern::Bayer4, lab: true, apply_to_alpha: false, alpha_threshold: 0.5 }
    }

    #[test]
    fn two_clusters_give_their_means() {
        let mut samples = Vec::new();
        for i in 0..50 {
            let j = i as f32 * 0.001;
            samples.push([0.1 + j, 0.2, 0.3]);
            samples.push([0.8, 0.7 - j, 0.1]);
        }
        let pal = palette(&samples, 2, true);
        assert_eq!(pal.colors.len(), 2);
        let near = |c: [f32; 3]| pal.colors.iter().map(|p| dist2(*p, c)).fold(f32::INFINITY, f32::min).sqrt();
        assert!(near([0.1245, 0.2, 0.3]) < 1e-3 && near([0.8, 0.6755, 0.1]) < 1e-3, "{:?}", pal.colors);
        // Mapping keeps coverage and snaps to a palette colour.
        let out = map([0.12, 0.21, 0.29, 0.4], 0, 0, &pal, q(0.0));
        assert!(near([out[0], out[1], out[2]]) < 1e-6 && out[3] == 0.4);
        assert!((out[0] - 0.1245).abs() < 1e-3);
    }

    #[test]
    fn palette_size_and_determinism() {
        // A grey ramp of 256 values into 4 colours: 4 distinct, evenly spread, same every time.
        let ramp: Vec<[f32; 3]> = (0..256).map(|i| [i as f32 / 255.0; 3]).collect();
        let a = palette(&ramp, 4, false);
        assert_eq!(a.colors.len(), 4);
        assert_eq!(a, palette(&ramp, 4, false));
        let greys: Vec<f32> = a.colors.iter().map(|c| c[0]).collect();
        for (g, want) in greys.iter().zip([0.125, 0.375, 0.625, 0.875]) {
            assert!((g - want).abs() < 0.02, "{greys:?}");
        }
        // Fewer distinct values than asked: only those come back.
        let two = vec![[0.2; 3], [0.2; 3], [0.6; 3]];
        assert_eq!(palette(&two, 8, true).colors, vec![[0.2; 3], [0.6; 3]]);
        assert!(palette(&[], 8, true).colors.is_empty());
    }

    #[test]
    fn dithering_mixes_neighbouring_colours() {
        let pal = palette(&[[0.0; 3], [1.0; 3]], 2, false);
        // A flat 0.5 area: without dithering one colour; with full dithering a 50 / 50 mix.
        let count = |amount: f32, pattern| {
            let qq = Quantize { pattern, ..q(amount) };
            (0..16u32).flat_map(|y| (0..16u32).map(move |x| (x, y))).filter(|&(x, y)| map([0.5, 0.5, 0.5, 1.0], x, y, &pal, qq)[0] > 0.5).count()
        };
        let plain = count(0.0, DitherPattern::Bayer4);
        assert!(plain == 0 || plain == 256);
        assert_eq!(count(1.0, DitherPattern::Bayer4), 128);
        assert_eq!(count(1.0, DitherPattern::Bayer8), 128);
        let noise = count(1.0, DitherPattern::Noise);
        assert!((96..=160).contains(&noise), "noise about half: {noise}");
        // A 0.25 area gets about a quarter of white.
        let qq = q(1.0);
        let white = (0..4u32).flat_map(|y| (0..4u32).map(move |x| (x, y))).filter(|&(x, y)| map([0.25, 0.25, 0.25, 1.0], x, y, &pal, qq)[0] > 0.5).count();
        assert_eq!(white, 4);
    }

    #[test]
    fn bayer_thresholds_cover_the_range_once() {
        let mut t4: Vec<f32> = (0..16).map(|i| dither_threshold(i % 4, i / 4, DitherPattern::Bayer4)).collect();
        t4.sort_by(f32::total_cmp);
        assert!(t4.iter().enumerate().all(|(i, t)| (t - (i as f32 + 0.5) / 16.0).abs() < 1e-6));
        let mut t8: Vec<f32> = (0..64).map(|i| dither_threshold(i % 8, i / 8, DitherPattern::Bayer8)).collect();
        t8.sort_by(f32::total_cmp);
        assert!(t8.iter().enumerate().all(|(i, t)| (t - (i as f32 + 0.5) / 64.0).abs() < 1e-6));
    }

    #[test]
    fn alpha_threshold_and_lab() {
        let pal = palette(&[[0.3; 3]], 4, true);
        let qq = Quantize { apply_to_alpha: true, alpha_threshold: 0.5, ..q(0.0) };
        assert_eq!(map([0.3, 0.3, 0.3, 0.49], 0, 0, &pal, qq)[3], 0.0);
        assert_eq!(map([0.3, 0.3, 0.3, 0.5], 0, 0, &pal, qq)[3], 1.0);
        // Lab: white is L 1, black L 0, neutral greys have no a/b.
        let w = linear_to_lab([1.0; 3]);
        assert!((w[0] - 1.0).abs() < 1e-3 && w[1].abs() < 1e-3 && w[2].abs() < 1e-3, "{w:?}");
        assert!(linear_to_lab([0.0; 3])[0].abs() < 1e-6);
        assert!(uses_lab(true, ChannelKind::BaseColor) && !uses_lab(true, ChannelKind::Roughness) && !uses_lab(false, ChannelKind::BaseColor));
    }
}
