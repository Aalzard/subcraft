//! The embedded fonts cover every icon and the typographic characters the UI uses.

use egui::{FontFamily, FontId};
use subcraft_ui::{icons, theme};

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    // Fonts are (re)built at the start of a pass.
    let mut out = ctx.run_ui(egui::RawInput::default(), |_| {});
    out.textures_delta.clear();
    ctx
}

#[test]
fn every_icon_has_a_glyph_in_text_families() {
    let ctx = context();
    for family in [FontFamily::Proportional, theme::semibold()] {
        let font = FontId::new(13.0, family.clone());
        for icon in icons::ALL {
            let c = icon.chars().next().unwrap();
            assert!(ctx.fonts_mut(|f| f.has_glyph(&font, c)), "U+{:04X} missing in {family:?}", c as u32);
        }
    }
}

#[test]
fn ui_punctuation_is_covered() {
    let ctx = context();
    let font = FontId::new(13.0, FontFamily::Proportional);
    for c in ['²', '·', '…', '›', '°', '—', '|'] {
        assert!(ctx.fonts_mut(|f| f.has_glyph(&font, c)), "{c:?} missing");
    }
    // A Phosphor glyph that was not kept in the subset is really absent (the check is meaningful).
    let dropped = '\u{e0b0}';
    assert!(!ctx.fonts_mut(|f| f.has_glyph(&font, dropped)), "subset should not contain U+E0B0");
}

#[test]
fn app_icon_is_the_app_artwork() {
    let icon = theme::app_icon();
    assert_eq!((icon.width, icon.height), (128, 128));
    assert_eq!(icon.rgba.len(), 128 * 128 * 4);
    let px = |x: usize, y: usize| &icon.rgba[(y * 128 + x) * 4..(y * 128 + x) * 4 + 4];
    // A flat orange square (#FF7800) with the white ribbon-star mark; no rounded corners.
    let corner = px(0, 0);
    assert!(corner[3] == 255 && corner[0] >= 250 && (110..=130).contains(&corner[1]) && corner[2] < 12, "flat orange corner: {corner:?}");
    let bg = px(64, 6);
    assert!(bg[3] == 255 && bg[0] > 230 && bg[1] > 80 && bg[1] < 140 && bg[2] < 40, "orange background: {bg:?}");
    let mark = px(64, 90); // inside the lower ribbon of the S
    assert!(mark[0] > 240 && mark[1] > 240 && mark[2] > 240, "white mark: {mark:?}");
}

#[test]
fn header_logo_is_a_small_square_texture_that_is_cached() {
    let ctx = context();
    let first = theme::brand_mark(&ctx);
    let second = theme::brand_mark(&ctx);
    assert_eq!(first.id(), second.id(), "uploaded once per context");
    assert_eq!(first.size(), [96, 96]);
}

#[test]
fn the_tinted_palette_is_still_dark_and_text_keeps_its_contrast() {
    // HSL lightness (0..1) of a colour.
    let lightness = |c: egui::Color32| (f32::from(c.r().max(c.g()).max(c.b())) + f32::from(c.r().min(c.g()).min(c.b()))) / 510.0;
    for (name, c) in [("BG_0", theme::BG_0), ("BG_1", theme::BG_1), ("BG_2", theme::BG_2), ("BG_3", theme::BG_3), ("BG_4", theme::BG_4),
        ("FIELD", theme::FIELD), ("CARD", theme::CARD), ("BORDER", theme::BORDER), ("BORDER_STRONG", theme::BORDER_STRONG),
        ("ROW_BAND", theme::ROW_BANDS[1]), ("SECTION_BAND", theme::SECTION_BANDS[1])] {
        assert!(lightness(c) < 0.36, "{name} {c:?} must stay clearly dark");
        let sat = { let (mx, mn) = (f32::from(c.r().max(c.g()).max(c.b())), f32::from(c.r().min(c.g()).min(c.b()))); (mx - mn) / (mx + mn) };
        assert!(sat < 0.12, "{name} {c:?} stays neutral (saturation {sat:.3})");
    }
    // The panel grey the user picked (#2a2a2a to #2b2b2b, lightness ~0.165) is within a few levels of BG_1/BG_2.
    assert!(lightness(theme::BG_1) > 0.12 && lightness(theme::BG_2) < 0.20);
}

#[test]
fn charcoal_theme_has_readable_muted_text_and_thin_round_sliders() {
    let ctx = context();
    let style = ctx.style_of(egui::Theme::Dark);
    assert!(matches!(style.visuals.handle_shape, egui::style::HandleShape::Circle));
    assert_eq!(style.spacing.slider_rail_height, 4.0);
    let luminance = |c: egui::Color32| {
        let linear = |v: u8| { let s = f32::from(v) / 255.0; if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) } };
        0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
    };
    for bg in [theme::BG_1, theme::ROW_BANDS[1], theme::ACCENT_SOFT] {
        for fg in [theme::TEXT, theme::TEXT_DIM] {
            assert!((luminance(fg) + 0.05) / (luminance(bg) + 0.05) >= 4.5, "small labels remain legible");
        }
    }
    // The charcoal carries a faint warm tint (hue ~25 degrees, ~5% saturation): red leads blue by a
    // few levels, never enough to read as brown or orange.
    for bg in [theme::BG_0, theme::BG_1, theme::BG_2, theme::BG_3, theme::BG_4] {
        let spread = i16::from(bg.r()) - i16::from(bg.b());
        assert!((1..=8).contains(&spread), "surface {bg:?} is tinted slightly towards orange, got spread {spread}");
    }
    let selection = &style.visuals.selection;
    let a = luminance(selection.bg_fill);
    let b = luminance(selection.stroke.color);
    assert!((a.max(b) + 0.05) / (a.min(b) + 0.05) >= 4.5, "selected channel and menu labels must remain readable");
}
