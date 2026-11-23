//! Pen presentation: canvas coordinates and hit testing only. Editing/raster/history live in app.
use egui::{Color32, Pos2, Stroke, Ui};
use glam::Vec2;
use subcraft_app::pen::{Handle, PenHit};
use subcraft_doc::PenPath;
use crate::{state::{Tool, UiState}, theme};
use crate::widgets::{prop_row_reset, reset_on, slider_row_reset};

/// Painter's path snapping (the magnet menu): new and moved points jump to the model's
/// vertices, edges or edge centres near the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PenSnap {
    pub vertices: bool,
    pub edges: bool,
    pub edge_centres: bool,
    /// Ctrl's angle snapping step when adding a point, in degrees (Painter's Angle snapping
    /// settings).
    pub angle_deg: f32,
}

impl Default for PenSnap {
    fn default() -> Self {
        Self { vertices: false, edges: false, edge_centres: false, angle_deg: 15.0 }
    }
}

impl PenSnap {
    pub const ANGLE: std::ops::RangeInclusive<f32> = 1.0..=90.0;

    pub fn any(&self) -> bool {
        self.vertices || self.edges || self.edge_centres
    }

    /// The angle step in radians, kept in range.
    pub fn angle(&self) -> f32 {
        let deg = if self.angle_deg.is_finite() { self.angle_deg.clamp(*Self::ANGLE.start(), *Self::ANGLE.end()) } else { 15.0 };
        deg.to_radians()
    }
}

/// How close (screen points) the pointer must be to a snap target.
pub const SNAP_RADIUS: f32 = 10.0;

/// The snap target for `pointer` on the triangle whose corners are `tri` on screen: the
/// nearest enabled corner or edge centre within [`SNAP_RADIUS`], else the nearest point on an
/// edge within it (Snap to edges); `None` when nothing is close enough.
pub fn snap_to_triangle(pointer: Pos2, tri: [Pos2; 3], snap: PenSnap) -> Option<Pos2> {
    let nearest = |points: &mut dyn Iterator<Item = Pos2>| {
        points.map(|q| (q.distance(pointer), q)).filter(|(d, _)| *d <= SNAP_RADIUS).min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, q)| q)
    };
    let edges = [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])];
    let mut points = tri.iter().copied().filter(|_| snap.vertices).chain(edges.iter().map(|(a, b)| a.lerp(*b, 0.5)).filter(|_| snap.edge_centres));
    if let Some(q) = nearest(&mut points) {
        return Some(q);
    }
    if !snap.edges {
        return None;
    }
    let mut on_edges = edges.iter().map(|&(a, b)| {
        let d = b - a;
        let t = if d.length_sq() > 1e-9 { ((pointer - a).dot(d) / d.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
        a + d * t
    });
    nearest(&mut on_edges)
}

#[derive(Clone, Copy)]
pub enum After { Action(subcraft_app::Action), Request(crate::dialogs::Pending) }

pub fn hit(path: &PenPath, selected: Option<subcraft_core::AnchorId>, pointer: Pos2, map: impl Fn(Vec2) -> Option<Pos2>) -> Option<PenHit> {
    let mut best = None; let mut distance = 9.0;
    for a in &path.anchors {
        let mut candidates = vec![(Handle::Anchor, a.position)];
        if selected == Some(a.id) {
            if a.incoming.length_squared() > 1e-10 { candidates.push((Handle::Incoming, a.position + a.incoming)); }
            if a.outgoing.length_squared() > 1e-10 { candidates.push((Handle::Outgoing, a.position + a.outgoing)); }
        }
        for (handle, uv) in candidates {
            if let Some(p) = map(uv) && p.distance(pointer) < distance {
                distance = p.distance(pointer); best = Some(PenHit { anchor: a.id, handle });
            }
        }
    }
    best
}

/// The curve point nearest `pointer` within a few points on screen: segment and curve
/// parameter. Segments are sampled finely enough for a click on the visible curve.
pub fn curve_hit(path: &PenPath, pointer: Pos2, map: impl Fn(Vec2) -> Option<Pos2>) -> Option<(usize, f32)> {
    const STEPS: usize = 32;
    let mut best: Option<(f32, usize, f32)> = None;
    for seg in 0..path.segment_count() {
        let Some([p0, p1, p2, p3]) = path.segment(seg) else { continue };
        let at = |t: f32| {
            let u = 1.0 - t;
            p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t)
        };
        let mut prev: Option<(Pos2, f32)> = None;
        for k in 0..=STEPS {
            let t = k as f32 / STEPS as f32;
            let Some(s) = map(at(t)) else { prev = None; continue };
            if let Some((a, ta)) = prev {
                let d = s - a;
                let f = if d.length_sq() > 1e-6 { ((pointer - a).dot(d) / d.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
                let dist = pointer.distance(a + d * f);
                if dist <= 6.0 && best.is_none_or(|b| dist < b.0) {
                    best = Some((dist, seg, ta + (t - ta) * f));
                }
            }
            prev = Some((s, t));
        }
    }
    best.map(|(_, seg, t)| (seg, t))
}

/// The path, its points and the main selected point's tangents. `selected` lists every selected
/// point, the main one first.
/// Painter's path display settings: how paths are drawn over the views.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PenDisplay {
    /// Show the paths and their points at all (Show / hide viewport interface).
    pub show: bool,
    /// Radius of the points, in screen points.
    pub handle_size: f32,
    /// Width of the selected path's line.
    pub path_width: f32,
    /// Colour of the selected path; `None` is the theme's accent.
    pub path_color: Option<Color32>,
    /// Colour of the other paths of the texture set (drawn thinner, without points).
    pub unselected_color: Color32,
    /// Short ticks across the path (its normal on screen).
    pub normals: bool,
    /// Tangent handles of the selected point.
    pub tangents: bool,
    /// Arrows along the path showing its direction.
    pub direction: bool,
}

impl Default for PenDisplay {
    fn default() -> Self {
        Self {
            show: true,
            handle_size: 4.5,
            path_width: 1.5,
            path_color: None,
            unselected_color: Color32::from_rgba_unmultiplied(190, 190, 190, 150),
            normals: false,
            tangents: true,
            direction: false,
        }
    }
}

impl PenDisplay {
    pub const HANDLE_SIZE: std::ops::RangeInclusive<f32> = 2.0..=10.0;
    pub const PATH_WIDTH: std::ops::RangeInclusive<f32> = 0.5..=6.0;

    pub fn color(&self) -> Color32 {
        self.path_color.unwrap_or_else(theme::ACCENT)
    }
}

/// Direction arrows: one at the middle of each screen segment run, `size` points long.
pub fn direction_marks(points: &[Pos2], every: f32, size: f32) -> Vec<[Pos2; 3]> {
    let mut marks = Vec::new();
    let mut travelled = every * 0.5;
    for w in points.windows(2) {
        let d = w[1] - w[0];
        let len = d.length();
        if len < 1e-3 {
            continue;
        }
        let dir = d / len;
        let mut s = travelled;
        while s <= len {
            let tip = w[0] + dir * s;
            let back = tip - dir * size;
            let side = egui::vec2(-dir.y, dir.x) * (size * 0.5);
            marks.push([tip, back + side, back - side]);
            s += every;
        }
        travelled = s - len;
    }
    marks
}

/// The other paths of the texture set: a thin line in the unselected colour.
pub fn draw_unselected(painter: &egui::Painter, path: &PenPath, display: &PenDisplay, map: impl Fn(Vec2) -> Option<Pos2>) {
    if !display.show {
        return;
    }
    if let Ok(points) = path.polyline(0.004) {
        for pair in points.windows(2) {
            if let (Some(a), Some(b)) = (map(pair[0]), map(pair[1])) { painter.line_segment([a, b], Stroke::new((display.path_width * 0.7).max(0.5), display.unselected_color)); }
        }
    }
}

pub fn draw(painter: &egui::Painter, path: &PenPath, selected: &[subcraft_core::AnchorId], display: &PenDisplay, map: impl Fn(Vec2) -> Option<Pos2>) {
    if !display.show {
        return;
    }
    let color = display.color();
    if let Ok(points) = path.polyline(0.002) {
        let screen: Vec<Option<Pos2>> = points.iter().map(|&p| map(p)).collect();
        for pair in screen.windows(2) {
            if let (Some(a), Some(b)) = (pair[0], pair[1]) { painter.line_segment([a, b], Stroke::new(display.path_width, color)); }
        }
        // Runs of visible points for the normal ticks and direction arrows.
        for run in screen.split(Option::is_none) {
            let run: Vec<Pos2> = run.iter().flatten().copied().collect();
            if display.normals {
                for w in run.windows(2).step_by(6) {
                    let d = (w[1] - w[0]).normalized();
                    let n = egui::vec2(-d.y, d.x) * 6.0;
                    painter.line_segment([w[0] - n, w[0] + n], Stroke::new(1.0, color.gamma_multiply(0.7)));
                }
            }
            if display.direction {
                for [tip, a, b] in direction_marks(&run, 60.0, 7.0) {
                    painter.add(egui::Shape::convex_polygon(vec![tip, a, b], color, Stroke::NONE));
                }
            }
        }
    }
    let radius = display.handle_size.clamp(*PenDisplay::HANDLE_SIZE.start(), *PenDisplay::HANDLE_SIZE.end());
    for a in &path.anchors {
        let Some(p) = map(a.position) else { continue; };
        if display.tangents && selected.first() == Some(&a.id) {
            for tangent in [a.incoming, a.outgoing] {
                if tangent.length_squared() > 1e-10 && let Some(q) = map(a.position + tangent) {
                    painter.line_segment([p, q], Stroke::new(1.0, theme::OVERLAY_TEXT_DIM));
                    painter.circle_filled(q, radius * 0.66, theme::OVERLAY_TEXT);
                    painter.circle_stroke(q, radius * 0.66, Stroke::new(1.0, theme::OVERLAY_OUTLINE));
                }
            }
        }
        painter.circle_filled(p, radius, if selected.contains(&a.id) { color } else { theme::OVERLAY_TEXT });
        painter.circle_stroke(p, radius, Stroke::new(1.0, Color32::BLACK));
    }
}

pub fn keyboard(state: &mut UiState, ctx: &egui::Context, now: f64) {
    // Not behind a modal window: Enter / Escape belong to it, and Backspace deleted the path
    // layer behind the unsaved-changes prompt.
    if state.tool != Tool::Pen || ctx.egui_wants_keyboard_input() || state.modal_open() { return; }
    // The Paths panel row under the pointer takes Delete / Ctrl+C / Ctrl+V first.
    crate::paths_panel::keys(state, ctx, now);
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        if state.pen.is_editing() || state.pen.is_pending() { state.pen.undo_pending(&state.session); }
        else { state.pen.finish_path(); }
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
        if let Err(e) = state.pen.release(&state.session) { state.set_status(e.to_string(), now); }
        state.pen.finish_path();
    }
    // Ctrl+A selects every point of the path (before the layer shortcut takes it).
    if state.pen.preview(&state.session).is_some_and(|p| !p.anchors.is_empty()) && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::A)) {
        state.pen.select_all(&state.session);
    }
    // Only while the selected point is in the path on screen: the selection outlives its path
    // (another layer selected, the point undone), and Delete then did nothing at all instead of
    // reaching the layer shortcut.
    let selected = state.pen.selected_anchor().is_some_and(|id| state.pen.preview(&state.session).is_some_and(|p| p.anchors.iter().any(|a| a.id == id)));
    if selected && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete))
        && let Err(e) = state.pen.delete_anchor(&state.session) { state.set_status(e.to_string(), now); }
    // Backspace deletes the whole path (Painter 11): its layer, when the selected layer is one.
    let path_layer = state.session.selected_layer().filter(|l| matches!(&l.content, subcraft_doc::LayerContent::Paint(p) if p.path.is_some())).map(|l| l.id);
    if let Some(layer) = path_layer && !state.pen.is_editing() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)) {
        state.pen.cancel();
        match state.session.execute(subcraft_app::Command::DeleteLayer(layer)) {
            Ok(()) => state.set_status("Path deleted", now),
            Err(e) => state.set_status(e.to_string(), now),
        }
    }
}

pub fn finish_frame(state: &mut UiState, ctx: &egui::Context, now: f64) {
    if ctx.input(|i| i.pointer.primary_released()) && let Err(e) = state.pen.release(&state.session) { state.set_status(e.to_string(), now); }
    if state.pen.is_pending() {
        match state.pen.poll(&mut state.session, 16384) {
            Ok(true) => { state.set_status(format!("Drawing pen path… {:.0}%", state.pen.progress() * 100.0), now); ctx.request_repaint(); }
            Ok(false) => { state.set_status("Pen path updated", now); ctx.request_repaint(); }
            Err(e) => state.set_status(format!("Pen: {e}"), now),
        }
    }
}

pub fn uv_input(state: &mut UiState, ui: &Ui, response: &egui::Response, image: crate::panels::uv_view::Frame) {
    if state.tool != Tool::Pen || ui.ctx().egui_wants_keyboard_input() { return; }
    let (pointer, pressed, down, alt, separate) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_pressed(), i.pointer.primary_down(), i.modifiers.alt, i.modifiers.shift));
    let Some(pointer) = pointer else { return; };
    if alt { return; }
    let map = |uv: Vec2| Some(image.to_px(uv));
    let uv = image.to_uv(pointer);
    if pressed && response.hovered() {
        let target = state.pen.preview(&state.session).and_then(|p| hit(p, state.pen.selected_anchor(), pointer, map));
        crate::pen::refresh_style(state);
        if (target.is_some() || image.contains(pointer)) && let Err(e) = state.pen.begin(&state.session, uv, target) { state.set_status(format!("Pen: {e}"), ui.input(|i| i.time)); }
    } else if down && state.pen.is_editing() { state.pen.drag_to(uv, separate); }
}

/// Ribbon path: its image (the current brush tip), Stretching & Tiling mode, copies or aspect
/// factor, mirror every second copy, flip across the path, and whether Base Color takes the
/// image's colour.
fn ribbon_rows(state: &mut UiState, ui: &mut Ui, path: &mut PenPath) -> bool {
    use subcraft_doc::{PathRibbon, RibbonAlign, RibbonMode, RibbonRatio, RibbonTiling};
    let mut changed = false;
    let ribbon = path.ribbon.get_or_insert_with(PathRibbon::default);
    let d = PathRibbon::default();
    let name = ribbon.image.and_then(|id| state.session.resources().get(id)).map_or("none (solid strip)".to_string(), |r| r.name.clone());
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("Image: {name}")).color(crate::theme::TEXT_DIM()));
        if ui.small_button("Use current brush tip").on_hover_text("Lay the tip chosen in the Brush settings / Shelf along the path").clicked() {
            ribbon.image = state.session.brush_tip_resource();
            changed = true;
        }
        if ribbon.image.is_some() && ui.small_button("Clear").clicked() {
            ribbon.image = None;
            changed = true;
        }
    });
    changed |= prop_row_reset(ui, "Mode", &mut ribbon.mode, d.mode, |ui, v| {
        combo(ui, "ribbon_mode", v, &RibbonMode::ALL, |m| m.label())
    }).changed();
    changed |= prop_row_reset(ui, "Tiling mode", &mut ribbon.tiling, d.tiling, |ui, v| {
        combo(ui, "ribbon_tiling", v, &RibbonTiling::ALL, |t| t.label())
            .on_hover_text("None: one copy. Auto: whole copies close to the image's aspect (Stretch), or copies to the end (Keep aspect ratio). Custom: the Copies value")
    }).changed();
    if ribbon.mode == RibbonMode::Stretch {
        // The number of copies only applies to Custom tiling.
        ui.add_enabled_ui(ribbon.tiling == RibbonTiling::Custom, |ui| {
            changed |= slider_row_reset(ui, "Copies", &mut ribbon.tiles, d.tiles, |v| egui::Slider::new(v, PathRibbon::TILES).logarithmic(true)).changed();
        });
        // Painter's end caps: the image's ends keep their length, only its middle stretches.
        let r = ui.checkbox(&mut ribbon.stretch_between_offsets, "Stretch between offsets only")
            .on_hover_text("The parts of the image before the start offset and after the end offset keep their length at the path's width; only the part between them stretches");
        changed |= reset_on(ui, r, &mut ribbon.stretch_between_offsets, d.stretch_between_offsets).changed();
        ui.add_enabled_ui(ribbon.stretch_between_offsets, |ui| {
            changed |= slider_row_reset(ui, "Start offset", &mut ribbon.start_offset, d.start_offset, |v| egui::Slider::new(v, 0.0..=1.0)).changed();
            changed |= slider_row_reset(ui, "End offset", &mut ribbon.end_offset, d.end_offset, |v| egui::Slider::new(v, 0.0..=1.0)).changed();
        });
    } else {
        changed |= slider_row_reset(ui, "Aspect factor", &mut ribbon.tiles, d.tiles, |v| egui::Slider::new(v, PathRibbon::TILES).logarithmic(true)).changed();
        changed |= prop_row_reset(ui, "Ratio", &mut ribbon.ratio, d.ratio, |ui, v| combo(ui, "ribbon_ratio", v, &RibbonRatio::ALL, |r| r.label())).changed();
        changed |= prop_row_reset(ui, "Alignment", &mut ribbon.alignment, d.alignment, |ui, v| combo(ui, "ribbon_alignment", v, &RibbonAlign::ALL, |a| a.label())).changed();
        // Fit to path length only makes whole copies: nothing is cut.
        let r = ui.add_enabled(ribbon.ratio == RibbonRatio::FitWidth, egui::Checkbox::new(&mut ribbon.remove_clipped, "Remove clipped tiles"))
            .on_hover_text("Draw only whole copies of the image, not the cut ones at the ends");
        changed |= reset_on(ui, r, &mut ribbon.remove_clipped, d.remove_clipped).changed();
    }
    ui.horizontal(|ui| {
        let r = ui.checkbox(&mut ribbon.mirror, "Mirror every 2nd copy");
        changed |= reset_on(ui, r, &mut ribbon.mirror, d.mirror).changed();
        let r = ui.checkbox(&mut ribbon.flip, "Flip image");
        changed |= reset_on(ui, r, &mut ribbon.flip, d.flip).changed();
    });
    let r = ui.checkbox(&mut ribbon.color_from_image, "Base Color from the image").on_hover_text("Otherwise the image only shapes the strip and Base Color is the value below");
    changed |= reset_on(ui, r, &mut ribbon.color_from_image, d.color_from_image).changed();
    changed
}

/// A dropdown of `options`; its response is marked changed when a new option was picked.
fn combo<T: PartialEq + Copy>(ui: &mut Ui, id: &str, value: &mut T, options: &[T], label: impl Fn(T) -> &'static str) -> egui::Response {
    let mut picked = false;
    let mut r = egui::ComboBox::from_id_salt(id).selected_text(label(*value)).show_ui(ui, |ui| {
        for &o in options {
            picked |= ui.selectable_value(value, o, label(o)).changed();
        }
    }).response;
    if picked {
        r.mark_changed();
    }
    r
}

/// The stitch a path is drawn with (by its stamped tip), for the stitch row's selection.
fn stitch_of(state: &UiState, path: &PenPath) -> Option<&'static subcraft_app::stitch::Stitch> {
    let tip = path.stamp.and_then(|s| s.alpha).and_then(|id| state.session.resources().get(id)).map(|r| r.name.clone());
    subcraft_app::stitch::of_path(path, tip.as_deref())
}

/// Before a click that may start a new path: the chosen stitch's tip has to be a resource of
/// the project that is open now (the choice outlives New Project / Open).
pub fn refresh_style(state: &mut UiState) {
    if let Some(style) = state.pen.style && style.tip.is_none_or(|id| state.session.resources().get(id).is_none()) {
        state.pen.style = Some(subcraft_app::stitch::style(&mut state.session, style.stitch, &state.library.alphas));
    }
}

/// Choose what the pen draws: a stitch, or a plain line (`None`). New paths take it, and so does
/// the path the next click would add to (the selected path, unless it was finished with Enter).
pub fn choose_stitch(state: &mut UiState, stitch: Option<&'static subcraft_app::stitch::Stitch>, restyle_selected: bool, now: f64) {
    state.pen.style = stitch.map(|s| subcraft_app::stitch::style(&mut state.session, s, &state.library.alphas));
    let selected = state.pen.preview(&state.session).cloned().filter(|_| restyle_selected);
    let Some(mut path) = selected else {
        state.set_status(match stitch {
            Some(s) => format!("{}: click on the model to place the points of the stitched path, Enter finishes it", s.name),
            None => "Plain line: new paths are a stroke of the brush".to_string(),
        }, now);
        return;
    };
    match state.pen.style {
        Some(style) => {
            let channels = state.session.channels();
            style.stitch.apply_to_path(&mut path, style.tip, |k| channels.contains(&k));
        }
        None => path.stamp = None,
    }
    let done = state.pen.set_path(&state.session, path).and_then(|()| state.pen.release(&state.session));
    match (done, stitch) {
        (Err(e), _) => state.set_status(e.to_string(), now),
        (Ok(()), Some(s)) => state.set_status(format!("{} on this path and on new paths", s.name), now),
        (Ok(()), None) => state.set_status("Plain line on this path and on new paths", now),
    }
}

/// A small drawing of a stitch (or of the plain line) for its tile.
fn stitch_icon(painter: &egui::Painter, rect: egui::Rect, stitch: Option<&subcraft_app::stitch::Stitch>, color: Color32) {
    let stroke = Stroke::new(2.0, color);
    let (l, r, y) = (rect.left() + 5.0, rect.right() - 5.0, rect.center().y);
    let n = 3;
    let cell = (r - l) / n as f32;
    let Some(s) = stitch else {
        painter.line_segment([Pos2::new(l, y), Pos2::new(r, y)], stroke);
        return;
    };
    for i in 0..n {
        let (a, b) = (l + cell * i as f32, l + cell * (i + 1) as f32);
        let mid = (a + b) * 0.5;
        match s.tip {
            "Stitch Cross" => {
                let h = cell * 0.32;
                painter.line_segment([Pos2::new(mid - h, y - h), Pos2::new(mid + h, y + h)], stroke);
                painter.line_segment([Pos2::new(mid - h, y + h), Pos2::new(mid + h, y - h)], stroke);
            }
            "Stitch Zigzag" => {
                let h = cell * 0.3;
                painter.line_segment([Pos2::new(a, y - h), Pos2::new(mid, y + h)], stroke);
                painter.line_segment([Pos2::new(mid, y + h), Pos2::new(b, y - h)], stroke);
            }
            "Stitch Saddle" => {
                painter.line_segment([Pos2::new(a + 1.5, y + 3.0), Pos2::new(b - 1.5, y - 3.0)], Stroke::new(3.0, color));
            }
            "Stitch Double" => {
                for off in [-4.0, 4.0] {
                    painter.line_segment([Pos2::new(a + 2.0, y + off), Pos2::new(b - 2.0, y + off)], stroke);
                }
            }
            "Stitch Needle Holes" => {
                painter.circle_filled(Pos2::new(mid, y), 2.2, color);
            }
            _ => {
                // Straight; Staggered puts every second stitch on the other side of the line.
                let off = if s.alternate != 0.0 { if i % 2 == 0 { -3.0 } else { 3.0 } } else { 0.0 };
                painter.line_segment([Pos2::new(a + 2.0, y + off), Pos2::new(b - 2.0, y + off)], stroke);
            }
        }
    }
}

/// The stitch row: a tile for the plain line and one per stitch, the current one outlined.
/// Returns the tile that was clicked.
fn stitch_row(ui: &mut Ui, current: Option<&'static subcraft_app::stitch::Stitch>) -> Option<Option<&'static subcraft_app::stitch::Stitch>> {
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        let options = std::iter::once(None).chain(subcraft_app::stitch::STITCHES.iter().map(Some));
        for option in options {
            let selected = option.map(|s| s.name) == current.map(|s| s.name);
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(44.0, 30.0), egui::Sense::click());
            let (name, about) = option.map_or(("Plain line", "A continuous stroke of the brush along the path"), |s| (s.name, s.description));
            let resp = resp.on_hover_text(format!("{name}\n{about}"));
            let bg = if selected { theme::BG_3() } else if resp.hovered() { theme::BG_2() } else { theme::BG_1() };
            ui.painter().rect_filled(rect, egui::CornerRadius::same(theme::radius()), bg);
            let outline = if selected { Stroke::new(1.5, theme::ACCENT()) } else { Stroke::new(1.0, theme::TEXT_FAINT().gamma_multiply(0.5)) };
            ui.painter().rect_stroke(rect, egui::CornerRadius::same(theme::radius()), outline, egui::StrokeKind::Inside);
            stitch_icon(ui.painter(), rect, option, if selected { theme::ACCENT() } else { theme::TEXT() });
            if resp.clicked() {
                picked = Some(option);
            }
        }
    });
    picked
}

/// A stitch needs the Height channel to be raised: say so when the texture set has none, with
/// a button that adds it.
fn height_note(state: &mut UiState, ui: &mut Ui, now: f64) {
    if state.session.channels().contains(&subcraft_core::ChannelKind::Height) {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("This texture set has no Height channel, so stitches are flat colour.").small().color(theme::WARNING()));
        if ui.small_button("Add Height").on_hover_text("Add the Height channel to this texture set, so stitches are raised").clicked() {
            let texture_set = state.session.selected_texture_set().id;
            match state.session.execute(subcraft_app::Command::AddChannel { texture_set, kind: subcraft_core::ChannelKind::Height }) {
                Ok(()) => state.set_status("Height channel added: choose the stitch again to raise this path", now),
                Err(e) => state.set_status(e.to_string(), now),
            }
        }
    });
}

/// Properties of the Pen tool before a path exists (the selected layer is not a path): what
/// the next path is drawn with.
pub fn tool_section(state: &mut UiState, ui: &mut Ui, now: f64) {
    ui.label(egui::RichText::new("Draw with").color(theme::TEXT_DIM()));
    let current = state.pen.style.map(|s| s.stitch);
    if let Some(choice) = stitch_row(ui, current) {
        choose_stitch(state, choice, false, now);
    }
    let name = state.pen.style.map_or("Plain line", |s| s.stitch.name);
    ui.label(egui::RichText::new(name).strong());
    if state.pen.style.is_some() {
        height_note(state, ui, now);
    }
    crate::widgets::hint(ui, "Choose a stitch, then click on the model to place the points of the path (drag for a curve). Enter finishes the path. Width, spacing, thread colour and height can be changed afterwards, here.");
}

pub fn properties(state: &mut UiState, ui: &mut Ui, path: &PenPath, now: f64) {
    // What the path is drawn with: a plain line or a stitch.
    let current = stitch_of(state, path);
    if let Some(choice) = stitch_row(ui, current) {
        choose_stitch(state, choice, true, now);
        return;
    }
    ui.label(egui::RichText::new(current.map_or(if path.stamp.is_some() { "Stamped tip" } else { "Plain line" }, |s| s.name)).strong());
    if current.is_some() && !path.values.contains_key(&subcraft_core::ChannelKind::Height) {
        height_note(state, ui, now);
    }
    ui.horizontal_wrapped(|ui| {
        if ui.button("New path").clicked() { state.pen.new_path(&state.session); state.tool = Tool::Pen; }
        let selected = state.pen.selected_anchor().is_some();
        for (label, smooth) in [("Corner", false), ("Smooth", true)] {
            if ui.add_enabled(selected, egui::Button::new(label)).clicked() && let Err(e) = state.pen.shape_point(&state.session, smooth) { state.set_status(e.to_string(), now); }
        }
        if ui.add_enabled(selected, egui::Button::new("Delete point")).clicked() && let Err(e) = state.pen.delete_anchor(&state.session) { state.set_status(e.to_string(), now); }
        if ui.add_enabled(path.anchors.len() > 1, egui::Button::new("Reverse")).on_hover_text("Run the path the other way: stamps and a ribbon's image start from the other end").clicked()
            && let Err(e) = state.pen.reverse_path(&state.session) { state.set_status(e.to_string(), now); }
    });
    // Painter's path display settings.
    egui::CollapsingHeader::new("Display").id_salt("pen_display").default_open(false).show(ui, |ui| {
        let d = &mut state.pen_display;
        let dd = PenDisplay::default();
        let r = ui.checkbox(&mut d.show, "Show paths").on_hover_text("Show the paths and their points in the views");
        reset_on(ui, r, &mut d.show, dd.show);
        slider_row_reset(ui, "Handle size", &mut d.handle_size, dd.handle_size, |v| egui::Slider::new(v, PenDisplay::HANDLE_SIZE));
        slider_row_reset(ui, "Path width", &mut d.path_width, dd.path_width, |v| egui::Slider::new(v, PenDisplay::PATH_WIDTH));
        crate::widgets::prop_row(ui, "Path colour", |ui| {
            let mut c = d.color();
            if crate::color_picker::swatch_color32(ui, &mut c).changed() {
                d.path_color = Some(c);
            }
            if d.path_color.is_some() && ui.small_button("Theme").on_hover_text("Back to the theme's accent colour").clicked() {
                d.path_color = None;
            }
        });
        prop_row_reset(ui, "Other paths", &mut d.unselected_color, dd.unselected_color, |ui, c| {
            crate::color_picker::swatch_color32(ui, c).on_hover_text("Colour of the texture set's other paths")
        });
        ui.horizontal(|ui| {
            let r = ui.checkbox(&mut d.tangents, "Tangents");
            reset_on(ui, r, &mut d.tangents, dd.tangents);
            let r = ui.checkbox(&mut d.normals, "Normals");
            reset_on(ui, r, &mut d.normals, dd.normals);
            let r = ui.checkbox(&mut d.direction, "Direction");
            reset_on(ui, r, &mut d.direction, dd.direction);
        });
    });
    // Painter's magnet menu: snap new and moved points to the model.
    crate::widgets::prop_row(ui, "Snap", |ui| {
        let s = &mut state.pen_snap;
        let r = ui.toggle_value(&mut s.vertices, "Vertices").on_hover_text("New and moved points jump to the model's vertices near the pointer");
        reset_on(ui, r, &mut s.vertices, false);
        let r = ui.toggle_value(&mut s.edges, "Edges").on_hover_text("New and moved points jump onto the model's edges near the pointer");
        reset_on(ui, r, &mut s.edges, false);
        let r = ui.toggle_value(&mut s.edge_centres, "Edge centres").on_hover_text("New and moved points jump to the centres of the model's edges near the pointer");
        reset_on(ui, r, &mut s.edge_centres, false);
    });
    slider_row_reset(ui, "Snap angle", &mut state.pen_snap.angle_deg, PenSnap::default().angle_deg, |v| egui::Slider::new(v, PenSnap::ANGLE).suffix("\u{00b0}"))
        .on_hover_text("Ctrl while adding a point: the direction from the previous point snaps to steps of this angle");
    let mut path = path.clone(); let mut changed = false;
    // Painter's path types: a stroke along the curve, or its filled interior.
    changed |= prop_row_reset(ui, "Path type", &mut path.kind, subcraft_doc::PathKind::default(), |ui, v| {
        combo(ui, "pen_path_kind", v, &subcraft_doc::PathKind::ALL, |k| k.label())
    }).changed();
    let filled = path.kind == subcraft_doc::PathKind::Filled;
    if path.kind == subcraft_doc::PathKind::Ribbon {
        changed |= ui.add_enabled(path.anchors.len() > 1, egui::Checkbox::new(&mut path.closed, "Closed path")).changed();
        let mut width = path.radius * 200.0;
        changed |= crate::widgets::slider_row(ui, "Width (% UV)", egui::Slider::new(&mut width, 0.01..=100.0).logarithmic(true)).changed();
        path.radius = width / 200.0;
        changed |= ribbon_rows(state, ui, &mut path);
    } else if filled {
        ui.label(egui::RichText::new("The interior is filled; the path closes on its first point").small().color(crate::theme::TEXT_DIM()));
    } else {
        changed |= ui.add_enabled(path.anchors.len() > 1, egui::Checkbox::new(&mut path.closed, "Closed path")).changed();
        let mut width = path.radius * 200.0;
        changed |= crate::widgets::slider_row(ui, "Width (% UV)", egui::Slider::new(&mut width, 0.01..=100.0).logarithmic(true)).changed();
        path.radius = width / 200.0;
        changed |= crate::widgets::slider_row(ui, "Hardness", egui::Slider::new(&mut path.hardness, 0.0..=1.0)).changed();
        // Brush tip stamped along the path, with spacing (Painter's brush on a path).
        let mut stamped = path.stamp.is_some();
        if ui.checkbox(&mut stamped, "Stamp the brush tip").on_hover_text("Place the current brush tip along the path instead of one continuous stroke").changed() {
            changed = true;
            path.stamp = stamped.then(|| subcraft_doc::PathStamp { alpha: state.session.brush_tip_resource(), ..Default::default() });
        }
        if let Some(stamp) = path.stamp.as_mut() {
            let ds = subcraft_doc::PathStamp::default();
            let mut spacing = stamp.spacing * 100.0;
            changed |= slider_row_reset(ui, "Spacing (%)", &mut spacing, ds.spacing * 100.0, |v| egui::Slider::new(v, 2.0..=400.0).logarithmic(true)).changed();
            stamp.spacing = (spacing / 100.0).clamp(*subcraft_doc::PathStamp::SPACING.start(), *subcraft_doc::PathStamp::SPACING.end());
            let r = ui.checkbox(&mut stamp.follow, "Follow path").on_hover_text("Turn each stamp along the curve");
            changed |= reset_on(ui, r, &mut stamp.follow, ds.follow).changed();
            changed |= slider_row_reset(ui, "Alternate offset", &mut stamp.alternate, ds.alternate, |v| egui::Slider::new(v, subcraft_doc::PathStamp::ALTERNATE))
                .on_hover_text("Every second stamp moves across the path by this fraction of half the width")
                .changed();
            let tip = stamp.alpha.and_then(|id| state.session.resources().get(id)).map_or("Round".to_string(), |r| r.name.clone());
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("Tip: {tip}")).color(crate::theme::TEXT_DIM()));
                if ui.small_button("Use current brush tip").on_hover_text("Stamp the tip chosen in the Brush settings / Shelf").clicked() {
                    stamp.alpha = state.session.brush_tip_resource();
                    changed = true;
                }
            });
        }
    }
    changed |= crate::widgets::slider_row(ui, "Opacity", egui::Slider::new(&mut path.opacity, 0.0..=1.0)).changed();
    for kind in state.session.channels() {
        let mut enabled = path.values.contains_key(&kind);
        let editable = !enabled || path.values.len() > 1;
        let r = ui.add_enabled(editable, egui::Checkbox::new(&mut enabled, kind.display_name())).on_hover_text("Choose the channels this path paints. Hide the layer to hide all channels.");
        if r.changed() {
            changed = true;
            if enabled { path.values.insert(kind, kind.default_paint_value()); }
            else { path.values.remove(&kind); }
        }
        if let Some(value) = path.values.get_mut(&kind) {
            crate::widgets::prop_row(ui, "Value", |ui| { changed |= crate::widgets::channel_value(ui, kind, value).changed; });
        }
    }
    if changed {
        if let Err(e) = state.pen.set_path(&state.session, path) { state.set_status(e.to_string(), now); }
        if !ui.input(|i| i.pointer.primary_down()) && let Err(e) = state.pen.release(&state.session) { state.set_status(e.to_string(), now); }
    }
    crate::widgets::hint(ui, "Click to add a point; drag on creation for Bézier handles. Drag points or handles to edit; Shift breaks the mirrored handle. Enter finishes, Esc cancels an edit, Delete removes a point. Middle/Alt drag pans in 2D.");
}

#[cfg(test)]
mod snap_tests {
    use super::*;

    #[test]
    fn direction_marks_follow_the_line_at_their_spacing() {
        // A straight 200-point line to the right: arrows every 60 from 30, pointing right.
        let line = [Pos2::new(0.0, 0.0), Pos2::new(120.0, 0.0), Pos2::new(200.0, 0.0)];
        let marks = direction_marks(&line, 60.0, 7.0);
        let tips: Vec<f32> = marks.iter().map(|m| m[0].x).collect();
        assert_eq!(tips, vec![30.0, 90.0, 150.0], "spacing carries across segments");
        assert!(marks.iter().all(|[tip, a, b]| a.x < tip.x && b.x < tip.x && (a.y + b.y).abs() < 1e-4), "pointing along the line");
        assert!(direction_marks(&[Pos2::ZERO], 60.0, 7.0).is_empty());
        let d = PenDisplay::default();
        assert!(d.show && d.tangents && !d.normals && !d.direction && d.path_color.is_none());
    }

    #[test]
    fn snapping_prefers_corners_and_centres_then_edges_within_the_radius() {
        let tri = [Pos2::new(0.0, 0.0), Pos2::new(100.0, 0.0), Pos2::new(0.0, 100.0)];
        let all = PenSnap { vertices: true, edges: true, edge_centres: true, ..PenSnap::default() };
        let near = |got: Option<Pos2>, want: Pos2| got.is_some_and(|g| g.distance(want) < 1e-3);
        assert!(near(snap_to_triangle(Pos2::new(4.0, 3.0), tri, all), Pos2::new(0.0, 0.0)), "corner");
        assert!(near(snap_to_triangle(Pos2::new(52.0, 6.0), tri, all), Pos2::new(50.0, 0.0)), "edge centre");
        assert!(near(snap_to_triangle(Pos2::new(30.0, 5.0), tri, all), Pos2::new(30.0, 0.0)), "onto the edge");
        assert_eq!(snap_to_triangle(Pos2::new(30.0, 30.0), tri, all), None, "nothing near");
        // Only what is enabled.
        let edges_only = PenSnap { edges: true, ..Default::default() };
        assert!(near(snap_to_triangle(Pos2::new(4.0, 3.0), tri, edges_only), Pos2::new(4.0, 0.0)));
        assert_eq!(snap_to_triangle(Pos2::new(4.0, 3.0), tri, PenSnap::default()), None);
        assert!(!PenSnap::default().any() && all.any());
        // The angle step: 15 degrees by default, kept in range.
        assert!((PenSnap::default().angle() - 15f32.to_radians()).abs() < 1e-6);
        assert!((PenSnap { angle_deg: 400.0, ..PenSnap::default() }.angle() - 90f32.to_radians()).abs() < 1e-6);
        assert!((PenSnap { angle_deg: f32::NAN, ..PenSnap::default() }.angle() - 15f32.to_radians()).abs() < 1e-6);
    }
}

#[cfg(test)]
mod keyboard_tests {
    use super::*;

    fn drawn_path(state: &UiState) -> PenPath {
        let subcraft_doc::LayerContent::Paint(p) = &state.session.selected_layer().unwrap().content else { panic!("a paint layer") };
        p.path.clone().expect("a path layer")
    }

    fn click(state: &mut UiState, uv: Vec2) {
        refresh_style(state);
        state.pen.begin(&state.session, uv, None).unwrap();
        state.pen.release(&state.session).unwrap();
        while state.pen.poll(&mut state.session, 1 << 20).unwrap() {}
    }

    /// Stitches with the Pen: choose the stitch first (the row in Properties or a Stitch brush
    /// in the Shelf), then click. A Stitch brush clicked in the Shelf used to switch the Pen to
    /// the Brush tool, and the only way to a stitched path was a "Choose..." menu on a finished one.
    #[test]
    fn a_stitch_is_chosen_before_drawing_and_from_the_shelf() {
        let mut state = UiState::new(subcraft_app::Session::sample(), None);
        state.tool = Tool::Pen;
        let saddle = subcraft_app::stitch::find("Stitch: Saddle").unwrap();
        choose_stitch(&mut state, Some(saddle), false, 0.0);
        assert_eq!(state.pen.style.map(|s| s.stitch.name), Some("Stitch: Saddle"));
        click(&mut state, Vec2::new(0.2, 0.3));
        click(&mut state, Vec2::new(0.6, 0.3));
        let path = drawn_path(&state);
        assert_eq!(stitch_of(&state, &path).map(|s| s.name), Some("Stitch: Saddle"));
        assert_eq!(path.anchors.len(), 2);

        // A Stitch brush from the Shelf while the Pen is in use: the Pen stays, and the path
        // being drawn takes the stitch.
        let cross = state.brush_presets.iter().find(|p| p.name == "Stitch: Cross").cloned().expect("the stitch brushes are in the Shelf");
        state.select_shelf_brush(subcraft_app::brush_preset::ShelfBrush::Preset(&cross), 0.0);
        while state.pen.poll(&mut state.session, 1 << 20).unwrap() {}
        assert_eq!(state.tool, Tool::Pen, "the Pen is still the tool");
        let path = drawn_path(&state);
        assert_eq!(stitch_of(&state, &path).map(|s| s.name), Some("Stitch: Cross"));
        assert_eq!(path.anchors.len(), 2, "the points stay");

        // A finished path (Enter) is left alone by the next choice; the next path takes it.
        state.pen.finish_path();
        let zigzag = state.brush_presets.iter().find(|p| p.name == "Stitch: Zigzag").cloned().unwrap();
        state.select_shelf_brush(subcraft_app::brush_preset::ShelfBrush::Preset(&zigzag), 0.0);
        assert_eq!(stitch_of(&state, &drawn_path(&state)).map(|s| s.name), Some("Stitch: Cross"), "the finished path keeps its stitch");
        click(&mut state, Vec2::new(0.2, 0.7));
        assert_eq!(stitch_of(&state, &drawn_path(&state)).map(|s| s.name), Some("Stitch: Zigzag"));

        // Plain line on the selected path: the stamp goes, and new paths are plain again.
        choose_stitch(&mut state, None, true, 0.0);
        while state.pen.poll(&mut state.session, 1 << 20).unwrap() {}
        assert_eq!(drawn_path(&state).stamp, None);
        assert!(state.pen.style.is_none());

        // Any other brush from the Shelf still goes to the Brush tool.
        let other = state.brush_presets.iter().find(|p| !p.name.starts_with("Stitch")).cloned().unwrap();
        state.select_shelf_brush(subcraft_app::brush_preset::ShelfBrush::Preset(&other), 0.0);
        assert_ne!(state.tool, Tool::Pen, "{}", other.name);
    }

    /// The stitch choice outlives the project it was made in: its tip becomes a resource of
    /// the project that is open at the next click.
    #[test]
    fn the_chosen_stitch_follows_a_new_project() {
        let mut state = UiState::new(subcraft_app::Session::sample(), None);
        state.tool = Tool::Pen;
        choose_stitch(&mut state, subcraft_app::stitch::find("Stitch: Double"), false, 0.0);
        let old = state.pen.style.unwrap().tip.unwrap();
        state.session = subcraft_app::Session::sample();
        assert!(state.session.resources().get(old).is_none());
        click(&mut state, Vec2::new(0.3, 0.3));
        let path = drawn_path(&state);
        assert_eq!(stitch_of(&state, &path).map(|s| s.name), Some("Stitch: Double"));
    }

    /// The Pen tool's Properties show the stitch row before any path exists, and on a path.
    #[test]
    fn the_stitch_row_is_drawn_with_and_without_a_path() {
        let mut state = UiState::new(subcraft_app::Session::sample(), None);
        state.tool = Tool::Pen;
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let frame = |state: &mut UiState| {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| crate::panels::properties::show(state, ui));
            out.textures_delta.clear();
        };
        frame(&mut state);
        choose_stitch(&mut state, subcraft_app::stitch::find("Stitch: Straight"), false, 0.0);
        frame(&mut state);
        click(&mut state, Vec2::new(0.3, 0.3));
        click(&mut state, Vec2::new(0.5, 0.3));
        frame(&mut state);
        assert_eq!(stitch_of(&state, &drawn_path(&state)).map(|s| s.name), Some("Stitch: Straight"), "drawing the panel changes nothing");
    }

    /// QA 2026-11: the pen keys (Enter, Escape, Delete, Backspace) had no modal check: they
    /// acted behind the unsaved-changes prompt and New Project, and took the dialog's Enter.
    #[test]
    fn pen_keys_do_nothing_behind_a_modal_window() {
        let mut state = UiState::new(subcraft_app::Session::sample(), None);
        state.tool = Tool::Pen;
        let ctx = egui::Context::default();
        let enter_left = |state: &mut UiState| {
            let events = vec![egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }];
            let mut left = None;
            let mut out = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                keyboard(state, ui.ctx(), 0.0);
                left = Some(ui.input(|i| i.key_pressed(egui::Key::Enter)));
            });
            out.textures_delta.clear();
            left.unwrap()
        };
        state.confirm = Some(crate::dialogs::Pending::Close);
        assert!(enter_left(&mut state), "the dialog keeps its Enter");
        state.confirm = None;
        state.new_project.open = true;
        assert!(enter_left(&mut state), "New Project keeps its Enter");
        state.new_project.open = false;
        assert!(!enter_left(&mut state), "without a modal window the pen takes Enter");
    }
}
