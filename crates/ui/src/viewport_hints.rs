//! The 3D view's bottom-left hint list, modeled on Substance Painter's: the mouse and key actions
//! that apply right now. What it lists depends on the active tool, the held modifiers and a few
//! document facts (hidden parts, a stencil, mask painting, Rendering mode).
//!
//! [`build`] is pure (no egui) and takes everything it needs in a [`HintContext`], so every row is
//! unit-tested. Keys come from the active keymap preset (`subcraft_app::label_in`), so the list
//! stays right when the Painter key set is chosen. The viewport rebuilds the rows only when the
//! context changes (see `cached`); drawing reuses the cached rows.

use std::sync::Arc;

use subcraft_app::{Action, Keymap};
#[cfg(test)]
use subcraft_app::KeymapPreset;

use crate::nav::NavScheme;
use crate::state::Tool;

/// One hint: the key / mouse glyph and a short label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hint {
    pub keys: String,
    pub what: &'static str,
}

/// Everything the list depends on. Cheap to build and compare every frame.
#[derive(Clone, Debug, PartialEq)]
pub struct HintContext {
    pub tool: Tool,
    pub nav: NavScheme,
    pub keymap: Keymap,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub tracing: bool,
    pub hidden_parts: bool,
    pub stencil: bool,
    pub mask: bool,
    /// Clicking parts shows / hides the layer on them.
    pub picking_parts: bool,
    /// A text layer or stamp waits for a click.
    pub text_armed: bool,
}

fn h(keys: impl Into<String>, what: &'static str) -> Hint {
    Hint { keys: keys.into(), what }
}

/// The key of `action` in the active set, if it has one.
fn key(c: &HintContext, action: Action) -> Option<String> {
    let s = c.keymap.label(action);
    (!s.is_empty()).then(|| crate::keys::native_string(s))
}

fn camera(c: &HintContext, out: &mut Vec<Hint>) {
    for (what, how) in c.nav.help() {
        out.push(h(how.replace(" drag", ""), what));
    }
    if let Some(k) = key(c, Action::ResetCamera) {
        out.push(h(k, "Frame model"));
    }
}

fn paint_verb(tool: Tool) -> &'static str {
    match tool {
        Tool::Eraser => "Erase",
        Tool::Smudge => "Smudge",
        Tool::Clone => "Clone",
        _ => "Paint",
    }
}

/// The hint rows for `c`, most relevant first (the view drops rows from the end when short of room).
pub fn build(c: &HintContext) -> Vec<Hint> {
    let mut out = Vec::with_capacity(14);

    if c.tracing {
        if let Some(k) = key(c, Action::TogglePathTracing) {
            out.push(h(k, "Leave Rendering mode"));
        }
        out.push(h("Rendering bar", "Pause / denoise / samples"));
        camera(c, &mut out);
        return out;
    }
    // Alt held: only navigation counts (Painter shows the camera gestures).
    if c.alt && c.nav.alt_navigates() {
        out.push(h("Alt+LMB", "Orbit"));
        out.push(h("Alt+MMB", "Pan"));
        out.push(h("Ctrl+Alt+LMB", "Pan"));
        out.push(h("Alt+RMB", "Zoom"));
        return out;
    }
    if c.ctrl {
        match c.tool {
            Tool::Brush | Tool::Eraser | Tool::Smudge => {
                out.push(h("Ctrl+RMB drag", "Size / hardness"));
                out.push(h("Ctrl+LMB drag", "Flow / angle"));
            }
            Tool::Clone => {
                out.push(h("Ctrl+RMB drag", "Size / hardness"));
                out.push(h("Ctrl+LMB", "Set clone source"));
            }
            _ => {}
        }
        for (action, what) in [(Action::Undo, "Undo"), (Action::Redo, "Redo"), (Action::Save, "Save"), (Action::Export, "Export textures")] {
            if let Some(k) = key(c, action) {
                out.push(h(k, what));
            }
        }
        return out;
    }
    if c.shift {
        match c.tool {
            Tool::Brush | Tool::Eraser | Tool::Smudge | Tool::Clone => out.push(h("Shift+LMB", "Straight line from the last point")),
            Tool::Eyedropper => out.push(h("Shift+LMB", "Pick material")),
            _ => {}
        }
        if let Some(k) = key(c, Action::PrevChannel) {
            out.push(h(k, "Previous channel"));
        }
        camera(c, &mut out);
        return out;
    }

    // Modal states first.
    if c.text_armed {
        out.push(h("LMB", "Place text"));
        out.push(h("Esc", "Cancel"));
        return out;
    } else if c.picking_parts {
        out.push(h("LMB", "Show / hide the layer on a part"));
        out.push(h("Esc", "Done"));
        return out;
    } else {
        match c.tool {
            Tool::Pen => {
                out.push(h("LMB", "Add point"));
                out.push(h("LMB drag", "Curve handle"));
                out.push(h("Enter", "Finish path"));
                out.push(h("Esc", "Cancel"));
            }
            Tool::PolygonFill => {
                out.push(h("LMB", "Fill polygon"));
                out.push(h("LMB drag", "Box fill"));
            }
            Tool::Eyedropper => {
                out.push(h("LMB", "Pick color"));
                out.push(h("Shift+LMB", "Pick material"));
            }
            _ => {
                out.push(h("LMB", if c.mask { "Paint mask" } else { paint_verb(c.tool) }));
                if c.tool == Tool::Clone {
                    out.push(h("Ctrl+LMB / V+LMB", "Set clone source"));
                }
                out.push(h("Shift+LMB", "Straight line"));
                out.push(h("Ctrl+RMB drag", "Size / hardness"));
                if c.tool != Tool::Clone {
                    out.push(h("Ctrl+LMB drag", "Flow / angle"));
                }
                if let (Some(a), Some(b)) = (key(c, Action::BrushSmaller), key(c, Action::BrushLarger)) {
                    out.push(h(format!("{a} {b}"), "Brush size"));
                }
                if let Some(k) = key(c, Action::ToggleSymmetryX) {
                    out.push(h(k, "Symmetry X"));
                }
                if let Some(k) = key(c, Action::ToggleLazyMouse) {
                    out.push(h(k, "Lazy mouse"));
                }
                if c.stencil {
                    out.push(h("S+LMB / MMB / RMB", "Rotate / move / scale stencil"));
                    out.push(h("N (hold)", "Ignore stencil"));
                }
            }
        }
    }
    out.push(h("RMB click", "Properties / part menu"));
    out.push(h("I / H", "Isolate / hide part"));
    if c.hidden_parts {
        out.push(h("Esc", "Show all parts"));
    }
    camera(c, &mut out);
    if let Some(k) = key(c, Action::NextChannel) {
        out.push(h(k, "Next channel"));
    }
    if let Some(k) = key(c, Action::TogglePathTracing) {
        out.push(h(k, "Rendering mode"));
    }
    out
}

/// Paint `rows` bottom-left of `rect`: small, low-contrast, one line each. Rows that do not fit in the
/// lower 40% of the view are dropped from the end so the model stays visible.
pub fn draw(painter: &egui::Painter, rect: egui::Rect, rows: &[Hint]) {
    use egui::{Color32, Pos2, vec2};
    let font = crate::theme::font(11.0);
    let line = 17.0;
    let fit = (((rect.height() * 0.4) / line) as usize).min(rows.len());
    if fit == 0 {
        return;
    }
    let rows = &rows[..fit];
    let keys: Vec<_> = rows.iter().map(|r| painter.layout_no_wrap(r.keys.clone(), font.clone(), crate::theme::TEXT_DIM)).collect();
    let key_w = keys.iter().map(|g| g.size().x).fold(0.0, f32::max) + 12.0;
    let label_w = rows.iter().map(|r| painter.layout_no_wrap(r.what.to_string(), font.clone(), crate::theme::TEXT_FAINT).size().x).fold(0.0, f32::max);
    let size = vec2(key_w + label_w + 16.0, line * rows.len() as f32 + 8.0);
    let top_left = egui::Align2::LEFT_BOTTOM.anchor_size(rect.left_bottom() + vec2(10.0, -10.0), size).min;
    painter.rect_filled(egui::Rect::from_min_size(top_left, size), 3.0, Color32::from_black_alpha(70));
    for (i, (row, glyph)) in rows.iter().zip(keys).enumerate() {
        let y = top_left.y + 4.0 + line * i as f32;
        painter.galley(Pos2::new(top_left.x + 8.0, y), glyph, crate::theme::TEXT_DIM);
        painter.text(Pos2::new(top_left.x + 8.0 + key_w, y), egui::Align2::LEFT_TOP, row.what, font.clone(), crate::theme::TEXT_FAINT);
    }
}

/// Rows for `c`, rebuilt only when the context differs from the previous frame's.
pub fn cached(ctx: &egui::Context, c: HintContext) -> Arc<Vec<Hint>> {
    let id = egui::Id::new("viewport_hints");
    if let Some((old, rows)) = ctx.data(|d| d.get_temp::<(HintContextKey, Arc<Vec<Hint>>)>(id))
        && old.0 == c
    {
        return rows;
    }
    let rows = Arc::new(build(&c));
    ctx.data_mut(|d| d.insert_temp(id, (HintContextKey(c), rows.clone())));
    rows
}

/// `HintContext` for egui's temp storage (needs `Send + Sync + Clone`).
#[derive(Clone)]
struct HintContextKey(HintContext);

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(tool: Tool) -> HintContext {
        HintContext {
            tool,
            nav: NavScheme::Painter,
            keymap: KeymapPreset::SubCraft.into(),
            ctrl: false,
            shift: false,
            alt: false,
            tracing: false,
            hidden_parts: false,
            stencil: false,
            mask: false,
            picking_parts: false,
            text_armed: false,
        }
    }
    fn whats(c: &HintContext) -> Vec<&'static str> {
        build(c).iter().map(|r| r.what).collect()
    }
    fn keys_of(c: &HintContext, what: &str) -> String {
        build(c).iter().find(|r| r.what == what).map(|r| r.keys.clone()).unwrap_or_else(|| panic!("no row {what}"))
    }

    #[test]
    fn brush_lists_painting_camera_and_part_hints() {
        let w = whats(&ctx(Tool::Brush));
        for want in ["Paint", "Straight line", "Size / hardness", "Flow / angle", "Brush size", "Symmetry X", "Lazy mouse", "Orbit", "Pan", "Zoom", "Frame model", "Properties / part menu", "Isolate / hide part"] {
            assert!(w.contains(&want), "{want} missing from {w:?}");
        }
        assert!(!w.contains(&"Show all parts"), "only when something is hidden");
        assert!(!w.contains(&"Rotate / move / scale stencil"));
    }

    #[test]
    fn rows_follow_the_tool() {
        assert!(whats(&ctx(Tool::Eraser)).contains(&"Erase"));
        assert!(whats(&ctx(Tool::Smudge)).contains(&"Smudge"));
        let clone = whats(&ctx(Tool::Clone));
        assert!(clone.contains(&"Set clone source") && !clone.contains(&"Flow / angle"));
        assert!(whats(&ctx(Tool::PolygonFill)).contains(&"Fill polygon"));
        let eye = whats(&ctx(Tool::Eyedropper));
        assert!(eye.contains(&"Pick color") && eye.contains(&"Pick material") && !eye.contains(&"Brush size"));
        let pen = whats(&ctx(Tool::Pen));
        assert!(pen.contains(&"Add point") && pen.contains(&"Finish path") && !pen.contains(&"Symmetry X"));
        let mut m = ctx(Tool::Brush);
        m.mask = true;
        assert!(whats(&m).contains(&"Paint mask"));
    }

    #[test]
    fn document_state_adds_rows() {
        let mut c = ctx(Tool::Brush);
        c.hidden_parts = true;
        c.stencil = true;
        let w = whats(&c);
        assert!(w.contains(&"Show all parts") && w.contains(&"Ignore stencil") && w.contains(&"Rotate / move / scale stencil"));
        c.picking_parts = true;
        assert_eq!(whats(&c), vec!["Show / hide the layer on a part", "Done"]);
        c.picking_parts = false;
        c.text_armed = true;
        assert_eq!(whats(&c), vec!["Place text", "Cancel"]);
    }

    #[test]
    fn held_modifiers_change_the_list() {
        let mut c = ctx(Tool::Brush);
        c.alt = true;
        assert_eq!(whats(&c), vec!["Orbit", "Pan", "Pan", "Zoom"]);
        c.alt = false;
        c.ctrl = true;
        let w = whats(&c);
        assert!(w.contains(&"Size / hardness") && w.contains(&"Undo") && !w.contains(&"Orbit"));
        c.ctrl = false;
        c.shift = true;
        let w = whats(&c);
        assert!(w.contains(&"Straight line from the last point") && w.contains(&"Orbit") && !w.contains(&"Lazy mouse"));
        // Blender scheme: Alt is not a navigation modifier, so it leaves the normal list.
        let mut b = ctx(Tool::Brush);
        b.nav = NavScheme::Blender;
        b.alt = true;
        assert!(whats(&b).contains(&"Paint"));
    }

    #[test]
    fn keys_come_from_the_active_keymap() {
        let mut c = ctx(Tool::Brush);
        assert_eq!(keys_of(&c, "Frame model"), "Home");
        assert_eq!(keys_of(&c, "Symmetry X"), "X");
        assert_eq!(keys_of(&c, "Rendering mode"), "F10");
        c.keymap = KeymapPreset::Painter.into();
        assert_eq!(keys_of(&c, "Symmetry X"), "L", "the Painter preset moves symmetry to L");
        c.tool = Tool::Eyedropper;
        c.keymap = KeymapPreset::SubCraft.into();
        assert_eq!(keys_of(&ctx(Tool::Brush), "Brush size"), "[ ]");
        // A key the user rebound in Settings > Shortcuts shows up here too.
        c.keymap.bind(Action::ResetCamera, subcraft_app::Shortcut::key(subcraft_app::Key::G));
        c.keymap.unbind(Action::ResetCamera, subcraft_app::Shortcut::key(subcraft_app::Key::Home));
        c.tool = Tool::Brush;
        assert_eq!(keys_of(&c, "Frame model"), "F");
    }

    #[test]
    fn rendering_mode_lists_exit_pause_and_camera() {
        let mut c = ctx(Tool::Brush);
        c.tracing = true;
        let w = whats(&c);
        assert_eq!(w[0], "Leave Rendering mode");
        assert!(w.contains(&"Pause / denoise / samples") && w.contains(&"Orbit") && !w.contains(&"Paint"));
        assert_eq!(keys_of(&c, "Leave Rendering mode"), "F10");
    }

    #[test]
    fn camera_rows_follow_the_navigation_scheme() {
        let mut c = ctx(Tool::Brush);
        assert_eq!(keys_of(&c, "Orbit"), "Alt+LMB or RMB (add Shift to snap every 90°)");
        c.nav = NavScheme::Blender;
        assert_eq!(keys_of(&c, "Orbit"), "MMB (Alt+MMB click centers the view on the model)");
        assert_eq!(keys_of(&c, "Pan"), "Shift+MMB");
    }

    #[test]
    fn list_stays_short_enough_to_be_compact() {
        for tool in [Tool::Brush, Tool::Eraser, Tool::Clone, Tool::Smudge, Tool::PolygonFill, Tool::Eyedropper, Tool::Pen] {
            let mut c = ctx(tool);
            c.hidden_parts = true;
            c.stencil = true;
            assert!(build(&c).len() <= 20, "{tool:?}");
        }
    }
}
