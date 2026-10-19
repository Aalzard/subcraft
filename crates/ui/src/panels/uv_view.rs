//! 2D view: the selected texture set's channels or baked mesh maps, with its UV wireframe.
//! Brush, eraser and polygon fill paint the selected layer (or its mask) here in UV space.

use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Ui};
use glam::Vec2;
use subcraft_core::{ChannelKind, MeshMapKind};
use subcraft_mesh::topology;
use subcraft_paint::StrokeMode;
use subcraft_render::{PreviewSource, wgpu_api as wgpu};

use crate::state::{Tool, UiState};

fn label(s: PreviewSource) -> String {
    match s {
        PreviewSource::Channel(k) => k.display_name().to_string(),
        PreviewSource::MeshMap(k) => format!("Baked {}", k.display_name()),
    }
}

pub fn show(state: &mut UiState, ui: &mut Ui) {
    let ts = state.session.selected_texture_set();
    let (ts_id, slot, resolution) = (ts.id, ts.material_slot, ts.resolution);
    let mut sources: Vec<PreviewSource> = ts.channel_kinds().map(PreviewSource::Channel).collect();
    if let Some(maps) = state.session.mesh_maps(ts_id) {
        sources.extend(MeshMapKind::ALL.iter().filter(|k| maps.get(**k).is_some()).map(|k| PreviewSource::MeshMap(*k)));
    }
    if !sources.contains(&state.uv.source) {
        state.uv.source = PreviewSource::Channel(ChannelKind::BaseColor);
    }
    let painting_tool = crate::brush_bar::active(state.tool);
    egui::Frame::new().fill(crate::theme::BG_1).inner_margin(egui::Margin::symmetric(crate::theme::PAD, 6)).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new("Show").color(crate::theme::TEXT_DIM));
            egui::ComboBox::from_id_salt("uv_source").selected_text(label(state.uv.source)).width(200.0).show_ui(ui, |ui| {
                for s in &sources {
                    ui.selectable_value(&mut state.uv.source, *s, label(*s));
                }
            });
            if let PreviewSource::Channel(kind) = state.uv.source
                && crate::widgets::icon_button(ui, crate::icons::CROSSHAIR, "Paint only this channel (Shift+click a channel switch to undo)").clicked()
            {
                let mut material = state.session.brush.material.clone();
                let channels = state.session.channels();
                if !(channels.iter().all(|&k| material.is_enabled(k) == (k == kind))) {
                    material.solo(kind, &channels);
                    let now = ui.input(|i| i.time);
                    if let Err(e) = state.session.execute(subcraft_app::Command::SetBrushMaterial(material)) {
                        state.set_status(e.to_string(), now);
                    }
                }
            }
            ui.add_space(8.0);
            if crate::widgets::icon_toggle(ui, crate::icons::GRID, state.uv_wireframe, "Show UV lines").clicked() {
                state.uv_wireframe = !state.uv_wireframe;
            }
            if crate::widgets::icon_button(ui, crate::icons::FIT, "Fit to view").clicked() {
                state.uv.zoom = 1.0;
                state.uv.offset = egui::Vec2::ZERO;
            }
        });
        ui.horizontal_wrapped(|ui| brush_controls(state, ui));
        if painting_tool {
            crate::brush_bar::channel_switches(ui, state);
        }
    });
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            state.uv.zoom = (state.uv.zoom * (scroll * 0.002).exp()).clamp(0.2, 20.0);
        }
    }
    let alt = ui.input(|i| i.modifiers.alt);
    let paints = painting_tool && !alt;
    if response.dragged_by(egui::PointerButton::Middle)
        || (response.dragged_by(egui::PointerButton::Primary) && !state.uv.painting && (alt || !(paints || state.tool == Tool::Pen)))
    {
        state.uv.offset += response.drag_delta();
    }
    let side = rect.width().min(rect.height()) * 0.95 * state.uv.zoom;
    let img_rect = Rect::from_center_size(rect.center() + state.uv.offset, egui::vec2(side, side));
    crate::pen::uv_input(state, ui, &response, img_rect);
    paint_input(state, ui, &response, img_rect, resolution);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, crate::theme::BG_0);

    state.sync_gpu();
    if let (Some(rs), Some(renderer)) = (&state.render_state, &mut state.renderer)
        && let Some(preview) = renderer.preview(&rs.device, &rs.queue, slot, state.uv.source)
    {
        let view = &preview.display_view;
        let mut r = rs.renderer.write();
        let id = match state.uv.texture {
            Some(id) => {
                r.update_egui_texture_from_wgpu_texture(&rs.device, view, wgpu::FilterMode::Linear, id);
                id
            }
            None => {
                let id = r.register_native_texture(&rs.device, view, wgpu::FilterMode::Linear);
                state.uv.texture = Some(id);
                id
            }
        };
        painter.image(id, img_rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }
    painter.rect_stroke(img_rect, 0.0, Stroke::new(1.0, crate::theme::BORDER_STRONG), egui::StrokeKind::Outside);

    if state.uv_wireframe {
        let mesh = state.session.scene().mesh.clone();
        let edges = state.uv_edges.entry(slot).or_insert_with(|| topology::uv_edges(&mesh, mesh.material_triangles(slot)));
        let to = |p: glam::Vec2| img_rect.min + egui::vec2(p.x * side, p.y * side);
        let stroke = Stroke::new(0.6, Color32::from_rgba_unmultiplied(255, 255, 255, 70));
        for [a, b] in edges.iter() {
            painter.line_segment([to(*a), to(*b)], stroke);
        }
    }
    if state.tool == Tool::Pen && let Some(path) = state.pen.preview(&state.session) {
        crate::pen::draw(&painter, path, state.pen.selected_anchor(), |uv| Some(img_rect.min + egui::vec2(uv.x, uv.y) * side));
    }
    if let Some(p) = response.hover_pos() {
        if matches!(state.tool, Tool::Brush | Tool::Eraser) && !alt {
            let mesh = &state.session.scene().mesh;
            let density = *state.uv_density.entry(slot).or_insert_with(|| subcraft_paint::stroke::uv_per_unit(mesh, slot));
            let r_px = state.session.brush_radius_uv(density) * side;
            let color = if state.tool == Tool::Eraser { Color32::from_rgb(120, 190, 255) } else { crate::theme::ACCENT };
            painter.circle_stroke(p, r_px, Stroke::new(1.5, Color32::from_black_alpha(160)));
            painter.circle_stroke(p, r_px, Stroke::new(1.0, color));
        } else if state.tool == Tool::PolygonFill && !alt {
            painter.circle_filled(p, 3.0, crate::theme::ACCENT);
        }
        if paints {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
    }
    let hint = if paints { "LMB paint · Shift+click line · Alt+LMB / MMB pan · Wheel zoom" } else { "LMB / MMB pan · Wheel zoom" };
    painter.text(rect.left_bottom() + egui::vec2(10.0, -10.0), egui::Align2::LEFT_BOTTOM, hint, crate::theme::font(11.5), crate::theme::TEXT_FAINT);
    if state.uv.painting {
        ui.ctx().request_repaint();
    }
}

/// Tool icons and brush values, shared with the 3D view's top strip.
fn brush_controls(state: &mut UiState, ui: &mut Ui) {
    let mut settings = state.session.brush.settings;
    let mut options = state.session.brush.options;
    let (original, original_options) = (settings, options);
    let width = ui.available_width();
    crate::brush_bar::show(ui, &mut state.tool, &mut settings, &mut options, width);
    let now = ui.input(|i| i.time);
    if settings != original && let Err(e) = state.session.execute(subcraft_app::Command::SetBrushSettings(settings)) {
        state.set_status(e.to_string(), now);
    }
    if options != original_options && let Err(e) = state.session.execute(subcraft_app::Command::SetStrokeOptions(options)) {
        state.set_status(e.to_string(), now);
    }
}

/// Brush / eraser strokes and polygon fill at the cursor's texture UV.
fn paint_input(state: &mut UiState, ui: &Ui, response: &egui::Response, image: Rect, resolution: u32) {
    let typing = ui.ctx().egui_wants_keyboard_input();
    let (pointer, pressed, down, alt, shift) = ui.input(|i| {
        (i.pointer.interact_pos(), i.pointer.primary_pressed(), i.pointer.primary_down(), i.modifiers.alt, i.modifiers.shift)
    });
    let to_uv = |p: Pos2| Vec2::new((p.x - image.left()) / image.width(), (p.y - image.top()) / image.height());
    let texels_per_px = resolution as f32 / image.width().max(1.0);
    let now = ui.input(|i| i.time);
    if state.uv.painting {
        if !down {
            state.session.end_stroke();
            state.uv.painting = false;
        } else if let Some(p) = pointer
            && response.drag_delta() != egui::Vec2::ZERO
        {
            let uv = to_uv(p);
            let _ = state.session.stroke_to_uv(uv, false, texels_per_px);
            state.uv.last_point = Some(uv);
        }
        return;
    }
    if !pressed || !response.hovered() || alt || typing || state.painting || !crate::brush_bar::active(state.tool) {
        return;
    }
    let Some(p) = pointer else { return };
    let uv = to_uv(p);
    if state.tool == Tool::PolygonFill {
        state.session.brush.mode = StrokeMode::Paint;
        match state.session.polygon_fill_uv(uv, state.fill_mode) {
            Ok(tiles) if tiles.is_empty() => state.set_status("Polygon fill: click inside a UV island of this texture set", now),
            Ok(_) => {}
            Err(e) => state.set_status(format!("Polygon fill: {e}"), now),
        }
        return;
    }
    state.session.brush.mode = if state.tool == Tool::Eraser { StrokeMode::Erase } else { StrokeMode::Paint };
    match state.session.begin_stroke() {
        Ok(()) => {
            state.uv.painting = true;
            if let Some(from) = state.uv.last_point.filter(|_| shift) {
                let _ = state.session.stroke_to_uv(from, true, texels_per_px);
                let _ = state.session.stroke_to_uv(uv, true, texels_per_px);
            } else {
                let _ = state.session.stroke_to_uv(uv, false, texels_per_px);
            }
            state.uv.last_point = Some(uv);
        }
        Err(e) => state.set_status(format!("Can't paint here: {e} (select a paint layer)"), now),
    }
}
