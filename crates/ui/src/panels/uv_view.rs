//! 2D view: the selected texture set's channels or baked mesh maps, with its UV wireframe.
//! Every paint tool works here in UV space on the selected layer (or its mask), like in the 3D
//! view: brush, eraser, smudge, clone, polygon fill and the eyedropper, with the stencil,
//! symmetry, Shift lines and Ctrl brush gestures.

use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Ui};
use glam::Vec2;
use subcraft_core::{ChannelKind, MeshMapKind};
use subcraft_mesh::topology;
use subcraft_paint::StrokeMode;
use subcraft_render::{PreviewSource, wgpu_api as wgpu};

use crate::state::{Tool, UiState};

/// The export crate's name for what the 2D view shows.
pub fn export_view(s: PreviewSource) -> subcraft_export::View2d {
    match s {
        PreviewSource::Channel(k) => subcraft_export::View2d::Channel(k),
        PreviewSource::MeshMap(k) => subcraft_export::View2d::MeshMap(k),
    }
}

pub fn label(s: PreviewSource) -> String {
    match s {
        PreviewSource::Channel(k) => k.display_name().to_string(),
        PreviewSource::MeshMap(k) => format!("Baked {}", k.display_name()),
    }
}

/// Where the texture is drawn in the 2D view: the upright square `rect`, turned by `rotation`
/// radians (clockwise on screen) about its centre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub rect: Rect,
    pub rotation: f32,
}

impl Frame {
    /// Width of the texture on screen, in points.
    pub fn side(&self) -> f32 {
        self.rect.width()
    }

    fn turn(&self, p: Pos2, angle: f32) -> Pos2 {
        if angle == 0.0 {
            return p;
        }
        let (s, c) = angle.sin_cos();
        let d = p - self.rect.center();
        self.rect.center() + egui::vec2(d.x * c - d.y * s, d.x * s + d.y * c)
    }

    /// Screen position of texture position `uv`.
    pub fn to_px(&self, uv: Vec2) -> Pos2 {
        self.turn(self.rect.min + egui::vec2(uv.x, uv.y) * self.side(), self.rotation)
    }

    /// Texture position under screen position `p`.
    pub fn to_uv(&self, p: Pos2) -> Vec2 {
        let q = self.turn(p, -self.rotation);
        Vec2::new((q.x - self.rect.left()) / self.rect.width(), (q.y - self.rect.top()) / self.rect.height())
    }

    /// Whether `p` is over the 0..1 texture square.
    pub fn contains(&self, p: Pos2) -> bool {
        self.rect.contains(self.turn(p, -self.rotation))
    }

    /// The texture square's corners on screen: UV (0,0), (1,0), (1,1), (0,1).
    pub fn corners(&self) -> [Pos2; 4] {
        [Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(1.0, 1.0), Vec2::new(0.0, 1.0)].map(|uv| self.to_px(uv))
    }

    /// The box a drag from `a` to `b` selects: aligned to the texture, so it is exactly the UV
    /// box the tools use, whatever the rotation. Its corners on screen.
    pub fn drag_box(&self, a: Pos2, b: Pos2) -> [Pos2; 4] {
        let (p, q) = (self.to_uv(a), self.to_uv(b));
        [p, Vec2::new(q.x, p.y), q, Vec2::new(p.x, q.y)].map(|uv| self.to_px(uv))
    }
}

/// Turn the view from `rotation` to `next` keeping the texture point at `pivot` (a position
/// relative to the view's centre) where it is. Returns the new rotation and pan offset.
pub fn rotate_about(rotation: f32, offset: egui::Vec2, pivot: egui::Vec2, next: f32) -> (f32, egui::Vec2) {
    let (s, c) = (next - rotation).sin_cos();
    let d = offset - pivot;
    (next, pivot + egui::vec2(d.x * c - d.y * s, d.x * s + d.y * c))
}

/// `angle` snapped to the nearest quarter turn (Shift while rotating).
pub fn snap_quarter(angle: f32) -> f32 {
    let quarter = std::f32::consts::FRAC_PI_2;
    (angle / quarter).round() * quarter
}

/// A texture over the frame: an upright image, or a turned quad.
fn draw_texture(painter: &egui::Painter, frame: Frame, texture: egui::TextureId, tint: Color32) {
    if frame.rotation == 0.0 {
        painter.image(texture, frame.rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), tint);
        return;
    }
    let mut mesh = egui::Mesh::with_texture(texture);
    let uvs = [egui::pos2(0.0, 0.0), egui::pos2(1.0, 0.0), egui::pos2(1.0, 1.0), egui::pos2(0.0, 1.0)];
    for (pos, uv) in frame.corners().into_iter().zip(uvs) {
        mesh.vertices.push(egui::epaint::Vertex { pos, uv, color: tint });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

/// Zoom limits of the 2D view.
const ZOOM_RANGE: std::ops::RangeInclusive<f32> = 0.2..=20.0;

/// Zoom by `factor` keeping the texture point at `anchor` (a position relative to the view's
/// centre) where it is. Returns the new zoom and pan offset.
pub fn zoom_about(zoom: f32, offset: egui::Vec2, anchor: egui::Vec2, factor: f32) -> (f32, egui::Vec2) {
    let next = (zoom * factor).clamp(*ZOOM_RANGE.start(), *ZOOM_RANGE.end());
    let ratio = next / zoom;
    (next, anchor - (anchor - offset) * ratio)
}

/// What the 2D view shows when the 3D view's display changes with C / Shift+C / M / B / Shift+B:
/// the mesh map, else the channel shown alone, else (the lit material, which the 2D view cannot
/// draw) the base colour.
pub fn source_for(mesh_map: Option<MeshMapKind>, channel: Option<ChannelKind>) -> PreviewSource {
    match (mesh_map, channel) {
        (Some(map), _) => PreviewSource::MeshMap(map),
        (None, Some(kind)) => PreviewSource::Channel(kind),
        (None, None) => PreviewSource::Channel(ChannelKind::BaseColor),
    }
}

/// The lit material image's longest side in texels (larger texture sets are sampled).
const MATERIAL_SIDE: u32 = 1024;

/// The texture set's lit material in UV space (Painter's 2D view "Material" display): each
/// texel of the composite shaded flat with [`subcraft_materials::uv_material::shade`], encoded
/// sRGB for display. Missing channels take neutral values.
pub fn material_image(composite: &subcraft_compositor::Composite) -> egui::ColorImage {
    use subcraft_materials::uv_material::{UvTexel, shade};
    let res = composite.resolution.max(1);
    let step = res.div_ceil(MATERIAL_SIDE).max(1);
    let side = res.div_ceil(step);
    let ch = |k: ChannelKind| composite.channel(k).filter(|i| i.width() == res && i.height() == res);
    let (base, normal, rough, metal, ao) =
        (ch(ChannelKind::BaseColor), ch(ChannelKind::Normal), ch(ChannelKind::Roughness), ch(ChannelKind::Metallic), ch(ChannelKind::AmbientOcclusion));
    let d = UvTexel::default();
    let mut rgba = Vec::with_capacity((side * side * 4) as usize);
    for y in 0..side {
        for x in 0..side {
            let (px, py) = ((x * step).min(res - 1), (y * step).min(res - 1));
            let get = |img: Option<&subcraft_image::TiledImage>| img.map(|i| i.get(px, py));
            let t = UvTexel {
                base: get(base).map_or(d.base, |v| [v[0], v[1], v[2]]),
                normal: get(normal).map_or(d.normal, |v| [v[0], v[1], v[2]]),
                roughness: get(rough).map_or(d.roughness, |v| v[0]),
                metallic: get(metal).map_or(d.metallic, |v| v[0]),
                ao: get(ao).map_or(d.ao, |v| v[0]),
            };
            let c = subcraft_core::color::linear3_to_srgb(shade(t).map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }));
            rgba.extend(c.map(|v| (v * 255.0).round() as u8));
            rgba.push(255);
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([side as usize, side as usize], &rgba)
}

/// The lit material texture for the 2D view, rebuilt when the document or the composite
/// changed (at most every quarter second while edits stream in).
fn material_texture(state: &mut UiState, ctx: &egui::Context, ts: subcraft_core::TextureSetId, now: f64) -> Option<egui::TextureId> {
    let key = (ts, state.session.revision(), state.session.composite_generation());
    let stale = state.gpu_changed || state.uv.material_image.as_ref().is_none_or(|m| m.key != key);
    let due = state.uv.material_image.as_ref().is_none_or(|m| m.key.0 != ts || now - m.built >= 0.25);
    if stale && due {
        if let Some(c) = state.session.composite(ts) {
            let image = material_image(c);
            match &mut state.uv.material_image {
                Some(m) => {
                    m.texture.set(image, egui::TextureOptions::LINEAR);
                    m.key = key;
                    m.built = now;
                }
                None => {
                    let texture = ctx.load_texture("uv_material", image, egui::TextureOptions::LINEAR);
                    state.uv.material_image = Some(crate::state::UvMaterialImage { key, built: now, texture });
                }
            }
        }
    } else if stale {
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
    state.uv.material_image.as_ref().filter(|m| m.key.0 == ts).map(|m| m.texture.id())
}

/// Axis information in the bottom-right corner: the U and V directions of the texture as drawn
/// (U to the right, V downwards).
fn axis_information(painter: &egui::Painter, rect: Rect) {
    let origin = rect.right_bottom() - egui::vec2(46.0, 46.0);
    let font = crate::theme::font(10.5);
    for (dir, name, color) in [(egui::vec2(1.0, 0.0), "U", Color32::from_rgb(226, 92, 92)), (egui::vec2(0.0, 1.0), "V", Color32::from_rgb(120, 200, 110))] {
        let tip = origin + dir * 26.0;
        let side = egui::vec2(-dir.y, dir.x);
        for (width, c) in [(3.0, Color32::from_black_alpha(150)), (1.5, color)] {
            painter.line_segment([origin, tip], Stroke::new(width, c));
            painter.line_segment([tip, tip - dir * 6.0 + side * 3.5], Stroke::new(width, c));
            painter.line_segment([tip, tip - dir * 6.0 - side * 3.5], Stroke::new(width, c));
        }
        painter.text(tip + dir * 8.0, egui::Align2::CENTER_CENTER, name, font.clone(), color);
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
    egui::Frame::new().fill(crate::theme::BG_1()).inner_margin(egui::Margin::symmetric(crate::theme::PAD, 6)).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new("Show").color(crate::theme::TEXT_DIM()));
            let shown = if state.uv.material { "Material".to_string() } else { label(state.uv.source) };
            egui::ComboBox::from_id_salt("uv_source").selected_text(shown).width(200.0).show_ui(ui, |ui| {
                // Painter's lit material, drawn flat in UV space.
                if ui.selectable_label(state.uv.material, "Material").on_hover_text("The material lit by a studio light, flat in UV space").clicked() {
                    state.uv.material = true;
                }
                for s in &sources {
                    if ui.selectable_label(!state.uv.material && state.uv.source == *s, label(*s)).clicked() {
                        state.uv.source = *s;
                        state.uv.material = false;
                    }
                }
            });
            if !state.uv.material
                && let PreviewSource::Channel(kind) = state.uv.source
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
            let lines = crate::widgets::icon_toggle(ui, crate::icons::GRID, state.uv_wireframe, "UV wireframe · click to show/hide · right-click for color and opacity");
            if lines.clicked() {
                state.uv_wireframe = !state.uv_wireframe;
            }
            egui::Popup::context_menu(&lines).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                let prefs = &mut state.recovery.prefs.viewport;
                ui.label(RichText::new("UV wireframe").color(crate::theme::TEXT_DIM()));
                ui.checkbox(&mut state.uv_wireframe, "Show UV wireframe");
                ui.horizontal(|ui| {
                    ui.label("Color");
                    ui.color_edit_button_rgb(&mut prefs.wire_color);
                });
                ui.add(egui::Slider::new(&mut prefs.wire_opacity, 0.05..=1.0).text("Opacity"));
                ui.label(RichText::new("Shared with the 3D view's mesh wireframe (Display settings)").small().color(crate::theme::TEXT_FAINT()));
            });
            if crate::widgets::icon_button(ui, crate::icons::FIT, "Fit to view").clicked() {
                state.uv.rotation = 0.0;
                state.uv.zoom = 1.0;
                state.uv.offset = egui::Vec2::ZERO;
            }
        });
        ui.horizontal_wrapped(|ui| brush_controls(state, ui));
        if state.picking_parts {
            ui.horizontal_wrapped(|ui| crate::panels::parts::mask_bar(ui, state));
        }
    });
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    state.hovered_2d = response.hovered();
    let (alt, mods, delta) = ui.input(|i| (i.modifiers.alt, i.modifiers, i.pointer.delta()));
    // The wheel zooms on the texture point under the cursor; Alt+RMB drag zooms on the point
    // where the drag began (Painter's 2D view zoom).
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 && let Some(cursor) = response.hover_pos() {
            (state.uv.zoom, state.uv.offset) = zoom_about(state.uv.zoom, state.uv.offset, cursor - rect.center(), state.recovery.prefs.camera_speed.wheel_magnify_2d(scroll));
        }
    }
    let zoom_drag = alt && !mods.command && !mods.ctrl && response.dragged_by(egui::PointerButton::Secondary);
    if zoom_drag {
        let anchor = ui.input(|i| i.pointer.press_origin()).unwrap_or(rect.center());
        (state.uv.zoom, state.uv.offset) = zoom_about(state.uv.zoom, state.uv.offset, anchor - rect.center(), ((delta.x - delta.y) * 0.01).exp());
    }
    let typing = ui.ctx().egui_wants_keyboard_input();
    // Hold S and drag to place the stencil over this view, hold N to paint without it: the same
    // keys as the 3D view.
    let stencil_edit = crate::viewport::stencil_input(state, ui, &response, rect, mods, delta);
    // Ctrl+RMB drag: size / hardness; Ctrl+LMB drag: flow / angle (as in the 3D view).
    let lmb_adjust = crate::viewport::brush_gestures(state, &response, mods, delta, !stencil_edit && !typing && !state.uv.painting);
    let paints = painting_tool && !alt && !stencil_edit;
    // Tools that use a click in the view: a drag with them must not pan.
    let picking = state.picking_parts && state.session.selected_layer().is_some();
    let clicks = paints || picking || matches!(state.tool, Tool::Pen | Tool::Eyedropper);
    // Right-click (no drag): the Properties popup at the cursor.
    if response.secondary_clicked() && !stencil_edit && crate::view_props::opens(state, mods) && let Some(p) = response.interact_pointer_pos() {
        crate::view_props::open(state, p, crate::view_props::View::View2d);
    }
    let ctrl = mods.command || mods.ctrl;
    let lmb_nav = !stencil_edit && response.dragged_by(egui::PointerButton::Primary) && !state.uv.painting && !lmb_adjust;
    if lmb_nav && alt && !ctrl {
        // Alt+LMB turns the texture about the point where the drag began; Shift snaps it to
        // quarter turns (Painter's 2D view).
        let pivot = ui.input(|i| i.pointer.press_origin()).unwrap_or(rect.center());
        state.uv.rotation_raw += delta.x * 0.01;
        let next = if mods.shift { snap_quarter(state.uv.rotation_raw) } else { state.uv.rotation_raw };
        (state.uv.rotation, state.uv.offset) = rotate_about(state.uv.rotation, state.uv.offset, pivot - rect.center(), next);
    } else {
        state.uv.rotation_raw = state.uv.rotation;
        // MMB and Ctrl+Alt+LMB pan; so does a plain LMB drag with a tool that does not use it.
        if !stencil_edit && (response.dragged_by(egui::PointerButton::Middle) || (lmb_nav && ((alt && ctrl) || !clicks))) {
            state.uv.offset += response.drag_delta();
        }
    }
    if let Some((center, extent)) = state.uv.frame.take() {
        let (zoom, offset) = uv_frame(rect.width().min(rect.height()) * 0.95, center, extent);
        state.uv.zoom = zoom;
        // The offset is measured on screen: turn it with the texture.
        state.uv.offset = rotate_about(0.0, offset, egui::Vec2::ZERO, state.uv.rotation).1;
    }
    let side = rect.width().min(rect.height()) * 0.95 * state.uv.zoom;
    let img_rect = Rect::from_center_size(rect.center() + state.uv.offset, egui::vec2(side, side));
    let viewport = [rect.width().round().max(1.0) as u32, rect.height().round().max(1.0) as u32];
    let frame = Frame { rect: img_rect, rotation: state.uv.rotation };
    let view = subcraft_paint::UvView {
        origin: Vec2::new(img_rect.min.x - rect.min.x, img_rect.min.y - rect.min.y),
        side,
        viewport,
        rotation: frame.rotation,
    };
    crate::pen::uv_input(state, ui, &response, frame);
    // Picking parts for a geometry mask owns the clicks; otherwise the ID eyedropper, then tools.
    let id_eyedropper = !picking && id_pick_input(state, ui, &response, frame, ts_id);
    if picking {
        // A layer without a geometry mask gets one by UV island when it is picked here.
        if response.hovered() {
            state.session.set_new_mask_kind(subcraft_core::PartKind::UvIsland);
        }
        parts_input(state, ui, &response, frame);
    } else if !id_eyedropper && !stencil_edit {
        paint_input(state, ui, &response, frame, resolution, ts_id, view, lmb_adjust);
    }
    let painter = ui.painter_at(rect);
    let filter = if state.uv.source == PreviewSource::Channel(ChannelKind::BaseColor)
        && state.session.selected_texture_set().albedo_filter == subcraft_image::TextureFilter::Nearest {
        wgpu::FilterMode::Nearest
    } else { wgpu::FilterMode::Linear };
    painter.rect_filled(rect, 0.0, crate::theme::BG_0());

    state.sync_gpu();
    if state.uv.material {
        let now = ui.input(|i| i.time);
        if let Some(id) = material_texture(state, ui.ctx(), ts_id, now) {
            draw_texture(&painter, frame, id, Color32::WHITE);
        }
    } else if let (Some(rs), Some(renderer)) = (&state.render_state, &mut state.renderer)
        && let Some(preview) = renderer.preview_display(&rs.device, &rs.queue, slot, state.uv.source, &display_2d(&state.recovery.prefs.viewport))
    {
        let view = &preview.display_view;
        let mut r = rs.renderer.write();
        let id = match state.uv.texture {
            Some(id) => {
                r.update_egui_texture_from_wgpu_texture(&rs.device, view, filter, id);
                id
            }
            None => {
                let id = r.register_native_texture(&rs.device, view, filter);
                state.uv.texture = Some(id);
                id
            }
        };
        draw_texture(&painter, frame, id, Color32::WHITE);
    }
    if frame.rotation == 0.0 {
        painter.rect_stroke(img_rect, 0.0, Stroke::new(1.0, crate::theme::BORDER_STRONG()), egui::StrokeKind::Outside);
    } else {
        painter.add(egui::Shape::closed_line(frame.corners().to_vec(), Stroke::new(1.0, crate::theme::BORDER_STRONG())));
    }
    axis_information(&painter, rect);
    // The isolated component's name, as in the 3D view.
    if let Some(name) = shown_component(&state.recovery.prefs.viewport).overlay() {
        crate::viewport::pill(&painter, rect.left_top() + egui::vec2(10.0, 10.0), egui::Align2::LEFT_TOP, crate::i18n::tr(name), crate::theme::OVERLAY_TEXT);
    }
    quick_mask_overlay(state, ui, &painter, frame);
    parts_overlay(state, ui, &painter, frame, picking, response.hover_pos());
    let fill_hover = state.tool == Tool::PolygonFill && paints && !picking && state.uv.fill_box.is_none();
    fill_hover_overlay(state, &painter, frame, fill_hover.then(|| response.hover_pos()).flatten());

    if state.uv_wireframe {
        let mesh = state.session.scene().mesh.clone();
        let edges = state.uv_edges.entry(slot).or_insert_with(|| topology::uv_edges(&mesh, mesh.material_triangles(slot)));
        let to = |p: glam::Vec2| frame.to_px(p);
        let stroke = Stroke::new(0.6, wire_color(&state.recovery.prefs.viewport));
        for [a, b] in edges.iter() {
            painter.line_segment([to(*a), to(*b)], stroke);
        }
    }
    if state.tool == Tool::Pen && let Some(path) = state.pen.preview(&state.session) {
        crate::pen::draw(&painter, path, state.pen.selected_anchor(), |uv| Some(frame.to_px(uv)));
    }
    for start in [state.uv.fill_box, state.uv.parts_box] {
        if let (Some(a), Some(b)) = (start, response.hover_pos().or(ui.input(|i| i.pointer.latest_pos())))
            && crate::view_props::is_box_drag(a, b)
        {
            if frame.rotation == 0.0 {
                crate::view_props::draw_fill_box(&painter, Rect::from_two_pos(a, b), false);
            } else {
                // Turned view: the box stays aligned to the texture, as the selection is.
                let accent = crate::theme::ACCENT();
                let fill = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 28);
                painter.add(egui::Shape::convex_polygon(frame.drag_box(a, b).to_vec(), fill, Stroke::new(1.0, accent)));
            }
        }
    }
    let to_px = |uv: Vec2| frame.to_px(uv);
    if state.tool == Tool::Clone && let Some(source) = state.session.clone_source_uv() {
        let marker = to_px(source);
        let cross = Color32::from_rgb(120, 220, 255);
        for (w, c) in [(3.0, Color32::from_black_alpha(160)), (1.5, cross)] {
            painter.line_segment([marker - egui::vec2(7.0, 0.0), marker + egui::vec2(7.0, 0.0)], Stroke::new(w, c));
            painter.line_segment([marker - egui::vec2(0.0, 7.0), marker + egui::vec2(0.0, 7.0)], Stroke::new(w, c));
        }
        // While an aligned offset exists the copy comes from beside the cursor: show where.
        if let (Some(offset), Some(p)) = (state.session.clone_offset_uv(), response.hover_pos()) {
            painter.circle_stroke(frame.to_px(frame.to_uv(p) + offset), 6.0, Stroke::new(1.5, cross));
        }
    }
    crate::viewport::draw_stencil(state, ui, &painter, rect, viewport);
    if let Some(p) = response.hover_pos() {
        if matches!(state.tool, Tool::Brush | Tool::Eraser | Tool::Clone | Tool::Smudge) && paints {
            let mesh = &state.session.scene().mesh;
            let density = *state.uv_density.entry(slot).or_insert_with(|| subcraft_paint::stroke::uv_per_unit(mesh, slot));
            let r_px = state.session.brush_radius_uv_in(density, Some(view)) * side;
            let color = if state.tool == Tool::Eraser { Color32::from_rgb(120, 190, 255) } else { crate::theme::ACCENT() };
            painter.circle_stroke(p, r_px, Stroke::new(1.5, Color32::from_black_alpha(160)));
            painter.circle_stroke(p, r_px, Stroke::new(1.0, color));
            // Shift: dotted preview of the straight line from the last stroke point.
            if mods.shift && !state.uv.painting && let Some(from) = state.uv.last_point {
                let to = line_end(to_px(from), p, mods.command || mods.ctrl);
                let dash = Stroke::new(1.2, Color32::from_white_alpha(200));
                painter.extend(egui::Shape::dashed_line(&[to_px(from), to], dash, 5.0, 4.0));
            }
        } else if state.tool == Tool::PolygonFill && paints {
            painter.circle_filled(p, 3.0, crate::theme::ACCENT());
        }
        if (clicks && !alt) && !stencil_edit {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
    }
    let hint = if picking { "Geometry mask: click an island to show / hide the layer on it · drag a box to add · Ctrl+drag to remove · Esc done" } else { hint(state.tool, paints) };
    painter.text(rect.left_bottom() + egui::vec2(10.0, -10.0), egui::Align2::LEFT_BOTTOM, hint, crate::theme::font(11.5), crate::theme::TEXT_FAINT());
    if state.uv.painting {
        ui.ctx().request_repaint();
    }
}

/// Geometry mask editing in the 2D view (Properties → Parts → pick, or the layer row's geometry
/// mask icon): a click shows / hides the layer on the part under the cursor, a drag adds every
/// part the box touches, Ctrl+drag removes them.
fn parts_input(state: &mut UiState, ui: &Ui, response: &egui::Response, image: Frame) {
    let (pointer, pressed, down, mods) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_pressed(), i.pointer.primary_down(), i.modifiers));
    let to_uv = |p: Pos2| image.to_uv(p);
    let now = ui.input(|i| i.time);
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.uv.parts_box = None;
        state.picking_parts = false;
        return;
    }
    let Some(start) = state.uv.parts_box else {
        if pressed && response.hovered() && !mods.alt && !state.view_props_swallow && let Some(p) = pointer {
            state.uv.parts_box = Some(p);
        }
        return;
    };
    if down {
        return;
    }
    state.uv.parts_box = None;
    let Some(layer) = state.session.selected_layer().map(|l| l.id) else { return };
    let end = pointer.unwrap_or(start);
    let result = if crate::view_props::is_box_drag(start, end) {
        let parts = state.session.parts_in_uv_box(to_uv(start), to_uv(end));
        if parts.is_empty() { Err("the box touched no UV island of this texture set".to_string()) }
        else { state.session.set_layer_parts(layer, &parts, !(mods.command || mods.ctrl)).map_err(|e| e.to_string()) }
    } else {
        match state.session.part_at_uv(to_uv(start)) {
            Some(part) => state.session.toggle_layer_part(layer, part).map_err(|e| e.to_string()),
            None => Err("click inside a UV island of this texture set".to_string()),
        }
    };
    if let Err(e) = result {
        state.set_status(format!("Parts: {e}"), now);
    }
}

/// Cached geometry of the 2D view's geometry mask overlay.
#[derive(Default)]
pub struct PartsOverlay {
    excluded_key: Option<(subcraft_core::TextureSetId, subcraft_core::LayerId, subcraft_doc::PartMask)>,
    excluded: Vec<[Vec2; 3]>,
    hover_key: Option<(subcraft_core::TextureSetId, subcraft_core::PartKind, u32)>,
    hover: Vec<[Vec2; 3]>,
    hatch: Option<egui::TextureHandle>,
    fill_key: Option<(subcraft_core::TextureSetId, subcraft_paint::PolygonFillMode, u32)>,
    fill: Vec<[Vec2; 3]>,
    /// The part whose row is under the cursor in the Properties list (set there every frame).
    pub list_hover: Option<u32>,
}

/// Size of one hatch pattern repeat, in points.
const HATCH: f32 = 8.0;

/// Grey with dark diagonal lines: Painter's look for geometry a geometry mask leaves out.
fn hatch_image() -> egui::ColorImage {
    let n = HATCH as usize;
    let mut rgba = Vec::with_capacity(n * n * 4);
    for y in 0..n {
        for x in 0..n {
            let px: [u8; 4] = if (x + y) % n < 2 { [30, 30, 30, 210] } else { [120, 120, 120, 150] };
            rgba.extend_from_slice(&px);
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([n, n], &rgba)
}

/// UV triangles as one screen mesh: a flat `color`, or the hatch texture repeated in screen space.
fn triangles_mesh(tris: &[[Vec2; 3]], to_px: impl Fn(Vec2) -> Pos2, clip: Rect, color: Color32, texture: Option<egui::TextureId>) -> egui::Shape {
    let mut mesh = egui::Mesh::default();
    if let Some(t) = texture {
        mesh.texture_id = t;
    }
    for tri in tris {
        let p = [to_px(tri[0]), to_px(tri[1]), to_px(tri[2])];
        if !Rect::from_points(&p).intersects(clip) {
            continue;
        }
        let i = mesh.vertices.len() as u32;
        for q in p {
            let uv = if texture.is_some() { egui::pos2(q.x / HATCH, q.y / HATCH) } else { egui::epaint::WHITE_UV };
            mesh.vertices.push(egui::epaint::Vertex { pos: q, uv, color });
        }
        mesh.add_triangle(i, i + 1, i + 2);
    }
    egui::Shape::mesh(mesh)
}

/// Polygon Fill's preview: the triangle, polygon, mesh, UV island or object under the cursor (what
/// a click would fill in the current mode) is tinted.
fn fill_hover_overlay(state: &mut UiState, painter: &egui::Painter, image: Frame, pointer: Option<Pos2>) {
    let triangle = pointer
        .filter(|p| image.contains(*p))
        .and_then(|p| state.session.triangle_at_uv(image.to_uv(p)));
    let key = triangle.map(|t| (state.session.selected_texture_set().id, state.fill_mode, t));
    if key != state.uv_parts.fill_key {
        // In every mode but Triangle the element is the same for most cursor moves: skip the
        // rebuild while the cursor stays on a triangle that is already tinted.
        state.uv_parts.fill = triangle.map_or_else(Vec::new, |t| state.session.polygon_fill_preview_uv(t, state.fill_mode));
        state.uv_parts.fill_key = key;
    }
    if !state.uv_parts.fill.is_empty() {
        let to_px = |uv: Vec2| image.to_px(uv);
        let tint = Color32::from_rgba_unmultiplied(crate::theme::ACCENT().r(), crate::theme::ACCENT().g(), crate::theme::ACCENT().b(), 80);
        painter.add(triangles_mesh(&state.uv_parts.fill, to_px, painter.clip_rect(), tint, None));
    }
}

/// The geometry mask over the 2D view, while it is edited or Alt+H hides what it leaves out:
/// excluded geometry grey with diagonal lines; while editing, the part under the cursor (or under
/// the cursor in the Properties list) is highlighted.
fn parts_overlay(state: &mut UiState, ui: &Ui, painter: &egui::Painter, image: Frame, picking: bool, pointer: Option<Pos2>) {
    // Set by the Properties part list every frame; the 3D view reads it too.
    let list_hover = state.uv_parts.list_hover;
    if !picking && !state.session.hide_excluded() {
        return;
    }
    let ts = state.session.selected_texture_set().id;
    let key = state.session.selected_layer().and_then(|l| l.parts.clone().filter(|p| p.enabled).map(|p| (ts, l.id, p)));
    if key != state.uv_parts.excluded_key {
        state.uv_parts.excluded = if key.is_some() { state.session.excluded_uv_triangles() } else { Vec::new() };
        state.uv_parts.excluded_key = key;
    }
    let to_px = |uv: Vec2| image.to_px(uv);
    let clip = painter.clip_rect();
    if !state.uv_parts.excluded.is_empty() {
        let options = egui::TextureOptions { wrap_mode: egui::TextureWrapMode::Repeat, ..egui::TextureOptions::NEAREST };
        let hatch = state.uv_parts.hatch.get_or_insert_with(|| ui.ctx().load_texture("geometry-mask-hatch", hatch_image(), options)).id();
        painter.add(triangles_mesh(&state.uv_parts.excluded, to_px, clip, Color32::WHITE, Some(hatch)));
    }
    if !picking {
        return;
    }
    let under_cursor = pointer.filter(|p| image.contains(*p)).and_then(|p| state.session.part_at_uv(image.to_uv(p)));
    let part = list_hover.or(under_cursor);
    let hover_key = part.map(|p| (ts, state.session.mask_kind(), p));
    if hover_key != state.uv_parts.hover_key {
        state.uv_parts.hover = part.map_or_else(Vec::new, |p| state.session.part_uv_triangles(p));
        state.uv_parts.hover_key = hover_key;
    }
    if !state.uv_parts.hover.is_empty() {
        let tint = Color32::from_rgba_unmultiplied(crate::theme::ACCENT().r(), crate::theme::ACCENT().g(), crate::theme::ACCENT().b(), 70);
        painter.add(triangles_mesh(&state.uv_parts.hover, to_px, clip, tint, None));
    }
}

/// The quick mask over the 2D view: its protected areas tinted red (strongly while it is being
/// edited, lightly once applied).
fn quick_mask_overlay(state: &mut UiState, ui: &Ui, painter: &egui::Painter, image: Frame) {
    let Some(q) = state.session.quick_mask() else {
        state.quick_mask_tex = None;
        return;
    };
    let (revision, editing) = (q.revision, q.editing);
    // While editing, the Base Color texture itself carries the tint (see `UiState::sync_gpu`).
    if editing && state.uv.source == PreviewSource::Channel(ChannelKind::BaseColor) && state.renderer.is_some() {
        return;
    }
    if state.quick_mask_tex.as_ref().is_none_or(|(r, _)| *r != revision) {
        let img = &q.image;
        let n = img.width().min(512).max(1);
        let step = img.width() as f32 / n as f32;
        let mut rgba = Vec::with_capacity((n * n * 4) as usize);
        for y in 0..n {
            for x in 0..n {
                let v = img.get(((x as f32 + 0.5) * step) as u32, ((y as f32 + 0.5) * step) as u32)[0].clamp(0.0, 1.0);
                rgba.extend_from_slice(&[230, 40, 40, ((1.0 - v) * 255.0) as u8]);
            }
        }
        let tex = ui.ctx().load_texture("quick_mask", egui::ColorImage::from_rgba_unmultiplied([n as usize, n as usize], &rgba), egui::TextureOptions::LINEAR);
        state.quick_mask_tex = Some((revision, tex));
    }
    if let Some((_, tex)) = &state.quick_mask_tex {
        let tint = Color32::from_white_alpha(if editing { 128 } else { 50 });
        draw_texture(painter, image, tex.id(), tint);
    }
}

/// Colour of the UV lines: Display settings' mesh wireframe colour and opacity, which Painter
/// applies to both views.
pub fn wire_color(prefs: &subcraft_doc::DisplaySettings) -> Color32 {
    let [r, g, b] = prefs.wire_color.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
    Color32::from_rgba_unmultiplied(r, g, b, (prefs.wire_opacity.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// The bottom-left help line for the active tool.
pub fn hint(tool: Tool, paints: bool) -> &'static str {
    if !paints {
        return match tool {
            Tool::Eyedropper => "LMB pick color · Shift+LMB pick material · MMB pan · Wheel zoom",
            _ => "LMB / MMB pan · Alt+LMB rotate · RMB properties · Wheel zoom",
        };
    }
    match tool {
        Tool::PolygonFill => "LMB click fill · LMB drag box fill · RMB properties · Alt+LMB rotate · MMB pan · Wheel zoom",
        Tool::Clone => "Ctrl+click or V+click set source · LMB clone · Shift+click line · MMB pan · Wheel zoom",
        Tool::Smudge => "LMB smudge · Shift+click line · Ctrl+drag brush values · MMB pan · Wheel zoom",
        _ => "LMB paint · Shift+click line · Ctrl+drag brush values · hold S stencil · MMB pan · Wheel zoom",
    }
}

/// End of a Shift+click straight line in view pixels: Ctrl snaps it to 5 degree steps (Painter).
fn line_end(from: Pos2, to: Pos2, snap: bool) -> Pos2 {
    if !snap {
        return to;
    }
    let e = crate::nav::snap_line_end(Vec2::new(from.x, from.y), Vec2::new(to.x, to.y), 5.0);
    Pos2::new(e.x, e.y)
}

/// Tool icons and brush values, shared with the 3D view's top strip.
fn brush_controls(state: &mut UiState, ui: &mut Ui) {
    let mut settings = state.session.brush.settings;
    let mut options = state.session.brush.options;
    let (original, original_options) = (settings, options);
    let width = ui.available_width();
    crate::brush_bar::show(ui, &mut state.tool, &mut settings, &mut options, &mut state.recovery.prefs.viewport, width);
    if state.tool == Tool::PolygonFill {
        let destination = state.session.polygon_fill_destination();
        crate::brush_bar::fill_options(ui, &mut state.fill_mode, None, destination, "fill_mode_2d");
    }
    let now = ui.input(|i| i.time);
    if settings != original && let Err(e) = state.session.execute(subcraft_app::Command::SetBrushSettings(settings)) {
        state.set_status(e.to_string(), now);
    }
    if options != original_options && let Err(e) = state.session.execute(subcraft_app::Command::SetStrokeOptions(options)) {
        state.set_status(e.to_string(), now);
    }
}

/// The ID eyedroppers in the 2D view (an ID Color mask's, or the ID color editor's): a click picks
/// the ID map color at that texture UV. Returns whether an eyedropper is active (no painting then).
fn id_pick_input(state: &mut UiState, ui: &Ui, response: &egui::Response, image: Frame, ts: subcraft_core::TextureSetId) -> bool {
    let mask_pick = state.picking_id_color.filter(|(layer, _)| state.session.selected_texture_set().find(*layer).is_some());
    if mask_pick.is_none() && !state.picking_id_palette {
        return false;
    }
    if response.hovered() { ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair); }
    let (pointer, pressed, alt) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_pressed(), i.modifiers.alt));
    if !pressed || alt || !response.hovered() { return true; }
    let Some(p) = pointer else { return true };
    let uv = image.to_uv(p);
    let now = ui.input(|i| i.time);
    if !(0.0..=1.0).contains(&uv.x) || !(0.0..=1.0).contains(&uv.y) { return true; }
    if let Some((layer, effect)) = mask_pick {
        match state.session.pick_id_color_uv(layer, effect, uv) {
            Ok(Some(_)) => state.set_status("ID color added. Click more islands, Esc when done.", now),
            Ok(None) if !state.session.has_id_map(ts) => state.set_status("No ID map for this texture set: bake or import one.", now),
            Ok(None) => state.set_status("Nothing added: that ID color is already picked.", now),
            Err(e) => state.set_status(format!("ID Color Mask: {e}"), now),
        }
    } else {
        match state.session.id_map_color(ts, uv) {
            Some(color) => crate::panels::id_colors::picked(state, ts, color, now),
            None => state.set_status("No ID map for this texture set: bake or import one.", now),
        }
    }
    true
}

/// Every paint tool at the cursor's texture UV, as in the 3D view: brush, eraser, smudge and
/// clone strokes (Shift+click lines, Ctrl+Shift snapped), polygon fill clicks and boxes, the
/// eyedropper, and the clone source (Ctrl+click or V+click).
#[allow(clippy::too_many_arguments)]
fn paint_input(state: &mut UiState, ui: &Ui, response: &egui::Response, image: Frame, resolution: u32, ts_id: subcraft_core::TextureSetId, view: subcraft_paint::UvView, lmb_adjust: bool) {
    let typing = ui.ctx().egui_wants_keyboard_input();
    let (pointer, pressed, down, mods) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_pressed(), i.pointer.primary_down(), i.modifiers));
    let (alt, shift, ctrl) = (mods.alt, mods.shift, mods.command || mods.ctrl);
    let v_down = ui.input(|i| i.key_down(egui::Key::V)) && !typing && !mods.any();
    let pressed = pressed && !state.view_props_swallow;
    let to_uv = |p: Pos2| image.to_uv(p);
    let to_px = |uv: Vec2| image.to_px(uv);
    let texels_per_px = resolution as f32 / image.side().max(1.0);
    let now = ui.input(|i| i.time);
    // Pen pressure, as in the 3D view (a mouse reports none: 1).
    let (pressure, carried) = crate::viewport::pen_pressure(&ui.input(|i| i.events.clone()), state.pen_pressure);
    state.pen_pressure = carried;
    let pressure = state.recovery.prefs.viewport.pressure_curve.apply(pressure);
    // Polygon fill: a click fills the element under the cursor, a drag every element the UV box touches.
    if let Some(start) = state.uv.fill_box {
        if state.tool != Tool::PolygonFill || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            state.uv.fill_box = None;
        } else if !down {
            state.uv.fill_box = None;
            let end = pointer.unwrap_or(start);
            let boxed = crate::view_props::is_box_drag(start, end);
            state.session.brush.mode = StrokeMode::Paint;
            let result = if boxed {
                state.session.polygon_fill_uv_box_hit(to_uv(start), to_uv(end), state.fill_mode, state.fill_hit_test)
            } else {
                state.session.polygon_fill_uv(to_uv(start), state.fill_mode)
            };
            match result {
                Ok(tiles) if tiles.is_empty() && boxed => state.set_status("Polygon fill: the box touched no UV island of this texture set", now),
                Ok(tiles) if tiles.is_empty() => state.set_status("Polygon fill: click inside a UV island of this texture set", now),
                Ok(_) => state.gpu_dirty = true,
                Err(e) => state.set_status(format!("Polygon fill: {e}"), now),
            }
        }
        return;
    }
    if state.uv.painting {
        if !down {
            state.session.end_stroke();
            state.uv.painting = false;
        } else if let Some(p) = pointer
            && response.drag_delta() != egui::Vec2::ZERO
        {
            let uv = to_uv(p);
            state.session.set_stroke_pressure(pressure);
            let _ = state.session.stroke_to_uv(uv, false, texels_per_px);
            state.uv.last_point = Some(uv);
        }
        return;
    }
    if !pressed || !response.hovered() || alt || typing || state.painting || lmb_adjust {
        return;
    }
    let Some(p) = pointer else { return };
    let uv = to_uv(p);
    match state.tool {
        Tool::Eyedropper => {
            state.session.sample_material_at(ts_id, uv, shift);
            state.gpu_dirty = true;
            state.set_status(if shift { "Picked material" } else { "Picked color" }, now);
            return;
        }
        Tool::PolygonFill => {
            state.uv.fill_box = Some(p);
            return;
        }
        Tool::Clone if (ctrl && !shift) || v_down => {
            if state.session.set_clone_source_uv(uv) {
                state.set_status("Clone source set: now paint where the copy should go", now);
            } else {
                state.set_status(crate::keys::native("Ctrl+click inside a UV island to set the clone source"), now);
            }
            return;
        }
        Tool::Brush | Tool::Eraser | Tool::Clone | Tool::Smudge => {}
        _ => return,
    }
    state.session.brush.mode = match state.tool {
        Tool::Eraser => StrokeMode::Erase,
        Tool::Clone => StrokeMode::Clone,
        Tool::Smudge => StrokeMode::Smudge,
        _ => StrokeMode::Paint,
    };
    // Projection casts from the 3D view; 2D strokes never do.
    state.session.brush.projection = false;
    if let Some(name) = state.session.ensure_paint_layer() {
        state.gpu_dirty = true;
        state.set_status(format!("Painting on a new paint layer, \"{name}\" (the selected layer was not a paint layer)"), now);
    }
    // A held: continue the previous stroke (stroke opacity keeps its cap).
    state.session.set_continue_stroke(ui.input(|i| i.key_down(egui::Key::A)));
    match state.session.begin_uv_stroke(Some(view)) {
        Ok(()) => {
            state.uv.painting = true;
            state.session.set_stroke_pressure(pressure);
            if let Some(from) = state.uv.last_point.filter(|_| shift) {
                // Ctrl+Shift+click: the line snaps to 5 degree steps, as in the 3D view.
                let to = to_uv(line_end(to_px(from), p, ctrl));
                let _ = state.session.stroke_to_uv(from, true, texels_per_px);
                let _ = state.session.stroke_to_uv(to, true, texels_per_px);
                state.uv.last_point = Some(to);
            } else {
                let _ = state.session.stroke_to_uv(uv, false, texels_per_px);
                state.uv.last_point = Some(uv);
            }
        }
        Err(subcraft_paint::PaintError::NoCloneSource) => state.set_status(crate::keys::native("Ctrl+click inside a UV island to set the clone source first"), now),
        Err(e) => state.set_status(format!("Can't paint here: {e} (select a paint layer)"), now),
    }
}

/// Zoom and offset of the 2D view (whose unzoomed texture is `base` points wide) that centre UV
/// `center` and show a UV rectangle of `extent` with a margin, within the view's zoom range.
pub fn uv_frame(base: f32, center: Vec2, extent: Vec2) -> (f32, egui::Vec2) {
    let zoom = (1.0 / (extent.max_element().max(1e-3) * 1.3)).clamp(0.2, 20.0);
    let side = base * zoom;
    (zoom, egui::vec2(0.5 - center.x, 0.5 - center.y) * side)
}

#[cfg(test)]
mod frame_tests {
    use super::*;

    #[test]
    fn framing_a_uv_rectangle_centres_it_and_fits_it() {
        let view = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(500.0, 400.0));
        let base = 400.0 * 0.95;
        let (center, extent) = (Vec2::new(0.25, 0.7), Vec2::new(0.1, 0.05));
        let (zoom, offset) = uv_frame(base, center, extent);
        let side = base * zoom;
        let img = egui::Rect::from_center_size(view.center() + offset, egui::vec2(side, side));
        let at = img.min + egui::vec2(center.x, center.y) * side;
        assert!((at - view.center()).length() < 1e-3, "the rectangle's centre is the view's centre: {at:?}");
        assert!(extent.x * side < view.width() && extent.x * side > view.width() * 0.4, "fits with a margin: {}", extent.x * side);
        // Huge and tiny rectangles stay within the zoom range.
        assert_eq!(uv_frame(base, center, Vec2::splat(10.0)).0, 0.2);
        assert_eq!(uv_frame(base, center, Vec2::ZERO).0, 20.0);
    }
}
#[cfg(test)]
mod navigation_tests {
    use super::*;

    #[test]
    fn zooming_keeps_the_texture_point_under_the_anchor() {
        // A texture point drawn at `anchor` is at (anchor - offset) / zoom in zoom-free units.
        let point = |zoom: f32, offset: egui::Vec2, anchor: egui::Vec2| (anchor - offset) / zoom;
        let (zoom, offset, anchor) = (1.5, egui::vec2(40.0, -25.0), egui::vec2(120.0, 80.0));
        for factor in [1.3, 0.6] {
            let (z, o) = zoom_about(zoom, offset, anchor, factor);
            assert!((z - zoom * factor).abs() < 1e-5);
            let (before, after) = (point(zoom, offset, anchor), point(z, o, anchor));
            assert!((before - after).length() < 1e-3, "{before:?} moved to {after:?}");
        }
        // Zooming on the view centre with no pan leaves the pan alone.
        assert_eq!(zoom_about(1.0, egui::Vec2::ZERO, egui::Vec2::ZERO, 2.0), (2.0, egui::Vec2::ZERO));
    }

    #[test]
    fn zoom_stops_at_its_limits_without_drifting() {
        let (z, o) = zoom_about(20.0, egui::vec2(10.0, 10.0), egui::vec2(200.0, 50.0), 4.0);
        assert_eq!((z, o), (20.0, egui::vec2(10.0, 10.0)), "at the limit nothing moves");
        assert_eq!(zoom_about(0.25, egui::Vec2::ZERO, egui::Vec2::ZERO, 0.1).0, 0.2);
    }

    #[test]
    fn the_material_image_shades_the_composite() {
        use subcraft_image::TiledImage;
        let composite = |res: u32, base: [f32; 4]| {
            let mut channels = std::collections::BTreeMap::new();
            channels.insert(ChannelKind::BaseColor, TiledImage::new(res, res, 4, base));
            channels.insert(ChannelKind::Roughness, TiledImage::new(res, res, 2, [1.0, 1.0, 0.0, 0.0]));
            subcraft_compositor::Composite { resolution: res, channels }
        };
        // A flat red rough dielectric: lit red everywhere (no normal map: flat).
        let image = material_image(&composite(128, [0.8, 0.05, 0.05, 1.0]));
        assert_eq!(image.size, [128, 128], "small sets keep their resolution");
        let first = image.pixels[0];
        assert!(image.pixels.iter().all(|p| *p == first));
        assert!(first.r() > 150 && first.g() < 90 && first.g() == first.b(), "{first:?}");
        // Brighter base, brighter result; large sets are sampled down to 1024.
        let grey = material_image(&composite(2048, [0.2; 4]));
        let white = material_image(&composite(16, [1.0; 4]));
        assert_eq!(grey.size, [1024, 1024]);
        assert!(white.pixels[0].r() > grey.pixels[0].r());
    }

    #[test]
    fn the_2d_view_follows_the_3d_display() {
        use PreviewSource::*;
        assert_eq!(source_for(None, None), Channel(ChannelKind::BaseColor), "material: base colour");
        assert_eq!(source_for(None, Some(ChannelKind::Roughness)), Channel(ChannelKind::Roughness));
        let map = MeshMapKind::ALL[0];
        assert_eq!(source_for(Some(map), Some(ChannelKind::Roughness)), MeshMap(map), "the mesh map viewer wins");
    }
}

/// The component the 2D view isolates: Display Settings > Channel component while the 3D view
/// shows a single channel (the 2D view follows it), otherwise all of them.
/// Display Settings > Channel display for the 2D view: the HDR scale and sign colors always (the
/// 2D view always shows one channel), the isolated component while the 3D view shows a channel.
pub fn display_2d(prefs: &crate::viewport_bar::ViewportPrefs) -> subcraft_render::ChannelDisplay {
    subcraft_render::ChannelDisplay { component: shown_component(prefs), ..prefs.channel_display() }
}

pub fn shown_component(prefs: &crate::viewport_bar::ViewportPrefs) -> subcraft_render::ChannelComponent {
    if prefs.shown_channel().is_some() { prefs.channel_component } else { subcraft_render::ChannelComponent::All }
}

#[cfg(test)]
mod component_tests {
    use super::*;

    #[test]
    fn the_2d_view_isolates_the_component_only_for_a_single_channel() {
        use subcraft_render::ChannelComponent;
        let mut prefs = crate::viewport_bar::ViewportPrefs::default();
        prefs.display.channel_component = ChannelComponent::G;
        assert_eq!(shown_component(&prefs), ChannelComponent::All, "the material view shows every component");
        prefs.solo = Some(ChannelKind::Roughness);
        assert_eq!(shown_component(&prefs), ChannelComponent::G);
        prefs.mesh_map = Some(MeshMapKind::ALL[0]);
        assert_eq!(shown_component(&prefs), ChannelComponent::All, "a mesh map is not a channel");
    }
}

#[cfg(test)]
mod rotation_tests {
    use super::*;

    fn frame(view: Rect, offset: egui::Vec2, rotation: f32) -> Frame {
        Frame { rect: Rect::from_center_size(view.center() + offset, egui::vec2(300.0, 300.0)), rotation }
    }

    #[test]
    fn an_upright_frame_is_the_plain_square() {
        let f = Frame { rect: Rect::from_min_size(egui::pos2(20.0, 40.0), egui::vec2(200.0, 200.0)), rotation: 0.0 };
        assert_eq!(f.to_px(Vec2::new(0.25, 0.5)), egui::pos2(70.0, 140.0));
        assert_eq!(f.to_uv(egui::pos2(70.0, 140.0)), Vec2::new(0.25, 0.5));
        assert!(f.contains(egui::pos2(21.0, 41.0)) && !f.contains(egui::pos2(19.0, 41.0)));
    }

    #[test]
    fn a_quarter_turn_points_u_down_the_screen() {
        let f = Frame { rect: Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 200.0)), rotation: std::f32::consts::FRAC_PI_2 };
        // The middle of the texture's right edge is now at the bottom middle of the square.
        assert!((f.to_px(Vec2::new(1.0, 0.5)) - egui::pos2(100.0, 200.0)).length() < 1e-3);
        for uv in [Vec2::new(0.1, 0.2), Vec2::new(0.9, 0.6)] {
            assert!((f.to_uv(f.to_px(uv)) - uv).length() < 1e-5, "{uv}");
            assert!(f.contains(f.to_px(uv)));
        }
        assert!(!f.contains(f.to_px(Vec2::new(1.2, 0.5))));
    }

    #[test]
    fn rotating_keeps_the_texture_point_under_the_pivot() {
        let view = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        let (rotation, offset) = (0.3, egui::vec2(35.0, -20.0));
        let pivot = egui::pos2(520.0, 210.0);
        let before = frame(view, offset, rotation).to_uv(pivot);
        for next in [0.9, -1.2, snap_quarter(0.9)] {
            let (r, o) = rotate_about(rotation, offset, pivot - view.center(), next);
            assert_eq!(r, next);
            let after = frame(view, o, r).to_uv(pivot);
            assert!((before - after).length() < 1e-4, "{before} moved to {after} at {next}");
        }
    }

    #[test]
    fn shift_snaps_to_quarter_turns() {
        let q = std::f32::consts::FRAC_PI_2;
        assert_eq!(snap_quarter(0.2), 0.0);
        assert_eq!(snap_quarter(1.2), q);
        assert_eq!(snap_quarter(-2.0), -q);
        assert_eq!(snap_quarter(3.0), 2.0 * q);
    }

    #[test]
    fn a_drag_box_in_a_turned_view_is_the_uv_box_of_its_two_corners() {
        let f = Frame { rect: Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(400.0, 400.0)), rotation: 0.7 };
        let (a, b) = (f.to_px(Vec2::new(0.2, 0.3)), f.to_px(Vec2::new(0.6, 0.8)));
        let corners = f.drag_box(a, b).map(|p| f.to_uv(p));
        let want = [Vec2::new(0.2, 0.3), Vec2::new(0.6, 0.3), Vec2::new(0.6, 0.8), Vec2::new(0.2, 0.8)];
        for (got, want) in corners.iter().zip(want) {
            assert!((*got - want).length() < 1e-4, "{got} vs {want}");
        }
    }
}

