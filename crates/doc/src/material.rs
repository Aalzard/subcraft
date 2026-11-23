//! Procedural materials (ADR-0047): parameters a layer exposes, bound to properties of itself and
//! of the layers inside it.
//!
//! A procedural material is an ordinary layer (a fill, or a folder of masked fills) that also
//! carries [`MaterialControls`]: a list of named parameters (Wear, Paint colour, Scale, ...) and
//! a list of bindings. A binding says "this property of that layer is this formula of the
//! parameters". Setting a parameter evaluates the bindings and writes the results into the
//! layers, so the compositor, the exporter and every tool keep seeing plain layers. Nothing
//! about evaluation changes, and the user can still open the folder and edit any layer by hand.

use serde::{Deserialize, Serialize};
use subcraft_core::{ChannelKind, ChannelValue, EffectId, LayerId};

use crate::material_expr::{self, ExprError, Value};
use crate::{FillSource, Layer, LayerContent, MaskEffectKind, ProceduralParams};

/// Most parameters / bindings one material may carry (files claiming more are cut).
pub const MAX_PARAMS: usize = 64;
pub const MAX_BINDINGS: usize = 512;

/// What kind of control a parameter is, with its range and default.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ParamKind {
    /// A slider.
    Float { min: f32, max: f32, default: f32 },
    /// A linear RGB colour.
    Color { default: [f32; 3] },
    /// A checkbox; formulas read it as 1 or 0.
    Toggle { default: bool },
    /// A whole number with a Random button; formulas read it as a number.
    Seed { default: u32 },
}

/// A parameter's current value. Seeds are stored as numbers.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamValue {
    Toggle(bool),
    Number(f32),
    Color([f32; 3]),
}

impl ParamValue {
    /// The value a formula reads.
    pub fn as_value(self) -> Value {
        match self {
            ParamValue::Toggle(b) => [if b { 1.0 } else { 0.0 }; 3],
            ParamValue::Number(n) => [n; 3],
            ParamValue::Color(c) => c,
        }
    }
}

/// Largest seed a parameter holds (exact as `f32`, and what the Random button draws below).
pub const MAX_SEED: u32 = 99_999;

impl ParamKind {
    pub fn default_value(&self) -> ParamValue {
        match *self {
            ParamKind::Float { default, .. } => ParamValue::Number(default),
            ParamKind::Color { default } => ParamValue::Color(default),
            ParamKind::Toggle { default } => ParamValue::Toggle(default),
            ParamKind::Seed { default } => ParamValue::Number(default.min(MAX_SEED) as f32),
        }
    }

    /// `value` made valid for this kind: clamped to the range, whole for seeds, finite. `None`
    /// when it is the wrong type (a colour for a slider).
    pub fn sanitize(&self, value: ParamValue) -> Option<ParamValue> {
        match (*self, value) {
            (ParamKind::Float { min, max, default }, ParamValue::Number(n)) => {
                let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
                Some(ParamValue::Number(if n.is_finite() { n.clamp(lo, hi) } else { default }))
            }
            (ParamKind::Color { default }, ParamValue::Color(c)) => {
                Some(ParamValue::Color(if c.iter().all(|v| v.is_finite()) { c.map(|v| v.max(0.0)) } else { default }))
            }
            (ParamKind::Toggle { .. }, ParamValue::Toggle(b)) => Some(ParamValue::Toggle(b)),
            (ParamKind::Seed { default }, ParamValue::Number(n)) => {
                Some(ParamValue::Number(if n.is_finite() { n.round().clamp(0.0, MAX_SEED as f32) } else { default.min(MAX_SEED) as f32 }))
            }
            _ => None,
        }
    }
}

/// One exposed parameter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Param {
    /// Name formulas use: letters, digits and `_`, not starting with a digit.
    pub id: String,
    /// Name shown in Properties.
    pub label: String,
    /// Heading the parameter is listed under (empty = no heading).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    /// Hover text.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tip: String,
    #[serde(flatten)]
    pub kind: ParamKind,
    pub value: ParamValue,
}

impl Param {
    fn new(id: &str, label: &str, kind: ParamKind) -> Self {
        let value = kind.default_value();
        Self { id: id.into(), label: label.into(), group: String::new(), tip: String::new(), kind, value }
    }

    pub fn float(id: &str, label: &str, min: f32, max: f32, default: f32) -> Self {
        Self::new(id, label, ParamKind::Float { min, max, default })
    }

    pub fn color(id: &str, label: &str, default: [f32; 3]) -> Self {
        Self::new(id, label, ParamKind::Color { default })
    }

    pub fn toggle(id: &str, label: &str, default: bool) -> Self {
        Self::new(id, label, ParamKind::Toggle { default })
    }

    pub fn seed(id: &str, label: &str, default: u32) -> Self {
        Self::new(id, label, ParamKind::Seed { default })
    }

    pub fn group(mut self, group: &str) -> Self {
        self.group = group.into();
        self
    }

    pub fn tip(mut self, tip: &str) -> Self {
        self.tip = tip.into();
        self
    }

    pub fn is_default(&self) -> bool {
        self.value == self.kind.default_value()
    }
}

/// Whether `id` can name a parameter in a formula.
pub fn valid_param_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && id.len() <= 48
        && !material_expr::FUNCTIONS.iter().any(|(f, _)| *f == id)
}

/// The property a binding writes. With [`Binding::effect`] unset it is a property of the layer
/// (its fill, blend or visibility); with it set, of that mask or content effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "prop", rename_all = "snake_case")]
pub enum Prop {
    /// Layer or effect shown (formula >= 0.5) or hidden.
    Visible,
    /// Layer: blend opacity of one channel, or of every channel. Effect: its opacity.
    Opacity {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        channel: Option<ChannelKind>,
    },
    /// Value of a uniform fill channel.
    Value { channel: ChannelKind },
    /// Dark / bright end of a procedural fill channel.
    Low { channel: ChannelKind },
    High { channel: ChannelKind },
    /// Pattern scale / balance / contrast / seed of a procedural fill channel (every procedural
    /// channel of the layer when `channel` is unset), or of a procedural mask fill.
    PatternScale {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        channel: Option<ChannelKind>,
    },
    PatternBalance {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        channel: Option<ChannelKind>,
    },
    PatternContrast {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        channel: Option<ChannelKind>,
    },
    PatternSeed {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        channel: Option<ChannelKind>,
    },
    /// Strength of a fill channel (bump scale for Height).
    Strength { channel: ChannelKind },
    /// Tiling of the fill's projection (both axes).
    Tiling,
    /// Rotation of the fill's projection, degrees.
    Rotation,
    /// Generator: threshold (`level`). The panel shows Dirt / AO levels as `1 - level`.
    Level,
    /// Generator: transition width (the panel's Contrast is `1 - softness`).
    Softness,
    /// Generator: grunge / breakup amount.
    Breakup,
    /// Generator: breakup pattern scale (the panel's Pattern size is `100 / scale` percent).
    BreakupScale,
    BreakupSeed,
    /// Generator: fine grain, grunge floor, Dirt's edges masking, wear's AO masking and
    /// smoothness (`PainterControls`).
    Grain,
    GrungeFloor,
    EdgesMasking,
    AoMasking,
    Smoothness,
    /// Edge wear: reach as percent of the model's longest side (generators with natural spread).
    Spread,
    /// Mask fill: its grey value.
    FillValue,
}

/// "This property of that layer is this formula of the parameters."
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Binding {
    /// The layer carrying the controls, or a layer inside it.
    pub layer: LayerId,
    /// A mask effect or content effect of that layer; unset for the layer itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<EffectId>,
    #[serde(flatten)]
    pub prop: Prop,
    /// See [`crate::material_expr`].
    pub formula: String,
}

impl Binding {
    pub fn layer(layer: LayerId, prop: Prop, formula: &str) -> Self {
        Self { layer, effect: None, prop, formula: formula.into() }
    }

    pub fn effect(layer: LayerId, effect: EffectId, prop: Prop, formula: &str) -> Self {
        Self { layer, effect: Some(effect), prop, formula: formula.into() }
    }
}

/// Why a binding did nothing (reported by [`MaterialControls::check`], skipped by `apply`).
#[derive(Clone, Debug, PartialEq)]
pub enum BindingProblem {
    Formula(ExprError),
    /// The layer or effect is gone (the user deleted it): harmless, the binding is idle.
    TargetMissing,
    /// The target exists but has no such property (a Level on a paint effect, a Low on a
    /// uniform channel the user switched to an image, ...).
    NotApplicable,
}

/// A layer's exposed parameters and what they drive.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MaterialControls {
    #[serde(default)]
    pub params: Vec<Param>,
    #[serde(default)]
    pub bindings: Vec<Binding>,
}

impl MaterialControls {
    pub fn param(&self, id: &str) -> Option<&Param> {
        self.params.iter().find(|p| p.id == id)
    }

    /// Set a parameter (sanitized for its kind). `false` when there is no such parameter, the
    /// value has the wrong type, or nothing changed.
    pub fn set(&mut self, id: &str, value: ParamValue) -> bool {
        let Some(p) = self.params.iter_mut().find(|p| p.id == id) else { return false };
        let Some(v) = p.kind.sanitize(value) else { return false };
        if p.value == v {
            return false;
        }
        p.value = v;
        true
    }

    /// Put every parameter back to its default. `false` when all were there already.
    pub fn reset(&mut self) -> bool {
        let mut changed = false;
        for p in &mut self.params {
            let d = p.kind.default_value();
            changed |= p.value != d;
            p.value = d;
        }
        changed
    }

    pub fn is_default(&self) -> bool {
        self.params.iter().all(Param::is_default)
    }

    fn lookup(&self) -> impl Fn(&str) -> Option<Value> + '_ {
        |id| self.param(id).map(|p| p.value.as_value())
    }

    /// Write every binding's value into `root` (the layer carrying these controls) and the
    /// layers inside it. Bindings that cannot apply are skipped. Returns whether a layer changed.
    pub fn apply(&self, root: &mut Layer) -> bool {
        let lookup = self.lookup();
        let mut changed = false;
        for b in &self.bindings {
            let Ok(v) = material_expr::eval(&b.formula, &lookup) else { continue };
            if !v.iter().all(|c| c.is_finite()) {
                continue;
            }
            // Read first: an unchanged property must not copy-on-write the layer (the compositor
            // sees a new `Arc` as an edit).
            if with_layer(root, b.layer, &mut |l| write(&mut l.clone(), b, v)) != Some(Ok(true)) {
                continue;
            }
            with_layer_mut(root, b.layer, &mut |l| write(l, b, v));
            changed = true;
        }
        changed
    }

    /// Every binding with a problem, by index: for tests of built-in materials and for the
    /// editor to show what is idle.
    pub fn check(&self, root: &Layer) -> Vec<(usize, BindingProblem)> {
        let lookup = self.lookup();
        let mut out = Vec::new();
        for (i, b) in self.bindings.iter().enumerate() {
            let v = match material_expr::eval(&b.formula, &lookup) {
                Ok(v) => v,
                Err(e) => {
                    out.push((i, BindingProblem::Formula(e)));
                    continue;
                }
            };
            match with_layer(root, b.layer, &mut |l| write(&mut l.clone(), b, v)) {
                None => out.push((i, BindingProblem::TargetMissing)),
                Some(Err(p)) => out.push((i, p)),
                Some(Ok(_)) => {}
            }
        }
        out
    }

    /// Structural problems of the definition itself (duplicate or invalid ids, values out of
    /// range). Empty for a well-formed material.
    pub fn validate(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (i, p) in self.params.iter().enumerate() {
            if !valid_param_id(&p.id) {
                out.push(format!("parameter id '{}' is not a valid name", p.id));
            }
            if self.params[..i].iter().any(|q| q.id == p.id) {
                out.push(format!("parameter id '{}' is used twice", p.id));
            }
            if p.kind.sanitize(p.value) != Some(p.value) {
                out.push(format!("parameter '{}' has a value outside its range", p.id));
            }
            if p.kind.sanitize(p.kind.default_value()) != Some(p.kind.default_value()) {
                out.push(format!("parameter '{}' has a default outside its range", p.id));
            }
        }
        out
    }

    /// Make a definition read from a file safe to use: invalid or duplicate parameters and
    /// everything past the size limits are dropped, values are put in range.
    pub fn sanitized(mut self) -> Self {
        let mut seen = std::collections::HashSet::new();
        self.params.retain(|p| valid_param_id(&p.id) && seen.insert(p.id.clone()));
        self.params.truncate(MAX_PARAMS);
        for p in &mut self.params {
            p.value = p.kind.sanitize(p.value).unwrap_or(p.kind.default_value());
        }
        self.bindings.truncate(MAX_BINDINGS);
        self
    }

    /// Ids rewritten after a copy with fresh ids (see `Layer::duplicate_with_new_ids`).
    pub(crate) fn remap(&mut self, layers: &std::collections::HashMap<LayerId, LayerId>, effects: &std::collections::HashMap<EffectId, EffectId>) {
        for b in &mut self.bindings {
            if let Some(new) = layers.get(&b.layer) {
                b.layer = *new;
            }
            if let Some(new) = b.effect.and_then(|e| effects.get(&e)) {
                b.effect = Some(*new);
            }
        }
    }
}

fn with_layer<R>(root: &Layer, id: LayerId, f: &mut dyn FnMut(&Layer) -> R) -> Option<R> {
    if root.id == id {
        return Some(f(root));
    }
    root.children().iter().find_map(|c| with_layer(c, id, f))
}

fn with_layer_mut<R>(root: &mut Layer, id: LayerId, f: &mut dyn FnMut(&mut Layer) -> R) -> Option<R> {
    if root.id == id {
        return Some(f(root));
    }
    // Find the branch first, so only the layers on the path are made unique.
    let index = root.children().iter().position(|c| with_layer(c, id, &mut |_| ()).is_some())?;
    let child = std::sync::Arc::make_mut(&mut root.children_mut()?[index]);
    with_layer_mut(child, id, f)
}

fn channel_value(kind: ChannelKind, v: Value) -> ChannelValue {
    if kind.components() >= 3 { ChannelValue::rgb(v[0], v[1], v[2]) } else { ChannelValue::scalar(v[0]) }
}

/// Assign and report whether the value changed.
fn put<T: PartialEq>(slot: &mut T, v: T) -> bool {
    if *slot == v {
        return false;
    }
    *slot = v;
    true
}

fn seed(v: Value) -> u32 {
    v[0].round().clamp(0.0, u32::MAX as f32) as u32
}

const SCALE_RANGE: (f32, f32) = (0.01, 1000.0);

/// Apply a pattern property to procedural parameters.
fn write_pattern(p: &mut ProceduralParams, prop: Prop, v: Value) -> bool {
    match prop {
        Prop::PatternScale { .. } => put(&mut p.scale, v[0].clamp(SCALE_RANGE.0, SCALE_RANGE.1)),
        Prop::PatternBalance { .. } => put(&mut p.balance, v[0].clamp(0.0, 1.0)),
        Prop::PatternContrast { .. } => put(&mut p.contrast, v[0].clamp(0.0, 1.0)),
        Prop::PatternSeed { .. } => put(&mut p.seed, seed(v)),
        _ => false,
    }
}

/// Write one binding's value into its layer: `Ok(changed)`, or why it does not apply.
fn write(l: &mut Layer, b: &Binding, v: Value) -> Result<bool, BindingProblem> {
    match b.effect {
        None => write_layer(l, b.prop, v),
        Some(effect) => write_effect(l, effect, b.prop, v),
    }
}

fn write_layer(l: &mut Layer, prop: Prop, v: Value) -> Result<bool, BindingProblem> {
    let na = Err(BindingProblem::NotApplicable);
    match prop {
        Prop::Visible => return Ok(put(&mut l.visible, v[0] >= 0.5)),
        Prop::Opacity { channel } => {
            let o = v[0].clamp(0.0, 1.0);
            let mut changed = false;
            let mut any = false;
            for (_, blend) in l.blend.iter_mut().filter(|(k, _)| channel.is_none_or(|c| c == **k)) {
                any = true;
                changed |= put(&mut blend.opacity, o);
            }
            return if any { Ok(changed) } else { na };
        }
        _ => {}
    }
    let LayerContent::Fill(f) = &mut l.content else { return na };
    match prop {
        Prop::Tiling => {
            let s = v[0].clamp(0.001, 1000.0);
            Ok(put(&mut f.projection.scale, [s, s]))
        }
        Prop::Rotation => Ok(put(&mut f.projection.rotation_deg, v[0].clamp(-360.0, 360.0))),
        Prop::Strength { channel } => {
            let Some(ch) = f.channels.get_mut(&channel) else { return na };
            Ok(put(&mut ch.strength, v[0].clamp(-100.0, 100.0)))
        }
        Prop::Value { channel } => match f.channels.get_mut(&channel).map(|c| &mut c.source) {
            Some(FillSource::Uniform { value }) => Ok(put(value, channel_value(channel, v))),
            _ => na,
        },
        Prop::Low { channel } | Prop::High { channel } => match f.channels.get_mut(&channel).map(|c| &mut c.source) {
            Some(FillSource::Procedural { low, high, .. }) => {
                Ok(put(if matches!(prop, Prop::Low { .. }) { low } else { high }, channel_value(channel, v)))
            }
            _ => na,
        },
        Prop::PatternScale { channel } | Prop::PatternBalance { channel } | Prop::PatternContrast { channel } | Prop::PatternSeed { channel } => {
            let mut changed = false;
            let mut any = false;
            for (_, ch) in f.channels.iter_mut().filter(|(k, _)| channel.is_none_or(|c| c == **k)) {
                if let FillSource::Procedural { params, .. } = &mut ch.source {
                    any = true;
                    changed |= write_pattern(params, prop, v);
                }
            }
            if any { Ok(changed) } else { na }
        }
        _ => na,
    }
}

fn write_effect(l: &mut Layer, effect: EffectId, prop: Prop, v: Value) -> Result<bool, BindingProblem> {
    let na = Err(BindingProblem::NotApplicable);
    if let Some(e) = l.content_effects.iter_mut().find(|e| e.id == effect) {
        // Read before `make_mut`, as in `MaterialControls::apply`.
        return match prop {
            Prop::Visible => {
                let on = v[0] >= 0.5;
                Ok(e.visible != on && put(&mut std::sync::Arc::make_mut(e).visible, on))
            }
            Prop::Opacity { channel } => {
                let o = v[0].clamp(0.0, 1.0);
                let hit = |k: &ChannelKind| channel.is_none_or(|c| c == *k);
                if !e.blend.keys().any(hit) {
                    return na;
                }
                if e.blend.iter().filter(|(k, _)| hit(k)).all(|(_, b)| b.opacity == o) {
                    return Ok(false);
                }
                for (_, b) in std::sync::Arc::make_mut(e).blend.iter_mut().filter(|(k, _)| hit(k)) {
                    b.opacity = o;
                }
                Ok(true)
            }
            _ => na,
        };
    }
    let Some(e) = l.mask.as_mut().and_then(|m| m.effects.iter_mut().find(|e| e.id == effect)) else {
        return Err(BindingProblem::TargetMissing);
    };
    let mut next = (**e).clone();
    let changed = match prop {
        Prop::Visible => put(&mut next.visible, v[0] >= 0.5),
        Prop::Opacity { channel: None } => put(&mut next.opacity, v[0].clamp(0.0, 1.0)),
        _ => match &mut next.kind {
            MaskEffectKind::Generator(g) => {
                let unit = v[0].clamp(0.0, 1.0);
                match prop {
                    Prop::Level => put(&mut g.level, unit),
                    Prop::Softness => put(&mut g.softness, v[0].clamp(0.001, 1.0)),
                    Prop::Breakup => put(&mut g.noise_amount, unit),
                    Prop::BreakupScale => put(&mut g.noise.scale, v[0].clamp(1.0, 100.0)),
                    Prop::BreakupSeed => put(&mut g.noise.seed, seed(v)),
                    Prop::Grain | Prop::GrungeFloor | Prop::EdgesMasking | Prop::AoMasking | Prop::Smoothness => {
                        let mut pc = g.painter.unwrap_or_default();
                        match prop {
                            Prop::Grain => pc.grain = unit,
                            Prop::GrungeFloor => pc.grunge_floor = unit,
                            Prop::EdgesMasking => pc.edges_masking = unit,
                            Prop::AoMasking => pc.ao_masking = unit,
                            _ => pc.smoothness = unit,
                        }
                        // Untouched generators keep `painter: None` (their exact older look).
                        if g.painter.is_none() && pc == Default::default() { false } else { put(&mut g.painter, Some(pc)) }
                    }
                    Prop::Spread => match &mut g.edge_spread {
                        Some(s) => put(&mut s.distance, v[0].clamp(0.0, 100.0)),
                        None => return na,
                    },
                    _ => return na,
                }
            }
            MaskEffectKind::Fill { value, procedural, .. } => match (prop, procedural) {
                (Prop::FillValue, _) => put(value, v[0].clamp(0.0, 1.0)),
                (Prop::PatternScale { .. } | Prop::PatternBalance { .. } | Prop::PatternContrast { .. } | Prop::PatternSeed { .. }, Some(p)) => write_pattern(p, prop, v),
                _ => return na,
            },
            _ => return na,
        },
    };
    if changed {
        *e = std::sync::Arc::new(next);
    }
    Ok(changed)
}

impl Layer {
    /// Re-apply this layer's material bindings (after its parameters changed). Returns whether
    /// any layer changed.
    pub fn apply_material(&mut self) -> bool {
        match self.material.clone() {
            Some(m) => m.apply(self),
            None => false,
        }
    }
}

mod author;
pub use author::{Exposable, exposable};

#[cfg(test)]
mod tests;
