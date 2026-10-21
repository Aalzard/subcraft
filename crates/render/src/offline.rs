//! Pure helpers for high-quality stills and turntable frames (no GPU): camera angles, file names
//! and the linear-light box filter that turns a supersampled render into the final image.

use std::f32::consts::TAU;

/// Camera yaw (radians) of each frame of a full turn that starts at `start` and never repeats
/// the first view: frame `i` of `frames` is `start + i / frames` of a turn.
pub fn turntable_yaws(start: f32, frames: u32) -> Vec<f32> {
    (0..frames).map(|i| start + TAU * i as f32 / frames.max(1) as f32).collect()
}

/// `turntable_0007.png` style names, zero padded so the sequence sorts and imports in order.
pub fn turntable_file_name(index: u32) -> String {
    format!("turntable_{:04}.png", index + 1)
}

fn to_linear(v: u8) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn to_srgb(l: f32) -> u8 {
    let l = l.clamp(0.0, 1.0);
    let c = if l <= 0.003_130_8 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
    (c * 255.0).round() as u8
}

/// Average `factor` x `factor` blocks of an sRGB RGBA8 image in linear light (alpha linearly).
/// Edge pixels that do not fill a block are dropped, so the result is `size / factor` rounded down.
pub fn downsample_rgba8(src: &[u8], size: [u32; 2], factor: u32) -> (Vec<u8>, [u32; 2]) {
    let factor = factor.max(1);
    if factor == 1 {
        return (src.to_vec(), size);
    }
    let out = [(size[0] / factor).max(1), (size[1] / factor).max(1)];
    let mut dst = vec![0u8; (out[0] * out[1] * 4) as usize];
    let lut: Vec<f32> = (0..=255u8).map(to_linear).collect();
    let n = (factor * factor) as f32;
    for y in 0..out[1] {
        for x in 0..out[0] {
            let mut acc = [0.0f32; 4];
            for dy in 0..factor {
                for dx in 0..factor {
                    let sx = (x * factor + dx).min(size[0] - 1);
                    let sy = (y * factor + dy).min(size[1] - 1);
                    let i = ((sy * size[0] + sx) * 4) as usize;
                    for c in 0..3 {
                        acc[c] += lut[src[i + c] as usize];
                    }
                    acc[3] += src[i + 3] as f32 / 255.0;
                }
            }
            let o = ((y * out[0] + x) * 4) as usize;
            for c in 0..3 {
                dst[o + c] = to_srgb(acc[c] / n);
            }
            dst[o + 3] = ((acc[3] / n) * 255.0).round() as u8;
        }
    }
    (dst, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turntable_covers_one_full_turn_without_repeating_the_first_view() {
        let yaws = turntable_yaws(0.5, 36);
        assert_eq!(yaws.len(), 36);
        assert_eq!(yaws[0], 0.5);
        let step = TAU / 36.0;
        assert!(yaws.windows(2).all(|w| ((w[1] - w[0]) - step).abs() < 1e-5));
        assert!((yaws[35] + step - (0.5 + TAU)).abs() < 1e-4, "the next frame would be the first again");
        assert!(turntable_yaws(0.0, 0).is_empty());
    }

    #[test]
    fn frame_names_sort_in_order() {
        assert_eq!(turntable_file_name(0), "turntable_0001.png");
        assert_eq!(turntable_file_name(71), "turntable_0072.png");
        let mut names: Vec<_> = (0..120).map(turntable_file_name).collect();
        let sorted = names.clone();
        names.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn box_filter_averages_in_linear_light_and_keeps_flat_colors() {
        // A uniform image stays the same colour at any factor.
        let flat: Vec<u8> = (0..16).flat_map(|_| [200u8, 100, 50, 255]).collect();
        let (out, size) = downsample_rgba8(&flat, [4, 4], 2);
        assert_eq!(size, [2, 2]);
        assert!(out.as_chunks::<4>().0.iter().all(|p| *p == [200, 100, 50, 255]));
        // Black and white average to linear 0.5, which is sRGB 188, not 128.
        let checker: Vec<u8> = (0..4).flat_map(|i| if i % 2 == 0 { [0u8, 0, 0, 255] } else { [255, 255, 255, 255] }).collect();
        let (out, size) = downsample_rgba8(&checker, [2, 2], 2);
        assert_eq!(size, [1, 1]);
        assert!(out[..3].iter().all(|&v| (v as i32 - 188).abs() <= 1), "{out:?}");
        // Factor 1 is the identity; odd sizes drop the last row/column.
        assert_eq!(downsample_rgba8(&flat, [4, 4], 1).0, flat);
        assert_eq!(downsample_rgba8(&[255u8; 5 * 5 * 4], [5, 5], 2).1, [2, 2]);
    }
}
