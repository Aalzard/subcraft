//! 3D viewport: renders the session, handles navigation, painting and the brush cursor.

use egui::{Color32, PointerButton, Pos2, Rect, Sense, Stroke, Ui};
use glam::Vec2;
use subcraft_paint::StrokeMode;
use subcraft_render::wgpu_api as wgpu;

use crate::state::{Tool, UiState};
use crate::theme::ACCENT;

pub fn show(state: &mut UiState, ui: &mut Ui) {
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let ppp = ui.ctx().pixels_per_point();
    let size = [(rect.width() * ppp).round().max(1.0) as u32, (rect.height() * ppp).round().max(1.0) as u32];
    if size[0] < 8 || size[1] < 8 {
        return;
    }
    let now = ui.input(|i| i.time);
    let to_screen = |p: Pos2| Vec2::new((p.x - rect.min.x) * ppp, (p.y - rect.min.y) * ppp);
    let (mods, scroll, delta, primary_pressed, primary_down, pointer) = ui.input(|i| {
        (i.modifiers, i.smooth_scroll_delta.y, i.pointer.delta(), i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.latest_pos())
    });

    // ----- navigation (Painter-style: Alt+LMB orbit, Alt+MMB / MMB pan, Alt+RMB zoom; RMB orbit) --
    if response.hovered() && scroll != 0.0 {
        state.camera.zoom((-scroll * 0.0015).exp());
    }
    let alt = mods.alt;
    if response.dragged_by(PointerButton::Secondary) && alt {
        state.camera.zoom((delta.y * 0.01).exp());
    } else if response.dragged_by(PointerButton::Secondary) || (response.dragged_by(PointerButton::Primary) && alt) {
        state.camera.orbit(delta.x, delta.y);
    }
    if response.dragged_by(PointerButton::Middle) {
        state.camera.pan(delta.x * ppp, delta.y * ppp, size[1] as f32);
    }

    let camera = state.camera.paint_camera(size);

    // ----- painting ----------------------------------------------------------------------------
    let paint_tool = matches!(state.tool, Tool::Brush | Tool::Eraser);
    if primary_pressed && response.hovered() && !alt {
        if let Some(p) = pointer {
            let sp = to_screen(p);
            match state.tool {
                Tool::PolygonFill => {
                    state.session.brush.mode = StrokeMode::Paint;
                    if let Err(e) = state.session.polygon_fill(&camera, sp, state.fill_mode) {
                        state.set_status(format!("Polygon fill: {e}"), now);
                    }
                }
                _ => {
                    state.session.brush.mode = if state.tool == Tool::Eraser { StrokeMode::Erase } else { StrokeMode::Paint };
                    match state.session.begin_stroke() {
                        Ok(()) => {
                            state.painting = true;
                            let line_from = if mods.shift { state.last_stroke_point } else { None };
                            if let Some(from) = line_from {
                                let _ = state.session.stroke_to(&camera, from, true);
                                let _ = state.session.stroke_to(&camera, sp, true);
                            } else {
                                let _ = state.session.stroke_to(&camera, sp, false);
                            }
                            state.last_stroke_point = Some(sp);
                        }
                        Err(e) => state.set_status(format!("Can't paint here: {e} (select a paint layer)"), now),
                    }
                }
            }
        }
    } else if state.painting && primary_down {
        if let Some(p) = pointer
            && delta != egui::Vec2::ZERO
        {
            let sp = to_screen(p);
            let _ = state.session.stroke_to(&camera, sp, false);
            state.last_stroke_point = Some(sp);
        }
    } else if state.painting && !primary_down {
        state.session.end_stroke();
        state.painting = false;
    }

    // ----- render --------------------------------------------------------------------------------
    state.sync_gpu();
    let painter = ui.painter_at(rect);
    match (&state.render_state, &mut state.renderer) {
        (Some(rs), Some(renderer)) => {
            let t0 = std::time::Instant::now();
            let mut params = state.camera.view_params(size);
            params.height_scale = state.height_scale;
            let target = renderer.render(&rs.device, &rs.queue, size, &params);
            let mut egui_renderer = rs.renderer.write();
            let id = match state.viewport_texture {
                Some(id) => {
                    egui_renderer.update_egui_texture_from_wgpu_texture(&rs.device, &target.display_view, wgpu::FilterMode::Linear, id);
                    id
                }
                None => {
                    let id = egui_renderer.register_native_texture(&rs.device, &target.display_view, wgpu::FilterMode::Linear);
                    state.viewport_texture = Some(id);
                    id
                }
            };
            state.frame_time_ms = t0.elapsed().as_secs_f32() * 1000.0;
            painter.image(id, rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        _ => {
            painter.rect_filled(rect, 0.0, Color32::from_rgb(20, 20, 22));
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, "No GPU renderer available", egui::FontId::proportional(16.0), Color32::GRAY);
        }
    }

    // ----- overlays ------------------------------------------------------------------------------
    if let Some(p) = response.hover_pos() {
        if paint_tool && !alt {
            if let Some(pick) = state.session.pick(&camera, to_screen(p)) {
                let r_px = state.session.brush_radius() * camera.pixels_per_unit(pick.depth) / ppp;
                let color = if state.tool == Tool::Eraser { Color32::from_rgb(120, 190, 255) } else { ACCENT };
                painter.circle_stroke(p, r_px, Stroke::new(1.5, Color32::from_black_alpha(160)));
                painter.circle_stroke(p, r_px, Stroke::new(1.0, color));
            }
        } else if state.tool == Tool::PolygonFill {
            painter.circle_filled(p, 3.0, ACCENT);
        }
        ui.ctx().set_cursor_icon(if alt { egui::CursorIcon::Move } else { egui::CursorIcon::Crosshair });
    }
    let hint = "LMB paint · Shift+LMB line · Alt+LMB / RMB orbit · MMB pan · wheel zoom · [ ] size";
    painter.text(rect.left_top() + egui::vec2(8.0, 6.0), egui::Align2::LEFT_TOP, hint, egui::FontId::proportional(12.0), Color32::from_gray(150));
    if state.session.brush.options.symmetry.iter().any(|&s| s) {
        let axes: String = ["X", "Y", "Z"].iter().zip(state.session.brush.options.symmetry).filter(|(_, on)| *on).map(|(a, _)| *a).collect();
        painter.text(rect.right_top() + egui::vec2(-8.0, 6.0), egui::Align2::RIGHT_TOP, format!("Symmetry {axes}"), egui::FontId::proportional(12.0), ACCENT);
    }
    if state.painting || response.dragged() {
        ui.ctx().request_repaint();
    }
}
