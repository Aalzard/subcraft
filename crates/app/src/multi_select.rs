//! Multi-select in the layer list, as pure functions over the visible row order.

use subcraft_core::LayerId;

/// Modifier keys of a click.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClickMods {
    /// Ctrl / Cmd: add or remove the clicked row.
    pub toggle: bool,
    /// Shift: select the rows between the anchor and the clicked row.
    pub range: bool,
}

/// A selection of layers plus the row range selections grow from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MultiSelect {
    pub ids: Vec<LayerId>,
    anchor: Option<LayerId>,
}

impl MultiSelect {
    /// Apply a click on `clicked`. `rows` is the visible order, top to bottom.
    pub fn click(&mut self, rows: &[LayerId], clicked: LayerId, mods: ClickMods) {
        if mods.range
            && let Some(anchor) = self.anchor.filter(|a| rows.contains(a))
            && let (Some(a), Some(b)) = (rows.iter().position(|&r| r == anchor), rows.iter().position(|&r| r == clicked))
        {
            let (lo, hi) = (a.min(b), a.max(b));
            let range = rows[lo..=hi].iter().copied();
            if mods.toggle {
                for r in range {
                    if !self.ids.contains(&r) { self.ids.push(r); }
                }
            } else {
                self.ids = range.collect();
            }
        } else if mods.toggle {
            if let Some(i) = self.ids.iter().position(|&r| r == clicked) {
                self.ids.remove(i);
            } else {
                self.ids.push(clicked);
            }
            self.anchor = Some(clicked);
        } else {
            self.ids = vec![clicked];
            self.anchor = Some(clicked);
        }
    }

    /// The layer to make the primary selection (highlighted row, Properties, Delete) after a click
    /// on `clicked`: the clicked row, unless the click just removed it from the selection (Ctrl +
    /// click on a selected row). Then it is a row that is still selected; making the deselected row
    /// primary meant Delete removed exactly the layer the user had just taken out of the selection.
    pub fn primary_after_click(&self, clicked: LayerId) -> LayerId {
        if self.ids.contains(&clicked) { clicked } else { self.ids.last().copied().unwrap_or(clicked) }
    }

    /// Select every row (Ctrl+A in the layer list); the first one anchors range clicks.
    pub fn select_all(&mut self, rows: &[LayerId]) {
        self.ids = rows.to_vec();
        self.anchor = rows.first().copied();
    }

    /// Forget ids that no longer exist in `rows`.
    pub fn retain_existing(&mut self, rows: &[LayerId]) {
        self.ids.retain(|i| rows.contains(i));
        if self.anchor.is_some_and(|a| !rows.contains(&a)) {
            self.anchor = None;
        }
    }

    pub fn clear(&mut self) {
        self.ids.clear();
        self.anchor = None;
    }

    pub fn contains(&self, id: LayerId) -> bool {
        self.ids.contains(&id)
    }

    /// More than one layer is selected.
    pub fn is_multi(&self) -> bool {
        self.ids.len() > 1
    }

    /// The layers an operation applies to: the multi-selection, or just `primary`.
    pub fn targets(&self, primary: Option<LayerId>) -> Vec<LayerId> {
        if self.is_multi() { self.ids.clone() } else { primary.into_iter().collect() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(n: usize) -> Vec<LayerId> { (0..n).map(|_| LayerId::new()).collect() }
    const CTRL: ClickMods = ClickMods { toggle: true, range: false };
    const SHIFT: ClickMods = ClickMods { toggle: false, range: true };

    /// QA 2026-11: Ctrl + click on a selected row took it out of the selection but made it the
    /// primary layer, so Delete removed the layer the user had just deselected.
    #[test]
    fn deselecting_a_row_does_not_make_it_the_primary_layer() {
        let r = rows(4);
        let mut m = MultiSelect::default();
        m.click(&r, r[0], ClickMods::default());
        assert_eq!(m.primary_after_click(r[0]), r[0]);
        m.click(&r, r[1], CTRL);
        assert_eq!(m.primary_after_click(r[1]), r[1], "an added row becomes primary");
        m.click(&r, r[1], CTRL);
        assert_eq!(m.ids, vec![r[0]]);
        assert_eq!(m.primary_after_click(r[1]), r[0], "the row that is still selected is primary");
        assert_eq!(m.targets(Some(m.primary_after_click(r[1]))), vec![r[0]], "Delete acts on the selected row");
        // Deselecting the only row leaves it primary (something is always shown in Properties).
        m.click(&r, r[0], CTRL);
        assert!(m.ids.is_empty());
        assert_eq!(m.primary_after_click(r[0]), r[0]);
    }

    #[test]
    fn plain_click_selects_one_ctrl_toggles_shift_selects_a_range() {
        let r = rows(6);
        let mut m = MultiSelect::default();
        m.click(&r, r[1], ClickMods::default());
        assert_eq!(m.ids, vec![r[1]]);
        m.click(&r, r[3], CTRL);
        assert_eq!(m.ids, vec![r[1], r[3]]);
        m.click(&r, r[1], CTRL);
        assert_eq!(m.ids, vec![r[3]]);
        m.click(&r, r[5], SHIFT); // anchor is the last toggled row, r[1]
        assert_eq!(m.ids, vec![r[1], r[2], r[3], r[4], r[5]]);
        m.click(&r, r[0], ClickMods::default());
        assert_eq!(m.ids, vec![r[0]]);
        assert!(!m.is_multi());
    }

    #[test]
    fn targets_fall_back_to_the_primary_layer() {
        let r = rows(3);
        let mut m = MultiSelect::default();
        assert_eq!(m.targets(Some(r[2])), vec![r[2]]);
        m.click(&r, r[0], ClickMods::default());
        m.click(&r, r[1], CTRL);
        assert_eq!(m.targets(Some(r[2])), vec![r[0], r[1]]);
        m.retain_existing(&r[1..]);
        assert_eq!(m.ids, vec![r[1]]);
    }
}
