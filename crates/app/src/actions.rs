//! Named actions with default keyboard shortcuts (toolkit-independent).
//!
//! The UI translates its key events into a [`Shortcut`] and looks up the [`Action`] through the
//! user's [`Keymap`] (defaults plus their own bindings); menus show [`ActionInfo::label`] and
//! [`Shortcut::label`]. The defaults follow the common painter layout: number keys for tools,
//! M / C / B for the material, channel and mesh-map views, F1 / F2 / F3 for the 2D, 3D and
//! split views. Keeping the table here means shortcuts are data, testable without a window.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    Undo,
    Redo,
    NewPaintLayer,
    NewFillLayer,
    NewFolder,
    /// Put the selected layers into a new folder.
    GroupLayers,
    DeleteLayer,
    DuplicateLayer,
    /// Rename the selected layer (in the Layers panel).
    RenameLayer,
    /// Select every layer of the texture set (then Delete, Duplicate... act on all of them).
    SelectAllLayers,
    BrushTool,
    EraserTool,
    PolygonFillTool,
    SmudgeTool,
    CloneTool,
    EyedropperTool,
    PenTool,
    BrushSmaller,
    BrushLarger,
    ToggleSymmetryX,
    /// Frame the whole model in the 3D view.
    FrameModel,
    ResetCamera,
    /// Full material shading in the 3D view.
    ViewMaterial,
    /// Show the next material channel alone in the 3D view.
    CycleChannel,
    /// Show the next baked mesh map in the 3D view.
    CycleMeshMap,
    /// The 3D and 2D views side by side.
    ViewSplit,
    /// Only the 3D view.
    View3D,
    /// Only the 2D view.
    View2D,
    /// Load the model again from its file, keeping the layers.
    ReimportMesh,
    NewProject,
    Save,
    SaveAs,
    OpenProject,
    Export,
    Bake,
    /// Switch the 3D view between the raster viewport and the path-traced Rendering mode.
    TogglePathTracing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Key {
    A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    Num0, Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9,
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    Delete,
    Backspace,
    Space,
    Tab,
    OpenBracket,
    CloseBracket,
    Comma,
    Period,
    Home,
}

impl Key {
    /// Every key, in a stable order (for settings lists and tests).
    pub const ALL: [Key; 57] = [
        Key::A, Key::B, Key::C, Key::D, Key::E, Key::F, Key::G, Key::H, Key::I, Key::J, Key::K, Key::L, Key::M,
        Key::N, Key::O, Key::P, Key::Q, Key::R, Key::S, Key::T, Key::U, Key::V, Key::W, Key::X, Key::Y, Key::Z,
        Key::Num0, Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9,
        Key::F1, Key::F2, Key::F3, Key::F4, Key::F5, Key::F6, Key::F7, Key::F8, Key::F9, Key::F10, Key::F11, Key::F12,
        Key::Delete, Key::Backspace, Key::Space, Key::Tab, Key::OpenBracket, Key::CloseBracket, Key::Comma, Key::Period, Key::Home,
    ];

    pub fn label(self) -> &'static str {
        use Key::*;
        match self {
            A => "A", B => "B", C => "C", D => "D", E => "E", F => "F", G => "G", H => "H", I => "I", J => "J", K => "K", L => "L", M => "M",
            N => "N", O => "O", P => "P", Q => "Q", R => "R", S => "S", T => "T", U => "U", V => "V", W => "W", X => "X", Y => "Y", Z => "Z",
            Num0 => "0", Num1 => "1", Num2 => "2", Num3 => "3", Num4 => "4", Num5 => "5", Num6 => "6", Num7 => "7", Num8 => "8", Num9 => "9",
            F1 => "F1", F2 => "F2", F3 => "F3", F4 => "F4", F5 => "F5", F6 => "F6", F7 => "F7", F8 => "F8", F9 => "F9", F10 => "F10", F11 => "F11", F12 => "F12",
            Delete => "Del", Backspace => "Backspace", Space => "Space", Tab => "Tab", OpenBracket => "[", CloseBracket => "]", Comma => ",", Period => ".", Home => "Home",
        }
    }
}

/// A key with modifiers (`ctrl` means Cmd on macOS).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    /// Group in the shortcut settings.
    pub group: &'static str,
    pub shortcuts: &'static [Shortcut],
}

const fn a(action: Action, label: &'static str, group: &'static str, shortcuts: &'static [Shortcut]) -> ActionInfo {
    ActionInfo { action, label, group, shortcuts }
}

/// Every action with its default shortcuts.
pub const ACTIONS: &[ActionInfo] = &[
    a(Action::Undo, "Undo", "Edit", &[Shortcut::ctrl(Key::Z)]),
    a(Action::Redo, "Redo", "Edit", &[Shortcut::ctrl(Key::Y), Shortcut::ctrl_shift(Key::Z)]),
    a(Action::NewPaintLayer, "New Paint Layer", "Layers", &[Shortcut::ctrl_shift(Key::N)]),
    a(Action::NewFillLayer, "New Fill Layer", "Layers", &[Shortcut::ctrl_shift(Key::F)]),
    a(Action::NewFolder, "New Folder", "Layers", &[]),
    a(Action::GroupLayers, "Group Layers into a Folder", "Layers", &[Shortcut::ctrl(Key::G)]),
    a(Action::DeleteLayer, "Delete Layer", "Layers", &[Shortcut::key(Key::Delete)]),
    a(Action::DuplicateLayer, "Duplicate Layer", "Layers", &[Shortcut::ctrl(Key::D)]),
    a(Action::RenameLayer, "Rename Layer (over the Layers panel)", "Layers", &[Shortcut::key(Key::F2)]),
    a(Action::SelectAllLayers, "Select All Layers", "Layers", &[Shortcut::ctrl(Key::A)]),
    a(Action::BrushTool, "Paint Brush", "Tools", &[Shortcut::key(Key::Num1)]),
    a(Action::EraserTool, "Eraser", "Tools", &[Shortcut::key(Key::Num2), Shortcut::key(Key::E)]),
    a(Action::PolygonFillTool, "Polygon Fill", "Tools", &[Shortcut::key(Key::Num4)]),
    a(Action::SmudgeTool, "Smudge", "Tools", &[Shortcut::key(Key::Num5)]),
    a(Action::CloneTool, "Clone", "Tools", &[Shortcut::key(Key::Num6)]),
    a(Action::EyedropperTool, "Eyedropper", "Tools", &[]),
    a(Action::PenTool, "Pen Paths", "Tools", &[Shortcut::key(Key::P)]),
    a(Action::BrushSmaller, "Brush Smaller", "Brush", &[Shortcut::key(Key::OpenBracket)]),
    a(Action::BrushLarger, "Brush Larger", "Brush", &[Shortcut::key(Key::CloseBracket)]),
    a(Action::ToggleSymmetryX, "Toggle Symmetry", "Brush", &[Shortcut::key(Key::X)]),
    a(Action::FrameModel, "Frame the Model", "View", &[Shortcut::key(Key::F)]),
    a(Action::ResetCamera, "Reset Camera", "View", &[Shortcut::key(Key::Home)]),
    a(Action::ViewMaterial, "Material View", "View", &[Shortcut::key(Key::M)]),
    a(Action::CycleChannel, "Next Channel View", "View", &[Shortcut::key(Key::C)]),
    a(Action::CycleMeshMap, "Next Mesh Map View", "View", &[Shortcut::key(Key::B)]),
    a(Action::ViewSplit, "3D and 2D Views", "View", &[Shortcut::key(Key::F1)]),
    a(Action::View3D, "3D View Only", "View", &[Shortcut::key(Key::F2)]),
    a(Action::View2D, "2D View Only", "View", &[Shortcut::key(Key::F3)]),
    a(Action::TogglePathTracing, "Rendering Mode (path tracing)", "View", &[Shortcut::key(Key::F10)]),
    a(Action::NewProject, "New Project", "File", &[Shortcut::ctrl(Key::N)]),
    a(Action::OpenProject, "Open Project", "File", &[Shortcut::ctrl(Key::O)]),
    a(Action::Save, "Save Project", "File", &[Shortcut::ctrl(Key::S)]),
    a(Action::SaveAs, "Save Project As", "File", &[Shortcut::ctrl_shift(Key::S)]),
    a(Action::ReimportMesh, "Reimport Mesh", "File", &[Shortcut::ctrl_shift(Key::R)]),
    a(Action::Export, "Export Textures", "File", &[Shortcut::ctrl_shift(Key::E)]),
    a(Action::Bake, "Bake Mesh Maps", "File", &[Shortcut::ctrl(Key::B)]),
];

pub fn info(action: Action) -> &'static ActionInfo {
    ACTIONS.iter().find(|a| a.action == action).expect("every action is listed")
}

/// The action bound to an exact key + modifier combination by default.
pub fn action_for(shortcut: Shortcut) -> Option<Action> {
    Keymap::default().action_for(shortcut)
}

/// The user's shortcuts: the defaults with their own bindings on top. Saved with the
/// preferences; an action they rebound keeps only their shortcuts (none = unbound).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Keymap {
    pub custom: Vec<(Action, Vec<Shortcut>)>,
}

impl Keymap {
    /// The shortcuts that trigger `action` now.
    pub fn shortcuts(&self, action: Action) -> Vec<Shortcut> {
        match self.custom.iter().find(|(a, _)| *a == action) {
            Some((_, s)) => s.clone(),
            None => info(action).shortcuts.to_vec(),
        }
    }

    pub fn is_custom(&self, action: Action) -> bool {
        self.custom.iter().any(|(a, _)| *a == action)
    }

    /// The action `shortcut` triggers (exact modifiers).
    pub fn action_for(&self, shortcut: Shortcut) -> Option<Action> {
        ACTIONS.iter().map(|a| a.action).find(|&a| self.shortcuts(a).contains(&shortcut))
    }

    /// Every action `shortcut` triggers (F2 is both rename and the 3D view by default; the UI
    /// picks by where the pointer is).
    pub fn actions_for(&self, shortcut: Shortcut) -> Vec<Action> {
        ACTIONS.iter().map(|a| a.action).filter(|&a| self.shortcuts(a).contains(&shortcut)).collect()
    }

    /// Bind `action` to exactly `shortcuts`, taking each of them away from any other action
    /// that had it (a key does one thing).
    pub fn set(&mut self, action: Action, shortcuts: Vec<Shortcut>) {
        for other in ACTIONS.iter().map(|a| a.action).filter(|&a| a != action) {
            let current = self.shortcuts(other);
            if current.iter().any(|s| shortcuts.contains(s)) {
                let kept: Vec<Shortcut> = current.into_iter().filter(|s| !shortcuts.contains(s)).collect();
                self.store(other, kept);
            }
        }
        self.store(action, shortcuts);
    }

    /// Back to the default shortcuts of `action`.
    pub fn reset(&mut self, action: Action) {
        self.custom.retain(|(a, _)| *a != action);
    }

    /// Every action back to its defaults.
    pub fn reset_all(&mut self) {
        self.custom.clear();
    }

    /// Other actions already using `shortcut` (shown before rebinding takes it from them).
    pub fn conflicts(&self, action: Action, shortcut: Shortcut) -> Vec<Action> {
        ACTIONS.iter().map(|a| a.action).filter(|&a| a != action && self.shortcuts(a).contains(&shortcut)).collect()
    }

    fn store(&mut self, action: Action, shortcuts: Vec<Shortcut>) {
        let default = info(action).shortcuts;
        self.custom.retain(|(a, _)| *a != action);
        if shortcuts.as_slice() != default {
            self.custom.push((action, shortcuts));
        }
    }
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
        assert_eq!(action_for(Shortcut::key(Key::F10)), Some(Action::TogglePathTracing));
        assert_eq!(action_for(Shortcut::ctrl(Key::A)), Some(Action::SelectAllLayers));
        let mut seen = std::collections::HashMap::new();
        for a in ACTIONS {
            for s in a.shortcuts {
                // F2 renames over the Layers panel and shows the 3D view elsewhere: the UI decides.
                if s.key == Key::F2 && !s.ctrl && !s.shift && !s.alt { continue; }
                assert!(seen.insert(*s, a.action).is_none(), "duplicate shortcut {}", s.label());
            }
        }
        assert_eq!(Shortcut::ctrl_shift(Key::N).label(), "Ctrl+Shift+N");
        assert_eq!(info(Action::Redo).label, "Redo");
        assert_eq!(Key::ALL.len(), 57);
    }

    #[test]
    fn painter_style_defaults() {
        let k = |key| action_for(Shortcut::key(key));
        assert_eq!(k(Key::Num1), Some(Action::BrushTool));
        assert_eq!(k(Key::Num2), Some(Action::EraserTool));
        assert_eq!(k(Key::Num4), Some(Action::PolygonFillTool));
        assert_eq!(k(Key::Num5), Some(Action::SmudgeTool));
        assert_eq!(k(Key::Num6), Some(Action::CloneTool));
        assert_eq!(k(Key::M), Some(Action::ViewMaterial));
        assert_eq!(k(Key::C), Some(Action::CycleChannel));
        assert_eq!(k(Key::B), Some(Action::CycleMeshMap));
        assert_eq!(k(Key::F1), Some(Action::ViewSplit));
        assert_eq!(k(Key::F3), Some(Action::View2D));
        assert_eq!(Keymap::default().actions_for(Shortcut::key(Key::F2)), vec![Action::RenameLayer, Action::View3D]);
        assert_eq!(k(Key::X), Some(Action::ToggleSymmetryX));
        assert_eq!(k(Key::F), Some(Action::FrameModel));
        assert_eq!(action_for(Shortcut::ctrl(Key::G)), Some(Action::GroupLayers));
        assert_eq!(action_for(Shortcut::ctrl_shift(Key::R)), Some(Action::ReimportMesh));
    }

    #[test]
    fn rebinding_moves_a_key_and_reset_restores_defaults() {
        let mut map = Keymap::default();
        assert_eq!(map.conflicts(Action::PenTool, Shortcut::key(Key::Num1)), vec![Action::BrushTool]);
        map.set(Action::PenTool, vec![Shortcut::key(Key::Num1)]);
        assert_eq!(map.action_for(Shortcut::key(Key::Num1)), Some(Action::PenTool));
        assert!(map.shortcuts(Action::BrushTool).is_empty(), "the brush lost the key");
        assert_eq!(map.action_for(Shortcut::key(Key::P)), None, "P is free now");
        map.reset(Action::PenTool);
        map.reset(Action::BrushTool);
        assert_eq!(map, Keymap::default());
        // Setting the default back stores nothing.
        map.set(Action::Undo, vec![Shortcut::ctrl(Key::Z)]);
        assert!(map.custom.is_empty());
        // It survives saving.
        map.set(Action::Bake, vec![Shortcut::ctrl_shift(Key::B)]);
        let json = serde_json::to_string(&map).unwrap();
        assert_eq!(serde_json::from_str::<Keymap>(&json).unwrap(), map);
    }
}
