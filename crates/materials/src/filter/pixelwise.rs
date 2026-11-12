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

/// Where a Transform filter reads the image for the output point `uv` (both in UV, top-left
/// origin). The image is scaled about its centre, rotated by `rotation_deg` (counter-clockwise on
/// screen) and moved by `offset`; flips mirror the image itself. `None` when the point falls
/// outside the image on an axis that does not repeat; a repeating axis wraps into 0..1.
///
/// `tile_safe` snaps the rotation to the nearest quarter turn and each scale to `1 / n` for a
/// whole `n >= 1` (whole repetitions), so a tiling image still tiles.
pub fn transform_source(uv: [f32; 2], scale: [f32; 2], offset: [f32; 2], rotation_deg: f32, flip: [bool; 2], repeat: [bool; 2], tile_safe: bool) -> Option<[f32; 2]> {
    let (scale, rotation_deg) = if tile_safe { (scale.map(tile_safe_scale), (rotation_deg / 90.0).round() * 90.0) } else { (scale, rotation_deg) };
    // Inverse of: p = R(theta) * (S * (q - c)) + c + offset, with y pointing down.
    let (dx, dy) = (uv[0] - 0.5 - offset[0], uv[1] - 0.5 - offset[1]);
    let quarter = (rotation_deg / 90.0).round();
    let (sin, cos) = if rotation_deg == quarter * 90.0 {
        // Exact quarter turns (no float residue to break tiling).
        [(0.0, 1.0), (1.0, 0.0), (0.0, -1.0), (-1.0, 0.0)][(quarter as i32).rem_euclid(4) as usize]
    } else { rotation_deg.to_radians().sin_cos() };
    // Counter-clockwise on screen (y down) is the matrix [[cos, sin], [-sin, cos]]; its inverse:
    let (rx, ry) = (dx * cos - dy * sin, dx * sin + dy * cos);
    let mut q = [rx / scale[0] + 0.5, ry / scale[1] + 0.5];
    for a in 0..2 {
        if repeat[a] { q[a] -= q[a].floor(); } else if !(0.0..=1.0).contains(&q[a]) { return None; }
        if flip[a] { q[a] = 1.0 - q[a]; }
    }
    Some(q)
}

/// The scale `1 / n` (whole `n >= 1` repetitions) nearest to `scale`.
pub fn tile_safe_scale(scale: f32) -> f32 {
    1.0 / (1.0 / scale.max(1e-3)).round().max(1.0)
}

/// Where a Mirror filter reads the image for the output point `uv`: a point on the kept side of
/// an axis reads itself, a point on the other side reads its reflection across the axis (clamped
/// to the image). `axes` says which axes mirror, `offset` is the axis position and `far` whether
/// the kept side is the far one (right / bottom).
pub fn mirror_source(uv: [f32; 2], axes: [bool; 2], offset: [f32; 2], far: [bool; 2]) -> [f32; 2] {
    std::array::from_fn(|a| {
        let kept = if far[a] { uv[a] >= offset[a] } else { uv[a] <= offset[a] };
        if !axes[a] || kept { uv[a] } else { (2.0 * offset[a] - uv[a]).clamp(0.0, 1.0) }
    })
}

/// Axes and kept sides of a Mirror filter's settings.
pub fn mirror_axes(mode: subcraft_doc::MirrorMode, invert_x: bool, invert_y: bool, corner: subcraft_doc::MirrorCorner) -> ([bool; 2], [bool; 2]) {
    use subcraft_doc::MirrorMode as M;
    let axes = match mode { M::AxisX => [true, false], M::AxisY => [false, true], M::Corner => [true, true] };
    let base = if mode == M::Corner { corner.far() } else { [false, false] };
    (axes, [base[0] ^ invert_x, base[1] ^ invert_y])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: Option<[f32; 2]>, b: [f32; 2]) {
        let a = a.expect("inside the image");
        assert!((a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5, "{a:?} vs {b:?}");
    }

    #[test]
    fn transform_maps_output_points_back_to_the_image() {
        let (no, all) = ([false, false], [true, true]);
        let t = |uv, scale, offset, rot| transform_source(uv, scale, offset, rot, no, all, false);
        // Identity.
        near(t([0.3, 0.8], [1.0, 1.0], [0.0, 0.0], 0.0), [0.3, 0.8]);
        // Offset moves the image right / down: the output reads from the left / above.
        near(t([0.3, 0.8], [1.0, 1.0], [0.1, 0.25], 0.0), [0.2, 0.55]);
        // Scale 2 about the centre: the output corner shows the image quarter point.
        near(t([0.0, 1.0], [2.0, 2.0], [0.0, 0.0], 0.0), [0.25, 0.75]);
        near(t([0.5, 0.5], [2.0, 0.5], [0.0, 0.0], 0.0), [0.5, 0.5]);
        // Scale 0.5 repeats: the output corner reads the wrapped centre.
        near(t([0.0, 0.0], [0.5, 0.5], [0.0, 0.0], 0.0), [0.5, 0.5]);
        // A quarter turn counter-clockwise on screen: the image point right of centre appears
        // above the centre, so the output point above the centre reads from the right.
        near(t([0.5, 0.25], [1.0, 1.0], [0.0, 0.0], 90.0), [0.75, 0.5]);
        near(t([0.25, 0.5], [1.0, 1.0], [0.0, 0.0], 90.0), [0.5, 0.25]);
        near(t([0.5, 0.25], [1.0, 1.0], [0.0, 0.0], 180.0), [0.5, 0.75]);
        // 45 degrees: the output point up-right of centre reads from straight right.
        let d = 0.2 / 2f32.sqrt();
        near(t([0.5 + d, 0.5 - d], [1.0, 1.0], [0.0, 0.0], 45.0), [0.7, 0.5]);
        // Flips mirror the image itself.
        near(transform_source([0.3, 0.8], [1.0; 2], [0.0; 2], 0.0, [true, false], all, false), [0.7, 0.8]);
        near(transform_source([0.3, 0.8], [1.0; 2], [0.0; 2], 0.0, [true, true], all, false), [0.7, 0.2]);
        // No repeat: outside the image there is nothing; one axis may still repeat.
        assert_eq!(transform_source([0.1, 0.5], [0.5; 2], [0.0; 2], 0.0, no, no, false), None);
        near(transform_source([0.3, 0.5], [0.5; 2], [0.0; 2], 0.0, no, no, false), [0.1, 0.5]);
        near(transform_source([0.1, 0.5], [0.5; 2], [0.0; 2], 0.0, no, [true, false], false), [0.7, 0.5]);
        assert_eq!(transform_source([0.5, 0.1], [0.5; 2], [0.0; 2], 0.0, no, [true, false], false), None);
        // Tile safe: 37 degrees snaps to 0, 50 to 90; a scale snaps to the nearest whole number
        // of repetitions (0.3 -> 1/3, 0.45 -> 1/2, anything above 1 -> 1).
        near(transform_source([0.3, 0.8], [1.0; 2], [0.0; 2], 37.0, no, all, true), [0.3, 0.8]);
        near(transform_source([0.5, 0.25], [1.0; 2], [0.0; 2], 50.0, no, all, true), [0.75, 0.5]);
        assert_eq!(tile_safe_scale(0.3), 1.0 / 3.0);
        assert_eq!(tile_safe_scale(0.45), 0.5);
        assert_eq!(tile_safe_scale(0.5), 0.5);
        assert_eq!(tile_safe_scale(3.0), 1.0);
        // With whole repetitions and quarter turns, opposite borders read the same image point.
        for v in [0.1, 0.37, 0.9] {
            let (a, b) = (transform_source([0.0, v], [0.4; 2], [0.0; 2], 80.0, no, all, true).unwrap(), transform_source([1.0, v], [0.4; 2], [0.0; 2], 80.0, no, all, true).unwrap());
            let wrap = |x: f32| (x - x.round()).abs();
            assert!(wrap(a[0] - b[0]) < 1e-4 && wrap(a[1] - b[1]) < 1e-4, "{a:?} {b:?}");
        }
    }

    #[test]
    fn mirror_reflects_the_far_side_onto_the_kept_side() {
        use subcraft_doc::{MirrorCorner as C, MirrorMode as M};
        let (axes, far) = mirror_axes(M::AxisX, false, false, C::BottomRight);
        assert_eq!((axes, far), ([true, false], [false, false]), "the corner only matters in Corner mode");
        // Left kept: the right side reads its reflection, v is untouched.
        assert_eq!(mirror_source([0.3, 0.9], axes, [0.5; 2], far), [0.3, 0.9]);
        assert_eq!(mirror_source([0.75, 0.9], axes, [0.5; 2], far), [0.25, 0.9]);
        // Inverted: the right side is kept.
        let (axes, far) = mirror_axes(M::AxisX, true, false, C::TopLeft);
        assert_eq!(mirror_source([0.25, 0.9], axes, [0.5; 2], far), [0.75, 0.9]);
        assert_eq!(mirror_source([0.75, 0.9], axes, [0.5; 2], far), [0.75, 0.9]);
        // Axis Y, off centre: reflections that leave the image clamp to its border.
        let (axes, far) = mirror_axes(M::AxisY, false, false, C::TopLeft);
        assert_eq!(mirror_source([0.1, 0.5], axes, [0.5, 0.25], far), [0.1, 0.0]);
        assert_eq!(mirror_source([0.1, 0.375], axes, [0.5, 0.25], far), [0.1, 0.125]);
        // Corner: the kept corner fills all four quadrants; inverting an axis swaps it.
        let (axes, far) = mirror_axes(M::Corner, false, false, C::TopRight);
        assert_eq!((axes, far), ([true, true], [true, false]));
        assert_eq!(mirror_source([0.25, 0.75], axes, [0.5; 2], far), [0.75, 0.25]);
        assert_eq!(mirror_axes(M::Corner, true, true, C::TopRight).1, [false, true]);
    }

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
