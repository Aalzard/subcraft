//! 3D viewport: renders the session, handles navigation, painting and the brush cursor.

use egui::{Color32, PointerButton, Pos2, Rect, Sense, Stroke, Ui};
use glam::Vec2;
use subcraft_paint::StrokeMode;
use subcraft_render::wgpu_api as wgpu;

use crate::state::{Tool, UiState};
use crate::theme::{self, ACCENT};
use crate::icons;

/// Most border edges tested for visibility when outlining a drop target (one ray each).
const DROP_OUTLINE_EDGES: usize = 6000;

/// Screen segments of a part's border, hidden edges left out. The edges and their visibility
/// are computed once per part and camera (a ray per edge), then only projected.
fn drop_outline(state: &mut UiState, part: subcraft_app::PartKey, camera: &subcraft_app::Camera, rect: Rect, pixel_scale: Vec2) -> Vec<[Pos2; 2]> {
    if !state.drop_outline.as_ref().is_some_and(|(k, c, _)| *k == part && c == camera) {
        let all = state.session.part_outline(part);
        let stride = all.len().div_ceil(DROP_OUTLINE_EDGES).max(1);
        let tolerance = state.session.scene().mesh.bounds.diagonal() * 0.01;
        let visible = all
            .into_iter()
            .step_by(stride)
            .filter(|[a, b]| {
                let mid = (*a + *b) * 0.5;
                camera.project(mid).and_then(|s| state.session.pick(camera, s.truncate())).is_some_and(|hit| hit.position.distance(mid) <= tolerance)
            })
            .collect();
        state.drop_outline = Some((part, *camera, visible));
    }
    let Some((_, _, edges)) = &state.drop_outline else { return Vec::new() };
    let to_pos = |p: glam::Vec3| camera.project(p).map(|s| rect.min + egui::vec2(s.x / pixel_scale.x, s.y / pixel_scale.y));
    edges.iter().filter_map(|[a, b]| Some([to_pos(*a)?, to_pos(*b)?])).collect()
}

/// Text on a translucent dark plate, anchored at `pos`; returns the plate rect.
pub(crate) fn pill(painter: &egui::Painter, pos: Pos2, align: egui::Align2, text: &str, color: Color32) -> Rect {
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
    state.hovered_3d = response.hovered();
    let ppp = ui.ctx().pixels_per_point();
    let size = physical_size([rect.width(), rect.height()], ppp);
    if size[0] < 8 || size[1] < 8 {
        return;
    }
    state.last_view_size = size;
    state.last_view_rect = rect;
    let now = ui.input(|i| i.time);
    // Alt+H follows the selected layer's geometry mask.
    state.session.sync_excluded_view();
    // The render target is rounded to whole physical pixels. Map through that exact size,
    // rather than DPI alone, to keep picking/painting stable at fractional UI zoom factors.
    let pixel_scale = Vec2::new(size[0] as f32 / rect.width(), size[1] as f32 / rect.height());
    let to_screen = |p: Pos2| Vec2::new((p.x - rect.min.x) * pixel_scale.x, (p.y - rect.min.y) * pixel_scale.y);
    let (mods, scroll, delta, primary_pressed, primary_down, pointer) = ui.input(|i| {
        (i.modifiers, i.smooth_scroll_delta.y, i.pointer.delta(), i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.latest_pos())
    });
    // The press that closed the right-click Properties popup only closes it.
    let primary_pressed = primary_pressed && !state.view_props_swallow;

    // ----- stencil: hold S + drag (LMB rotate, MMB move, RMB scale); hold N to bypass ------------
    // Shift is the only modifier allowed with S: it snaps the rotation to quarter turns.
    let typing = ui.ctx().egui_wants_keyboard_input();
    let stencil_edit = stencil_input(state, ui, &response, rect, mods, delta);

    // ----- navigation: scheme from Preferences; mode changes mid-drag take over seamlessly -----
    use crate::nav::{NavDrag, NavInput, NavMode};
    let scheme = state.recovery.prefs.nav_scheme;
    if response.hovered() && scroll != 0.0 {
        state.camera.zoom(state.recovery.prefs.camera_speed.wheel_zoom(scroll));
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
            state.nav_drag = NavDrag::begin_with(scheme, &state.recovery.prefs.custom_mouse, nav_input, pos);
            state.orbit_free = None;
            // An orbit that starts over the model turns around that point (issue #115); over the
            // background the previous point is kept.
            if state.recovery.prefs.orbit_around_cursor
                && state.nav_drag.is_some_and(|d| matches!(d.mode(), NavMode::Orbit | NavMode::OrbitSnap))
                && let Some(hit) = state.session.pick(&state.camera.paint_camera(size), to_screen(p))
            {
                state.orbit_pivot = Some(hit.position);
            }
            // Blender scheme: Alt+MMB click centers the view on the point under the cursor.
            if scheme == crate::nav::NavScheme::Blender && nav_input.alt && nav_input.middle
                && let Some(hit) = state.session.pick(&state.camera.paint_camera(size), to_screen(p))
            {
                state.camera.target = hit.position;
                state.orbit_pivot = None;
            }
        } else if let Some(drag) = state.nav_drag.as_mut() {
            let pivot = state.orbit_pivot.filter(|_| state.recovery.prefs.orbit_around_cursor).unwrap_or(state.camera.target);
            match drag.step_with(scheme, &state.recovery.prefs.custom_mouse, nav_input, pos) {
                Some((mode, d)) => {
                    // Settings > General > Camera speeds.
                    let speed = state.recovery.prefs.camera_speed;
                    let turn = speed.orbit(d);
                    if mode != NavMode::OrbitSnap { state.orbit_free = None; }
                    match mode {
                        NavMode::Orbit if state.recovery.prefs.viewport.free_rotation => state.camera.free_orbit_around(pivot, turn.x, turn.y),
                        NavMode::Orbit => state.camera.orbit_around(pivot, turn.x, turn.y),
                        NavMode::OrbitSnap => {
                            let (yaw, pitch) = state.orbit_free.unwrap_or((state.camera.yaw, state.camera.pitch));
                            let free = (yaw - turn.x * 0.008, (pitch + turn.y * 0.008).clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2));
                            state.orbit_free = Some(free);
                            let step = std::f32::consts::FRAC_PI_2;
                            state.camera.set_angles_around(pivot, crate::nav::snap_angle(free.0, step), crate::nav::snap_angle(free.1, step));
                        }
                        NavMode::Pan => state.camera.pan(d.x * pixel_scale.x, d.y * pixel_scale.y, size[1] as f32),
                        NavMode::Zoom => state.camera.zoom(speed.drag_zoom(d.y)),
                        NavMode::Light => {
                            // Sideways turns the key light around the model, up and down tilts it
                            // (down to below the horizon: light from underneath).
                            state.light_yaw = (state.light_yaw + d.x * 0.01).rem_euclid(std::f32::consts::TAU);
                            state.light_pitch = (state.light_pitch - d.y * 0.01).clamp(-std::f32::consts::PI, std::f32::consts::PI);
                        }
                        NavMode::Environment => state.env.rotation_deg = crate::panels::shelf::wrap_rotation(state.env.rotation_deg + d.x * 0.5),
                        NavMode::None => {}
                    }
                }
                None => { state.nav_drag = None; state.orbit_free = None; }
            }
        }
    }
    // ----- Painter clicks that are not navigation: Ctrl+MMB focus, Ctrl+Alt+RMB texture set -----
    let ctrl = mods.ctrl || mods.command;
    let painter_nav = scheme == crate::nav::NavScheme::Painter;
    let (middle_pressed, secondary_pressed) = ui.input(|i| (i.pointer.button_pressed(PointerButton::Middle), i.pointer.button_pressed(PointerButton::Secondary)));
    let pick_set = painter_nav && ctrl && mods.alt && !mods.shift;
    if painter_nav && !stencil_edit && response.hovered() && let Some(p) = pointer {
        if middle_pressed && ctrl && !mods.alt && !mods.shift {
            match state.session.pick(&state.camera.paint_camera(size), to_screen(p)) {
                Some(hit) => {
                    let view = &mut state.recovery.prefs.viewport;
                    view.dof = true;
                    view.dof_focus_distance = Some(hit.depth.max(1e-4));
                    state.set_status(crate::i18n::tr("Focus set"), now);
                }
                None => state.set_status(crate::i18n::tr("Click the model to set the focus"), now),
            }
        }
        if secondary_pressed && pick_set
            && let Some(hit) = state.session.pick(&state.camera.paint_camera(size), to_screen(p))
            && let Some((id, name)) = state.session.document().texture_set_for_slot(hit.material_slot).map(|t| (t.id, t.name.clone()))
        {
            match state.session.execute(subcraft_app::Command::SelectTextureSet(id)) {
                Ok(()) => state.set_status(format!("Texture set: {name}"), now),
                Err(e) => state.set_status(e.to_string(), now),
            }
        }
    }

    // ----- Painter-style brush gestures: Ctrl+RMB drag = size / hardness, Ctrl+LMB drag = flow / angle -----
    let gesture_ok = !stencil_edit && !typing && !(state.bake.open || state.bake.job.is_some());
    let lmb_adjust = brush_gestures(state, &response, mods, delta, gesture_ok && !state.painting);
    let v_down = ui.input(|i| i.key_down(egui::Key::V)) && !typing && !mods.any();

    // Imported cameras: a locked one ignores the navigation above, an unlocked one hands over.
    crate::view_extras::follow_camera(state);
    let camera = state.camera.paint_camera(size);
    // Auto-focus: the depth of field focuses on whatever is at the centre of the view.
    if !(state.bake.open || state.bake.job.is_some()) && state.recovery.prefs.viewport.dof && state.recovery.prefs.viewport.dof_autofocus
        && let Some(hit) = state.session.pick(&camera, glam::Vec2::new(size[0] as f32 * 0.5, size[1] as f32 * 0.5))
    {
        state.recovery.prefs.viewport.dof_focus_distance = Some(hit.depth.max(1e-4));
    }

    // Right-click (no drag): the Properties popup at the cursor, part actions in its header.
    if response.secondary_clicked() && !stencil_edit && !pick_set && crate::view_props::opens(state, mods) && let Some(p) = pointer {
        state.context_part = state.session.surface_target(&camera, to_screen(p), state.parts_kind);
        crate::view_props::open(state, p, crate::view_props::View::View3d);
    }
    if response.hovered() && !typing && !mods.any() {
        let target = pointer.and_then(|p| state.session.surface_target(&camera, to_screen(p), state.parts_kind));
        // With a quick mask, I inverts it (Painter) instead of isolating the part under the cursor.
        let quick = state.session.quick_mask().is_some();
        for (key, action) in [(egui::Key::I, subcraft_app::PartAction::Isolate), (egui::Key::H, subcraft_app::PartAction::Hide)] {
            if quick && key == egui::Key::I { continue; }
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
        // Ctrl (Painter): the new layer shows only on the part under the cursor (geometry mask).
        let part = if (mods.command || mods.ctrl) && !mods.alt { pointer.and_then(|p| state.session.surface_target(&camera, to_screen(p), state.parts_kind)) } else { None };
        let before = state.session.selection().layer;
        // Alt: apply in place to the selected layer (alphas: brush tip unless Alt).
        crate::panels::shelf::apply_shelf_item_with(state, *item, mods.alt, now);
        let added = state.session.selection().layer.filter(|l| Some(*l) != before);
        if let (Some(target), Some(layer)) = (part, added)
            && state.session.selection().texture_set == target.texture_set
        {
            let parts = subcraft_doc::PartMask::for_kind(target.part.kind, [target.part.id]);
            match state.session.execute(subcraft_app::Command::SetParts { layer, parts: Some(parts) }) {
                Ok(()) => state.set_status(format!("Added on one {} only (its geometry mask)", target.part.kind.label().trim_end_matches('s').to_lowercase()), now),
                Err(e) => state.set_status(e.to_string(), now),
            }
        }
    }
    // An environment tile dropped on the view becomes the environment map.
    if let Some(env) = response.dnd_release_payload::<crate::panels::shelf::EnvironmentDrag>()
        && let Some(name) = state.library.environments.get(env.0).map(|e| e.name.clone())
    {
        state.env.index = env.0;
        state.set_status(format!("Environment: {name}"), now);
    }
    let env_hover = response.dnd_hover_payload::<crate::panels::shelf::EnvironmentDrag>().is_some();
    let drop_hover = response.dnd_hover_payload::<crate::panels::shelf::ShelfDrag>().map(|d| *d);

    // ----- part picking: clicks show / hide the selected layer on mesh parts ----------------------
    let bake_mode = state.bake.open || state.bake.job.is_some();
    // Symmetry manipulator: a press on a handle moves the mirror plane / radial axis instead of painting.
    // Not with the Pen, whose handles are not drawn either (QA 2026-11: an invisible handle took
    // the press that should place an anchor, and the drag moved the mirror plane).
    let symmetry_grab = !bake_mode && !alt && !stencil_edit && state.tool != Tool::Pen
        && crate::symmetry_view::manipulate(state, ui, &response, rect, pixel_scale, &camera, primary_pressed, primary_down, pointer);
    let primary_pressed = primary_pressed && !symmetry_grab;
    // Warp projection grid of the selected fill layer: presses on it edit the grid, not paint.
    let warp_grab = !bake_mode && !alt && !stencil_edit
        && crate::warp_view::manipulate(state, ui, &response, rect, pixel_scale, &camera, primary_pressed, primary_down, pointer);
    let primary_pressed = primary_pressed && !warp_grab;
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
    // ----- ID color editor eyedropper: clicks select the ID color under the cursor --------------
    let palette_picking = !bake_mode && !state.painting && !id_picking && state.picking_id_palette;
    if state.picking_id_palette && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.picking_id_palette = false;
    }
    if palette_picking && primary_pressed && response.hovered() && !alt && !stencil_edit && let Some(p) = pointer {
        match state.session.id_color_under(&camera, to_screen(p)) {
            Some((ts, color)) => {
                if state.session.selection().texture_set != ts { let _ = state.session.execute(subcraft_app::Command::SelectTextureSet(ts)); }
                crate::panels::id_colors::picked(state, ts, color, now);
            }
            None => state.set_status("No ID color there: missed the model, or this part has no ID map.", now),
        }
    }
    if palette_picking && response.hovered() { ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair); }
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
    // Ctrl + middle click focuses on the point under the cursor (the Display Settings' shortcut).
    if !bake_mode && !state.painting && response.hovered() && mods.command && !mods.shift && !mods.alt
        && ui.input(|i| i.pointer.button_clicked(egui::PointerButton::Middle))
        && let Some(p) = pointer
        && let Some(hit) = state.session.pick(&camera, to_screen(p))
    {
        state.recovery.prefs.viewport.dof_focus_distance = Some(hit.depth.max(1e-4));
        state.set_status(crate::i18n::tr("Focus set"), now);
    }
    let focus_picking = !bake_mode && !state.painting && state.picking_focus && state.recovery.prefs.viewport.dof;
    if focus_picking && primary_pressed && response.hovered() && let Some(p) = pointer {
        if let Some(hit) = state.session.pick(&camera, to_screen(p)) {
            state.recovery.prefs.viewport.dof_focus_distance = Some(hit.depth.max(1e-4));
            state.set_status(crate::i18n::tr("Focus set"), now);
        } else {
            state.set_status(crate::i18n::tr("Click the model to set the focus"), now);
        }
        state.picking_focus = false;
    } else if state.picking_focus && (!focus_picking || ui.input(|i| i.key_pressed(egui::Key::Escape))) {
        state.picking_focus = false;
    }
    let gizmo_busy = !bake_mode && !state.painting && !text_armed && text_gizmo(state, ui, &camera, rect, size_scale(rect, &camera), now);
    let picking = parts_picking || id_picking || palette_picking || text_armed || focus_picking || gizmo_busy;
    if state.picking_parts && (!picking || ui.input(|i| i.key_pressed(egui::Key::Escape))) {
        state.picking_parts = false;
    }
    let hovered_part = if parts_picking { pointer.and_then(|p| state.session.part_at(&camera, to_screen(p))) } else { None };
    // Geometry mask editing: a click shows / hides the layer on the part under the cursor, a drag
    // adds every part the box touches (Ctrl+drag removes them).
    if parts_picking && primary_pressed && response.hovered() && !alt && !stencil_edit && let Some(p) = pointer {
        // A layer without a geometry mask gets one of the kind chosen in the Parts panel.
        state.session.set_new_mask_kind(state.parts_kind);
        state.parts_box = Some(p);
    }
    if let Some(start) = state.parts_box {
        if !parts_picking || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            state.parts_box = None;
        } else if !primary_down {
            state.parts_box = None;
            let end = pointer.unwrap_or(start);
            if let Some(layer) = state.session.selected_layer().map(|l| l.id) {
                let result = if crate::view_props::is_box_drag(start, end) {
                    let parts = state.session.parts_in_screen_box(&camera, to_screen(start), to_screen(end));
                    if parts.is_empty() { Err("the box touched no part of the selected texture set".to_string()) }
                    else { state.session.set_layer_parts(layer, &parts, !(mods.command || mods.ctrl)).map_err(|e| e.to_string()) }
                } else {
                    match state.session.part_at(&camera, to_screen(start)) {
                        Some(part) => state.session.toggle_layer_part(layer, part).map_err(|e| e.to_string()),
                        None => Err("click a part of the selected texture set".to_string()),
                    }
                };
                if let Err(e) = result { state.set_status(format!("Parts: {e}"), now); }
            }
        }
    }

    // ----- painting ----------------------------------------------------------------------------
    // `stroke_pressure` is this frame's contact force: a quick pen tap can touch down, press and
    // lift within one frame, and its dab must use the tap's force, not the reset after release.
    let (stroke_pressure, carried) = pen_pressure(&ui.input(|i| i.events.clone()), state.pen_pressure);
    state.pen_pressure = carried;
    // The toolbar's pressure curve (a mouse reports 1, which every curve keeps).
    let stroke_pressure = state.recovery.prefs.viewport.pressure_curve.apply(stroke_pressure);
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
                Tool::Clone if (mods.command && !mods.shift) || v_down => {
                    if state.session.set_clone_source(&camera, sp) { state.set_status("Clone source set: now paint where the copy should go", now); }
                    else { state.set_status(crate::keys::native("Ctrl+click the model to set the clone source"), now); }
                }
                Tool::PolygonFill => state.fill_box = Some(p),
                _ => {
                    if let Err(e) = state.session.prepare_surface_paint(&camera, sp) { state.set_status(format!("Parts: {e}"), now); }
                    state.session.brush.mode = match state.tool {
                        Tool::Eraser => StrokeMode::Erase,
                        Tool::Clone => StrokeMode::Clone,
                        Tool::Smudge => StrokeMode::Smudge,
                        _ => StrokeMode::Paint,
                    };
                    if let Some(name) = state.session.ensure_paint_layer() {
                        state.gpu_dirty = true;
                        state.set_status(format!("Painting on a new paint layer, \"{name}\" (the selected layer was not a paint layer)"), now);
                    }
                    // Painter warns when the stroke starts on another texture set's part.
                    if let Some(hit) = state.session.pick(&camera, sp)
                        && hit.material_slot != state.session.selected_texture_set().material_slot
                        && let Some(name) = state.session.document().texture_set_for_slot(hit.material_slot).map(|t| t.name.clone())
                    {
                        state.set_status(crate::keys::native_string(format!("This part belongs to texture set \"{name}\": Ctrl+Alt+right-click it to paint there")), now);
                    }
                    match state.session.begin_stroke() {
                        Ok(()) => {
                            state.painting = true;
                            state.session.set_stroke_pressure(stroke_pressure);
                            let line_from = if mods.shift { state.line_start(&camera) } else { None };
                            // Ctrl+Shift+click: the line snaps to 5 degree steps, as in Painter, and
                            // the next line starts where this one ended (not at the raw click).
                            let end = line_from.map_or(sp, |from| straight_line_end(from, sp, mods.command || mods.ctrl));
                            if let Some(from) = line_from {
                                let _ = state.session.stroke_to(&camera, from, true);
                                let _ = state.session.stroke_to(&camera, end, true);
                            } else {
                                let _ = state.session.stroke_to(&camera, sp, false);
                            }
                            state.set_line_start(end, &camera);
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
            state.session.set_stroke_pressure(stroke_pressure);
            // Every pointer position of this frame, not just the last: a pen reports several
            // hundred per second, and stamping along all of them follows a fast curve instead
            // of cutting it into one chord per frame (at no extra stamping cost).
            let mut path: Vec<Vec2> = ui.input(|i| i.events.iter().filter_map(|e| if let egui::Event::PointerMoved(q) = e { Some(to_screen(*q)) } else { None }).collect());
            if path.last() != Some(&sp) { path.push(sp); }
            let _ = state.session.stroke_to_many(&camera, &path);
            state.set_line_start(sp, &camera);
        }
    } else if state.painting && !primary_down {
        state.session.end_stroke();
        state.painting = false;
    }

    // ----- polygon fill: a click fills the element under the cursor, a drag every element in the box -----
    let fill_rect = state.fill_box.zip(pointer).map(|(a, b)| Rect::from_two_pos(a, b));
    if state.fill_box.is_some() && (state.tool != Tool::PolygonFill || bake_mode || ui.input(|i| i.key_pressed(egui::Key::Escape))) {
        state.fill_box = None;
    } else if let Some(start) = state.fill_box
        && !primary_down
    {
        state.fill_box = None;
        let end = pointer.unwrap_or(start);
        state.session.brush.mode = StrokeMode::Paint;
        let result = if crate::view_props::is_box_drag(start, end) {
            state.session.polygon_fill_box(&camera, to_screen(start), to_screen(end), state.fill_mode, state.fill_visible_only)
        } else {
            state.session.polygon_fill(&camera, to_screen(start), state.fill_mode)
        };
        match result {
            Ok(tiles) if tiles.is_empty() && crate::view_props::is_box_drag(start, end) => state.set_status("Polygon fill: the box touched no triangle of this texture set", now),
            Ok(_) => {}
            Err(e) => state.set_status(format!("Polygon fill: {e}"), now),
        }
    }

    // ----- render --------------------------------------------------------------------------------
    state.sync_gpu();
    let painter = ui.painter_at(rect);
    let tracing = state.recovery.prefs.viewport.path_tracing && !bake_mode;
    let mut pt_error = None;
    let mask_view = crate::mask_view::active(state);
    match (&state.render_state, &mut state.renderer) {
        (Some(rs), Some(renderer)) => {
            let t0 = web_time::Instant::now();
            let mut params = state.camera.view_params(size);
            params.height_scale = state.height_scale;
            params.background = state.recovery.prefs.background_bottom;
            params.background_top = Some(state.recovery.prefs.background_top);
            let view = &state.recovery.prefs.viewport;
            params.antialias = view.antialias;
            params.oit = view.oit;
            params.exposure = view.exposure_value();
            params.post = view.post_params_at(state.session.scene().mesh.bounds.diagonal(), state.camera.eye_distance());
            params.view_mode = view.view_mode();
            params.solo_channel = view.solo.filter(|_| view.mesh_map.is_none());
            params.mask_view = mask_view;
            params.channel_display = view.channel_display();
            params.mip_bias = -(view.mip_bias.min(3) as f32);
            params.shadows = view.shadow_params();
            params.highlight_slot = state.highlight_slot.take();
            renderer.set_texture_filtering(&rs.device, view.anisotropy.samples());
            // Uploads that change neither the view nor the document (symmetry guides, wireframe,
            // bake preview, environment): TAA restarts on them too (QA 2026-11: a finished average
            // kept the old guide lines after a mirror-plane move).
            let mut overlay_uploaded = false;
            let symmetry_display = view.symmetry_display;
            params.symmetry_guides = view.symmetry_guides && state.session.brush.options.has_symmetry() && !bake_mode
                && !(view.symmetry_cursor_hide_painting && state.painting);
            if params.symmetry_guides {
                let key = crate::symmetry_view::GuideKey::new(&state.session, &symmetry_display);
                if state.symmetry_uploaded != Some(key) {
                    renderer.set_symmetry_guides(&rs.device, &crate::symmetry_view::guide_lines(&state.session, &symmetry_display));
                    state.symmetry_uploaded = Some(key);
                    overlay_uploaded = true;
                }
            }
            params.wireframe = view.wireframe && !bake_mode;
            if params.wireframe {
                let before = state.wire_uploaded;
                crate::view_extras::sync_wireframe(renderer, &rs.device, view, &state.session.scene().mesh, &mut state.wire_uploaded);
                overlay_uploaded |= state.wire_uploaded != before;
            }
            if let Some(high) = &state.bake.preview_mesh {
                if !state.bake.preview_uploaded {
                    renderer.set_bake_preview(&rs.device, Some((&state.session.scene().mesh, high)));
                    state.bake.preview_uploaded = true;
                    state.bake.miss_uploaded = false;
                    overlay_uploaded = true;
                }
            } else if !state.bake.preview_uploaded {
                renderer.set_bake_preview(&rs.device, None);
                state.bake.preview_uploaded = true;
            }
            // Seam check: found once per mesh, uploaded when the lines change.
            if state.bake.vis.show_seams {
                let mesh = &state.session.scene().mesh;
                let key = std::sync::Arc::as_ptr(mesh) as usize;
                if state.bake.seam_edges.as_ref().is_none_or(|(k, _)| *k != key) {
                    let edges = subcraft_bake::seam_check::hard_edges_without_seams(mesh).iter().map(|e| [e.a, e.b, e.normal]).collect();
                    state.bake.seam_edges = Some((key, edges));
                    state.bake.seams_uploaded = false;
                }
                if !state.bake.seams_uploaded && let Some((_, edges)) = &state.bake.seam_edges {
                    renderer.set_bake_seam_lines(&rs.device, edges);
                    state.bake.seams_uploaded = true;
                }
            }
            params.bake_overlay = state.bake.overlay_active();
            if state.bake.preview_mesh.is_some() && !state.bake.miss_uploaded && let Some(r) = &state.bake.cage_result {
                let maps: Vec<(u32, &subcraft_image::TiledImage)> = r.maps.iter().map(|(_, slot, img)| (*slot, img)).collect();
                renderer.set_bake_miss_maps(&rs.device, &rs.queue, &maps);
                state.bake.miss_uploaded = true;
                overlay_uploaded = true;
            }
            params.bake_miss_heatmap = state.bake.heatmap_shown();
            params.bake_cage = state.bake.cage;
            if let Some(high) = &state.bake.preview_mesh {
                let low = state.session.scene().mesh.bounds;
                params.bake_high_transform = state.session.settings().bake_placement.matrix((low.min, low.max), (high.bounds.min, high.bounds.max));
            }
            params.bake_high_opacity = state.bake.high_opacity;
            params.bake_vis = state.bake.vis;
            params.bake_vis.only_slot = state.bake.vis_selected_only.then(|| state.session.selected_texture_set().material_slot);
            params.bake_distances = [state.bake.settings.cage_front, state.bake.settings.cage_rear];
            // The mesh map viewer (View > Mesh maps, B): a baked map raw on the model.
            if !bake_mode { params.bake_map = state.recovery.prefs.viewport.mesh_map; }
            if state.bake.open && state.bake.live_maps && !params.bake_overlay {
                params.bake_map = state.bake.displayed_map.or(state.recovery.prefs.viewport.mesh_map);
            }
            params.grid = view.grid_params(state.session.scene().mesh.bounds);
            let render_size = view.render_size(size);
            let (env_rotation, env_intensity) = crate::panels::shelf::environment_view(&view.display, state.env.rotation_deg, state.camera.yaw);
            params.environment.background_opacity = view.env_opacity;
            params.environment.background_blur = view.env_blur;
            let env_before = state.env.uploaded;
            crate::panels::shelf::apply_environment(
                &state.library,
                state.env.index,
                env_rotation,
                env_intensity,
                &mut params,
                renderer,
                &mut state.env.uploaded,
                &rs.device,
                &rs.queue,
            );
            // Shift+MMB turns the key light around the model (on top of following the camera).
            if state.light_yaw != 0.0 || state.light_pitch != 0.0 {
                params.light_dir = lit_direction(params.light_dir, state.light_yaw, state.light_pitch);
            }
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
                // A resolution override keeps the vertical field of view and widens or narrows it.
                let trace_size = view.trace_size(size);
                let pt_params = trace_params(&params, render_size, trace_size);
                match renderer.render_path_traced(&rs.device, &rs.queue, trace_size, &pt_params, &pt_scene, &pt_settings) {
                    Ok((_, status)) => {
                        traced = true;
                        state.pt_active = true;
                        state.pt_status = Some(status);
                    }
                    Err(e) => pt_error = Some(e),
                }
            }
            if traced {
                state.taa = TaaState::default();
            } else {
                // Temporal anti-aliasing: number the frames of the average; once it has its
                // accumulations the finished image stays on screen until something changes.
                overlay_uploaded |= state.env.uploaded != env_before;
                let scene = (state.session.revision(), state.gpu_changed || state.painting || overlay_uploaded);
                let accumulations = view.taa.then_some(view.taa_accumulations);
                let kept = renderer.target().is_some_and(|t| t.size == render_size);
                match taa_step(&mut state.taa, &params, accumulations, scene) {
                    None if kept => {}
                    step => {
                        params.taa_frame = step.unwrap_or(0);
                        if step.is_none() { state.taa = TaaState::default(); }
                        renderer.render(&rs.device, &rs.queue, render_size, &params);
                        if params.taa_frame > 0 { ui.ctx().request_repaint(); }
                    }
                }
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
            let full = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            if traced && view.pt_override {
                // The override has its own aspect: letterbox it inside the view.
                let [w, h] = view.trace_size(size);
                painter.rect_filled(rect, 0.0, Color32::BLACK);
                painter.image(id, camera_frame_rect(rect, w as f32 / h as f32), full, Color32::WHITE);
            } else {
                painter.image(id, rect, full, Color32::WHITE);
            }
        }
        _ => {
            painter.rect_filled(rect, 0.0, theme::BG_0());
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, "No GPU renderer available", theme::font(15.0), theme::TEXT_FAINT());
        }
    }

    if let Some(e) = pt_error {
        log::warn!("{e}");
        state.recovery.prefs.viewport.path_tracing = false;
        state.pt_status = None;
        state.set_status(e, now);
    }

    // ----- overlays ------------------------------------------------------------------------------
    // Symmetry "Show Plane": each active mirror plane as a translucent sheet in its axis colour.
    if !bake_mode && state.recovery.prefs.viewport.symmetry_plane && state.tool != Tool::Pen {
        let (options, bounds) = (&state.session.brush.options, state.session.scene().mesh.bounds);
        for axis in (0..3).filter(|&a| options.symmetry[a]) {
            // At the plane's axis position (moved with the manipulator or the menu).
            let plane = options.mirror_plane(axis, bounds.center());
            let corners: Option<Vec<Pos2>> = mirror_plane_corners(&bounds, axis).iter()
                .map(|&q| { let mut q = q; q[axis] = plane; q })
                .map(|q| camera.project(q).map(|s| rect.min + egui::vec2(s.x / pixel_scale.x, s.y / pixel_scale.y)))
                .collect();
            if let Some(corners) = corners {
                let c = AXIS_COLORS[axis];
                painter.add(egui::Shape::convex_polygon(corners, c.gamma_multiply(0.12), Stroke::new(1.0, c.gamma_multiply(0.6))));
            }
        }
    }
    if !bake_mode {
        crate::paths_panel::show(state, ui, rect, now);
    }
    if state.tool == Tool::Pen && let Some(path) = state.pen.preview(&state.session) {
        crate::pen::draw(&painter, path, state.pen.selected_anchor(), |uv| pen_position(state, &camera, rect, uv));
    }
    if let Some(p) = response.hover_pos() {
        if paint_tool && !alt {
            if let Some(pick) = state.session.pick(&camera, to_screen(p)) {
                // Texture size space needs the set's texel density (cached per material slot).
                let density = if state.session.brush.settings.size_space == subcraft_paint::SizeSpace::Texture {
                    let (slot, mesh) = (pick.material_slot, &state.session.scene().mesh);
                    *state.uv_density.entry(slot).or_insert_with(|| subcraft_paint::stroke::uv_per_unit(mesh, slot))
                } else { 0.0 };
                let ppu = camera.pixels_per_unit(pick.depth);
                let r_px = state.session.brush_radius_in(ppu, size[1] as f32, density) * ppu / pixel_scale.y;
                let color = if state.tool == Tool::Eraser { Color32::from_rgb(120, 190, 255) } else { ACCENT() };
                let style = state.recovery.prefs.viewport.brush_cursor;
                let hardness = state.session.brush.settings.hardness;
                brush_mark(&painter, p, r_px, hardness, color, style, true);
                // Symmetry: a second cursor where each copy paints (faint when the copy's point
                // is hidden behind the model).
                let prefs = &state.recovery.prefs.viewport;
                if prefs.symmetry_cursor && !(prefs.symmetry_cursor_hide_painting && state.painting) {
                    let options = &state.session.brush.options;
                    let center = state.session.scene().mesh.bounds.center();
                    for (at, visible) in symmetry_cursors(&camera, options, center, pick.position, |s| state.session.pick(&camera, s).map(|h| h.depth)) {
                        let at = rect.min + egui::vec2(at.x / pixel_scale.x, at.y / pixel_scale.y);
                        let c = if visible { color.gamma_multiply(0.8) } else { color.gamma_multiply(0.3) };
                        brush_mark(&painter, at, r_px, hardness, c, style, false);
                    }
                }
            }
            // Lazy mouse: the grey ring the cursor moves in before the brush follows.
            let lazy = state.session.brush.options.lazy_distance_px;
            if lazy > 0.0 {
                painter.circle_stroke(p, lazy / pixel_scale.y, Stroke::new(1.0, Color32::from_gray(150)));
            }
            // Shift: dotted preview of the straight line from the last stroke point (snapped with
            // Ctrl, exactly where the click would draw it).
            if mods.shift && !state.painting && let Some(from) = state.line_start(&camera) {
                let to_pos = |s: Vec2| rect.min + egui::vec2(s.x / pixel_scale.x, s.y / pixel_scale.y);
                let end = straight_line_end(from, to_screen(p), mods.command || mods.ctrl);
                painter.extend(egui::Shape::dashed_line(&[to_pos(from), to_pos(end)], Stroke::new(1.2, Color32::from_white_alpha(200)), 5.0, 4.0));
            }
        } else if state.tool == Tool::PolygonFill && !picking {
            painter.circle_filled(p, 3.0, ACCENT());
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
    if let Some(r) = fill_rect.filter(|r| state.fill_box.is_some() && crate::view_props::is_box_drag(r.min, r.max)) {
        crate::view_props::draw_fill_box(&painter, r, state.fill_visible_only);
    }
    if let (Some(a), Some(b)) = (state.parts_box, pointer) && crate::view_props::is_box_drag(a, b) {
        crate::view_props::draw_fill_box(&painter, Rect::from_two_pos(a, b), false);
    }
    crate::symmetry_view::overlay(state, ui, &painter, rect, pixel_scale, &camera, !bake_mode && state.tool != Tool::Pen);
    if !bake_mode {
        crate::warp_view::overlay(state, ui, &painter, rect, pixel_scale, &camera);
    }
    draw_stencil(state, ui, &painter, rect, size);
    // Reset button at the top right of the view while S is held.
    if stencil_edit {
        let transformed = state.session.brush.stencil.as_ref().is_some_and(|s| s.is_transformed());
        let at = Rect::from_min_size(egui::pos2(rect.right() - 152.0, rect.top() + 10.0), egui::vec2(142.0, 26.0));
        let button = ui.put(at, egui::Button::new(format!("{}  Reset stencil", crate::icons::UNDO)));
        if button.on_hover_text("Stencil back to the center, upright, at its default size").clicked() && transformed {
            reset_stencil(state);
        }
    }
    visualization_overlays(state, &painter, rect, bake_mode);
    if env_hover {
        painter.rect_stroke(rect.shrink(2.0), 0.0, egui::Stroke::new(2.0, theme::ACCENT()), egui::StrokeKind::Inside);
        painter.text(rect.center_top() + egui::vec2(0.0, 28.0), egui::Align2::CENTER_CENTER, crate::i18n::tr("Use as the environment map"), theme::font(13.0), theme::ACCENT());
    }
    if let Some(item) = drop_hover {
        painter.rect_stroke(rect.shrink(1.0), 0.0, Stroke::new(2.0, ACCENT()), egui::StrokeKind::Inside);
        // The geometry that receives the drop: the part under the cursor with Ctrl (outlined),
        // otherwise its texture set (highlighted by the renderer).
        let part_drop = (mods.command || mods.ctrl) && !mods.alt && !matches!(item, crate::panels::shelf::ShelfDrag::Alpha(_) | crate::panels::shelf::ShelfDrag::SmartMask(_));
        if let Some(p) = pointer {
            if part_drop && let Some(target) = state.session.surface_target(&camera, to_screen(p), state.parts_kind) {
                for [a, b] in drop_outline(state, target.part, &camera, rect, pixel_scale) {
                    painter.line_segment([a, b], Stroke::new(3.0, Color32::from_black_alpha(140)));
                    painter.line_segment([a, b], Stroke::new(1.6, ACCENT()));
                }
            } else if let Some(hit) = state.session.pick(&camera, to_screen(p)) {
                state.highlight_slot = Some(hit.material_slot);
                ui.ctx().request_repaint();
            }
        }
        let target = state.session.selected_layer().map(|l| l.name.clone());
        let mut what = crate::panels::shelf::drop_label(item, mods.alt, target.as_deref().filter(|_| mods.alt), false);
        if (mods.command || mods.ctrl) && !mods.alt && !matches!(item, crate::panels::shelf::ShelfDrag::Alpha(_) | crate::panels::shelf::ShelfDrag::SmartMask(_)) {
            what.push_str(", only on the part under the cursor");
        }
        pill(&painter, rect.center_top() + egui::vec2(0.0, 14.0), egui::Align2::CENTER_TOP, &format!("Release to: {what}"), theme::OVERLAY_TEXT);
    }
    // Painter's mask view: say what is shown and how to get back; Esc goes back too.
    if let Some(text) = crate::mask_view::label(state) {
        pill(&painter, rect.center_top() + egui::vec2(0.0, 40.0), egui::Align2::CENTER_TOP, &text, theme::OVERLAY_TEXT);
        if response.hovered() && ui.input(|i| i.key_pressed(egui::Key::Escape)) { state.mask_view = None; }
    }
    let bake_hint = state.bake.displayed_map.map(|k| format!("Bake preview · {} · close Bake Mesh Maps to return to material", k.display_name()));
    let orbit = scheme.orbit_hint();
    let hint: String = if bake_mode {
        if state.bake.overlay_active() { format!("Blue: high poly · Red: exposed low poly · Pale: cage · {orbit} · Wheel zoom") }
        else { bake_hint.clone().unwrap_or_else(|| format!("Baking mesh maps · {orbit} · Wheel zoom")) }
    } else { String::new() };
    let mut hint_plate = None;
    if bake_mode {
        pill(&painter, rect.left_bottom() + egui::vec2(10.0, -10.0), egui::Align2::LEFT_BOTTOM, &hint, theme::OVERLAY_TEXT_FAINT);
    } else if state.recovery.prefs.viewport.hints {
        let c = crate::viewport_hints::HintContext {
            tool: state.tool,
            nav: scheme,
            keymap: state.recovery.prefs.keys(),
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
        hint_plate = crate::viewport_hints::draw(&painter, rect, &crate::viewport_hints::cached(ui.ctx(), c));
    }
    // The grid's unit, bottom left while the grid is on: just above the hints when they show.
    let grid_on = state.recovery.prefs.viewport.grid;
    if !bake_mode && grid_on && let Some(step) = subcraft_render::grid_step(state.session.scene().mesh.bounds) {
        pill(&painter, grid_readout_anchor(rect, hint_plate), egui::Align2::LEFT_BOTTOM, &grid_unit_label(step), theme::OVERLAY_TEXT_FAINT);
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
        let r = pill(&painter, right, egui::Align2::RIGHT_TOP, &format!("{}  Symmetry {axes}", icons::SYMMETRY), ACCENT());
        right.x = r.left() - 6.0;
    }
    if let Some(q) = state.session.quick_mask() {
        let text = if q.editing { "Quick mask · editing · U apply · Y clear" } else { "Quick mask on · U edit · I invert · Y clear" };
        let r = pill(&painter, right, egui::Align2::RIGHT_TOP, &format!("{}  {text}", icons::SELECTION), ACCENT());
        right.x = r.left() - 6.0;
    }
    if state.session.hide_excluded() {
        let r = pill(&painter, right, egui::Align2::RIGHT_TOP, &format!("{}  Excluded geometry hidden · Alt+H", icons::EYE_SLASH), ACCENT());
        right.x = r.left() - 6.0;
    }
    if tracing && let Some(s) = state.pt_status {
        let progress = if s.max_samples > 0 { format!("{} / {} samples", s.samples, s.max_samples) } else { format!("{} samples", s.samples) };
        let what = if state.pt_paused { "paused".to_string() } else if s.converged() { "done".to_string() } else { format!("{:.0} samples/s", s.samples_per_second) };
        let r = pill(&painter, right, egui::Align2::RIGHT_TOP, &format!("{}  Rendering · {progress} · {what}  ·  F10 exit", icons::SUN), ACCENT());
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
        let r = pill(&painter, right, egui::Align2::RIGHT_TOP, &format!("{}  Picking parts: {what}  ·  Esc done", icons::CURSOR), ACCENT());
        right.x = r.left() - 6.0;
    }
    match state.session.brush.target {
        subcraft_paint::PaintTarget::MaskBase => { pill(&painter, right, egui::Align2::RIGHT_TOP, &format!("{}  Painting mask", icons::MASK), ACCENT()); }
        subcraft_paint::PaintTarget::MaskEffect(_) => { pill(&painter, right, egui::Align2::RIGHT_TOP, &format!("{}  Painting mask Paint effect", icons::MASK), ACCENT()); }
        subcraft_paint::PaintTarget::ContentEffect(_) => { pill(&painter, right, egui::Align2::RIGHT_TOP, &format!("{}  Painting Paint effect", icons::PAINT_BRUSH), ACCENT()); }
        subcraft_paint::PaintTarget::Content => {}
    }
    brush_notice(state, ui, &painter, rect, now);
    // Painter's engine progress bar: a thin moving bar at the top while layers are computed;
    // paused (Shift+Esc), a notice instead and nothing moves.
    if state.session.engine_paused() {
        pill(&painter, rect.center_top() + egui::vec2(0.0, 44.0), egui::Align2::CENTER_TOP, crate::i18n::tr("Computation paused · Shift+Esc to resume"), ACCENT());
    } else if state.session.composites_pending() {
        computing_bar(&painter, rect, now);
        ui.ctx().request_repaint();
    }
    rendering_toggle(state, ui, rect, bake_mode);
    // Texture set highlight under the cursor while Ctrl+Alt (texture set selection) is held, not
    // while painting or navigating.
    if !bake_mode && (mods.ctrl || mods.command) && mods.alt && !state.painting && state.nav_drag.is_none()
        && let Some(p) = response.hover_pos()
        && let Some(hit) = state.session.pick(&state.camera.paint_camera(size), to_screen(p))
    {
        state.highlight_slot = Some(hit.material_slot);
        ui.ctx().request_repaint();
    }
    display_corner(state, ui, rect, bake_mode);
    camera_corner(state, ui, rect, bake_mode);
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

/// Painter's camera dropdown in the 3D view's top-right corner, only when the project has
/// imported cameras: Default camera or one of them, with a lock beside it.
/// Height taken by the display-mode dropdown at the 3D view's top right (the camera dropdown
/// sits below it).
const DISPLAY_CORNER_HEIGHT: f32 = 32.0;

/// Painter's display-mode dropdown at the 3D view's top right: material, each channel of the
/// selected texture set, the baked-normal views and the mesh maps.
fn display_corner(state: &mut UiState, ui: &Ui, rect: Rect, bake_mode: bool) {
    if bake_mode {
        return;
    }
    let channels: Vec<_> = state.session.selected_texture_set().channel_kinds().collect();
    egui::Area::new(ui.id().with("display_corner"))
        .pivot(egui::Align2::RIGHT_TOP)
        .fixed_pos(rect.right_top() + egui::vec2(-10.0, 40.0))
        .constrain_to(rect)
        .show(ui.ctx(), |ui| {
            ui.horizontal(|ui| crate::viewport_bar::view_mode(ui, &mut state.recovery.prefs.viewport, &channels, true));
        });
}

fn camera_corner(state: &mut UiState, ui: &Ui, rect: Rect, bake_mode: bool) {
    if bake_mode || state.session.scene_cameras().is_empty() {
        return;
    }
    egui::Area::new(ui.id().with("camera_corner"))
        .pivot(egui::Align2::RIGHT_TOP)
        .fixed_pos(rect.right_top() + egui::vec2(-10.0, 40.0 + DISPLAY_CORNER_HEIGHT))
        .constrain_to(rect)
        .show(ui.ctx(), |ui| {
            ui.horizontal(|ui| {
                crate::view_extras::camera_dropdown(ui, state, "viewport_camera");
                if state.active_camera.is_some() {
                    ui.checkbox(&mut state.camera_locked, crate::i18n::tr("Lock")).on_hover_text(crate::i18n::tr("Lock the camera: navigation leaves it where it is"));
                }
            });
        });
}

/// "Brush: name" with its tip thumbnail, anchored inside the 3D view's top-left corner (below
/// the viewport toolbar). Paint-only: it never takes pointer input. Holds, then fades out.
fn brush_notice(state: &mut UiState, ui: &Ui, painter: &egui::Painter, rect: Rect, now: f64) {
    let Some(notice) = &state.brush_notice else { return };
    let Some(opacity) = notice.opacity(now) else { state.brush_notice = None; return };
    let fade = |c: Color32| c.gamma_multiply(opacity);
    let galley = painter.layout_no_wrap(format!("Brush: {}", notice.name), theme::font(12.0), fade(theme::OVERLAY_TEXT));
    let thumb = crate::panels::shelf::tip_thumbnail(state, &notice.tip);
    let side = 22.0;
    let inset = if thumb.is_some() { side + 8.0 } else { 0.0 };
    let size = egui::vec2(galley.size().x + inset + 18.0, side.max(galley.size().y) + 10.0);
    let plate = Rect::from_min_size(rect.left_top() + egui::vec2(10.0, 10.0), size);
    painter.rect_filled(plate, 2.0, Color32::from_black_alpha((150.0 * opacity) as u8));
    painter.rect_filled(Rect::from_min_size(plate.min, egui::vec2(2.0, plate.height())), 0.0, fade(ACCENT()));
    if let Some(texture) = thumb {
        let image = Rect::from_min_size(plate.min + egui::vec2(9.0, (plate.height() - side) / 2.0), egui::vec2(side, side));
        painter.image(texture, image, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), fade(Color32::WHITE));
    }
    painter.galley(plate.min + egui::vec2(9.0 + inset, (plate.height() - galley.size().y) / 2.0), galley, fade(theme::OVERLAY_TEXT));
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

/// The key light direction `base` turned `yaw` radians around the vertical and tilted `pitch`
/// radians up (negative: down, below the horizon lights the model from underneath).
pub fn lit_direction(base: glam::Vec3, yaw: f32, pitch: f32) -> glam::Vec3 {
    let turned = glam::Quat::from_rotation_y(yaw) * base.normalize_or(glam::Vec3::Y);
    let flat = glam::Vec2::new(turned.x, turned.z);
    let flat = if flat.length() > 1e-5 { flat.normalize() } else { glam::Vec2::new(1.0, 0.0) };
    let limit = std::f32::consts::FRAC_PI_2 - 0.02;
    let elevation = (turned.y.clamp(-1.0, 1.0).asin() + pitch).clamp(-limit, limit);
    glam::Vec3::new(flat.x * elevation.cos(), elevation.sin(), flat.y * elevation.cos())
}

/// Position (0..1) of the moving segment of the progress bar at time `now` (seconds).
pub fn computing_phase(now: f64) -> f32 {
    (now * 0.8).rem_euclid(1.0) as f32
}

fn computing_bar(painter: &egui::Painter, rect: Rect, now: f64) {
    let track = Rect::from_min_size(rect.left_top(), egui::vec2(rect.width(), 2.0));
    painter.rect_filled(track, 0.0, Color32::from_white_alpha(18));
    let width = rect.width() * 0.25;
    let x = rect.left() - width + (rect.width() + width) * computing_phase(now);
    let seg = Rect::from_min_max(egui::pos2(x.max(rect.left()), track.top()), egui::pos2((x + width).min(rect.right()), track.bottom()));
    if seg.width() > 0.0 {
        painter.rect_filled(seg, 0.0, ACCENT());
    }
}

#[cfg(test)]
mod light_tests {
    use super::{computing_phase, lit_direction};
    use glam::Vec3;

    #[test]
    fn the_key_light_turns_tilts_and_reaches_under_the_model() {
        let base = Vec3::new(0.65, 0.75, 0.35).normalize();
        assert!((lit_direction(base, 0.0, 0.0) - base).length() < 1e-5, "no turn, no tilt: unchanged");
        let turned = lit_direction(base, std::f32::consts::FRAC_PI_2, 0.0);
        assert!((turned.y - base.y).abs() < 1e-5 && (turned.length() - 1.0).abs() < 1e-5, "turning keeps the height");
        let under = lit_direction(base, 0.0, -2.0);
        assert!(under.y < -0.5, "tilted down past the horizon the light comes from underneath: {under:?}");
        let top = lit_direction(base, 0.0, 10.0);
        assert!(top.y > 0.99 && top.y < 1.0, "the tilt stops just short of straight up (the direction stays defined): {top:?}");
        let straight_up = lit_direction(Vec3::Y, 1.0, -0.5);
        assert!(straight_up.is_finite() && straight_up.y < 1.0);
    }

    #[test]
    fn the_progress_segment_sweeps_and_wraps() {
        assert_eq!(computing_phase(0.0), 0.0);
        assert!((computing_phase(0.5) - 0.4).abs() < 1e-6);
        assert!(computing_phase(1.25) < 0.05, "wraps back to the start");
    }
}

/// Rotation of a stencil being dragged: the free angle, or its nearest quarter turn with Shift.
pub fn stencil_drag_rotation(free: f32, snap: bool) -> f32 {
    if snap { subcraft_paint::snap_rotation(free) } else { free }
}

/// Put the stencil back where a newly loaded one sits (image and tiling kept).
pub fn reset_stencil(state: &mut UiState) {
    if let Some(st) = state.session.brush.stencil.clone() {
        let mut st = (*st).clone();
        st.reset_transform();
        state.session.brush.stencil = Some(std::sync::Arc::new(st));
    }
}

/// Alpha (0..=255) of the stencil overlay: its display opacity, or 0 while it is hidden.
pub fn stencil_overlay_alpha(display: crate::state::StencilDisplay, painting: bool, bypass: bool) -> u8 {
    if bypass || (painting && display.hide_when_painting) {
        return 0;
    }
    (display.opacity.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// View parameters for tracing at `trace` pixels when the view renders at `view` pixels: the
/// projection keeps its vertical extent and takes the trace's aspect.
pub fn trace_params(params: &subcraft_render::ViewParams, view: [u32; 2], trace: [u32; 2]) -> subcraft_render::ViewParams {
    let aspect = |s: [u32; 2]| s[0].max(1) as f32 / s[1].max(1) as f32;
    let mut p = *params;
    let k = aspect(view) / aspect(trace);
    if (k - 1.0).abs() > 1e-6 && k.is_finite() {
        p.proj = glam::Mat4::from_scale(glam::Vec3::new(k, 1.0, 1.0)) * p.proj;
    }
    p
}

#[cfg(test)]
mod trace_params_tests {
    use super::trace_params;
    use glam::{Mat4, Vec3};

    #[test]
    fn a_wider_override_keeps_the_vertical_view_and_shows_more_to_the_sides() {
        let view = Mat4::look_at_rh(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y);
        let proj = Mat4::perspective_rh(40f32.to_radians(), 800.0 / 600.0, 0.1, 100.0);
        let params = subcraft_render::ViewParams::new(view, proj);
        // Same aspect: unchanged.
        assert_eq!(trace_params(&params, [800, 600], [1600, 1200]).proj, proj);
        // 16:9 from 4:3: the projection is exactly the 16:9 one with the same vertical angle.
        let wide = trace_params(&params, [800, 600], [1920, 1080]);
        let want = Mat4::perspective_rh(40f32.to_radians(), 1920.0 / 1080.0, 0.1, 100.0);
        assert!(wide.proj.abs_diff_eq(want, 1e-5), "{:?} vs {want:?}", wide.proj);
        assert_eq!(wide.view, params.view, "only the projection changes");
        // A point at the view's top edge stays at the top edge.
        let top = Vec3::new(0.0, 5.0 * (20f32.to_radians()).tan(), 0.0);
        assert!(((wide.proj * wide.view).project_point3(top).y - 1.0).abs() < 1e-4);
    }
}

/// The largest picture of aspect `ratio` (width / height) that fits `view`, centred: the camera
/// frame. A ratio that is not a positive number gives the whole view.
pub fn camera_frame_rect(view: Rect, ratio: f32) -> Rect {
    if !(ratio.is_finite() && ratio > 0.0) || view.width() <= 0.0 || view.height() <= 0.0 {
        return view;
    }
    let size = if view.width() / view.height() > ratio { egui::vec2(view.height() * ratio, view.height()) } else { egui::vec2(view.width(), view.width() / ratio) };
    Rect::from_center_size(view.center(), size)
}

/// Display Settings overlays: the camera frame with its gate mask, the grid unit and the name
/// of the color component shown.
fn visualization_overlays(state: &UiState, painter: &egui::Painter, rect: Rect, bake_mode: bool) {
    let prefs = &state.recovery.prefs.viewport;
    if prefs.camera_frame {
        let frame = camera_frame_rect(rect, prefs.camera_frame_ratio);
        let mask = Color32::from_black_alpha((prefs.gate_opacity.clamp(0.0, 1.0) * 255.0).round() as u8);
        // The four bars around the frame (two of them are empty, depending on the aspect).
        for bar in [
            Rect::from_min_max(rect.min, egui::pos2(rect.max.x, frame.min.y)),
            Rect::from_min_max(egui::pos2(rect.min.x, frame.max.y), rect.max),
            Rect::from_min_max(egui::pos2(rect.min.x, frame.min.y), egui::pos2(frame.min.x, frame.max.y)),
            Rect::from_min_max(egui::pos2(frame.max.x, frame.min.y), egui::pos2(rect.max.x, frame.max.y)),
        ] {
            if bar.width() > 0.5 && bar.height() > 0.5 {
                painter.rect_filled(bar, 0.0, mask);
            }
        }
        painter.rect_stroke(frame, 0.0, Stroke::new(1.0, Color32::from_white_alpha(170)), egui::StrokeKind::Inside);
    }
    if bake_mode {
        return;
    }
    if prefs.shown_channel().is_some() && let Some(name) = prefs.channel_component.overlay() {
        pill(painter, rect.left_top() + egui::vec2(10.0, 10.0), egui::Align2::LEFT_TOP, crate::i18n::tr(name), theme::OVERLAY_TEXT);
    }
    // Axis indicator above the Render toggle in the bottom-right corner.
    if prefs.axis_indicator {
        draw_axis_indicator(painter, rect.right_bottom() + egui::vec2(-44.0, -78.0), state.camera.view());
    }
}

/// Bottom-left corner of the grid unit read-out: the view's corner, or just above the hints
/// plate when the hints show.
pub fn grid_readout_anchor(view: Rect, hints: Option<Rect>) -> Pos2 {
    match hints {
        Some(plate) => egui::pos2(view.left() + 10.0, plate.top() - 6.0),
        None => view.left_bottom() + egui::vec2(10.0, -10.0),
    }
}

/// End of a Shift+click straight line from `from` towards the cursor: Ctrl snaps it to 5 degree
/// steps, as in Painter. The stroke and its dotted preview both use it.
pub fn straight_line_end(from: Vec2, cursor: Vec2, snap: bool) -> Vec2 {
    if snap { crate::nav::snap_line_end(from, cursor, 5.0) } else { cursor }
}

/// The world X, Y and Z axes as seen through `view`: axis index, screen direction (y down, length
/// up to 1) and view depth, farthest first so nearer axes draw over it.
pub fn axis_indicator(view: glam::Mat4) -> [(usize, egui::Vec2, f32); 3] {
    let mut axes = [0, 1, 2].map(|i| {
        let d = view.transform_vector3([glam::Vec3::X, glam::Vec3::Y, glam::Vec3::Z][i]);
        (i, egui::vec2(d.x, -d.y), d.z)
    });
    // The view looks down -Z: the most negative depth is the farthest.
    axes.sort_by(|a, b| a.2.total_cmp(&b.2));
    axes
}

/// Red X, green Y, blue Z (axis indicator, mirror planes).
const AXIS_COLORS: [Color32; 3] = [Color32::from_rgb(232, 84, 84), Color32::from_rgb(116, 204, 92), Color32::from_rgb(86, 146, 240)];

/// Corners of the mirror plane of `axis` (0 = X) through the centre of `bounds`, in order around
/// it, reaching 10 % past the model each way (and never thinner than 5 % of its size, so a flat
/// model still shows a plane).
pub fn mirror_plane_corners(bounds: &subcraft_mesh::Aabb, axis: usize) -> [glam::Vec3; 4] {
    let c = bounds.center();
    let half = ((bounds.max - bounds.min) * 0.55).max(glam::Vec3::splat(bounds.diagonal() * 0.05));
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(a, b): (f32, f32)| {
        let mut p = c;
        p[u] += a * half[u];
        p[v] += b * half[v];
        p
    })
}

/// Painter's orientation indicator: red X, green Y, blue Z from a common centre.
fn draw_axis_indicator(painter: &egui::Painter, centre: Pos2, view: glam::Mat4) {
    painter.circle_filled(centre, 32.0, Color32::from_black_alpha(60));
    for (i, d, depth) in axis_indicator(view) {
        let tip = centre + d * 22.0;
        // Axes pointing away are drawn dimmer.
        let color = if depth < -0.05 { AXIS_COLORS[i].gamma_multiply(0.55) } else { AXIS_COLORS[i] };
        painter.line_segment([centre, tip], Stroke::new(2.0, color));
        painter.circle_filled(tip, 7.0, color);
        painter.text(tip, egui::Align2::CENTER_CENTER, ["X", "Y", "Z"][i], egui::FontId::proportional(9.5), Color32::from_gray(20));
    }
}

/// Temporal anti-aliasing progress: the view the average belongs to and how many frames it has.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TaaState {
    last: Option<subcraft_render::ViewParams>,
    revision: u64,
    pub frame: u32,
}

/// The next temporal anti-aliasing frame for a view drawn with `params`: `Some(n)` draws frame
/// `n` of the average (1 starts it again, 0 means TAA is off); `None` means the average already
/// has its `accumulations` frames, so the image on screen is final. `scene` is the document
/// revision and whether the renderer received anything this frame; any change to it or to
/// `params` starts a new average.
pub fn taa_step(s: &mut TaaState, params: &subcraft_render::ViewParams, accumulations: Option<u32>, scene: (u64, bool)) -> Option<u32> {
    let Some(n) = accumulations.map(|n| n.clamp(1, 256)) else {
        *s = TaaState::default();
        return Some(0);
    };
    let key = subcraft_render::ViewParams { taa_frame: 0, ..*params };
    if scene.1 || s.revision != scene.0 || s.last != Some(key) {
        s.frame = 0;
    }
    s.last = Some(key);
    s.revision = scene.0;
    if s.frame >= n {
        return None;
    }
    s.frame += 1;
    Some(s.frame)
}

/// Accumulation counts offered for temporal anti-aliasing.
pub const TAA_ACCUMULATIONS: [u32; 5] = [4, 8, 16, 32, 64];

/// Half length of the crosshair cursor's arms, in points.
pub const CROSSHAIR_ARM: f32 = 8.0;

/// The brush cursor at `at`: the outline circle of radius `r` or a crosshair (Painter's cursor
/// display). `strong` adds a dark underlay so the cursor reads on any surface.
fn brush_mark(painter: &egui::Painter, at: Pos2, r: f32, hardness: f32, color: Color32, style: crate::viewport_bar::BrushCursor, strong: bool) {
    // Full preview: the tip's falloff as translucent rings under the outline (main cursor only).
    if strong && style == crate::viewport_bar::BrushCursor::Full {
        for (radius, width, strength) in crate::brush_cursor::preview_rings(r, hardness) {
            if strength > 0.0 && radius > 0.0 {
                // Slightly wider than their band so neighbours meet without gaps.
                painter.circle_stroke(at, radius, Stroke::new(width + 0.5, color.gamma_multiply(strength * crate::brush_cursor::PREVIEW_ALPHA)));
            }
        }
    }
    let mut strokes = Vec::new();
    if strong { strokes.push(Stroke::new(1.5, Color32::from_black_alpha(160))); }
    strokes.push(Stroke::new(1.0, color));
    for s in strokes {
        match style {
            crate::viewport_bar::BrushCursor::Outline | crate::viewport_bar::BrushCursor::Full => { painter.circle_stroke(at, r, s); }
            crate::viewport_bar::BrushCursor::Crosshair => {
                let (h, v) = (egui::vec2(CROSSHAIR_ARM, 0.0), egui::vec2(0.0, CROSSHAIR_ARM));
                painter.line_segment([at - h, at + h], s);
                painter.line_segment([at - v, at + v], s);
            }
        }
    }
}

/// Most symmetry cursors drawn (radial symmetry can make up to 64 copies).
const MAX_SYMMETRY_CURSORS: usize = 16;

/// Screen positions (physical pixels) of the symmetry copies of the brush point `p`, without
/// the brush itself, and whether each is visible: `depth_at` gives the depth of the model under
/// a screen point, and a copy farther than that (beyond a small tolerance) is behind it.
pub fn symmetry_cursors(camera: &subcraft_app::Camera, options: &subcraft_paint::StrokeOptions, center: glam::Vec3, p: glam::Vec3, depth_at: impl Fn(Vec2) -> Option<f32>) -> Vec<(Vec2, bool)> {
    let tolerance = |d: f32| d * 0.01 + 1e-4;
    options.symmetry_points(center, p).into_iter().skip(1).take(MAX_SYMMETRY_CURSORS).filter_map(|q| {
        let s = camera.project(q)?;
        let at = Vec2::new(s.x, s.y);
        let inside = at.x >= 0.0 && at.y >= 0.0 && at.x <= camera.viewport[0] as f32 && at.y <= camera.viewport[1] as f32;
        if !inside { return None; }
        let depth = camera.view.transform_point3(q).z.abs();
        let visible = depth_at(at).is_some_and(|d| depth <= d + tolerance(d));
        Some((at, visible))
    }).collect()
}

/// Bottom-left readout of the grid spacing, in scene units.
pub fn grid_unit_label(step: f32) -> String {
    let digits = if step >= 1.0 { 0 } else { (-step.log10()).ceil().clamp(0.0, 6.0) as usize };
    format!("Grid: {step:.digits$}")
}

/// Semi-transparent stencil image over the 3D view (hidden while N bypasses it), repeated along
/// its tiled axes.
/// Stencil keys shared by the 3D and 2D views: hold N to paint without it; hold S and drag to
/// place it (LMB rotate, Shift snaps to quarter turns; MMB move; RMB scale). `rect` is the view
/// the stencil is laid over. Returns whether S is editing the stencil (drags belong to it).
pub(crate) fn stencil_input(state: &mut UiState, ui: &Ui, response: &egui::Response, rect: Rect, mods: egui::Modifiers, delta: egui::Vec2) -> bool {
    // Shift is the only modifier allowed with S: it snaps the rotation to quarter turns.
    let (s_down, n_down) = ui.input(|i| (i.key_down(egui::Key::S) && !(i.modifiers.ctrl || i.modifiers.command || i.modifiers.alt), i.key_down(egui::Key::N) && !i.modifiers.any()));
    let typing = ui.ctx().egui_wants_keyboard_input();
    state.session.brush.stencil_bypass = n_down && !typing;
    let stencil_edit = s_down && !typing && state.session.brush.stencil.is_some();
    if stencil_edit && response.hovered() {
        let mut st = (**state.session.brush.stencil.as_ref().expect("checked above")).clone();
        let mut changed = false;
        // The free angle of the drag is kept apart so a snapped rotation follows the whole
        // gesture, not each small mouse step.
        let free_id = egui::Id::new("stencil-free-rotation");
        if response.dragged_by(PointerButton::Primary) {
            let free = ui.data(|d| d.get_temp::<f32>(free_id)).unwrap_or(st.rotation) + delta.x * 0.01;
            ui.data_mut(|d| d.insert_temp(free_id, free));
            st.rotation = stencil_drag_rotation(free, mods.shift);
            changed = true;
        } else {
            ui.data_mut(|d| d.remove::<f32>(free_id));
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
    stencil_edit
}

/// Painter's brush gestures, shared by the 3D and 2D views: Ctrl+RMB drag = size (left-right) /
/// hardness (up-down), Ctrl+LMB drag = flow / angle. `allowed` is false while the view's own
/// state rules them out (a stroke, the stencil, a text field). Returns whether Ctrl+LMB belongs
/// to the flow / angle drag (so the press must not start a stroke).
pub(crate) fn brush_gestures(state: &mut UiState, response: &egui::Response, mods: egui::Modifiers, delta: egui::Vec2, allowed: bool) -> bool {
    let gesture_ok = allowed && mods.command && !mods.alt;
    let brush_tool = matches!(state.tool, Tool::Brush | Tool::Eraser | Tool::Smudge | Tool::Clone);
    // Ctrl+Shift+LMB is the snapped straight line, so the flow / angle drag needs Shift up.
    let lmb_adjust = gesture_ok && !mods.shift && matches!(state.tool, Tool::Brush | Tool::Eraser | Tool::Smudge);
    if gesture_ok && brush_tool {
        let s = &mut state.session.brush.settings;
        if response.dragged_by(PointerButton::Secondary) {
            s.size = (s.size * (delta.x * 0.01).exp()).clamp(0.1, 100.0);
            s.hardness = (s.hardness - delta.y * 0.005).clamp(0.0, 1.0);
        } else if lmb_adjust && response.dragged_by(PointerButton::Primary) {
            s.flow = (s.flow + delta.x * 0.003).clamp(0.01, 1.0);
            s.angle_deg = (s.angle_deg + delta.y * 0.5).rem_euclid(360.0);
        }
    }
    lmb_adjust
}

pub(crate) fn draw_stencil(state: &mut UiState, ui: &Ui, painter: &egui::Painter, rect: Rect, size: [u32; 2]) {
    let Some(st) = state.session.brush.stencil.clone() else {
        state.stencil_tex = None;
        return;
    };
    let alpha = stencil_overlay_alpha(state.stencil_display, state.painting, state.session.brush.stencil_bypass);
    if alpha == 0 {
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
    let tint = Color32::from_white_alpha(alpha);
    let mut mesh = egui::Mesh::with_texture(tex.id());
    for offset in st.copy_offsets(size) {
        let base = mesh.vertices.len() as u32;
        for (p, uv) in c.iter().zip([egui::pos2(0.0, 0.0), egui::pos2(1.0, 0.0), egui::pos2(1.0, 1.0), egui::pos2(0.0, 1.0)]) {
            mesh.vertices.push(egui::epaint::Vertex { pos: to_ui(*p + offset), uv, color: tint });
        }
        mesh.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    painter.with_clip_rect(rect).add(egui::Shape::mesh(mesh));
    let outline: Vec<egui::Pos2> = c.iter().map(|p| to_ui(*p)).collect();
    painter.with_clip_rect(rect).add(egui::Shape::closed_line(outline, Stroke::new(1.0, Color32::from_white_alpha(90))));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::BrushNotice;

    #[test]
    fn taa_counts_frames_while_still_restarts_on_change_and_stops_when_done() {
        let view = subcraft_render::ViewParams::new(glam::Mat4::IDENTITY, glam::Mat4::IDENTITY);
        let mut s = TaaState::default();
        assert_eq!(taa_step(&mut s, &view, None, (0, false)), Some(0), "off");
        let steps: Vec<_> = (0..6).map(|_| taa_step(&mut s, &view, Some(4), (0, false))).collect();
        assert_eq!(steps, [Some(1), Some(2), Some(3), Some(4), None, None], "four frames, then the image is final");
        // The frame number itself does not count as a change.
        let numbered = subcraft_render::ViewParams { taa_frame: 9, ..view };
        assert_eq!(taa_step(&mut s, &numbered, Some(4), (0, false)), None);
        // A moved camera, an edit and a GPU upload each start again.
        let moved = subcraft_render::ViewParams { eye: glam::Vec3::X, ..view };
        assert_eq!(taa_step(&mut s, &moved, Some(4), (0, false)), Some(1));
        assert_eq!(taa_step(&mut s, &moved, Some(4), (0, false)), Some(2));
        assert_eq!(taa_step(&mut s, &moved, Some(4), (1, false)), Some(1), "document revision changed");
        assert_eq!(taa_step(&mut s, &moved, Some(4), (1, true)), Some(1), "the renderer received new data");
        assert_eq!(taa_step(&mut s, &moved, Some(4), (1, false)), Some(2));
        // Turning it off forgets the average.
        assert_eq!(taa_step(&mut s, &moved, None, (1, false)), Some(0));
        assert_eq!(s, TaaState::default());
    }

    #[test]
    fn the_grid_readout_sits_in_the_corner_or_above_the_hints() {
        let view = Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(800.0, 600.0));
        assert_eq!(grid_readout_anchor(view, None), egui::pos2(110.0, 640.0));
        let plate = Rect::from_min_max(egui::pos2(110.0, 560.0), egui::pos2(400.0, 640.0));
        let above = grid_readout_anchor(view, Some(plate));
        assert_eq!(above, egui::pos2(110.0, 554.0));
        assert!(above.y < plate.top(), "never on the hints");
    }

    #[test]
    fn mirror_planes_cut_through_the_centre_and_reach_past_the_model() {
        let b = subcraft_mesh::Aabb { min: glam::Vec3::new(-1.0, 0.0, -2.0), max: glam::Vec3::new(3.0, 2.0, 2.0) };
        let x = mirror_plane_corners(&b, 0);
        assert!(x.iter().all(|p| p.x == 1.0), "the X plane sits at the centre x: {x:?}");
        assert_eq!((x[0].y, x[2].y, x[0].z, x[2].z), (1.0 - 1.1, 1.0 + 1.1, -2.2, 2.2), "10 % past the model");
        let y = mirror_plane_corners(&b, 1);
        assert!(y.iter().all(|p| p.y == 1.0));
        // Corners go around the rectangle: opposite corners share no coordinate in the plane.
        for c in [x, y, mirror_plane_corners(&b, 2)] {
            let (d1, d2) = (c[2] - c[0], c[3] - c[1]);
            assert!((d1.length() - d2.length()).abs() < 1e-5 && d1.dot(d2).abs() < d1.length_squared(), "{c:?}");
        }
        // A flat model (no thickness along Y) still gets a visible Z plane height.
        let flat = subcraft_mesh::Aabb { min: glam::Vec3::new(-1.0, 0.0, -1.0), max: glam::Vec3::new(1.0, 0.0, 1.0) };
        let z = mirror_plane_corners(&flat, 2);
        assert!((z[2].y - z[0].y) > 0.1, "{z:?}");
    }

    #[test]
    fn symmetry_cursors_sit_on_the_mirrored_points_and_know_when_they_are_hidden() {
        let camera = subcraft_app::Camera {
            view: glam::Mat4::look_at_rh(glam::Vec3::new(0.0, 0.0, 10.0), glam::Vec3::ZERO, glam::Vec3::Y),
            proj: glam::Mat4::perspective_rh(40f32.to_radians(), 1.0, 0.1, 100.0),
            viewport: [400, 400],
        };
        let mirror_x = subcraft_paint::StrokeOptions { symmetry: [true, false, false], ..Default::default() };
        let p = glam::Vec3::new(1.0, 0.5, 0.0);
        // Nothing in front: the copy is visible, at the mirror image of the brush on screen.
        let cursors = symmetry_cursors(&camera, &mirror_x, glam::Vec3::ZERO, p, |_| Some(10.0));
        assert_eq!(cursors.len(), 1);
        let brush = camera.project(p).unwrap();
        let (at, visible) = cursors[0];
        assert!(visible);
        assert!((at.x - (400.0 - brush.x)).abs() < 1e-2 && (at.y - brush.y).abs() < 1e-2, "{at} vs brush {brush}");
        // The model is nearer there: the copy is behind it.
        let hidden = symmetry_cursors(&camera, &mirror_x, glam::Vec3::ZERO, p, |_| Some(5.0));
        assert!(!hidden[0].1);
        // No symmetry, no cursors; a copy off screen is left out.
        assert!(symmetry_cursors(&camera, &Default::default(), glam::Vec3::ZERO, p, |_| Some(10.0)).is_empty());
        let off = symmetry_cursors(&camera, &mirror_x, glam::Vec3::new(40.0, 0.0, 0.0), p, |_| Some(10.0));
        assert!(off.is_empty(), "the mirror image at x = 79 is outside the view");
        // Radial symmetry draws at most MAX_SYMMETRY_CURSORS.
        let many = subcraft_paint::StrokeOptions { radial: subcraft_paint::RadialSymmetry { enabled: true, axis: 2, count: 64, center_offset: [0.0; 3], flip: false }, ..Default::default() };
        assert_eq!(symmetry_cursors(&camera, &many, glam::Vec3::ZERO, glam::Vec3::new(0.5, 0.0, 0.0), |_| Some(10.0)).len(), MAX_SYMMETRY_CURSORS);
    }

    #[test]
    fn the_axis_indicator_follows_the_camera_and_draws_far_axes_first() {
        // Looking straight down -Z: X to the right, Y up, Z towards the viewer (a dot, drawn last).
        let front = glam::Mat4::look_at_rh(glam::Vec3::new(0.0, 0.0, 5.0), glam::Vec3::ZERO, glam::Vec3::Y);
        let axes = axis_indicator(front);
        let get = |i: usize| axes.iter().find(|a| a.0 == i).copied().unwrap();
        assert!((get(0).1 - egui::vec2(1.0, 0.0)).length() < 1e-5, "X points right");
        assert!((get(1).1 - egui::vec2(0.0, -1.0)).length() < 1e-5, "Y points up the screen");
        assert!(get(2).1.length() < 1e-5 && get(2).2 > 0.9, "Z faces the viewer");
        assert_eq!(axes[2].0, 2, "the axis towards the viewer is drawn last");
        // Orbiting to the right side (+X): Z now points left, X at the viewer.
        let side = glam::Mat4::look_at_rh(glam::Vec3::new(5.0, 0.0, 0.0), glam::Vec3::ZERO, glam::Vec3::Y);
        let axes = axis_indicator(side);
        let get = |i: usize| axes.iter().find(|a| a.0 == i).copied().unwrap();
        assert!((get(2).1 - egui::vec2(-1.0, 0.0)).length() < 1e-5, "Z points left from the +X side: {:?}", get(2).1);
        assert_eq!(axes[2].0, 0);
        // The orbit camera's three-quarter view: every axis visible, depths sorted.
        let b = subcraft_mesh::Aabb { min: glam::Vec3::splat(-1.0), max: glam::Vec3::splat(1.0) };
        let axes = axis_indicator(crate::orbit::OrbitCamera::framing(&b).view());
        assert!(axes.windows(2).all(|w| w[0].2 <= w[1].2));
        assert!(axes.iter().all(|a| a.1.length() > 0.2 && a.1.length() <= 1.0 + 1e-5));
    }

    #[test]
    fn the_straight_line_preview_ends_where_the_click_draws() {
        let from = Vec2::new(100.0, 100.0);
        let cursor = Vec2::new(200.0, 104.0);
        assert_eq!(straight_line_end(from, cursor, false), cursor);
        let snapped = straight_line_end(from, cursor, true);
        assert_eq!(snapped, crate::nav::snap_line_end(from, cursor, 5.0));
        assert!((snapped.y - from.y).abs() < 1e-3, "a near-horizontal line snaps flat: {snapped}");
    }

    #[test]
    fn the_camera_frame_is_the_largest_centred_picture_of_its_ratio() {
        let view = Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(800.0, 600.0));
        // 16:9 in a 4:3 view: full width, bars above and below.
        let wide = camera_frame_rect(view, 16.0 / 9.0);
        assert_eq!((wide.width(), wide.height(), wide.center()), (800.0, 450.0, view.center()));
        // A square: full height, bars left and right.
        let square = camera_frame_rect(view, 1.0);
        assert_eq!((square.width(), square.height(), square.center()), (600.0, 600.0, view.center()));
        assert_eq!(camera_frame_rect(view, 4.0 / 3.0), view, "the view's own ratio fills it");
        for bad in [0.0, -2.0, f32::NAN, f32::INFINITY] {
            assert_eq!(camera_frame_rect(view, bad), view);
        }
        assert_eq!((grid_unit_label(0.2), grid_unit_label(5.0), grid_unit_label(0.05), grid_unit_label(100.0)), ("Grid: 0.2".to_string(), "Grid: 5".to_string(), "Grid: 0.05".to_string(), "Grid: 100".to_string()));
    }

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


/// Pen pressure for this frame's input events, as `(this frame, carried to the next frame)`.
/// egui forwards pen/finger contacts as touch events; a pen reports its force (0..1), a finger or
/// a mouse none, which counts as full pressure. This frame's value is the last contact force seen
/// in the frame (or the carried one when there was none). The carried value is kept while the
/// contact lasts and resets to full on release, so a later mouse stroke isn't light.
pub(crate) fn pen_pressure(events: &[egui::Event], current: f32) -> (f32, f32) {
    let mut frame = None;
    let mut carried = current;
    for event in events {
        if let egui::Event::Touch { phase, force, .. } = event {
            match phase {
                egui::TouchPhase::End | egui::TouchPhase::Cancel => carried = 1.0,
                _ => {
                    let f = force.map_or(1.0, |f| f.clamp(0.0, 1.0));
                    frame = Some(f);
                    carried = f;
                }
            }
        }
    }
    (frame.unwrap_or(current), carried)
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
        assert_eq!(pen_pressure(&[], 1.0), (1.0, 1.0));
    }

    #[test]
    fn pen_force_is_followed_and_reset_on_release() {
        let (p, c) = pen_pressure(&[touch(egui::TouchPhase::Start, Some(0.25))], 1.0);
        assert_eq!((p, c), (0.25, 0.25));
        let (p, c) = pen_pressure(&[], c);
        assert_eq!((p, c), (0.25, 0.25), "kept while the pen stays down");
        let (p, c) = pen_pressure(&[touch(egui::TouchPhase::Move, Some(0.8))], c);
        assert_eq!((p, c), (0.8, 0.8));
        assert_eq!(pen_pressure(&[touch(egui::TouchPhase::End, Some(0.0))], c).1, 1.0);
    }

    #[test]
    fn a_tap_within_one_frame_paints_with_the_tap_force() {
        // Touch down, press and lift all arrive in one frame (a quick tap on a slow frame): the
        // stroke that starts this frame uses the tap's light force, not full pressure.
        let events = [touch(egui::TouchPhase::Start, Some(0.2)), touch(egui::TouchPhase::End, Some(0.0))];
        assert_eq!(pen_pressure(&events, 1.0), (0.2, 1.0));
    }

    #[test]
    fn finger_without_force_is_full_pressure() {
        assert_eq!(pen_pressure(&[touch(egui::TouchPhase::Start, None)], 0.3), (1.0, 1.0));
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
    let accent = theme::ACCENT();
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
        let fill = if resp.hovered() || resp.dragged() { accent } else { theme::BG_0() };
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
