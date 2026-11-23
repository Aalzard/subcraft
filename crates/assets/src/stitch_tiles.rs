//! Sample stitch tiles: a colour image (with transparency), a height map and a normal map of
//! one repeat of a seam, laid along a pen path as a ribbon. They are made here, procedurally and
//! the same each time, and they show what an imported tile needs: the seam runs along the image's
//! x axis, the two long edges are flat surface, and the left edge continues the right one.

use std::sync::Arc;

use glam::Vec2;
use subcraft_image::Bitmap;
use subcraft_materials::noise;

/// One repeat of a seam as three images of the same size.
#[derive(Clone, Debug)]
pub struct StitchTile {
    pub name: &'static str,
    pub description: &'static str,
    /// RGBA, linear colour; transparent where the surface under the seam shows.
    pub color: Arc<Bitmap>,
    /// Grayscale height; 0.5 is the surface, above it is raised.
    pub height: Arc<Bitmap>,
    /// Tangent space normal map (Y up) of `height`.
    pub normal: Arc<Bitmap>,
    /// Width of a new path, in percent of the texture.
    pub size: f32,
    pub roughness: f32,
    /// Height value of a new path: the height map's full range reaches this far.
    pub relief: f32,
}

/// What one texel of a tile holds: colour, opacity and height from -1 to 1.
#[derive(Clone, Copy, Default)]
struct Texel {
    rgb: [f32; 3],
    alpha: f32,
    height: f32,
}

const HEIGHT_PX: u32 = 256;

/// Distance from `p` to the segment `a`-`b`, and how far along it (0..1) the nearest point is.
fn segment(p: Vec2, a: Vec2, b: Vec2) -> (f32, f32) {
    let d = b - a;
    let t = ((p - a).dot(d) / d.length_squared().max(1e-12)).clamp(0.0, 1.0);
    (p.distance(a + d * t), t)
}

fn smooth(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A thread from `a` to `b` of half width `r`: (height 0..1, coverage, shade of its colour).
/// Twisted plies, fine fibres along them, ends that go down into their holes.
fn thread(p: Vec2, a: Vec2, b: Vec2, r: f32, seed: u32) -> (f32, f32, f32) {
    let (dist, t) = segment(p, a, b);
    if dist >= r {
        return (0.0, 0.0, 1.0);
    }
    let dir = (b - a).normalize_or(Vec2::X);
    let len = a.distance(b);
    let (along, across) = ((p - a).dot(dir), (p - a).dot(dir.perp()));
    let x = dist / r;
    let round = (1.0 - x * x).sqrt();
    // Plies: slanted grooves across the thread.
    let phase = (along * 0.8 + across) / r * std::f32::consts::PI;
    let ply = 0.5 + 0.5 * phase.cos();
    let groove = 1.0 - 0.28 * ply.powi(3);
    // Fibres: thin streaks that run with the plies.
    let streak = noise::hash_f(((along * 0.8 + across) / r * 9.0).floor() as i32, ((along - across) / r * 1.5).floor() as i32, seed);
    let fibre = 1.0 - 0.1 * streak;
    let end = (t.min(1.0 - t) * len / (r * 2.2)).clamp(0.0, 1.0);
    let dive = 0.35 + 0.65 * end * (2.0 - end);
    let height = round * groove * fibre * dive;
    let coverage = smooth(1.0, 0.86, x);
    // Darker in the grooves, at the rim and where it goes into the hole.
    let shade = (0.62 + 0.38 * round) * (1.0 - 0.3 * ply.powi(3)) * (0.9 + 0.1 * streak) * (0.7 + 0.3 * dive);
    (height, coverage, shade)
}

/// Put `top` over `under` by its alpha.
fn over(under: Texel, rgb: [f32; 3], alpha: f32) -> Texel {
    let a = alpha.clamp(0.0, 1.0);
    let out = under.alpha + a * (1.0 - under.alpha);
    let mix = |i: usize| if out > 1e-6 { (rgb[i] * a + under.rgb[i] * under.alpha * (1.0 - a)) / out } else { 0.0 };
    Texel { rgb: [mix(0), mix(1), mix(2)], alpha: out, height: under.height }
}

/// The three images of a tile `aspect` times as long as it is wide. `f` gets a point in
/// (0..aspect, 0..1) and must be flat (height 0, transparent) on both long edges and continue
/// from the right edge to the left one.
fn build(aspect: u32, f: impl Fn(Vec2) -> Texel + Sync) -> (Arc<Bitmap>, Arc<Bitmap>, Arc<Bitmap>) {
    use rayon::prelude::*;
    let (w, h) = (HEIGHT_PX * aspect, HEIGHT_PX);
    let texels: Vec<Texel> = (0..w * h).into_par_iter().map(|i| f(Vec2::new(((i % w) as f32 + 0.5) / h as f32, ((i / w) as f32 + 0.5) / h as f32))).collect();
    let at = |x: i64, y: i64| texels[(y.clamp(0, h as i64 - 1) as u32 * w + x.rem_euclid(w as i64) as u32) as usize].height;
    let mut color = Vec::with_capacity(texels.len() * 4);
    let mut height = Vec::with_capacity(texels.len());
    let mut normal = Vec::with_capacity(texels.len() * 3);
    // Slope of the relief for a seam about a tenth as high as it is wide.
    let strength = h as f32 * 0.05;
    for (i, t) in texels.iter().enumerate() {
        let (x, y) = ((i as u32 % w) as i64, (i as u32 / w) as i64);
        color.extend_from_slice(&[t.rgb[0], t.rgb[1], t.rgb[2], t.alpha.clamp(0.0, 1.0)]);
        height.push((0.5 + 0.5 * t.height).clamp(0.0, 1.0));
        let dx = (at(x + 1, y) - at(x - 1, y)) * strength;
        let dy = (at(x, y + 1) - at(x, y - 1)) * strength;
        // Y up: the image's rows run down.
        let n = glam::Vec3::new(-dx, dy, 1.0).normalize();
        normal.extend_from_slice(&[n.x * 0.5 + 0.5, n.y * 0.5 + 0.5, n.z * 0.5 + 0.5]);
    }
    (Arc::new(Bitmap::new_f32(w, h, 4, color)), Arc::new(Bitmap::new_f32(w, h, 1, height)), Arc::new(Bitmap::new_f32(w, h, 3, normal)))
}

/// A needle hole at `c`: pressed in, dark.
fn hole(t: Texel, p: Vec2, c: Vec2, r: f32) -> Texel {
    let d = p.distance(c) / r;
    if d >= 1.6 {
        return t;
    }
    // A dent with a slightly raised rim, as a needle leaves in leather.
    let dent = -0.8 * smooth(1.0, 0.2, d) + 0.12 * smooth(1.6, 1.1, d) * smooth(0.8, 1.1, d);
    let mut out = over(t, [0.02, 0.015, 0.01], 0.85 * smooth(1.0, 0.5, d));
    out.height = t.height + dent;
    out
}

/// The thread over what is there: its soft shadow first, then the thread itself.
fn sew(t: Texel, p: Vec2, a: Vec2, b: Vec2, r: f32, rgb: [f32; 3], seed: u32) -> Texel {
    let (dist, _) = segment(p, a + Vec2::new(0.012, 0.02), b + Vec2::new(0.012, 0.02));
    let mut out = over(t, [0.0, 0.0, 0.0], 0.45 * smooth(r * 1.5, r * 0.8, dist));
    let (h, cover, shade) = thread(p, a, b, r, seed);
    if cover > 0.0 {
        out = over(out, rgb.map(|c| c * shade), cover);
        out.height = out.height.max(h * 0.9);
    }
    out
}

/// The surface along the seam line: pressed in between the stitches, flat at both long edges.
fn pressed(p: Vec2, centre: f32, width: f32, depth: f32) -> Texel {
    let d = (p.y - centre) / width;
    Texel { rgb: [0.0; 3], alpha: 0.0, height: -depth * (-d * d).exp() * smooth(0.0, 0.08, p.y) * smooth(1.0, 0.92, p.y) }
}

/// The built-in sample tiles.
pub fn builtin() -> Vec<StitchTile> {
    let saddle = build(2, |p| {
        // One thick slanted stitch per repeat, a needle hole at each end (half on each side
        // edge), in a pressed line.
        let mut t = pressed(p, 0.5, 0.2, 0.22);
        for x in [0.0, 2.0] {
            t = hole(t, p, Vec2::new(x, 0.5), 0.11);
        }
        sew(t, p, Vec2::new(0.22, 0.56), Vec2::new(1.78, 0.44), 0.16, [0.78, 0.69, 0.5], 7)
    });
    let denim = build(2, |p| {
        // Two rows of topstitching, the cloth pressed under each and puffed up between them.
        let mut t = pressed(p, 0.3, 0.09, 0.2);
        t.height += pressed(p, 0.7, 0.09, 0.2).height + 0.16 * (-((p.y - 0.5) / 0.1).powi(2)).exp();
        for (y, shift) in [(0.3, 0.0), (0.7, 1.0)] {
            for k in [-1.0, 0.0, 1.0] {
                let x = shift + k * 2.0;
                t = hole(t, p, Vec2::new(x, y), 0.04);
                t = sew(t, p, Vec2::new(x + 0.07, y), Vec2::new(x + 1.93, y), 0.05, [0.83, 0.45, 0.07], 11);
            }
        }
        t
    });
    let zipper = build(1, |p| {
        // Two teeth per repeat, one from each side, meeting in the middle; a cloth tape under
        // each row.
        let mut t = Texel::default();
        let tape = smooth(0.08, 0.12, p.y) * smooth(0.92, 0.88, p.y);
        let weave = 0.5 + 0.5 * (p.x * 40.0 * std::f32::consts::PI).sin() * (p.y * 40.0 * std::f32::consts::PI).sin();
        t = over(t, [0.025 + 0.012 * weave, 0.025 + 0.012 * weave, 0.03 + 0.012 * weave], tape);
        t.height = tape * (0.1 + 0.04 * weave);
        for (x, from, to) in [(0.25, 0.3, 0.58), (0.75, 0.7, 0.42), (-0.25, 0.7, 0.42), (1.25, 0.3, 0.58)] {
            let (dist, along) = segment(p, Vec2::new(x, from), Vec2::new(x, to));
            let r = 0.15;
            if dist < r {
                let x = dist / r;
                let top = (1.0 - x.powi(4)).max(0.0).powf(0.6);
                let brushed = 0.9 + 0.1 * noise::hash_f((p.y * 180.0) as i32, 0, 5);
                t = over(t, [0.62, 0.58, 0.5].map(|c| c * brushed * (0.75 + 0.25 * top)), smooth(1.0, 0.9, x));
                t.height = t.height.max(top * (0.75 + 0.2 * along));
            }
        }
        t
    });
    let tile = |name, description, (color, height, normal): (Arc<Bitmap>, Arc<Bitmap>, Arc<Bitmap>), size, roughness, relief| StitchTile { name, description, color, height, normal, size, roughness, relief };
    vec![
        tile("Tile: Saddle Leather", "Thick waxed thread in slanted stitches with needle holes, in a pressed seam", saddle, 3.0, 0.6, 0.7),
        tile("Tile: Denim Topstitch", "Two rows of orange topstitching with the cloth puffed up between them", denim, 4.0, 0.8, 0.6),
        tile("Tile: Zipper", "Metal zipper teeth on a cloth tape", zipper, 4.0, 0.4, 0.8),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_tiles_are_flat_at_the_long_edges_and_continue_at_the_short_ones() {
        let tiles = builtin();
        assert_eq!(tiles.len(), 3);
        for t in &tiles {
            let (w, h) = (t.color.width, t.color.height);
            assert_eq!((t.height.width, t.height.height, t.normal.width, t.normal.height), (w, h, w, h), "{}", t.name);
            assert_eq!((t.color.channels, t.height.channels, t.normal.channels), (4, 1, 3));
            let (mut raised, mut sunk, mut opaque, mut clear) = (0, 0, 0, 0);
            for y in 0..h {
                for x in 0..w {
                    let height = t.height.texel(x, y, false)[0];
                    let alpha = t.color.texel(x, y, false)[3];
                    raised += (height > 0.7) as u32;
                    sunk += (height < 0.45) as u32;
                    opaque += (alpha > 0.9) as u32;
                    clear += (alpha < 0.05) as u32;
                    let n = t.normal.texel(x, y, false);
                    let len = ((n[0] * 2.0 - 1.0).powi(2) + (n[1] * 2.0 - 1.0).powi(2) + (n[2] * 2.0 - 1.0).powi(2)).sqrt();
                    assert!((len - 1.0).abs() < 1e-3 && n[2] > 0.5, "{}: unit normals that face out", t.name);
                }
            }
            assert!(raised > 500 && opaque > 500 && clear > 500, "{}: a raised, opaque thing on a clear ground ({raised} {opaque} {clear})", t.name);
            if t.name != "Tile: Zipper" {
                assert!(sunk > 100, "{}: holes or a pressed seam ({sunk})", t.name);
            }
            for x in 0..w {
                for y in [0, h - 1] {
                    assert!((t.height.texel(x, y, false)[0] - 0.5).abs() < 0.03, "{}: long edges are at the surface", t.name);
                }
            }
            for y in 0..h {
                let (a, b) = (t.height.texel(0, y, false)[0], t.height.texel(w - 1, y, false)[0]);
                assert!((a - b).abs() < 0.12, "{}: the right edge continues into the left one at row {y}: {a} {b}", t.name);
            }
        }
        // The same each time.
        let again = builtin();
        assert!(tiles.iter().zip(&again).all(|(a, b)| *a.color == *b.color && *a.height == *b.height && *a.normal == *b.normal));
    }
}
