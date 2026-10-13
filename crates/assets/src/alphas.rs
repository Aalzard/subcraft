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
    BrushAlpha { id: AssetId::from_name(&format!("alpha/{name}")), name: name.into(), bitmap }
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
    ]
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
    })
}
