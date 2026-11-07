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

/// Physical pixels of a view `points` large at `ppp` pixels per point; never zero.
pub fn physical_size(points: [f32; 2], ppp: f32) -> [u32; 2] {
    points.map(|p| (p * ppp).round().max(1.0) as u32)
}

#[cfg(test)]
mod size_tests {
    use super::physical_size;
    use subcraft_render::fit_target_size;

    /// Issue #74: a 2880x1920 panel at every common scale; the viewport texture must stay
    /// within an 8192 device limit, and nothing changes at 100%.
    #[test]
    fn viewport_target_fits_the_device_limit_at_every_scale() {
        for scale in [1.0f32, 1.25, 1.5, 1.75, 2.0, 3.0] {
            let points = [2880.0 / scale, 1920.0 / scale];
            let physical = physical_size(points, scale);
            assert!(physical[0].abs_diff(2880) <= 1 && physical[1].abs_diff(1920) <= 1, "{scale}: {physical:?}");
            let fitted = fit_target_size(physical, 8192);
            assert_eq!(fitted, physical, "normal sizes are untouched at {scale}");
            // a window restored at physical-size-in-points (the reported 9375 px case)
            let wide = physical_size([6250.0 * scale / 1.5, 1920.0 / scale], scale);
            let fitted = fit_target_size(wide, 8192);
            assert!(fitted.iter().all(|&s| (1..=8192).contains(&s)), "{scale}: {fitted:?}");
        }
        assert_eq!(fit_target_size([9375, 1200], 8192), [8192, 1200]);
        assert_eq!(fit_target_size([8272, 1200], 8192), [8192, 1200]);
        assert_eq!(physical_size([0.0, 0.2], 1.5), [1, 1]);
    }
}

fn primary_or_other_pressed(ui: &Ui) -> bool {
    ui.input(|i| i.pointer.any_pressed())
}

pub fn show(state: &mut UiState, ui: &mut Ui) {
    crate::viewport_bar::show(ui, state);
    state.camera.orthographic = state.recovery.prefs.viewport.orthographic;
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let ppp = ui.ctx().pixels_per_point();
    let size = physical_size([rect.width(), rect.height()], ppp);
    if size[0] < 8 || size[1] < 8 {
        return;
    }
    state.last_view_size = size;
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

    // ----- navigation: scheme from Preferences; mode changes mid-drag take over seamlessly -----
    use crate::nav::{NavDrag, NavInput, NavMode};
    let scheme = state.recovery.prefs.nav_scheme;
    if response.hovered() && scroll != 0.0 {
        state.camera.zoom((-scroll * 0.0015).exp());
    }
    let alt = mods.alt && scheme.alt_navigates();
    let nav_input = ui.input(|i| NavInput {
        primary: i.pointer.primary_down(),
        middle: i.pointer.middle_down(),
        secondary: i.pointer.secondary_down(),
        shift: i.modifiers.shift,
        ctrl: i.modifiers.ctrl || i.modifiers.command,
        alt: i.modifiers.alt,
    });
    if stencil_edit {
        state.nav_drag = None; // Drags belong to the stencil.
    } else if let Some(p) = pointer {
        let pos = Vec2::new(p.x, p.y);
        if state.nav_drag.is_none() && primary_or_other_pressed(ui) && response.hovered() {
            state.nav_drag = NavDrag::begin(scheme, nav_input, pos);
        } else if let Some(drag) = state.nav_drag.as_mut() {
            match drag.step(scheme, nav_input, pos) {
                Some((mode, d)) => {
                    match mode {
                        NavMode::Orbit => state.camera.orbit(d.x, d.y),
                        NavMode::Pan => state.camera.pan(d.x * pixel_scale.x, d.y * pixel_scale.y, size[1] as f32),
                        NavMode::Zoom => state.camera.zoom((d.y * 0.01).exp()),
                        NavMode::None => {}
                    }
                }
                None => state.nav_drag = None,
            }
        }
    }

    // ----- Painter-style brush gestures: Ctrl+RMB drag = size / hardness, Ctrl+LMB drag = flow / angle -----
    let gesture_ok = mods.command && !mods.alt && !stencil_edit && !typing && !(state.bake.open || state.bake.job.is_some());
    let brush_tool = matches!(state.tool, Tool::Brush | Tool::Eraser | Tool::Smudge | Tool::Clone);
    let lmb_adjust = gesture_ok && matches!(state.tool, Tool::Brush | Tool::Eraser | Tool::Smudge);
    if gesture_ok && brush_tool && !state.painting {
        let s = &mut state.session.brush.settings;
        if response.dragged_by(PointerButton::Secondary) {
            s.size = (s.size * (delta.x * 0.01).exp()).clamp(0.1, 100.0);
            s.hardness = (s.hardness - delta.y * 0.005).clamp(0.0, 1.0);
        } else if lmb_adjust && response.dragged_by(PointerButton::Primary) {
            s.flow = (s.flow + delta.x * 0.003).clamp(0.01, 1.0);
            s.angle_deg = (s.angle_deg + delta.y * 0.5).rem_euclid(360.0);
        }
    }
    let v_down = ui.input(|i| i.key_down(egui::Key::V)) && !typing && !mods.any();

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
        // Alt: apply in place to the selected layer (alphas: brush tip unless Alt).
        crate::panels::shelf::apply_shelf_item_with(state, *item, mods.alt, now);
    }
    let drop_hover = response.dnd_hover_payload::<crate::panels::shelf::ShelfDrag>().map(|d| *d);

    // ----- part picking: clicks show / hide the selected layer on mesh parts ----------------------
    let bake_mode = state.bake.open || state.bake.job.is_some();
    let parts_picking = !bake_mode && state.picking_parts && state.session.selected_layer().is_some() && !state.painting;
    // ----- ID Color Mask eyedropper: clicks add the ID map color under the cursor ------------
    let id_picking = !bake_mode && !state.painting
        && state.picking_id_color.is_some_and(|(layer, _)| state.session.selected_texture_set().find(layer).is_some());
    if state.picking_id_color.is_some() && (!id_picking || ui.input(|i| i.key_pressed(egui::Key::Escape))) {
        state.picking_id_color = None;
    }
    if id_picking && primary_pressed && response.hovered() && !alt && !stencil_edit
        && let (Some((layer, effect)), Some(p)) = (state.picking_id_color, pointer)
    {
        match state.session.pick_id_color(layer, effect, &camera, to_screen(p)) {
            Ok(Some(_)) => state.set_status("ID color added. Click more parts, Esc when done.", now),
            Ok(None) if !state.session.has_id_map(state.session.selection().texture_set) => state.set_status("No ID map baked for this texture set: bake one in Bake Mesh Maps.", now),
            Ok(None) => state.set_status("Nothing added: missed the model or that ID color is already picked.", now),
            Err(e) => state.set_status(format!("ID Color Mask: {e}"), now),
        }
    }
    if id_picking && response.hovered() { ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair); }
    // ----- text tool: the next click places the selected text layer or stamps the draft text ------
    let text_armed = !bake_mode && !state.painting && state.text.armed.is_some();
    if text_armed && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.text.armed = None;
    } else if text_armed {
        if response.hovered() { ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair); }
        if primary_pressed && response.hovered() && !alt && let Some(p) = pointer {
            let sp = to_screen(p);
            let result = match state.text.armed {
                Some(crate::text_panel::Armed::PlaceDecal) => state.session.place_selected_text_at(&camera, sp),
                Some(crate::text_panel::Armed::StampProjected) => match state.text.draft.clone() {
                    Some(d) => state.session.stamp_text_projected(&camera, sp, &d.font, &d.style, state.text.stamp_size, state.text.stamp_angle),
                    None => Ok(()),
                },
                None => Ok(()),
            };
            match result {
                Ok(()) => { state.gpu_dirty = true; state.text.armed = None; }
                Err(e) => state.set_status(format!("Text: {e}"), now),
            }
        }
    }
    // ----- depth of field focus picker: the next click on the model sets the focus distance --------
    let focus_picking = !bake_mode && !state.painting && state.picking_focus && state.recovery.prefs.viewport.dof;
    if focus_picking && primary_pressed && response.hovered() && let Some(p) = pointer {
        if let Some(hit) = state.session.pick(&camera, to_screen(p)) {
            let orbit = state.camera.eye_distance().max(1e-4);
            state.recovery.prefs.viewport.dof_focus = (hit.depth / orbit).clamp(0.2, 3.0);
            state.set_status(crate::i18n::tr("Focus set"), now);
        } else {
            state.set_status(crate::i18n::tr("Click the model to set the focus"), now);
        }
        state.picking_focus = false;
    } else if state.picking_focus && (!focus_picking || ui.input(|i| i.key_pressed(egui::Key::Escape))) {
        state.picking_focus = false;
    }
    let gizmo_busy = !bake_mode && !state.painting && !text_armed && text_gizmo(state, ui, &camera, rect, size_scale(rect, &camera), now);
    let picking = parts_picking || id_picking || text_armed || focus_picking || gizmo_busy;
    if state.picking_parts && (!picking || ui.input(|i| i.key_pressed(egui::Key::Escape))) {
        state.picking_parts = false;
    }
    let hovered_part = if parts_picking { pointer.and_then(|p| state.session.part_at(&camera, to_screen(p))) } else { None };
    if parts_picking && primary_pressed && response.hovered() && !alt && !stencil_edit {
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
    state.pen_pressure = pen_pressure(&ui.input(|i| i.events.clone()), state.pen_pressure);
    if !bake_mode && state.tool == Tool::Pen && !picking && !alt && !stencil_edit && !typing && let Some(p) = pointer {
            if primary_pressed && response.hovered() {
                let hit = state.pen.preview(&state.session).and_then(|path| crate::pen::hit(path, state.pen.selected_anchor(), p, |uv| pen_position(state, &camera, rect, uv)));
                if let Err(e) = state.pen.begin_surface(&mut state.session, &camera, to_screen(p), hit) { state.set_status(format!("Pen: {e}"), now); }
            } else if primary_down && state.pen.is_editing() {
                state.pen.drag_surface(&state.session, &camera, to_screen(p), mods.shift);
            }
    }
    let paint_tool = !bake_mode && matches!(state.tool, Tool::Brush | Tool::Eraser | Tool::Clone | Tool::Smudge) && !picking;
    if !bake_mode && primary_pressed && response.hovered() && !alt && !stencil_edit && !picking && state.tool != Tool::Pen && !lmb_adjust {
        if let Some(p) = pointer {
            let sp = to_screen(p);
            match state.tool {
                Tool::Eyedropper => {
                    if state.session.sample_surface(&camera, sp, mods.shift) {
                        state.gpu_dirty = true;
                        state.set_status(if mods.shift { "Picked material" } else { "Picked color" }, now);
                    }
                }
                Tool::Clone if mods.command || v_down => {
                    if state.session.set_clone_source(&camera, sp) { state.set_status("Clone source set: now paint where the copy should go", now); }
                    else { state.set_status(crate::keys::native("Ctrl+click the model to set the clone source"), now); }
                }
                Tool::PolygonFill => {
                    state.session.brush.mode = StrokeMode::Paint;
                    if let Err(e) = state.session.polygon_fill(&camera, sp, state.fill_mode) {
                        state.set_status(format!("Polygon fill: {e}"), now);
                    }
                }
                _ => {
                    if let Err(e) = state.session.prepare_surface_paint(&camera, sp) { state.set_status(format!("Parts: {e}"), now); }
                    state.session.brush.mode = match state.tool {
                        Tool::Eraser => StrokeMode::Erase,
                        Tool::Clone => StrokeMode::Clone,
                        Tool::Smudge => StrokeMode::Smudge,
                        _ => StrokeMode::Paint,
                    };
                    match state.session.begin_stroke() {
                        Ok(()) => {
                            state.painting = true;
                            state.session.set_stroke_pressure(state.pen_pressure);
                            let line_from = if mods.shift { state.last_stroke_point } else { None };
                            if let Some(from) = line_from {
                                let _ = state.session.stroke_to(&camera, from, true);
                                let _ = state.session.stroke_to(&camera, sp, true);
                            } else {
                                let _ = state.session.stroke_to(&camera, sp, false);
                            }
                            state.last_stroke_point = Some(sp);
                        }
                        Err(subcraft_paint::PaintError::NoCloneSource) => state.set_status(crate::keys::native("Ctrl+click the model to set the clone source first"), now),
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
            state.session.set_stroke_pressure(state.pen_pressure);
            // Every pointer position of this frame, not just the last: a pen reports several
            // hundred per second, and stamping along all of them follows a fast curve instead
            // of cutting it into one chord per frame (at no extra stamping cost).
            let mut path: Vec<Vec2> = ui.input(|i| i.events.iter().filter_map(|e| if let egui::Event::PointerMoved(q) = e { Some(to_screen(*q)) } else { None }).collect());
            if path.last() != Some(&sp) { path.push(sp); }
            let _ = state.session.stroke_to_many(&camera, &path);
            state.last_stroke_point = Some(sp);
        }
    } else if state.painting && !primary_down {
        state.session.end_stroke();
        state.painting = false;
    }

    // ----- render --------------------------------------------------------------------------------
    state.sync_gpu();
    let painter = ui.painter_at(rect);
    let tracing = state.recovery.prefs.viewport.path_tracing && !bake_mode;
    let mut pt_error = None;
    match (&state.render_state, &mut state.renderer) {
        (Some(rs), Some(renderer)) => {
            let t0 = web_time::Instant::now();
            let mut params = state.camera.view_params(size);
            params.height_scale = state.height_scale;
            let view = &state.recovery.prefs.viewport;
            params.antialias = view.antialias;
            params.exposure = view.exposure;
            params.post = view.post_params_at(state.session.scene().mesh.bounds.diagonal(), state.camera.eye_distance());
            params.view_mode = view.view_mode();
            params.symmetry_guides = view.symmetry_guides && state.session.brush.options.has_symmetry() && !bake_mode;
            if params.symmetry_guides {
                let key = (std::sync::Arc::as_ptr(&state.session.scene().mesh) as usize, state.session.brush.options.symmetry, state.session.brush.options.radial);
                if state.symmetry_uploaded != Some(key) {
                    renderer.set_symmetry_guides(&rs.device, &state.session.symmetry_guide_segments());
                    state.symmetry_uploaded = Some(key);
                }
            }
            params.wireframe = view.wireframe && !bake_mode;
            if params.wireframe {
                crate::view_extras::sync_wireframe(renderer, &rs.device, view, &state.session.scene().mesh, &mut state.wire_uploaded);
            }
            if let Some(high) = &state.bake.preview_mesh {
                if !state.bake.preview_uploaded {
                    renderer.set_bake_preview(&rs.device, Some((&state.session.scene().mesh, high)));
                    state.bake.preview_uploaded = true;
                }
            } else if !state.bake.preview_uploaded {
                renderer.set_bake_preview(&rs.device, None);
                state.bake.preview_uploaded = true;
            }
            params.bake_overlay = state.bake.overlay_active();
            params.bake_cage = state.bake.cage;
            if let Some(high) = &state.bake.preview_mesh {
                let low = state.session.scene().mesh.bounds;
                params.bake_high_transform = state.session.settings().bake_placement.matrix((low.min, low.max), (high.bounds.min, high.bounds.max));
            }
            params.bake_high_opacity = state.bake.high_opacity;
            params.bake_distances = [state.bake.settings.cage_front, state.bake.settings.cage_rear];
            if state.bake.open && state.bake.live_maps && !params.bake_overlay {
                params.bake_map = state.bake.displayed_map;
            }
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
            // Rendering mode: path trace instead of rasterizing; any failure falls back to raster.
            let pt_settings = view.path_trace_settings(state.pt_paused);
            if !tracing && state.pt_active {
                renderer.release_path_tracer();
                state.pt_active = false;
                state.pt_status = None;
            }
            let mut traced = false;
            if tracing {
                let scene = state.session.scene();
                let key = std::sync::Arc::as_ptr(&scene.mesh) as usize as u64 ^ ((scene.mesh.triangle_count() as u64) << 40);
                let pt_scene = subcraft_render::PathTraceScene { mesh: &scene.mesh, bvh: &scene.bvh, key };
                match renderer.render_path_traced(&rs.device, &rs.queue, render_size, &params, &pt_scene, &pt_settings) {
                    Ok((_, status)) => {
                        traced = true;
                        state.pt_active = true;
                        state.pt_status = Some(status);
                    }
                    Err(e) => pt_error = Some(e),
                }
            }
            if !traced {
                renderer.render(&rs.device, &rs.queue, render_size, &params);
            }
            let target = if traced { renderer.path_target() } else { renderer.target() }.expect("rendered above");
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

    if let Some(e) = pt_error {
        log::warn!("{e}");
        state.recovery.prefs.viewport.path_tracing = false;
        state.pt_status = None;
        state.set_status(e, now);
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
    if state.tool == Tool::Clone && let Some(source) = state.session.clone_source() && let Some(at) = camera.project(source) {
        let marker = rect.min + egui::vec2(at.x / pixel_scale.x, at.y / pixel_scale.y);
        let cross = Color32::from_rgb(120, 220, 255);
        for (w, c) in [(3.0, Color32::from_black_alpha(160)), (1.5, cross)] {
            painter.line_segment([marker - egui::vec2(7.0, 0.0), marker + egui::vec2(7.0, 0.0)], Stroke::new(w, c));
            painter.line_segment([marker - egui::vec2(0.0, 7.0), marker + egui::vec2(0.0, 7.0)], Stroke::new(w, c));
        }
        // While an aligned offset exists, the copy comes from beside the cursor: show it.
        if let (Some(offset), Some(p)) = (state.session.clone_offset(), response.hover_pos()) {
            let from = p + egui::vec2(offset.x / pixel_scale.x, offset.y / pixel_scale.y);
            painter.circle_stroke(from, 6.0, Stroke::new(1.5, cross));
        }
    }
    draw_stencil(state, ui, &painter, rect, size);
    if let Some(item) = drop_hover {
        painter.rect_stroke(rect.shrink(1.0), 0.0, Stroke::new(2.0, ACCENT), egui::StrokeKind::Inside);
        let target = state.session.selected_layer().map(|l| l.name.clone());
        let what = crate::panels::shelf::drop_label(item, mods.alt, target.as_deref().filter(|_| mods.alt), false);
        pill(&painter, rect.center_top() + egui::vec2(0.0, 14.0), egui::Align2::CENTER_TOP, &format!("Release to: {what}"), theme::TEXT);
    }
    let bake_hint = state.bake.displayed_map.map(|k| format!("Bake preview · {} · close Bake Mesh Maps to return to material", k.display_name()));
    let orbit = scheme.orbit_hint();
    let hint: String = if bake_mode {
        if state.bake.overlay_active() { format!("Blue: high poly · Red: exposed low poly · Pale: cage · {orbit} · Wheel zoom") }
        else { bake_hint.clone().unwrap_or_else(|| format!("Baking mesh maps · {orbit} · Wheel zoom")) }
    } else { String::new() };
    if bake_mode {
        pill(&painter, rect.left_bottom() + egui::vec2(10.0, -10.0), egui::Align2::LEFT_BOTTOM, &hint, theme::TEXT_FAINT);
    } else if state.recovery.prefs.viewport.hints {
        let c = crate::viewport_hints::HintContext {
            tool: state.tool,
            nav: scheme,
            keymap: state.recovery.prefs.keymap,
            ctrl: mods.command || mods.ctrl,
            shift: mods.shift,
            alt: mods.alt,
            tracing,
            hidden_parts: state.session.has_hidden_parts(),
            stencil: state.session.brush.stencil.is_some(),
            mask: state.session.brush.target.is_mask(),
            picking_parts: parts_picking,
            text_armed,
        };
        crate::viewport_hints::draw(&painter, rect, &crate::viewport_hints::cached(ui.ctx(), c));
    }
    let mut right = rect.right_top() + egui::vec2(-10.0, 10.0);
    if state.tool != Tool::Pen && state.session.brush.options.has_symmetry() {
        let options = &state.session.brush.options;
        let mut axes: String = ["X", "Y", "Z"].iter().zip(options.symmetry).filter(|(_, on)| *on).map(|(a, _)| *a).collect();
        let n = options.radial.effective_count(options.symmetry.iter().filter(|&&m| m).count());
        if n > 1 {
            if !axes.is_empty() { axes.push_str(" + "); }
            axes.push_str(&format!("Radial {}×{n}", ["X", "Y", "Z"][(options.radial.axis as usize).min(2)]));
        }
        let r = pill(&painter, right, egui::Align2::RIGHT_TOP, &format!("{}  Symmetry {axes}", icons::SYMMETRY), ACCENT);
        right.x = r.left() - 6.0;
    }
    if tracing && let Some(s) = state.pt_status {
        let progress = if s.max_samples > 0 { format!("{} / {} samples", s.samples, s.max_samples) } else { format!("{} samples", s.samples) };
        let what = if state.pt_paused { "paused".to_string() } else if s.converged() { "done".to_string() } else { format!("{:.0} samples/s", s.samples_per_second) };
        let r = pill(&painter, right, egui::Align2::RIGHT_TOP, &format!("{}  Rendering · {progress} · {what}  ·  F10 exit", icons::SUN), ACCENT);
        right.x = r.left() - 6.0;
        if !state.pt_paused && !s.converged() {
            ui.ctx().request_repaint();
        }
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
    brush_notice(state, ui, &painter, rect, now);
    rendering_toggle(state, ui, rect, bake_mode);
    if state.painting || response.dragged() {
        ui.ctx().request_repaint();
    }
}

/// Rendering mode (path tracing) toggle in the 3D view's bottom-right corner (F10 and the View
/// menu do the same); right-click for its settings, Pause while it is on.
fn rendering_toggle(state: &mut UiState, ui: &Ui, rect: Rect, bake_mode: bool) {
    if bake_mode {
        return;
    }
    egui::Area::new(ui.id().with("rendering_toggle"))
        .pivot(egui::Align2::RIGHT_BOTTOM)
        .fixed_pos(rect.right_bottom() + egui::vec2(-10.0, -10.0))
        .constrain_to(rect)
        .show(ui.ctx(), |ui| {
            ui.horizontal(|ui| {
                let prefs = &mut state.recovery.prefs.viewport;
                if prefs.path_tracing {
                    let label = if state.pt_paused { "Resume" } else { "Pause" };
                    let tip = if state.pt_paused { "Resume sampling" } else { "Pause sampling (keeps the current image)" };
                    if ui.button(label).on_hover_text(tip).clicked() {
                        state.pt_paused = !state.pt_paused;
                    }
                }
                let response = ui.selectable_label(prefs.path_tracing, format!("{}  Render", icons::SUN))
                    .on_hover_text("Rendering mode (F10) · path-traced preview: real shadows, reflections and bounce light. Samples add up while the camera is still. Right-click for settings");
                if response.clicked() {
                    prefs.path_tracing = !prefs.path_tracing;
                }
                egui::Popup::context_menu(&response)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| crate::viewport_bar::rendering_settings(ui, prefs));
            });
        });
}

/// "Brush: name" with its tip thumbnail, anchored inside the 3D view's top-left corner (below
/// the viewport toolbar). Paint-only: it never takes pointer input. Holds, then fades out.
fn brush_notice(state: &mut UiState, ui: &Ui, painter: &egui::Painter, rect: Rect, now: f64) {
    let Some(notice) = &state.brush_notice else { return };
    let Some(opacity) = notice.opacity(now) else { state.brush_notice = None; return };
    let fade = |c: Color32| c.gamma_multiply(opacity);
    let galley = painter.layout_no_wrap(format!("Brush: {}", notice.name), theme::font(12.0), fade(theme::TEXT));
    let thumb = crate::panels::shelf::tip_thumbnail(state, &notice.tip);
    let side = 22.0;
    let inset = if thumb.is_some() { side + 8.0 } else { 0.0 };
    let size = egui::vec2(galley.size().x + inset + 18.0, side.max(galley.size().y) + 10.0);
    let plate = Rect::from_min_size(rect.left_top() + egui::vec2(10.0, 10.0), size);
    painter.rect_filled(plate, 2.0, Color32::from_black_alpha((150.0 * opacity) as u8));
    painter.rect_filled(Rect::from_min_size(plate.min, egui::vec2(2.0, plate.height())), 0.0, fade(ACCENT));
    if let Some(texture) = thumb {
        let image = Rect::from_min_size(plate.min + egui::vec2(9.0, (plate.height() - side) / 2.0), egui::vec2(side, side));
        painter.image(texture, image, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), fade(Color32::WHITE));
    }
    painter.galley(plate.min + egui::vec2(9.0 + inset, (plate.height() - galley.size().y) / 2.0), galley, fade(theme::TEXT));
    ui.ctx().request_repaint();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::BrushNotice;

    fn draw(state: &mut UiState, ctx: &egui::Context, time: f64) -> Vec<egui::epaint::ClippedShape> {
        let rect = Rect::from_min_size(egui::pos2(200.0, 80.0), egui::vec2(600.0, 400.0));
        let mut out = ctx.run_ui(egui::RawInput { time: Some(time), screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 600.0))), ..Default::default() }, |ui| {
            let painter = ui.painter_at(rect);
            brush_notice(state, ui, &painter, rect, time);
        });
        out.textures_delta.clear();
        out.shapes
    }

    #[test]
    fn brush_notice_sits_in_the_viewport_top_left_and_fades_away() {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let mut state = UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
        state.brush_notice = Some(BrushNotice { name: "Scratches".into(), tip: "Scratches".into(), since: 1.0 });
        let text = |shapes: &[egui::epaint::ClippedShape]| shapes.iter().find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == "Brush: Scratches" => Some((t.pos, t.fallback_color, t.override_text_color)),
            _ => None,
        });
        let (pos, ..) = text(&draw(&mut state, &ctx, 1.2)).expect("notice drawn");
        assert!(pos.x > 200.0 && pos.x < 240.0 && pos.y > 80.0 && pos.y < 110.0, "anchored inside the top-left corner: {pos:?}");
        assert!(text(&draw(&mut state, &ctx, 1.0 + BrushNotice::HOLD + BrushNotice::FADE * 0.5)).is_some(), "still fading");
        assert!(text(&draw(&mut state, &ctx, 1.0 + BrushNotice::HOLD + BrushNotice::FADE + 0.01)).is_none());
        assert!(state.brush_notice.is_none(), "expired notice is cleared");
    }
}


/// Pen pressure after this frame's input events. egui forwards pen/finger contacts as touch
/// events; a pen reports its force (0..1), a finger or a mouse none, which counts as full
/// pressure. The value is kept between frames while the contact lasts and resets on release.
pub(crate) fn pen_pressure(events: &[egui::Event], current: f32) -> f32 {
    let mut pressure = current;
    for event in events {
        if let egui::Event::Touch { phase, force, .. } = event {
            pressure = match phase {
                egui::TouchPhase::End | egui::TouchPhase::Cancel => 1.0,
                _ => force.map_or(1.0, |f| f.clamp(0.0, 1.0)),
            };
        }
    }
    pressure
}

#[cfg(test)]
mod pressure_tests {
    use super::pen_pressure;

    fn touch(phase: egui::TouchPhase, force: Option<f32>) -> egui::Event {
        egui::Event::Touch {
            device_id: egui::TouchDeviceId(0),
            id: egui::TouchId(1),
            phase,
            pos: egui::Pos2::ZERO,
            force,
        }
    }

    #[test]
    fn mouse_stays_full_pressure() {
        assert_eq!(pen_pressure(&[], 1.0), 1.0);
    }

    #[test]
    fn pen_force_is_followed_and_reset_on_release() {
        let p = pen_pressure(&[touch(egui::TouchPhase::Start, Some(0.25))], 1.0);
        assert_eq!(p, 0.25);
        let p = pen_pressure(&[], p);
        assert_eq!(p, 0.25, "kept while the pen stays down");
        let p = pen_pressure(&[touch(egui::TouchPhase::Move, Some(0.8))], p);
        assert_eq!(p, 0.8);
        assert_eq!(pen_pressure(&[touch(egui::TouchPhase::End, Some(0.0))], p), 1.0);
    }

    #[test]
    fn finger_without_force_is_full_pressure() {
        assert_eq!(pen_pressure(&[touch(egui::TouchPhase::Start, None)], 0.3), 1.0);
    }
}

fn size_scale(rect: Rect, camera: &subcraft_app::Camera) -> Vec2 {
    Vec2::new(camera.viewport[0] as f32 / rect.width(), camera.viewport[1] as f32 / rect.height())
}

/// On-model handles for the selected text layer: centre = move (Shift+drag rotates), square corner =
/// scale, top circle = rotate. Drags coalesce into one undo step. Returns true while the pointer is
/// over a handle or dragging one, so the brush does not paint under it.
fn text_gizmo(state: &mut UiState, ui: &mut Ui, camera: &subcraft_app::Camera, rect: Rect, pixel_scale: Vec2, now: f64) -> bool {
    let Some(decal) = state.session.selected_text().cloned() else { return false };
    let Some(c) = pen_position(state, camera, rect, decal.center) else { return false };
    let painter = ui.painter_at(rect);
    let accent = theme::ACCENT;
    let corner = c + egui::vec2(36.0, 36.0);
    let top = c + egui::vec2(0.0, -40.0);
    let handles = [
        ("text_gizmo_move", c, 9.0),
        ("text_gizmo_scale", corner, 7.0),
        ("text_gizmo_rotate", top, 7.0),
    ];
    painter.line_segment([c, top], egui::Stroke::new(1.0, accent));
    painter.rect_stroke(Rect::from_two_pos(c, corner), 0.0, egui::Stroke::new(1.0, accent), egui::StrokeKind::Middle);
    let mut busy = false;
    let mut ended = false;
    let mut moved: Option<(&str, Vec2, Option<Pos2>)> = None;
    for (id, pos, r) in handles {
        let resp = ui.interact(Rect::from_center_size(pos, egui::vec2(r * 2.0 + 6.0, r * 2.0 + 6.0)), egui::Id::new(id), egui::Sense::click_and_drag());
        busy |= resp.hovered() || resp.dragged();
        let fill = if resp.hovered() || resp.dragged() { accent } else { theme::BG_0 };
        match id {
            "text_gizmo_move" => { painter.circle_filled(pos, r, fill); painter.circle_stroke(pos, r, egui::Stroke::new(1.5, accent)); }
            "text_gizmo_scale" => { painter.rect_filled(Rect::from_center_size(pos, egui::vec2(r * 2.0, r * 2.0)), 1.0, fill); }
            _ => { painter.circle_filled(pos, r, fill); painter.circle_stroke(pos, r, egui::Stroke::new(1.5, accent)); }
        }
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(if id == "text_gizmo_move" { egui::CursorIcon::Grab } else { egui::CursorIcon::ResizeNeSw });
            resp.clone().on_hover_text(match id { "text_gizmo_move" => "Drag to move the text on the surface (Shift+drag rotates)", "text_gizmo_scale" => "Drag to scale", _ => "Drag left or right to rotate" });
        }
        if resp.dragged() { moved = Some((id, Vec2::new(resp.drag_delta().x, resp.drag_delta().y), resp.interact_pointer_pos())); }
        if resp.drag_stopped() { ended = true; }
    }
    if let Some((id, delta, ptr)) = moved {
        let shift = ui.input(|i| i.modifiers.shift);
        let result = match (id, shift) {
            ("text_gizmo_move", false) => match ptr {
                Some(p) => state.session.drag_selected_text_to(camera, Vec2::new((p.x - rect.min.x) * pixel_scale.x, (p.y - rect.min.y) * pixel_scale.y)),
                None => Ok(()),
            },
            ("text_gizmo_scale", _) => {
                let mut d = decal.clone();
                d.size = (d.size * (1.0 + delta.x / 120.0)).clamp(subcraft_doc::TextDecal::MIN_SIZE, subcraft_doc::TextDecal::MAX_SIZE);
                state.session.update_selected_text_live(d)
            }
            _ => {
                let mut d = decal.clone();
                d.rotation += delta.x * 0.5f32.to_radians();
                state.session.update_selected_text_live(d)
            }
        };
        match result { Ok(()) => state.gpu_dirty = true, Err(e) => state.set_status(format!("Text: {e}"), now) }
    }
    if ended { state.session.end_text_gesture(); }
    busy
}
