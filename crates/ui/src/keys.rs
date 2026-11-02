//! Keyboard-shortcut wording for the host platform. egui already maps `Modifiers::command` to Cmd on
//! macOS and Ctrl elsewhere; this only rewrites the hint text the interface shows.

/// `"Save (Ctrl+S)"` shows as `"Save (Cmd+S)"` on macOS and is unchanged elsewhere.
pub fn native(text: &'static str) -> &'static str {
    if cfg!(target_os = "macos") { mac_cached(text) } else { text }
}

fn mac_cached(text: &'static str) -> &'static str {
    use std::{collections::HashMap, sync::{Mutex, OnceLock}};
    static CACHE: OnceLock<Mutex<HashMap<&'static str, &'static str>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    cache.entry(text).or_insert_with(|| Box::leak(for_mac(text).into_boxed_str()))
}

pub(crate) fn for_mac(text: &str) -> String { text.replace("Ctrl+", "Cmd+") }

#[cfg(test)]
mod tests {
    #[test]
    fn macos_wording_uses_cmd() {
        assert_eq!(super::for_mac("Redo (Ctrl+Shift+Z)"), "Redo (Cmd+Shift+Z)");
        assert_eq!(super::for_mac("Ctrl+click the model"), "Cmd+click the model");
        assert_eq!(super::native("No shortcut"), "No shortcut");
    }
}
