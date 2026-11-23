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
    Some(Arc::new(Bitmap::from_fn_gray(SIZE, SIZE, |u, v| {
        f(u, v).clamp(0.0, 1.0)
    })))
}

fn alpha(name: &str, bitmap: Option<Arc<Bitmap>>) -> BrushAlpha {
    BrushAlpha {
        id: AssetId::from_name(&format!("alpha/{name}")),
        name: name.into(),
        bitmap,
        user_file: None,
    }
}

/// Built-in tips (deterministic, generated at startup).
pub fn builtin() -> Vec<BrushAlpha> {
    vec![
        alpha("Round", None),
        alpha("Square", gray(|_, _| 1.0)),
        alpha(
            "Soft Square",
            gray(|u, v| {
                let e = (0.5 - (u - 0.5).abs()).min(0.5 - (v - 0.5).abs()) * 2.0;
                (e / 0.3).min(1.0)
            }),
        ),
        alpha(
            "Splatter",
            gray(|u, v| {
                let n = pattern::raw(ProceduralKind::Cells, Vec2::new(u, v), 6.0, 11);
                let blob = if n > 0.35 { 1.0 } else { 0.0 };
                blob * circle_falloff(u, v, 0.55)
            }),
        ),
        alpha(
            "Grunge",
            gray(|u, v| {
                let p = ProceduralParams {
                    contrast: 0.6,
                    ..ProceduralParams::new(ProceduralKind::Grunge)
                };
                pattern::eval(&p, Vec2::new(u, v)) * circle_falloff(u, v, 0.4)
            }),
        ),
        alpha(
            "Scratches",
            gray(|u, v| {
                let s = pattern::raw(ProceduralKind::Scratches, Vec2::new(u, v), 3.0, 5);
                s * circle_falloff(u, v, 0.6)
            }),
        ),
        alpha(
            "Spray",
            gray(|u, v| {
                let (x, y) = ((u * 64.0) as i32, (v * 64.0) as i32);
                let dot = if noise::hash_f(x, y, 77) > 0.82 {
                    1.0
                } else {
                    0.0
                };
                dot * circle_falloff(u, v, 0.0)
            }),
        ),
        alpha(
            "Line",
            gray(|_, v| {
                let d = (v - 0.5).abs();
                1.0 - ((d - 0.06) / 0.04).clamp(0.0, 1.0)
            }),
        ),
        // Stitch tips (issue #553): the thread runs along the tip's x axis, which Follow path
        // turns along the stroke. At a spacing of one diameter they join into a line of stitches.
        // The thread has a rounded profile, so painted into Height it reads as a raised cord.
        alpha(
            "Stitch Straight",
            gray(|u, v| thread(segment_distance(u, v, (0.12, 0.5), (0.88, 0.5)), THREAD)),
        ),
        alpha(
            "Stitch Cross",
            gray(|u, v| {
                let a = segment_distance(u, v, (0.22, 0.22), (0.78, 0.78));
                let b = segment_distance(u, v, (0.22, 0.78), (0.78, 0.22));
                thread(a.min(b), THREAD * 0.9)
            }),
        ),
        alpha(
            "Stitch Zigzag",
            gray(|u, v| {
                // Ends at the same height on both sides, so neighbouring stamps continue the line.
                let a = segment_distance(u, v, (0.0, 0.32), (0.5, 0.68));
                let b = segment_distance(u, v, (0.5, 0.68), (1.0, 0.32));
                thread(a.min(b), THREAD * 0.9)
            }),
        ),
        // Needle holes: half a hole on each side edge, so stamps one diameter apart complete
        // each hole in the gap between two straight stitches.
        alpha(
            "Stitch Needle Holes",
            gray(|u, v| {
                let d = Vec2::new(u.min(1.0 - u), v - 0.5).length();
                thread(d, 0.075)
            }),
        ),
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

#[cfg(test)]
mod stitch_tests {
    #[test]
    fn stitch_tips_are_threads_along_x_that_join_at_one_diameter() {
        let tips = super::builtin();
        let get = |name: &str| {
            tips.iter()
                .find(|a| a.name == name)
                .and_then(|a| a.bitmap.clone())
                .unwrap_or_else(|| panic!("{name}"))
        };
        let at = |b: &subcraft_image::Bitmap, u: f32, v: f32| {
            b.sample(u, v, subcraft_image::Wrap::Clamp, false)[0]
        };
        let straight = get("Stitch Straight");
        assert!(at(&straight, 0.5, 0.5) > 0.95, "thread on the axis");
        assert!(
            at(&straight, 0.5, 0.56) > 0.1 && at(&straight, 0.5, 0.56) < at(&straight, 0.5, 0.5),
            "rounded profile"
        );
        assert!(
            at(&straight, 0.5, 0.7) < 0.01 && at(&straight, 0.02, 0.5) < 0.01,
            "nothing beside it or past its ends"
        );
        let cross = get("Stitch Cross");
        assert!(
            at(&cross, 0.5, 0.5) > 0.95
                && at(&cross, 0.3, 0.3) > 0.9
                && at(&cross, 0.3, 0.7) > 0.9
                && at(&cross, 0.5, 0.25) < 0.01
        );
        let zigzag = get("Stitch Zigzag");
        // Both side edges sit at the same height: the next stamp continues the line.
        assert!(
            at(&zigzag, 0.005, 0.32) > 0.8
                && at(&zigzag, 0.995, 0.32) > 0.8
                && at(&zigzag, 0.5, 0.68) > 0.9
        );
        assert!(at(&zigzag, 0.5, 0.32) < 0.01);
        let holes = get("Stitch Needle Holes");
        assert!(
            at(&holes, 0.005, 0.5) > 0.9
                && at(&holes, 0.995, 0.5) > 0.9
                && at(&holes, 0.5, 0.5) < 0.01,
            "half holes on both edges"
        );
        // Deterministic: generated the same each time.
        assert_eq!(
            *straight,
            *super::builtin()
                .iter()
                .find(|a| a.name == "Stitch Straight")
                .unwrap()
                .bitmap
                .clone()
                .unwrap()
        );
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
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Alpha")
        .to_string();
    Ok(BrushAlpha {
        id: AssetId::from_name(&format!("alpha/import/{}", path.display())),
        name,
        bitmap: Some(Arc::new(Bitmap::new_f32(w, h, 2, data))),
        user_file: None,
    })
}
