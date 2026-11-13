//! Whole-model parts catalogue. All geometry and editing logic lives in app.

use crate::i18n::tr;
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
        if ui.button(tr("Mask by ID color")).on_hover_text(tr("Mask the selected layer to the baked ID map color under the cursor")).clicked() {
            action = Some(PartAction::MaskByIdColor); ui.close();
        }
        ui.menu_button(tr("Add generator as mask"), |ui| {
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
        ui.label(RichText::new(tr("Parts")).strong());
        egui::ComboBox::from_id_salt("parts-kind").selected_text(state.parts_kind.label()).show_ui(ui, |ui| {
            for kind in PartKind::ALL { ui.selectable_value(&mut state.parts_kind, kind, kind.label()); }
        });
        if state.session.has_hidden_parts() && ui.button(tr("Show all parts")).clicked() { state.session.show_all_parts(); }
        ui.label(RichText::new(tr("Right-click a part to texture it. I isolates, H hides the part under the cursor.")).small().color(theme::TEXT_FAINT()));
        let parts = state.session.parts(state.parts_kind);
        let mut request = None;
        egui::ScrollArea::vertical().id_salt("model-parts-list").max_height(230.0).min_scrolled_height(0.0).show(ui, |ui| {
            if parts.is_empty() { ui.label(tr("No imported object names. Try connected pieces or UV islands.")); }
            ui.spacing_mut().item_spacing.y = 1.0;
            for (index, part) in parts.into_iter().enumerate() {
                let key = part.key();
                let visible = state.session.part_visible(key);
                let resp = crate::widgets::striped_list_row(ui, index, false, egui::Sense::click(), |ui| {
                    let icon = if visible { crate::icons::CUBE } else { crate::icons::EYE_SLASH };
                    ui.label(RichText::new(icon).font(theme::font(14.0)).color(theme::TEXT_FAINT()));
                    ui.add(egui::Label::new(RichText::new(&part.name).color(if visible { theme::TEXT() } else { theme::TEXT_FAINT() })).truncate().selectable(false));
                });
                let materials: Vec<_> = part.materials.iter().map(|&m| format!("{} ({m})", state.session.scene().mesh.materials[m as usize].name)).collect();
                resp.clone().on_hover_text(format!("{} triangles · {}", part.triangles, materials.join(", ")));
                resp.context_menu(|ui| {
                    if let Some(target) = state.session.target_for_part(key) && let Some(action) = menu(ui, target, state.session.selection().layer.is_some() && state.session.selection().texture_set == target.texture_set) { request = Some((target, action)); }
                });
            }
        });
        if let Some((target, action)) = request { apply(state, target, action, now); }
    });
}

/// The geometry mask bar of the 3D and 2D views while a layer's geometry mask is edited: what the
/// mask lists (connected pieces, UV islands, objects, material IDs), the included count, All /
/// None / Invert, and Done. Choosing a kind starts a mask of that kind with everything included.
pub fn mask_bar(ui: &mut Ui, state: &mut UiState) {
    use subcraft_doc::PartMask;
    let Some((id, parts)) = state.session.selected_layer().map(|l| (l.id, l.parts.clone())) else { return };
    let slot = state.session.selected_texture_set().material_slot;
    let now = ui.input(|i| i.time);
    let current = parts.as_ref().map(|p| p.kind);
    let mut start = None;
    let mut set = None;
    ui.label(RichText::new(format!("{}  Geometry mask by", crate::icons::POLYGON)).color(theme::ACCENT()));
    for kind in PartKind::ALL {
        let tip = match kind {
            PartKind::Connected => "Connected pieces of the mesh",
            PartKind::UvIsland => "UV islands",
            PartKind::Object => "Named objects of the model",
            PartKind::Material => "Material IDs",
        };
        if ui.selectable_label(current == Some(kind), kind.label()).on_hover_text(tip).clicked() && current != Some(kind) {
            start = Some(kind);
        }
    }
    match &parts {
        Some(p) => {
            let all: std::collections::BTreeSet<u32> = state.session.parts(p.kind).iter().filter(|i| i.materials.contains(&slot)).map(|i| i.id).collect();
            let shown = all.iter().filter(|part| p.parts.contains(part)).count();
            ui.separator();
            ui.label(RichText::new(format!("{shown} / {}", all.len())).color(theme::TEXT_DIM())).on_hover_text("Included parts of this texture set");
            if ui.small_button(tr("All")).clicked() { set = Some(PartMask::for_kind(p.kind, all.clone())); }
            if ui.small_button(tr("None")).clicked() { set = Some(PartMask::for_kind(p.kind, [])); }
            if ui.small_button(tr("Invert")).clicked() { set = Some(PartMask::for_kind(p.kind, all.difference(&p.parts).copied())); }
        }
        None => {
            ui.label(RichText::new("click a part or drag a box in the view").color(theme::TEXT_DIM()));
        }
    }
    ui.separator();
    if ui.button(format!("{}  Done", crate::icons::CHECK)).on_hover_text("Leave geometry mask editing (Esc)").clicked() {
        state.picking_parts = false;
    }
    let result = match (start, set) {
        (Some(kind), _) => state.session.begin_layer_parts(id, kind),
        (None, Some(mask)) => state.session.execute(subcraft_app::Command::SetParts { layer: id, parts: Some(mask) }),
        _ => Ok(()),
    };
    if let Err(e) = result {
        state.set_status(format!("Parts: {e}"), now);
    }
}

