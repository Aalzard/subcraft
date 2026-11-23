//! Properties of a procedural material (ADR-0047): one row per exposed parameter. Every change is
//! a `SetMaterialParam` command; the session writes the bound layer properties.

use egui::Ui;
use subcraft_app::Command;
use subcraft_core::LayerId;
use subcraft_doc::{MaterialControls, ParamKind, ParamValue};

use crate::i18n::tr;
use crate::icons;
use crate::widgets::{check_row_reset, icon_button, prop_row_reset, section_with, slider_row_reset};

/// The "Material parameters" section of `layer`. Labels, groups and hover texts are the
/// material's own (they come from its file), so they are shown as written.
pub(crate) fn section(ui: &mut Ui, layer: LayerId, m: &MaterialControls, cmds: &mut Vec<Command>) {
    if m.params.is_empty() {
        return;
    }
    let mut reset = false;
    let mut edits: Vec<(String, ParamValue)> = Vec::new();
    section_with(
        ui,
        "material-params",
        tr("Material parameters"),
        true,
        |ui| {
            if !m.is_default() && icon_button(ui, icons::UNDO, tr("Put every parameter back to its default")).clicked() {
                reset = true;
            }
        },
        |ui| {
            let mut group = "";
            for p in &m.params {
                if p.group != group {
                    group = &p.group;
                    if !group.is_empty() {
                        super::properties::block_header(ui, group);
                    }
                }
                if let Some(v) = row(ui, &p.label, &p.tip, p.kind, p.value) {
                    edits.push((p.id.clone(), v));
                }
            }
        },
    );
    if reset {
        cmds.push(Command::ResetMaterialParams(layer));
    }
    cmds.extend(edits.into_iter().map(|(param, value)| Command::SetMaterialParam { layer, param, value }));
}

/// One parameter's row; the new value when the user changed it.
fn row(ui: &mut Ui, label: &str, tip: &str, kind: ParamKind, value: ParamValue) -> Option<ParamValue> {
    let hover = |r: egui::Response| if tip.is_empty() { r } else { r.on_hover_text(tip) };
    match (kind, value) {
        (ParamKind::Float { min, max, default }, ParamValue::Number(mut v)) => {
            hover(slider_row_reset(ui, label, &mut v, default, |v| egui::Slider::new(v, min..=max).max_decimals(3))).changed().then_some(ParamValue::Number(v))
        }
        (ParamKind::Color { default }, ParamValue::Color(mut c)) => super::properties::linear_color_row(ui, label, &mut c, default).then_some(ParamValue::Color(c)),
        (ParamKind::Toggle { default }, ParamValue::Toggle(mut b)) => hover(check_row_reset(ui, "", label, &mut b, default)).changed().then_some(ParamValue::Toggle(b)),
        (ParamKind::Seed { default }, ParamValue::Number(v)) => {
            let mut seed = v.max(0.0) as u32;
            hover(prop_row_reset(ui, label, &mut seed, default, |ui, seed| {
                let mut r = ui.add(egui::DragValue::new(seed).speed(1.).range(0..=subcraft_doc::material::MAX_SEED));
                if ui.small_button(tr("Random")).on_hover_text(tr("Pick a new random seed")).clicked() {
                    *seed = super::properties::random_seed();
                    r.mark_changed();
                }
                r
            }))
            .changed()
            .then_some(ParamValue::Number(seed as f32))
        }
        // A value of the wrong type cannot come out of a loaded file (it is sanitized).
        _ => None,
    }
}
