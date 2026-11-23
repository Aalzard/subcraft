//! Making a material by hand: which properties of a layer tree can become parameters, and
//! exposing / removing them. The panel lists [`exposable`] and sends the edited
//! [`MaterialControls`] back as one command.

use subcraft_core::{ChannelKind, EffectId, LayerId};

use super::{Binding, MaterialControls, Param, ParamKind, ParamValue, Prop, MAX_BINDINGS, MAX_PARAMS, MAX_SEED, valid_param_id};
use crate::material_expr;
use crate::{FillSource, Layer, LayerContent, MaskEffectKind, ProceduralParams};

/// A property that can be exposed as a parameter, with the control it would get. The control's
/// default is the property's value now, so exposing changes nothing until the slider moves.
#[derive(Clone, Debug, PartialEq)]
pub struct Exposable {
    pub layer: LayerId,
    pub effect: Option<EffectId>,
    pub prop: Prop,
    /// Name of the layer (and of the mask effect), for the menu.
    pub layer_name: String,
    pub effect_name: Option<String>,
    /// Name of the property: "Base Color", "Roughness: pattern scale", "Level", ...
    pub label: String,
    pub kind: ParamKind,
}

fn float(v: f32, min: f32, max: f32) -> ParamKind {
    // A value outside the usual range keeps the range wide enough to hold it.
    ParamKind::Float { min: min.min(v), max: max.max(v), default: v }
}

fn seed_kind(seed: u32) -> ParamKind {
    ParamKind::Seed { default: seed.min(MAX_SEED) }
}

fn channel_kind(kind: ChannelKind, v: subcraft_core::ChannelValue) -> ParamKind {
    if kind.components() >= 3 {
        ParamKind::Color { default: v.0.map(|c| c.max(0.0)) }
    } else if kind.is_signed() {
        float(v.0[0], -1.0, 1.0)
    } else {
        float(v.0[0], 0.0, 1.0)
    }
}

fn pattern(out: &mut Vec<(Prop, String, ParamKind)>, p: &ProceduralParams, channel: Option<ChannelKind>, prefix: &str) {
    out.push((Prop::PatternScale { channel }, format!("{prefix}Pattern scale"), float(p.scale, 0.1, 64.0)));
    out.push((Prop::PatternBalance { channel }, format!("{prefix}Pattern balance"), float(p.balance, 0.0, 1.0)));
    out.push((Prop::PatternContrast { channel }, format!("{prefix}Pattern contrast"), float(p.contrast, 0.0, 1.0)));
    out.push((Prop::PatternSeed { channel }, format!("{prefix}Pattern seed"), seed_kind(p.seed)));
}

fn of_layer(l: &Layer) -> Vec<(Prop, String, ParamKind)> {
    let mut out = Vec::new();
    // One Opacity for the layer when its channels agree, else one per channel: a control for
    // all of them would flatten the differences as soon as it is exposed.
    match l.blend.values().next() {
        Some(first) if l.blend.values().all(|b| b.opacity == first.opacity) => {
            out.push((Prop::Opacity { channel: None }, "Opacity".into(), float(first.opacity, 0.0, 1.0)));
        }
        _ => {
            for (kind, b) in &l.blend {
                out.push((Prop::Opacity { channel: Some(*kind) }, format!("{}: opacity", kind.display_name()), float(b.opacity, 0.0, 1.0)));
            }
        }
    }
    let LayerContent::Fill(f) = &l.content else { return out };
    for (kind, ch) in &f.channels {
        let name = kind.display_name();
        match &ch.source {
            FillSource::Uniform { value } => out.push((Prop::Value { channel: *kind }, name.into(), channel_kind(*kind, *value))),
            FillSource::Procedural { params, low, high } => {
                out.push((Prop::Low { channel: *kind }, format!("{name}: dark end"), channel_kind(*kind, *low)));
                out.push((Prop::High { channel: *kind }, format!("{name}: bright end"), channel_kind(*kind, *high)));
                pattern(&mut out, params, Some(*kind), &format!("{name}: "));
            }
            _ => {}
        }
    }
    out.push((Prop::Tiling, "Tiling".into(), float(f.projection.scale[0], 0.1, 32.0)));
    out.push((Prop::Rotation, "Rotation".into(), float(f.projection.rotation_deg, -180.0, 180.0)));
    out
}

fn of_mask_effect(kind: &MaskEffectKind, opacity: f32) -> Vec<(Prop, String, ParamKind)> {
    let mut out = vec![(Prop::Opacity { channel: None }, "Opacity".into(), float(opacity, 0.0, 1.0))];
    match kind {
        MaskEffectKind::Generator(g) => {
            out.push((Prop::Level, "Level".into(), float(g.level, 0.0, 1.0)));
            out.push((Prop::Softness, "Softness".into(), float(g.softness, 0.001, 1.0)));
            out.push((Prop::Breakup, "Breakup".into(), float(g.noise_amount, 0.0, 1.0)));
            out.push((Prop::BreakupScale, "Breakup scale".into(), float(g.noise.scale, 1.0, 100.0)));
            out.push((Prop::BreakupSeed, "Breakup seed".into(), seed_kind(g.noise.seed)));
            if let Some(s) = &g.edge_spread {
                out.push((Prop::Spread, "Spread".into(), float(s.distance, 0.0, 25.0)));
            }
        }
        MaskEffectKind::Fill { procedural: Some(p), .. } => pattern(&mut out, p, None, ""),
        MaskEffectKind::Fill { value, procedural: None, .. } => out.push((Prop::FillValue, "Value".into(), float(*value, 0.0, 1.0))),
        _ => {}
    }
    out
}

fn of_content_effect(e: &crate::ContentEffect) -> Vec<(Prop, String, ParamKind)> {
    let crate::ContentEffectKind::Material(filter) = &e.kind else { return Vec::new() };
    filter
        .settings()
        .into_iter()
        .map(|(setting, value)| {
            let kind = match value {
                crate::FilterValue::Unit(v) => float(v, 0.0, 1.0),
                crate::FilterValue::Scale(v) => float(v, 0.1, crate::MAX_MATERIAL_FILTER_SCALE),
                crate::FilterValue::Spread(v) => float(v, 0.0, crate::MAX_EDGE_DAMAGE_SPREAD),
                crate::FilterValue::Color(c) => ParamKind::Color { default: c },
                crate::FilterValue::Seed(s) => seed_kind(s),
            };
            (Prop::Filter { setting }, setting.label().to_string(), kind)
        })
        .collect()
}

/// Every property of `root` and the layers inside it that can be exposed, top layer first.
pub fn exposable(root: &Layer) -> Vec<Exposable> {
    let mut out = Vec::new();
    collect(root, &mut out);
    out
}

fn collect(l: &Layer, out: &mut Vec<Exposable>) {
    for (prop, label, kind) in of_layer(l) {
        out.push(Exposable { layer: l.id, effect: None, prop, layer_name: l.name.clone(), effect_name: None, label, kind });
    }
    for e in l.mask.iter().flat_map(|m| m.effects.iter().rev()) {
        for (prop, label, kind) in of_mask_effect(&e.kind, e.opacity) {
            out.push(Exposable { layer: l.id, effect: Some(e.id), prop, layer_name: l.name.clone(), effect_name: Some(e.name.clone()), label, kind });
        }
    }
    for e in l.content_effects.iter().rev() {
        for (prop, label, kind) in of_content_effect(e) {
            out.push(Exposable { layer: l.id, effect: Some(e.id), prop, layer_name: l.name.clone(), effect_name: Some(e.name.clone()), label, kind });
        }
    }
    for c in l.children().iter().rev() {
        collect(c, out);
    }
}

/// A formula-safe id made from a label ("Base Color" gives `base_color`), not used in `taken`.
fn fresh_id(label: &str, taken: &dyn Fn(&str) -> bool) -> String {
    let mut base: String = label.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
    while base.contains("__") {
        base = base.replace("__", "_");
    }
    let mut base = base.trim_matches('_').to_string();
    base.truncate(40);
    if !valid_param_id(&base) {
        base = format!("p_{base}");
        base.truncate(40);
        if !valid_param_id(&base) {
            base = "param".into();
        }
    }
    if !taken(&base) {
        return base;
    }
    (2..).map(|n| format!("{base}_{n}")).find(|id| !taken(id)).expect("an unused number exists")
}

impl MaterialControls {
    /// Whether a binding already drives this property.
    pub fn is_bound(&self, layer: LayerId, effect: Option<EffectId>, prop: Prop) -> bool {
        self.bindings.iter().any(|b| b.layer == layer && b.effect == effect && b.prop == prop)
    }

    /// Add a parameter that drives `e`, at the property's value now. Returns the new
    /// parameter's id, or `None` when the property is driven already or the material is full.
    pub fn expose(&mut self, e: &Exposable) -> Option<String> {
        if self.is_bound(e.layer, e.effect, e.prop) || self.params.len() >= MAX_PARAMS || self.bindings.len() >= MAX_BINDINGS {
            return None;
        }
        let label = match &e.effect_name {
            Some(fx) if e.label == "Opacity" || e.label == "Level" || e.label == "Value" => format!("{} {}", fx, e.label.to_lowercase()),
            _ if e.label == "Opacity" || e.label == "Tiling" || e.label == "Rotation" => format!("{} {}", e.layer_name, e.label.to_lowercase()),
            _ => e.label.clone(),
        };
        let id = fresh_id(&label, &|id| self.param(id).is_some());
        let value = e.kind.default_value();
        self.params.push(Param { id: id.clone(), label, group: e.layer_name.clone(), tip: String::new(), kind: e.kind, value });
        self.bindings.push(Binding { layer: e.layer, effect: e.effect, prop: e.prop, formula: id.clone() });
        Some(id)
    }

    /// Remove a parameter and every binding whose formula reads it (they could not be
    /// evaluated any more). The layers keep the values they have.
    pub fn remove_param(&mut self, id: &str) -> bool {
        let before = self.params.len();
        self.params.retain(|p| p.id != id);
        if self.params.len() == before {
            return false;
        }
        self.bindings.retain(|b| !material_expr::params_used(&b.formula).is_ok_and(|used| used.iter().any(|p| p == id)));
        true
    }

    /// Change the name shown in Properties (formulas use the id, which stays).
    pub fn rename_param(&mut self, id: &str, label: &str) -> bool {
        let label = label.trim();
        match self.params.iter_mut().find(|p| p.id == id) {
            Some(p) if !label.is_empty() && p.label != label && label.len() <= 64 => {
                p.label = label.into();
                true
            }
            _ => false,
        }
    }

    /// Change a slider's range. The value and the default are kept inside it.
    pub fn set_range(&mut self, id: &str, min: f32, max: f32) -> bool {
        let Some(p) = self.params.iter_mut().find(|p| p.id == id) else { return false };
        let ParamKind::Float { default, .. } = p.kind else { return false };
        if !(min.is_finite() && max.is_finite() && min < max) {
            return false;
        }
        let kind = ParamKind::Float { min, max, default: default.clamp(min, max) };
        if kind == p.kind {
            return false;
        }
        p.kind = kind;
        p.value = kind.sanitize(p.value).unwrap_or(ParamValue::Number(default.clamp(min, max)));
        true
    }

    /// Make the values the parameters have now their defaults (what Reset goes back to).
    pub fn values_as_defaults(&mut self) -> bool {
        let mut changed = false;
        for p in &mut self.params {
            let kind = match (p.kind, p.value) {
                (ParamKind::Float { min, max, .. }, ParamValue::Number(n)) => ParamKind::Float { min, max, default: n },
                (ParamKind::Color { .. }, ParamValue::Color(c)) => ParamKind::Color { default: c },
                (ParamKind::Toggle { .. }, ParamValue::Toggle(b)) => ParamKind::Toggle { default: b },
                (ParamKind::Seed { .. }, ParamValue::Number(n)) => ParamKind::Seed { default: n as u32 },
                (kind, _) => kind,
            };
            changed |= kind != p.kind;
            p.kind = kind;
        }
        changed
    }

    /// Bindings whose formula reads parameter `id`, by index.
    pub fn bindings_of(&self, id: &str) -> Vec<usize> {
        (0..self.bindings.len()).filter(|&i| material_expr::params_used(&self.bindings[i].formula).is_ok_and(|used| used.iter().any(|p| p == id))).collect()
    }

    /// Whether parameter `id` can also drive `e`: the same kind of value (a slider drives
    /// numbers, a colour drives colours, a seed drives seeds) and not driven yet.
    pub fn can_link(&self, id: &str, e: &Exposable) -> bool {
        let Some(p) = self.param(id) else { return false };
        let same = matches!(
            (p.kind, e.kind),
            (ParamKind::Float { .. }, ParamKind::Float { .. }) | (ParamKind::Color { .. }, ParamKind::Color { .. }) | (ParamKind::Seed { .. }, ParamKind::Seed { .. })
        );
        same && !self.is_bound(e.layer, e.effect, e.prop) && self.bindings.len() < MAX_BINDINGS
    }

    /// Make parameter `id` also drive `e`. The formula keeps the property at the value it has
    /// now while the parameter is at its default (a ratio for sliders and colours, an offset for
    /// seeds), so linking changes nothing by itself. The user can rewrite it afterwards.
    pub fn link(&mut self, id: &str, e: &Exposable) -> bool {
        if !self.can_link(id, e) {
            return false;
        }
        let Some(p) = self.param(id) else { return false };
        let number = |v: f32| {
            let t = format!("{v:.4}");
            t.trim_end_matches('0').trim_end_matches('.').to_string()
        };
        let formula = match (p.kind, e.kind) {
            (ParamKind::Float { default, .. }, ParamKind::Float { default: now, .. }) => {
                if default == now {
                    id.to_string()
                } else if default.abs() > 1e-4 && now.abs() > 1e-4 {
                    format!("{id} * {}", number(now / default))
                } else if now >= default {
                    format!("{id} + {}", number(now - default))
                } else {
                    format!("{id} - {}", number(default - now))
                }
            }
            (ParamKind::Color { default }, ParamKind::Color { default: now }) => {
                if default == now {
                    id.to_string()
                } else if default.iter().all(|c| *c > 1e-4) {
                    format!("{id} * rgb({}, {}, {})", number(now[0] / default[0]), number(now[1] / default[1]), number(now[2] / default[2]))
                } else {
                    id.to_string()
                }
            }
            (ParamKind::Seed { default }, ParamKind::Seed { default: now }) => {
                if now >= default { format!("{id} + {}", now - default) } else { format!("{id} - {}", default - now) }
            }
            _ => return false,
        };
        self.bindings.push(Binding { layer: e.layer, effect: e.effect, prop: e.prop, formula });
        true
    }

    /// Replace a binding's formula. `Err` with a short reason when the text is not a formula
    /// or names a parameter the material does not have; `Ok(false)` when it is unchanged.
    pub fn set_formula(&mut self, binding: usize, text: &str) -> Result<bool, String> {
        let text = text.trim();
        let Some(b) = self.bindings.get(binding) else { return Err("no such binding".into()) };
        if b.formula == text {
            return Ok(false);
        }
        let lookup = |id: &str| self.param(id).map(|p| p.value.as_value());
        match material_expr::eval(text, &lookup) {
            Ok(v) if v.iter().all(|c| c.is_finite()) => {}
            Ok(_) => return Err("the result is not a number".into()),
            Err(e) => return Err(e.to_string()),
        }
        self.bindings[binding].formula = text.into();
        Ok(true)
    }

    /// Stop driving a property. The parameter stays, even with nothing left to drive.
    pub fn remove_binding(&mut self, binding: usize) -> bool {
        if binding >= self.bindings.len() {
            return false;
        }
        self.bindings.remove(binding);
        true
    }
}
