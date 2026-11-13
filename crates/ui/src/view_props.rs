//! Right-click in the 3D or 2D view: the Properties panel's current content in a compact popup at
//! the cursor (brush settings while painting, fill / generator / effect settings for the selected
//! layer or effect), with the 3D view's part actions in its header. A right-button *drag* still
//! navigates (Painter scheme orbit, Alt+RMB zoom, Shift+RMB environment, Ctrl+RMB brush size):
//! only a click without movement opens it. It closes on Esc, its close button, or a click outside.
//!
//! Also the polygon fill's shared click-or-box helpers.

use egui::{Color32, Pos2, Rect, RichText, Stroke};

use crate::state::{Tool, UiState};
use crate::theme;

/// Popup size in points (shrunk to fit small windows).
pub const WIDTH: f32 = 320.0;
pub const HEIGHT: f32 = 460.0;
/// Pointer travel (points) beyond which a polygon fill press is a box, not a click.
pub const BOX_DRAG: f32 = 5.0;

/// Which view opened the popup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    View3d,
    View2d,
}

#[derive(Clone, Copy, Debug)]
pub struct ViewProps {
    /// Where the right-click happened (screen points).
    pub pos: Pos2,
    pub view: View,
}

/// Whether a polygon fill press at `a` released at `b` is a box selection.
pub fn is_box_drag(a: Pos2, b: Pos2) -> bool {
    (b - a).abs().max_elem() > BOX_DRAG
}

/// The polygon fill box: translucent accent fill, dashed outline (solid when it selects x-ray).
pub fn draw_fill_box(painter: &egui::Painter, r: Rect, visible_only: bool) {
    painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(theme::ACCENT().r(), theme::ACCENT().g(), theme::ACCENT().b(), 28));
    let stroke = Stroke::new(1.0, theme::ACCENT());
    if visible_only {
        let corners = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
        painter.extend(egui::Shape::dashed_line(&corners, stroke, 5.0, 3.0));
    } else {
        painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
    }
}

/// Whether a right-click in a view opens the popup: not while painting or editing the stencil,
/// not with Alt (Painter zoom) or Ctrl (brush size gesture) held.
pub fn opens(state: &UiState, mods: egui::Modifiers) -> bool {
    !state.painting && !state.uv.painting && !mods.alt && !mods.command && !(state.bake.open || state.bake.job.is_some())
}

/// Open the popup at `pos` for `view`.
pub fn open(state: &mut UiState, pos: Pos2, view: View) {
    state.fill_box = None;
    state.uv.fill_box = None;
    state.view_props = Some(ViewProps { pos, view });
}

/// Where the popup goes for a click at `pos` so it stays inside `screen`.
pub fn placement(pos: Pos2, screen: Rect) -> Rect {
    let size = egui::vec2(WIDTH.min(screen.width() - 16.0).max(160.0), HEIGHT.min(screen.height() - 16.0).max(120.0));
    let mut min = pos + egui::vec2(4.0, 4.0);
    if min.x + size.x > screen.right() - 8.0 { min.x = (pos.x - size.x - 4.0).max(screen.left() + 8.0); }
    if min.y + size.y > screen.bottom() - 8.0 { min.y = (screen.bottom() - size.y - 8.0).max(screen.top() + 8.0); }
    Rect::from_min_size(min, size)
}

/// Draw the popup (call once per frame, before the views, so it can take Esc and the click that
/// closes it). Returns whether this frame's primary press closed it (the views ignore that press).
pub fn show(ctx: &egui::Context, state: &mut UiState) -> bool {
    let Some(props) = state.view_props else { return false };
    let now = ctx.input(|i| i.time);
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        state.view_props = None;
        return false;
    }
    let screen = ctx.content_rect();
    let rect = placement(props.pos, screen);
    // A press outside the popup (and outside its own dropdowns, which live on higher layers) closes it.
    let (pressed, primary, at) = ctx.input(|i| (i.pointer.any_pressed(), i.pointer.primary_pressed(), i.pointer.interact_pos()));
    if pressed && let Some(at) = at
        && !rect.contains(at)
        && ctx.layer_id_at(at).is_none_or(|l| matches!(l.order, egui::Order::Background | egui::Order::Middle))
    {
        state.view_props = None;
        return primary;
    }
    let mut close = false;
    let mut part_action = None;
    egui::Area::new(egui::Id::new("view_props_popup"))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).fill(theme::BG_1()).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                let inner = rect.size() - egui::vec2(12.0, 12.0);
                ui.set_width(inner.x);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(crate::i18n::tr("Properties")).font(theme::font_strong(12.5)).color(theme::TEXT()));
                    if props.view == View::View3d {
                        part_action = part_menu(ui, state);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::widgets::icon_button(ui, crate::icons::X, "Close (Esc)").clicked() {
                            close = true;
                        }
                    });
                });
                ui.separator();
                let body = egui::vec2(inner.x, (inner.y - ui.min_rect().height() - 6.0).max(60.0));
                ui.allocate_ui(body, |ui| {
                    ui.set_min_size(body);
                    ui.set_max_size(body);
                    crate::panels::properties::show(state, ui);
                });
            });
        });
    if let Some((target, action)) = part_action {
        crate::panels::parts::apply(state, target, action, now);
        close = true;
    }
    if close {
        state.view_props = None;
    }
    false
}

/// The 3D view's part actions (as the old right-click menu had them) behind a header button.
fn part_menu(ui: &mut egui::Ui, state: &mut UiState) -> Option<(subcraft_app::SurfaceTarget, subcraft_app::PartAction)> {
    let mut picked = None;
    let label = match state.context_part {
        Some(t) => format!("{}  {}", crate::icons::CUBE, t.part.kind.label()),
        None => format!("{}  Part", crate::icons::CUBE),
    };
    ui.menu_button(label, |ui| {
        if let Some(target) = state.context_part {
            ui.label(target.part.kind.label());
            ui.menu_button("Target", |ui| {
                for kind in subcraft_core::PartKind::ALL {
                    if ui.selectable_label(state.parts_kind == kind, kind.label()).clicked()
                        && let Some(next) = state.session.target_kind(target, kind)
                    {
                        state.context_part = Some(next);
                        state.parts_kind = kind;
                    }
                }
            });
            let current = state.context_part.unwrap_or(target);
            let has_layer = state.session.selection().layer.is_some() && state.session.selection().texture_set == target.texture_set;
            if let Some(action) = crate::panels::parts::menu(ui, current, has_layer) {
                picked = Some((current, action));
            }
        } else {
            ui.label("Right-click the model to texture a part.");
        }
        if state.session.has_hidden_parts() && ui.button("Show all parts").clicked() {
            state.session.show_all_parts();
            ui.close();
        }
        if ui.button("Eyedropper tool").clicked() {
            state.tool = Tool::Eyedropper;
            ui.close();
        }
    });
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_press_is_a_click_and_a_longer_drag_a_box() {
        let a = Pos2::new(100.0, 100.0);
        assert!(!is_box_drag(a, a));
        assert!(!is_box_drag(a, a + egui::vec2(3.0, -4.0)));
        assert!(is_box_drag(a, a + egui::vec2(0.0, 12.0)));
        assert!(is_box_drag(a, a + egui::vec2(-40.0, 2.0)));
    }

    #[test]
    fn the_popup_stays_on_screen_near_the_cursor() {
        let screen = Rect::from_min_size(Pos2::ZERO, egui::vec2(1600.0, 900.0));
        let r = placement(Pos2::new(300.0, 200.0), screen);
        assert_eq!(r.min, Pos2::new(304.0, 204.0), "below right of the cursor");
        assert_eq!(r.size(), egui::vec2(WIDTH, HEIGHT));
        // Near the bottom-right corner it flips left and moves up, staying inside.
        let r = placement(Pos2::new(1590.0, 890.0), screen);
        assert!(screen.contains_rect(r), "{r:?}");
        assert!(r.max.x <= 1590.0);
        // A tiny window still gets a usable popup inside it.
        let small = Rect::from_min_size(Pos2::ZERO, egui::vec2(300.0, 200.0));
        assert!(small.contains_rect(placement(Pos2::new(150.0, 100.0), small)));
    }
}
