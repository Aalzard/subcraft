use std::sync::Arc;

use glam::{Affine2, Vec2};
use rustybuzz::ttf_parser::{GlyphId, OutlineBuilder};

use super::*;

fn sans() -> Font {
    Font::from_bytes(Arc::new(BUNDLED_FONTS[0].1.to_vec()), 0).unwrap()
}

fn style(text: &str) -> TextStyle {
    TextStyle::new(text)
}

/// Independent reference: flatten the glyph outlines and sum the shoelace area (em²).
fn outline_area(font: &Font, l: &TextLayout) -> f64 {
    struct Area { area: f64, last: (f64, f64), start: (f64, f64) }
    impl Area {
        fn edge(&mut self, p: (f64, f64)) {
            self.area += self.last.0 * p.1 - p.0 * self.last.1;
            self.last = p;
        }
    }
    impl OutlineBuilder for Area {
        fn move_to(&mut self, x: f32, y: f32) { self.last = (x as f64, y as f64); self.start = self.last; }
        fn line_to(&mut self, x: f32, y: f32) { self.edge((x as f64, y as f64)); }
        fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
            let p0 = self.last;
            for i in 1..=64 {
                let t = i as f64 / 64.0;
                let (a, b, c) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t);
                self.edge((a * p0.0 + b * x1 as f64 + c * x as f64, a * p0.1 + b * y1 as f64 + c * y as f64));
            }
        }
        fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
            let p0 = self.last;
            for i in 1..=64 {
                let t = i as f64 / 64.0;
                let u = 1.0 - t;
                let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                self.edge((a * p0.0 + b * x1 as f64 + c * x2 as f64 + d * x as f64, a * p0.1 + b * y1 as f64 + c * y2 as f64 + d * y as f64));
            }
        }
        fn close(&mut self) { let s = self.start; self.edge(s); }
    }
    font.with_face(|face| {
        let upem = face.units_per_em() as f64;
        l.glyphs.iter().map(|g| {
            let mut a = Area { area: 0.0, last: (0.0, 0.0), start: (0.0, 0.0) };
            face.outline_glyph(GlyphId(g.id), &mut a);
            (a.area * 0.5).abs() / (upem * upem)
        }).sum()
    }).unwrap()
}

fn raster_scale(font: &Font, l: &TextLayout, px: f32, pad: f32) -> Coverage {
    let (lo, hi) = l.bounds();
    let w = ((hi.x - lo.x) * px + 2.0 * pad).ceil() as u32;
    let h = ((hi.y - lo.y) * px + 2.0 * pad).ceil() as u32;
    let xf = Affine2::from_translation(Vec2::splat(pad) - lo * px) * Affine2::from_scale(Vec2::splat(px));
    rasterize(font, l, xf, w, h).unwrap()
}

#[test]
fn bundled_fonts_parse_and_name_themselves() {
    let fonts = FontLibrary::bundled().unwrap();
    assert_eq!(fonts.len(), BUNDLED_FONTS.len());
    assert_eq!(fonts[0].1.family_name().as_deref(), Some("DejaVu Sans"));
    assert_eq!(Font::from_bytes(Arc::new(vec![1, 2, 3, 4]), 0).unwrap_err(), TextError::BadFont);
}

#[test]
fn coverage_matches_independent_outline_area() {
    let f = sans();
    // Mixed straight and curved glyphs: an "I" (rectangle), "O", "g" (cubic/quad outlines).
    for s in ["I", "O", "Hg8"] {
        let l = layout(&f, &style(s)).unwrap();
        let reference = outline_area(&f, &l);
        let cov = raster_scale(&f, &l, 200.0, 4.0);
        let measured = cov.sum() / (200.0 * 200.0);
        assert!((measured - reference).abs() / reference < 0.01, "{s}: measured {measured} vs reference {reference}");
    }
}

#[test]
fn ink_bounds_match_pixel_bounds() {
    let f = sans();
    let l = layout(&f, &style("Hello")).unwrap();
    let px = 100.0;
    let cov = raster_scale(&f, &l, px, 5.0);
    let (x0, y0, x1, y1) = cov.bounds(0.02).unwrap();
    let (lo, hi) = l.ink.unwrap();
    // The raster was placed so that `lo` lands at (5, 5).
    assert!((x0 as f32 - 5.0).abs() <= 1.0 && (y0 as f32 - 5.0).abs() <= 1.0, "{x0} {y0}");
    assert!(((x1 - x0) as f32 - (hi.x - lo.x) * px).abs() <= 2.0);
    assert!(((y1 - y0) as f32 - (hi.y - lo.y) * px).abs() <= 2.0);
}

#[test]
fn scaling_doubles_extent_and_quadruples_area() {
    let f = sans();
    let l = layout(&f, &style("Rg")).unwrap();
    let a = raster_scale(&f, &l, 80.0, 3.0);
    let b = raster_scale(&f, &l, 160.0, 3.0);
    assert!((b.sum() / a.sum() - 4.0).abs() < 0.04, "{}", b.sum() / a.sum());
    let (ax0, ay0, ax1, ay1) = a.bounds(0.02).unwrap();
    let (bx0, by0, bx1, by1) = b.bounds(0.02).unwrap();
    assert!((((bx1 - bx0) as f32) / ((ax1 - ax0) as f32) - 2.0).abs() < 0.05);
    assert!((((by1 - by0) as f32) / ((ay1 - ay0) as f32) - 2.0).abs() < 0.05);
}

#[test]
fn rotation_by_90_degrees_is_an_exact_transpose_of_the_flat_render() {
    let f = sans();
    let l = layout(&f, &style("Tes\nt")).unwrap();
    let n = 320u32;
    let flat_xf = Affine2::from_translation(Vec2::new(20.0, 30.0)) * Affine2::from_scale(Vec2::splat(90.0));
    let flat = rasterize(&f, &l, flat_xf, n, n).unwrap();
    // (x, y) -> (-y, x) is +90° in y-down pixel space; shifting by n maps the canvas onto itself,
    // so flat pixel (i, j) must equal rotated pixel (n-1-j, i).
    let rot_xf = Affine2::from_translation(Vec2::new(n as f32, 0.0)) * Affine2::from_angle(std::f32::consts::FRAC_PI_2) * flat_xf;
    let rot = rasterize(&f, &l, rot_xf, n, n).unwrap();
    let mut worst = 0.0f32;
    for j in 0..n { for i in 0..n { worst = worst.max((flat.get(i, j) - rot.get(n - 1 - j, i)).abs()); } }
    assert!(worst < 0.02, "worst {worst}");
    assert!(flat.sum() > 500.0);
    let (x0, y0, x1, y1) = flat.bounds(0.02).unwrap();
    let (rx0, ry0, rx1, ry1) = rot.bounds(0.02).unwrap();
    assert_eq!((rx1 - rx0, ry1 - ry0), (y1 - y0, x1 - x0), "extent swaps");
    // 45° keeps the area (rotation preserves it) while the bounds grow.
    let r45 = rasterize(&f, &l, Affine2::from_translation(Vec2::new(160.0, 0.0)) * Affine2::from_angle(std::f32::consts::FRAC_PI_4) * flat_xf, n, n).unwrap();
    assert!((r45.sum() / flat.sum() - 1.0).abs() < 0.02, "{} vs {}", r45.sum(), flat.sum());
}

#[test]
fn clipping_at_the_raster_edge_equals_a_crop_of_the_full_render() {
    let f = sans();
    let l = layout(&f, &style("Wide clip")).unwrap();
    let px = 60.0;
    let full = raster_scale(&f, &l, px, 4.0);
    let (lo, _) = l.bounds();
    let (cx, cy, cw, ch) = (full.width / 3, 0u32, full.width / 3, full.height);
    let xf = Affine2::from_translation(Vec2::new(-(cx as f32), 0.0)) * Affine2::from_translation(Vec2::splat(4.0)) * Affine2::from_scale(Vec2::splat(px)) * Affine2::from_translation(-lo);
    let crop = rasterize(&f, &l, xf, cw, ch).unwrap();
    let mut worst = 0.0f32;
    for y in 0..ch { for x in 0..cw { worst = worst.max((crop.get(x, y) - full.get(x + cx, y + cy)).abs()); } }
    assert!(worst < 0.02, "worst difference {worst}");
    assert!(crop.sum() > 0.0);
}

#[test]
fn alignment_places_lines_left_center_right() {
    let f = sans();
    let line_spans = |align: Align| {
        let mut s = style("WWWWWWWW\nW");
        s.align = align;
        let l = layout(&f, &s).unwrap();
        let px = 100.0;
        let cov = raster_scale(&f, &l, px, 2.0);
        // Rows of the second line: below the first line's baseline + descender region.
        let split = ((l.line_height * 0.9) * px) as u32 + 2;
        let mut first = (u32::MAX, 0u32);
        let mut second = (u32::MAX, 0u32);
        for y in 0..cov.height { for x in 0..cov.width {
            if cov.get(x, y) > 0.05 {
                let r = if y < split { &mut first } else { &mut second };
                r.0 = r.0.min(x); r.1 = r.1.max(x + 1);
            }
        } }
        (first, second)
    };
    let (f1, s1) = line_spans(Align::Left);
    assert!((f1.0 as i32 - s1.0 as i32).abs() <= 8, "left edges {f1:?} {s1:?}");
    let (f2, s2) = line_spans(Align::Right);
    assert!((f2.1 as i32 - s2.1 as i32).abs() <= 8, "right edges {f2:?} {s2:?}");
    let (f3, s3) = line_spans(Align::Center);
    let (c1, c2) = ((f3.0 + f3.1) as i32 / 2, (s3.0 + s3.1) as i32 / 2);
    assert!((c1 - c2).abs() <= 6, "centres {c1} {c2}");
    assert!(s1.1 - s1.0 < f1.1 - f1.0);
}

#[test]
fn letter_and_line_spacing_change_the_layout_exactly() {
    let f = sans();
    let base = layout(&f, &style("abcd")).unwrap();
    let mut s = style("abcd");
    s.letter_spacing = 0.25;
    let wide = layout(&f, &s).unwrap();
    assert!((wide.width - base.width - 4.0 * 0.25).abs() < 1e-4);
    assert_eq!(base.glyphs.len(), 4);
    let two = layout(&f, &style("a\nb")).unwrap();
    let mut s2 = style("a\nb");
    s2.line_spacing = 2.0;
    let spaced = layout(&f, &s2).unwrap();
    assert_eq!(two.lines, 2);
    assert!((spaced.line_height / two.line_height - 2.0).abs() < 1e-5);
    assert!((spaced.glyphs[1].y - spaced.glyphs[0].y - spaced.line_height).abs() < 1e-5);
}

#[test]
fn arabic_is_shaped_joined_and_laid_out_right_to_left() {
    let f = sans();
    let text = "مرحبا";
    let l = layout(&f, &style(text)).unwrap();
    assert_eq!(l.missing, 0, "the bundled font has Arabic");
    // Contextual shaping: joined forms differ from the isolated (cmap) glyphs.
    let nominal: Vec<u16> = f.with_face(|face| text.chars().map(|c| face.glyph_index(c).unwrap().0).collect()).unwrap();
    let mut shaped: Vec<u16> = l.glyphs.iter().map(|g| g.id).collect();
    let mut sorted_nominal = nominal.clone();
    shaped.sort();
    sorted_nominal.sort();
    assert_ne!(shaped, sorted_nominal, "joining forms must be selected by the shaper");
    // Visual order is right to left: the first character is the rightmost glyph.
    let first_logical = l.glyphs.iter().min_by_key(|g| g.cluster).unwrap();
    let last_logical = l.glyphs.iter().max_by_key(|g| g.cluster).unwrap();
    assert!(first_logical.x > last_logical.x, "{first_logical:?} {last_logical:?}");
    // Letter spacing must not break the cursive joins.
    let mut s = style(text);
    s.letter_spacing = 0.5;
    assert!((layout(&f, &s).unwrap().width - l.width).abs() < 1e-5);
    // And it renders ink.
    assert!(raster_scale(&f, &l, 80.0, 2.0).sum() > 100.0);
}

#[test]
fn mixed_direction_text_keeps_latin_runs_left_to_right() {
    let f = sans();
    let l = layout(&f, &style("abc مرحبا")).unwrap();
    // Auto direction picks LTR from "a": the Latin letters come first, the Arabic word is to the right.
    let a = l.glyphs.iter().find(|g| g.cluster == 0).unwrap();
    let arabic_x: Vec<f32> = l.glyphs.iter().filter(|g| g.cluster >= 4).map(|g| g.x).collect();
    assert!(arabic_x.iter().all(|&x| x > a.x));
    // Forcing RTL puts the Latin run on the right.
    let mut s = style("abc مرحبا");
    s.direction = Direction::Rtl;
    let r = layout(&f, &s).unwrap();
    let a = r.glyphs.iter().find(|g| g.cluster == 0).unwrap();
    let arabic_x: Vec<f32> = r.glyphs.iter().filter(|g| g.cluster >= 4).map(|g| g.x).collect();
    assert!(arabic_x.iter().all(|&x| x < a.x));
}

#[test]
fn missing_characters_are_reported_and_blank_text_is_valid() {
    let f = sans();
    assert!(layout(&f, &style("a\u{4E2D}")).unwrap().missing >= 1);
    let blank = layout(&f, &style("   ")).unwrap();
    assert!(blank.ink.is_none() && blank.width > 0.0);
    let empty = layout(&f, &style("")).unwrap();
    assert_eq!((empty.glyphs.len(), empty.lines), (0, 1));
    let cov = rasterize_fit(&f, &empty, 32, 0.1).unwrap();
    assert_eq!(cov.sum(), 0.0);
}

#[test]
fn rasterize_fit_centres_and_respects_padding() {
    let f = sans();
    let l = layout(&f, &style("Fit me")).unwrap();
    let c = rasterize_fit(&f, &l, 128, 0.1).unwrap();
    let (x0, y0, x1, y1) = c.bounds(0.02).unwrap();
    assert!(x0 >= 12 && y0 >= 12 && x1 <= 128 - 12 && y1 <= 128 - 12, "{x0} {y0} {x1} {y1}");
    assert!((x0 + x1).abs_diff(128) <= 3 && (y0 + y1).abs_diff(128) <= 3);
    assert!(x1 - x0 >= 100 || y1 - y0 >= 100);
}

#[test]
fn rasterization_is_deterministic_and_validates_input() {
    let f = sans();
    let l = layout(&f, &style("Det")).unwrap();
    assert_eq!(raster_scale(&f, &l, 50.0, 2.0), raster_scale(&f, &l, 50.0, 2.0));
    let mut bad = style("x");
    bad.letter_spacing = f32::NAN;
    assert!(layout(&f, &bad).is_err());
    assert_eq!(layout(&f, &style(&"x".repeat(MAX_TEXT_CHARS + 1))).unwrap_err(), TextError::TooLong(MAX_TEXT_CHARS));
    assert_eq!(rasterize(&f, &l, Affine2::IDENTITY, 100_000, 100_000).unwrap_err(), TextError::TooLarge);
    assert!(rasterize(&f, &l, Affine2::from_scale(Vec2::splat(f32::NAN)), 8, 8).is_err());
}

#[test]
fn library_resolves_bundled_system_and_embedded() {
    let lib = FontLibrary::new();
    let none = |_| None;
    assert!(lib.resolve(&FontRef::default(), &none).is_ok());
    // Unknown bundled / system names fall back to the default font instead of failing.
    assert!(lib.resolve(&FontRef::Bundled { name: "nope".into() }, &none).is_ok());
    assert!(lib.resolve(&FontRef::System { family: "No Such Family 12345".into(), bold: false, italic: false }, &none).is_ok());
    let id = subcraft_core::ResourceId::new();
    assert!(matches!(lib.resolve(&FontRef::Embedded { resource: id }, &none), Err(TextError::FontNotFound(_))));
    let bytes = Arc::new(BUNDLED_FONTS[2].1.to_vec());
    let serif = lib.resolve(&FontRef::Embedded { resource: id }, &|r| (r == id).then(|| bytes.clone())).unwrap();
    assert_eq!(serif.family_name().as_deref(), Some("DejaVu Serif"));
    // Serde round trip of the persistent identity.
    let json = serde_json::to_string(&FontRef::Embedded { resource: id }).unwrap();
    assert_eq!(serde_json::from_str::<FontRef>(&json).unwrap(), FontRef::Embedded { resource: id });
}
