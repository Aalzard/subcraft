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
            let lines = crate::widgets::icon_toggle(ui, crate::icons::GRID, state.uv_wireframe, "UV wireframe · click to show/hide · right-click for color and opacity");
            if lines.clicked() {
                state.uv_wireframe = !state.uv_wireframe;
            }
            egui::Popup::context_menu(&lines).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                let prefs = &mut state.recovery.prefs.viewport;
                ui.label(RichText::new("UV wireframe").color(crate::theme::TEXT_DIM));
                ui.checkbox(&mut state.uv_wireframe, "Show UV wireframe");
                ui.horizontal(|ui| {
                    ui.label("Color");
                    ui.color_edit_button_rgb(&mut prefs.wire_color);
                });
                ui.add(egui::Slider::new(&mut prefs.wire_opacity, 0.05..=1.0).text("Opacity"));
                ui.label(RichText::new("Shared with the 3D view's mesh wireframe (Display settings)").small().color(crate::theme::TEXT_FAINT));
            });
            if crate::widgets::icon_button(ui, crate::icons::FIT, "Fit to view").clicked() {
                state.uv.zoom = 1.0;
                state.uv.offset = egui::Vec2::ZERO;
            }
        });
        ui.horizontal_wrapped(|ui| brush_controls(state, ui));
        if painting_tool {
            crate::brush_bar::channel_switches(ui, state);
        }
        if state.picking_parts {
            ui.horizontal_wrapped(|ui| crate::panels::parts::mask_bar(ui, state));
        }
    });
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    state.hovered_2d = response.hovered();
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            state.uv.zoom = (state.uv.zoom * (scroll * 0.002).exp()).clamp(0.2, 20.0);
        }
    }
    let (alt, mods, delta) = ui.input(|i| (i.modifiers.alt, i.modifiers, i.pointer.delta()));
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
    if !stencil_edit
        && (response.dragged_by(egui::PointerButton::Middle)
            || (response.dragged_by(egui::PointerButton::Primary) && !state.uv.painting && !lmb_adjust && (alt || !clicks)))
    {
        state.uv.offset += response.drag_delta();
    }
    let side = rect.width().min(rect.height()) * 0.95 * state.uv.zoom;
    let img_rect = Rect::from_center_size(rect.center() + state.uv.offset, egui::vec2(side, side));
    let viewport = [rect.width().round().max(1.0) as u32, rect.height().round().max(1.0) as u32];
    let view = subcraft_paint::UvView {
        origin: Vec2::new(img_rect.min.x - rect.min.x, img_rect.min.y - rect.min.y),
        side,
        viewport,
    };
    crate::pen::uv_input(state, ui, &response, img_rect);
    // Picking parts for a geometry mask owns the clicks; otherwise the ID eyedropper, then tools.
    let id_eyedropper = !picking && id_pick_input(state, ui, &response, img_rect, ts_id);
    if picking {
        // A layer without a geometry mask gets one by UV island when it is picked here.
        if response.hovered() {
            state.session.set_new_mask_kind(subcraft_core::PartKind::UvIsland);
        }
        parts_input(state, ui, &response, img_rect);
    } else if !id_eyedropper && !stencil_edit {
        paint_input(state, ui, &response, img_rect, resolution, ts_id, view, lmb_adjust);
    }
    let painter = ui.painter_at(rect);
    let filter = if state.uv.source == PreviewSource::Channel(ChannelKind::BaseColor)
        && state.session.selected_texture_set().albedo_filter == subcraft_image::TextureFilter::Nearest {
        wgpu::FilterMode::Nearest
    } else { wgpu::FilterMode::Linear };
    painter.rect_filled(rect, 0.0, crate::theme::BG_0);

    state.sync_gpu();
    if let (Some(rs), Some(renderer)) = (&state.render_state, &mut state.renderer)
        && let Some(preview) = renderer.preview(&rs.device, &rs.queue, slot, state.uv.source)
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
        painter.image(id, img_rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }
    painter.rect_stroke(img_rect, 0.0, Stroke::new(1.0, crate::theme::BORDER_STRONG), egui::StrokeKind::Outside);
    quick_mask_overlay(state, ui, &painter, img_rect);
    parts_overlay(state, ui, &painter, img_rect, picking, response.hover_pos());

    if state.uv_wireframe {
        let mesh = state.session.scene().mesh.clone();
        let edges = state.uv_edges.entry(slot).or_insert_with(|| topology::uv_edges(&mesh, mesh.material_triangles(slot)));
        let to = |p: glam::Vec2| img_rect.min + egui::vec2(p.x * side, p.y * side);
        let stroke = Stroke::new(0.6, wire_color(&state.recovery.prefs.viewport));
        for [a, b] in edges.iter() {
            painter.line_segment([to(*a), to(*b)], stroke);
        }
    }
    if state.tool == Tool::Pen && let Some(path) = state.pen.preview(&state.session) {
        crate::pen::draw(&painter, path, state.pen.selected_anchor(), |uv| Some(img_rect.min + egui::vec2(uv.x, uv.y) * side));
    }
    for start in [state.uv.fill_box, state.uv.parts_box] {
        if let (Some(a), Some(b)) = (start, response.hover_pos().or(ui.input(|i| i.pointer.latest_pos())))
            && crate::view_props::is_box_drag(a, b)
        {
            crate::view_props::draw_fill_box(&painter, Rect::from_two_pos(a, b), false);
        }
    }
    let to_px = |uv: Vec2| img_rect.min + egui::vec2(uv.x, uv.y) * side;
    if state.tool == Tool::Clone && let Some(source) = state.session.clone_source_uv() {
        let marker = to_px(source);
        let cross = Color32::from_rgb(120, 220, 255);
        for (w, c) in [(3.0, Color32::from_black_alpha(160)), (1.5, cross)] {
            painter.line_segment([marker - egui::vec2(7.0, 0.0), marker + egui::vec2(7.0, 0.0)], Stroke::new(w, c));
            painter.line_segment([marker - egui::vec2(0.0, 7.0), marker + egui::vec2(0.0, 7.0)], Stroke::new(w, c));
        }
        // While an aligned offset exists the copy comes from beside the cursor: show where.
        if let (Some(offset), Some(p)) = (state.session.clone_offset_uv(), response.hover_pos()) {
            painter.circle_stroke(p + egui::vec2(offset.x, offset.y) * side, 6.0, Stroke::new(1.5, cross));
        }
    }
    crate::viewport::draw_stencil(state, ui, &painter, rect, viewport);
    if let Some(p) = response.hover_pos() {
        if matches!(state.tool, Tool::Brush | Tool::Eraser | Tool::Clone | Tool::Smudge) && paints {
            let mesh = &state.session.scene().mesh;
            let density = *state.uv_density.entry(slot).or_insert_with(|| subcraft_paint::stroke::uv_per_unit(mesh, slot));
            let r_px = state.session.brush_radius_uv(density) * side;
            let color = if state.tool == Tool::Eraser { Color32::from_rgb(120, 190, 255) } else { crate::theme::ACCENT };
            painter.circle_stroke(p, r_px, Stroke::new(1.5, Color32::from_black_alpha(160)));
            painter.circle_stroke(p, r_px, Stroke::new(1.0, color));
            // Shift: dotted preview of the straight line from the last stroke point.
            if mods.shift && !state.uv.painting && let Some(from) = state.uv.last_point {
                let to = line_end(to_px(from), p, mods.command || mods.ctrl);
                let dash = Stroke::new(1.2, Color32::from_white_alpha(200));
                painter.extend(egui::Shape::dashed_line(&[to_px(from), to], dash, 5.0, 4.0));
            }
        } else if state.tool == Tool::PolygonFill && paints {
            painter.circle_filled(p, 3.0, crate::theme::ACCENT);
        }
        if (clicks && !alt) && !stencil_edit {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
    }
    let hint = if picking { "Geometry mask: click an island to show / hide the layer on it · drag a box to add · Ctrl+drag to remove · Esc done" } else { hint(state.tool, paints) };
    painter.text(rect.left_bottom() + egui::vec2(10.0, -10.0), egui::Align2::LEFT_BOTTOM, hint, crate::theme::font(11.5), crate::theme::TEXT_FAINT);
    if state.uv.painting {
        ui.ctx().request_repaint();
    }
}

/// Geometry mask editing in the 2D view (Properties → Parts → pick, or the layer row's geometry
/// mask icon): a click shows / hides the layer on the part under the cursor, a drag adds every
/// part the box touches, Ctrl+drag removes them.
fn parts_input(state: &mut UiState, ui: &Ui, response: &egui::Response, image: Rect) {
    let (pointer, pressed, down, mods) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_pressed(), i.pointer.primary_down(), i.modifiers));
    let to_uv = |p: Pos2| Vec2::new((p.x - image.left()) / image.width(), (p.y - image.top()) / image.height());
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

/// The geometry mask over the 2D view, while it is edited or Alt+H hides what it leaves out:
/// excluded geometry grey with diagonal lines; while editing, the part under the cursor (or under
/// the cursor in the Properties list) is highlighted.
fn parts_overlay(state: &mut UiState, ui: &Ui, painter: &egui::Painter, image: Rect, picking: bool, pointer: Option<Pos2>) {
    let list_hover = state.uv_parts.list_hover.take();
    if !picking && !state.session.hide_excluded() {
        return;
    }
    let ts = state.session.selected_texture_set().id;
    let key = state.session.selected_layer().and_then(|l| l.parts.clone().filter(|p| p.enabled).map(|p| (ts, l.id, p)));
    if key != state.uv_parts.excluded_key {
        state.uv_parts.excluded = if key.is_some() { state.session.excluded_uv_triangles() } else { Vec::new() };
        state.uv_parts.excluded_key = key;
    }
    let side = image.width();
    let to_px = |uv: Vec2| image.min + egui::vec2(uv.x, uv.y) * side;
    let clip = painter.clip_rect();
    if !state.uv_parts.excluded.is_empty() {
        let options = egui::TextureOptions { wrap_mode: egui::TextureWrapMode::Repeat, ..egui::TextureOptions::NEAREST };
        let hatch = state.uv_parts.hatch.get_or_insert_with(|| ui.ctx().load_texture("geometry-mask-hatch", hatch_image(), options)).id();
        painter.add(triangles_mesh(&state.uv_parts.excluded, to_px, clip, Color32::WHITE, Some(hatch)));
    }
    if !picking {
        return;
    }
    let under_cursor = pointer.filter(|p| image.contains(*p)).and_then(|p| state.session.part_at_uv(Vec2::new((p.x - image.left()) / side, (p.y - image.top()) / side)));
    let part = list_hover.or(under_cursor);
    let hover_key = part.map(|p| (ts, state.session.mask_kind(), p));
    if hover_key != state.uv_parts.hover_key {
        state.uv_parts.hover = part.map_or_else(Vec::new, |p| state.session.part_uv_triangles(p));
        state.uv_parts.hover_key = hover_key;
    }
    if !state.uv_parts.hover.is_empty() {
        let tint = Color32::from_rgba_unmultiplied(crate::theme::ACCENT.r(), crate::theme::ACCENT.g(), crate::theme::ACCENT.b(), 70);
        painter.add(triangles_mesh(&state.uv_parts.hover, to_px, clip, tint, None));
    }
}

/// The quick mask over the 2D view: its protected areas tinted red (strongly while it is being
/// edited, lightly once applied).
fn quick_mask_overlay(state: &mut UiState, ui: &Ui, painter: &egui::Painter, image: Rect) {
    let Some(q) = state.session.quick_mask() else {
        state.quick_mask_tex = None;
        return;
    };
    let (revision, editing) = (q.revision, q.editing);
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
        painter.image(tex.id(), image, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), tint);
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
            _ => "LMB / MMB pan · RMB properties · Wheel zoom",
        };
    }
    match tool {
        Tool::PolygonFill => "LMB click fill · LMB drag box fill · RMB properties · Alt+LMB / MMB pan · Wheel zoom",
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
    crate::brush_bar::show(ui, &mut state.tool, &mut settings, &mut options, &mut state.recovery.prefs.viewport.symmetry_guides, width);
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
fn id_pick_input(state: &mut UiState, ui: &Ui, response: &egui::Response, image: Rect, ts: subcraft_core::TextureSetId) -> bool {
    let mask_pick = state.picking_id_color.filter(|(layer, _)| state.session.selected_texture_set().find(*layer).is_some());
    if mask_pick.is_none() && !state.picking_id_palette {
        return false;
    }
    if response.hovered() { ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair); }
    let (pointer, pressed, alt) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_pressed(), i.modifiers.alt));
    if !pressed || alt || !response.hovered() { return true; }
    let Some(p) = pointer else { return true };
    let uv = Vec2::new((p.x - image.left()) / image.width(), (p.y - image.top()) / image.height());
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
fn paint_input(state: &mut UiState, ui: &Ui, response: &egui::Response, image: Rect, resolution: u32, ts_id: subcraft_core::TextureSetId, view: subcraft_paint::UvView, lmb_adjust: bool) {
    let typing = ui.ctx().egui_wants_keyboard_input();
    let (pointer, pressed, down, mods) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_pressed(), i.pointer.primary_down(), i.modifiers));
    let (alt, shift, ctrl) = (mods.alt, mods.shift, mods.command || mods.ctrl);
    let v_down = ui.input(|i| i.key_down(egui::Key::V)) && !typing && !mods.any();
    let pressed = pressed && !state.view_props_swallow;
    let to_uv = |p: Pos2| Vec2::new((p.x - image.left()) / image.width(), (p.y - image.top()) / image.height());
    let to_px = |uv: Vec2| image.min + egui::vec2(uv.x, uv.y) * image.width();
    let texels_per_px = resolution as f32 / image.width().max(1.0);
    let now = ui.input(|i| i.time);
    // Pen pressure, as in the 3D view (a mouse reports none: 1).
    let (pressure, carried) = crate::viewport::pen_pressure(&ui.input(|i| i.events.clone()), state.pen_pressure);
    state.pen_pressure = carried;
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
                state.session.polygon_fill_uv_box(to_uv(start), to_uv(end), state.fill_mode)
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
    if let Some(name) = state.session.ensure_paint_layer() {
        state.gpu_dirty = true;
        state.set_status(format!("Painting on a new paint layer, \"{name}\" (the selected layer was not a paint layer)"), now);
    }
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
