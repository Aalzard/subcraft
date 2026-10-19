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
    // Until the compositor reports a window size, egui lays out in its 10000x10000-point fallback
    // canvas. A render target built from that would dwarf the real viewport (10k+ wide MSAA
    // textures) or outright exceed the device texture limit; wait for a real screen rect.
    if ui.ctx().input(|i| i.raw.screen_rect.is_none()) {
        return;
    }
    crate::viewport_bar::show(ui, state);
    state.camera.orthographic = state.recovery.prefs.viewport.orthographic;
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let ppp = ui.ctx().pixels_per_point();
    let size = [(rect.width() * ppp).round().max(1.0) as u32, (rect.height() * ppp).round().max(1.0) as u32];
    if size[0] < 8 || size[1] < 8 {
        return;
    }
    let now = ui.input(|i| i.time);
    // The render target is rounded to whole physical pixels. Map through that exact size,
    // rather than DPI alone, to keep picking/painting stable at fractional UI zoom factors.
    let pixel_scale = Vec2::new(size[0] as f32 / rect.width(), size[1] as f32 / rect.height());
    let to_screen = |p: Pos2| Vec2::new((p.x - rect.min.x) * pixel_scale.x, (p.y - rect.min.y) * pixel_scale.y);
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
        state.camera.pan(delta.x * pixel_scale.x, delta.y * pixel_scale.y, size[1] as f32);
    }

    let camera = state.camera.paint_camera(size);

    if response.secondary_clicked() && !alt && !stencil_edit {
        state.context_part = pointer.and_then(|p| state.session.surface_target(&camera, to_screen(p), state.parts_kind));
    }
    let mut part_action = None;
    response.context_menu(|ui| {
        if let Some(target) = state.context_part {
            ui.label(target.part.kind.label());
            ui.menu_button("Target", |ui| {
                for kind in subcraft_core::PartKind::ALL {
                    if ui.selectable_label(state.parts_kind == kind, kind.label()).clicked()
                        && let Some(next) = state.session.target_kind(target, kind) {
                        state.context_part = Some(next); state.parts_kind = kind;
                    }
                }
            });
            if let Some(action) = crate::panels::parts::menu(ui, state.context_part.unwrap_or(target), state.session.selection().layer.is_some() && state.session.selection().texture_set == target.texture_set) {
                part_action = Some((state.context_part.unwrap_or(target), action));
            }
        } else { ui.label("Right-click the model to texture a part."); }
        if state.session.has_hidden_parts() && ui.button("Show all parts").clicked() { state.session.show_all_parts(); ui.close(); }
        if ui.button("Eyedropper tool").clicked() { state.tool = Tool::Eyedropper; ui.close(); }
    });
    if let Some((target, action)) = part_action { crate::panels::parts::apply(state, target, action, now); }
    if response.hovered() && !typing && !mods.any() {
        let target = pointer.and_then(|p| state.session.surface_target(&camera, to_screen(p), state.parts_kind));
        for (key, action) in [(egui::Key::I, subcraft_app::PartAction::Isolate), (egui::Key::H, subcraft_app::PartAction::Hide)] {
            if ui.input(|i| i.key_pressed(key)) && let Some(target) = target { crate::panels::parts::apply(state, target, action, now); }
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) { state.session.show_all_parts(); }
    }

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

    // ----- part picking: clicks show / hide the selected layer on mesh parts ----------------------
    let picking = state.picking_parts && state.session.selected_layer().is_some() && !state.painting;
    if state.picking_parts && (!picking || ui.input(|i| i.key_pressed(egui::Key::Escape))) {
        state.picking_parts = false;
    }
    let hovered_part = if picking { pointer.and_then(|p| state.session.part_at(&camera, to_screen(p))) } else { None };
    if picking && primary_pressed && response.hovered() && !alt && !stencil_edit {
        let layer = state.session.selected_layer().map(|l| l.id);
        match (layer, hovered_part) {
            (Some(layer), Some(part)) => {
                if let Err(e) = state.session.toggle_layer_part(layer, part) {
                    state.set_status(format!("Parts: {e}"), now);
                }
            }
            _ => state.set_status("Click a part of the selected texture set", now),
        }
    }

    // ----- painting ----------------------------------------------------------------------------
    if state.tool == Tool::Pen && !picking && !alt && !stencil_edit && !typing && let Some(p) = pointer {
            if primary_pressed && response.hovered() {
                let hit = state.pen.preview(&state.session).and_then(|path| crate::pen::hit(path, state.pen.selected_anchor(), p, |uv| pen_position(state, &camera, rect, uv)));
                if let Err(e) = state.pen.begin_surface(&mut state.session, &camera, to_screen(p), hit) { state.set_status(format!("Pen: {e}"), now); }
            } else if primary_down && state.pen.is_editing() {
                state.pen.drag_surface(&state.session, &camera, to_screen(p), mods.shift);
            }
    }
    let paint_tool = matches!(state.tool, Tool::Brush | Tool::Eraser) && !picking;
    if primary_pressed && response.hovered() && !alt && !stencil_edit && !picking && state.tool != Tool::Pen {
        if let Some(p) = pointer {
            let sp = to_screen(p);
            match state.tool {
                Tool::Eyedropper => {
                    if state.session.sample_surface(&camera, sp, mods.shift) {
                        state.gpu_dirty = true;
                        state.set_status(if mods.shift { "Picked material" } else { "Picked color" }, now);
                    }
                }
                Tool::PolygonFill => {
                    state.session.brush.mode = StrokeMode::Paint;
                    if let Err(e) = state.session.polygon_fill(&camera, sp, state.fill_mode) {
                        state.set_status(format!("Polygon fill: {e}"), now);
                    }
                }
                _ => {
                    if let Err(e) = state.session.prepare_surface_paint(&camera, sp) { state.set_status(format!("Parts: {e}"), now); }
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
            let t0 = web_time::Instant::now();
            let mut params = state.camera.view_params(size);
            params.height_scale = state.height_scale;
            let view = &state.recovery.prefs.viewport;
            params.antialias = view.antialias;
            params.view_mode = view.view_mode();
            params.grid = view.grid.then_some(subcraft_render::GridParams {
                bounds: state.session.scene().mesh.bounds,
                axes: view.grid_axes,
            });
            let render_size = view.render_size(size);
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
            let target = renderer.render(&rs.device, &rs.queue, render_size, &params);
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
    if state.tool == Tool::Pen && let Some(path) = state.pen.preview(&state.session) {
        crate::pen::draw(&painter, path, state.pen.selected_anchor(), |uv| pen_position(state, &camera, rect, uv));
    }
    if let Some(p) = response.hover_pos() {
        if paint_tool && !alt {
            if let Some(pick) = state.session.pick(&camera, to_screen(p)) {
                let r_px = state.session.brush_radius() * camera.pixels_per_unit(pick.depth) / pixel_scale.y;
                let color = if state.tool == Tool::Eraser { Color32::from_rgb(120, 190, 255) } else { ACCENT };
                painter.circle_stroke(p, r_px, Stroke::new(1.5, Color32::from_black_alpha(160)));
                painter.circle_stroke(p, r_px, Stroke::new(1.0, color));
            }
        } else if state.tool == Tool::PolygonFill && !picking {
            painter.circle_filled(p, 3.0, ACCENT);
        }
        ui.ctx().set_cursor_icon(if alt { egui::CursorIcon::Move } else { egui::CursorIcon::Crosshair });
    }
    draw_stencil(state, ui, &painter, rect, size);
    if drop_hover {
        painter.rect_stroke(rect.shrink(1.0), 0.0, Stroke::new(2.0, ACCENT), egui::StrokeKind::Inside);
    }
    let hint = if state.tool == Tool::Pen { "Click points · drag for curves · edit in 2D View · Enter finish · Esc cancel" }
        else { "LMB paint · RMB menu / drag orbit · I isolate · H hide · Esc show all · Wheel zoom" };
    pill(&painter, rect.left_bottom() + egui::vec2(10.0, -10.0), egui::Align2::LEFT_BOTTOM, hint, theme::TEXT_FAINT);
    let mut right = rect.right_top() + egui::vec2(-10.0, 10.0);
    if state.tool != Tool::Pen && state.session.brush.options.symmetry.iter().any(|&s| s) {
        let axes: String = ["X", "Y", "Z"].iter().zip(state.session.brush.options.symmetry).filter(|(_, on)| *on).map(|(a, _)| *a).collect();
        let r = pill(&painter, right, egui::Align2::RIGHT_TOP, &format!("{}  Symmetry {axes}", icons::SYMMETRY), ACCENT);
        right.x = r.left() - 6.0;
    }
    if picking {
        let shown = state.session.selected_layer().and_then(|l| l.parts.as_ref()).filter(|p| p.enabled).map(|p| p.parts.contains(&hovered_part.unwrap_or(u32::MAX)));
        let what = match (hovered_part, shown) {
            (Some(_), Some(true)) => "click to hide the layer on this part",
            (Some(_), _) => "click to show the layer on this part",
            (None, _) => "click parts to show / hide the layer",
        };
        let r = pill(&painter, right, egui::Align2::RIGHT_TOP, &format!("{}  Picking parts: {what}  ·  Esc done", icons::CURSOR), ACCENT);
        right.x = r.left() - 6.0;
    }
    match state.session.brush.target {
        subcraft_paint::PaintTarget::MaskBase => { pill(&painter, right, egui::Align2::RIGHT_TOP, &format!("{}  Painting mask", icons::MASK), ACCENT); }
        subcraft_paint::PaintTarget::MaskEffect(_) => { pill(&painter, right, egui::Align2::RIGHT_TOP, &format!("{}  Painting mask Paint effect", icons::MASK), ACCENT); }
        subcraft_paint::PaintTarget::ContentEffect(_) => { pill(&painter, right, egui::Align2::RIGHT_TOP, &format!("{}  Painting Paint effect", icons::PAINT_BRUSH), ACCENT); }
        subcraft_paint::PaintTarget::Content => {}
    }
    if state.painting || response.dragged() {
        ui.ctx().request_repaint();
    }
}

/// Project UV controls only where their sampled surface is visible.
fn pen_position(state: &UiState, camera: &subcraft_app::Camera, rect: Rect, uv: Vec2) -> Option<Pos2> {
    let position = state.session.pen_uv_position(uv)?;
    let screen = camera.project(position)?.truncate();
    let pick = state.session.pick(camera, screen)?;
    // Keep overlays off occluded/hidden surfaces. UV sampling is quantized to texel centers.
    let tolerance = state.session.scene().mesh.bounds.diagonal() * 0.03;
    if pick.material_slot != state.session.selected_texture_set().material_slot || pick.position.distance(position) > tolerance { return None; }
    Some(rect.min + egui::vec2(screen.x * rect.width() / camera.viewport[0] as f32, screen.y * rect.height() / camera.viewport[1] as f32))
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
    let to_ui = |v: glam::Vec2| egui::pos2(rect.min.x + v.x * rect.width() / size[0] as f32, rect.min.y + v.y * rect.height() / size[1] as f32);
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
