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

/// Directional Distance: the brightest value within `distance` pixels behind the texel along
/// `angle_deg` (0 = right, counter-clockwise on screen), each faded by how far away it is;
/// Contrast steepens the fade about its middle. `read(dx, dy)` is relative to the texel. The
/// texel's own coverage is kept.
pub fn directional_distance(read: impl Fn(i32, i32) -> [f32; 4], distance: f32, angle_deg: f32, contrast: f32, kind: ChannelKind) -> [f32; 4] {
    let here = read(0, 0);
    let a = angle_deg.to_radians();
    let (dx, dy) = (a.cos(), -a.sin());
    let mut best = scalar(here, kind);
    for step in 1..=distance.ceil().max(0.0) as i32 {
        let s = step as f32;
        if s >= distance { break; }
        let fade = (((1.0 - s / distance) - 0.5) / (1.0 - contrast).max(1e-3) + 0.5).clamp(0.0, 1.0);
        if fade <= 0.0 { break; }
        let p = read((-dx * s).round() as i32, (-dy * s).round() as i32);
        best = best.max(scalar(p, kind) * p[3].min(1.0) * fade);
    }
    [best, best, best, here[3]]
}

/// Blur Slope with a mode. The reads go evenly from the texel to `offset` (the noise sources,
/// as the plain filter does) or, `two_sided`, from `-offset` to `offset` through the texel (the
/// line across the nearest edge of the image itself, see [`slope_offset`]).
///
/// Blur averages `samples` reads of `sample(dx, dy)`, the premultiplied bilinear read at an
/// offset from the texel. Min and Max keep the texel with the lowest or highest value times
/// coverage among every texel the line touches: at each stop, one per pixel of the way at
/// least, the texels a bilinear read there would mix (`texel(dx, dy)`, straight alpha, whole
/// offsets). Whole texels, because a bilinear read of a thin line returns a different mix at
/// every phase, which showed as ribs along widened lines. The result is straight alpha.
pub fn slope_blur(
    sample: impl Fn(f32, f32) -> [f32; 4],
    texel: impl Fn(i32, i32) -> [f32; 4],
    offset: [f32; 2],
    two_sided: bool,
    samples: u32,
    mode: subcraft_doc::SlopeMode,
    kind: ChannelKind,
) -> [f32; 4] {
    use subcraft_doc::SlopeMode;
    let [dx, dy] = offset;
    let length = (dx * dx + dy * dy).sqrt() * if two_sided { 2.0 } else { 1.0 };
    let samples = if mode == SlopeMode::Blur { samples.max(2) } else { samples.max(length.ceil() as u32 + 1) };
    // An odd count on a two-sided line, so the texel itself is one of the reads.
    let samples = if two_sided { samples | 1 } else { samples };
    let at = |k: u32| {
        let t = k as f32 / (samples - 1) as f32;
        let t = if two_sided { t * 2.0 - 1.0 } else { t };
        (dx * t, dy * t)
    };
    if mode == SlopeMode::Blur {
        let mut out = [0.0f32; 4];
        for k in 0..samples {
            let (x, y) = at(k);
            let s = sample(x, y);
            for c in 0..4 {
                out[c] += s[c] / samples as f32;
            }
        }
        if out[3] > 1e-12 {
            let a = out[3];
            for c in &mut out[..3] {
                *c /= a;
            }
        }
        return out;
    }
    let mut out = texel(0, 0);
    let mut best = scalar(out, kind) * out[3].min(1.0);
    for k in 0..samples {
        let (x, y) = at(k);
        let (x0, y0) = (x.floor(), y.floor());
        // Only the texels with a share in the bilinear read: one column or row when on it.
        let xs = [Some(x0 as i32), (x > x0).then_some(x0 as i32 + 1)];
        let ys = [Some(y0 as i32), (y > y0).then_some(y0 as i32 + 1)];
        for ty in ys.into_iter().flatten() {
            for tx in xs.into_iter().flatten() {
                let p = texel(tx, ty);
                let key = scalar(p, kind) * p[3].min(1.0);
                if (mode == SlopeMode::Min && key < best) || (mode == SlopeMode::Max && key > best) {
                    best = key;
                    out = p;
                }
            }
        }
    }
    out
}

/// Far end of the line a Blur Slope on Previous input reads along, from the gradient `g`
/// (value change per pixel) of the smoothed image at the texel: along the gradient, `intensity`
/// pixels long where the value changes by a quarter or more over that distance, shorter on
/// fainter slopes, nothing on a flat area.
pub fn slope_offset(g: [f32; 2], intensity: f32) -> [f32; 2] {
    let len = (g[0] * g[0] + g[1] * g[1]).sqrt();
    if len < 1e-7 || intensity <= 0.0 {
        return [0.0; 2];
    }
    let reach = intensity * (len * intensity * 4.0).min(1.0);
    [g[0] / len * reach, g[1] / len * reach]
}

/// Radius map of a Non-uniform Blur at the texel centre (x, y): 0..=1. `value` is the texel's
/// own value (luminance on colour, the magnitude for signed Height), `image` the map image's.
pub fn blur_map(map: subcraft_doc::BlurMap, value: f32, image: Option<f32>, x: u32, y: u32, res: u32, scale: u32, seed: u32) -> f32 {
    use subcraft_doc::BlurMap;
    match map {
        BlurMap::Value => value.abs().clamp(0.0, 1.0),
        BlurMap::InvertedValue => 1.0 - value.abs().clamp(0.0, 1.0),
        // The image's value at the texel (read by the compositor); nothing without an image.
        BlurMap::Image => image.unwrap_or(0.0).clamp(0.0, 1.0),
        BlurMap::Noise => {
            let p = (glam::Vec2::new(x as f32, y as f32) + 0.5) / res.max(1) as f32 * scale as f32;
            // Stretched so the noise reaches both a sharp 0 and a full 1.
            ((crate::noise::fbm(p, scale.max(1) as i32, 2, seed) - 0.5) * 2.5 + 0.5).clamp(0.0, 1.0)
        }
    }
}

/// Gaussian-weighted average of `samples` reads on a golden-angle spiral of radius `radius`
/// pixels around the texel (sigma = radius / 2), the texel itself included. `sample(dx, dy)` is
/// the premultiplied, bilinear read; the result is straight alpha. Under half a pixel of radius
/// the texel is returned as it is.
pub fn disc_blur(sample: impl Fn(f32, f32) -> [f32; 4], radius: f32, samples: u32) -> [f32; 4] {
    let mut out = sample(0.0, 0.0);
    if radius >= 0.5 {
        let mut total = 1.0f32;
        let n = samples.max(1);
        for k in 0..n {
            // Equal-area rings: the radius grows with the square root of the index.
            let t = ((k as f32 + 0.5) / n as f32).sqrt();
            let a = k as f32 * 2.399_963_2;
            let w = (-2.0 * t * t).exp();
            let s = sample(a.cos() * t * radius, a.sin() * t * radius);
            for c in 0..4 {
                out[c] += s[c] * w;
            }
            total += w;
        }
        for c in &mut out {
            *c /= total;
        }
    }
    if out[3] > 1e-12 {
        let a = out[3];
        for c in &mut out[..3] {
            *c /= a;
        }
    }
    out
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

    #[test]
    fn slope_blur_and_disc_blur_kernels() {
        use subcraft_doc::SlopeMode;
        let k = ChannelKind::Roughness;
        // A vertical step at x = 3.5 seen from the texel (bright to the right), full coverage.
        let step_at = |dx: f32| ((dx - 3.0).clamp(0.0, 1.0)) * 0.8 + 0.1;
        let step = |dx: f32, _dy: f32| { let v = step_at(dx); [v, v, v, 1.0] };
        let step_px = |dx: i32, _dy: i32| { let v = step_at(dx as f32); [v, v, v, 1.0] };
        let flat = |_: f32, _: f32| [0.3, 0.3, 0.3, 1.0];
        let flat_px = |_: i32, _: i32| [0.3, 0.3, 0.3, 1.0];
        for mode in SlopeMode::ALL {
            for two_sided in [false, true] {
                // Flat input, or no offset: the texel itself comes back.
                let n = slope_blur(flat, flat_px, [5.0, -3.0], two_sided, 8, mode, k);
                assert!((n[0] - 0.3).abs() < 1e-6 && (n[3] - 1.0).abs() < 1e-6);
                assert!((slope_blur(step, step_px, [0.0; 2], two_sided, 8, mode, k)[0] - 0.1).abs() < 1e-6);
            }
        }
        // The edge is within reach: Max takes the bright side, Min keeps the dark texel, Blur
        // lands between. Two-sided reads both ways, so the direction's sign does not matter.
        for off in [[6.0, 0.0], [-6.0, 0.0]] {
            let run = |mode| slope_blur(step, step_px, off, true, 12, mode, k)[0];
            let (lo, mid, hi) = (run(SlopeMode::Min), run(SlopeMode::Blur), run(SlopeMode::Max));
            assert!((lo - 0.1).abs() < 1e-6 && (hi - 0.9).abs() < 1e-6 && mid > 0.15 && mid < 0.85, "{lo} {mid} {hi}");
        }
        // Out of reach: untouched.
        assert!((slope_blur(step, step_px, [2.0, 0.0], true, 12, SlopeMode::Max, k)[0] - 0.1).abs() < 1e-6);
        // One-sided (noise source): only toward the offset.
        assert!((slope_blur(step, step_px, [6.0, 0.0], false, 7, SlopeMode::Max, k)[0] - 0.9).abs() < 1e-6);
        assert!((slope_blur(step, step_px, [-6.0, 0.0], false, 7, SlopeMode::Max, k)[0] - 0.1).abs() < 1e-6);
        // Min and Max return the exact value of a one-pixel diagonal line wherever the read
        // line crosses it, at any slant and phase and at the lowest Quality: no ribs.
        let diagonal = |dx: i32, dy: i32| { let v = if dx + dy == 7 { 0.0 } else { 1.0 }; [v, v, v, 1.0] };
        let soft = |dx: f32, dy: f32| { let v = if (dx + dy - 7.0).abs() < 0.5 { 0.0 } else { 1.0 }; [v, v, v, 1.0] };
        for i in 0..40 {
            let a = i as f32 * 0.039;
            let off = [a.cos() * 12.0, a.sin() * 12.0];
            assert_eq!(slope_blur(soft, diagonal, off, false, 2, SlopeMode::Min, k)[0], 0.0, "slant {a}");
        }
        // Coverage: a transparent neighbour does not darken the colour (premultiplied average),
        // and Max never picks an uncovered texel over a covered one.
        let half = |dx: f32, _dy: f32| if dx > 0.5 { [0.0; 4] } else { [0.8, 0.8, 0.8, 1.0] };
        let half_px = |dx: i32, _dy: i32| if dx > 0 { [0.9, 0.9, 0.9, 0.0] } else { [0.8, 0.8, 0.8, 1.0] };
        let b = slope_blur(half, half_px, [4.0, 0.0], false, 5, SlopeMode::Blur, k);
        assert!((b[0] - 0.8).abs() < 1e-5 && b[3] > 0.1 && b[3] < 0.9, "{b:?}");
        assert_eq!(slope_blur(half, half_px, [4.0, 0.0], false, 5, SlopeMode::Max, k), [0.8, 0.8, 0.8, 1.0]);
        // The line follows the gradient: full length on a clear slope, shorter on a faint one,
        // none on a flat area.
        assert_eq!(slope_offset([0.0; 2], 8.0), [0.0; 2]);
        let o = slope_offset([0.03, 0.04], 8.0);
        assert!((o[0] - 4.8).abs() < 1e-4 && (o[1] - 6.4).abs() < 1e-4, "{o:?}");
        let faint = slope_offset([0.001, 0.0], 8.0);
        assert!(faint[0] > 0.0 && faint[0] < 0.5 && faint[1] == 0.0, "{faint:?}");

        // Disc blur: identity under half a pixel, flat stays flat, a step is softened, more
        // radius softens more, and coverage is handled premultiplied.
        assert_eq!(disc_blur(step, 0.4, 32), step(0.0, 0.0));
        let d = disc_blur(flat, 9.0, 32);
        assert!((d[0] - 0.3).abs() < 1e-6 && (d[3] - 1.0).abs() < 1e-6);
        let edge = |dx: f32, _dy: f32| { let v = if dx > 0.0 { 1.0 } else { 0.0 }; [v, v, v, 1.0] };
        let (near, far) = (disc_blur(|x, y| edge(x - 2.0, y), 4.0, 48)[0], disc_blur(|x, y| edge(x - 2.0, y), 12.0, 48)[0]);
        assert!(near > 0.0 && near < far && far < 0.5, "{near} {far}");
        let c = disc_blur(half, 6.0, 48);
        assert!((c[0] - 0.8).abs() < 1e-5 && c[3] > 0.3 && c[3] < 0.8, "{c:?}");
        // The map: value, inverse (magnitude for signed Height), and a noise that tiles and
        // covers the range.
        use subcraft_doc::BlurMap;
        assert_eq!(blur_map(BlurMap::Value, 0.3, None, 0, 0, 64, 4, 1), 0.3);
        assert_eq!(blur_map(BlurMap::InvertedValue, 0.3, None, 0, 0, 64, 4, 1), 0.7);
        assert_eq!(blur_map(BlurMap::Value, -0.6, None, 0, 0, 64, 4, 1), 0.6);
        assert_eq!(blur_map(BlurMap::Value, 3.0, None, 0, 0, 64, 4, 1), 1.0);
        let (mut lo, mut hi) = (1.0f32, 0.0f32);
        for y in 0..64 { for x in 0..64 { let m = blur_map(BlurMap::Noise, 0.0, None, x, y, 64, 4, 1); assert!((0.0..=1.0).contains(&m)); lo = lo.min(m); hi = hi.max(m); } }
        assert!(lo < 0.15 && hi > 0.85, "{lo} {hi}");
        assert_ne!(blur_map(BlurMap::Noise, 0.0, None, 9, 9, 64, 4, 1), blur_map(BlurMap::Noise, 0.0, None, 9, 9, 64, 4, 2));
        // The image map: its value, or no blur while no image is picked.
        assert_eq!(blur_map(BlurMap::Image, 0.9, Some(0.25), 0, 0, 64, 4, 1), 0.25);
        assert_eq!(blur_map(BlurMap::Image, 0.9, None, 0, 0, 64, 4, 1), 0.0);
    }

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

/// FXAA (a compact variant of the edge-directed anti-aliasing filter): where the luminance
/// contrast of a texel against its four neighbours is high enough to be an edge, the texel is
/// blended toward its neighbour across the edge, more where it stands out from the neighbours'
/// mean (a one-texel step) and less on wide gradients. `read(dx, dy)` returns straight texels;
/// `alpha` also blends the coverage (else the texel keeps its own).
pub fn fxaa(read: impl Fn(i32, i32) -> [f32; 4], alpha: bool, kind: ChannelKind) -> [f32; 4] {
    let luma = |p: [f32; 4]| if kind.is_color() { p[0] * 0.2126 + p[1] * 0.7152 + p[2] * 0.0722 } else { p[0] };
    let c = read(0, 0);
    let (n, s, w, e) = (read(0, -1), read(0, 1), read(-1, 0), read(1, 0));
    let (lc, ln, ls, lw, le) = (luma(c), luma(n), luma(s), luma(w), luma(e));
    let hi = lc.max(ln).max(ls).max(lw).max(le);
    let lo = lc.min(ln).min(ls).min(lw).min(le);
    let range = hi - lo;
    // Edge thresholds of the reference implementation (relative 1/8, absolute 1/32).
    if range < (hi * 0.125).max(0.0312) {
        return c;
    }
    // A horizontal edge (luminance changes vertically) blends with the texel above or below.
    let horizontal = (ln + ls - 2.0 * lc).abs() >= (lw + le - 2.0 * lc).abs();
    let (a, b, la, lb) = if horizontal { (n, s, ln, ls) } else { (w, e, lw, le) };
    let across = if (la - lc).abs() >= (lb - lc).abs() { a } else { b };
    let sub = (((ln + ls + lw + le) * 0.25 - lc).abs() / range).clamp(0.0, 1.0);
    let t = (sub * sub * (3.0 - 2.0 * sub)).powi(2) * 0.75;
    let mut out = c;
    for i in 0..3 {
        out[i] = c[i] + (across[i] - c[i]) * t;
    }
    if alpha {
        out[3] = c[3] + (across[3] - c[3]) * t;
    }
    out
}

#[cfg(test)]
mod fxaa_tests {
    use super::*;

    #[test]
    fn fxaa_softens_a_one_texel_step_and_leaves_flat_areas_and_gradients() {
        // A lone bright texel in a dark field: blended toward a neighbour by 0.75.
        let lone = |dx: i32, dy: i32| if dx == 0 && dy == 0 { [1.0, 1.0, 1.0, 1.0] } else { [0.0, 0.0, 0.0, 1.0] };
        let v = fxaa(lone, false, ChannelKind::Roughness);
        assert!((v[0] - 0.25).abs() < 1e-6, "{v:?}");
        // Flat: unchanged.
        let flat = |_: i32, _: i32| [0.4, 0.4, 0.4, 1.0];
        assert_eq!(fxaa(flat, false, ChannelKind::Roughness), [0.4, 0.4, 0.4, 1.0]);
        // A smooth ramp (the centre is its neighbours' mean): no blending.
        let ramp = |dx: i32, _: i32| { let v = 0.5 + 0.1 * dx as f32; [v, v, v, 1.0] };
        assert_eq!(fxaa(ramp, false, ChannelKind::Roughness)[0], 0.5);
        // A vertical edge one texel off the centre blends horizontally, toward the bright side.
        let edge = |dx: i32, _: i32| if dx > 0 { [1.0, 1.0, 1.0, 1.0] } else { [0.0, 0.0, 0.0, 1.0] };
        let v = fxaa(edge, false, ChannelKind::Roughness);
        assert!(v[0] > 0.0 && v[0] < 0.5, "{v:?}");
        // Coverage moves only with `alpha`.
        let holes = |dx: i32, dy: i32| if dx == 0 && dy == 0 { [1.0, 1.0, 1.0, 1.0] } else { [0.0, 0.0, 0.0, 0.0] };
        assert_eq!(fxaa(holes, false, ChannelKind::Roughness)[3], 1.0);
        assert!((fxaa(holes, true, ChannelKind::Roughness)[3] - 0.25).abs() < 1e-6);
    }

    #[test]
    fn directional_distance_trails_bright_values_along_the_angle() {
        // One bright texel at x = 10 on a dark row.
        let at = |x: i32, angle: f32, distance: f32, contrast: f32| {
            let read = |dx: i32, dy: i32| if x + dx == 10 && dy == 0 { [1.0, 1.0, 1.0, 1.0] } else { [0.0, 0.0, 0.0, 1.0] };
            directional_distance(read, distance, angle, contrast, ChannelKind::Roughness)[0]
        };
        // Angle 0 spreads to the right: 4 texels on of 8 is half.
        assert_eq!(at(10, 0.0, 8.0, 0.0), 1.0);
        assert!((at(14, 0.0, 8.0, 0.0) - 0.5).abs() < 1e-6);
        assert_eq!(at(18, 0.0, 8.0, 0.0), 0.0);
        assert_eq!(at(6, 0.0, 8.0, 0.0), 0.0, "nothing behind the source");
        // 180 degrees spreads to the left.
        assert!((at(6, 180.0, 8.0, 0.0) - 0.5).abs() < 1e-5);
        // Contrast 0.5 doubles the fade's slope about its middle: 0.75 becomes 1, 0.25 becomes 0.
        assert_eq!(at(12, 0.0, 8.0, 0.5), 1.0);
        assert_eq!(at(16, 0.0, 8.0, 0.5), 0.0);
        // No distance: unchanged.
        assert_eq!(at(11, 0.0, 0.0, 0.0), 0.0);
    }
}

/// Settings of the Anisotropic Kuwahara filter (see `subcraft_doc::Filter::AnisotropicKuwahara`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Kuwahara {
    pub radius: f32,
    pub smoothness: f32,
    pub sharpness: f32,
    pub tensor_smoothness: f32,
    pub anisotropy: f32,
    /// Fixed direction in turns, used when `extract_direction` is off.
    pub angle: f32,
    pub extract_direction: bool,
}

/// Samples across the ellipse's widest axis at most (about 17 x 17 reads per texel).
const KUWAHARA_GRID: f32 = 8.0;
const KUWAHARA_SECTORS: usize = 8;

/// Direction (radians, along the edge) and anisotropy (0 round .. 1 a clean edge) at the texel,
/// from a structure tensor of Sobel gradients averaged over a 3 x 3 pattern `spread` pixels apart.
fn kuwahara_orientation(read: &impl Fn(i32, i32) -> [f32; 4], spread: f32, kind: ChannelKind) -> (f32, f32) {
    let v = |x: i32, y: i32| scalar(read(x, y), kind);
    let step = spread.round().max(0.0) as i32;
    let (mut e, mut f, mut g) = (0.0f32, 0.0f32, 0.0f32);
    for oy in -1..=1 {
        for ox in -1..=1 {
            if step == 0 && (ox != 0 || oy != 0) {
                continue;
            }
            let (cx, cy) = (ox * step, oy * step);
            let w = if ox == 0 && oy == 0 { 4.0 } else if ox == 0 || oy == 0 { 2.0 } else { 1.0 };
            let gx = (v(cx + 1, cy - 1) + 2.0 * v(cx + 1, cy) + v(cx + 1, cy + 1)) - (v(cx - 1, cy - 1) + 2.0 * v(cx - 1, cy) + v(cx - 1, cy + 1));
            let gy = (v(cx - 1, cy + 1) + 2.0 * v(cx, cy + 1) + v(cx + 1, cy + 1)) - (v(cx - 1, cy - 1) + 2.0 * v(cx, cy - 1) + v(cx + 1, cy - 1));
            e += w * gx * gx;
            f += w * gx * gy;
            g += w * gy * gy;
        }
    }
    let root = ((e - g) * (e - g) + 4.0 * f * f).sqrt();
    let (l1, l2) = ((e + g + root) * 0.5, (e + g - root) * 0.5);
    if l1 + l2 <= 1e-12 {
        return (0.0, 0.0);
    }
    // The eigenvector of the smaller eigenvalue runs along the edge.
    let (tx, ty) = (f, l2 - e);
    let angle = if tx.abs() + ty.abs() > 1e-12 { ty.atan2(tx) } else if e >= g { std::f32::consts::FRAC_PI_2 } else { 0.0 };
    (angle, ((l1 - l2) / (l1 + l2)).clamp(0.0, 1.0))
}

/// `c ^ power` for the sector lobes. The exponent is fixed per filter, and a whole or half
/// exponent (every Smoothness the presets use) needs only multiplications and a square root.
#[derive(Clone, Copy)]
enum Lobe {
    Halves { whole: u32, half: bool },
    Any(f32),
}

impl Lobe {
    fn new(power: f32) -> Self {
        let doubled = power * 2.0;
        if doubled.fract() == 0.0 && (2.0..=16.0).contains(&doubled) {
            Self::Halves { whole: doubled as u32 / 2, half: doubled as u32 % 2 == 1 }
        } else {
            Self::Any(power)
        }
    }

    #[inline]
    fn raise(self, c: f32) -> f32 {
        match self {
            Self::Halves { whole, half } => {
                let mut out = if half { c.sqrt() } else { 1.0 };
                for _ in 0..whole {
                    out *= c;
                }
                out
            }
            Self::Any(power) => c.powf(power),
        }
    }
}

/// Anisotropic Kuwahara at one texel (after Kyprianidis et al. 2009): the texels of an ellipse
/// stretched along the local edge are split into 8 overlapping sectors, and the result is the
/// sector means weighted by how calm each sector is (`1 / (1 + sigma^sharpness)`, sigma in
/// 8-bit steps). Edges stay sharp while flat areas smooth into painterly patches. `read(dx, dy)`
/// is relative to the texel; the texel keeps its coverage.
///
/// The cosine of a sample's angle to a sector axis is its unit direction dotted with the axis,
/// so the loop needs no `atan2` or `cos`, and the lobe power is usually a few multiplications
/// (see [`Lobe`]). The result matches the straightforward form to float rounding.
pub fn kuwahara(read: impl Fn(i32, i32) -> [f32; 4], k: Kuwahara, kind: ChannelKind) -> [f32; 4] {
    let here = read(0, 0);
    let radius = if k.radius.is_finite() { k.radius.clamp(0.0, 32.0) } else { 0.0 };
    if radius < 0.5 {
        return here;
    }
    let (phi, edge) = if k.extract_direction { kuwahara_orientation(&read, k.tensor_smoothness.clamp(0.0, 8.0), kind) } else { (k.angle * std::f32::consts::TAU, 1.0) };
    let stretch = 1.0 + k.anisotropy.clamp(0.0, 4.0) * edge;
    let (a, b) = (radius * stretch, radius / stretch);
    let (sin, cos) = phi.sin_cos();
    let extent = a.max(b).ceil();
    let step = (extent / KUWAHARA_GRID).max(1.0);
    // Sector lobes: cos^p of the angle to the sector axis; Smoothness widens them.
    let lobe = Lobe::new(1.0 + (1.0 - k.smoothness.clamp(0.0, 1.0)) * 7.0);
    let mut axes = [(0.0f32, 0.0f32); KUWAHARA_SECTORS];
    for (s, axis) in axes.iter_mut().enumerate() {
        let (sn, cs) = (s as f32 * std::f32::consts::TAU / KUWAHARA_SECTORS as f32).sin_cos();
        *axis = (cs, sn);
    }
    let mut sum = [[0.0f32; 3]; KUWAHARA_SECTORS];
    let mut sq = [[0.0f32; 3]; KUWAHARA_SECTORS];
    let mut weight = [0.0f32; KUWAHARA_SECTORS];
    let n = (extent / step).ceil() as i32;
    for iy in -n..=n {
        for ix in -n..=n {
            let (dx, dy) = (ix as f32 * step, iy as f32 * step);
            // Into the ellipse frame: u along the edge, v across it.
            // Divided exactly as before: a sample on the rim (d2 = 1) must stay on the same side.
            let (u, v) = ((dx * cos + dy * sin) / a, (-dx * sin + dy * cos) / b);
            let d2 = u * u + v * v;
            if d2 > 1.0 {
                continue;
            }
            let p = read(dx.round() as i32, dy.round() as i32);
            let g = (-2.0 * d2).exp();
            let squares = [p[0] * p[0], p[1] * p[1], p[2] * p[2]];
            let centre = d2 < 1e-6;
            let inv = if centre { 0.0 } else { 1.0 / d2.sqrt() };
            let (cu, cv) = (u * inv, v * inv);
            for s in 0..KUWAHARA_SECTORS {
                let w = if centre {
                    g
                } else {
                    let c = cu * axes[s].0 + cv * axes[s].1;
                    if c <= 0.0 {
                        continue;
                    }
                    g * lobe.raise(c)
                };
                if w <= 0.0 {
                    continue;
                }
                weight[s] += w;
                for c in 0..3 {
                    sum[s][c] += w * p[c];
                    sq[s][c] += w * squares[c];
                }
            }
        }
    }
    let q = k.sharpness.clamp(0.0, 16.0);
    let (mut out, mut total) = ([0.0f32; 3], 0.0f32);
    for s in 0..KUWAHARA_SECTORS {
        if weight[s] <= 0.0 {
            continue;
        }
        let mean = sum[s].map(|v| v / weight[s]);
        let var: f32 = (0..3).map(|c| (sq[s][c] / weight[s] - mean[c] * mean[c]).max(0.0)).sum();
        let sigma = var.sqrt() * 255.0;
        let alpha = 1.0 / (1.0 + sigma.powf(q));
        total += alpha;
        for c in 0..3 {
            out[c] += alpha * mean[c];
        }
    }
    if total <= 0.0 {
        return here;
    }
    [out[0] / total, out[1] / total, out[2] / total, here[3]]
}

#[cfg(test)]
/// The straightforward form of [`kuwahara`] (an `atan2`, eight `cos` and eight `powf` per
/// sample), kept as the reference the fast form is tested against.
/// Anisotropic Kuwahara at one texel (after Kyprianidis et al. 2009): the texels of an ellipse
/// stretched along the local edge are split into 8 overlapping sectors, and the result is the
/// sector means weighted by how calm each sector is (`1 / (1 + sigma^sharpness)`, sigma in
/// 8-bit steps). Edges stay sharp while flat areas smooth into painterly patches. `read(dx, dy)`
/// is relative to the texel; the texel keeps its coverage.
fn kuwahara_reference(read: impl Fn(i32, i32) -> [f32; 4], k: Kuwahara, kind: ChannelKind) -> [f32; 4] {
    let here = read(0, 0);
    let radius = if k.radius.is_finite() { k.radius.clamp(0.0, 32.0) } else { 0.0 };
    if radius < 0.5 {
        return here;
    }
    let (phi, edge) = if k.extract_direction { kuwahara_orientation(&read, k.tensor_smoothness.clamp(0.0, 8.0), kind) } else { (k.angle * std::f32::consts::TAU, 1.0) };
    let stretch = 1.0 + k.anisotropy.clamp(0.0, 4.0) * edge;
    let (a, b) = (radius * stretch, radius / stretch);
    let (sin, cos) = phi.sin_cos();
    let extent = a.max(b).ceil();
    let step = (extent / KUWAHARA_GRID).max(1.0);
    // Sector lobes: cos^p of the angle to the sector axis; Smoothness widens them.
    let power = 1.0 + (1.0 - k.smoothness.clamp(0.0, 1.0)) * 7.0;
    let mut sum = [[0.0f32; 3]; KUWAHARA_SECTORS];
    let mut sq = [[0.0f32; 3]; KUWAHARA_SECTORS];
    let mut weight = [0.0f32; KUWAHARA_SECTORS];
    let n = (extent / step).ceil() as i32;
    for iy in -n..=n {
        for ix in -n..=n {
            let (dx, dy) = (ix as f32 * step, iy as f32 * step);
            // Into the ellipse frame: u along the edge, v across it.
            let (u, v) = ((dx * cos + dy * sin) / a, (-dx * sin + dy * cos) / b);
            let d2 = u * u + v * v;
            if d2 > 1.0 {
                continue;
            }
            let p = read(dx.round() as i32, dy.round() as i32);
            let g = (-2.0 * d2).exp();
            let theta = v.atan2(u);
            for s in 0..KUWAHARA_SECTORS {
                let w = if d2 < 1e-6 { g } else { g * (theta - s as f32 * std::f32::consts::TAU / KUWAHARA_SECTORS as f32).cos().max(0.0).powf(power) };
                if w <= 0.0 {
                    continue;
                }
                weight[s] += w;
                for c in 0..3 {
                    sum[s][c] += w * p[c];
                    sq[s][c] += w * p[c] * p[c];
                }
            }
        }
    }
    let q = k.sharpness.clamp(0.0, 16.0);
    let (mut out, mut total) = ([0.0f32; 3], 0.0f32);
    for s in 0..KUWAHARA_SECTORS {
        if weight[s] <= 0.0 {
            continue;
        }
        let mean = sum[s].map(|v| v / weight[s]);
        let var: f32 = (0..3).map(|c| (sq[s][c] / weight[s] - mean[c] * mean[c]).max(0.0)).sum();
        let sigma = var.sqrt() * 255.0;
        let alpha = 1.0 / (1.0 + sigma.powf(q));
        total += alpha;
        for c in 0..3 {
            out[c] += alpha * mean[c];
        }
    }
    if total <= 0.0 {
        return here;
    }
    [out[0] / total, out[1] / total, out[2] / total, here[3]]
}

#[cfg(test)]
mod kuwahara_tests {
    use super::*;

    fn settings(radius: f32) -> Kuwahara {
        Kuwahara { radius, smoothness: 0.5, sharpness: 8.0, tensor_smoothness: 2.0, anisotropy: 1.0, angle: 0.0, extract_direction: true }
    }

    /// A 64 x 64 image from `f(x, y)`, read around texel (cx, cy) with clamped edges.
    fn image(f: impl Fn(i32, i32) -> f32 + Copy, cx: i32, cy: i32) -> impl Fn(i32, i32) -> [f32; 4] {
        move |dx, dy| {
            let v = f((cx + dx).clamp(0, 63), (cy + dy).clamp(0, 63));
            [v, v, v, 1.0]
        }
    }

    #[test]
    fn the_fast_form_matches_the_reference() {
        // Random colour images (smooth blobs plus noise and a hard edge), many settings. The two
        // forms differ only by float rounding in the sector weights. Where a sector is almost
        // flat its variance is a difference of nearly equal numbers, and a high Sharpness
        // magnifies that rounding, so the largest difference is about one 16-bit step (1.5e-5),
        // 170 times smaller than one 8-bit step.
        let h = |x: i32, y: i32, seed: u32| crate::noise::hash_f(x, y, seed);
        let scene = move |x: i32, y: i32| {
            let blob = ((x as f32 * 0.21).sin() * (y as f32 * 0.17).cos()) * 0.3 + 0.5;
            let edge = if x + y / 3 > 40 { 0.25 } else { 0.0 };
            [blob + edge + 0.1 * h(x, y, 1), blob * 0.7 + 0.15 * h(x, y, 2), 0.9 - blob * 0.5 + edge * 0.5 + 0.1 * h(x / 3, y / 3, 3)]
        };
        let colour = move |cx: i32, cy: i32| {
            move |dx: i32, dy: i32| {
                let v = scene((cx + dx).clamp(0, 63), (cy + dy).clamp(0, 63));
                [v[0], v[1], v[2], 0.8]
            }
        };
        let mut worst = 0.0f32;
        let mut cases = 0;
        for radius in [0.6, 1.0, 3.0, 5.0, 6.0, 13.5, 32.0] {
            for (smoothness, sharpness) in [(0.5, 8.0), (1.0, 4.0), (0.0, 16.0), (0.37, 2.5), (0.86, 0.0)] {
                for (anisotropy, extract_direction, angle) in [(1.0, true, 0.0), (0.0, true, 0.0), (4.0, true, 0.0), (2.0, false, 0.13), (1.0, false, 0.75)] {
                    let k = Kuwahara { radius, smoothness, sharpness, tensor_smoothness: 2.0, anisotropy, angle, extract_direction };
                    for (x, y) in [(5, 7), (31, 30), (40, 12), (62, 63), (20, 44)] {
                        for kind in [ChannelKind::BaseColor, ChannelKind::Roughness] {
                            let (fast, slow) = (kuwahara(colour(x, y), k, kind), kuwahara_reference(colour(x, y), k, kind));
                            for c in 0..4 {
                                worst = worst.max((fast[c] - slow[c]).abs());
                            }
                            cases += 1;
                        }
                    }
                }
            }
        }
        assert!(cases == 1750 && worst < 4e-5, "{cases} cases, largest difference {worst}");
    }

    #[test]
    fn flat_areas_stay_and_edges_stay_sharp() {
        let k = settings(6.0);
        let out = kuwahara(image(|_x, _y| 0.4, 20, 20), k, ChannelKind::Roughness);
        assert!((out[0] - 0.4).abs() < 1e-5 && out[3] == 1.0, "{out:?}");
        // A vertical step edge: texels on each side keep their side (no blur across it).
        let step = |x: i32, _y: i32| if x < 32 { 0.1 } else { 0.9 };
        let left = kuwahara(image(step, 30, 20), k, ChannelKind::Roughness)[0];
        let right = kuwahara(image(step, 33, 20), k, ChannelKind::Roughness)[0];
        assert!((left - 0.1).abs() < 0.02 && (right - 0.9).abs() < 0.02, "edge kept: {left} | {right}");
        // Radius below half a texel: unchanged.
        assert_eq!(kuwahara(image(step, 31, 20), settings(0.2), ChannelKind::Roughness)[0], 0.1);
    }

    #[test]
    fn noise_is_smoothed_away_from_edges() {
        // A checker of +/-0.05 around 0.5: the filter pulls texels towards the local mean.
        let noise = |x: i32, y: i32| if (x + y) % 2 == 0 { 0.55 } else { 0.45 };
        let out = kuwahara(image(noise, 20, 21), settings(4.0), ChannelKind::Roughness)[0];
        assert!((out - 0.5).abs() < 0.03, "smoothed: {out}");
    }

    #[test]
    fn the_structure_tensor_finds_the_edge_direction() {
        // A vertical edge runs along y: the edge direction is vertical, fully anisotropic.
        let step = |x: i32, _y: i32| if x < 32 { 0.0 } else { 1.0 };
        let (angle, edge) = kuwahara_orientation(&image(step, 32, 20), 2.0, ChannelKind::Roughness);
        assert!(edge > 0.95, "a clean edge: {edge}");
        assert!((angle.sin().abs() - 1.0).abs() < 1e-3, "along y: {angle}");
        // Flat: no direction.
        assert_eq!(kuwahara_orientation(&image(|_, _| 0.3, 20, 20), 2.0, ChannelKind::Roughness), (0.0, 0.0));
    }
}
