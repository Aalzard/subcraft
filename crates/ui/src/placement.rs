//! The Bake window's "Placement" section: line the high poly up with the low poly.
//!
//! Widgets only edit a [`BakePlacement`]; the maths lives in `subcraft_core::bake_placement` and
//! the transform is applied by the baker and the overlay, never to the imported file.

use egui::{RichText, Ui};
use subcraft_core::{BakePlacement, Box3, FitMode};

use crate::theme::{self, TEXT_DIM};
use crate::widgets::{hint, secondary_button};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlacementAction {
    CenterOnLow,
    PivotToLowOrigin,
    AutoFit,
    AutoAlign,
    ResetOffset,
    ResetScale,
    ResetRotation,
    ResetAll,
}

/// Apply one button press. Returns a short message when nothing could be done.
pub fn apply_action(p: &mut BakePlacement, action: PlacementAction, low: Box3, high: Option<Box3>, fit: FitMode) -> Option<&'static str> {
    const NO_HIGH: &str = "Load a high poly first.";
    const DEGENERATE: &str = "The high poly or low poly has no size to fit.";
    match action {
        PlacementAction::ResetOffset => p.offset = [0.; 3],
        PlacementAction::ResetScale => { p.scale = 1.; p.axis_scale = [1.; 3]; }
        PlacementAction::ResetRotation => p.rotation_deg = [0.; 3],
        PlacementAction::ResetAll => p.reset(),
        PlacementAction::PivotToLowOrigin => p.pivot_to_low_origin(),
        PlacementAction::CenterOnLow => p.center_on_low(),
        PlacementAction::AutoFit => {
            let Some(high) = high else { return Some(NO_HIGH) };
            match p.auto_fit_scale(low, high, fit) { Some(s) => p.scale = s, None => return Some(DEGENERATE) }
        }
        PlacementAction::AutoAlign => {
            let Some(high) = high else { return Some(NO_HIGH) };
            if !p.auto_align(low, high, fit) { return Some(DEGENERATE); }
        }
    }
    None
}

fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.label(RichText::new(label).color(TEXT_DIM()));
    ui.horizontal(add);
    ui.end_row();
}

/// Draws the section. `low` / `high` are the unplaced bounds; `high` is `None` until the file has loaded.
/// Returns true when the placement changed.
pub fn placement_section(ui: &mut Ui, p: &mut BakePlacement, low: Box3, high: Option<Box3>, fit: &mut FitMode, opacity: &mut f32, message: &mut Option<&'static str>) -> bool {
    let before = p.clone();
    let diag = (low.1 - low.0).length().max(1e-3);
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        if ui.button("Auto align").on_hover_text("Centre the high poly on the low poly and scale it to match, in one click").clicked() { action = Some(PlacementAction::AutoAlign); }
        if secondary_button(ui, "Center on low poly").on_hover_text("Move the high poly's bounding-box centre onto the low poly's bounding-box centre").clicked() { action = Some(PlacementAction::CenterOnLow); }
        if secondary_button(ui, "Pivot to low-poly origin").on_hover_text("Put the high poly's own origin on the low poly's origin").clicked() { action = Some(PlacementAction::PivotToLowOrigin); }
    });
    ui.add_space(2.0);
    egui::Grid::new("placement_grid").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
        row(ui, "Fit mode", |ui| {
            egui::ComboBox::from_id_salt("placement_fit").selected_text(fit.label()).show_ui(ui, |ui| {
                for m in FitMode::ALL { ui.selectable_value(fit, m, m.label()); }
            });
            if secondary_button(ui, "Auto-fit scale").on_hover_text("Set the scale so the high poly's box matches the low poly's box").clicked() { action = Some(PlacementAction::AutoFit); }
        });
        row(ui, "Scale", |ui| {
            ui.add(egui::Slider::new(&mut p.scale, 0.01..=10.0).logarithmic(true).clamping(egui::SliderClamping::Never));
            if ui.small_button("Reset").clicked() { action = Some(PlacementAction::ResetScale); }
        });
        for (i, axis) in ["Offset X (left / right)", "Offset Y (down / up)", "Offset Z (back / forward)"].into_iter().enumerate() {
            row(ui, axis, |ui| {
                ui.add(egui::Slider::new(&mut p.offset[i], -diag..=diag).clamping(egui::SliderClamping::Never).fixed_decimals(4));
                if ui.small_button("Reset").clicked() { action = Some(PlacementAction::ResetOffset); }
            });
        }
    });
    egui::CollapsingHeader::new("Per-axis scale and rotation").id_salt("placement_adv").show(ui, |ui| {
        egui::Grid::new("placement_adv_grid").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
            for (i, axis) in ["X", "Y", "Z"].into_iter().enumerate() {
                row(ui, &format!("Stretch {axis}"), |ui| { ui.add(egui::DragValue::new(&mut p.axis_scale[i]).speed(0.005).range(0.01..=100.0)); });
            }
            for (i, axis) in ["X", "Y", "Z"].into_iter().enumerate() {
                row(ui, &format!("Rotate {axis} (°)"), |ui| { ui.add(egui::Slider::new(&mut p.rotation_deg[i], -180.0..=180.0)); });
            }
            row(ui, "", |ui| {
                if ui.small_button("Reset rotation").clicked() { action = Some(PlacementAction::ResetRotation); }
            });
        });
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("High poly opacity").color(TEXT_DIM()));
        ui.add(egui::Slider::new(opacity, 0.1..=1.0));
        if secondary_button(ui, "Reset placement").clicked() { action = Some(PlacementAction::ResetAll); }
    });
    if let Some(a) = action { *message = apply_action(p, a, low, high, *fit); }
    if let Some(m) = message { ui.add(egui::Label::new(RichText::new(*m).color(theme::WARNING())).wrap()); }
    hint(ui, "Slide until the blue high poly sits exactly on the red low poly in the viewport, then press Bake. The file on disk is never changed.");
    *p != before
}

/// Parse the `--bake-place` spec (see `UiOptions::bake_place`) for scripted runs: loads the high
/// poly to know its bounds. Items apply in order, so `auto,offset=0.1:0:0` nudges an aligned model.
pub fn parse_place_spec(spec: &str, low: &subcraft_mesh::Mesh, high_path: &std::path::Path) -> Result<BakePlacement, String> {
    let high = subcraft_mesh::import_mesh_for_baking(high_path).map_err(|e| e.to_string())?.mesh;
    let (lb, hb) = ((low.bounds.min, low.bounds.max), (high.bounds.min, high.bounds.max));
    let triple = |v: &str| -> Result<[f32; 3], String> {
        let n: Vec<f32> = v.split(':').map(|x| x.parse::<f32>().map_err(|e| e.to_string())).collect::<Result<_, _>>()?;
        <[f32; 3]>::try_from(n).map_err(|_| format!("expected x:y:z, got {v}"))
    };
    let mut p = BakePlacement::default();
    for item in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        match item.split_once('=') {
            None => match item {
                "auto" => { if !p.auto_align(lb, hb, FitMode::LongestAxis) { return Err("degenerate bounds".into()); } }
                "center" => p.center_on_low(),
                "origin" => p.pivot_to_low_origin(),
                _ => return Err(format!("unknown item {item}")),
            },
            Some(("offset", v)) => p.offset = triple(v)?,
            Some(("rot", v)) => p.rotation_deg = triple(v)?,
            Some(("scale", v)) => p.scale = v.parse().map_err(|e: std::num::ParseFloatError| e.to_string())?,
            Some((k, _)) => return Err(format!("unknown key {k}")),
        }
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    const LOW: Box3 = (Vec3::new(-1., 0., -1.), Vec3::new(1., 2., 1.));
    const HIGH: Box3 = (Vec3::new(10., 10., 10.), Vec3::new(14., 18., 12.));

    #[test]
    fn actions_edit_the_placement() {
        let mut p = BakePlacement::default();
        assert_eq!(apply_action(&mut p, PlacementAction::AutoAlign, LOW, None, FitMode::Height), Some("Load a high poly first."));
        assert!(p.is_identity());
        assert_eq!(apply_action(&mut p, PlacementAction::AutoAlign, LOW, Some(HIGH), FitMode::Height), None);
        assert_eq!(p.scale, 0.25);
        p.offset = [1., 2., 3.]; p.scale = 3.; p.axis_scale = [2.; 3]; p.rotation_deg = [10.; 3];
        apply_action(&mut p, PlacementAction::ResetOffset, LOW, Some(HIGH), FitMode::Height);
        assert_eq!(p.offset, [0.; 3]);
        apply_action(&mut p, PlacementAction::ResetScale, LOW, Some(HIGH), FitMode::Height);
        assert_eq!((p.scale, p.axis_scale), (1., [1.; 3]));
        apply_action(&mut p, PlacementAction::ResetAll, LOW, Some(HIGH), FitMode::Height);
        assert!(p.is_identity());
        assert!(apply_action(&mut p, PlacementAction::AutoFit, LOW, Some((Vec3::ZERO, Vec3::ZERO)), FitMode::Height).is_some());
    }

    #[test]
    fn section_runs_headless_and_reports_unchanged() {
        let ctx = egui::Context::default();
        let mut p = BakePlacement::default();
        let (mut fit, mut opacity, mut msg) = (FitMode::Height, 1.0, None);
        let mut changed = true;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| { changed = placement_section(ui, &mut p, LOW, Some(HIGH), &mut fit, &mut opacity, &mut msg); });
        out.textures_delta.clear();
        assert!(!changed && p.is_identity());
    }
}
