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

pub fn draw(painter: &egui::Painter, path: &PenPath, selected: Option<subcraft_core::AnchorId>, map: impl Fn(Vec2) -> Option<Pos2>) {
    if let Ok(points) = path.polyline(0.002) {
        for pair in points.windows(2) {
            if let (Some(a), Some(b)) = (map(pair[0]), map(pair[1])) { painter.line_segment([a, b], Stroke::new(1.5, theme::ACCENT)); }
        }
    }
    for a in &path.anchors {
        let Some(p) = map(a.position) else { continue; };
        if selected == Some(a.id) {
            for tangent in [a.incoming, a.outgoing] {
                if tangent.length_squared() > 1e-10 && let Some(q) = map(a.position + tangent) {
                    painter.line_segment([p, q], Stroke::new(1.0, theme::TEXT_DIM));
                    painter.circle_filled(q, 3.0, theme::TEXT);
                    painter.circle_stroke(q, 3.0, Stroke::new(1.0, theme::BG_0));
                }
            }
        }
        painter.circle_filled(p, 4.5, if selected == Some(a.id) { theme::ACCENT } else { theme::TEXT });
        painter.circle_stroke(p, 4.5, Stroke::new(1.0, Color32::BLACK));
    }
}

pub fn keyboard(state: &mut UiState, ctx: &egui::Context, now: f64) {
    if state.tool != Tool::Pen || ctx.egui_wants_keyboard_input() { return; }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        if state.pen.is_editing() || state.pen.is_pending() { state.pen.undo_pending(&state.session); }
        else { state.pen.finish_path(); }
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
        if let Err(e) = state.pen.release(&state.session) { state.set_status(e.to_string(), now); }
        state.pen.finish_path();
    }
    if state.pen.selected_anchor().is_some() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete) || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace))
        && let Err(e) = state.pen.delete_anchor(&state.session) { state.set_status(e.to_string(), now); }
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

pub fn uv_input(state: &mut UiState, ui: &Ui, response: &egui::Response, image: egui::Rect) {
    if state.tool != Tool::Pen || ui.ctx().egui_wants_keyboard_input() { return; }
    let (pointer, pressed, down, alt, separate) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_pressed(), i.pointer.primary_down(), i.modifiers.alt, i.modifiers.shift));
    let Some(pointer) = pointer else { return; };
    if alt { return; }
    let map = |uv: Vec2| Some(image.min + egui::vec2(uv.x, uv.y) * image.width());
    let uv = Vec2::new((pointer.x - image.left()) / image.width(), (pointer.y - image.top()) / image.height());
    if pressed && response.hovered() {
        let target = state.pen.preview(&state.session).and_then(|p| hit(p, state.pen.selected_anchor(), pointer, map));
        if (target.is_some() || image.contains(pointer)) && let Err(e) = state.pen.begin(&state.session, uv, target) { state.set_status(format!("Pen: {e}"), ui.input(|i| i.time)); }
    } else if down && state.pen.is_editing() { state.pen.drag_to(uv, separate); }
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
    changed |= ui.add_enabled(path.anchors.len() > 1, egui::Checkbox::new(&mut path.closed, "Closed path")).changed();
    let mut width = path.radius * 200.0;
    changed |= crate::widgets::slider_row(ui, "Width (% UV)", egui::Slider::new(&mut width, 0.01..=100.0).logarithmic(true)).changed();
    path.radius = width / 200.0;
    changed |= crate::widgets::slider_row(ui, "Hardness", egui::Slider::new(&mut path.hardness, 0.0..=1.0)).changed();
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
