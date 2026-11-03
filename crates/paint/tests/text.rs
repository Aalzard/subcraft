//! Text decal rasterization into paint rasters: placement, scaling with resolution, rotation,
//! sparsity, opacity and the brush tip.

use std::collections::BTreeMap;
use std::sync::Arc;

use glam::Vec2;
use subcraft_core::{ChannelKind, ChannelValue};
use subcraft_doc::TextDecal;
use subcraft_image::TiledImage;
use subcraft_paint::text_decal::{rasterize_decal, text_tip};
use subcraft_text::{BUNDLED_FONTS, Font};

fn font() -> Font {
    Font::from_bytes(Arc::new(BUNDLED_FONTS[0].1.to_vec()), 0).unwrap()
}

fn decal(text: &str) -> TextDecal {
    let mut values = BTreeMap::new();
    values.insert(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.25, 0.0));
    let mut d = TextDecal::new(text, Vec2::splat(0.5), values);
    d.size = 0.1;
    d
}

fn alpha(img: &TiledImage) -> Vec<f32> {
    let c = img.comps();
    img.to_dense().chunks_exact(c).map(|p| p[c - 1]).collect()
}

fn bounds(img: &TiledImage) -> (u32, u32, u32, u32) {
    let a = alpha(img);
    let w = img.width();
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (i, &v) in a.iter().enumerate() {
        if v > 0.02 {
            let (x, y) = (i as u32 % w, i as u32 / w);
            x0 = x0.min(x); y0 = y0.min(y); x1 = x1.max(x + 1); y1 = y1.max(y + 1);
        }
    }
    (x0, y0, x1, y1)
}

fn base(d: &TextDecal, res: u32) -> TiledImage {
    rasterize_decal(d, &font(), res).unwrap().remove(&ChannelKind::BaseColor).unwrap()
}

#[test]
fn capital_h_has_the_expected_height_and_sits_on_the_centre() {
    let res = 512;
    let img = base(&decal("H"), res);
    let (x0, y0, x1, y1) = bounds(&img);
    // DejaVu Sans cap height is 0.729 em; size 0.1 of 512 px = 51.2 px per em.
    let cap = (y1 - y0) as f32;
    assert!((cap - 0.729 * 0.1 * res as f32).abs() <= 2.5, "cap height {cap}");
    // Horizontal centre of the ink is the centre of the glyph box (±) at texture centre.
    assert!((((x0 + x1) / 2) as f32 - 256.0).abs() <= 3.0, "x {x0} {x1}");
    // The layout box is centred, so the ink (cap-height) lies slightly above the box centre.
    assert!(((y0 + y1) / 2) < 256 + 4 && ((y0 + y1) / 2) > 256 - 30, "y {y0} {y1}");
    // Colour is carried at the covered texels, alpha is straight coverage.
    let px = img.get((x0 + x1) / 2 - 8, (y0 + y1) / 2); // inside the left stem region
    assert!(px[3] >= 0.0);
    let solid = (y0..y1).flat_map(|y| (x0..x1).map(move |x| (x, y))).map(|(x, y)| img.get(x, y)).find(|p| p[3] > 0.999).unwrap();
    assert_eq!(&solid[..3], &[1.0, 0.25, 0.0]);
}

#[test]
fn doubling_the_resolution_doubles_the_extent_and_keeps_the_text_sharp() {
    let a = base(&decal("Sharp"), 256);
    let b = base(&decal("Sharp"), 512);
    let (ax0, ay0, ax1, ay1) = bounds(&a);
    let (bx0, by0, bx1, by1) = bounds(&b);
    assert!((((bx1 - bx0) as f32) / ((ax1 - ax0) as f32) - 2.0).abs() < 0.05);
    assert!((((by1 - by0) as f32) / ((ay1 - ay0) as f32) - 2.0).abs() < 0.08);
    let sum = |i: &TiledImage| alpha(i).iter().map(|&v| v as f64).sum::<f64>();
    assert!((sum(&b) / sum(&a) - 4.0).abs() < 0.12, "{}", sum(&b) / sum(&a));
    // Sharper than resampling: a bilinear 2x upscale of the 256 raster has no fully covered
    // texel ratio advantage; the direct raster has crisp edges (many texels at exactly 0 or ~1).
    let crisp = |i: &TiledImage| { let v = alpha(i); let edge = v.iter().filter(|&&x| x > 0.02 && x < 0.98).count() as f64; edge / v.iter().filter(|&&x| x > 0.02).count() as f64 };
    let up = a.resampled(512, 512);
    assert!(crisp(&b) < crisp(&up), "direct {} vs upscaled {}", crisp(&b), crisp(&up));
}

#[test]
fn rotation_swaps_width_and_height_and_moves_with_the_centre() {
    let res = 512;
    let mut d = decal("LONG");
    let (x0, y0, x1, y1) = bounds(&base(&d, res));
    d.rotation = std::f32::consts::FRAC_PI_2;
    d.center = Vec2::new(0.25, 0.75);
    let (rx0, ry0, rx1, ry1) = bounds(&base(&d, res));
    assert!(((rx1 - rx0) as i32 - (y1 - y0) as i32).abs() <= 2 && ((ry1 - ry0) as i32 - (x1 - x0) as i32).abs() <= 2);
    // Rotated text is centred (box centre) on the new UV position.
    assert!((((rx0 + rx1) / 2) as f32 - 0.25 * res as f32).abs() <= 40.0);
    assert!((((ry0 + ry1) / 2) as f32 - 0.75 * res as f32).abs() <= 12.0);
}

#[test]
fn only_touched_tiles_are_allocated_and_off_texture_text_is_empty() {
    let img = base(&decal("i"), 1024);
    assert!(img.allocated_tiles() > 0 && img.allocated_tiles() < img.tile_count() / 8, "{} of {}", img.allocated_tiles(), img.tile_count());
    let mut d = decal("gone");
    d.center = Vec2::new(3.0, 3.0);
    let out = rasterize_decal(&d, &font(), 256).unwrap();
    assert_eq!(out[&ChannelKind::BaseColor].allocated_tiles(), 0);
    // Text running over the edge is clipped, not an error.
    d.center = Vec2::new(0.99, 0.5);
    d.size = 0.3;
    let clipped = base(&d, 256);
    assert!(bounds(&clipped).2 == 256);
}

#[test]
fn opacity_scales_alpha_and_extra_channels_follow_their_values() {
    let mut d = decal("O");
    d.opacity = 0.5;
    d.values.insert(ChannelKind::Roughness, ChannelValue::scalar(0.2));
    let out = rasterize_decal(&d, &font(), 256).unwrap();
    let rough = &out[&ChannelKind::Roughness];
    let maxa = alpha(rough).iter().cloned().fold(0.0, f32::max);
    assert!((maxa - 0.5).abs() < 0.01, "{maxa}");
    assert!(rough.to_dense().chunks_exact(2).any(|p| p[1] > 0.4 && (p[0] - 0.2).abs() < 1e-3));
    assert_eq!(out.len(), 2);
}

#[test]
fn invalid_decals_are_rejected() {
    let mut d = decal("x");
    d.size = 100.0;
    assert!(rasterize_decal(&d, &font(), 128).is_err());
    d.size = 0.1;
    d.values.clear();
    assert!(rasterize_decal(&d, &font(), 128).is_err());
    assert!(rasterize_decal(&decal("x"), &font(), 0).is_err());
}

#[test]
fn arabic_decals_render_ink() {
    let img = base(&decal("مرحبا"), 512);
    let (x0, _, x1, _) = bounds(&img);
    assert!(x1 > x0 + 40);
}

#[test]
fn text_tip_is_a_square_gray_tip_scaled_to_fit() {
    let style = subcraft_text::TextStyle::new("Tip");
    let tip = text_tip(&font(), &style, 256).unwrap();
    assert_eq!((tip.width, tip.height), (256, 256));
    let v: Vec<f32> = (0..256 * 256).map(|i| tip.texel(i % 256, i / 256, false)[0]).collect();
    assert!(v.iter().cloned().fold(0.0, f32::max) > 0.99 && v.iter().cloned().fold(1.0, f32::min) == 0.0);
    // Border stays empty (padding).
    assert!((0..256).all(|i| tip.texel(i, 0, false)[0] == 0.0 && tip.texel(0, i, false)[0] == 0.0));
}
