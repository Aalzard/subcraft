//! Mesh > Edit UV islands: pick islands in the 2D view, move / rotate / scale / flip them with a
//! live preview, then apply. The work is `subcraft_app::uv_edit` (background job, the painted work
//! is reprojected onto the edited layout like Auto UV).

use std::collections::BTreeSet;

use egui::{Color32, Pos2, RichText, Stroke};
use glam::Vec2;
use subcraft_app::{MeshReloadJob, UvEditOutput};
use subcraft_mesh::uv_edit::{UvPivot, UvTransform};

use crate::i18n::tr;
use crate::panels::uv_view::Frame;
use crate::state::UiState;
use crate::theme::{self, TEXT_DIM};
use crate::widgets::{caption, hint, primary_button, secondary_button};

#[derive(Default)]
pub struct UvEditUi {
    pub open: bool,
    /// UV island of every triangle, computed when the window opens (and again after the mesh changes).
    islands: Vec<u32>,
    /// The mesh `islands` belongs to (its address), to notice a reload.
    mesh_key: usize,
    pub selected: BTreeSet<u32>,
    pub transform: UvTransform,
    /// Scale both axes together.
    pub uniform: bool,
    pub job: Option<MeshReloadJob<UvEditOutput>>,
    pub error: Option<String>,
    /// Where a box drag in the 2D view began.
    pub box_start: Option<Pos2>,
    /// Cached overlay: (texture set, selection, transform) -> (selected triangles now, after).
    preview_key: Option<(subcraft_core::TextureSetId, BTreeSet<u32>, [u32; 8])>,
    now_tris: Vec<[Vec2; 3]>,
    moved_tris: Vec<[Vec2; 3]>,
}

impl UvEditUi {
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// True while the window takes the 2D view's clicks.
    pub fn picking(&self) -> bool {
        self.open && !self.busy()
    }
}

fn transform_key(t: &UvTransform) -> [u32; 8] {
    [
        t.offset.x.to_bits(),
        t.offset.y.to_bits(),
        t.rotation_deg.to_bits(),
        t.scale.x.to_bits(),
        t.scale.y.to_bits(),
        t.flip_u as u32,
        t.flip_v as u32,
        (t.pivot == UvPivot::EachIsland) as u32,
    ]
}

fn mesh_key(state: &UiState) -> usize {
    std::sync::Arc::as_ptr(&state.session.scene().mesh) as usize
}

/// Recompute the islands after a mesh change (the old selection means nothing then).
fn refresh(state: &mut UiState) {
    let key = mesh_key(state);
    if state.uv_edit.mesh_key != key || state.uv_edit.islands.is_empty() {
        state.uv_edit.islands = state.session.uv_islands();
        state.uv_edit.mesh_key = key;
        state.uv_edit.selected.clear();
        state.uv_edit.preview_key = None;
    }
}

pub fn open(state: &mut UiState) {
    let e = &mut state.uv_edit;
    if !e.open {
        e.open = true;
        e.error = None;
        e.transform = UvTransform::default();
        e.uniform = true;
    }
    refresh(state);
}

fn start(state: &mut UiState) {
    if state.session.is_stroking() {
        state.uv_edit.error = Some("finish the stroke first".into());
        return;
    }
    match state.session.start_uv_edit(state.uv_edit.selected.clone(), state.uv_edit.transform) {
        Ok(job) => {
            state.uv_edit.error = None;
            state.uv_edit.job = Some(job);
        }
        Err(e) => state.uv_edit.error = Some(e),
    }
}

/// Poll the background edit (call every frame).
pub fn poll(state: &mut UiState, ctx: &egui::Context, now: f64) {
    let Some(job) = &mut state.uv_edit.job else { return };
    ctx.request_repaint();
    let Some(result) = job.poll() else { return };
    state.uv_edit.job = None;
    match result.and_then(|out| state.session.finish_uv_edit(out)) {
        Ok(summary) => {
            state.mesh_reloaded();
            state.set_status(summary, now);
            // Ready for the next edit of the same islands.
            let selected = std::mem::take(&mut state.uv_edit.selected);
            refresh(state);
            state.uv_edit.selected = selected;
            // The pivot choice stays for the next edit.
            state.uv_edit.transform = UvTransform { pivot: state.uv_edit.transform.pivot, ..UvTransform::default() };
        }
        Err(e) => state.uv_edit.error = Some(e),
    }
}

pub fn window(state: &mut UiState, ctx: &egui::Context, now: f64) {
    poll(state, ctx, now);
    if !state.uv_edit.open {
        return;
    }
    refresh(state);
    let all = state.session.uv_islands_of_texture_set(&state.uv_edit.islands);
    let mut open = true;
    let (mut run, mut cancel) = (false, false);
    egui::Window::new(RichText::new(tr("Edit UV islands")).font(theme::font_strong(14.0))).open(&mut open).collapsible(false).default_width(380.0).show(ctx, |ui| {
        let e = &mut state.uv_edit;
        let busy = e.busy();
        ui.label(RichText::new(tr("Click islands in the 2D view to select them (drag a box to add, Ctrl+drag to remove), then move, turn or scale them. Painted layers move with them.")).color(TEXT_DIM()));
        ui.add_space(6.0);
        ui.add_enabled_ui(!busy, |ui| {
            ui.horizontal(|ui| {
                let picked = e.selected.intersection(&all).count();
                ui.label(format!("{picked} / {} {}", all.len(), tr("islands selected")));
                if ui.small_button(tr("All")).clicked() {
                    e.selected = all.clone();
                }
                if ui.small_button(tr("None")).clicked() {
                    e.selected.clear();
                }
            });
            ui.add_space(4.0);
            let t = &mut e.transform;
            egui::Grid::new("uv_edit_transform").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label(caption(tr("Move")));
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut t.offset.x).speed(0.002).fixed_decimals(3).prefix("U "));
                    ui.add(egui::DragValue::new(&mut t.offset.y).speed(0.002).fixed_decimals(3).prefix("V "));
                });
                ui.end_row();
                ui.label(caption(tr("Rotate")));
                ui.horizontal(|ui| {
                    if ui.small_button("-90°").clicked() {
                        t.rotation_deg -= 90.0;
                    }
                    ui.add(egui::DragValue::new(&mut t.rotation_deg).speed(0.5).suffix("°").fixed_decimals(1));
                    if ui.small_button("+90°").clicked() {
                        t.rotation_deg += 90.0;
                    }
                });
                ui.end_row();
                ui.label(caption(tr("Scale")));
                ui.horizontal(|ui| {
                    let before = t.scale;
                    ui.add(egui::DragValue::new(&mut t.scale.x).speed(0.005).range(0.01..=100.0).fixed_decimals(3).prefix("U "));
                    ui.add(egui::DragValue::new(&mut t.scale.y).speed(0.005).range(0.01..=100.0).fixed_decimals(3).prefix("V "));
                    if e.uniform {
                        if t.scale.x != before.x {
                            t.scale.y = t.scale.x;
                        } else if t.scale.y != before.y {
                            t.scale.x = t.scale.y;
                        }
                    }
                    ui.checkbox(&mut e.uniform, tr("Uniform"));
                });
                ui.end_row();
                ui.label(caption(tr("Flip")));
                ui.horizontal(|ui| {
                    ui.checkbox(&mut t.flip_u, "U");
                    ui.checkbox(&mut t.flip_v, "V");
                });
                ui.end_row();
                ui.label(caption(tr("Pivot")));
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut t.pivot, UvPivot::Selection, tr("Selection centre"));
                    ui.selectable_value(&mut t.pivot, UvPivot::EachIsland, tr("Each island"));
                });
                ui.end_row();
            });
            if ui.small_button(tr("Reset transform")).clicked() {
                *t = UvTransform { pivot: t.pivot, ..UvTransform::default() };
            }
        });
        ui.add_space(6.0);
        hint(ui, tr("This cannot be undone: the undo history is cleared and baked mesh maps are marked for rebaking. Save first to keep this version."));
        ui.add_space(8.0);
        if let Some(job) = &e.job {
            ui.add(egui::ProgressBar::new(job.progress().fraction()).text(job.progress().stage()));
            if secondary_button(ui, tr("Cancel")).clicked() {
                cancel = true;
            }
        } else {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let ready = !e.selected.is_empty() && !e.transform.is_identity();
                if ui.add_enabled_ui(ready, |ui| primary_button(ui, tr("Apply"))).inner.clicked() {
                    run = true;
                }
            });
        }
        if let Some(err) = &e.error {
            ui.add_space(6.0);
            ui.label(RichText::new(err).color(theme::WARNING()));
        }
    });
    if cancel && let Some(job) = state.uv_edit.job.take() {
        job.cancel();
    }
    if run {
        start(state);
    }
    if !open {
        state.uv_edit.open = false;
        state.uv_edit.job = None;
        state.uv_edit.box_start = None;
    }
}

/// The 2D view's clicks while the window is open: a click toggles the island under it, a box
/// adds every island it touches, Ctrl+box removes them.
pub fn input(state: &mut UiState, ui: &egui::Ui, response: &egui::Response, image: Frame) {
    refresh(state);
    let (pointer, pressed, down, mods) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.primary_pressed(), i.pointer.primary_down(), i.modifiers));
    let Some(start) = state.uv_edit.box_start else {
        if pressed && response.hovered() && !mods.alt && let Some(p) = pointer {
            state.uv_edit.box_start = Some(p);
        }
        return;
    };
    if down {
        return;
    }
    state.uv_edit.box_start = None;
    let end = pointer.unwrap_or(start);
    let e = &mut state.uv_edit;
    if crate::view_props::is_box_drag(start, end) {
        let touched = state.session.uv_islands_in_box(&e.islands, image.to_uv(start), image.to_uv(end));
        if mods.command || mods.ctrl {
            e.selected.retain(|i| !touched.contains(i));
        } else {
            e.selected.extend(touched);
        }
    } else if let Some(island) = state.session.uv_island_at(&e.islands, image.to_uv(start))
        && !e.selected.remove(&island)
    {
        e.selected.insert(island);
    }
}

/// Selected islands tinted where they are, and outlined where the transform puts them.
pub fn overlay(state: &mut UiState, painter: &egui::Painter, image: Frame) {
    if !state.uv_edit.open {
        return;
    }
    let ts = state.session.selected_texture_set().id;
    let key = Some((ts, state.uv_edit.selected.clone(), transform_key(&state.uv_edit.transform)));
    if key != state.uv_edit.preview_key {
        let e = &state.uv_edit;
        let now = state.session.uv_edit_triangles(&e.islands, &e.selected, None);
        let moved = if e.transform.is_identity() { Vec::new() } else { state.session.uv_edit_triangles(&e.islands, &e.selected, Some(&e.transform)) };
        let e = &mut state.uv_edit;
        (e.now_tris, e.moved_tris, e.preview_key) = (now, moved, key);
    }
    let accent = theme::ACCENT();
    let to_px = |uv: Vec2| image.to_px(uv);
    let mut mesh = egui::Mesh::default();
    let tint = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 110);
    for tri in &state.uv_edit.now_tris {
        let i = mesh.vertices.len() as u32;
        for p in tri {
            mesh.colored_vertex(to_px(*p), tint);
        }
        mesh.add_triangle(i, i + 1, i + 2);
    }
    painter.add(egui::Shape::mesh(mesh));
    let line = Stroke::new(1.0, Color32::from_rgb(255, 255, 255));
    for tri in &state.uv_edit.moved_tris {
        let p = tri.map(to_px);
        painter.add(egui::Shape::closed_line(p.to_vec(), line));
    }
}
