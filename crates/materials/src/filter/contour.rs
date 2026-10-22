//! Exact squared Euclidean distance on a bounded scalar neighborhood, O(width * height).
//! No atlas boundary is invented: if no opposite texel exists, distance is infinite.
use subcraft_core::ChannelKind;
use subcraft_doc::{Filter, OutlinePosition};

pub fn inside(pixel: [f32; 4], channel: ChannelKind, threshold: f32) -> bool {
    let value = if channel.is_color() {
        pixel[0] * 0.2126 + pixel[1] * 0.7152 + pixel[2] * 0.0722
    } else {
        pixel[0]
    };
    pixel[3] > 0.0 && value * pixel[3] >= threshold
}

/// Lower envelope of parabolas f[q] + (x-q)^2. Skip infinite sites explicitly.
fn line(input: &[f32], output: &mut [f32], sites: &mut Vec<usize>, cuts: &mut Vec<f32>) {
    sites.clear();
    cuts.clear();
    for (q, &f) in input.iter().enumerate().filter(|(_, f)| f.is_finite()) {
        let mut cut = f32::NEG_INFINITY;
        while let Some(&p) = sites.last() {
            cut = (f + (q * q) as f32 - input[p] - (p * p) as f32) / (2.0 * (q - p) as f32);
            if cut > *cuts.last().unwrap() {
                break;
            }
            sites.pop();
            cuts.pop();
        }
        if sites.is_empty() {
            cut = f32::NEG_INFINITY;
        }
        sites.push(q);
        cuts.push(cut);
    }
    if sites.is_empty() {
        output.fill(f32::INFINITY);
        return;
    }
    let mut k = 0;
    for (x, v) in output.iter_mut().enumerate() {
        while k + 1 < sites.len() && cuts[k + 1] < x as f32 {
            k += 1;
        }
        let q = sites[k];
        *v = input[q] + (x as f32 - q as f32).powi(2);
    }
}

pub fn squared_distance(classes: &[bool], width: usize, height: usize, feature: bool) -> Vec<f32> {
    assert!(width > 0 && height > 0);
    assert_eq!(classes.len(), width * height);
    let mut field: Vec<_> = classes
        .iter()
        .map(|&v| if v == feature { 0.0 } else { f32::INFINITY })
        .collect();
    let n = width.max(height);
    let mut input = vec![0.0; n];
    let mut output = vec![0.0; n];
    let mut sites = Vec::with_capacity(n);
    let mut cuts = Vec::with_capacity(n);
    for row in field.chunks_exact_mut(width) {
        input[..width].copy_from_slice(row);
        line(&input[..width], &mut output[..width], &mut sites, &mut cuts);
        row.copy_from_slice(&output[..width]);
    }
    for x in 0..width {
        for y in 0..height {
            input[y] = field[y * width + x];
        }
        line(
            &input[..height],
            &mut output[..height],
            &mut sites,
            &mut cuts,
        );
        for y in 0..height {
            field[y * width + x] = output[y];
        }
    }
    field
}

/// Nearest opposite-class center minus half a pixel approximates the binary contour.
pub fn value(
    filter: Filter,
    inside: bool,
    squared_distance: f32,
    radius: f32,
    channel: ChannelKind,
) -> f32 {
    let distance = (squared_distance.sqrt() - 0.5).max(0.0);
    match filter {
        Filter::Bevel { amount, .. } => {
            let ramp = if !inside {
                0.0
            } else if radius == 0.0 {
                1.0
            } else {
                (distance / radius).clamp(0.0, 1.0)
            };
            let v = ramp * amount;
            if channel.is_signed() {
                v
            } else {
                v.clamp(0.0, 1.0)
            }
        }
        Filter::MaskOutline { position, .. } => {
            if radius == 0.0
                || matches!(
                    (position, inside),
                    (OutlinePosition::Inner, false) | (OutlinePosition::Outer, true)
                )
            {
                0.0
            } else {
                (radius + 0.5 - distance).clamp(0.0, 1.0)
            }
        }
        _ => unreachable!("contour filter required"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn separable_distance_matches_brute_force_including_empty_and_rectangular_fields() {
        for (w, h) in [(1, 1), (13, 9), (5, 21)] {
            for seed in [0, 1, 17] {
                let classes: Vec<_> = (0..w * h)
                    .map(|i| seed != 0 && (i * 17 + seed) % 23 < 4)
                    .collect();
                for feature in [true, false] {
                    let actual = squared_distance(&classes, w, h, feature);
                    for y in 0..h {
                        for x in 0..w {
                            let expected = (0..h)
                                .flat_map(|yy| (0..w).map(move |xx| (xx, yy)))
                                .filter(|&(xx, yy)| classes[yy * w + xx] == feature)
                                .map(|(xx, yy)| {
                                    (xx as f32 - x as f32).powi(2) + (yy as f32 - y as f32).powi(2)
                                })
                                .fold(f32::INFINITY, f32::min);
                            assert_eq!(actual[y * w + x], expected);
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn contour_boundaries_zero_radius_and_signed_relief_are_explicit() {
        let bevel = Filter::Bevel {
            radius: 4.0,
            amount: -0.8,
            threshold: 0.5,
        };
        assert_eq!(value(bevel, true, 1.0, 4.0, ChannelKind::Height), -0.1);
        assert_eq!(
            value(bevel, true, f32::INFINITY, 4.0, ChannelKind::Height),
            -0.8
        );
        assert_eq!(value(bevel, true, 1.0, 0.0, ChannelKind::Height), -0.8);
        assert_eq!(value(bevel, false, 1.0, 4.0, ChannelKind::Height), 0.0);
        let outline = Filter::MaskOutline {
            radius: 2.0,
            threshold: 0.5,
            position: OutlinePosition::Both,
        };
        assert_eq!(value(outline, true, 4.0, 2.0, ChannelKind::Roughness), 1.0);
        assert_eq!(value(outline, false, 9.0, 2.0, ChannelKind::Roughness), 0.0);
        assert_eq!(value(outline, true, 1.0, 0.0, ChannelKind::Roughness), 0.0);
        assert!(!inside([1.0, 0.0, 0.0, 1.0], ChannelKind::BaseColor, 0.5));
        assert!(!inside(
            [1.0; 4].map(|v| v * 0.0),
            ChannelKind::Height,
            -0.5
        ));
    }
}
