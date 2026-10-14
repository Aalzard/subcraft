//! Whole-model parts catalogue. All geometry and editing logic lives in app.

use egui::{RichText, Ui};
use subcraft_app::{PartAction, SurfaceTarget};
use subcraft_core::PartKind;
use subcraft_doc::GeneratorKind;
use crate::{state::{Tool, UiState}, theme};

/// Same actions in the model's context menu and the parts list.
pub fn menu(ui: &mut Ui, target: SurfaceTarget, has_layer: bool) -> Option<PartAction> {
    let mut action = None;
    let label = if target.part.kind == PartKind::UvIsland { "UV island" } else { "part" };
    for (name, value) in [(format!("Fill this {label}"), PartAction::Fill), (format!("Paint only on this {label}"), PartAction::PaintOnly)] {
        if ui.button(name).clicked() { action = Some(value); ui.close(); }
    }
    ui.add_enabled_ui(has_layer, |ui| {
        if ui.button(format!("Mask selected layer to this {label}")).clicked() { action = Some(PartAction::MaskLayer); ui.close(); }
        ui.menu_button("Add generator as mask", |ui| {
            for (label, kind) in [("Edge wear", GeneratorKind::Edges), ("Dirt", GeneratorKind::Cavity), ("Directional", GeneratorKind::Directional), ("Position gradient", GeneratorKind::PositionGradient)] {
                if ui.button(label).clicked() { action = Some(PartAction::Generator(kind)); ui.close(); }
            }
        });
    });
    ui.separator();
    for (label, value) in [("Pick color", PartAction::PickColor), ("Pick material", PartAction::PickMaterial), ("Isolate / restore (I)", PartAction::Isolate), ("Hide / show (H)", PartAction::Hide)] {
        if ui.button(label).clicked() { action = Some(value); ui.close(); }
    }
    action
}

pub fn apply(state: &mut UiState, target: SurfaceTarget, action: PartAction, now: f64) {
    match state.session.apply_part_action(target, action) {
        Ok(()) => {
            if action == PartAction::PaintOnly { state.tool = Tool::Brush; }
            // Sampling evaluates composites; force a refresh so it cannot consume pending uploads.
            if matches!(action, PartAction::PickColor | PartAction::PickMaterial) { state.gpu_dirty = true; }
            state.set_status(action.label(), now);
        }
        Err(e) => state.set_status(format!("Parts: {e}"), now),
    }
}

pub fn show(state: &mut UiState, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    egui::Frame::new().inner_margin(egui::Margin::same(10)).show(ui, |ui| {
        ui.label(RichText::new("Parts").strong());
        egui::ComboBox::from_id_salt("parts-kind").selected_text(state.parts_kind.label()).show_ui(ui, |ui| {
            for kind in PartKind::ALL { ui.selectable_value(&mut state.parts_kind, kind, kind.label()); }
        });
        if state.session.has_hidden_parts() && ui.button("Show all parts").clicked() { state.session.show_all_parts(); }
        ui.label(RichText::new("Right-click a part to texture it. I isolates, H hides the part under the cursor.").small().color(theme::TEXT_FAINT));
        let parts = state.session.parts(state.parts_kind);
        let mut request = None;
        egui::ScrollArea::vertical().id_salt("model-parts-list").max_height(230.0).min_scrolled_height(0.0).show(ui, |ui| {
            if parts.is_empty() { ui.label("No imported object names. Try connected pieces or UV islands."); }
            for part in parts {
                let key = part.key();
                let visible = state.session.part_visible(key);
                let resp = ui.selectable_label(state.hovered_part == Some(key), RichText::new(&part.name).color(if visible { theme::TEXT } else { theme::TEXT_FAINT }));
                let materials: Vec<_> = part.materials.iter().map(|&m| format!("{} ({m})", state.session.scene().mesh.materials[m as usize].name)).collect();
                resp.clone().on_hover_text(format!("{} triangles · {}", part.triangles, materials.join(", ")));
                if resp.hovered() { state.hovered_part = Some(key); }
                resp.context_menu(|ui| {
                    if let Some(target) = state.session.target_for_part(key) && let Some(action) = menu(ui, target, state.session.selection().layer.is_some() && state.session.selection().texture_set == target.texture_set) { request = Some((target, action)); }
                });
            }
        });
        if let Some((target, action)) = request { apply(state, target, action, now); }
    });
}
