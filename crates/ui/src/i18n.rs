//! Interface language. English text is the key: `tr("Reset camera")` returns the English text
//! itself when the language is English or the text has no translation, so code can be wrapped
//! one string at a time and untranslated strings simply stay English.
//!
//! * Strings are wrapped where they are drawn (`ui.button(tr("Import"))`), every frame, so a
//!   language change applies at once without a restart.
//! * Texts with values use [`trf`]: `trf("Baked {} map(s)", &[&n])` replaces `{}` in order. A
//!   translation must contain the same placeholders as its English key (tested).
//! * The tables live in `i18n/ko.rs` and `i18n/zh_hant.rs`. Every key must be written exactly as
//!   in a `tr("...")` / `trf("...")` call in this crate (tested), and every wrapped key has an
//!   entry in each table (tested), so a typo cannot silently leave a string in English.
//! * CJK text needs a font with Hangul / Han glyphs. SubCraft does not bundle one (they are
//!   many MB); [`install_font`] finds the system's (Malgun Gothic and Microsoft JhengHei on
//!   Windows, Apple SD Gothic / PingFang on macOS, Noto Sans CJK on Linux, or the file named by
//!   `SUBCRAFT_CJK_FONT`) and adds it as a fallback behind Inter and the icon font.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

mod ko;
mod zh_hant;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    English,
    /// Traditional Chinese (needs a native-speaker review).
    ZhHant,
    /// Korean (needs a native-speaker review).
    Ko,
}

impl Language {
    pub const ALL: [Language; 3] = [Language::English, Language::ZhHant, Language::Ko];

    /// The language's own name, as shown in the picker (always in its own script).
    pub fn native_name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::ZhHant => "繁體中文",
            Language::Ko => "한국어",
        }
    }

    fn table(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Language::English => &[],
            Language::ZhHant => zh_hant::TABLE,
            Language::Ko => ko::TABLE,
        }
    }

    fn from_u8(v: u8) -> Self {
        match v { 1 => Language::ZhHant, 2 => Language::Ko, _ => Language::English }
    }

    fn to_u8(self) -> u8 {
        match self { Language::English => 0, Language::ZhHant => 1, Language::Ko => 2 }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn language() -> Language {
    Language::from_u8(CURRENT.load(Ordering::Relaxed))
}

/// Switch the language used by [`tr`] (no fonts are touched; see [`sync`]).
pub fn set_language(language: Language) {
    CURRENT.store(language.to_u8(), Ordering::Relaxed);
}

fn lookup(language: Language) -> &'static HashMap<&'static str, &'static str> {
    static MAPS: [OnceLock<HashMap<&'static str, &'static str>>; 3] = [OnceLock::new(), OnceLock::new(), OnceLock::new()];
    MAPS[language.to_u8() as usize].get_or_init(|| language.table().iter().copied().collect())
}

/// `text` in the current language, or `text` itself when it is not translated.
pub fn tr(text: &'static str) -> &'static str {
    translate(language(), text)
}

/// `text` in `language` (the text itself for English or when untranslated).
pub fn translate(language: Language, text: &'static str) -> &'static str {
    match language {
        Language::English => text,
        l => lookup(l).get(text).copied().unwrap_or(text),
    }
}

/// [`tr`] for a text with `{}` placeholders, filled in order from `args`.
pub fn trf(text: &'static str, args: &[&dyn std::fmt::Display]) -> String {
    fill(tr(text), args)
}

fn fill(template: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::with_capacity(template.len() + 8 * args.len());
    let mut rest = template;
    let mut next = args.iter();
    while let Some(at) = rest.find("{}") {
        out.push_str(&rest[..at]);
        if let Some(arg) = next.next() { out.push_str(&arg.to_string()); } else { out.push_str("{}"); }
        rest = &rest[at + 2..];
    }
    out.push_str(rest);
    out
}

/// The placeholders (`{}`, `{n}`...) of a text, in order.
pub fn placeholders(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        let Some(len) = rest[start..].find('}') else { break };
        out.push(rest[start..start + len + 1].to_string());
        rest = &rest[start + len + 1..];
    }
    out
}

/// Fonts that carry the glyphs of `language`: (path, collection index), best first.
fn font_candidates(language: Language) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    if let Ok(path) = std::env::var("SUBCRAFT_CJK_FONT") {
        out.push((path, 0));
    }
    let list: &[(&str, u32)] = match language {
        Language::English => &[],
        Language::Ko => &[
            ("C:\\Windows\\Fonts\\malgun.ttf", 0),
            ("/System/Library/Fonts/AppleSDGothicNeo.ttc", 0),
            ("/System/Library/Fonts/Supplemental/AppleGothic.ttf", 0),
            ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 1),
            ("/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc", 1),
            ("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc", 1),
            ("/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc", 1),
            ("/usr/share/fonts/truetype/nanum/NanumGothic.ttf", 0),
        ],
        Language::ZhHant => &[
            ("C:\\Windows\\Fonts\\msjh.ttc", 0),
            ("C:\\Windows\\Fonts\\mingliu.ttc", 0),
            ("/System/Library/Fonts/PingFang.ttc", 0),
            ("/System/Library/Fonts/STHeiti Medium.ttc", 0),
            ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 3),
            ("/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc", 3),
            ("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc", 3),
            ("/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc", 3),
            ("/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc", 0),
        ],
    };
    out.extend(list.iter().map(|(p, i)| (p.to_string(), *i)));
    out
}

/// Read the first usable CJK font for `language`.
fn find_font(language: Language) -> Option<(String, Vec<u8>, u32)> {
    font_candidates(language).into_iter().find_map(|(path, index)| {
        let bytes = std::fs::read(&path).ok()?;
        Some((path, bytes, index))
    })
}

/// Whether a font for `language` is installed on this computer (English needs none).
pub fn font_available(language: Language) -> bool {
    language == Language::English || font_candidates(language).iter().any(|(p, _)| std::path::Path::new(p).is_file())
}

/// Languages whose CJK fonts are added as fallbacks, best first: the interface language's, then,
/// while the language picker can be shown (Preferences open), every other one so the picker can
/// draw each language's native name (한국어, 繁體中文) even in English.
pub fn fallback_languages(language: Language, picker: bool) -> Vec<Language> {
    let mut out = Vec::new();
    if language != Language::English { out.push(language); }
    if picker {
        out.extend(Language::ALL.into_iter().filter(|l| *l != Language::English && *l != language));
    }
    out
}

/// Install the egui fonts for `language`: the interface fonts, plus CJK fallbacks (see
/// [`fallback_languages`]). Returns false when the interface language's CJK font was not found
/// (its text then shows boxes); a missing font for a picker-only name is just logged.
pub fn install_font(ctx: &egui::Context, language: Language) -> bool {
    install_fonts(ctx, language, false)
}

pub fn install_fonts(ctx: &egui::Context, language: Language, picker: bool) -> bool {
    let mut fonts = crate::theme::fonts();
    let mut found = true;
    let mut loaded: Vec<String> = Vec::new();
    for (n, wanted) in fallback_languages(language, picker).into_iter().enumerate() {
        match find_font(wanted) {
            Some((path, bytes, index)) => {
                if loaded.contains(&path) { continue; } // one file can cover several languages
                log::info!("interface font for {wanted:?}: {path} (face {index})");
                let mut data = egui::FontData::from_owned(bytes);
                data.index = index;
                let name = if n == 0 { "cjk".to_string() } else { format!("cjk-{n}") };
                fonts.font_data.insert(name.clone(), std::sync::Arc::new(data));
                for family in fonts.families.values_mut() {
                    family.push(name.clone());
                }
                loaded.push(path);
            }
            None if wanted == language => {
                log::warn!("no system font with {language:?} glyphs found; set SUBCRAFT_CJK_FONT to a .ttf/.ttc file");
                found = false;
            }
            None => log::info!("no system font for the {wanted:?} name in the language picker"),
        }
    }
    ctx.set_fonts(fonts);
    found
}

static APPLIED: AtomicU8 = AtomicU8::new(u8::MAX);

/// Make `language` the interface language: switch the texts and, when the language changed,
/// install its fonts. Cheap to call every frame; returns false only when a CJK font is missing.
pub fn sync(ctx: &egui::Context, language: Language) -> bool {
    sync_fonts(ctx, language, false)
}

/// Like [`sync`], and `picker` (the language picker may be shown) also loads the other languages'
/// fonts. Once loaded they stay until the language changes, so opening and closing Preferences does
/// not rebuild the font atlas each time; English without the picker loads no CJK font at all.
pub fn sync_fonts(ctx: &egui::Context, language: Language, picker: bool) -> bool {
    set_language(language);
    const PICKER: u8 = 0x10;
    let previous = APPLIED.load(Ordering::Relaxed);
    let keep_picker = previous != u8::MAX && previous & !PICKER == language.to_u8() && previous & PICKER != 0;
    let key = language.to_u8() | if picker || keep_picker { PICKER } else { 0 };
    if previous == key {
        return true;
    }
    APPLIED.store(key, Ordering::Relaxed);
    install_fonts(ctx, language, key & PICKER != 0)
}

#[cfg(test)]
mod tests;
