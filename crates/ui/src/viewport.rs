//! 3D viewport: renders the session, handles navigation, painting and the brush cursor.

use egui::{Color32, PointerButton, Pos2, Rect, Sense, Stroke, Ui};
use glam::Vec2;
use subcraft_paint::StrokeMode;
use subcraft_render::wgpu_api as wgpu;

use crate::state::{Tool, UiState};
use crate::theme::{self, ACCENT};
use crate::icons;

/// Text on a translucent dark plate, anchored at `pos`; returns the plate rect.
fn pill(painter: &egui::Painter, pos: Pos2, align: egui::Align2, text: &str, color: Color32) -> Rect {
    let galley = painter.layout_no_wrap(text.to_string(), theme::font(11.5), color);
    let size = galley.size() + egui::vec2(16.0, 10.0);
    let rect = align.anchor_size(pos, size);
    painter.rect_filled(rect, 0.0, Color32::from_black_alpha(150));
    painter.galley(rect.min + egui::vec2(8.0, 5.0), galley, color);
    rect
}

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

    // ----- stencil: hold S + drag (LMB rotate, MMB move, RMB scale); hold N to bypass ------------
    let (s_down, n_down) = ui.input(|i| (i.key_down(egui::Key::S) && !i.modifiers.any(), i.key_down(egui::Key::N) && !i.modifiers.any()));
    let typing = ui.ctx().egui_wants_keyboard_input();
    state.session.brush.stencil_bypass = n_down && !typing;
    let stencil_edit = s_down && !typing && state.session.brush.stencil.is_some();
    if stencil_edit && response.hovered() {
        let mut st = (**state.session.brush.stencil.as_ref().expect("checked above")).clone();
        let mut changed = false;
        if response.dragged_by(PointerButton::Primary) {
            st.rotation += delta.x * 0.01;
            changed = true;
        }
        if response.dragged_by(PointerButton::Middle) {
            st.center += Vec2::new(delta.x / rect.width(), delta.y / rect.height());
            changed = true;
        }
        if response.dragged_by(PointerButton::Secondary) {
            st.scale = (st.scale * (-delta.y * 0.01).exp()).clamp(0.02, 8.0);
            changed = true;
        }
        if changed {
            state.session.brush.stencil = Some(std::sync::Arc::new(st));
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::AllScroll);
    }

    // ----- navigation (Painter-style: Alt+LMB orbit, Alt+MMB / MMB pan, Alt+RMB zoom; RMB orbit) --
    if response.hovered() && scroll != 0.0 {
        state.camera.zoom((-scroll * 0.0015).exp());
    }
    let alt = mods.alt;
    if stencil_edit {
        // Drags belong to the stencil.
    } else if response.dragged_by(PointerButton::Secondary) && alt {
        state.camera.zoom((delta.y * 0.01).exp());
    } else if response.dragged_by(PointerButton::Secondary) || (response.dragged_by(PointerButton::Primary) && alt) {
        state.camera.orbit(delta.x, delta.y);
    }
    if response.dragged_by(PointerButton::Middle) && !stencil_edit {
        state.camera.pan(delta.x * ppp, delta.y * ppp, size[1] as f32);
    }

    let camera = state.camera.paint_camera(size);

    // ----- drops from the shelf: apply to the texture set under the cursor --------------------
    if let Some(item) = response.dnd_release_payload::<crate::panels::shelf::ShelfDrag>() {
        if let Some(p) = pointer
            && let Some(pick) = state.session.pick(&camera, to_screen(p))
            && let Some(ts) = state.session.document().texture_set_for_slot(pick.material_slot).map(|t| t.id)
        {
            let _ = state.session.execute(subcraft_app::Command::SelectTextureSet(ts));
        }
        crate::panels::shelf::apply_shelf_item(state, *item, now);
    }
    let drop_hover = response.dnd_hover_payload::<crate::panels::shelf::ShelfDrag>().is_some();

    // ----- painting ----------------------------------------------------------------------------
    let paint_tool = matches!(state.tool, Tool::Brush | Tool::Eraser);
    if primary_pressed && response.hovered() && !alt && !stencil_edit {
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
            crate::panels::shelf::apply_environment(
                &state.library,
                state.env.index,
                state.env.rotation_deg,
                state.env.intensity,
                &mut params,
                renderer,
                &mut state.env.uploaded,
                &rs.device,
                &rs.queue,
            );
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
            painter.rect_filled(rect, 0.0, theme::BG_0);
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, "No GPU renderer available", theme::font(15.0), theme::TEXT_FAINT);
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
    draw_stencil(state, ui, &painter, rect, size);
    if drop_hover {
        painter.rect_stroke(rect.shrink(1.0), 0.0, Stroke::new(2.0, ACCENT), egui::StrokeKind::Inside);
    }
    let hint = "LMB paint  ·  Shift+LMB line  ·  Alt+LMB / RMB orbit  ·  MMB pan  ·  Wheel zoom  ·  [ ] size";
    pill(&painter, rect.left_bottom() + egui::vec2(10.0, -10.0), egui::Align2::LEFT_BOTTOM, hint, theme::TEXT_FAINT);
    let mut right = rect.right_top() + egui::vec2(-10.0, 10.0);
    if state.session.brush.options.symmetry.iter().any(|&s| s) {
        let axes: String = ["X", "Y", "Z"].iter().zip(state.session.brush.options.symmetry).filter(|(_, on)| *on).map(|(a, _)| *a).collect();
        let r = pill(&painter, right, egui::Align2::RIGHT_TOP, &format!("{}  Symmetry {axes}", icons::SYMMETRY), ACCENT);
        right.x = r.left() - 6.0;
    }
    if state.session.brush.target == subcraft_paint::PaintTarget::MaskBase {
        pill(&painter, right, egui::Align2::RIGHT_TOP, &format!("{}  Painting mask", icons::MASK), ACCENT);
    }
    if state.painting || response.dragged() {
        ui.ctx().request_repaint();
    }
}

/// Semi-transparent stencil image over the 3D view (hidden while N bypasses it).
fn draw_stencil(state: &mut UiState, ui: &Ui, painter: &egui::Painter, rect: Rect, size: [u32; 2]) {
    let Some(st) = state.session.brush.stencil.clone() else {
        state.stencil_tex = None;
        return;
    };
    if state.session.brush.stencil_bypass {
        return;
    }
    let key = std::sync::Arc::as_ptr(&st.bitmap) as usize;
    if state.stencil_tex.as_ref().is_none_or(|(k, _)| *k != key) {
        let (w, h, rgba) = st.bitmap.preview_rgba8(512);
        let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
        let tex = ui.ctx().load_texture("stencil", img, egui::TextureOptions::LINEAR);
        state.stencil_tex = Some((key, tex));
    }
    let Some((_, tex)) = &state.stencil_tex else { return };
    let ppp = ui.ctx().pixels_per_point();
    let to_ui = |v: glam::Vec2| egui::pos2(rect.min.x + v.x / ppp, rect.min.y + v.y / ppp);
    let c = st.corners(size);
    let tint = Color32::from_white_alpha(110);
    let mut mesh = egui::Mesh::with_texture(tex.id());
    for (p, uv) in c.iter().zip([egui::pos2(0.0, 0.0), egui::pos2(1.0, 0.0), egui::pos2(1.0, 1.0), egui::pos2(0.0, 1.0)]) {
        mesh.vertices.push(egui::epaint::Vertex { pos: to_ui(*p), uv, color: tint });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    painter.with_clip_rect(rect).add(egui::Shape::mesh(mesh));
    let outline: Vec<egui::Pos2> = c.iter().map(|p| to_ui(*p)).collect();
    painter.with_clip_rect(rect).add(egui::Shape::closed_line(outline, Stroke::new(1.0, Color32::from_white_alpha(90))));
}
