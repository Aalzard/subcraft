//! 2D view: the selected texture set's channels or baked mesh maps, with its UV wireframe.

use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Ui};
use subcraft_core::{ChannelKind, MeshMapKind};
use subcraft_mesh::topology;
use subcraft_render::{PreviewSource, wgpu_api as wgpu};

use crate::state::UiState;

fn label(s: PreviewSource) -> String {
    match s {
        PreviewSource::Channel(k) => k.display_name().to_string(),
        PreviewSource::MeshMap(k) => format!("Baked {}", k.display_name()),
    }
}

pub fn show(state: &mut UiState, ui: &mut Ui) {
    let ts = state.session.selected_texture_set();
    let (ts_id, slot) = (ts.id, ts.material_slot);
    let mut sources: Vec<PreviewSource> = ts.channel_kinds().map(PreviewSource::Channel).collect();
    if let Some(maps) = state.session.mesh_maps(ts_id) {
        sources.extend(MeshMapKind::ALL.iter().filter(|k| maps.get(**k).is_some()).map(|k| PreviewSource::MeshMap(*k)));
    }
    if !sources.contains(&state.uv.source) {
        state.uv.source = PreviewSource::Channel(ChannelKind::BaseColor);
    }
    egui::Frame::new().fill(crate::theme::BG_1).inner_margin(egui::Margin::symmetric(crate::theme::PAD, 6)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Show").color(crate::theme::TEXT_DIM));
            egui::ComboBox::from_id_salt("uv_source").selected_text(label(state.uv.source)).width(200.0).show_ui(ui, |ui| {
                for s in &sources {
                    ui.selectable_value(&mut state.uv.source, *s, label(*s));
                }
            });
            ui.add_space(8.0);
            if crate::widgets::icon_toggle(ui, crate::icons::GRID, state.uv_wireframe, "Show UV lines").clicked() {
                state.uv_wireframe = !state.uv_wireframe;
            }
            if crate::widgets::icon_button(ui, crate::icons::FIT, "Fit to view").clicked() {
                state.uv.zoom = 1.0;
                state.uv.offset = egui::Vec2::ZERO;
            }
        });
    });
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            state.uv.zoom = (state.uv.zoom * (scroll * 0.002).exp()).clamp(0.2, 20.0);
        }
    }
    if response.dragged() {
        state.uv.offset += response.drag_delta();
    }
    let side = rect.width().min(rect.height()) * 0.95 * state.uv.zoom;
    let img_rect = Rect::from_center_size(rect.center() + state.uv.offset, egui::vec2(side, side));
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
}
