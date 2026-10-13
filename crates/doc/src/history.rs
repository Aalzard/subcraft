//! Linear undo/redo history of document snapshots.
//!
//! Each entry stores the complete document *after* an action. Because documents share all
//! untouched layers and tiles, an entry costs only what the action changed: a parameter edit is
//! a few hundred bytes, a brush stroke is the tiles it touched (the guide's "before-state
//! snapshots of touched tiles", obtained for free from copy-on-write).
//!
//! Continuous edits (slider drags) coalesce through a merge key until [`History::seal`].

use std::time::Instant;

use crate::document::Document;

#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub label: String,
    pub doc: Document,
    pub time: Instant,
    merge_key: Option<String>,
}

#[derive(Clone, Debug)]
pub struct History {
    entries: Vec<HistoryEntry>,
    cursor: usize,
    max_entries: usize,
    sealed: bool,
}

impl History {
    pub fn new(initial: Document, label: impl Into<String>) -> Self {
        Self {
            entries: vec![HistoryEntry { label: label.into(), doc: initial, time: Instant::now(), merge_key: None }],
            cursor: 0,
            max_entries: 200,
            sealed: true,
        }
    }

    pub fn set_max_entries(&mut self, n: usize) {
        self.max_entries = n.max(2);
        self.trim();
    }

    pub fn current(&self) -> &Document {
        &self.entries[self.cursor].doc
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    pub fn can_redo(&self) -> bool {
        self.cursor + 1 < self.entries.len()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.can_undo().then(|| self.entries[self.cursor].label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.can_redo().then(|| self.entries[self.cursor + 1].label.as_str())
    }

    /// Record a new state. Discards any redo branch.
    pub fn push(&mut self, label: impl Into<String>, doc: Document) {
        self.push_merge(label, doc, None);
    }

    /// Record a state, replacing the previous entry if it has the same unsealed merge key.
    pub fn push_merge(&mut self, label: impl Into<String>, doc: Document, merge_key: Option<String>) {
        let at_end = self.cursor + 1 == self.entries.len();
        if at_end && !self.sealed && merge_key.is_some() && self.entries[self.cursor].merge_key == merge_key && self.cursor > 0 {
            let e = &mut self.entries[self.cursor];
            e.doc = doc;
            e.time = Instant::now();
            return;
        }
        self.entries.truncate(self.cursor + 1);
        self.entries.push(HistoryEntry { label: label.into(), doc, time: Instant::now(), merge_key: merge_key.clone() });
        self.cursor = self.entries.len() - 1;
        self.sealed = merge_key.is_none();
        self.trim();
    }

    /// End a coalescing gesture; the next push always creates a new entry.
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn undo(&mut self) -> Option<&Document> {
        if !self.can_undo() {
            return None;
        }
        self.cursor -= 1;
        self.sealed = true;
        Some(self.current())
    }

    pub fn redo(&mut self) -> Option<&Document> {
        if !self.can_redo() {
            return None;
        }
        self.cursor += 1;
        self.sealed = true;
        Some(self.current())
    }

    /// Jump to an arbitrary entry (History panel click).
    pub fn jump(&mut self, index: usize) -> Option<&Document> {
        if index >= self.entries.len() {
            return None;
        }
        self.cursor = index;
        self.sealed = true;
        Some(self.current())
    }

    /// Replace the whole history (project load / new project).
    pub fn reset(&mut self, doc: Document, label: impl Into<String>) {
        *self = History { max_entries: self.max_entries, ..History::new(doc, label) };
    }

    fn trim(&mut self) {
        while self.entries.len() > self.max_entries && self.cursor > 0 {
            self.entries.remove(0);
            self.cursor -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::Layer;
    use crate::texture_set::TextureSet;

    fn doc_with(n: usize) -> Document {
        let mut ts = TextureSet::new("T", 0, 128);
        for i in 0..n {
            let l = Layer::new_paint(format!("L{i}"), &ts);
            ts.insert_above(None, l);
        }
        Document { texture_sets: vec![ts] }
    }

    #[test]
    fn undo_redo_and_branch_discard() {
        let mut h = History::new(doc_with(0), "New");
        h.push("A", doc_with(1));
        h.push("B", doc_with(2));
        assert_eq!(h.undo().unwrap().texture_sets[0].layers.len(), 1);
        assert_eq!(h.undo().unwrap().texture_sets[0].layers.len(), 0);
        assert!(h.undo().is_none());
        assert_eq!(h.redo().unwrap().texture_sets[0].layers.len(), 1);
        h.push("C", doc_with(5));
        assert!(!h.can_redo(), "redo branch discarded");
        assert_eq!(h.entries().len(), 3);
    }

    #[test]
    fn merge_key_coalesces_until_sealed() {
        let mut h = History::new(doc_with(0), "New");
        h.push_merge("Opacity", doc_with(1), Some("op".into()));
        h.push_merge("Opacity", doc_with(2), Some("op".into()));
        h.push_merge("Opacity", doc_with(3), Some("op".into()));
        assert_eq!(h.entries().len(), 2);
        assert_eq!(h.current().texture_sets[0].layers.len(), 3);
        h.seal();
        h.push_merge("Opacity", doc_with(4), Some("op".into()));
        assert_eq!(h.entries().len(), 3);
    }

    #[test]
    fn trims_oldest() {
        let mut h = History::new(doc_with(0), "New");
        h.set_max_entries(3);
        for i in 1..10 {
            h.push(format!("{i}"), doc_with(i));
        }
        assert_eq!(h.entries().len(), 3);
        assert_eq!(h.current().texture_sets[0].layers.len(), 9);
    }
}
