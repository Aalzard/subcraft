//! Pen presentation: canvas coordinates and hit testing only. Editing/raster/history live in app.
use egui::{Color32, Pos2, Stroke, Ui};
use glam::Vec2;
use subcraft_app::pen::{Handle, PenHit};
use subcraft_doc::PenPath;
use crate::{state::{Tool, UiState}, theme};

#[derive(Clone, Copy)]
pub enum After { Action(subcraft_app::Action), Request(crate::dialogs::Pending) }

pub fn hit(path: &PenPath, selected: Option<subcraft_core::AnchorId>, pointer: Pos2, map: impl Fn(Vec2) -> Option<Pos2>) -> Option<PenHit> {
    let mut best = None; let mut distance = 9.0;
    for a in &path.anchors {
        let mut candidates = vec![(Handle::Anchor, a.position)];
        if selected == Some(a.id) {
            if a.incoming.length_squared() > 1e-10 { candidates.push((Handle::Incoming, a.position + a.incoming)); }
            if a.outgoing.length_squared() > 1e-10 { candidates.push((Handle::Outgoing, a.position + a.outgoing)); }
        }
        for (handle, uv) in candidates {
            if let Some(p) = map(uv) && p.distance(pointer) < distance {
                distance = p.distance(pointer); best = Some(PenHit { anchor: a.id, handle });
            }
        }
    }
    best
}

/// The curve point nearest `pointer` within a few points on screen: segment and curve
/// parameter. Segments are sampled finely enough for a click on the visible curve.
pub fn curve_hit(path: &PenPath, pointer: Pos2, map: impl Fn(Vec2) -> Option<Pos2>) -> Option<(usize, f32)> {
    const STEPS: usize = 32;
    let mut best: Option<(f32, usize, f32)> = None;
    for seg in 0..path.segment_count() {
        let Some([p0, p1, p2, p3]) = path.segment(seg) else { continue };
        let at = |t: f32| {
            let u = 1.0 - t;
            p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t)
        };
        let mut prev: Option<(Pos2, f32)> = None;
        for k in 0..=STEPS {
            let t = k as f32 / STEPS as f32;
            let Some(s) = map(at(t)) else { prev = None; continue };
            if let Some((a, ta)) = prev {
                let d = s - a;
                let f = if d.length_sq() > 1e-6 { ((pointer - a).dot(d) / d.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
                let dist = pointer.distance(a + d * f);
                if dist <= 6.0 && best.is_none_or(|b| dist < b.0) {
                    best = Some((dist, seg, ta + (t - ta) * f));
                }
            }
            prev = Some((s, t));
        }
    }
    best.map(|(_, seg, t)| (seg, t))
}

pub fn draw(painter: &egui::Painter, path: &PenPath, selected: Option<subcraft_core::AnchorId>, map: impl Fn(Vec2) -> Option<Pos2>) {
    if let Ok(points) = path.polyline(0.002) {
        for pair in points.windows(2) {
            if let (Some(a), Some(b)) = (map(pair[0]), map(pair[1])) { painter.line_segment([a, b], Stroke::new(1.5, theme::ACCENT())); }
        }
    }
    for a in &path.anchors {
        let Some(p) = map(a.position) else { continue; };
        if selected == Some(a.id) {
            for tangent in [a.incoming, a.outgoing] {
                if tangent.length_squared() > 1e-10 && let Some(q) = map(a.position + tangent) {
                    painter.line_segment([p, q], Stroke::new(1.0, theme::OVERLAY_TEXT_DIM));
                    painter.circle_filled(q, 3.0, theme::OVERLAY_TEXT);
                    painter.circle_stroke(q, 3.0, Stroke::new(1.0, theme::OVERLAY_OUTLINE));
                }
            }
        }
        painter.circle_filled(p, 4.5, if selected == Some(a.id) { theme::ACCENT() } else { theme::OVERLAY_TEXT });
        painter.circle_stroke(p, 4.5, Stroke::new(1.0, Color32::BLACK));
    }
}

pub fn keyboard(state: &mut UiState, ctx: &egui::Context, now: f64) {
    // Not behind a modal window: Enter / Escape belong to it, and Backspace deleted the path
    // layer behind the unsaved-changes prompt.
    if state.tool != Tool::Pen || ctx.egui_wants_keyboard_input() || state.modal_open() { return; }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        if state.pen.is_editing() || state.pen.is_pending() { state.pen.undo_pending(&state.session); }
        else { state.pen.finish_path(); }
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
        if let Err(e) = state.pen.release(&state.session) { state.set_status(e.to_string(), now); }
        state.pen.finish_path();
    }
    // Only while the selected point is in the path on screen: the selection outlives its path
    // (another layer selected, the point undone), and Delete then did nothing at all instead of
    // reaching the layer shortcut.
    let selected = state.pen.selected_anchor().is_some_and(|id| state.pen.preview(&state.session).is_some_and(|p| p.anchors.iter().any(|a| a.id == id)));
    if selected && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete))
        && let Err(e) = state.pen.delete_anchor(&state.session) { state.set_status(e.to_string(), now); }
    // Backspace deletes the whole path (Painter 11): its layer, when the selected layer is one.
    let path_layer = state.session.selected_layer().filter(|l| matches!(&l.content, subcraft_doc::LayerContent::Paint(p) if p.path.is_some())).map(|l| l.id);
    if let Some(layer) = path_layer && !state.pen.is_editing() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)) {
        state.pen.cancel();
        match state.session.execute(subcraft_app::Command::DeleteLayer(layer)) {
            Ok(()) => state.set_status("Path deleted", now),
            Err(e) => state.set_status(e.to_string(), now),
        }
    }
}

pub fn finish_frame(state: &mut UiState, ctx: &egui::Context, now: f64) {
    if ctx.input(|i| i.pointer.primary_released()) && let Err(e) = state.pen.release(&state.session) { state.set_status(e.to_string(), now); }
    if state.pen.is_pending() {
        match state.pen.poll(&mut state.session, 16384) {
            Ok(true) => { state.set_status(format!("Drawing pen path… {:.0}%", state.pen.progress() * 100.0), now); ctx.request_repaint(); }
            Ok(false) => { state.set_status("Pen path updated", now); ctx.request_repaint(); }
            Err(e) => state.set_status(format!("Pen: {e}"), now),
        }
    }
}

pub fn uv_input(state: &mut UiState, ui: &Ui, response: &egui::Response, image: crate::panels::uv_view::Frame) {
    if state.tool != Tool::Pen || ui.ctx().egui_wants_keyboard_input() { return; }
    let (pointer, pressed, down, alt, separate) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_pressed(), i.pointer.primary_down(), i.modifiers.alt, i.modifiers.shift));
    let Some(pointer) = pointer else { return; };
    if alt { return; }
    let map = |uv: Vec2| Some(image.to_px(uv));
    let uv = image.to_uv(pointer);
    if pressed && response.hovered() {
        let target = state.pen.preview(&state.session).and_then(|p| hit(p, state.pen.selected_anchor(), pointer, map));
        if (target.is_some() || image.contains(pointer)) && let Err(e) = state.pen.begin(&state.session, uv, target) { state.set_status(format!("Pen: {e}"), ui.input(|i| i.time)); }
    } else if down && state.pen.is_editing() { state.pen.drag_to(uv, separate); }
}

/// Ribbon path: its image (the current brush tip), Stretching & Tiling mode, copies or aspect
/// factor, mirror every second copy, flip across the path, and whether Base Color takes the
/// image's colour.
fn ribbon_rows(state: &mut UiState, ui: &mut Ui, path: &mut PenPath) -> bool {
    use subcraft_doc::{PathRibbon, RibbonMode};
    let mut changed = false;
    let ribbon = path.ribbon.get_or_insert_with(PathRibbon::default);
    let name = ribbon.image.and_then(|id| state.session.resources().get(id)).map_or("none (solid strip)".to_string(), |r| r.name.clone());
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("Image: {name}")).color(crate::theme::TEXT_DIM()));
        if ui.small_button("Use current brush tip").on_hover_text("Lay the tip chosen in the Brush settings / Shelf along the path").clicked() {
            ribbon.image = state.session.brush_tip_resource();
            changed = true;
        }
        if ribbon.image.is_some() && ui.small_button("Clear").clicked() {
            ribbon.image = None;
            changed = true;
        }
    });
    crate::widgets::prop_row(ui, "Mode", |ui| {
        egui::ComboBox::from_id_salt("ribbon_mode").selected_text(ribbon.mode.label()).show_ui(ui, |ui| {
            for mode in RibbonMode::ALL {
                changed |= ui.selectable_value(&mut ribbon.mode, mode, mode.label()).changed();
            }
        });
    });
    let label = if ribbon.mode == RibbonMode::Stretch { "Copies" } else { "Aspect factor" };
    changed |= crate::widgets::slider_row(ui, label, egui::Slider::new(&mut ribbon.tiles, PathRibbon::TILES).logarithmic(true)).changed();
    ui.horizontal(|ui| {
        changed |= ui.checkbox(&mut ribbon.mirror, "Mirror every 2nd copy").changed();
        changed |= ui.checkbox(&mut ribbon.flip, "Flip image").changed();
    });
    changed |= ui.checkbox(&mut ribbon.color_from_image, "Base Color from the image").on_hover_text("Otherwise the image only shapes the strip and Base Color is the value below").changed();
    changed
}

pub fn properties(state: &mut UiState, ui: &mut Ui, path: &PenPath, now: f64) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("New path").clicked() { state.pen.new_path(); state.tool = Tool::Pen; }
        let selected = state.pen.selected_anchor().is_some();
        for (label, smooth) in [("Corner", false), ("Smooth", true)] {
            if ui.add_enabled(selected, egui::Button::new(label)).clicked() && let Err(e) = state.pen.shape_point(&state.session, smooth) { state.set_status(e.to_string(), now); }
        }
        if ui.add_enabled(selected, egui::Button::new("Delete point")).clicked() && let Err(e) = state.pen.delete_anchor(&state.session) { state.set_status(e.to_string(), now); }
    });
    let mut path = path.clone(); let mut changed = false;
    // Painter's path types: a stroke along the curve, or its filled interior.
    crate::widgets::prop_row(ui, "Path type", |ui| {
        egui::ComboBox::from_id_salt("pen_path_kind").selected_text(path.kind.label()).show_ui(ui, |ui| {
            for kind in subcraft_doc::PathKind::ALL {
                changed |= ui.selectable_value(&mut path.kind, kind, kind.label()).changed();
            }
        });
    });
    let filled = path.kind == subcraft_doc::PathKind::Filled;
    if path.kind == subcraft_doc::PathKind::Ribbon {
        changed |= ui.add_enabled(path.anchors.len() > 1, egui::Checkbox::new(&mut path.closed, "Closed path")).changed();
        let mut width = path.radius * 200.0;
        changed |= crate::widgets::slider_row(ui, "Width (% UV)", egui::Slider::new(&mut width, 0.01..=100.0).logarithmic(true)).changed();
        path.radius = width / 200.0;
        changed |= ribbon_rows(state, ui, &mut path);
    } else if filled {
        ui.label(egui::RichText::new("The interior is filled; the path closes on its first point").small().color(crate::theme::TEXT_DIM()));
    } else {
        changed |= ui.add_enabled(path.anchors.len() > 1, egui::Checkbox::new(&mut path.closed, "Closed path")).changed();
        let mut width = path.radius * 200.0;
        changed |= crate::widgets::slider_row(ui, "Width (% UV)", egui::Slider::new(&mut width, 0.01..=100.0).logarithmic(true)).changed();
        path.radius = width / 200.0;
        changed |= crate::widgets::slider_row(ui, "Hardness", egui::Slider::new(&mut path.hardness, 0.0..=1.0)).changed();
        // Brush tip stamped along the path, with spacing (Painter's brush on a path).
        let mut stamped = path.stamp.is_some();
        if ui.checkbox(&mut stamped, "Stamp the brush tip").on_hover_text("Place the current brush tip along the path instead of one continuous stroke").changed() {
            changed = true;
            path.stamp = stamped.then(|| subcraft_doc::PathStamp { alpha: state.session.brush_tip_resource(), ..Default::default() });
        }
        if let Some(stamp) = path.stamp.as_mut() {
            let mut spacing = stamp.spacing * 100.0;
            changed |= crate::widgets::slider_row(ui, "Spacing (%)", egui::Slider::new(&mut spacing, 2.0..=400.0).logarithmic(true)).changed();
            stamp.spacing = (spacing / 100.0).clamp(*subcraft_doc::PathStamp::SPACING.start(), *subcraft_doc::PathStamp::SPACING.end());
            changed |= ui.checkbox(&mut stamp.follow, "Follow path").on_hover_text("Turn each stamp along the curve").changed();
            let tip = stamp.alpha.and_then(|id| state.session.resources().get(id)).map_or("Round".to_string(), |r| r.name.clone());
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("Tip: {tip}")).color(crate::theme::TEXT_DIM()));
                if ui.small_button("Use current brush tip").on_hover_text("Stamp the tip chosen in the Brush settings / Shelf").clicked() {
                    stamp.alpha = state.session.brush_tip_resource();
                    changed = true;
                }
            });
        }
    }
    changed |= crate::widgets::slider_row(ui, "Opacity", egui::Slider::new(&mut path.opacity, 0.0..=1.0)).changed();
    for kind in state.session.channels() {
        let mut enabled = path.values.contains_key(&kind);
        let editable = !enabled || path.values.len() > 1;
        let r = ui.add_enabled(editable, egui::Checkbox::new(&mut enabled, kind.display_name())).on_hover_text("Choose the channels this path paints. Hide the layer to hide all channels.");
        if r.changed() {
            changed = true;
            if enabled { path.values.insert(kind, kind.default_paint_value()); }
            else { path.values.remove(&kind); }
        }
        if let Some(value) = path.values.get_mut(&kind) {
            crate::widgets::prop_row(ui, "Value", |ui| { changed |= crate::widgets::channel_value(ui, kind, value).changed; });
        }
    }
    if changed {
        if let Err(e) = state.pen.set_path(&state.session, path) { state.set_status(e.to_string(), now); }
        if !ui.input(|i| i.pointer.primary_down()) && let Err(e) = state.pen.release(&state.session) { state.set_status(e.to_string(), now); }
    }
    crate::widgets::hint(ui, "Click to add a point; drag on creation for Bézier handles. Drag points or handles to edit; Shift breaks the mirrored handle. Enter finishes, Esc cancels an edit, Delete removes a point. Middle/Alt drag pans in 2D.");
}

#[cfg(test)]
mod keyboard_tests {
    use super::*;

    /// QA 2026-11: the pen keys (Enter, Escape, Delete, Backspace) had no modal check: they
    /// acted behind the unsaved-changes prompt and New Project, and took the dialog's Enter.
    #[test]
    fn pen_keys_do_nothing_behind_a_modal_window() {
        let mut state = UiState::new(subcraft_app::Session::sample(), None);
        state.tool = Tool::Pen;
        let ctx = egui::Context::default();
        let enter_left = |state: &mut UiState| {
            let events = vec![egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }];
            let mut left = None;
            let mut out = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                keyboard(state, ui.ctx(), 0.0);
                left = Some(ui.input(|i| i.key_pressed(egui::Key::Enter)));
            });
            out.textures_delta.clear();
            left.unwrap()
        };
        state.confirm = Some(crate::dialogs::Pending::Close);
        assert!(enter_left(&mut state), "the dialog keeps its Enter");
        state.confirm = None;
        state.new_project.open = true;
        assert!(enter_left(&mut state), "New Project keeps its Enter");
        state.new_project.open = false;
        assert!(!enter_left(&mut state), "without a modal window the pen takes Enter");
    }
}
