//! 2D view of the selected texture set's composite with its UV wireframe.

use egui::{Color32, Pos2, Rect, Sense, Stroke, Ui};
use subcraft_core::ChannelKind;
use subcraft_mesh::topology;
use subcraft_render::wgpu_api as wgpu;

use crate::state::UiState;

const VIEW_CHANNELS: [ChannelKind; 3] = [ChannelKind::BaseColor, ChannelKind::Normal, ChannelKind::Emissive];

pub fn show(state: &mut UiState, ui: &mut Ui) {
    let ts = state.session.selected_texture_set();
    let slot = ts.material_slot;
    let available: Vec<ChannelKind> = VIEW_CHANNELS.iter().copied().filter(|k| ts.has_channel(*k)).collect();
    ui.horizontal(|ui| {
        ui.label("Channel");
        for k in &available {
            ui.selectable_value(&mut state.uv.channel, *k, k.display_name());
        }
        if ui.button("Fit").clicked() {
            state.uv.zoom = 1.0;
            state.uv.offset = egui::Vec2::ZERO;
        }
    });
    if !available.contains(&state.uv.channel) {
        state.uv.channel = ChannelKind::BaseColor;
    }
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
    painter.rect_filled(rect, 0.0, Color32::from_rgb(24, 25, 27));

    state.sync_gpu();
    if let (Some(rs), Some(renderer)) = (&state.render_state, &mut state.renderer)
        && let Some(preview) = renderer.channel_preview(&rs.device, &rs.queue, slot, state.uv.channel)
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
    painter.rect_stroke(img_rect, 0.0, Stroke::new(1.0, Color32::from_gray(90)), egui::StrokeKind::Outside);

    let mesh = state.session.scene().mesh.clone();
    let edges = state.uv_edges.entry(slot).or_insert_with(|| topology::uv_edges(&mesh, mesh.material_triangles(slot)));
    let to = |p: glam::Vec2| img_rect.min + egui::vec2(p.x * side, p.y * side);
    let stroke = Stroke::new(0.6, Color32::from_rgba_unmultiplied(255, 255, 255, 70));
    for [a, b] in edges.iter() {
        painter.line_segment([to(*a), to(*b)], stroke);
    }
}
