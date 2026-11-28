//! Properties of a graph layer (a `.sbsar` material): the graph's parameters as controls and the
//! render size. Rendering takes seconds, so edits are collected while the pointer is down and
//! sent as one render a moment after the last change; the layer's images change (one undo
//! step) when that render is done.

use egui::Ui;
use subcraft_app::graph_layer::GraphTarget;
use subcraft_doc::{GraphMaterial, GraphParam, GraphParamKind, Layer};

use crate::i18n::{tr, trf};
use crate::icons;
use crate::state::UiState;
use crate::widgets::{check_row_reset, hint, icon_button, prop_row_reset, section_with, slider_row_reset};

/// Seconds without a change (and with no button held) before an edit is rendered.
const SETTLE: f64 = 0.35;

/// Render sizes offered (log2 of the side).
const SIZES: [u32; 6] = [7, 8, 9, 10, 11, 12];

pub(crate) fn section(state: &mut UiState, ui: &mut Ui, layer: &Layer, now: f64) {
    section_for(state, ui, GraphTarget::Layer(layer.id), layer.graph.as_deref(), tr("Graph material"), now);
}

/// The parameters of a Substance generator (an image mask effect with a graph).
pub(crate) fn generator_section(state: &mut UiState, ui: &mut Ui, layer: subcraft_core::LayerId, effect: subcraft_core::EffectId, graph: &GraphMaterial, now: f64) {
    section_for(state, ui, GraphTarget::Mask { layer, effect }, Some(graph), tr("Substance generator"), now);
}

/// The parameters of a Substance filter (a fill content effect with a graph), and a button that
/// renders it again from the layers below as they are now.
pub(crate) fn filter_section(state: &mut UiState, ui: &mut Ui, layer: subcraft_core::LayerId, effect: subcraft_core::EffectId, graph: &GraphMaterial, now: f64) {
    let target = GraphTarget::Content { layer, effect };
    hint(ui, tr("A Substance filter renders from the layers below it as they were at its last render."));
    if ui.add_enabled(!state.graph_layers.is_rendering_target(target), egui::Button::new(tr("Update from layers below"))).on_hover_text(tr("Render the filter again from the layers below as they are now")).clicked() {
        let m = state.graph_layers.shown_for(target, Some(graph)).unwrap_or_else(|| graph.clone());
        if let Err(e) = state.graph_layers.update_filter_input(&state.session, layer, effect, m) {
            state.set_status(format!("Substance filter: {e}"), now);
        }
    }
    section_for(state, ui, target, Some(graph), tr("Substance filter"), now);
}

/// [`section`] for any graph target whose current material is `current`.
fn section_for(state: &mut UiState, ui: &mut Ui, target: GraphTarget, current: Option<&GraphMaterial>, title: &str, now: f64) {
    let Some(shown) = state.graph_layers.shown_for(target, current) else { return };
    let pending_id = egui::Id::new(("graph-params-pending", target));
    let pending: Option<(GraphMaterial, f64)> = ui.data(|d| d.get_temp(pending_id));
    let mut m = pending.as_ref().map(|p| p.0.clone()).unwrap_or_else(|| shown.clone());
    let rendering = state.graph_layers.is_rendering_target(target);
    let desc = state.graph_layers.graph(&state.session, &m);
    let mut changed = false;
    let mut send_now = false;
    let has_values = !m.values.is_empty();
    let mut reset = false;
    section_with(
        ui,
        "graph-params",
        title,
        true,
        |ui| {
            if rendering || pending.is_some() {
                ui.add(egui::Spinner::new().size(12.0)).on_hover_text(tr("Rendering the material with the new values"));
            }
            if has_values && icon_button(ui, icons::UNDO, tr("Put every parameter back to its default")).clicked() {
                reset = true;
            }
        },
        |ui| {
            let desc = match &desc {
                Ok(d) => d,
                Err(e) => {
                    hint(ui, &trf("This material cannot be rendered: {}", &[e]));
                    return;
                }
            };
            // Size: the CPU engine takes seconds at 512 px and much longer above.
            let mut size = m.size_log2;
            let r = prop_row_reset(ui, tr("Resolution"), &mut size, subcraft_sbsar::DEFAULT_SIZE_LOG2, |ui, size| {
                let mut r = egui::ComboBox::from_id_salt(("graph-size", target)).selected_text(format!("{0} x {0}", 1u32 << *size)).show_ui(ui, |ui| {
                    for s in SIZES {
                        ui.selectable_value(size, s, format!("{0} x {0}", 1u32 << s));
                    }
                });
                if r.inner.is_some() && r.response.changed() {
                    r.response.mark_changed();
                }
                r.response
            });
            if size != m.size_log2 || r.changed() {
                m.size_log2 = size;
                send_now = true;
            }
            hint(ui, tr("Larger sizes take much longer to render."));
            let mut group = "";
            for p in &desc.params {
                if p.group != group {
                    group = &p.group;
                    if !group.is_empty() {
                        super::properties::block_header(ui, group);
                    }
                }
                let mut v = m.value_of(p);
                if row(ui, target, p, &mut v) {
                    m.set_value(p, &v);
                    changed = true;
                }
            }
            for n in &desc.notes {
                hint(ui, n);
            }
        },
    );
    if reset {
        m.values.clear();
        send_now = true;
    }
    let held = ui.input(|i| i.pointer.any_down());
    if send_now {
        ui.data_mut(|d| d.remove::<(GraphMaterial, f64)>(pending_id));
        request(state, target, current, m, now);
    } else if changed {
        ui.data_mut(|d| d.insert_temp(pending_id, (m, now)));
        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(SETTLE));
    } else if let Some((p, t)) = pending {
        if now - t >= SETTLE && !held {
            ui.data_mut(|d| d.remove::<(GraphMaterial, f64)>(pending_id));
            request(state, target, current, p, now);
        } else {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(SETTLE));
        }
    }
}

fn request(state: &mut UiState, target: GraphTarget, current: Option<&GraphMaterial>, m: GraphMaterial, now: f64) {
    if current == Some(&m) && !state.graph_layers.is_rendering_target(target) {
        return;
    }
    if let Err(e) = state.graph_layers.request_target(&state.session, target, m) {
        state.set_status(format!("Material graph: {e}"), now);
    }
}

/// One parameter's control(s); true when the user changed `v`. Labels are the material's own.
fn row(ui: &mut Ui, layer: GraphTarget, p: &GraphParam, v: &mut Vec<f64>) -> bool {
    let before = v.clone();
    let default = p.default.clone();
    match &p.kind {
        GraphParamKind::Float { components, .. } | GraphParamKind::Int { components, .. } => {
            let integer = matches!(p.kind, GraphParamKind::Int { .. });
            const AXES: [&str; 4] = ["X", "Y", "Z", "W"];
            for i in 0..(*components).clamp(1, 4).min(v.len()) {
                let (lo, hi) = p.range(i);
                let label = if *components > 1 { format!("{} {}", p.label, AXES[i]) } else { p.label.clone() };
                let d = default.get(i).copied().unwrap_or(lo);
                slider_row_reset(ui, &label, &mut v[i], d, |x| {
                    let s = egui::Slider::new(x, lo..=hi).clamping(egui::SliderClamping::Never);
                    if integer { s.integer() } else { s.max_decimals(3) }
                });
            }
        }
        GraphParamKind::Bool => {
            let mut b = v.first().is_some_and(|x| *x >= 0.5);
            let d = default.first().is_some_and(|x| *x >= 0.5);
            if check_row_reset(ui, "", &p.label, &mut b, d).changed() {
                v[0] = if b { 1.0 } else { 0.0 };
            }
        }
        GraphParamKind::Color { linear, .. } => {
            let get = |v: &[f64]| [0, 1, 2].map(|i| v.get(i).copied().unwrap_or(0.0) as f32);
            let (mut c, d) = (get(v), get(&default));
            let changed = if *linear {
                super::properties::linear_color_row(ui, &p.label, &mut c, d)
            } else {
                // Substance colours are sRGB-encoded values.
                prop_row_reset(ui, &p.label, &mut c, d, crate::color_picker::swatch_srgb).changed()
            };
            if changed {
                for i in 0..3 {
                    v[i] = c[i] as f64;
                }
            }
        }
        GraphParamKind::Enum(options) => {
            let mut x = v.first().copied().unwrap_or(0.0).round() as i32;
            let d = default.first().copied().unwrap_or(0.0).round() as i32;
            let text = options.iter().find(|(o, _)| *o == x).map(|(_, t)| t.clone()).unwrap_or_else(|| x.to_string());
            prop_row_reset(ui, &p.label, &mut x, d, |ui, x| {
                let mut picked = false;
                let mut r = egui::ComboBox::from_id_salt(("graph-enum", layer, &p.id)).selected_text(text).show_ui(ui, |ui| {
                    for (o, t) in options {
                        picked |= ui.selectable_value(x, *o, t).clicked();
                    }
                });
                if picked {
                    r.response.mark_changed();
                }
                r.response
            });
            v[0] = x as f64;
        }
        GraphParamKind::Seed => {
            let mut seed = v.first().copied().unwrap_or(0.0).max(0.0) as u32;
            let d = default.first().copied().unwrap_or(0.0).max(0.0) as u32;
            prop_row_reset(ui, &p.label, &mut seed, d, |ui, seed| {
                let mut r = ui.add(egui::DragValue::new(seed).speed(1.).range(0..=subcraft_doc::material::MAX_SEED));
                if ui.small_button(tr("Random")).on_hover_text(tr("Pick a new random seed")).clicked() {
                    *seed = super::properties::random_seed();
                    r.mark_changed();
                }
                r
            });
            v[0] = seed as f64;
        }
    }
    *v != before
}
