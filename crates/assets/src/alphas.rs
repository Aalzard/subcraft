//! Brush alphas (tip images): built-in procedural tips and imported grayscale images.
//! White paints, black doesn't; an image's alpha channel multiplies its luminance.

use std::path::Path;
use std::sync::Arc;

use glam::Vec2;
use subcraft_core::AssetId;
use subcraft_doc::{ProceduralKind, ProceduralParams};
use subcraft_image::Bitmap;
use subcraft_materials::{noise, pattern};

#[derive(Clone, Debug)]
pub struct BrushAlpha {
    pub id: AssetId,
    pub name: String,
    /// `None` = the procedural round tip (hardness controls its falloff).
    pub bitmap: Option<Arc<Bitmap>>,
    /// File in the user library (`None` for built-in tips).
    pub user_file: Option<std::path::PathBuf>,
}

const SIZE: u32 = 256;

fn circle_falloff(u: f32, v: f32, inner: f32) -> f32 {
    let d = Vec2::new(u - 0.5, v - 0.5).length() * 2.0;
    1.0 - ((d - inner) / (1.0 - inner)).clamp(0.0, 1.0)
}

fn gray(f: impl Fn(f32, f32) -> f32 + Sync) -> Option<Arc<Bitmap>> {
    Some(Arc::new(Bitmap::from_fn_gray(SIZE, SIZE, |u, v| f(u, v).clamp(0.0, 1.0))))
}

fn alpha(name: &str, bitmap: Option<Arc<Bitmap>>) -> BrushAlpha {
    BrushAlpha { id: AssetId::from_name(&format!("alpha/{name}")), name: name.into(), bitmap, user_file: None }
}

/// Built-in tips (deterministic, generated at startup).
pub fn builtin() -> Vec<BrushAlpha> {
    vec![
        alpha("Round", None),
        alpha("Square", gray(|_, _| 1.0)),
        alpha("Soft Square", gray(|u, v| {
            let e = (0.5 - (u - 0.5).abs()).min(0.5 - (v - 0.5).abs()) * 2.0;
            (e / 0.3).min(1.0)
        })),
        alpha("Splatter", gray(|u, v| {
            let n = pattern::raw(ProceduralKind::Cells, Vec2::new(u, v), 6.0, 11);
            let blob = if n > 0.35 { 1.0 } else { 0.0 };
            blob * circle_falloff(u, v, 0.55)
        })),
        alpha("Grunge", gray(|u, v| {
            let p = ProceduralParams { contrast: 0.6, ..ProceduralParams::new(ProceduralKind::Grunge) };
            pattern::eval(&p, Vec2::new(u, v)) * circle_falloff(u, v, 0.4)
        })),
        alpha("Scratches", gray(|u, v| {
            let s = pattern::raw(ProceduralKind::Scratches, Vec2::new(u, v), 3.0, 5);
            s * circle_falloff(u, v, 0.6)
        })),
        alpha("Spray", gray(|u, v| {
            let (x, y) = ((u * 64.0) as i32, (v * 64.0) as i32);
            let dot = if noise::hash_f(x, y, 77) > 0.82 { 1.0 } else { 0.0 };
            dot * circle_falloff(u, v, 0.0)
        })),
        alpha("Line", gray(|_, v| {
            let d = (v - 0.5).abs();
            1.0 - ((d - 0.06) / 0.04).clamp(0.0, 1.0)
        })),
        // Stitch tips (issue #553): the thread runs along the tip's x axis, which Follow path
        // turns along the stroke. At a spacing of one diameter they join into a line of stitches.
        // The thread has a rounded profile, so painted into Height it reads as a raised cord.
        alpha("Stitch Straight", gray(|u, v| cord(u, v, (0.12, 0.5), (0.88, 0.5), THREAD))),
        alpha("Stitch Cross", gray(|u, v| {
            cord(u, v, (0.22, 0.22), (0.78, 0.78), THREAD * 0.9).max(cord(u, v, (0.22, 0.78), (0.78, 0.22), THREAD * 0.9))
        })),
        alpha("Stitch Zigzag", gray(|u, v| {
            // Ends at the same height on both sides, so neighbouring stamps continue the line.
            let a = segment_distance(u, v, (0.0, 0.32), (0.5, 0.68));
            let b = segment_distance(u, v, (0.5, 0.68), (1.0, 0.32));
            thread(a.min(b), THREAD * 0.9)
        })),
        // Saddle stitch: a thick thread slanted across the line, as on hand-sewn leather.
        alpha("Stitch Saddle", gray(|u, v| cord(u, v, (0.16, 0.6), (0.84, 0.4), THREAD * 1.6))),
        // Double row: two parallel lines of straight stitches (topstitching on denim). Two
        // stitches per row and half the thread width, as the tip is used at twice the size.
        alpha("Stitch Double", gray(|u, v| {
            [0.3, 0.7].into_iter().flat_map(|y| [(0.06, 0.44), (0.56, 0.94)].map(|(a, b)| cord(u, v, (a, y), (b, y), THREAD * 0.5))).fold(0.0, f32::max)
        })),
        // Whip stitch: steep slanted threads that wrap over an edge, close together.
        alpha("Stitch Whip", gray(|u, v| cord(u, v, (0.3, 0.9), (0.7, 0.1), THREAD * 1.5))),
        // Blanket stitch: a thread along the edge with a leg into the cloth at every stitch.
        alpha("Stitch Blanket", gray(|u, v| {
            // Six plies per tip length, so the edge thread continues into the next stamp.
            let groove = 0.5 + 0.5 * (std::f32::consts::TAU * 6.0 * u + std::f32::consts::PI * (v - 0.82) / (THREAD * 0.9)).cos();
            let edge = thread((v - 0.82).abs(), THREAD * 0.9) * (1.0 - 0.2 * groove.powi(4));
            edge.max(cord(u, v, (0.5, 0.82), (0.5, 0.16), THREAD * 0.9))
        })),
        // Chain stitch: a loop per stamp, each one starting inside the one before.
        alpha("Stitch Chain", gray(|u, v| {
            let loop_at = |u: f32| {
                // A drop shape: narrow where it starts, wide where the next loop holds it.
                let (x, y) = (u - 0.45, v - 0.5);
                let half = 0.24 * (0.35 + 0.65 * ((u + 0.15) / 1.2).clamp(0.0, 1.0)).sqrt();
                let r = ((x / 0.6).powi(2) + (y / half).powi(2)).sqrt();
                thread((r - 1.0).abs() * half, THREAD * 0.6)
            };
            loop_at(u).max(loop_at(u + 1.0)).max(loop_at(u - 1.0))
        })),
        // Herringbone: long slanted threads that cross near both edges, each over the next.
        alpha("Stitch Herringbone", gray(|u, v| {
            [-1.0, 0.0, 1.0].into_iter().map(|s| {
                cord(u + s, v, (-0.12, 0.8), (0.62, 0.2), THREAD * 0.8).max(cord(u + s, v, (0.38, 0.2), (1.12, 0.8), THREAD * 0.8))
            }).fold(0.0, f32::max)
        })),
        // Baseball stitch: pairs of threads that meet in a V over the seam between them.
        alpha("Stitch Baseball", gray(|u, v| cord(u, v, (0.12, 0.1), (0.82, 0.44), THREAD).max(cord(u, v, (0.12, 0.9), (0.82, 0.56), THREAD)))),
        // Triple row: three lines of straight stitches, the middle one half a stitch along.
        alpha("Stitch Triple", gray(|u, v| {
            let rows = [(0.2, 0.0), (0.5, 0.5), (0.8, 0.0)];
            rows.into_iter().flat_map(|(y, shift)| [-1.0, 0.0, 1.0].map(move |s| (y, shift + s))).map(|(y, s)| cord(u + s, v, (0.08, y), (0.92, y), THREAD * 0.5)).fold(0.0, f32::max)
        })),
        // Cord: one continuous twisted rope (laces, piping, drawstrings).
        alpha("Stitch Cord", gray(|u, v| {
            let w = (v - 0.5) / (THREAD * 2.4);
            // Three plies per tip length, so neighbouring stamps continue the twist.
            let twist = 0.5 + 0.5 * (std::f32::consts::TAU * (3.0 * u + 0.9 * w)).cos();
            thread((v - 0.5).abs(), THREAD * 2.4) * (0.55 + 0.45 * twist.sqrt())
        })),
        // Needle holes: half a hole on each side edge, so stamps one diameter apart complete
        // each hole in the gap between two straight stitches.
        alpha("Stitch Needle Holes", gray(|u, v| {
            let d = Vec2::new(u.min(1.0 - u), v - 0.5).length();
            thread(d, 0.075)
        })),
    ]
}

/// Half width of a stitch thread, in tip units.
const THREAD: f32 = 0.09;

/// Distance from `(u, v)` to the segment `a`-`b` (rounded ends).
fn segment_distance(u: f32, v: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (p, a, b) = (Vec2::new(u, v), Vec2::new(a.0, a.1), Vec2::new(b.0, b.1));
    let d = b - a;
    let t = ((p - a).dot(d) / d.length_squared().max(1e-12)).clamp(0.0, 1.0);
    p.distance(a + d * t)
}

/// Rounded cord profile: 1 on the thread's axis, falling to 0 at `half_width` like a circle's
/// section.
fn thread(distance: f32, half_width: f32) -> f32 {
    let x = (distance / half_width).min(1.0);
    (1.0 - x * x).sqrt()
}

/// How much of the thread's height is left at a point `along` and `across` it (both in tip
/// units): shallow slanted grooves between the plies of a twisted thread.
fn ply(along: f32, across: f32, half_width: f32) -> f32 {
    let groove = 0.5 + 0.5 * (std::f32::consts::PI * (along * 0.9 + across) / half_width).cos();
    1.0 - 0.2 * groove.powi(4)
}

/// A piece of sewing thread from `a` to `b`: the rounded cord profile, twisted plies, and ends
/// that go down into the cloth.
fn cord(u: f32, v: f32, a: (f32, f32), b: (f32, f32), half_width: f32) -> f32 {
    let (p, a, b) = (Vec2::new(u, v), Vec2::new(a.0, a.1), Vec2::new(b.0, b.1));
    let d = b - a;
    let len = d.length().max(1e-6);
    let dir = d / len;
    let along = (p - a).dot(dir);
    let across = (p - a).dot(dir.perp());
    let t = along.clamp(0.0, len);
    let distance = (p - (a + dir * t)).length();
    // The thread dives into its needle holes over the last two widths of each end.
    let end = (t.min(len - t) / (half_width * 2.0)).clamp(0.0, 1.0);
    let dive = 0.55 + 0.45 * (end * (2.0 - end));
    thread(distance, half_width) * ply(along, across, half_width) * dive
}

#[cfg(test)]
mod stitch_tests {
    #[test]
    fn stitch_tips_are_threads_along_x_that_join_at_one_diameter() {
        let tips = super::builtin();
        let get = |name: &str| tips.iter().find(|a| a.name == name).and_then(|a| a.bitmap.clone()).unwrap_or_else(|| panic!("{name}"));
        let at = |b: &subcraft_image::Bitmap, u: f32, v: f32| b.sample(u, v, subcraft_image::Wrap::Clamp, false)[0];
        let straight = get("Stitch Straight");
        assert!(at(&straight, 0.5, 0.5) > 0.78, "thread on the axis");
        assert!(at(&straight, 0.5, 0.56) > 0.1 && at(&straight, 0.5, 0.56) < at(&straight, 0.5, 0.5), "rounded profile");
        assert!(at(&straight, 0.5, 0.7) < 0.01 && at(&straight, 0.02, 0.5) < 0.01, "nothing beside it or past its ends");
        let cross = get("Stitch Cross");
        assert!(at(&cross, 0.5, 0.5) > 0.78 && at(&cross, 0.3, 0.3) > 0.5 && at(&cross, 0.3, 0.7) > 0.5 && at(&cross, 0.5, 0.25) < 0.01);
        let zigzag = get("Stitch Zigzag");
        // Both side edges sit at the same height: the next stamp continues the line.
        assert!(at(&zigzag, 0.005, 0.32) > 0.8 && at(&zigzag, 0.995, 0.32) > 0.8 && at(&zigzag, 0.5, 0.68) > 0.9);
        assert!(at(&zigzag, 0.5, 0.32) < 0.01);
        let holes = get("Stitch Needle Holes");
        assert!(at(&holes, 0.005, 0.5) > 0.9 && at(&holes, 0.995, 0.5) > 0.9 && at(&holes, 0.5, 0.5) < 0.01, "half holes on both edges");
        // The thread dives at its ends, and the twist never cuts it through.
        assert!(at(&straight, 0.14, 0.5) < at(&straight, 0.3, 0.5), "ends go down into the cloth");
        assert!((20..=80).all(|i| at(&straight, i as f32 / 100.0, 0.5) > 0.6), "the plies are shallow grooves");
        // Continuous tips meet their neighbour: the same value on both side edges.
        for name in ["Stitch Cord", "Stitch Blanket", "Stitch Chain", "Stitch Herringbone", "Stitch Triple"] {
            let tip = get(name);
            for i in 0..=20 {
                let v = i as f32 / 20.0;
                assert!((at(&tip, 0.002, v) - at(&tip, 0.998, v)).abs() < 0.08, "{name} joins at v = {v}");
            }
        }
        for name in ["Stitch Whip", "Stitch Blanket", "Stitch Chain", "Stitch Herringbone", "Stitch Baseball", "Stitch Triple", "Stitch Cord"] {
            let tip = get(name);
            let (mut covered, mut top) = (0, 0.0f32);
            for y in 0..32 { for x in 0..32 { let a = at(&tip, (x as f32 + 0.5) / 32.0, (y as f32 + 0.5) / 32.0); covered += (a > 0.05) as u32; top = top.max(a); } }
            assert!(top > 0.7 && (40..800).contains(&covered), "{name}: a drawing, not empty or full ({covered}, {top})");
        }
        // Deterministic: generated the same each time.
        assert_eq!(*straight, *super::builtin().iter().find(|a| a.name == "Stitch Straight").unwrap().bitmap.clone().unwrap());
    }
}

/// Import an image file as a brush alpha (grayscale luminance × alpha).
pub fn import(path: &Path) -> Result<BrushAlpha, String> {
    let bmp = subcraft_image::load_bitmap(path).map_err(|e| e.to_string())?;
    let (w, h) = (bmp.width, bmp.height);
    let mut data = Vec::with_capacity((w * h) as usize * 2);
    for y in 0..h {
        for x in 0..w {
            let t = bmp.texel(x, y, false);
            let lum = 0.2126 * t[0] + 0.7152 * t[1] + 0.0722 * t[2];
            data.extend_from_slice(&[lum.clamp(0.0, 1.0), t[3]]);
        }
    }
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Alpha").to_string();
    Ok(BrushAlpha {
        id: AssetId::from_name(&format!("alpha/import/{}", path.display())),
        name,
        bitmap: Some(Arc::new(Bitmap::new_f32(w, h, 2, data))),
        user_file: None,
    })
}
