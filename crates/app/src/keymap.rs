//! The user's own key bindings on top of a built-in key set (Painter's Settings > Shortcuts).
//!
//! [`CustomKeys`] holds only the actions the user rebound; everything else keeps the binding of
//! the active [`KeymapPreset`]. A user binding wins over a preset binding of the same key, so
//! giving a key to one action takes it away from the action that had it by default, without
//! editing that action. Bindings are saved by name (`"Undo"` → `["Ctrl+Z"]`), never by enum index,
//! and names this build does not know are skipped on load.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::actions::{self, Action, Key, KeymapPreset, Shortcut};

impl Key {
    /// Every key the tables can name, in declaration order.
    pub const ALL: [Key; 59] = [
        Key::A, Key::B, Key::C, Key::D, Key::E, Key::F, Key::G, Key::H, Key::I, Key::J, Key::K, Key::L, Key::M,
        Key::N, Key::O, Key::P, Key::Q, Key::R, Key::S, Key::T, Key::U, Key::V, Key::W, Key::X, Key::Y, Key::Z,
        Key::Num0, Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9,
        Key::F1, Key::F2, Key::F3, Key::F4, Key::F5, Key::F6, Key::F7, Key::F8, Key::F9, Key::F10, Key::F11, Key::F12,
        Key::Delete, Key::Tab, Key::Space, Key::Escape, Key::Backspace, Key::Enter, Key::Plus, Key::Minus,
        Key::OpenBracket, Key::CloseBracket, Key::Home,
    ];

    /// The key whose [`Key::label`] is `s` (case-insensitive).
    pub fn from_label(s: &str) -> Option<Key> {
        Key::ALL.into_iter().find(|k| k.label().eq_ignore_ascii_case(s))
    }
}

impl Shortcut {
    /// Reads [`Shortcut::label`] back: `"Ctrl+Shift+Z"`, `"Alt+Q"`, `"Ctrl++"`.
    pub fn parse(s: &str) -> Option<Shortcut> {
        let mut rest = s.trim();
        let (mut ctrl, mut shift, mut alt) = (false, false, false);
        loop {
            let lower = rest.to_ascii_lowercase();
            // A modifier prefix is only one if a key follows it ("Ctrl++" is Ctrl and the + key).
            let take = |prefix: &str| lower.starts_with(prefix) && rest.len() > prefix.len();
            if take("ctrl+") {
                ctrl = true;
                rest = &rest[5..];
            } else if take("shift+") {
                shift = true;
                rest = &rest[6..];
            } else if take("alt+") {
                alt = true;
                rest = &rest[4..];
            } else {
                break;
            }
        }
        Key::from_label(rest).map(|key| Shortcut { ctrl, shift, alt, key })
    }
}

impl Action {
    /// Stable name used in the preferences file (the variant name).
    pub fn id(self) -> String {
        format!("{self:?}")
    }
    pub fn from_id(id: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|a| a.id() == id)
    }
}

/// Actions the user rebound, with their keys (an empty list = the user removed every key).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "BTreeMap<String, Vec<String>>", into = "BTreeMap<String, Vec<String>>")]
pub struct CustomKeys {
    entries: Vec<(Action, Vec<Shortcut>)>,
}

impl From<BTreeMap<String, Vec<String>>> for CustomKeys {
    fn from(map: BTreeMap<String, Vec<String>>) -> Self {
        let mut keys = CustomKeys::default();
        for (id, list) in map {
            let Some(action) = Action::from_id(&id) else {
                log::warn!("unknown action \"{id}\" in the saved shortcuts; skipped");
                continue;
            };
            let mut parsed: Vec<Shortcut> = Vec::new();
            for s in list.iter().filter_map(|s| Shortcut::parse(s)) {
                if !parsed.contains(&s) {
                    parsed.push(s);
                }
            }
            // Keys this build cannot read (a newer key name, a hand edit) leave the action on its
            // default keys; only a list saved empty means "no key". An empty list here used to
            // unbind the action, Ctrl+Z included.
            if parsed.is_empty() && !list.is_empty() {
                log::warn!("unreadable saved shortcuts {list:?} for \"{id}\"; using its default keys");
                continue;
            }
            keys.entries.push((action, parsed));
        }
        // Two actions saved with the same key: the first (by name) keeps it.
        let mut seen = Vec::new();
        for (_, list) in &mut keys.entries {
            list.retain(|s| if seen.contains(s) { false } else { seen.push(*s); true });
        }
        keys
    }
}

impl From<CustomKeys> for BTreeMap<String, Vec<String>> {
    fn from(keys: CustomKeys) -> Self {
        keys.entries.into_iter().map(|(a, list)| (a.id(), list.iter().map(Shortcut::label).collect())).collect()
    }
}

impl CustomKeys {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// The user's keys of `action`, when it was rebound.
    pub fn get(&self, action: Action) -> Option<&[Shortcut]> {
        self.entries.iter().find(|(a, _)| *a == action).map(|(_, l)| l.as_slice())
    }
    /// The rebound action that owns `shortcut`.
    pub fn owner(&self, shortcut: Shortcut) -> Option<Action> {
        self.entries.iter().find(|(_, l)| l.contains(&shortcut)).map(|(a, _)| *a)
    }
    fn slot(&mut self, action: Action, initial: Vec<Shortcut>) -> &mut Vec<Shortcut> {
        let i = match self.entries.iter().position(|(a, _)| *a == action) {
            Some(i) => i,
            None => {
                // Kept sorted by name, as a saved file reads back, so equal maps compare equal.
                let id = action.id();
                let at = self.entries.partition_point(|(a, _)| a.id() < id);
                self.entries.insert(at, (action, initial));
                at
            }
        };
        &mut self.entries[i].1
    }
}

/// The key bindings in force: a built-in key set plus the user's rebinds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Keymap {
    pub preset: KeymapPreset,
    pub custom: CustomKeys,
}

impl From<KeymapPreset> for Keymap {
    fn from(preset: KeymapPreset) -> Self {
        Keymap { preset, custom: CustomKeys::default() }
    }
}

impl Keymap {
    pub fn new(preset: KeymapPreset, custom: CustomKeys) -> Self {
        Keymap { preset, custom }
    }

    /// The keys of `action`: the user's when rebound, else the key set's minus keys the user gave
    /// to another action.
    pub fn shortcuts(&self, action: Action) -> Vec<Shortcut> {
        match self.custom.get(action) {
            Some(list) => list.to_vec(),
            None => actions::shortcuts_in(self.preset, action).iter().copied().filter(|s| self.custom.owner(*s).is_none()).collect(),
        }
    }

    /// The action an exact key + modifier combination triggers.
    pub fn action_for(&self, shortcut: Shortcut) -> Option<Action> {
        self.custom.owner(shortcut).or_else(|| {
            Action::ALL.into_iter().find(|&a| self.custom.get(a).is_none() && actions::shortcuts_in(self.preset, a).contains(&shortcut))
        })
    }

    /// Menu / tooltip text: the first key of `action` ("" when it has none).
    pub fn label(&self, action: Action) -> String {
        self.shortcuts(action).first().map(Shortcut::label).unwrap_or_default()
    }

    /// Whether the user changed `action`'s keys.
    pub fn is_custom(&self, action: Action) -> bool {
        self.custom.get(action).is_some()
    }

    /// Whether `action` has the key set's keys (a rebind that ended where it started counts as default).
    pub fn is_default(&self, action: Action) -> bool {
        self.shortcuts(action) == actions::shortcuts_in(self.preset, action)
    }

    /// Bind `shortcut` to `action` too (Painter: press the new combination). The key leaves the
    /// action that had it; that action is returned so the UI can say so.
    pub fn bind(&mut self, action: Action, shortcut: Shortcut) -> Option<Action> {
        let before = self.action_for(shortcut).filter(|a| *a != action);
        if let Some(other) = before.filter(|a| self.is_custom(*a)) {
            self.custom.slot(other, Vec::new()).retain(|s| *s != shortcut);
        }
        let current = self.shortcuts(action);
        let slot = self.custom.slot(action, current);
        if !slot.contains(&shortcut) {
            slot.push(shortcut);
        }
        before
    }

    /// Replace every key of `action` by `shortcut`.
    pub fn rebind(&mut self, action: Action, shortcut: Shortcut) -> Option<Action> {
        let others: Vec<Shortcut> = self.shortcuts(action).into_iter().filter(|s| *s != shortcut).collect();
        for s in others {
            self.unbind(action, s);
        }
        self.bind(action, shortcut)
    }

    /// Remove one key of `action`.
    pub fn unbind(&mut self, action: Action, shortcut: Shortcut) {
        let current = self.shortcuts(action);
        self.custom.slot(action, current).retain(|s| *s != shortcut);
    }

    /// Back to the key set's keys (Painter: right-click a shortcut). A default key the user gave
    /// to another action stays there.
    pub fn reset(&mut self, action: Action) {
        self.custom.entries.retain(|(a, _)| *a != action);
    }

    pub fn reset_all(&mut self) {
        self.custom = CustomKeys::default();
    }

    /// Keys bound to more than one action (should never happen; the tests and the editor check).
    pub fn conflicts(&self) -> Vec<(Shortcut, Vec<Action>)> {
        let mut by_key: Vec<(Shortcut, Vec<Action>)> = Vec::new();
        for a in Action::ALL {
            for s in self.shortcuts(a) {
                match by_key.iter_mut().find(|(k, _)| *k == s) {
                    Some((_, list)) => list.push(a),
                    None => by_key.push((s, vec![a])),
                }
            }
        }
        by_key.retain(|(_, list)| list.len() > 1);
        by_key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_label_and_every_action_name_reads_back() {
        for k in Key::ALL {
            assert_eq!(Key::from_label(k.label()), Some(k), "{k:?}");
            for (ctrl, shift, alt) in [(false, false, false), (true, false, false), (true, true, false), (false, true, true), (true, true, true)] {
                let s = Shortcut { ctrl, shift, alt, key: k };
                assert_eq!(Shortcut::parse(&s.label()), Some(s), "{}", s.label());
            }
        }
        assert_eq!(Shortcut::parse("ctrl+shift+z"), Some(Shortcut::ctrl_shift(Key::Z)));
        assert_eq!(Shortcut::parse("Ctrl+"), None);
        assert_eq!(Shortcut::parse("Hyper+Z"), None);
        let ids: std::collections::HashSet<_> = Action::ALL.iter().map(|a| a.id()).collect();
        assert_eq!(ids.len(), Action::ALL.len(), "action names are unique");
        for a in Action::ALL {
            assert_eq!(Action::from_id(&a.id()), Some(a));
        }
    }

    /// QA 2026-11: a saved shortcut this build cannot read unbound the action instead of leaving
    /// its default keys.
    #[test]
    fn unreadable_saved_keys_keep_the_default_and_an_empty_list_still_unbinds() {
        let custom: CustomKeys = serde_json::from_str(r#"{"Undo": ["Ctrl+Hyperspace"], "Redo": [], "Save": ["Ctrl+Nope", "Ctrl+S"]}"#).unwrap();
        let km = Keymap::new(KeymapPreset::SubCraft, custom);
        assert_eq!(km.shortcuts(Action::Undo), actions::shortcuts_in(KeymapPreset::SubCraft, Action::Undo), "unreadable: default keys");
        assert!(!km.is_custom(Action::Undo));
        assert!(km.shortcuts(Action::Redo).is_empty(), "saved empty: the user removed every key");
        assert_eq!(km.shortcuts(Action::Save), [Shortcut::ctrl(Key::S)], "the readable key of a list is kept");
    }

    #[test]
    fn without_rebinds_the_keymap_is_the_key_set() {
        for preset in KeymapPreset::ALL {
            let km = Keymap::from(preset);
            for a in Action::ALL {
                assert_eq!(km.shortcuts(a), actions::shortcuts_in(preset, a));
                assert!(km.is_default(a) && !km.is_custom(a));
                for s in actions::shortcuts_in(preset, a) {
                    assert_eq!(km.action_for(*s), Some(a));
                }
            }
            assert!(km.conflicts().is_empty());
        }
    }

    #[test]
    fn a_rebound_key_moves_from_its_old_action_and_reset_brings_the_defaults_back() {
        let mut km = Keymap::from(KeymapPreset::SubCraft);
        // E is the Eraser; give it to the Brush.
        assert_eq!(km.bind(Action::BrushTool, Shortcut::key(Key::E)), Some(Action::EraserTool));
        assert_eq!(km.action_for(Shortcut::key(Key::E)), Some(Action::BrushTool));
        assert_eq!(km.shortcuts(Action::BrushTool), [Shortcut::key(Key::B), Shortcut::key(Key::Num1), Shortcut::key(Key::E)]);
        assert_eq!(km.shortcuts(Action::EraserTool), [Shortcut::key(Key::Num2)], "E left the Eraser");
        assert!(!km.is_custom(Action::EraserTool), "the Eraser itself was not edited");
        assert!(km.conflicts().is_empty());

        // Replace every key of Undo; the old one is free afterwards.
        assert_eq!(km.rebind(Action::Undo, Shortcut::alt(Key::Z)), None);
        assert_eq!(km.shortcuts(Action::Undo), [Shortcut::alt(Key::Z)]);
        assert_eq!(km.action_for(Shortcut::ctrl(Key::Z)), None);

        // A key taken from another rebound action leaves that one's list.
        assert_eq!(km.bind(Action::Redo, Shortcut::alt(Key::Z)), Some(Action::Undo));
        assert!(km.shortcuts(Action::Undo).is_empty());
        assert_eq!(km.label(Action::Undo), "");
        assert!(km.conflicts().is_empty());

        km.reset(Action::Undo);
        assert_eq!(km.shortcuts(Action::Undo), [Shortcut::ctrl(Key::Z)]);
        km.reset(Action::BrushTool);
        assert_eq!(km.shortcuts(Action::EraserTool), [Shortcut::key(Key::E), Shortcut::key(Key::Num2)], "the default comes back with the reset");
        km.reset_all();
        assert_eq!(km, Keymap::from(KeymapPreset::SubCraft));
    }

    #[test]
    fn an_action_without_a_default_key_can_get_one_and_a_key_can_be_removed() {
        let mut km = Keymap::from(KeymapPreset::SubCraft);
        assert!(km.shortcuts(Action::EyedropperTool).is_empty());
        assert_eq!(km.bind(Action::EyedropperTool, Shortcut::key(Key::I)), None);
        assert_eq!(km.action_for(Shortcut::key(Key::I)), Some(Action::EyedropperTool));
        km.unbind(Action::ResetCamera, Shortcut::key(Key::Home));
        assert_eq!(km.shortcuts(Action::ResetCamera), [Shortcut::key(Key::F)]);
        assert_eq!(km.action_for(Shortcut::key(Key::Home)), None);
        assert!(!km.is_default(Action::ResetCamera));
    }

    #[test]
    fn rebinds_are_saved_by_name_and_unknown_names_are_skipped() {
        let mut km = Keymap::from(KeymapPreset::Painter);
        km.bind(Action::EyedropperTool, Shortcut::alt(Key::I));
        km.unbind(Action::Undo, Shortcut::ctrl(Key::Z));
        let json = serde_json::to_string(&km.custom).unwrap();
        assert_eq!(json, r#"{"EyedropperTool":["P","Alt+I"],"Undo":[]}"#);
        assert_eq!(serde_json::from_str::<CustomKeys>(&json).unwrap(), km.custom);
        let mut other_order = Keymap::from(KeymapPreset::Painter);
        other_order.unbind(Action::Undo, Shortcut::ctrl(Key::Z));
        other_order.bind(Action::EyedropperTool, Shortcut::alt(Key::I));
        assert_eq!(other_order, km, "the order of the edits does not matter");

        let future = r#"{"Undo":["Alt+U","Hyper+X"],"TeleportTool":["T"],"Redo":["Alt+U"]}"#;
        let loaded: CustomKeys = serde_json::from_str(future).unwrap();
        assert_eq!(loaded.get(Action::Redo), Some(&[Shortcut::alt(Key::U)][..]), "the first name (Redo) keeps a shared key");
        assert_eq!(loaded.get(Action::Undo), Some(&[][..]), "an unreadable key is dropped");
        assert!(Keymap::new(KeymapPreset::SubCraft, loaded).conflicts().is_empty());
    }
}
