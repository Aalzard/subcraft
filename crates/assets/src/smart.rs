//! Smart materials: folders of material layers masked by generators (wear on edges, dirt in
//! cavities, dust on top). Edges / Cavity read baked curvature / AO, so bake first.

use std::sync::Arc;

use subcraft_core::{AssetId, ChannelKind};
use subcraft_doc::{BlendMode, GeneratorKind, Layer, LayerContent, Mask, MaskEffect, MaskEffectKind, ProceduralKind, ProceduralParams, TextureSet};

use crate::presets::{MaterialPreset, generator, materials};

#[derive(Clone)]
pub struct SmartMaterial {
    pub id: AssetId,
    pub name: String,
    build: fn(&TextureSet) -> Layer,
}

impl std::fmt::Debug for SmartMaterial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmartMaterial").field("name", &self.name).finish()
    }
}

impl SmartMaterial {
    /// The folder layer for `ts`.
    pub fn build(&self, ts: &TextureSet) -> Layer {
        (self.build)(ts)
    }
}

fn base(name: &str) -> MaterialPreset {
    materials().into_iter().find(|m| m.name == name).expect("built-in material exists")
}

/// A uniform-color dielectric layer.
fn coat(ts: &TextureSet, name: &str, color: [f32; 3], rough: f32, metal: f32) -> Layer {
    let m = MaterialPreset {
        id: AssetId::from_name(name),
        name: name.into(),
        category: "",
        channels: vec![
            (ChannelKind::BaseColor, subcraft_doc::FillSource::uniform(subcraft_core::ChannelValue::rgb(color[0], color[1], color[2]))),
            (ChannelKind::Roughness, subcraft_doc::FillSource::uniform(subcraft_core::ChannelValue::scalar(rough))),
            (ChannelKind::Metallic, subcraft_doc::FillSource::uniform(subcraft_core::ChannelValue::scalar(metal))),
        ],
        projection: Default::default(),
    };
    m.build(ts)
}

fn masked(mut layer: Layer, ts: &TextureSet, effects: Vec<MaskEffect>) -> Layer {
    let mut mask = Mask::new(ts.resolution, false);
    mask.effects = effects.into_iter().map(Arc::new).collect();
    layer.mask = Some(mask);
    layer
}

fn fx(kind: MaskEffectKind) -> MaskEffect {
    MaskEffect::new(kind)
}

fn fx_blend(kind: MaskEffectKind, blend: BlendMode) -> MaskEffect {
    MaskEffect { blend, ..MaskEffect::new(kind) }
}

fn grunge(scale: f32, seed: u32) -> MaskEffectKind {
    MaskEffectKind::Fill {
        value: 1.0,
        procedural: Some(ProceduralParams { scale, seed, contrast: 0.3, ..ProceduralParams::new(ProceduralKind::Grunge) }),
        projection: subcraft_doc::Projection { mode: subcraft_doc::ProjectionMode::Triplanar, scale: [2.0, 2.0], ..Default::default() },
    }
}

fn folder(name: &str, ts: &TextureSet, children: Vec<Layer>) -> Layer {
    let mut f = Layer::new_folder(name, ts);
    if let LayerContent::Group(g) = &mut f.content {
        g.children = children.into_iter().map(Arc::new).collect();
        g.expanded = false;
    }
    f
}

fn worn_painted_metal(ts: &TextureSet) -> Layer {
    let paint = masked(coat(ts, "Paint", [0.12, 0.2, 0.32], 0.5, 0.0), ts, vec![fx(generator(GeneratorKind::Edges, 0.6, 0.08, 0.4, true))]);
    let dirt = masked(coat(ts, "Dirt", [0.06, 0.045, 0.03], 0.9, 0.0), ts, vec![fx(generator(GeneratorKind::Cavity, 0.25, 0.25, 0.35, false))]);
    folder("Worn Painted Metal", ts, vec![base("Steel").build(ts), paint, dirt])
}

fn rusty_iron(ts: &TextureSet) -> Layer {
    let rust = masked(
        base("Rust").build(ts),
        ts,
        vec![fx(generator(GeneratorKind::Cavity, 0.2, 0.35, 0.5, false)), fx_blend(grunge(5.0, 7), BlendMode::Multiply)],
    );
    folder("Rusty Iron", ts, vec![base("Iron Rough").build(ts), rust])
}

fn dusty_plastic(ts: &TextureSet) -> Layer {
    let dust = masked(coat(ts, "Dust", [0.5, 0.48, 0.44], 0.95, 0.0), ts, vec![fx(generator(GeneratorKind::Directional, 0.72, 0.25, 0.45, false))]);
    folder("Dusty Plastic", ts, vec![base("Plastic Glossy Red").build(ts), dust])
}

fn old_wood(ts: &TextureSet) -> Layer {
    let grime = masked(coat(ts, "Grime", [0.07, 0.045, 0.025], 0.85, 0.0), ts, vec![fx(generator(GeneratorKind::Cavity, 0.25, 0.3, 0.3, false))]);
    let worn = masked(coat(ts, "Worn Edges", [0.55, 0.38, 0.2], 0.7, 0.0), ts, vec![fx(generator(GeneratorKind::Edges, 0.62, 0.1, 0.4, false))]);
    folder("Old Wood", ts, vec![base("Wood").build(ts), grime, worn])
}

fn tarnished_gold(ts: &TextureSet) -> Layer {
    let tarnish = masked(coat(ts, "Tarnish", [0.12, 0.09, 0.05], 0.7, 0.3), ts, vec![fx(generator(GeneratorKind::Cavity, 0.2, 0.3, 0.3, false))]);
    folder("Tarnished Gold", ts, vec![base("Gold").build(ts), tarnish])
}

fn smart(name: &str, build: fn(&TextureSet) -> Layer) -> SmartMaterial {
    SmartMaterial { id: AssetId::from_name(&format!("smart_material/{name}")), name: name.into(), build }
}

pub fn smart_materials() -> Vec<SmartMaterial> {
    vec![
        smart("Worn Painted Metal", worn_painted_metal),
        smart("Rusty Iron", rusty_iron),
        smart("Dusty Plastic", dusty_plastic),
        smart("Old Wood", old_wood),
        smart("Tarnished Gold", tarnished_gold),
    ]
}
