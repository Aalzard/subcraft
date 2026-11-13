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

/// Squared Chebyshev (chessboard) distance to the nearest `feature` texel, searched up to
/// `reach` texels away (infinite beyond): square corners for Angular bevels.
pub fn squared_chebyshev(classes: &[bool], width: usize, height: usize, feature: bool, reach: usize) -> Vec<f32> {
    assert_eq!(classes.len(), width * height);
    // Per row: the horizontal distance to the nearest feature texel.
    let mut row = vec![f32::INFINITY; width * height];
    for y in 0..height {
        let line = &classes[y * width..(y + 1) * width];
        let out = &mut row[y * width..(y + 1) * width];
        let mut last = f32::INFINITY;
        for x in 0..width {
            last = if line[x] == feature { 0.0 } else { last + 1.0 };
            out[x] = last;
        }
        last = f32::INFINITY;
        for x in (0..width).rev() {
            last = if line[x] == feature { 0.0 } else { last + 1.0 };
            out[x] = out[x].min(last);
        }
    }
    // Per column: the smallest max(vertical, horizontal) distance within reach.
    let mut field = vec![f32::INFINITY; width * height];
    for x in 0..width {
        for y in 0..height {
            let mut best = f32::INFINITY;
            for q in y.saturating_sub(reach)..(y + reach + 1).min(height) {
                let d = (q as f32 - y as f32).abs().max(row[q * width + x]);
                best = best.min(d);
            }
            field[y * width + x] = best * best;
        }
    }
    field
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
        Filter::BevelShape { amount, smoothing, .. } => {
            let ramp = if !inside {
                0.0
            } else if radius == 0.0 {
                1.0
            } else {
                (distance / radius).clamp(0.0, 1.0)
            };
            let eased = ramp + (ramp * ramp * (3.0 - 2.0 * ramp) - ramp) * smoothing;
            let v = eased * amount;
            if channel.is_signed() { v } else { v.clamp(0.0, 1.0) }
        }
        Filter::BevelSmooth { direction, smoothing, curve_offset, curve_shape, .. } => {
            use subcraft_doc::BevelDirection as B;
            // How far along the ramp's width the texel is from the contour (1 with no width).
            let near = if radius == 0.0 { 1.0 } else { (distance / radius).clamp(0.0, 1.0) };
            let t = match (direction, inside) {
                (B::Inward, true) => near,
                (B::Inward, false) => 0.0,
                (B::Outward, true) => 1.0,
                (B::Outward, false) => 1.0 - near,
                (B::Both, true) => 0.5 + near * 0.5,
                (B::Both, false) => 0.5 - near * 0.5,
            };
            smooth_ramp(t, smoothing, curve_offset, curve_shape)
        }
        Filter::MaskOutlineShape { radius: width, position, start_black, blur, curve, .. } => {
            if width == 0.0
                || matches!((position, inside), (OutlinePosition::Inner, false) | (OutlinePosition::Outer, true))
            {
                return 0.0;
            }
            // The band, with its far edge faded over `blur` pixels (one pixel when sharp).
            let band = ((width + 0.5 + blur - distance) / (1.0 + blur)).clamp(0.0, 1.0);
            let ramp = if start_black { (distance / width).clamp(0.0, 1.0) } else { 1.0 };
            (band * ramp).powf(4f32.powf(curve))
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

/// Bevel Smooth's ramp profile: Curve Offset delays (above 0) or shortens (below 0) the ramp,
/// Curve Shape raises it to the power `4^-shape`, Smoothing eases it toward a smoothstep.
pub fn smooth_ramp(t: f32, smoothing: f32, curve_offset: f32, curve_shape: f32) -> f32 {
    let span = (1.0 - curve_offset.abs()).max(1e-3);
    let t = ((t - curve_offset.max(0.0)) / span).clamp(0.0, 1.0);
    let t = t.powf(4f32.powf(-curve_shape));
    t + (t * t * (3.0 - 2.0 * t) - t) * smoothing
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
    fn angular_distance_is_the_larger_axis_distance() {
        // A single feature texel in the middle of 7 x 5.
        let classes: Vec<bool> = (0..35).map(|i| i == 17).collect();
        let d = squared_chebyshev(&classes, 7, 5, true, 8);
        assert_eq!(d[17], 0.0);
        assert_eq!(d[0], 9.0, "corner (0, 0) is 3 columns and 2 rows away");
        assert_eq!(d[6], 9.0);
        assert_eq!(d[3], 4.0);
        // Beyond the reach: infinite.
        let near = squared_chebyshev(&classes, 7, 5, true, 1);
        assert_eq!(near[3], f32::INFINITY);
        assert_eq!(near[10], 1.0);
    }

    #[test]
    fn bevel_smooth_ramps_inward_outward_and_both_ways() {
        use subcraft_doc::BevelDirection as B;
        let f = |direction, smoothing, curve_offset, curve_shape| Filter::BevelSmooth { radius: 4.0, direction, smoothing, curve_offset, curve_shape, threshold: 0.5 };
        let at = |filter, inside, distance: f32| value(filter, inside, (distance + 0.5).powi(2), 4.0, ChannelKind::Roughness);
        // Inward: 0 outside, linear up to 1 at the width inside.
        assert_eq!(at(f(B::Inward, 0.0, 0.0, 0.0), false, 1.0), 0.0);
        assert!((at(f(B::Inward, 0.0, 0.0, 0.0), true, 1.0) - 0.25).abs() < 1e-6);
        assert_eq!(at(f(B::Inward, 0.0, 0.0, 0.0), true, 9.0), 1.0);
        // Outward: 1 inside, falling to 0 at the width outside.
        assert_eq!(at(f(B::Outward, 0.0, 0.0, 0.0), true, 1.0), 1.0);
        assert!((at(f(B::Outward, 0.0, 0.0, 0.0), false, 1.0) - 0.75).abs() < 1e-6);
        assert_eq!(at(f(B::Outward, 0.0, 0.0, 0.0), false, 9.0), 0.0);
        // Both: half at the contour, a width either side.
        assert!((at(f(B::Both, 0.0, 0.0, 0.0), true, 2.0) - 0.75).abs() < 1e-6);
        assert!((at(f(B::Both, 0.0, 0.0, 0.0), false, 2.0) - 0.25).abs() < 1e-6);
        // Smoothing 1 is a smoothstep; Curve Shape 0.5 is a square root; Curve Offset 0.5 starts half way.
        assert!((at(f(B::Inward, 1.0, 0.0, 0.0), true, 1.0) - 0.15625).abs() < 1e-6);
        assert!((at(f(B::Inward, 0.0, 0.0, 0.5), true, 1.0) - 0.5).abs() < 1e-6);
        assert_eq!(at(f(B::Inward, 0.0, 0.5, 0.0), true, 1.0), 0.0);
        assert!((at(f(B::Inward, 0.0, 0.5, 0.0), true, 3.0) - 0.5).abs() < 1e-6);
        assert!((at(f(B::Inward, 0.0, -0.5, 0.0), true, 1.0) - 0.5).abs() < 1e-6);
        // A uniform input (no contour in reach) is flat.
        assert_eq!(value(f(B::Both, 0.5, 0.0, 0.0), true, f32::INFINITY, 4.0, ChannelKind::Roughness), 1.0);
        assert_eq!(value(f(B::Both, 0.5, 0.0, 0.0), false, f32::INFINITY, 4.0, ChannelKind::Roughness), 0.0);
        assert_eq!(value(f(B::Inward, 0.5, 0.0, 0.0), true, 1.0, 0.0, ChannelKind::Roughness), 1.0, "no width");
    }

    #[test]
    fn bevel_smoothing_and_outline_shape_options() {
        let bevel = |smoothing| Filter::BevelShape { radius: 4.0, amount: 1.0, threshold: 0.5, corner: subcraft_doc::BevelCorner::Rounded, smoothing };
        // Distance 1.5 (squared 4 minus the half pixel) of 4: linear 0.375, smoothstep 0.3164.
        assert!((value(bevel(0.0), true, 4.0, 4.0, ChannelKind::Roughness) - 0.375).abs() < 1e-6);
        let s = 0.375f32 * 0.375 * (3.0 - 2.0 * 0.375);
        assert!((value(bevel(1.0), true, 4.0, 4.0, ChannelKind::Roughness) - s).abs() < 1e-6);
        let outline = |start_black, blur, curve| Filter::MaskOutlineShape { radius: 4.0, threshold: 0.5, position: OutlinePosition::Both, start_black, blur, curve };
        // Plain band: 1 inside the width, 0 past it.
        assert_eq!(value(outline(false, 0.0, 0.0), true, 4.0, 4.0, ChannelKind::Roughness), 1.0);
        assert_eq!(value(outline(false, 0.0, 0.0), false, 36.0, 4.0, ChannelKind::Roughness), 0.0);
        // Blur 2: distance 5.5 fades to (4.5 + 2 - 5.5) / 3 = 1/3.
        let d = (5.5f32 + 0.5).powi(2);
        assert!((value(outline(false, 2.0, 0.0), false, d, 4.0, ChannelKind::Roughness) - 1.0 / 3.0).abs() < 1e-5);
        // Start From Black: 0 at the contour, half way at half the width.
        assert_eq!(value(outline(true, 0.0, 0.0), true, 0.25, 4.0, ChannelKind::Roughness), 0.0);
        assert!((value(outline(true, 0.0, 0.0), true, 2.5f32.powi(2), 4.0, ChannelKind::Roughness) - 0.5).abs() < 1e-6);
        // Curve 0.5: squared.
        assert!((value(outline(true, 0.0, 0.5), true, 2.5f32.powi(2), 4.0, ChannelKind::Roughness) - 0.25).abs() < 1e-6);
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
