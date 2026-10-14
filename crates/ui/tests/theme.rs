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
