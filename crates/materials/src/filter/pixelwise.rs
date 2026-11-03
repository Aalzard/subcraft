//! Neighbourhood filters that read source pixels directly: Edge Detect, Directional Warp, Mosaic.
//! Pure functions over a pixel reader so the compositor and the tests share the same math.
use subcraft_core::ChannelKind;

/// Value a spatial filter reads: Rec.709 luminance for colour, the raw first component otherwise.
pub fn scalar(p: [f32; 4], channel: ChannelKind) -> f32 {
    if channel.is_color() { p[0] * 0.2126 + p[1] * 0.7152 + p[2] * 0.0722 } else { p[0] }
}

/// Sobel magnitude with taps `d = max(round(radius), 1)` pixels apart, normalised so a unit step
/// gives 1, times `strength`, clamped to 0..=1. `read(dx, dy)` is relative to the texel.
pub fn edge_detect(read: impl Fn(i32, i32) -> f32, radius: f32, strength: f32) -> f32 {
    let d = radius.round().max(1.0) as i32;
    let p = |x, y| read(x * d, y * d);
    let gx = (p(1, -1) + 2.0 * p(1, 0) + p(1, 1)) - (p(-1, -1) + 2.0 * p(-1, 0) + p(-1, 1));
    let gy = (p(-1, 1) + 2.0 * p(0, 1) + p(1, 1)) - (p(-1, -1) + 2.0 * p(0, -1) + p(1, -1));
    ((gx * gx + gy * gy).sqrt() * 0.25 * strength).clamp(0.0, 1.0)
}

/// Source offset in pixels for Directional Warp from the value `v` read at the texel itself.
pub fn directional_offset(v: f32, intensity: f32, angle_deg: f32, channel: ChannelKind) -> [f32; 2] {
    // Clamped so HDR values never read beyond `intensity` pixels (the compositor's tile margin).
    let k = (if channel.is_signed() { v } else { 2.0 * v - 1.0 }).clamp(-1.0, 1.0) * intensity;
    let a = angle_deg.to_radians();
    [a.cos() * k, a.sin() * k]
}

/// Pixel a Mosaic block of `size` takes its value from (the block's centre pixel, clamped).
pub fn mosaic_source(x: u32, y: u32, size: u32, res: u32) -> (u32, u32) {
    let size = size.max(1);
    let at = |v: u32| (v / size * size + size / 2).min(res.saturating_sub(1));
    (at(x), at(y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_detect_is_zero_on_flat_and_one_on_a_unit_step() {
        assert_eq!(edge_detect(|_, _| 0.7, 1.0, 1.0), 0.0);
        let step = |dx: i32, _dy: i32| if dx > 0 { 1.0 } else { 0.0 };
        // Texel just left of the step: right taps see 1, left taps see 0.
        assert!((edge_detect(&step, 1.0, 1.0) - 1.0).abs() < 1e-6);
        assert!((edge_detect(&step, 1.0, 0.5) - 0.5).abs() < 1e-6);
        // Horizontal ramp of slope 0.1 per pixel: Sobel gx = 8 * 0.1 / 4 * ... = 0.2 per tap of 1 px.
        let ramp = |dx: i32, _dy: i32| 0.1 * dx as f32;
        assert!((edge_detect(&ramp, 1.0, 1.0) - 0.2).abs() < 1e-6);
        // Vertical edges are detected too, and tap distance scales the response.
        let vramp = |_dx: i32, dy: i32| 0.1 * dy as f32;
        assert!((edge_detect(&vramp, 2.0, 1.0) - 0.4).abs() < 1e-6);
    }

    #[test]
    fn directional_offset_follows_value_and_angle() {
        let c = ChannelKind::Roughness;
        assert_eq!(directional_offset(0.5, 10.0, 0.0, c), [0.0, 0.0]);
        let o = directional_offset(1.0, 10.0, 0.0, c);
        assert!((o[0] - 10.0).abs() < 1e-5 && o[1].abs() < 1e-5);
        let o = directional_offset(0.0, 10.0, 90.0, c);
        assert!(o[0].abs() < 1e-5 && (o[1] + 10.0).abs() < 1e-5);
        // Signed Height uses the value directly.
        let o = directional_offset(0.5, 10.0, 0.0, ChannelKind::Height);
        assert!((o[0] - 5.0).abs() < 1e-5);
        let o = directional_offset(7.0, 10.0, 0.0, c);
        assert!((o[0] - 10.0).abs() < 1e-5, "values above 1 are clamped to the declared reach");
    }

    #[test]
    fn mosaic_blocks_share_their_centre_pixel() {
        assert_eq!(mosaic_source(0, 0, 8, 64), (4, 4));
        assert_eq!(mosaic_source(7, 3, 8, 64), (4, 4));
        assert_eq!(mosaic_source(8, 9, 8, 64), (12, 12));
        assert_eq!(mosaic_source(62, 62, 8, 64), (60, 60));
        assert_eq!(mosaic_source(58, 58, 8, 60), (59, 59), "a centre past the texture edge clamps");
        assert_eq!(mosaic_source(5, 6, 1, 64), (5, 6));
    }
}
