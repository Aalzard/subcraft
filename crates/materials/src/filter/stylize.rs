//! Stylization filters: MatFX Comic Book, MatFX Watercolor and MatFX Oil Paint strokes.
//! Pure functions over a pixel reader (`read(dx, dy)`, relative to the texel, straight alpha) so
//! the compositor and the tests share the same math. Original implementations of the looks:
//! none of them copies Painter's graphs.
use super::pixelwise::scalar;
use crate::noise::{fbm, hash2};
use glam::Vec2;
use subcraft_core::ChannelKind;

fn ramp(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a).max(1e-6)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn unit(h: u32) -> f32 {
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Settings of the Comic Book filter (see `subcraft_doc::Filter::ComicBook`); the paint
/// smoothing is a separate pass run before it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Comic {
    /// Flat brightness steps (0 = smooth).
    pub tones: u32,
    pub color_intensity: f32,
    pub border_opacity: f32,
    pub border_tolerance: f32,
    pub border_smoothness: f32,
    pub border_width: f32,
    pub input_contrast: f32,
    pub input_luminosity: f32,
}

/// How much the image changes around the texel: Sobel over taps `d` pixels apart, over all
/// three components for colour (so a hue change with equal brightness still counts). A full
/// black-to-white step gives 1.
fn edge_strength(read: &impl Fn(i32, i32) -> [f32; 3], d: i32, kind: ChannelKind) -> f32 {
    let p = |x: i32, y: i32| read(x * d, y * d);
    let n = [p(-1, -1), p(0, -1), p(1, -1), p(-1, 0), p(1, 0), p(-1, 1), p(0, 1), p(1, 1)];
    let channels = if kind.is_color() { 3 } else { 1 };
    let mut sum = 0.0f32;
    for c in 0..channels {
        let gx = (n[2][c] + 2.0 * n[4][c] + n[7][c]) - (n[0][c] + 2.0 * n[3][c] + n[5][c]);
        let gy = (n[5][c] + 2.0 * n[6][c] + n[7][c]) - (n[0][c] + 2.0 * n[1][c] + n[2][c]);
        sum += gx * gx + gy * gy;
    }
    (sum / channels as f32).sqrt() * 0.25
}

/// Comic Book at one texel of the (already smoothed) image: the colour adjusted and saturated,
/// then darkened to an ink line where the image changes. The texel keeps its coverage.
pub fn comic_book(read: impl Fn(i32, i32) -> [f32; 4], c: Comic, kind: ChannelKind) -> [f32; 4] {
    let here = read(0, 0);
    // The input adjustment and the tone steps, so that borders follow the steps too.
    let tones = c.tones.min(64) as f32;
    let toned = |p: [f32; 4]| {
        let v = [p[0], p[1], p[2]].map(|v| super::contrast_luminosity(v, c.input_contrast, c.input_luminosity, kind));
        if tones < 2.0 {
            return v;
        }
        // Steps are even to the eye (square root of the linear value), each at its middle.
        let l = scalar([v[0], v[1], v[2], 1.0], kind).max(0.0);
        let step = ((l.min(1.0).sqrt() * tones).floor().min(tones - 1.0) + 0.5) / tones;
        // Black has no colour to scale: it takes the lowest step as gray.
        if l <= 1e-6 { [step * step; 3] } else { v.map(|x| x * step * step / l) }
    };
    let mut v = toned(here);
    if kind.is_color() {
        let l = super::luminance(v);
        v = v.map(|x| (l + (x - l) * c.color_intensity).max(0.0));
    }
    let opacity = c.border_opacity.clamp(0.0, 1.0);
    if opacity > 0.0 {
        let d = c.border_width.round().max(1.0) as i32;
        let start = c.border_tolerance.clamp(0.0, 1.0) * 0.5;
        let width = 0.02 + c.border_smoothness.clamp(0.0, 1.0) * 0.3;
        let ink = ramp(start, start + width, edge_strength(&|dx, dy| toned(read(dx, dy)), d, kind)) * opacity;
        v = v.map(|x| x * (1.0 - ink));
    }
    [v[0], v[1], v[2], here[3]]
}

/// Settings of the Watercolor filter (see `subcraft_doc::Filter::Watercolor`); the wash
/// smoothing is a separate pass run before it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Watercolor {
    pub paint_opacity: f32,
    pub paper_absorption: f32,
    pub borders_spread: f32,
    pub borders_density: f32,
    pub hide_paper: bool,
    pub seed: u32,
}

/// Paper grain cells across the texture: blotches the pigment soaks into, and fine fibres.
const PAPER_BLOTCHES: i32 = 10;
const PAPER_FIBRES: i32 = 384;
/// Pigment gathered at a wash edge darkens the colour by at most this.
const EDGE_DARKENING: f32 = 0.3;

/// Watercolor at one texel of the (already smoothed) image, `uv` its position in the texture.
/// Colour is thinned towards white paper (as a transparent film, see below), unevenly where the paper soaks it up, and pigment
/// gathers where the wash changes. Data channels have no paper: they only get the edge and
/// grain modulation. The texel keeps its coverage.
pub fn watercolor(read: impl Fn(i32, i32) -> [f32; 4], w: Watercolor, uv: [f32; 2], kind: ChannelKind) -> [f32; 4] {
    let here = read(0, 0);
    let rgb = [here[0], here[1], here[2]];
    // How much the wash changes on a ring `borders_spread` pixels out.
    let mut edge = 0.0f32;
    let density = w.borders_density.clamp(0.0, 1.0);
    if w.borders_spread >= 0.5 && density > 0.0 {
        let channels = if kind.is_color() { 3 } else { 1 };
        for i in 0..8 {
            let a = i as f32 * std::f32::consts::FRAC_PI_4;
            let p = read((a.cos() * w.borders_spread).round() as i32, (a.sin() * w.borders_spread).round() as i32);
            let d: f32 = (0..channels).map(|c| (p[c] - rgb[c]) * (p[c] - rgb[c])).sum();
            edge += (d / channels as f32).sqrt() / 8.0;
        }
        edge = ramp(0.01, 0.2, edge) * density;
    }
    let q = Vec2::new(uv[0], uv[1]);
    let absorption = w.paper_absorption.clamp(0.0, 1.0);
    let blotch = ramp(0.2, 0.8, fbm(q * PAPER_BLOTCHES as f32, PAPER_BLOTCHES, 4, w.seed));
    let pigment = rgb.map(|v| v * (1.0 - EDGE_DARKENING * edge));
    if !kind.is_color() {
        let k = 1.0 - absorption * 0.3 * (blotch - 0.5);
        let v = pigment.map(|v| if kind.is_signed() { v * k } else { (v * k).clamp(0.0, 1.0) });
        return [v[0], v[1], v[2], here[3]];
    }
    // Watercolor is a transparent film over white paper: a film `thickness` thick lets through
    // the colour to that power (none is white paper, 1 is the colour itself). The paper soaks
    // pigment away in blotches; edges hold more.
    let thickness = w.paint_opacity.clamp(0.0, 1.0) * (1.0 - absorption * 0.5 * blotch) + edge * 0.5;
    let mut out = pigment.map(|v| if thickness <= 0.0 { 1.0 } else { v.max(0.0).powf(thickness) });
    if !w.hide_paper {
        let fibre = fbm(q * PAPER_FIBRES as f32, PAPER_FIBRES, 2, w.seed.wrapping_add(7));
        out = out.map(|v| v * (1.0 - 0.12 * fibre));
    }
    [out[0], out[1], out[2], here[3]]
}

/// Settings of the Oil Paint strokes filter (see `subcraft_doc::Filter::OilPaint`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OilPaint {
    pub effect_intensity: f32,
    pub stroke_size: f32,
    pub stroke_length: f32,
    pub size_random: f32,
    pub color_random: f32,
    pub bristles: f32,
    /// Stroke direction in turns where the image is flat.
    pub angle: f32,
    pub seed: u32,
}

/// Stroke centres sit on a grid this many stroke widths apart (then jittered by a cell), so
/// neighbouring strokes overlap and no texel is left unpainted.
pub const STROKE_SPACING: f32 = 0.75;
/// Bristle streaks across a stroke.
const BRISTLES: f32 = 7.0;
/// Strokes kept per texel (the topmost ones; deeper ones are hidden anyway).
const MAX_LAYERS: usize = 12;

/// One stroke as seen from a texel: its stacking order, coverage there and colour.
type Dab = (f32, f32, [f32; 3]);

/// Oil Paint at texel (x, y): every stroke that covers the texel is found from the jittered
/// grid, and they are laid over the input in their (random) stacking order. A stroke takes the
/// colour under its centre, runs along the local edge there (Sobel over taps a quarter of its
/// width apart; `angle` where the image is flat) and is streaked by its bristles. Strokes
/// depend only on absolute texel positions, so tiles join without seams.
pub fn oil_paint(read: impl Fn(i32, i32) -> [f32; 4], x: u32, y: u32, o: OilPaint, kind: ChannelKind) -> [f32; 4] {
    let here = read(0, 0);
    let intensity = o.effect_intensity.clamp(0.0, 1.0);
    if intensity <= 0.0 || !o.stroke_size.is_finite() || o.stroke_size < 1.0 {
        return here;
    }
    let size = o.stroke_size.min(64.0);
    let length = o.stroke_length.clamp(1.0, 4.0);
    let size_random = o.size_random.clamp(0.0, 1.0);
    let bristles = o.bristles.clamp(0.0, 1.0);
    let spacing = size * STROKE_SPACING;
    let longest = size * (1.0 + 0.5 * size_random) * length * 0.5;
    let n = ((longest + spacing) / spacing).ceil() as i32;
    let tap = (size * 0.25).round().max(1.0) as i32;
    let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
    let (ci, cj) = ((fx / spacing).floor() as i32, (fy / spacing).floor() as i32);
    let mut dabs: [Dab; MAX_LAYERS] = [(0.0, 0.0, [0.0; 3]); MAX_LAYERS];
    let mut count = 0usize;
    for j in cj - n..=cj + n {
        for i in ci - n..=ci + n {
            let h = hash2(i, j, o.seed);
            let scale = 1.0 + size_random * (unit(hash2(i, j, h ^ 0x51)) - 0.5);
            let (half_w, half_l) = (size * scale * 0.5, size * scale * length * 0.5);
            let cx = (i as f32 + unit(hash2(i, j, h ^ 0x1f))) * spacing;
            let cy = (j as f32 + unit(hash2(i, j, h ^ 0x2e))) * spacing;
            let (dx, dy) = (fx - cx, fy - cy);
            if dx * dx + dy * dy > half_l * half_l {
                continue;
            }
            // Reads are relative to the texel.
            let (rx, ry) = (cx.floor() as i32 - x as i32, cy.floor() as i32 - y as i32);
            let centre = read(rx, ry);
            let v = |ox: i32, oy: i32| scalar(read(rx + ox * tap, ry + oy * tap), kind);
            let gx = (v(1, -1) + 2.0 * v(1, 0) + v(1, 1)) - (v(-1, -1) + 2.0 * v(-1, 0) + v(-1, 1));
            let gy = (v(-1, 1) + 2.0 * v(0, 1) + v(1, 1)) - (v(-1, -1) + 2.0 * v(0, -1) + v(1, -1));
            let wobble = (unit(hash2(i, j, h ^ 0x3d)) - 0.5) * 0.6;
            // Along the edge: across the gradient.
            let phi = if gx * gx + gy * gy > 0.0036 { gx.atan2(-gy) } else { o.angle * std::f32::consts::TAU } + wobble;
            let (sin, cos) = phi.sin_cos();
            let (u, w) = ((dx * cos + dy * sin) / half_l, (-dx * sin + dy * cos) / half_w);
            let e = (u * u * u * u + w * w).sqrt();
            if e >= 1.0 {
                continue;
            }
            // The streak this texel is on: its own brightness, and how early it runs dry.
            let streak = hash2(((w * 0.5 + 0.5) * BRISTLES) as i32, 0, h ^ 0x4c);
            let dry = ramp(0.45 + 0.5 * unit(streak), 1.0, u.abs()) * bristles * unit(hash2(1, 0, streak));
            let alpha = (1.0 - ramp(0.8, 1.0, e)) * (1.0 - dry) * centre[3].clamp(0.0, 1.0);
            if alpha <= 0.0 {
                continue;
            }
            let tone = (1.0 + o.color_random.clamp(0.0, 1.0) * 0.5 * (unit(hash2(i, j, h ^ 0x6a)) - 0.5)) * (1.0 + bristles * 0.25 * (unit(streak) - 0.5));
            let dab: Dab = (unit(h), alpha, [centre[0] * tone, centre[1] * tone, centre[2] * tone]);
            // Insertion sort by stacking order, keeping the topmost `MAX_LAYERS`.
            let mut at = count.min(MAX_LAYERS);
            while at > 0 && dabs[at - 1].0 < dab.0 {
                at -= 1;
            }
            if at < MAX_LAYERS {
                let end = count.min(MAX_LAYERS - 1);
                dabs.copy_within(at..end, at + 1);
                dabs[at] = dab;
                count = (count + 1).min(MAX_LAYERS);
            }
        }
    }
    // `dabs` is topmost first: lay them down from the bottom.
    let mut out = [here[0], here[1], here[2]];
    for &(_, alpha, colour) in dabs[..count].iter().rev() {
        for c in 0..3 {
            out[c] += (colour[c] - out[c]) * alpha;
        }
    }
    let signed = kind.is_signed();
    let mix = |a: f32, b: f32| {
        let v = a + (b - a) * intensity;
        if signed || kind.is_color() { v } else { v.clamp(0.0, 1.0) }
    };
    [mix(here[0], out[0]), mix(here[1], out[1]), mix(here[2], out[2]), here[3]]
}

#[cfg(test)]
mod tests {
    use super::*;

    const N: i32 = 96;

    /// An `N` x `N` colour image from `f(x, y)`, read around texel (cx, cy) with clamped edges.
    fn image(f: impl Fn(i32, i32) -> [f32; 3] + Copy, cx: i32, cy: i32) -> impl Fn(i32, i32) -> [f32; 4] {
        move |dx, dy| {
            let v = f((cx + dx).clamp(0, N - 1), (cy + dy).clamp(0, N - 1));
            [v[0], v[1], v[2], 1.0]
        }
    }

    fn comic() -> Comic {
        Comic { tones: 0, color_intensity: 1.0, border_opacity: 1.0, border_tolerance: 0.25, border_smoothness: 0.3, border_width: 1.5, input_contrast: 0.0, input_luminosity: 0.0 }
    }

    #[test]
    fn comic_book_inks_edges_and_leaves_flat_colour_alone() {
        let step = |x: i32, _y: i32| if x < 48 { [0.8, 0.2, 0.2] } else { [0.2, 0.2, 0.8] };
        let kind = ChannelKind::BaseColor;
        // Far from the edge: unchanged at neutral settings.
        let near = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-6);
        assert!(near(comic_book(image(step, 20, 20), comic(), kind), [0.8, 0.2, 0.2, 1.0]));
        // On the edge: an ink line (the two sides have nearly the same brightness, so this
        // also shows that a hue change counts as an edge).
        let on = comic_book(image(step, 47, 20), comic(), kind);
        assert!(on[0] < 0.05 && on[2] < 0.05, "ink: {on:?}");
        // No border opacity: no line.
        let none = comic_book(image(step, 47, 20), Comic { border_opacity: 0.0, ..comic() }, kind);
        assert!(near(none, [0.8, 0.2, 0.2, 1.0]));
        // A high tolerance ignores a faint edge that a low one inks.
        let faint = |x: i32, _y: i32| if x < 48 { [0.5; 3] } else { [0.6; 3] };
        let low = comic_book(image(faint, 47, 20), Comic { border_tolerance: 0.0, ..comic() }, kind)[0];
        let high = comic_book(image(faint, 47, 20), Comic { border_tolerance: 1.0, ..comic() }, kind)[0];
        assert!(low < 0.3 && (high - 0.5).abs() < 1e-6, "tolerance: {low} / {high}");
    }

    #[test]
    fn comic_book_colour_intensity_saturates_colour_only() {
        let flat = |_x: i32, _y: i32| [0.6, 0.4, 0.4];
        let c = Comic { color_intensity: 2.0, ..comic() };
        let out = comic_book(image(flat, 20, 20), c, ChannelKind::BaseColor);
        assert!(out[0] > 0.7 && out[1] < 0.4, "more saturated: {out:?}");
        let before = super::super::luminance([0.6, 0.4, 0.4]);
        assert!((super::super::luminance([out[0], out[1], out[2]]) - before).abs() < 1e-5, "same brightness");
        // Zero is gray.
        let gray = comic_book(image(flat, 20, 20), Comic { color_intensity: 0.0, ..comic() }, ChannelKind::BaseColor);
        assert!((gray[0] - gray[1]).abs() < 1e-6 && (gray[1] - gray[2]).abs() < 1e-6);
        // Data channels are not saturated.
        assert!((comic_book(image(flat, 20, 20), c, ChannelKind::Roughness)[0] - 0.6).abs() < 1e-6);
    }

    #[test]
    fn comic_book_tones_cut_a_gradient_into_inked_steps() {
        let gradient = |x: i32, _y: i32| [x as f32 / N as f32; 3];
        let c = Comic { tones: 3, border_opacity: 0.0, ..comic() };
        let values: Vec<f32> = (0..N).map(|x| comic_book(image(gradient, x, 20), c, ChannelKind::BaseColor)[0]).collect();
        let mut steps = values.clone();
        steps.dedup_by(|a, b| (*a - *b).abs() < 1e-5);
        assert_eq!(steps.len(), 3, "three flat steps: {steps:?}");
        assert!(steps.windows(2).all(|w| w[1] > w[0]));
        // The step borders get ink; the smooth gradient (tones off) gets none.
        let inked = |tones: u32| (2..N - 2).filter(|&x| {
            let c = Comic { tones, ..comic() };
            comic_book(image(gradient, x, 20), c, ChannelKind::BaseColor)[0] < 0.5 * comic_book(image(gradient, x, 20), Comic { border_opacity: 0.0, ..c }, ChannelKind::BaseColor)[0]
        }).count();
        assert!(inked(3) >= 4 && inked(0) == 0, "ink at steps only: {} / {}", inked(3), inked(0));
    }

    fn wash() -> Watercolor {
        Watercolor { paint_opacity: 0.8, paper_absorption: 0.5, borders_spread: 4.0, borders_density: 0.5, hide_paper: true, seed: 1 }
    }

    #[test]
    fn watercolor_thins_colour_towards_white_paper() {
        let flat = |_x: i32, _y: i32| [0.2, 0.4, 0.6];
        let kind = ChannelKind::BaseColor;
        let w = Watercolor { paper_absorption: 0.0, ..wash() };
        // No absorption, no paper: a film 0.8 thick, lighter than the colour.
        let out = watercolor(image(flat, 20, 20), w, [0.2, 0.2], kind);
        for (o, v) in out.iter().zip([0.2f32, 0.4, 0.6]) {
            assert!((o - v.powf(0.8)).abs() < 1e-5 && *o > v, "{out:?}");
        }
        // Full opacity keeps the colour; none leaves white paper.
        let full = watercolor(image(flat, 20, 20), Watercolor { paint_opacity: 1.0, ..w }, [0.2, 0.2], kind);
        assert!((full[0] - 0.2).abs() < 1e-5 && (full[2] - 0.6).abs() < 1e-5);
        assert_eq!(watercolor(image(flat, 20, 20), Watercolor { paint_opacity: 0.0, ..w }, [0.2, 0.2], kind), [1.0, 1.0, 1.0, 1.0]);
        // The paper's fibres darken it a little, unless hidden.
        let paper = watercolor(image(flat, 20, 20), Watercolor { paint_opacity: 0.0, hide_paper: false, ..w }, [0.2, 0.2], kind);
        assert!(paper[0] < 1.0 && paper[0] > 0.85, "paper: {paper:?}");
    }

    #[test]
    fn watercolor_gathers_pigment_at_wash_edges_and_soaks_unevenly() {
        let step = |x: i32, _y: i32| if x < 48 { [0.2, 0.3, 0.7] } else { [0.9, 0.9, 0.9] };
        let kind = ChannelKind::BaseColor;
        let w = Watercolor { paper_absorption: 0.0, ..wash() };
        let inside = watercolor(image(step, 20, 20), w, [0.2, 0.2], kind);
        let edge = watercolor(image(step, 46, 20), w, [0.2, 0.2], kind);
        assert!(edge[2] < inside[2] - 0.05, "darker at the edge: {edge:?} vs {inside:?}");
        // No density: the edge matches the inside.
        let off = watercolor(image(step, 46, 20), Watercolor { borders_density: 0.0, ..w }, [0.2, 0.2], kind);
        assert!((off[2] - inside[2]).abs() < 1e-6);
        // Absorption makes a flat wash uneven across the texture; without it, it stays flat.
        let flat = |_x: i32, _y: i32| [0.2, 0.3, 0.7];
        let spread = |absorption: f32| {
            let values: Vec<f32> = (0..64).map(|i| watercolor(image(flat, 20, 20), Watercolor { paper_absorption: absorption, ..wash() }, [i as f32 / 64.0, 0.37], kind)[0]).collect();
            values.iter().cloned().fold(f32::MIN, f32::max) - values.iter().cloned().fold(f32::MAX, f32::min)
        };
        assert!(spread(0.0) < 1e-6 && spread(1.0) > 0.05, "uneven: {} / {}", spread(0.0), spread(1.0));
    }

    fn oil() -> OilPaint {
        OilPaint { effect_intensity: 1.0, stroke_size: 8.0, stroke_length: 3.0, size_random: 0.0, color_random: 0.0, bristles: 0.0, angle: 0.0, seed: 3 }
    }

    fn paint(f: impl Fn(i32, i32) -> [f32; 3] + Copy, o: OilPaint, x: i32, y: i32) -> [f32; 4] {
        oil_paint(image(f, x, y), x as u32, y as u32, o, ChannelKind::BaseColor)
    }

    #[test]
    fn oil_paint_keeps_a_flat_colour_and_covers_every_texel() {
        let flat = |_x: i32, _y: i32| [0.3, 0.5, 0.7];
        for y in (8..88).step_by(5) {
            for x in (8..88).step_by(5) {
                let out = paint(flat, oil(), x, y);
                assert!((out[0] - 0.3).abs() < 1e-5 && (out[2] - 0.7).abs() < 1e-5, "{out:?}");
            }
        }
        // Covered everywhere: with a colour jitter no texel in a region keeps the exact input.
        let o = OilPaint { color_random: 1.0, ..oil() };
        let untouched = (20..60).flat_map(|y| (20..60).map(move |x| (x, y))).filter(|&(x, y)| paint(flat, o, x, y)[0] == 0.3).count();
        assert!(untouched < 16, "unpainted texels: {untouched}");
    }

    #[test]
    fn oil_paint_strokes_are_patches_of_one_colour() {
        // On fine noise each stroke carries the colour under its centre, so neighbouring texels
        // mostly share a value: far fewer changes along a row than in the input.
        let noise = |x: i32, y: i32| [unit(hash2(x, y, 9)); 3];
        let changes = |f: &dyn Fn(i32, i32) -> f32| (20..76).flat_map(|y| (20..76).map(move |x| (x, y))).filter(|&(x, y)| (f(x, y) - f(x + 1, y)).abs() > 0.02).count();
        let input = changes(&|x, y| noise(x, y)[0]);
        let painted = changes(&|x, y| paint(noise, oil(), x, y)[0]);
        assert!(painted * 2 < input, "strokes: {painted} changes vs {input}");
        // Effect Intensity 0 is the input; 0.5 is halfway.
        assert_eq!(paint(noise, OilPaint { effect_intensity: 0.0, ..oil() }, 30, 30)[0], noise(30, 30)[0]);
        let (a, b, half) = (noise(30, 30)[0], paint(noise, oil(), 30, 30)[0], paint(noise, OilPaint { effect_intensity: 0.5, ..oil() }, 30, 30)[0]);
        assert!((half - (a + b) * 0.5).abs() < 1e-5);
    }

    #[test]
    fn oil_paint_strokes_follow_edges_and_the_angle_on_flat_areas() {
        // Horizontal bands: strokes run along them, so a row inside a band changes less than a
        // column crossing strokes of different bands. Use colour jitter to see single strokes.
        let o = OilPaint { color_random: 1.0, ..oil() };
        let run = |f: &dyn Fn(i32, i32) -> [f32; 3], o: OilPaint, horizontal: bool| {
            (24..72).flat_map(|a| (24..72).map(move |b| (a, b))).filter(|&(a, b)| {
                let (x, y, x2, y2) = if horizontal { (b, a, b + 1, a) } else { (a, b, a, b + 1) };
                (oil_paint(image(f, x, y), x as u32, y as u32, o, ChannelKind::BaseColor)[0] - oil_paint(image(f, x2, y2), x2 as u32, y2 as u32, o, ChannelKind::BaseColor)[0]).abs() > 0.004
            }).count()
        };
        // A flat image: the angle decides. Angle 0 runs along x, a quarter turn along y.
        let flat = |_x: i32, _y: i32| [0.5; 3];
        let (along, across) = (run(&flat, o, true), run(&flat, o, false));
        assert!(along * 5 < across * 4, "angle 0 runs along x: {along} vs {across}");
        let turned = OilPaint { angle: 0.25, ..o };
        let (along, across) = (run(&flat, turned, true), run(&flat, turned, false));
        assert!(across * 5 < along * 4, "a quarter turn runs along y: {along} vs {across}");
        // A vertical gradient (edges run along x) overrides a quarter-turn angle.
        let ramp_y = |_x: i32, y: i32| [y as f32 / N as f32; 3];
        let o = OilPaint { color_random: 0.0, bristles: 1.0, ..turned };
        let (along, across) = (run(&ramp_y, o, true), run(&ramp_y, o, false));
        assert!(along < across, "strokes follow the gradient's edge direction: {along} vs {across}");
    }

    #[test]
    fn oil_paint_is_the_same_whatever_window_reads_it() {
        // The stroke layout depends on absolute positions only (what makes tiles seamless):
        // evaluating a texel is deterministic and neighbours across any boundary share strokes.
        let noise = |x: i32, y: i32| [unit(hash2(x / 6, y / 6, 2)); 3];
        let o = OilPaint { size_random: 1.0, color_random: 0.5, bristles: 1.0, ..oil() };
        assert_eq!(paint(noise, o, 63, 40), paint(noise, o, 63, 40));
        assert_ne!(paint(noise, OilPaint { seed: 4, ..o }, 40, 40), paint(noise, o, 40, 40));
    }
}
