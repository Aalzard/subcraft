//! Named actions with default keyboard shortcuts (toolkit-independent).
//!
//! The UI translates its key events into a [`Shortcut`] and looks up the [`Action`]; menus show
//! [`ActionInfo::label`] and [`Shortcut::label`]. Keeping the table here means shortcuts are
//! data, testable without a window, and shared by every front end.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Undo,
    Redo,
    NewPaintLayer,
    NewFillLayer,
    NewFolder,
    DeleteLayer,
    DuplicateLayer,
    BrushTool,
    EraserTool,
    BrushSmaller,
    BrushLarger,
    ToggleSymmetryX,
    ResetCamera,
    Save,
    SaveAs,
    OpenProject,
    Export,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    B,
    D,
    E,
    F,
    G,
    N,
    O,
    S,
    X,
    Y,
    Z,
    Delete,
    OpenBracket,
    CloseBracket,
    Home,
}

impl Key {
    pub fn label(self) -> &'static str {
        match self {
            Key::B => "B",
            Key::D => "D",
            Key::E => "E",
            Key::F => "F",
            Key::G => "G",
            Key::N => "N",
            Key::O => "O",
            Key::S => "S",
            Key::X => "X",
            Key::Y => "Y",
            Key::Z => "Z",
            Key::Delete => "Del",
            Key::OpenBracket => "[",
            Key::CloseBracket => "]",
            Key::Home => "Home",
        }
    }
}

/// A key with modifiers (`ctrl` means Cmd on macOS).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: Key,
}

impl Shortcut {
    pub const fn key(key: Key) -> Self {
        Self { ctrl: false, shift: false, alt: false, key }
    }
    pub const fn ctrl(key: Key) -> Self {
        Self { ctrl: true, shift: false, alt: false, key }
    }
    pub const fn ctrl_shift(key: Key) -> Self {
        Self { ctrl: true, shift: true, alt: false, key }
    }

    pub fn label(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("Ctrl+");
        }
        if self.shift {
            s.push_str("Shift+");
        }
        if self.alt {
            s.push_str("Alt+");
        }
        s.push_str(self.key.label());
        s
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ActionInfo {
    pub action: Action,
    pub label: &'static str,
    pub shortcuts: &'static [Shortcut],
}

pub const ACTIONS: &[ActionInfo] = &[
    ActionInfo { action: Action::Undo, label: "Undo", shortcuts: &[Shortcut::ctrl(Key::Z)] },
    ActionInfo { action: Action::Redo, label: "Redo", shortcuts: &[Shortcut::ctrl(Key::Y), Shortcut::ctrl_shift(Key::Z)] },
    ActionInfo { action: Action::NewPaintLayer, label: "New Paint Layer", shortcuts: &[Shortcut::ctrl_shift(Key::N)] },
    ActionInfo { action: Action::NewFillLayer, label: "New Fill Layer", shortcuts: &[Shortcut::ctrl_shift(Key::F)] },
    ActionInfo { action: Action::NewFolder, label: "New Folder", shortcuts: &[Shortcut::ctrl(Key::G)] },
    ActionInfo { action: Action::DeleteLayer, label: "Delete Layer", shortcuts: &[Shortcut::key(Key::Delete)] },
    ActionInfo { action: Action::DuplicateLayer, label: "Duplicate Layer", shortcuts: &[Shortcut::ctrl(Key::D)] },
    ActionInfo { action: Action::BrushTool, label: "Brush", shortcuts: &[Shortcut::key(Key::B)] },
    ActionInfo { action: Action::EraserTool, label: "Eraser", shortcuts: &[Shortcut::key(Key::E)] },
    ActionInfo { action: Action::BrushSmaller, label: "Brush Smaller", shortcuts: &[Shortcut::key(Key::OpenBracket)] },
    ActionInfo { action: Action::BrushLarger, label: "Brush Larger", shortcuts: &[Shortcut::key(Key::CloseBracket)] },
    ActionInfo { action: Action::ToggleSymmetryX, label: "Toggle X Symmetry", shortcuts: &[Shortcut::key(Key::X)] },
    ActionInfo { action: Action::ResetCamera, label: "Reset Camera", shortcuts: &[Shortcut::key(Key::Home)] },
    ActionInfo { action: Action::Save, label: "Save Project", shortcuts: &[Shortcut::ctrl(Key::S)] },
    ActionInfo { action: Action::SaveAs, label: "Save Project As", shortcuts: &[Shortcut::ctrl_shift(Key::S)] },
    ActionInfo { action: Action::OpenProject, label: "Open Project", shortcuts: &[Shortcut::ctrl(Key::O)] },
    ActionInfo { action: Action::Export, label: "Export Textures", shortcuts: &[Shortcut::ctrl_shift(Key::E)] },
];

pub fn info(action: Action) -> &'static ActionInfo {
    ACTIONS.iter().find(|a| a.action == action).expect("every action is listed")
}

/// The action bound to an exact key + modifier combination.
pub fn action_for(shortcut: Shortcut) -> Option<Action> {
    ACTIONS.iter().find(|a| a.shortcuts.contains(&shortcut)).map(|a| a.action)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_resolve_and_are_unique() {
        assert_eq!(action_for(Shortcut::ctrl(Key::Z)), Some(Action::Undo));
        assert_eq!(action_for(Shortcut::ctrl_shift(Key::Z)), Some(Action::Redo));
        assert_eq!(action_for(Shortcut::key(Key::CloseBracket)), Some(Action::BrushLarger));
        assert_eq!(action_for(Shortcut::key(Key::Z)), None, "modifiers must match exactly");
        let mut seen = std::collections::HashSet::new();
        for a in ACTIONS {
            for s in a.shortcuts {
                assert!(seen.insert(*s), "duplicate shortcut {}", s.label());
            }
        }
        assert_eq!(Shortcut::ctrl_shift(Key::N).label(), "Ctrl+Shift+N");
        assert_eq!(info(Action::Redo).label, "Redo");
    }
}
