//! Named actions with default keyboard shortcuts (toolkit-independent).
//!
//! The UI translates its key events into a [`Shortcut`] and looks up the [`Action`]; menus show
//! [`ActionInfo::label`] and [`Shortcut::label`]. Keeping the table here means shortcuts are
//! data, testable without a window, and shared by every front end.
//!
//! Two key tables ([`KeymapPreset`]) are built in: the SubCraft default and one that follows
//! Substance 3D Painter. [`PAINTER_REFERENCE`] records every entry of Painter's documented list and
//! how SubCraft handles it (it is what the shortcut cheat sheet shows, and the tests keep it honest).

use serde::{Deserialize, Serialize};

/// Which key table is active. `SubCraft` is the original keymap; `Painter` follows Substance 3D
/// Painter's defaults wherever SubCraft has the matching feature. Both contain every shortcut whose
/// key was free; they differ only where a Painter key means something else in SubCraft (P, X).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KeymapPreset {
    #[default]
    SubCraft,
    Painter,
}

impl KeymapPreset {
    pub const ALL: [KeymapPreset; 2] = [KeymapPreset::SubCraft, KeymapPreset::Painter];
    pub fn label(self) -> &'static str {
        match self {
            KeymapPreset::SubCraft => "SubCraft (default)",
            KeymapPreset::Painter => "Substance Painter",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Undo,
    Redo,
    NewPaintLayer,
    NewFillLayer,
    NewFolder,
    /// A text layer: the UI opens the text composer; the engine adds a "Text" layer.
    NewTextLayer,
    DeleteLayer,
    DuplicateLayer,
    BrushTool,
    EraserTool,
    PenTool,
    PolygonFillTool,
    SmudgeTool,
    CloneTool,
    /// Eyedropper tool (Painter's "pick stroke material" key).
    EyedropperTool,
    BrushSmaller,
    BrushLarger,
    ToggleSymmetryX,
    /// Lazy mouse on (default rope length) / off.
    ToggleLazyMouse,
    ResetCamera,
    PerspectiveCamera,
    OrthographicCamera,
    /// Show the next / previous channel in the 3D view (wraps; the Material view is first).
    NextChannel,
    PrevChannel,
    /// Back to the full Material view.
    DisplayMaterial,
    NewProject,
    Quit,
    Save,
    SaveAs,
    OpenProject,
    Export,
    Bake,
    /// Switch the 3D view between the raster viewport and the path-traced Rendering mode.
    TogglePathTracing,
    /// Leave the Rendering mode: back to the painting viewport.
    PaintingMode,
    /// Select every layer of the texture set (then Delete, Duplicate... act on all of them).
    SelectAllLayers,
    /// Hide every panel so the view(s) fill the window (Painter's Tab); again to bring them back.
    HideUi,
    /// Close the project: back to the sample model (asks about unsaved changes first).
    CloseProject,
    /// 3D and 2D views side by side / 3D only / 2D only / swap them (Painter's F1..F4).
    View3dAnd2d,
    View3dOnly,
    View2dOnly,
    SwapViews,
    /// Show the next / previous baked mesh map on the model (wraps through the material view).
    NextMeshMap,
    PrevMeshMap,
    /// While painting a mask: swap the brush between white and black (gray g becomes 1 - g).
    InvertMaskGray,
    /// Show only the selected texture set's part of the model; again to show everything.
    IsolateTextureSet,
}

impl Action {
    /// Every action, in declaration order (the tests check [`ACTIONS`] against it).
    pub const ALL: [Action; 45] = [
        Action::Undo, Action::Redo, Action::NewPaintLayer, Action::NewFillLayer, Action::NewFolder, Action::NewTextLayer, Action::DeleteLayer, Action::DuplicateLayer,
        Action::BrushTool, Action::EraserTool, Action::PenTool, Action::PolygonFillTool, Action::SmudgeTool, Action::CloneTool, Action::EyedropperTool,
        Action::BrushSmaller, Action::BrushLarger, Action::ToggleSymmetryX, Action::ToggleLazyMouse, Action::ResetCamera, Action::PerspectiveCamera,
        Action::OrthographicCamera, Action::NextChannel, Action::PrevChannel, Action::DisplayMaterial, Action::NewProject, Action::Quit, Action::Save,
        Action::SaveAs, Action::OpenProject, Action::Export, Action::Bake, Action::TogglePathTracing, Action::PaintingMode, Action::SelectAllLayers,
        Action::HideUi, Action::CloseProject, Action::View3dAnd2d, Action::View3dOnly, Action::View2dOnly,
        Action::SwapViews, Action::NextMeshMap, Action::PrevMeshMap, Action::InvertMaskGray, Action::IsolateTextureSet,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    Num0, Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9,
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    Delete,
    Tab,
    Space,
    Escape,
    Backspace,
    Enter,
    Plus,
    Minus,
    OpenBracket,
    CloseBracket,
    Home,
}

impl Key {
    pub fn label(self) -> &'static str {
        const LETTERS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        const DIGITS: &str = "0123456789";
        const FKEYS: [&str; 12] = ["F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12"];
        let k = self as usize;
        if (Key::A as usize..=Key::Z as usize).contains(&k) {
            let i = k - Key::A as usize;
            &LETTERS[i..=i]
        } else if (Key::Num0 as usize..=Key::Num9 as usize).contains(&k) {
            let i = k - Key::Num0 as usize;
            &DIGITS[i..=i]
        } else if (Key::F1 as usize..=Key::F12 as usize).contains(&k) {
            FKEYS[k - Key::F1 as usize]
        } else {
            match self {
                Key::Delete => "Del",
                Key::Tab => "Tab",
                Key::Space => "Space",
                Key::Escape => "Esc",
                Key::Backspace => "Backspace",
                Key::Enter => "Enter",
                Key::Plus => "+",
                Key::Minus => "-",
                Key::OpenBracket => "[",
                Key::CloseBracket => "]",
                Key::Home => "Home",
                _ => unreachable!("handled by the ranges above"),
            }
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
    pub const fn new(ctrl: bool, shift: bool, alt: bool, key: Key) -> Self {
        Self { ctrl, shift, alt, key }
    }
    pub const fn key(key: Key) -> Self {
        Self::new(false, false, false, key)
    }
    pub const fn ctrl(key: Key) -> Self {
        Self::new(true, false, false, key)
    }
    pub const fn shift(key: Key) -> Self {
        Self::new(false, true, false, key)
    }
    pub const fn alt(key: Key) -> Self {
        Self::new(false, false, true, key)
    }
    pub const fn ctrl_shift(key: Key) -> Self {
        Self::new(true, true, false, key)
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
    /// SubCraft-preset bindings (this includes every Painter shortcut whose key was free).
    pub shortcuts: &'static [Shortcut],
    /// Painter-preset bindings where they differ from `shortcuts` (a Painter key that SubCraft
    /// used for something else moves the old binding away). `None` = same as `shortcuts`.
    pub painter: Option<&'static [Shortcut]>,
}

const fn a(action: Action, label: &'static str, shortcuts: &'static [Shortcut]) -> ActionInfo {
    ActionInfo { action, label, shortcuts, painter: None }
}

pub const ACTIONS: &[ActionInfo] = &[
    a(Action::Undo, "Undo", &[Shortcut::ctrl(Key::Z)]),
    a(Action::Redo, "Redo", &[Shortcut::ctrl(Key::Y), Shortcut::ctrl_shift(Key::Z)]),
    a(Action::NewPaintLayer, "New Paint Layer", &[Shortcut::ctrl_shift(Key::N)]),
    a(Action::NewFillLayer, "New Fill Layer", &[Shortcut::ctrl_shift(Key::F)]),
    a(Action::NewFolder, "New Folder", &[Shortcut::ctrl(Key::G)]),
    a(Action::NewTextLayer, "New Text Layer", &[Shortcut::ctrl_shift(Key::T)]),
    a(Action::DeleteLayer, "Delete Layer", &[Shortcut::key(Key::Delete)]),
    a(Action::DuplicateLayer, "Duplicate Layer", &[Shortcut::ctrl(Key::D)]),
    ActionInfo { action: Action::BrushTool, label: "Brush", shortcuts: &[Shortcut::key(Key::B), Shortcut::key(Key::Num1)], painter: Some(&[Shortcut::key(Key::Num1)]) },
    a(Action::EraserTool, "Eraser", &[Shortcut::key(Key::E), Shortcut::key(Key::Num2)]),
    ActionInfo { action: Action::PenTool, label: "Pen paths", shortcuts: &[Shortcut::key(Key::P)], painter: Some(&[Shortcut::shift(Key::P)]) },
    a(Action::PolygonFillTool, "Polygon Fill", &[Shortcut::key(Key::Num4)]),
    a(Action::SmudgeTool, "Smudge", &[Shortcut::key(Key::Num5)]),
    a(Action::CloneTool, "Clone", &[Shortcut::key(Key::Num6)]),
    ActionInfo { action: Action::EyedropperTool, label: "Eyedropper (pick color / material)", shortcuts: &[], painter: Some(&[Shortcut::key(Key::P)]) },
    a(Action::BrushSmaller, "Brush Smaller", &[Shortcut::key(Key::OpenBracket)]),
    a(Action::BrushLarger, "Brush Larger", &[Shortcut::key(Key::CloseBracket)]),
    ActionInfo { action: Action::ToggleSymmetryX, label: "Toggle X Symmetry", shortcuts: &[Shortcut::key(Key::X), Shortcut::key(Key::L)], painter: Some(&[Shortcut::key(Key::L)]) },
    a(Action::ToggleLazyMouse, "Toggle Lazy Mouse", &[Shortcut::key(Key::D)]),
    a(Action::ResetCamera, "Frame Model / Reset Camera", &[Shortcut::key(Key::Home), Shortcut::key(Key::F)]),
    a(Action::PerspectiveCamera, "Perspective Camera", &[Shortcut::key(Key::F5)]),
    a(Action::OrthographicCamera, "Orthographic Camera", &[Shortcut::key(Key::F6)]),
    a(Action::NextChannel, "Display Next Channel", &[Shortcut::key(Key::C)]),
    a(Action::PrevChannel, "Display Previous Channel", &[Shortcut::shift(Key::C)]),
    a(Action::DisplayMaterial, "Display Material", &[Shortcut::key(Key::M)]),
    a(Action::NewProject, "New Project from a Model", &[Shortcut::ctrl(Key::N)]),
    a(Action::Quit, "Quit", &[Shortcut::ctrl(Key::Q), Shortcut::alt(Key::F4)]),
    a(Action::Save, "Save Project", &[Shortcut::ctrl(Key::S)]),
    a(Action::SaveAs, "Save Project As", &[Shortcut::ctrl_shift(Key::S)]),
    a(Action::OpenProject, "Open Project", &[Shortcut::ctrl(Key::O)]),
    a(Action::Export, "Export Textures", &[Shortcut::ctrl_shift(Key::E)]),
    a(Action::Bake, "Bake Mesh Maps", &[Shortcut::ctrl(Key::B), Shortcut::key(Key::F8), Shortcut::ctrl_shift(Key::B)]),
    a(Action::TogglePathTracing, "Rendering Mode (path tracing)", &[Shortcut::key(Key::F10)]),
    a(Action::PaintingMode, "Painting Mode (leave Rendering mode)", &[Shortcut::key(Key::F9)]),
    a(Action::SelectAllLayers, "Select All Layers", &[Shortcut::ctrl(Key::A)]),
    a(Action::HideUi, "Hide UI", &[Shortcut::key(Key::Tab)]),
    a(Action::CloseProject, "Close Project", &[Shortcut::ctrl(Key::F4)]),
    a(Action::View3dAnd2d, "Show 3D and 2D Views", &[Shortcut::key(Key::F1)]),
    // F2 renames the selected layer in the SubCraft key set; Painter uses it for the 3D-only layout.
    ActionInfo { action: Action::View3dOnly, label: "Show 3D View Only", shortcuts: &[], painter: Some(&[Shortcut::key(Key::F2)]) },
    a(Action::View2dOnly, "Show 2D View Only", &[Shortcut::key(Key::F3)]),
    a(Action::SwapViews, "Swap 3D and 2D Views", &[Shortcut::key(Key::F4)]),
    ActionInfo { action: Action::NextMeshMap, label: "Display Next Mesh Map", shortcuts: &[], painter: Some(&[Shortcut::key(Key::B)]) },
    a(Action::PrevMeshMap, "Display Previous Mesh Map", &[Shortcut::shift(Key::B)]),
    ActionInfo { action: Action::InvertMaskGray, label: "Invert Mask Brush (white / black)", shortcuts: &[], painter: Some(&[Shortcut::key(Key::X)]) },
    a(Action::IsolateTextureSet, "Isolate Texture Set", &[Shortcut::alt(Key::Q)]),
];

/// Whether F2 belongs to the key table in `preset` (otherwise the Layers panel uses it to rename).
pub fn f2_is_bound(preset: KeymapPreset) -> bool {
    action_for(preset, Shortcut::key(Key::F2)).is_some()
}

pub fn info(action: Action) -> &'static ActionInfo {
    ACTIONS.iter().find(|a| a.action == action).expect("every action is listed")
}

/// The bindings of `action` in `preset`.
pub fn shortcuts_in(preset: KeymapPreset, action: Action) -> &'static [Shortcut] {
    let i = info(action);
    match preset {
        KeymapPreset::SubCraft => i.shortcuts,
        KeymapPreset::Painter => i.painter.unwrap_or(i.shortcuts),
    }
}

/// The action bound to an exact key + modifier combination in `preset`.
pub fn action_for(preset: KeymapPreset, shortcut: Shortcut) -> Option<Action> {
    ACTIONS.iter().find(|a| shortcuts_in(preset, a.action).contains(&shortcut)).map(|a| a.action)
}

/// Menu / tooltip text: the first binding of `action` in `preset` ("" when unbound).
pub fn label_in(preset: KeymapPreset, action: Action) -> String {
    shortcuts_in(preset, action).first().map(|s| s.label()).unwrap_or_default()
}

/// How a Substance Painter shortcut is handled here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Support {
    /// Bound in both presets; SubCraft already used this key for the same thing.
    Same,
    /// The key was free: bound in both presets.
    Added,
    /// The key means something else in the SubCraft preset: bound only in the Painter preset.
    PainterPreset,
    /// A mouse gesture or hold-key in the viewport (not a table entry).
    Gesture,
    /// SubCraft has no matching feature (or the platform owns the key); not bound.
    None,
}

/// One row of Painter's documented shortcut list.
#[derive(Clone, Copy, Debug)]
pub struct PainterEntry {
    pub group: &'static str,
    /// As Painter documents it (Windows).
    pub keys: &'static str,
    pub what: &'static str,
    pub support: Support,
    /// The SubCraft action this key triggers (table entries only).
    pub action: Option<Action>,
    /// The key as a table entry (for the consistency test).
    pub shortcut: Option<Shortcut>,
    pub note: &'static str,
}

const fn e(group: &'static str, keys: &'static str, what: &'static str, support: Support, action: Action, shortcut: Shortcut, note: &'static str) -> PainterEntry {
    PainterEntry { group, keys, what, support, action: Some(action), shortcut: Some(shortcut), note }
}
const fn none(group: &'static str, keys: &'static str, what: &'static str, note: &'static str) -> PainterEntry {
    PainterEntry { group, keys, what, support: Support::None, action: None, shortcut: None, note }
}
const fn gesture(group: &'static str, keys: &'static str, what: &'static str, note: &'static str) -> PainterEntry {
    PainterEntry { group, keys, what, support: Support::Gesture, action: None, shortcut: None, note }
}

/// Painter's list (Adobe Experience League "Shortcuts", Windows column), with SubCraft's handling.
pub const PAINTER_REFERENCE: &[PainterEntry] = &[
    // General
    e("General", "Tab", "Hide UI", Support::Added, Action::HideUi, Shortcut::key(Key::Tab), "hides every panel; Tab again brings them back"),
    e("General", "Ctrl+Z", "Undo", Support::Same, Action::Undo, Shortcut::ctrl(Key::Z), ""),
    e("General", "Ctrl+Y", "Redo", Support::Same, Action::Redo, Shortcut::ctrl(Key::Y), ""),
    e("General", "F8", "Baking mode", Support::Added, Action::Bake, Shortcut::key(Key::F8), "opens the Bake window"),
    e("General", "F9", "Painting mode", Support::Added, Action::PaintingMode, Shortcut::key(Key::F9), "closes the Bake window and leaves Rendering mode"),
    e("General", "F10", "Rendering (Iray) mode", Support::Same, Action::TogglePathTracing, Shortcut::key(Key::F10), "path-traced Rendering mode"),
    e("General", "Ctrl+O", "Open project", Support::Same, Action::OpenProject, Shortcut::ctrl(Key::O), ""),
    e("General", "Ctrl+F4", "Close project", Support::Added, Action::CloseProject, Shortcut::ctrl(Key::F4), "returns to the sample model after the unsaved-changes prompt"),
    e("General", "Ctrl+S", "Save project", Support::Same, Action::Save, Shortcut::ctrl(Key::S), ""),
    none("General", "(none)", "Save as", "Painter has no key; SubCraft keeps Ctrl+Shift+S"),
    e("General", "Ctrl+N", "New project", Support::Added, Action::NewProject, Shortcut::ctrl(Key::N), "New project from a model"),
    e("General", "Alt+F4", "Quit application", Support::Added, Action::Quit, Shortcut::alt(Key::F4), "also Ctrl+Q; Windows may close the window itself first"),
    e("General", "Ctrl+Shift+E", "Export textures", Support::Same, Action::Export, Shortcut::ctrl_shift(Key::E), ""),
    e("General", "Ctrl+Shift+B", "Bake mesh maps", Support::Added, Action::Bake, Shortcut::ctrl_shift(Key::B), "Ctrl+B kept as well"),
    // Manipulator / transform
    none("Manipulator", "Q", "Show/hide manipulator", "no layer transform manipulator"),
    none("Manipulator", "+", "Increase manipulator size", "no manipulator"),
    none("Manipulator", "-", "Decrease manipulator size", "no manipulator"),
    none("Manipulator", "T", "Cycle manipulation spaces", "no manipulator"),
    none("Manipulator", "Shift+V", "Toggle warp edition mode", "no warp mode"),
    none("Manipulator", "W", "Translate tool", "no manipulator"),
    none("Manipulator", "E", "Rotate tool", "no manipulator (E stays Eraser)"),
    none("Manipulator", "R", "Scale tool", "no manipulator"),
    none("Manipulator", "Shift+W", "Surface tool", "no equivalent"),
    e("Tools", "L", "Symmetry", Support::Added, Action::ToggleSymmetryX, Shortcut::key(Key::L), "toggles X mirror"),
    none("Tools", "Shift+Esc", "Pause engine computation", "no engine pause"),
    // Tools
    e("Tools", "1", "Paint tool", Support::Added, Action::BrushTool, Shortcut::key(Key::Num1), "B kept as well in the SubCraft key set"),
    none("Tools", "Ctrl+1", "Paint tool + Particles", "no particle tool (scatter lives in brush settings)"),
    e("Tools", "2", "Eraser tool", Support::Added, Action::EraserTool, Shortcut::key(Key::Num2), "E kept as well"),
    none("Tools", "Ctrl+2", "Eraser tool + Particles", "no particle tool"),
    none("Tools", "3", "Projection tool", "no projection tool"),
    none("Tools", "Ctrl+3", "Projection tool + Particles", "no projection tool"),
    e("Tools", "4", "Polygon Fill", Support::Added, Action::PolygonFillTool, Shortcut::key(Key::Num4), ""),
    e("Tools", "5", "Smudge tool", Support::Added, Action::SmudgeTool, Shortcut::key(Key::Num5), ""),
    e("Tools", "6", "Clone tool (relative source)", Support::Added, Action::CloneTool, Shortcut::key(Key::Num6), ""),
    none("Tools", "Ctrl+6", "Clone tool (absolute source)", "only one clone mode"),
    e("Tools", "]", "Increase tool size", Support::Same, Action::BrushLarger, Shortcut::key(Key::CloseBracket), ""),
    e("Tools", "[", "Decrease tool size", Support::Same, Action::BrushSmaller, Shortcut::key(Key::OpenBracket), ""),
    e("Tools", "X", "Invert grayscale tool", Support::PainterPreset, Action::InvertMaskGray, Shortcut::key(Key::X), "mask brush value g becomes 1 - g; X stays symmetry in the SubCraft key set"),
    e("Tools", "P", "Pick stroke material", Support::PainterPreset, Action::EyedropperTool, Shortcut::key(Key::P), "Eyedropper tool; Pen paths moves to Shift+P in this preset"),
    e("Tools", "D", "Lazy Mouse", Support::Added, Action::ToggleLazyMouse, Shortcut::key(Key::D), "toggles a 20 px rope"),
    none("Tools", "Alt+H", "Hide/ignore excluded geometry", "no equivalent"),
    gesture("Tools", "N (hold)", "Ignore stencil mask", "already worked"),
    none("Tools", "A", "Resume previous stroke", "no equivalent"),
    gesture("Tools", "V (hold) + LMB", "Set Clone tool source", "added; Ctrl+click still works"),
    // Camera / display
    e("Viewport", "F5", "Camera perspective", Support::Added, Action::PerspectiveCamera, Shortcut::key(Key::F5), ""),
    e("Viewport", "F6", "Camera orthographic", Support::Added, Action::OrthographicCamera, Shortcut::key(Key::F6), ""),
    e("Viewport", "C", "Display next channel", Support::Added, Action::NextChannel, Shortcut::key(Key::C), "cycles the 3D view channels"),
    e("Viewport", "Shift+C", "Display previous channel", Support::Added, Action::PrevChannel, Shortcut::shift(Key::C), ""),
    e("Viewport", "M", "Display material", Support::Added, Action::DisplayMaterial, Shortcut::key(Key::M), ""),
    e("Viewport", "B", "Display next mesh map", Support::PainterPreset, Action::NextMeshMap, Shortcut::key(Key::B), "B stays Brush in the SubCraft key set"),
    e("Viewport", "Shift+B", "Display previous mesh map", Support::Added, Action::PrevMeshMap, Shortcut::shift(Key::B), ""),
    none("Viewport", "Space", "Toggle animation", "no animation"),
    e("Viewport", "F", "Center the whole mesh", Support::Added, Action::ResetCamera, Shortcut::key(Key::F), "Home kept as well"),
    none("Viewport", "U", "Toggle quick mask edition", "no quick mask"),
    none("Viewport", "Y", "Clear quick mask", "no quick mask"),
    none("Viewport", "I", "Invert quick mask", "no quick mask (I over the model isolates a part)"),
    e("Viewport", "F1", "Viewport layout 3D/2D", Support::Added, Action::View3dAnd2d, Shortcut::key(Key::F1), "2D view in its own pane beside the 3D view"),
    e("Viewport", "F2", "Viewport layout 3D only", Support::PainterPreset, Action::View3dOnly, Shortcut::key(Key::F2), "F2 renames the selected layer in the SubCraft key set"),
    e("Viewport", "F3", "Viewport layout 2D only", Support::Added, Action::View2dOnly, Shortcut::key(Key::F3), ""),
    e("Viewport", "F4", "Swap 2D / 3D view", Support::Added, Action::SwapViews, Shortcut::key(Key::F4), "swaps the two panes, or the front tab when they share one"),
    e("Viewport", "Alt+Q", "Texture Set isolate", Support::Added, Action::IsolateTextureSet, Shortcut::alt(Key::Q), "hides the other texture sets' geometry; again shows everything"),
    // Mouse
    gesture("Mouse", "LMB", "Use tool / paint", "already worked"),
    gesture("Mouse", "Shift+LMB", "Draw straight lines", "already worked"),
    gesture("Mouse", "Ctrl+Shift+LMB", "Draw straight lines with snapping", "added: the line from the last stroke point snaps to 5 degree steps"),
    gesture("Mouse", "Alt+LMB", "Camera rotate", "already worked (Painter navigation)"),
    gesture("Mouse", "Alt+Shift+LMB", "Camera snap rotate", "added: the view snaps every 90 degrees"),
    gesture("Mouse", "Alt+MMB", "Camera translate", "already worked (plain MMB too)"),
    gesture("Mouse", "Ctrl+Alt+LMB", "Camera translate (alternative)", "added"),
    gesture("Mouse", "Alt+RMB", "Camera zoom", "already worked"),
    gesture("Mouse", "S (hold) + LMB", "Stencil rotate", "already worked"),
    gesture("Mouse", "Shift+S + LMB", "Stencil snap rotate", "added: 90 degree steps"),
    gesture("Mouse", "S (hold) + MMB", "Stencil translate", "already worked"),
    gesture("Mouse", "S (hold) + RMB", "Stencil zoom", "already worked"),
    gesture("Mouse", "Ctrl+RMB drag", "Change tool size / hardness", "added: horizontal = size, vertical = hardness"),
    gesture("Mouse", "Ctrl+LMB drag", "Change tool flow / rotation", "added: horizontal = flow, vertical = angle (Clone keeps Ctrl+click for its source)"),
    gesture("Mouse", "Shift+RMB", "Rotate environment", "added: drag left-right turns the environment and its lighting (Shift+MMB turns only the key light)"),
    gesture("Mouse", "Ctrl+MMB", "Auto-focus (Depth of Field)", "added: focuses the point under the cursor (turns depth of field on)"),
    gesture("Mouse", "Ctrl+Alt+RMB", "Texture set selection", "added: selects the texture set under the cursor"),
    gesture("Mouse", "RMB", "Context menu", "added: a click without dragging opens the Properties popup at the cursor (3D and 2D views); its header holds the part actions"),
    // Layers (non-editable in Painter)
    gesture("Layers", "Ctrl+C / X / V", "Copy / cut / paste layer", "already worked"),
    e("Layers", "Delete", "Delete layer", Support::Same, Action::DeleteLayer, Shortcut::key(Key::Delete), ""),
    e("Layers", "Ctrl+D", "Duplicate layer", Support::Same, Action::DuplicateLayer, Shortcut::ctrl(Key::D), ""),
    e("Layers", "Ctrl+G", "Group layer", Support::Same, Action::NewFolder, Shortcut::ctrl(Key::G), "creates a folder"),
    none("Layers", "Ctrl+Shift+C", "Copy layer content", "no content-only clipboard"),
    none("Layers", "Ctrl+Shift+V", "Paste layer content", "no content-only clipboard"),
    gesture("Layers", "Alt+LMB on mask", "Display mask in viewport", "already worked: opens the mask inspector"),
    gesture("Layers", "Shift+LMB on mask", "Disable/enable mask", "already worked"),
    none("Layers", "Ctrl+drag & drop", "Drag material", "no equivalent"),
    none("Layers", "Shift / Ctrl (drag)", "Manipulator snapping / mirror", "no manipulator"),
];

/// `docs/painter-shortcuts-table.md`: one row per entry of [`PAINTER_REFERENCE`] (a test keeps the
/// file equal to this).
pub fn reference_table_markdown() -> String {
    let mut out = String::from("| Group | Painter key | Painter action | SubCraft action | Status | Note |\n|---|---|---|---|---|---|\n");
    for e in PAINTER_REFERENCE {
        let action = e.action.map(|a| format!("{a:?}")).unwrap_or_default();
        let status = match e.support {
            Support::Same => "already the same",
            Support::Added => "added (key was free)",
            Support::PainterPreset => "Painter preset only (conflict)",
            Support::Gesture => "mouse / hold key",
            Support::None => "no equivalent",
        };
        out.push_str(&format!("| {} | `{}` | {} | {} | {} | {} |\n", e.group, e.keys, e.what, action, status, e.note));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shortcut table in the docs is this code's table. Run the test with
    /// `SUBCRAFT_WRITE_DOCS=1` to rewrite the file after changing [`PAINTER_REFERENCE`].
    #[test]
    fn docs_table_is_generated_from_the_reference() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/painter-shortcuts-table.md");
        let table = reference_table_markdown();
        if std::env::var_os("SUBCRAFT_WRITE_DOCS").is_some() {
            std::fs::write(path, &table).unwrap();
        }
        let on_disk = std::fs::read_to_string(path).unwrap().replace("\r\n", "\n");
        assert_eq!(on_disk.trim_end(), table.trim_end(), "docs/painter-shortcuts-table.md is out of date (rerun with SUBCRAFT_WRITE_DOCS=1)");
    }

    /// How much of Painter's documented list is covered, so a regression shows up as a number.
    #[test]
    fn painter_coverage_counts() {
        let count = |s: Support| PAINTER_REFERENCE.iter().filter(|e| e.support == s).count();
        let covered = PAINTER_REFERENCE.len() - count(Support::None);
        assert!(covered >= 63, "{covered} of {} Painter shortcuts are covered", PAINTER_REFERENCE.len());
        // What is left has no SubCraft feature behind it: manipulator, particles, projection tool,
        // quick mask, geometry mask, engine pause, stroke resume, content clipboard, material drag.
        assert!(count(Support::None) <= 26, "{}", count(Support::None));
    }

    #[test]
    fn shortcuts_resolve() {
        let sc = KeymapPreset::SubCraft;
        assert_eq!(action_for(sc, Shortcut::ctrl(Key::Z)), Some(Action::Undo));
        assert_eq!(action_for(sc, Shortcut::ctrl_shift(Key::Z)), Some(Action::Redo));
        assert_eq!(action_for(sc, Shortcut::key(Key::CloseBracket)), Some(Action::BrushLarger));
        assert_eq!(action_for(sc, Shortcut::key(Key::Z)), None, "modifiers must match exactly");
        assert_eq!(action_for(sc, Shortcut::key(Key::F10)), Some(Action::TogglePathTracing));
        assert_eq!(action_for(sc, Shortcut::ctrl_shift(Key::T)), Some(Action::NewTextLayer));
        assert_eq!(action_for(sc, Shortcut::ctrl(Key::A)), Some(Action::SelectAllLayers));
        assert_eq!(Shortcut::ctrl_shift(Key::N).label(), "Ctrl+Shift+N");
        assert_eq!(info(Action::Redo).label, "Redo");
    }

    #[test]
    fn no_duplicate_bindings_inside_a_preset() {
        for preset in KeymapPreset::ALL {
            let mut seen = std::collections::HashMap::new();
            for a in ACTIONS {
                for s in shortcuts_in(preset, a.action) {
                    if let Some(other) = seen.insert(*s, a.action) {
                        panic!("{preset:?}: {} bound to both {other:?} and {:?}", s.label(), a.action);
                    }
                }
            }
        }
    }

    #[test]
    fn every_action_is_listed_once() {
        use std::collections::HashSet;
        let all = Action::ALL;
        let listed: HashSet<_> = ACTIONS.iter().map(|a| a.action).collect();
        assert_eq!(listed.len(), ACTIONS.len(), "an action is listed twice");
        for a in all {
            assert!(listed.contains(&a), "{a:?} missing from ACTIONS");
        }
        assert_eq!(listed.len(), all.len());
    }

    #[test]
    fn painter_reference_matches_the_presets() {
        for entry in PAINTER_REFERENCE {
            match (entry.support, entry.action, entry.shortcut) {
                (Support::Same | Support::Added | Support::PainterPreset, Some(action), Some(sc)) => {
                    assert_eq!(action_for(KeymapPreset::Painter, sc), Some(action), "{} ({})", entry.keys, entry.what);
                    if entry.support == Support::PainterPreset {
                        assert_ne!(action_for(KeymapPreset::SubCraft, sc), Some(action), "{} is a preset-only binding", entry.keys);
                    } else {
                        assert_eq!(action_for(KeymapPreset::SubCraft, sc), Some(action), "{} must also work in the default preset", entry.keys);
                    }
                }
                (Support::Gesture | Support::None, None, None) => {}
                other => panic!("inconsistent reference row {}: {other:?}", entry.keys),
            }
        }
        assert!(PAINTER_REFERENCE.len() >= 89, "{}", PAINTER_REFERENCE.len());
    }

    #[test]
    fn painter_conflicts_keep_both_meanings_reachable() {
        let (sc, pa) = (KeymapPreset::SubCraft, KeymapPreset::Painter);
        // P: Pen in SubCraft, pick-material in Painter; Pen is still reachable in the Painter preset.
        assert_eq!(action_for(sc, Shortcut::key(Key::P)), Some(Action::PenTool));
        assert_eq!(action_for(pa, Shortcut::key(Key::P)), Some(Action::EyedropperTool));
        assert_eq!(action_for(pa, Shortcut::shift(Key::P)), Some(Action::PenTool));
        // X / L: symmetry; Painter's X inverts the mask brush.
        assert_eq!(action_for(sc, Shortcut::key(Key::X)), Some(Action::ToggleSymmetryX));
        assert_eq!(action_for(pa, Shortcut::key(Key::X)), Some(Action::InvertMaskGray));
        // B: Brush in SubCraft, the mesh map viewer in Painter (Brush stays on 1).
        assert_eq!(action_for(sc, Shortcut::key(Key::B)), Some(Action::BrushTool));
        assert_eq!(action_for(pa, Shortcut::key(Key::B)), Some(Action::NextMeshMap));
        assert_eq!(action_for(pa, Shortcut::key(Key::Num1)), Some(Action::BrushTool));
        // F2: layer rename in SubCraft, 3D-only layout in Painter.
        assert!(!f2_is_bound(sc) && f2_is_bound(pa));
        assert_eq!(action_for(pa, Shortcut::key(Key::L)), Some(Action::ToggleSymmetryX));
        // SubCraft bindings that Painter does not use survive in the Painter preset.
        for s in [Shortcut::key(Key::E), Shortcut::key(Key::Home), Shortcut::ctrl(Key::B), Shortcut::ctrl_shift(Key::Z)] {
            assert_eq!(action_for(pa, s), action_for(sc, s), "{}", s.label());
        }
    }

    /// Every action can be reached from the keyboard in the Painter key set, and every Painter
    /// key that SubCraft implements is a table entry or a documented gesture.
    #[test]
    fn every_action_is_bound_in_the_painter_key_set() {
        for a in Action::ALL {
            assert!(!shortcuts_in(KeymapPreset::Painter, a).is_empty(), "{a:?} has no key in the Painter key set");
        }
        // Only the Painter-conflict actions may be keyless in the SubCraft key set.
        let keyless: Vec<Action> = Action::ALL.into_iter().filter(|a| shortcuts_in(KeymapPreset::SubCraft, *a).is_empty()).collect();
        assert_eq!(keyless, [Action::EyedropperTool, Action::View3dOnly, Action::NextMeshMap, Action::InvertMaskGray]);
    }

    /// The keys Painter documents for features SubCraft has, spelled out (a typo in the table fails here).
    #[test]
    fn painter_key_set_matches_painters_documented_keys() {
        let pa = KeymapPreset::Painter;
        let expect = [
            (Shortcut::key(Key::Tab), Action::HideUi), (Shortcut::ctrl(Key::Z), Action::Undo), (Shortcut::ctrl(Key::Y), Action::Redo),
            (Shortcut::key(Key::F8), Action::Bake), (Shortcut::key(Key::F9), Action::PaintingMode), (Shortcut::key(Key::F10), Action::TogglePathTracing),
            (Shortcut::ctrl(Key::O), Action::OpenProject), (Shortcut::ctrl(Key::F4), Action::CloseProject), (Shortcut::ctrl(Key::S), Action::Save),
            (Shortcut::ctrl(Key::N), Action::NewProject), (Shortcut::alt(Key::F4), Action::Quit), (Shortcut::key(Key::L), Action::ToggleSymmetryX),
            (Shortcut::key(Key::Num1), Action::BrushTool), (Shortcut::key(Key::Num2), Action::EraserTool), (Shortcut::key(Key::Num4), Action::PolygonFillTool),
            (Shortcut::key(Key::Num5), Action::SmudgeTool), (Shortcut::key(Key::Num6), Action::CloneTool), (Shortcut::ctrl_shift(Key::B), Action::Bake),
            (Shortcut::key(Key::CloseBracket), Action::BrushLarger), (Shortcut::key(Key::OpenBracket), Action::BrushSmaller), (Shortcut::key(Key::X), Action::InvertMaskGray),
            (Shortcut::key(Key::P), Action::EyedropperTool), (Shortcut::key(Key::D), Action::ToggleLazyMouse), (Shortcut::key(Key::F5), Action::PerspectiveCamera),
            (Shortcut::key(Key::F6), Action::OrthographicCamera), (Shortcut::key(Key::C), Action::NextChannel), (Shortcut::shift(Key::C), Action::PrevChannel),
            (Shortcut::key(Key::M), Action::DisplayMaterial), (Shortcut::key(Key::B), Action::NextMeshMap), (Shortcut::shift(Key::B), Action::PrevMeshMap),
            (Shortcut::ctrl_shift(Key::E), Action::Export), (Shortcut::key(Key::F), Action::ResetCamera), (Shortcut::key(Key::F1), Action::View3dAnd2d),
            (Shortcut::key(Key::F2), Action::View3dOnly), (Shortcut::key(Key::F3), Action::View2dOnly), (Shortcut::key(Key::F4), Action::SwapViews),
            (Shortcut::alt(Key::Q), Action::IsolateTextureSet), (Shortcut::key(Key::Delete), Action::DeleteLayer), (Shortcut::ctrl(Key::D), Action::DuplicateLayer),
            (Shortcut::ctrl(Key::G), Action::NewFolder),
        ];
        for (s, a) in expect {
            assert_eq!(action_for(pa, s), Some(a), "{}", s.label());
        }
        // Hold keys of the viewport (S stencil, N ignore stencil, V clone source) and the keys of
        // Painter features SubCraft lacks stay free in the Painter key set, so they never do something else.
        for k in [Key::S, Key::N, Key::V, Key::Q, Key::W, Key::R, Key::T, Key::U, Key::Y, Key::A, Key::Num3, Key::Space] {
            assert_eq!(action_for(pa, Shortcut::key(k)), None, "{} must stay free", k.label());
        }
    }

    #[test]
    fn preset_round_trips_and_defaults() {
        for p in KeymapPreset::ALL {
            let json = serde_json::to_string(&p).unwrap();
            assert_eq!(serde_json::from_str::<KeymapPreset>(&json).unwrap(), p);
        }
        assert_eq!(KeymapPreset::default(), KeymapPreset::SubCraft);
    }

    #[test]
    fn key_labels() {
        assert_eq!(Key::A.label(), "A");
        assert_eq!(Key::Z.label(), "Z");
        assert_eq!(Key::Num0.label(), "0");
        assert_eq!(Key::Num6.label(), "6");
        assert_eq!(Key::F1.label(), "F1");
        assert_eq!(Key::F12.label(), "F12");
        assert_eq!(Shortcut::alt(Key::F4).label(), "Alt+F4");
    }
}
