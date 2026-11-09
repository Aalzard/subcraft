//! Frameless main window: SubCraft draws its own title bar (the header) like Substance Painter.
//!
//! The OS title bar is turned off in `lib.rs`; this module gives back what it provided:
//! - minimize, maximize / restore and close buttons at the right end of the header,
//! - dragging the window by any empty part of the header (the OS drag, so Windows snap and
//!   macOS / Linux window-manager moves keep working) and double-click to maximize / restore,
//! - resizing from the window edges and corners.
//!
//! `SUBCRAFT_NATIVE_TITLE_BAR=1` brings the OS frame back (for window managers that misbehave
//! with undecorated windows); the custom buttons then hide.

use egui::{Color32, CursorIcon, Id, Pos2, Rect, ResizeDirection, Sense, Stroke, Ui, ViewportCommand, vec2};

use crate::theme;

/// Whether the main window runs without the OS title bar (the default).
pub fn frameless() -> bool {
    !std::env::var("SUBCRAFT_NATIVE_TITLE_BAR").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

/// Width of one caption button (Windows uses 46 px).
pub const BUTTON_WIDTH: f32 = 46.0;
/// Thickness of the invisible resize band along the window edges.
const EDGE: f32 = 5.0;
/// Corner hit size (larger than the edge so corners are easy to grab).
const CORNER: f32 = 12.0;

fn maximized(ui: &Ui) -> bool {
    ui.input(|i| i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Caption {
    Minimize,
    Maximize,
    Restore,
    Close,
}

impl Caption {
    pub fn tooltip(self) -> &'static str {
        match self {
            Caption::Minimize => "Minimize",
            Caption::Maximize => "Maximize",
            Caption::Restore => "Restore down",
            Caption::Close => "Close",
        }
    }
}

/// The three caption buttons, drawn right to left (close first) into a right-to-left layout that
/// fills the header's height. Clicking them sends the matching viewport command.
pub fn caption_buttons(ui: &mut Ui) {
    let max = maximized(ui);
    let height = ui.available_height();
    for kind in [Caption::Close, if max { Caption::Restore } else { Caption::Maximize }, Caption::Minimize] {
        let (rect, response) = ui.allocate_exact_size(vec2(BUTTON_WIDTH, height), Sense::click());
        let response = response.on_hover_text(kind.tooltip());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, kind.tooltip()));
        paint_caption(ui, rect, kind, response.hovered(), response.is_pointer_button_down_on());
        if response.clicked() {
            ui.ctx().send_viewport_cmd(match kind {
                Caption::Minimize => ViewportCommand::Minimized(true),
                Caption::Maximize => ViewportCommand::Maximized(true),
                Caption::Restore => ViewportCommand::Maximized(false),
                Caption::Close => ViewportCommand::Close,
            });
        }
    }
}

fn paint_caption(ui: &Ui, rect: Rect, kind: Caption, hovered: bool, pressed: bool) {
    let painter = ui.painter();
    let (bg, fg) = match (kind, hovered, pressed) {
        (Caption::Close, true, false) => (Color32::from_rgb(196, 43, 28), Color32::WHITE),
        (Caption::Close, true, true) => (Color32::from_rgb(160, 36, 24), Color32::WHITE),
        (_, true, false) => (theme::BG_4, theme::TEXT),
        (_, true, true) => (theme::BG_3, theme::TEXT),
        _ => (Color32::TRANSPARENT, theme::TEXT_DIM),
    };
    painter.rect_filled(rect, 0.0, bg);
    let c = rect.center();
    let s = 5.0; // half size of the glyph
    let stroke = Stroke::new(1.0, fg);
    let ppp = ui.pixels_per_point();
    let px = |p: Pos2| egui::emath::GuiRounding::round_to_pixel_center(p, ppp);
    match kind {
        Caption::Minimize => {
            painter.line_segment([px(c + vec2(-s, 0.0)), px(c + vec2(s, 0.0))], stroke);
        }
        Caption::Maximize => {
            painter.rect_stroke(Rect::from_center_size(px(c), vec2(2.0 * s, 2.0 * s)), 0.0, stroke, egui::StrokeKind::Middle);
        }
        Caption::Restore => {
            // Two overlapping windows: the back one shows only its top-right corner.
            let front = Rect::from_min_size(px(c + vec2(-s, -s + 2.0)), vec2(2.0 * s - 2.0, 2.0 * s - 2.0));
            painter.rect_stroke(front, 0.0, stroke, egui::StrokeKind::Middle);
            let (a, b, d) = (px(c + vec2(-s + 2.0, -s)), px(c + vec2(s, -s)), px(c + vec2(s, s - 2.0)));
            painter.line_segment([a, b], stroke);
            painter.line_segment([b, d], stroke);
        }
        Caption::Close => {
            painter.line_segment([c + vec2(-s, -s), c + vec2(s, s)], stroke);
            painter.line_segment([c + vec2(-s, s), c + vec2(s, -s)], stroke);
        }
    }
}

/// Make the empty parts of the header move the window. Call it *before* drawing the header's
/// widgets: later widgets sit on top and keep their own clicks.
pub fn title_bar_background(ui: &mut Ui, rect: Rect) {
    let response = ui.interact(rect, Id::new("subcraft-title-bar"), Sense::click_and_drag());
    if response.double_clicked() {
        let max = maximized(ui);
        ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!max));
    } else if response.drag_started_by(egui::PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
}

/// Which resize direction a pointer at `p` means in a window of `screen` (None away from the edges).
pub fn edge_at(screen: Rect, p: Pos2) -> Option<ResizeDirection> {
    if !screen.expand(0.5).contains(p) {
        return None;
    }
    let (l, r, t, b) = (p.x - screen.left(), screen.right() - p.x, p.y - screen.top(), screen.bottom() - p.y);
    let (left, right, top, bottom) = (l < EDGE, r < EDGE, t < EDGE, b < EDGE);
    let (near_l, near_r, near_t, near_b) = (l < CORNER, r < CORNER, t < CORNER, b < CORNER);
    use ResizeDirection::*;
    Some(match () {
        _ if (top && near_l) || (left && near_t) => NorthWest,
        _ if (top && near_r) || (right && near_t) => NorthEast,
        _ if (bottom && near_l) || (left && near_b) => SouthWest,
        _ if (bottom && near_r) || (right && near_b) => SouthEast,
        _ if top => North,
        _ if bottom => South,
        _ if left => West,
        _ if right => East,
        _ => return None,
    })
}

fn cursor_for(d: ResizeDirection) -> CursorIcon {
    match d {
        ResizeDirection::North => CursorIcon::ResizeNorth,
        ResizeDirection::South => CursorIcon::ResizeSouth,
        ResizeDirection::East => CursorIcon::ResizeEast,
        ResizeDirection::West => CursorIcon::ResizeWest,
        ResizeDirection::NorthEast => CursorIcon::ResizeNorthEast,
        ResizeDirection::NorthWest => CursorIcon::ResizeNorthWest,
        ResizeDirection::SouthEast => CursorIcon::ResizeSouthEast,
        ResizeDirection::SouthWest => CursorIcon::ResizeSouthWest,
    }
}

/// Resize handles along the window edges, above every panel and dialog. Call once per frame,
/// after the rest of the UI. Does nothing while maximized or full screen.
pub fn resize_edges(ctx: &egui::Context) {
    let maxed = ctx.input(|i| i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false));
    if maxed {
        return;
    }
    let screen = ctx.content_rect();
    let Some(pos) = ctx.input(|i| i.pointer.latest_pos()) else { return };
    let Some(dir) = edge_at(screen, pos) else { return };
    // A dragged widget (slider, dock split) keeps the pointer even when it crosses an edge.
    if ctx.dragged_id().is_some() {
        return;
    }
    ctx.set_cursor_icon(cursor_for(dir));
    if ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    #[test]
    fn edges_and_corners_map_to_resize_directions() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 600.0));
        assert_eq!(edge_at(screen, pos2(500.0, 300.0)), None, "middle of the window");
        assert_eq!(edge_at(screen, pos2(500.0, 1.0)), Some(ResizeDirection::North));
        assert_eq!(edge_at(screen, pos2(500.0, 598.0)), Some(ResizeDirection::South));
        assert_eq!(edge_at(screen, pos2(2.0, 300.0)), Some(ResizeDirection::West));
        assert_eq!(edge_at(screen, pos2(998.0, 300.0)), Some(ResizeDirection::East));
        assert_eq!(edge_at(screen, pos2(1.0, 1.0)), Some(ResizeDirection::NorthWest));
        assert_eq!(edge_at(screen, pos2(8.0, 2.0)), Some(ResizeDirection::NorthWest), "corners are wider than edges");
        assert_eq!(edge_at(screen, pos2(998.0, 595.0)), Some(ResizeDirection::SouthEast));
        assert_eq!(edge_at(screen, pos2(997.0, 3.0)), Some(ResizeDirection::NorthEast));
        assert_eq!(edge_at(screen, pos2(3.0, 590.0)), Some(ResizeDirection::SouthWest));
        assert_eq!(edge_at(screen, pos2(20.0, 20.0)), None, "inside, near a corner but past the band");
        assert_eq!(edge_at(screen, pos2(-20.0, 300.0)), None, "outside the window");
    }

    #[test]
    fn caption_buttons_name_their_action() {
        assert_eq!(Caption::Close.tooltip(), "Close");
        assert_eq!(Caption::Restore.tooltip(), "Restore down");
    }

    #[test]
    fn frameless_unless_the_native_bar_is_requested() {
        // The env var is read live; this only checks the default (tests do not set it).
        if std::env::var("SUBCRAFT_NATIVE_TITLE_BAR").is_err() {
            assert!(frameless());
        }
    }
}
