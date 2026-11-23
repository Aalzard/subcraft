//! Properties of a procedural material (ADR-0047): one row per exposed parameter, and the tools
//! to expose, rename and remove parameters. A value change is a `SetMaterialParam` command (the
//! session writes the bound layer properties); an authoring step sends the whole definition as
//! one `SetMaterialControls`.

use egui::Ui;
use subcraft_app::Command;
use subcraft_core::LayerId;
use subcraft_doc::material::exposable;
use subcraft_doc::{Exposable, Layer, MaterialControls, Param, ParamKind, ParamValue};

use crate::i18n::tr;
use crate::icons;
use crate::widgets::{check_row_reset, hint, icon_button, icon_toggle, prop_row_reset, section_with, slider_row_reset};

/// The "Material parameters" section of `layer`: the sliders of its material, and the tools to
/// make one (expose a property of this layer or of a layer inside it, rename, remove). Labels,
/// groups and hover texts are the material's own (they come from its file or from the user),
/// so they are shown as written.
pub(crate) fn section(ui: &mut Ui, layer: &Layer, cmds: &mut Vec<Command>) {
    let none = MaterialControls::default();
    let m = layer.material.as_deref().unwrap_or(&none);
    let id = layer.id;
    let edit_id = egui::Id::new(("material-params-edit", id));
    let editing = !m.params.is_empty() && ui.data(|d| d.get_temp::<bool>(edit_id)).unwrap_or(false);
    let mut toggled = false;
    let mut reset = false;
    let mut edits: Vec<(String, ParamValue)> = Vec::new();
    // The whole definition after an authoring step; sent as one undoable command.
    let mut next: Option<Option<MaterialControls>> = None;
    section_with(
        ui,
        "material-params",
        tr("Material parameters"),
        !m.params.is_empty(),
        |ui| {
            if m.params.is_empty() {
                return;
            }
            if icon_toggle(ui, icons::GEAR, editing, tr("Rename or remove parameters")).clicked() {
                toggled = true;
            }
            if !m.is_default() && icon_button(ui, icons::UNDO, tr("Put every parameter back to its default")).clicked() {
                reset = true;
            }
        },
        |ui| {
            if m.params.is_empty() {
                hint(ui, tr("Turn settings of this layer, or of the layers inside it, into sliders shown here. A folder with sliders works like a smart material you can keep adjusting."));
            }
            // Names of what the bindings drive, for the edit mode only.
            let targets = if editing { exposable(layer) } else { Vec::new() };
            let mut group = "";
            for p in &m.params {
                if p.group != group {
                    group = &p.group;
                    if !group.is_empty() {
                        super::properties::block_header(ui, group);
                    }
                }
                if editing {
                    if let Some(change) = edit_row(ui, id, m, p, &targets) {
                        next = Some(Some(change));
                    }
                } else if let Some(v) = row(ui, &p.label, &p.tip, p.kind, p.value) {
                    edits.push((p.id.clone(), v));
                }
            }
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                if let Some(change) = add_menu(ui, layer, m) {
                    next = Some(Some(change));
                }
                if editing {
                    if !m.is_default() && ui.button(tr("Use as defaults")).on_hover_text(tr("Reset will come back to the values the parameters have now")).clicked() {
                        let mut c = m.clone();
                        if c.values_as_defaults() {
                            next = Some(Some(c));
                        }
                    }
                    if ui.button(tr("Remove all")).on_hover_text(tr("Remove every parameter. The layers keep the look they have now")).clicked() {
                        next = Some(None);
                    }
                }
            });
        },
    );
    ui.data_mut(|d| d.insert_temp(edit_id, editing != toggled));
    if reset {
        cmds.push(Command::ResetMaterialParams(id));
    }
    cmds.extend(edits.into_iter().map(|(param, value)| Command::SetMaterialParam { layer: id, param, value }));
    if let Some(controls) = next {
        // The last parameter removed leaves a plain layer.
        let controls = controls.filter(|c| !c.params.is_empty()).map(Box::new);
        cmds.push(Command::SetMaterialControls { layer: id, controls });
    }
}

/// "Add parameter": the properties of the layer tree that can be exposed, one submenu per
/// layer or mask effect. The definition with the chosen one added.
fn add_menu(ui: &mut Ui, layer: &Layer, m: &MaterialControls) -> Option<MaterialControls> {
    let mut out = None;
    ui.menu_button(format!("{}  {}", icons::PLUS, tr("Add parameter")), |ui| {
        // Listed only while the menu is open.
        let list = exposable(layer);
        let mut i = 0;
        let mut any = false;
        egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
            while i < list.len() {
                let (first, mut end) = (&list[i], i + 1);
                while end < list.len() && list[end].layer == first.layer && list[end].effect == first.effect {
                    end += 1;
                }
                let free: Vec<&Exposable> = list[i..end].iter().filter(|e| !m.is_bound(e.layer, e.effect, e.prop)).collect();
                i = end;
                if free.is_empty() {
                    continue;
                }
                any = true;
                let title = match &first.effect_name {
                    Some(fx) => format!("{}  \u{203a}  {}", first.layer_name, fx),
                    None => first.layer_name.clone(),
                };
                ui.menu_button(title, |ui| {
                    for e in free {
                        if ui.button(&e.label).clicked() {
                            let mut c = m.clone();
                            if c.expose(e).is_some() {
                                out = Some(c);
                            }
                            ui.close();
                        }
                    }
                });
            }
        });
        if !any {
            hint(ui, tr("Nothing left to expose"));
        }
    })
    .response
    .on_hover_text(tr("Pick a setting of this layer, its mask, or a layer inside it. It becomes a slider here that starts at the value it has now"));
    out
}

/// A parameter's row while editing: its name, the slider's range, and remove. The definition
/// after a change. Name and range are committed when the field is left or the drag ends, so
/// one edit is one undo step.
fn edit_row(ui: &mut Ui, layer: LayerId, m: &MaterialControls, p: &Param, targets: &[Exposable]) -> Option<MaterialControls> {
    let mut out = None;
    let name_id = egui::Id::new(("material-param-name", layer, &p.id));
    let range_id = egui::Id::new(("material-param-range", layer, &p.id));
    ui.horizontal(|ui| {
        let mut name = ui.data(|d| d.get_temp::<String>(name_id)).unwrap_or_else(|| p.label.clone());
        let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(120.0)).on_hover_text(tr("Name shown for this parameter"));
        if r.lost_focus() {
            let mut c = m.clone();
            if c.rename_param(&p.id, &name) {
                out = Some(c);
            }
            ui.data_mut(|d| d.remove::<String>(name_id));
        } else if r.has_focus() {
            ui.data_mut(|d| d.insert_temp(name_id, name));
        }
        if let ParamKind::Float { min, max, .. } = p.kind {
            let (mut lo, mut hi) = ui.data(|d| d.get_temp::<(f32, f32)>(range_id)).unwrap_or((min, max));
            let a = ui.add(egui::DragValue::new(&mut lo).speed(0.01).max_decimals(3)).on_hover_text(tr("Lowest value of the slider"));
            let b = ui.add(egui::DragValue::new(&mut hi).speed(0.01).max_decimals(3)).on_hover_text(tr("Highest value of the slider"));
            let busy = a.dragged() || b.dragged() || a.has_focus() || b.has_focus();
            if busy {
                ui.data_mut(|d| d.insert_temp(range_id, (lo, hi)));
            } else if (lo, hi) != (min, max) {
                let mut c = m.clone();
                if c.set_range(&p.id, lo, hi) {
                    out = Some(c);
                }
                ui.data_mut(|d| d.remove::<(f32, f32)>(range_id));
            }
        }
        if icon_button(ui, icons::TRASH, tr("Remove this parameter. The layers keep the look they have now")).clicked() {
            let mut c = m.clone();
            if c.remove_param(&p.id) {
                out = Some(c);
            }
        }
    });
    // What the parameter drives: one line per setting, with the formula that turns the
    // parameter (named by its id) into the setting's value.
    let name_of = |layer: LayerId, effect: Option<subcraft_core::EffectId>, prop: subcraft_doc::Prop| {
        targets.iter().find(|e| e.layer == layer && e.effect == effect && e.prop == prop).map(|e| match &e.effect_name {
            Some(fx) => format!("{}  \u{203a}  {}  \u{203a}  {}", e.layer_name, fx, e.label),
            None => format!("{}  \u{203a}  {}", e.layer_name, e.label),
        })
    };
    for index in m.bindings_of(&p.id) {
        let b = &m.bindings[index];
        let formula_id = egui::Id::new(("material-binding-formula", layer, index));
        let error_id = formula_id.with("error");
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            let name = name_of(b.layer, b.effect, b.prop).unwrap_or_else(|| tr("(setting no longer exists)").to_string());
            ui.add(egui::Label::new(egui::RichText::new(name).small()).truncate());
        });
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            let mut text = ui.data(|d| d.get_temp::<String>(formula_id)).unwrap_or_else(|| b.formula.clone());
            let r = ui
                .add(egui::TextEdit::singleline(&mut text).desired_width((ui.available_width() - 40.0).max(60.0)).font(egui::TextStyle::Monospace))
                .on_hover_text(format!("{} {}", tr("Formula for this setting. Use the parameter by its id:"), p.id));
            if r.lost_focus() {
                let mut c = m.clone();
                match c.set_formula(index, &text) {
                    Ok(true) => {
                        out = Some(c);
                        ui.data_mut(|d| {
                            d.remove::<String>(formula_id);
                            d.remove::<String>(error_id);
                        });
                    }
                    Ok(false) => ui.data_mut(|d| {
                        d.remove::<String>(formula_id);
                        d.remove::<String>(error_id);
                    }),
                    // Kept as typed, with the reason below, until it is fixed.
                    Err(why) => ui.data_mut(|d| {
                        d.insert_temp(formula_id, text);
                        d.insert_temp(error_id, why);
                    }),
                }
            } else if r.has_focus() {
                ui.data_mut(|d| d.insert_temp(formula_id, text));
            }
            if icon_button(ui, icons::X, tr("Stop driving this setting")).clicked() {
                let mut c = m.clone();
                if c.remove_binding(index) {
                    out = Some(c);
                }
            }
        });
        if let Some(why) = ui.data(|d| d.get_temp::<String>(error_id)) {
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.add(egui::Label::new(egui::RichText::new(why).small().color(ui.visuals().error_fg_color)).wrap());
            });
        }
    }
    let free: Vec<&Exposable> = targets.iter().filter(|e| m.can_link(&p.id, e)).collect();
    if !free.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.menu_button(tr("Also drive..."), |ui| {
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    let mut i = 0;
                    while i < free.len() {
                        let (first, mut end) = (free[i], i + 1);
                        while end < free.len() && free[end].layer == first.layer && free[end].effect == first.effect {
                            end += 1;
                        }
                        let title = match &first.effect_name {
                            Some(fx) => format!("{}  \u{203a}  {}", first.layer_name, fx),
                            None => first.layer_name.clone(),
                        };
                        ui.menu_button(title, |ui| {
                            for e in &free[i..end] {
                                if ui.button(&e.label).clicked() {
                                    let mut c = m.clone();
                                    if c.link(&p.id, e) {
                                        out = Some(c);
                                    }
                                    ui.close();
                                }
                            }
                        });
                        i = end;
                    }
                });
            })
            .response
            .on_hover_text(tr("Make this parameter drive one more setting. It keeps the value it has now until the parameter moves"));
        });
    }
    out
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
