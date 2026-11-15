//! Brush-panel preview of the active tip: one large stamp and an example stroke, drawn with the
//! same tip footprint the painting kernel uses ([`crate::stamp::tip_footprint`]).

use glam::Vec2;
use subcraft_image::Bitmap;

use crate::settings::BrushSettings;
use crate::stamp::tip_footprint;

/// Background and full-coverage gray levels of the preview (dark panel, white paint).
pub const BACKGROUND: u8 = 30;
pub const PAINT: u8 = 235;

/// Stamp centres and radii for a `width` × `height` preview: one stamp filling the left square,
/// then a wavy stroke at the brush spacing.
fn layout(width: u32, height: u32, spacing: f32) -> Vec<(Vec2, f32)> {
    let tile = height as f32;
    let mut out = vec![(Vec2::splat(tile * 0.5), tile * 0.42)];
    let r = tile * 0.24;
    let (x0, x1) = (tile + r, width as f32 - r);
    if x1 <= x0 {
        return out;
    }
    let step = (2.0 * r * spacing.max(0.05)).max(1.0);
    let mut x = x0;
    while x <= x1 + 0.1 {
        let t = (x - x0) / (x1 - x0);
        out.push((Vec2::new(x, tile * (0.5 + 0.2 * (t * std::f32::consts::TAU).sin())), r));
        x += step;
    }
    out
}

/// RGBA8 preview: gray paint coverage over a dark background. `alpha` is the tip image (`None`
/// for the round tip); size jitter and pressure are not shown.
pub fn render_tip_preview(alpha: Option<&Bitmap>, settings: &BrushSettings, width: u32, height: u32) -> Vec<u8> {
    draw(alpha, settings, width, height, &layout(width, height, settings.spacing), true)
}

/// Stamp centres every brush spacing along the polyline `path` (preview pixels), radius `r`.
pub fn stamps_along(path: &[Vec2], r: f32, spacing: f32) -> Vec<(Vec2, f32)> {
    let Some(&first) = path.first() else { return Vec::new() };
    let step = (2.0 * r * spacing.max(0.05)).max(1.0);
    let mut out = vec![(first, r)];
    let mut carried = 0.0f32;
    for w in path.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = (b - a).length();
        if len <= 0.0 {
            continue;
        }
        let mut t = step - carried;
        while t <= len {
            out.push((a + (b - a) * (t / len), r));
            t += step;
        }
        carried = len - (t - step);
    }
    out
}

/// Painter's custom test stroke: the brush drawn along `path` (preview pixels, as the user
/// dragged it over the preview), building up with the flow like a real stroke.
pub fn render_tip_stroke(alpha: Option<&Bitmap>, settings: &BrushSettings, width: u32, height: u32, path: &[Vec2]) -> Vec<u8> {
    let r = height as f32 * 0.24;
    draw(alpha, settings, width, height, &stamps_along(path, r, settings.spacing), false)
}

/// Gray coverage of `centers`' stamps. `single_first`: the first one is the tip on its own (full
/// strength), the rest build up with the flow.
fn draw(alpha: Option<&Bitmap>, settings: &BrushSettings, width: u32, height: u32, centers: &[(Vec2, f32)], single_first: bool) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let angle = settings.angle_deg.to_radians();
    let flow = settings.flow.clamp(0.05, 1.0);
    let reach = crate::stamp::reach(alpha.is_some());
    let mut out = vec![0u8; w * h * 4];
    for py in 0..h {
        for px in 0..w {
            let p = Vec2::new(px as f32 + 0.5, py as f32 + 0.5);
            let mut cover = 0.0f32;
            for (i, &(c, r)) in centers.iter().enumerate() {
                let d = (p - c) / r;
                if d.length() >= reach {
                    continue;
                }
                let a = tip_footprint(alpha, settings.hardness, angle, settings.roundness, d);
                // The single stamp shows the tip itself; the stroke builds up with the flow.
                let single = single_first && i == 0;
                let a = if single { a } else { a * flow };
                cover = if single { a } else { cover + a * (1.0 - cover) };
            }
            let v = (f32::from(BACKGROUND) + (f32::from(PAINT) - f32::from(BACKGROUND)) * cover.clamp(0.0, 1.0)).round() as u8;
            out[(py * w + px) * 4..][..4].copy_from_slice(&[v, v, v, 255]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gray(out: &[u8], w: u32, x: u32, y: u32) -> u8 {
        out[((y * w + x) * 4) as usize]
    }

    #[test]
    fn round_tip_preview_is_painted_at_the_centre_and_empty_at_the_corner() {
        let s = BrushSettings { hardness: 1.0, ..BrushSettings::default() };
        let out = render_tip_preview(None, &s, 264, 88);
        assert_eq!(out.len(), 264 * 88 * 4);
        assert_eq!(gray(&out, 264, 44, 44), PAINT, "centre of the single stamp is full coverage");
        assert_eq!(gray(&out, 264, 1, 1), BACKGROUND, "corner outside the round tip");
        // The stroke part has paint too.
        assert!((100..264).any(|x| (0..88).any(|y| gray(&out, 264, x, y) > BACKGROUND + 50)));
    }

    #[test]
    fn alpha_tip_preview_follows_the_image() {
        // Left half white, right half black: the single stamp shows the same split.
        let alpha = Bitmap::from_fn_gray(16, 16, |u, _| if u < 0.5 { 1.0 } else { 0.0 });
        let out = render_tip_preview(Some(&alpha), &BrushSettings { angle_deg: 0.0, roundness: 1.0, ..BrushSettings::default() }, 264, 88);
        assert!(gray(&out, 264, 30, 44) > 200, "left of the centre is painted");
        assert_eq!(gray(&out, 264, 60, 44), BACKGROUND, "right of the centre is not");
    }

    #[test]
    fn a_custom_stroke_paints_along_the_dragged_path() {
        let s = BrushSettings { hardness: 1.0, flow: 1.0, spacing: 0.1, ..BrushSettings::default() };
        // A diagonal drag from the top left to the bottom right.
        let path = [Vec2::new(30.0, 20.0), Vec2::new(230.0, 70.0)];
        let out = render_tip_stroke(None, &s, 264, 88, &path);
        assert!(gray(&out, 264, 130, 45) > BACKGROUND + 100, "painted on the path");
        assert_eq!(gray(&out, 264, 230, 10), BACKGROUND, "nothing off the path");
        assert_ne!(out, render_tip_preview(None, &s, 264, 88), "not the default example");
        // Stamps every spacing along the polyline, the first at its start.
        let stamps = stamps_along(&[Vec2::ZERO, Vec2::new(100.0, 0.0)], 10.0, 0.5);
        assert_eq!(stamps[0].0, Vec2::ZERO);
        assert_eq!(stamps.len(), 11, "every 10 px along 100 px: {stamps:?}");
        assert!(stamps_along(&[], 10.0, 0.5).is_empty());
    }

    #[test]
    fn a_different_tip_gives_a_different_preview() {
        let s = BrushSettings::default();
        let dots = Bitmap::from_fn_gray(16, 16, |u, v| if (u * 4.0) as u32 % 2 == (v * 4.0) as u32 % 2 { 1.0 } else { 0.0 });
        assert_ne!(render_tip_preview(None, &s, 132, 44), render_tip_preview(Some(&dots), &s, 132, 44));
    }
}
