use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::*;

/// Texts that are the same in every language (names, file suffixes).
const SAME_EVERYWHERE: &[&str] = &["SubCraft", "_high", "_low", "Blender"];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "i18n") { continue; }
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") && !path.ends_with("i18n.rs") {
            out.push(path);
        }
    }
}

/// Every string literal passed straight to `tr("...")` or `trf("...", ...)` in this crate.
fn wrapped_keys() -> BTreeSet<String> {
    let mut files = Vec::new();
    rust_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
    let mut keys = BTreeSet::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        for marker in ["tr(\"", "trf(\""] {
            let mut rest = text.as_str();
            while let Some(at) = rest.find(marker) {
                // `str(`, `attr(` ... are not calls of tr.
                let before = rest[..at].chars().next_back();
                let body = &rest[at + marker.len()..];
                let end = body.find('"').expect("unterminated literal");
                if !before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    keys.insert(body[..end].to_string());
                }
                rest = &body[end..];
            }
        }
    }
    keys
}

fn tables() -> [(Language, BTreeMap<&'static str, &'static str>); 2] {
    [Language::ZhHant, Language::Ko].map(|l| {
        let table = l.table();
        let map: BTreeMap<_, _> = table.iter().copied().collect();
        assert_eq!(map.len(), table.len(), "{l:?} has a duplicate key");
        (l, map)
    })
}

#[test]
fn every_translated_key_exists_in_the_english_source_and_every_wrapped_text_is_translated() {
    let wrapped = wrapped_keys();
    assert!(wrapped.len() > 200, "the scan finds the wrapped texts: {}", wrapped.len());
    for (language, table) in tables() {
        let unknown: Vec<_> = table.keys().filter(|k| !wrapped.contains(**k)).collect();
        assert!(unknown.is_empty(), "{language:?} translates texts that no tr(\"...\") uses (typo?): {unknown:?}");
        let untranslated: Vec<_> = wrapped.iter().filter(|k| !table.contains_key(k.as_str()) && !SAME_EVERYWHERE.contains(&k.as_str())).collect();
        assert!(untranslated.is_empty(), "{language:?} lacks: {untranslated:?}");
    }
}

#[test]
fn placeholders_match_the_english_text_in_every_language() {
    for (language, table) in tables() {
        for (english, translated) in table {
            assert!(!translated.trim().is_empty(), "{language:?}: empty translation of {english:?}");
            let mut want = placeholders(english);
            let mut got = placeholders(translated);
            want.sort();
            got.sort();
            assert_eq!(want, got, "{language:?}: placeholders of {english:?} differ in {translated:?}");
        }
    }
    assert_eq!(placeholders("Baked {} of {n} sets"), ["{}", "{n}"]);
}

#[test]
fn translations_use_the_right_script_and_keep_untranslated_texts_english() {
    let hangul = |s: &str| s.chars().any(|c| ('\u{AC00}'..='\u{D7A3}').contains(&c));
    let han = |s: &str| s.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c));
    for (language, table) in tables() {
        let with_script = table.values().filter(|t| if language == Language::Ko { hangul(t) } else { han(t) }).count();
        assert!(with_script * 10 > table.len() * 9, "{language:?}: only {with_script} of {} translations use its script", table.len());
    }
    assert_eq!(translate(Language::English, "Help"), "Help");
    assert_eq!(translate(Language::Ko, "Help"), "도움말");
    assert_eq!(translate(Language::ZhHant, "Help"), "說明");
    assert_eq!(translate(Language::Ko, "A text nobody translated"), "A text nobody translated");
}

#[test]
fn formatting_fills_placeholders_in_order_and_leaves_extras() {
    assert_eq!(fill("Baked {} of {} sets", &[&2, &5]), "Baked 2 of 5 sets");
    assert_eq!(fill("{} / {}", &[&"a"]), "a / {}");
    assert_eq!(fill("no placeholders", &[&1]), "no placeholders");
    assert_eq!(fill("끝 {} 번", &[&3]), "끝 3 번");
}

#[test]
fn the_language_is_a_saved_preference_and_old_settings_files_default_to_english() {
    let old: crate::recovery::Preferences = serde_json::from_str("{\"ui_size_percent\":100}").unwrap();
    assert_eq!(old.language, Language::English);
    let mut prefs = old;
    for language in Language::ALL {
        prefs.language = language;
        let back: crate::recovery::Preferences = serde_json::from_str(&serde_json::to_string(&prefs).unwrap()).unwrap();
        assert_eq!(back.language, language);
    }
    assert_eq!(serde_json::to_string(&Language::ZhHant).unwrap(), "\"zh_hant\"");
    assert_eq!(Language::ALL.map(Language::native_name), ["English", "繁體中文", "한국어"]);
}

#[test]
fn a_cjk_font_is_looked_for_per_language_and_english_needs_none() {
    assert!(font_available(Language::English));
    assert!(font_candidates(Language::English).is_empty() || std::env::var("SUBCRAFT_CJK_FONT").is_ok());
    for l in [Language::Ko, Language::ZhHant] {
        assert!(font_candidates(l).len() >= 5, "several well-known locations for {l:?}");
    }
}

#[test]
fn the_language_picker_loads_every_native_name_font_even_in_english() {
    assert!(fallback_languages(Language::English, false).is_empty(), "English alone loads no CJK font");
    assert_eq!(fallback_languages(Language::English, true), vec![Language::ZhHant, Language::Ko]);
    assert_eq!(fallback_languages(Language::Ko, false), vec![Language::Ko]);
    assert_eq!(fallback_languages(Language::Ko, true), vec![Language::Ko, Language::ZhHant], "own language first");
}
