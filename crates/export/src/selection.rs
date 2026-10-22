//! Which output maps of a template are switched on, per texture set. Saved with the project (as the
//! unchecked ones) and edited by the Export window; all the rules live here so the window only
//! forwards clicks.

use std::collections::BTreeSet;

/// A `(texture set name, map suffix)` pair, e.g. `("Body", "BaseColor")`.
pub type MapKey = (String, String);

/// Unchecked maps plus the selection to restore after a solo (Alt+click).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MapSelection {
    off: BTreeSet<String>,
    solo_previous: Option<BTreeSet<String>>,
}

fn key(set: &str, suffix: &str) -> String {
    format!("{set}/{suffix}")
}

impl MapSelection {
    /// From the keys stored in the project (`"set/suffix"`) and, optionally, the remembered selection.
    pub fn from_stored(off: &[String], solo_previous: Option<&[String]>) -> Self {
        Self { off: off.iter().cloned().collect(), solo_previous: solo_previous.map(|p| p.iter().cloned().collect()) }
    }

    pub fn stored_off(&self) -> Vec<String> {
        self.off.iter().cloned().collect()
    }

    pub fn stored_solo_previous(&self) -> Option<Vec<String>> {
        self.solo_previous.as_ref().map(|p| p.iter().cloned().collect())
    }

    pub fn is_on(&self, set: &str, suffix: &str) -> bool {
        !self.off.contains(&key(set, suffix))
    }

    /// True while a solo can be undone by Alt+clicking the same map again.
    pub fn is_solo(&self) -> bool {
        self.solo_previous.is_some()
    }

    pub fn set_on(&mut self, set: &str, suffix: &str, on: bool) {
        self.solo_previous = None;
        let k = key(set, suffix);
        if on { self.off.remove(&k); } else { self.off.insert(k); }
    }

    /// Switch every map of one texture set.
    pub fn set_set_on(&mut self, set: &str, all: &[MapKey], on: bool) {
        for (s, suffix) in all.iter().filter(|(s, _)| s == set) {
            self.set_on(s, suffix, on);
        }
    }

    pub fn select_all(&mut self) {
        self.solo_previous = None;
        self.off.clear();
    }

    pub fn select_none(&mut self, all: &[MapKey]) {
        self.solo_previous = None;
        self.off = all.iter().map(|(s, m)| key(s, m)).collect();
    }

    pub fn invert(&mut self, all: &[MapKey]) {
        self.solo_previous = None;
        self.off = all.iter().map(|(s, m)| key(s, m)).filter(|k| !self.off.contains(k)).collect();
    }

    /// Alt+click: keep only this map. Doing it again on the same map restores what was selected
    /// before. Alt+clicking another map while soloed moves the solo and keeps the original selection.
    pub fn solo(&mut self, set: &str, suffix: &str, all: &[MapKey]) {
        let this = key(set, suffix);
        let only_this = self.is_on(set, suffix) && all.iter().map(|(s, m)| key(s, m)).filter(|k| *k != this).all(|k| self.off.contains(&k));
        if only_this && let Some(previous) = self.solo_previous.take() {
            self.off = previous;
            return;
        }
        if self.solo_previous.is_none() {
            self.solo_previous = Some(self.off.clone());
        }
        self.off = all.iter().map(|(s, m)| key(s, m)).filter(|k| *k != this).collect();
    }
}
