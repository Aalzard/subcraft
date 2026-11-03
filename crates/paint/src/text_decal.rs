//! Text → paint rasters. Two entry points share one shaping/rasterization path:
//!
//! * [`rasterize_decal`]: a UV-space decal rendered at the texture set's resolution straight from
//!   the glyph outlines (no intermediate bitmap, so it is as sharp at 8192² as at 512²).
//! * [`text_tip`]: the same text as a square alpha tip for the brush, so text can be stamped along
//!   strokes and projected through the camera with the normal stroke machinery.
use std::collections::BTreeMap;
use std::sync::Arc;

use glam::{Affine2, Vec2};
use subcraft_core::{ChannelKind, IRect};
use subcraft_doc::{TextDecal, empty_paint_image};
use subcraft_image::{Bitmap, TILE_SIZE, TILE_TEXELS, TiledImage};
use subcraft_text::{Font, TextError, TextStyle};

/// Pixel-space transform from layout em units for `decal` at `resolution`.
pub fn decal_transform(decal: &TextDecal, layout: &subcraft_text::TextLayout, resolution: u32) -> Affine2 {
    let res = resolution as f32;
    let box_center = Vec2::new(layout.width, layout.height) * 0.5;
    let (sin, cos) = decal.rotation.sin_cos();
    // UV y points down, so this angle is clockwise on screen.
    Affine2::from_translation(decal.center * res)
        * Affine2::from_mat2_translation(glam::Mat2::from_cols(Vec2::new(cos, sin), Vec2::new(-sin, cos)), Vec2::ZERO)
        * Affine2::from_scale(Vec2::splat(decal.size * res))
        * Affine2::from_translation(-box_center)
}

/// Rasterize `decal` into per-channel paint images of `resolution`². Only tiles the text touches
/// are allocated. `Ok(empty)` images when the text has no ink or lies outside the texture.
pub fn rasterize_decal(decal: &TextDecal, font: &Font, resolution: u32) -> Result<BTreeMap<ChannelKind, TiledImage>, TextError> {
    if resolution == 0 || resolution > 16384 {
        return Err(TextError::Invalid("invalid texture resolution"));
    }
    decal.validate().map_err(TextError::Invalid)?;
    let mut images: BTreeMap<_, _> = decal.values.keys().map(|&k| (k, empty_paint_image(k, resolution))).collect();
    let layout = subcraft_text::layout(font, &decal.style)?;
    let Some((lo, hi)) = layout.ink else { return Ok(images) };
    let xf = decal_transform(decal, &layout, resolution);

    // Pixel bounds of the transformed ink rectangle, clipped to the texture.
    let corners = [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)].map(|p| xf.transform_point2(p));
    let min = corners.iter().fold(Vec2::splat(f32::INFINITY), |a, &b| a.min(b));
    let max = corners.iter().fold(Vec2::splat(f32::NEG_INFINITY), |a, &b| a.max(b));
    let region = IRect::new(min.x.floor() as i32 - 2, min.y.floor() as i32 - 2, max.x.ceil() as i32 + 2, max.y.ceil() as i32 + 2)
        .intersect(&IRect::from_size(resolution, resolution));
    if region.is_empty() {
        return Ok(images);
    }
    let local = Affine2::from_translation(-Vec2::new(region.x0 as f32, region.y0 as f32)) * xf;
    let (w, h) = ((region.x1 - region.x0) as u32, (region.y1 - region.y0) as u32);
    let coverage = subcraft_text::rasterize(font, &layout, local, w, h)?;

    for (&kind, image) in &mut images {
        let n = kind.components();
        let comps = n + 1;
        let value = decal.values[&kind].0;
        for tile in image.tiles_in_rect(&region) {
            let rect = image.tile_rect(tile);
            let mut data = vec![0.0f32; TILE_TEXELS * comps];
            let mut any = false;
            for y in rect.y0.max(region.y0)..rect.y1.min(region.y1) {
                for x in rect.x0.max(region.x0)..rect.x1.min(region.x1) {
                    let a = coverage.get((x - region.x0) as u32, (y - region.y0) as u32) * decal.opacity;
                    let px = &mut data[(((y - rect.y0) as u32 * TILE_SIZE + (x - rect.x0) as u32) as usize) * comps..][..comps];
                    px[..n].copy_from_slice(&value[..n]);
                    px[n] = a;
                    any |= a > 0.0;
                }
            }
            if any {
                // Texels of the tile outside the region keep the channel's transparent default.
                let fill = image.fill();
                for (i, px) in data.chunks_exact_mut(comps).enumerate() {
                    let (tx, ty) = ((i as u32 % TILE_SIZE) as i32 + rect.x0, (i as u32 / TILE_SIZE) as i32 + rect.y0);
                    if tx < region.x0 || tx >= region.x1 || ty < region.y0 || ty >= region.y1 {
                        px.copy_from_slice(&fill[..comps]);
                    }
                }
                image.write_tile(tile, &data);
            }
        }
    }
    Ok(images)
}

/// The text as a square gray brush tip (white = paint), `size`² pixels, scaled to fit.
pub fn text_tip(font: &Font, style: &TextStyle, size: u32) -> Result<Arc<Bitmap>, TextError> {
    let layout = subcraft_text::layout(font, style)?;
    let cov = subcraft_text::rasterize_fit(font, &layout, size.clamp(16, 2048), 0.04)?;
    Ok(Arc::new(Bitmap::new_f32(cov.width, cov.height, 1, cov.data)))
}
