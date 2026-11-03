//! Unsaved-changes logic as pure functions, so the rules are testable without a window.
//!
//! * [`is_dirty`] decides whether a project has changes that a save would keep.
//! * [`guard`] decides whether something that replaces the session (New, Open, Quit, close
//!   button) may run now or needs the Save / Don't save / Cancel question first.
//! * [`resolve`] turns the user's answer (and whether Save worked) into what happens next.

/// What differs from the last save / load / new.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DirtyInputs {
    /// The document is not equal to the saved one.
    pub document_changed: bool,
    /// Project settings (export choices, normal format) differ from the saved ones.
    pub settings_changed: bool,
    /// Baked mesh maps changed since the last save.
    pub maps_changed: bool,
    /// The mesh was replaced since the last save.
    pub mesh_changed: bool,
    /// Opened from a recovery copy and not saved since.
    pub from_recovery: bool,
}

/// True when closing now would lose work.
pub fn is_dirty(i: DirtyInputs) -> bool {
    i.document_changed || i.settings_changed || i.maps_changed || i.mesh_changed || i.from_recovery
}

/// Whether an action that discards the session may run right away.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Guard {
    Proceed,
    AskToSave,
}

/// `busy` is true while an edit is still in flight (an open pen path or a pending asset apply):
/// its result is not in the document yet, but closing would lose it, so it counts as unsaved.
/// `scripted` runs (screenshots) never ask.
pub fn guard(dirty: bool, busy: bool, scripted: bool) -> Guard {
    if scripted || !(dirty || busy) { Guard::Proceed } else { Guard::AskToSave }
}

/// The three buttons of the question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Save,
    DontSave,
    Cancel,
}

/// What to do after the answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Run the guarded action now.
    Run,
    /// Stay where we are: cancelled, or Save failed / its file dialog was dismissed.
    Abort,
}

/// `saved` is the result of the save attempt; only read for [`Answer::Save`].
pub fn resolve(answer: Answer, saved: bool) -> Outcome {
    match answer {
        Answer::DontSave => Outcome::Run,
        Answer::Save if saved => Outcome::Run,
        Answer::Save | Answer::Cancel => Outcome::Abort,
    }
}

/// Autosave recovery: whether the startup question should be asked. `crashed` is the number of
/// leftover session folders that hold a usable copy.
pub fn should_offer_recovery(crashed: usize, scripted: bool) -> bool {
    crashed > 0 && !scripted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_project_is_not_dirty_and_each_input_alone_makes_it_dirty() {
        assert!(!is_dirty(DirtyInputs::default()));
        for i in [
            DirtyInputs { document_changed: true, ..Default::default() },
            DirtyInputs { settings_changed: true, ..Default::default() },
            DirtyInputs { maps_changed: true, ..Default::default() },
            DirtyInputs { mesh_changed: true, ..Default::default() },
            DirtyInputs { from_recovery: true, ..Default::default() },
        ] {
            assert!(is_dirty(i), "{i:?}");
        }
    }

    #[test]
    fn guard_asks_only_with_unsaved_or_in_flight_work_and_never_when_scripted() {
        assert_eq!(guard(false, false, false), Guard::Proceed);
        assert_eq!(guard(true, false, false), Guard::AskToSave);
        assert_eq!(guard(false, true, false), Guard::AskToSave);
        assert_eq!(guard(true, true, true), Guard::Proceed);
    }

    #[test]
    fn answers_resolve_and_a_failed_or_dismissed_save_never_discards_work() {
        assert_eq!(resolve(Answer::DontSave, false), Outcome::Run);
        assert_eq!(resolve(Answer::Save, true), Outcome::Run);
        assert_eq!(resolve(Answer::Save, false), Outcome::Abort);
        assert_eq!(resolve(Answer::Cancel, true), Outcome::Abort);
    }

    #[test]
    fn recovery_is_offered_only_when_something_crashed_and_not_scripted() {
        assert!(!should_offer_recovery(0, false));
        assert!(should_offer_recovery(2, false));
        assert!(!should_offer_recovery(2, true));
    }
}
