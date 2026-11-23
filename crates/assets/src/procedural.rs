//! Built-in procedural materials (ADR-0047): layer stacks made only of this project's own fills,
//! patterns and generators, with exposed parameters (colour, wear, dirt, scale, seed, ...) that
//! stay editable after the material is applied. Edges / Cavity / Directional generators read the
//! baked curvature and occlusion, so bake first; without maps those layers stay clean.
//!
//! Every material is built at its parameter defaults: a parameter left alone changes nothing, and
//! "Reset" returns exactly to what is built here.

use std::sync::Arc;

use subcraft_core::ChannelKind::{self, *};
use subcraft_doc::{
    Binding, BlendMode, FillSource, Generator, GeneratorKind, Layer, LayerContent, MaskEffectKind, MaterialControls, Param, ProceduralKind, Prop,
    TextureSet,
};

use crate::presets::{dirt_generator, proc, triplanar, uni};
use crate::smart::{SmartMaterial, coat, folder, fx, fx_blend, masked, occlusion, smart};
use crate::smart_more::{all_white, mat, pattern, s, top, v};

/// Name prefix of the shelf entries, so they sort together and read as a family.
pub const PREFIX: &str = "Procedural ";

/// Collects a material's parameters and bindings while its layers are built.
#[derive(Default)]
struct Recipe {
    params: Vec<Param>,
    bindings: Vec<Binding>,
}

impl Recipe {
    fn param(&mut self, p: Param) {
        self.params.push(p);
    }

    /// The Scale and Seed every material ends with.
    fn pattern_params(&mut self) {
        self.param(Param::float("scale", "Scale", 0.25, 4.0, 1.0).group("Pattern").tip("Size of every pattern in the material: above 1 is finer, below 1 is larger"));
        self.param(Param::seed("seed", "Seed", 0).group("Pattern").tip("Another seed gives the same material with different random details"));
    }

    fn bind(&mut self, layer: &Layer, prop: Prop, formula: &str) {
        self.bindings.push(Binding::layer(layer.id, prop, formula));
    }

    /// Bind a property of mask effect `index` (bottom to top) of `layer`.
    fn bind_fx(&mut self, layer: &Layer, index: usize, prop: Prop, formula: &str) {
        let effect = layer.mask.as_ref().expect("the layer has a mask").effects[index].id;
        self.bindings.push(Binding::effect(layer.id, effect, prop, formula));
    }

    /// A uniform channel follows a formula.
    fn value(&mut self, layer: &Layer, channel: ChannelKind, formula: &str) {
        self.bind(layer, Prop::Value { channel }, formula);
    }

    /// The two ends of a procedural channel follow formulas.
    fn range(&mut self, layer: &Layer, channel: ChannelKind, low: &str, high: &str) {
        self.bind(layer, Prop::Low { channel }, low);
        self.bind(layer, Prop::High { channel }, high);
    }

    /// Roughness as a base value with a variation around it.
    fn roughness(&mut self, layer: &Layer, base: &str, variation: &str) {
        self.range(layer, Roughness, &format!("clamp({base} - {variation} * 0.5, 0.02, 1)"), &format!("clamp({base} + {variation} * 0.5, 0.02, 1)"));
    }

    /// Base colour as a colour with a brightness variation around it.
    fn color(&mut self, layer: &Layer, color: &str, variation: &str) {
        self.range(layer, BaseColor, &format!("{color} * (1 - {variation} * 0.5)"), &format!("{color} * (1 + {variation} * 0.5)"));
    }

    /// Every pattern of `layer` (its fill, its mask fills, its generators' breakup) follows the
    /// material's Scale and Seed, starting from the values the layer was built with.
    fn follow_pattern(&mut self, layer: &Layer) {
        if let LayerContent::Fill(f) = &layer.content {
            let mut procedural = false;
            for (kind, ch) in &f.channels {
                if let FillSource::Procedural { params, .. } = &ch.source {
                    procedural = true;
                    self.bind(layer, Prop::PatternSeed { channel: Some(*kind) }, &format!("seed + {}", params.seed));
                }
            }
            if procedural {
                self.bind(layer, Prop::Tiling, &format!("{} * scale", f.projection.scale[0]));
            }
        }
        let effects = layer.mask.as_ref().map(|m| m.effects.clone()).unwrap_or_default();
        for (i, e) in effects.iter().enumerate() {
            match &e.kind {
                MaskEffectKind::Fill { procedural: Some(p), .. } => {
                    self.bind_fx(layer, i, Prop::PatternScale { channel: None }, &format!("{} * scale", p.scale));
                    self.bind_fx(layer, i, Prop::PatternSeed { channel: None }, &format!("seed + {}", p.seed));
                }
                MaskEffectKind::Generator(g) => {
                    self.bind_fx(layer, i, Prop::BreakupScale, &format!("{} * scale", g.noise.scale));
                    self.bind_fx(layer, i, Prop::BreakupSeed, &format!("seed + {}", g.noise.seed));
                }
                _ => {}
            }
        }
    }

    /// Attach the controls to `root` and settle the bound properties at the defaults.
    fn finish(self, mut root: Layer) -> Layer {
        root.material = Some(Arc::new(MaterialControls { params: self.params, bindings: self.bindings }));
        root.apply_material();
        root
    }
}

/// The owner-tuned edge wear generator (Painter-style model with natural spread).
fn wear() -> MaskEffectKind {
    MaskEffectKind::Generator(Generator::new(GeneratorKind::Edges))
}

fn color(id: &str, label: &str, c: [f32; 3]) -> Param {
    Param::color(id, label, c)
}

fn unit(id: &str, label: &str, default: f32) -> Param {
    Param::float(id, label, 0.0, 1.0, default)
}

/// Default of every amount that drives a Dirt generator: the generator's own Dirt Level, so a
/// material at its defaults carries exactly the tuned generator (`1 - DIRT_LEVEL` is its level).
const DIRT_LEVEL: f32 = 0.75;

/// A dirt layer in the occluded areas: its amount is the generator's Dirt Level and fades the
/// layer out near 0, so 0 is really clean. `strength` is the layer's opacity at full amount.
fn dirt_layer(r: &mut Recipe, ts: &TextureSet, name: &str, param: &str, strength: f32) -> Layer {
    let dirt = masked(coat(ts, name, [0.05, 0.04, 0.035], 0.85, 0.0), ts, vec![fx(dirt_generator())]);
    r.value(&dirt, BaseColor, &format!("{param}_color"));
    r.bind_fx(&dirt, 0, Prop::Level, &format!("1 - {param}"));
    r.bind(&dirt, Prop::Opacity { channel: None }, &format!("saturate({param} * 4) * {strength}"));
    dirt
}

fn dirt_params(r: &mut Recipe, id: &str, label: &str, c: [f32; 3]) {
    r.param(unit(id, label, DIRT_LEVEL).group(label).tip("How far it creeps out of the creases (reads the baked ambient occlusion)"));
    r.param(color(&format!("{id}_color"), "Color", c).group(label));
}

// ----- materials ------------------------------------------------------------------------------

fn painted_metal(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("paint_color", "Color", [0.07, 0.19, 0.33]).group("Paint"));
    r.param(unit("paint_roughness", "Roughness", 0.42).group("Paint"));
    r.param(Param::float("paint_thickness", "Thickness", 0.0, 1.0, 0.35).group("Paint").tip("Height of the paint over the bare metal: chips get a visible edge"));
    r.param(color("metal_color", "Color", [0.56, 0.57, 0.58]).group("Metal"));
    r.param(unit("metal_roughness", "Roughness", 0.45).group("Metal"));
    r.param(unit("wear", "Edge wear", 0.5).group("Wear").tip("Paint worn off the edges (reads the baked curvature)"));
    r.param(unit("chips", "Chips", 0.35).group("Wear").tip("Flaked patches away from the edges"));
    dirt_params(&mut r, "dirt", "Dirt", [0.05, 0.04, 0.035]);
    r.pattern_params();

    let metal = mat(ts, "Bare Metal", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 9.0, 71, v(0.42, 0.42, 0.43), v(0.58, 0.58, 0.59))),
        (Roughness, proc(ProceduralKind::Grunge, 8.0, 72, s(0.35), s(0.65))),
        (Metallic, uni(1.0)),
    ], triplanar(2.0));
    r.color(&metal, "metal_color", "0.3");
    r.roughness(&metal, "metal_roughness", "0.3");
    r.follow_pattern(&metal);

    let paint = masked(
        mat(ts, "Paint", vec![
            (BaseColor, proc(ProceduralKind::Clouds, 3.0, 73, v(0.1, 0.1, 0.1), v(0.1, 0.1, 0.1))),
            (Roughness, proc(ProceduralKind::Dirt, 5.0, 74, s(0.4), s(0.5))),
            (Metallic, uni(0.0)),
            (Height, uni(0.0)),
        ], triplanar(2.0)),
        ts,
        vec![fx(all_white()), fx_blend(wear(), BlendMode::Subtract), fx_blend(pattern(ProceduralKind::Clouds, 7.0, 14, 0.66, 0.92), BlendMode::Subtract)],
    );
    r.color(&paint, "paint_color", "0.16");
    r.roughness(&paint, "paint_roughness", "0.14");
    r.value(&paint, Height, "paint_thickness * 0.04");
    r.bind_fx(&paint, 1, Prop::Level, "wear");
    r.bind_fx(&paint, 1, Prop::Opacity { channel: None }, "saturate(wear * 20)");
    r.bind_fx(&paint, 2, Prop::PatternBalance { channel: None }, "remap(chips, 0.75, 0.5)");
    r.bind_fx(&paint, 2, Prop::Opacity { channel: None }, "saturate(chips * 5)");
    r.follow_pattern(&paint);

    let dirt = dirt_layer(&mut r, ts, "Dirt", "dirt", 1.0);
    r.follow_pattern(&dirt);
    r.finish(folder(&format!("{PREFIX}Painted Metal"), ts, vec![metal, paint, occlusion(ts), dirt]))
}

fn rusted_steel(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("steel_color", "Color", [0.5, 0.51, 0.53]).group("Steel"));
    r.param(unit("steel_roughness", "Roughness", 0.42).group("Steel"));
    r.param(unit("rust", "Amount", 0.5).group("Rust").tip("How far the rust spreads out of the creases and over open surfaces"));
    r.param(color("rust_color", "Color", [0.17, 0.052, 0.018]).group("Rust"));
    r.param(unit("rust_contrast", "Edge sharpness", 0.6).group("Rust").tip("Soft blooms of rust, or hard-edged patches"));
    r.param(unit("pitting", "Pitting", 0.5).group("Rust").tip("How much the rust is raised and rough"));
    r.pattern_params();

    let steel = mat(ts, "Steel", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 7.0, 81, v(0.4, 0.4, 0.4), v(0.5, 0.5, 0.5))),
        (Roughness, proc(ProceduralKind::Grunge, 8.0, 82, s(0.3), s(0.5))),
        (Metallic, uni(1.0)),
    ], triplanar(2.0));
    r.color(&steel, "steel_color", "0.25");
    r.roughness(&steel, "steel_roughness", "0.3");
    r.follow_pattern(&steel);

    // A thin brown stain around the rust, then the rust itself.
    let stain = masked(coat(ts, "Rust Stain", [0.2, 0.09, 0.04], 0.7, 0.4), ts, vec![
        fx(dirt_generator()),
        fx_blend(pattern(ProceduralKind::Grunge, 5.0, 83, 0.55, 0.45), BlendMode::LinearDodge),
        fx_blend(pattern(ProceduralKind::Clouds, 3.0, 84, 0.5, 0.5), BlendMode::Multiply),
    ]);
    r.value(&stain, BaseColor, "rust_color * 0.3");
    r.bind_fx(&stain, 0, Prop::Level, "remap(rust, 0.5, 0)");
    r.bind_fx(&stain, 1, Prop::PatternBalance { channel: None }, "remap(rust, 1.05, 0.3)");
    r.bind(&stain, Prop::Opacity { channel: None }, "saturate(rust * 3) * 0.7");
    r.follow_pattern(&stain);

    let rust = masked(
        mat(ts, "Rust", vec![
            (BaseColor, proc(ProceduralKind::Clouds, 14.0, 85, v(0.2, 0.07, 0.02), v(0.5, 0.2, 0.06))),
            (Roughness, proc(ProceduralKind::Clouds, 8.0, 86, s(0.75), s(0.95))),
            (Metallic, uni(0.0)),
            (Height, proc(ProceduralKind::Clouds, 16.0, 87, s(0.0), s(0.03))),
        ], triplanar(2.0)),
        ts,
        vec![fx(dirt_generator()), fx_blend(pattern(ProceduralKind::Grunge, 5.0, 83, 0.62, 0.632), BlendMode::LinearDodge)],
    );
    r.range(&rust, BaseColor, "rust_color * 0.4", "rust_color * rgb(1.15, 1.05, 0.95)");
    r.range(&rust, Height, "0", "pitting * 0.06");
    r.range(&rust, Roughness, "0.9 - pitting * 0.25", "0.97");
    r.bind_fx(&rust, 0, Prop::Level, "remap(rust, 0.5, 0)");
    r.bind_fx(&rust, 1, Prop::PatternBalance { channel: None }, "remap(rust, 1.15, 0.38)");
    r.bind_fx(&rust, 1, Prop::PatternContrast { channel: None }, "remap(rust_contrast, 0.2, 0.92)");
    r.bind(&rust, Prop::Opacity { channel: None }, "saturate(rust * 6)");
    r.follow_pattern(&rust);

    // The old, deep rust in the middle of each patch: darker, redder and broken up finely.
    let core = masked(
        mat(ts, "Deep Rust", vec![
            (BaseColor, proc(ProceduralKind::Clouds, 22.0, 88, v(0.05, 0.015, 0.008), v(0.1, 0.03, 0.012))),
            (Roughness, uni(0.95)),
            (Height, proc(ProceduralKind::Clouds, 24.0, 89, s(0.0), s(0.02))),
        ], triplanar(2.0)),
        ts,
        vec![
            fx(pattern(ProceduralKind::Grunge, 5.0, 83, 0.68, 0.6)),
            fx_blend(pattern(ProceduralKind::Clouds, 16.0, 90, 0.4, 0.6), BlendMode::Multiply),
        ],
    );
    r.range(&core, BaseColor, "rust_color * rgb(0.28, 0.24, 0.3)", "rust_color * rgb(0.6, 0.5, 0.55)");
    r.range(&core, Height, "0", "pitting * 0.04");
    r.bind_fx(&core, 0, Prop::PatternBalance { channel: None }, "remap(rust, 1.2, 0.44)");
    r.bind(&core, Prop::Opacity { channel: None }, "saturate(rust * 4) * 0.85");
    r.follow_pattern(&core);
    r.finish(folder(&format!("{PREFIX}Rusted Steel"), ts, vec![steel, stain, rust, core]))
}

fn brushed_metal(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.56, 0.57, 0.58]).group("Metal"));
    r.param(unit("roughness", "Roughness", 0.3).group("Metal"));
    r.param(unit("brushing", "Brushing", 0.5).group("Metal").tip("Strength of the brushed lines in colour, roughness and relief"));
    r.param(Param::float("direction", "Direction", 0.0, 180.0, 0.0).group("Metal").tip("Angle of the brushed lines, degrees"));
    r.param(unit("scratches", "Scratches", 0.35).group("Wear"));
    r.param(unit("smudges", "Smudges", 0.5).group("Wear").tip("Dull, darker patches in the creases"));
    r.pattern_params();

    let metal = mat(ts, "Brushed Metal", vec![
        (BaseColor, proc(ProceduralKind::Brushed, 6.0, 91, v(0.3, 0.31, 0.33), v(0.44, 0.45, 0.47))),
        (Roughness, proc(ProceduralKind::Brushed, 6.0, 92, s(0.2), s(0.4))),
        (Metallic, uni(1.0)),
        (Height, proc(ProceduralKind::Brushed, 6.0, 93, s(0.0), s(0.015))),
    ], triplanar(2.0));
    r.color(&metal, "color", "brushing * 0.5");
    r.roughness(&metal, "roughness", "brushing * 0.4");
    r.range(&metal, Height, "0", "brushing * 0.03");
    r.bind(&metal, Prop::Rotation, "direction");
    r.follow_pattern(&metal);

    let smudges = masked(coat(ts, "Smudges", [0.35, 0.35, 0.36], 0.6, 1.0), ts, vec![
        fx(dirt_generator()),
        fx_blend(pattern(ProceduralKind::Dirt, 4.0, 95, 0.55, 0.1), BlendMode::Multiply),
    ]);
    r.value(&smudges, BaseColor, "color * 0.6");
    r.value(&smudges, Roughness, "clamp(roughness + 0.3, 0, 1)");
    r.bind(&smudges, Prop::Opacity { channel: None }, "saturate(smudges * 2)");
    r.bind_fx(&smudges, 0, Prop::Level, "remap(smudges, 0.5, 0)");
    r.follow_pattern(&smudges);

    let scratches = masked(coat(ts, "Scratches", [0.78, 0.79, 0.8], 0.45, 1.0), ts, vec![fx(pattern(ProceduralKind::Scratches, 5.0, 94, 0.5, 0.6))]);
    r.value(&scratches, BaseColor, "min(color * 1.4, 1)");
    r.value(&scratches, Roughness, "clamp(roughness + 0.15, 0, 1)");
    r.bind(&scratches, Prop::Opacity { channel: None }, "saturate(scratches * 2.5)");
    r.bind_fx(&scratches, 0, Prop::PatternBalance { channel: None }, "remap(scratches, 0.75, 0.3)");
    r.follow_pattern(&scratches);
    r.finish(folder(&format!("{PREFIX}Brushed Metal"), ts, vec![metal, smudges, scratches]))
}

fn polished_metal(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [1.0, 0.766, 0.336]).group("Metal").tip("Gold by default; steel, copper or chrome are one colour away"));
    r.param(unit("roughness", "Roughness", 0.14).group("Metal"));
    r.param(unit("variation", "Roughness variation", 0.25).group("Metal").tip("Fingerprints and handling marks that break the mirror up"));
    r.param(unit("scratches", "Fine scratches", 0.3).group("Wear"));
    dirt_params(&mut r, "tarnish", "Tarnish", [0.14, 0.09, 0.04]);
    r.pattern_params();

    let metal = mat(ts, "Polished Metal", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 4.0, 201, v(0.9, 0.9, 0.9), v(1.0, 1.0, 1.0))),
        (Roughness, proc(ProceduralKind::Dirt, 3.0, 202, s(0.1), s(0.3))),
        (Metallic, uni(1.0)),
    ], triplanar(2.0));
    r.color(&metal, "color", "0.06");
    r.roughness(&metal, "roughness", "variation");
    r.follow_pattern(&metal);

    let scratches = masked(coat(ts, "Fine Scratches", [1.0, 1.0, 1.0], 0.4, 1.0), ts, vec![fx(pattern(ProceduralKind::Scratches, 9.0, 203, 0.5, 0.5))]);
    r.value(&scratches, BaseColor, "color");
    r.value(&scratches, Roughness, "clamp(roughness + 0.25, 0, 1)");
    r.bind(&scratches, Prop::Opacity { channel: None }, "saturate(scratches * 2.5)");
    r.bind_fx(&scratches, 0, Prop::PatternBalance { channel: None }, "remap(scratches, 0.8, 0.35)");
    r.follow_pattern(&scratches);

    let tarnish = dirt_layer(&mut r, ts, "Tarnish", "tarnish", 0.6);
    r.value(&tarnish, Metallic, "0.4");
    r.follow_pattern(&tarnish);
    r.finish(folder(&format!("{PREFIX}Polished Metal"), ts, vec![metal, scratches, tarnish]))
}

fn hammered_metal(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.78, 0.57, 0.22]).group("Metal").tip("Brass by default"));
    r.param(unit("roughness", "Roughness", 0.3).group("Metal"));
    r.param(Param::float("dent_size", "Dent size", 0.25, 4.0, 1.0).group("Hammering").tip("Above 1 is larger dents"));
    r.param(unit("dent_depth", "Dent depth", 0.5).group("Hammering"));
    dirt_params(&mut r, "tarnish", "Tarnish", [0.14, 0.09, 0.04]);
    r.param(unit("polish", "Polished edges", 0.5).group("Wear").tip("Edges rubbed bright (reads the baked curvature)"));
    r.pattern_params();

    let metal = mat(ts, "Hammered Metal", vec![
        (BaseColor, proc(ProceduralKind::Cells, 12.0, 171, v(0.62, 0.45, 0.16), v(0.85, 0.66, 0.28))),
        (Roughness, proc(ProceduralKind::Cells, 12.0, 171, s(0.22), s(0.4))),
        (Metallic, uni(1.0)),
        (Height, proc(ProceduralKind::Cells, 12.0, 171, s(0.0), s(0.1))),
    ], triplanar(2.0));
    r.color(&metal, "color", "0.3");
    r.roughness(&metal, "roughness", "0.2");
    r.range(&metal, Height, "0", "dent_depth * 0.2");
    // One pattern drives colour, roughness and relief, so they share scale and seed.
    r.bind(&metal, Prop::PatternScale { channel: None }, "12 / dent_size");
    r.bind(&metal, Prop::PatternSeed { channel: None }, "seed + 171");
    r.bind(&metal, Prop::Tiling, "2 * scale");

    let tarnish = dirt_layer(&mut r, ts, "Tarnish", "tarnish", 0.9);
    r.value(&tarnish, Metallic, "0.6");
    r.follow_pattern(&tarnish);

    let polish = masked(coat(ts, "Polished Edges", [1.0, 0.8, 0.4], 0.15, 1.0), ts, vec![fx(wear())]);
    r.value(&polish, BaseColor, "min(color * 1.25, 1)");
    r.value(&polish, Roughness, "clamp(roughness - 0.15, 0.04, 1)");
    r.bind_fx(&polish, 0, Prop::Level, "polish");
    r.bind(&polish, Prop::Opacity { channel: None }, "saturate(polish * 20)");
    r.follow_pattern(&polish);
    r.finish(folder(&format!("{PREFIX}Hammered Metal"), ts, vec![metal, tarnish, polish]))
}

fn aged_copper(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.72, 0.36, 0.23]).group("Copper"));
    r.param(unit("roughness", "Roughness", 0.36).group("Copper"));
    r.param(unit("patina", "Amount", 0.5).group("Patina").tip("How far the green patina spreads out of the creases"));
    r.param(color("patina_color", "Color", [0.1, 0.33, 0.27]).group("Patina"));
    r.param(unit("patina_relief", "Relief", 0.5).group("Patina"));
    r.param(unit("polish", "Polished edges", 0.4).group("Wear").tip("Edges rubbed back to bright metal (reads the baked curvature)"));
    r.pattern_params();

    let copper = mat(ts, "Copper", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 8.0, 111, v(0.6, 0.29, 0.19), v(0.8, 0.42, 0.27))),
        (Roughness, proc(ProceduralKind::Clouds, 10.0, 112, s(0.25), s(0.5))),
        (Metallic, uni(1.0)),
    ], triplanar(2.0));
    r.color(&copper, "color", "0.3");
    r.roughness(&copper, "roughness", "0.25");
    r.follow_pattern(&copper);

    let patina = masked(
        mat(ts, "Patina", vec![
            (BaseColor, proc(ProceduralKind::Clouds, 12.0, 113, v(0.08, 0.38, 0.3), v(0.2, 0.62, 0.5))),
            (Roughness, proc(ProceduralKind::Grunge, 16.0, 114, s(0.7), s(0.9))),
            (Metallic, uni(0.0)),
            (Height, proc(ProceduralKind::Grunge, 20.0, 115, s(0.0), s(0.06))),
        ], triplanar(2.0)),
        ts,
        vec![
            fx(dirt_generator()),
            fx_blend(pattern(ProceduralKind::Grunge, 7.0, 116, 0.62, 0.5), BlendMode::LinearDodge),
            fx_blend(pattern(ProceduralKind::Clouds, 8.0, 117, 0.3, 0.4), BlendMode::Multiply),
        ],
    );
    r.color(&patina, "patina_color", "0.7");
    r.range(&patina, Height, "0", "patina_relief * 0.12");
    r.bind_fx(&patina, 0, Prop::Level, "remap(patina, 0.5, 0)");
    r.bind_fx(&patina, 1, Prop::PatternBalance { channel: None }, "remap(patina, 1.2, 0.3)");
    r.bind_fx(&patina, 2, Prop::PatternBalance { channel: None }, "remap(patina, 0.55, 0.05)");
    r.bind(&patina, Prop::Opacity { channel: None }, "saturate(patina * 6)");
    r.follow_pattern(&patina);

    let polish = masked(coat(ts, "Polished Edges", [0.95, 0.6, 0.45], 0.2, 1.0), ts, vec![fx(wear())]);
    r.value(&polish, BaseColor, "min(color * 1.3, 1)");
    r.value(&polish, Roughness, "clamp(roughness - 0.15, 0.04, 1)");
    r.bind_fx(&polish, 0, Prop::Level, "polish");
    r.bind(&polish, Prop::Opacity { channel: None }, "saturate(polish * 20)");
    r.follow_pattern(&polish);
    r.finish(folder(&format!("{PREFIX}Aged Copper"), ts, vec![copper, patina, polish]))
}

fn worn_plastic(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.55, 0.05, 0.04]).group("Plastic"));
    r.param(unit("roughness", "Roughness", 0.35).group("Plastic"));
    r.param(unit("variation", "Roughness variation", 0.3).group("Plastic"));
    r.param(unit("grain", "Molded grain", 0.3).group("Plastic").tip("Fine bumps of textured plastic; 0 is smooth"));
    r.param(unit("scuffs", "Scuffs", 0.4).group("Wear").tip("Pale scratches and scuff marks"));
    r.param(unit("edge_wear", "Edge wear", 0.35).group("Wear").tip("Edges rubbed pale and smooth (reads the baked curvature)"));
    r.param(unit("dust", "Amount", 0.25).group("Dust").tip("Dust settled on the surfaces that face up"));
    r.param(color("dust_color", "Color", [0.5, 0.48, 0.44]).group("Dust"));
    r.pattern_params();

    let plastic = mat(ts, "Plastic", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 4.0, 211, v(0.5, 0.05, 0.04), v(0.6, 0.06, 0.05))),
        (Roughness, proc(ProceduralKind::Dirt, 4.0, 212, s(0.2), s(0.5))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Speckles, 40.0, 213, s(0.0), s(0.01))),
    ], triplanar(2.0));
    r.color(&plastic, "color", "0.1");
    r.roughness(&plastic, "roughness", "variation");
    r.range(&plastic, Height, "0", "grain * 0.03");
    r.follow_pattern(&plastic);

    let scuffs = masked(coat(ts, "Scuffs", [0.8, 0.5, 0.5], 0.65, 0.0), ts, vec![
        fx(pattern(ProceduralKind::Scratches, 4.0, 214, 0.5, 0.5)),
        fx_blend(pattern(ProceduralKind::Clouds, 3.0, 215, 0.5, 0.35), BlendMode::Multiply),
    ]);
    r.value(&scuffs, BaseColor, "mix(color, rgb(0.8, 0.8, 0.8), 0.45)");
    r.value(&scuffs, Roughness, "clamp(roughness + 0.3, 0, 1)");
    r.bind(&scuffs, Prop::Opacity { channel: None }, "saturate(scuffs * 2.5)");
    r.bind_fx(&scuffs, 0, Prop::PatternBalance { channel: None }, "remap(scuffs, 0.8, 0.3)");
    r.follow_pattern(&scuffs);

    let edges = masked(coat(ts, "Worn Edges", [0.8, 0.5, 0.5], 0.5, 0.0), ts, vec![fx(wear())]);
    r.value(&edges, BaseColor, "mix(color, rgb(0.85, 0.85, 0.85), 0.35)");
    r.value(&edges, Roughness, "clamp(roughness + 0.2, 0, 1)");
    r.bind_fx(&edges, 0, Prop::Level, "edge_wear");
    r.bind(&edges, Prop::Opacity { channel: None }, "saturate(edge_wear * 20) * 0.8");
    r.follow_pattern(&edges);

    let dust = masked(coat(ts, "Dust", [0.5, 0.48, 0.44], 0.95, 0.0), ts, vec![fx(top(0.72, 0.45, 0.45)), fx_blend(pattern(ProceduralKind::Clouds, 3.0, 216, 0.4, 0.0), BlendMode::Multiply)]);
    r.value(&dust, BaseColor, "dust_color");
    r.bind_fx(&dust, 0, Prop::Level, "remap(dust, 0.95, 0.35)");
    r.bind(&dust, Prop::Opacity { channel: None }, "saturate(dust * 3) * 0.75");
    r.follow_pattern(&dust);
    r.finish(folder(&format!("{PREFIX}Worn Plastic"), ts, vec![plastic, scuffs, edges, dust]))
}

fn rubber(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.03, 0.03, 0.032]).group("Rubber"));
    r.param(unit("roughness", "Roughness", 0.8).group("Rubber"));
    r.param(unit("texture", "Surface texture", 0.4).group("Rubber").tip("Fine pitted relief; 0 is smooth"));
    r.param(unit("wear", "Edge wear", 0.4).group("Wear").tip("Edges rubbed smooth and slightly shiny (reads the baked curvature)"));
    dirt_params(&mut r, "dust", "Dust", [0.3, 0.27, 0.23]);
    r.pattern_params();

    let base = mat(ts, "Rubber", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 5.0, 221, v(0.03, 0.03, 0.03), v(0.04, 0.04, 0.04))),
        (Roughness, proc(ProceduralKind::Dirt, 5.0, 222, s(0.7), s(0.9))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Pores, 30.0, 223, s(0.0), s(-0.01))),
    ], triplanar(2.0));
    r.color(&base, "color", "0.4");
    r.roughness(&base, "roughness", "0.2");
    r.range(&base, Height, "0", "0 - texture * 0.03");
    r.follow_pattern(&base);

    let worn = masked(coat(ts, "Worn Edges", [0.06, 0.06, 0.06], 0.5, 0.0), ts, vec![fx(wear())]);
    r.value(&worn, BaseColor, "color * 1.6 + 0.01");
    r.value(&worn, Roughness, "clamp(roughness - 0.3, 0.1, 1)");
    r.bind_fx(&worn, 0, Prop::Level, "wear");
    r.bind(&worn, Prop::Opacity { channel: None }, "saturate(wear * 20)");
    r.follow_pattern(&worn);

    let dust = dirt_layer(&mut r, ts, "Dust", "dust", 0.35);
    r.value(&dust, Roughness, "0.95");
    r.follow_pattern(&dust);
    r.finish(folder(&format!("{PREFIX}Rubber"), ts, vec![base, worn, dust]))
}

fn concrete(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.37, 0.365, 0.35]).group("Concrete"));
    r.param(unit("variation", "Color variation", 0.5).group("Concrete"));
    r.param(unit("roughness", "Roughness", 0.9).group("Concrete"));
    r.param(unit("pores", "Pores", 0.5).group("Concrete").tip("Depth of the small pits in the surface"));
    r.param(unit("stains", "Stains", 0.3).group("Weathering").tip("Dark streaks running down the surface"));
    r.param(unit("chips", "Chipped edges", 0.5).group("Weathering").tip("Pale, broken edges (reads the baked curvature)"));
    r.param(unit("dust", "Amount", 0.4).group("Dust").tip("Dust settled on the surfaces that face up"));
    r.param(color("dust_color", "Color", [0.52, 0.49, 0.44]).group("Dust"));
    r.pattern_params();

    let base = mat(ts, "Concrete", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 8.0, 101, v(0.28, 0.28, 0.27), v(0.46, 0.45, 0.43))),
        (Roughness, proc(ProceduralKind::Grunge, 12.0, 102, s(0.82), s(0.97))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Pores, 14.0, 103, s(0.0), s(-0.05))),
    ], triplanar(2.0));
    r.color(&base, "color", "variation");
    r.roughness(&base, "roughness", "0.15");
    r.range(&base, Height, "0", "0 - pores * 0.1");
    r.follow_pattern(&base);

    let stains = masked(coat(ts, "Stains", [0.2, 0.19, 0.18], 0.9, 0.0), ts, vec![fx(pattern(ProceduralKind::Streaks, 4.0, 105, 0.68, 0.4))]);
    r.value(&stains, BaseColor, "color * 0.55");
    r.bind_fx(&stains, 0, Prop::PatternBalance { channel: None }, "remap(stains, 0.95, 0.4)");
    r.bind(&stains, Prop::Opacity { channel: None }, "saturate(stains * 3) * 0.6");
    r.follow_pattern(&stains);

    let dust = masked(coat(ts, "Dust", [0.52, 0.49, 0.44], 0.98, 0.0), ts, vec![fx(top(0.62, 0.45, 0.45)), fx_blend(pattern(ProceduralKind::Clouds, 3.0, 104, 0.4, 0.0), BlendMode::Multiply)]);
    r.value(&dust, BaseColor, "dust_color");
    r.bind_fx(&dust, 0, Prop::Level, "remap(dust, 0.95, 0.35)");
    r.bind(&dust, Prop::Opacity { channel: None }, "saturate(dust * 3) * 0.8");
    r.follow_pattern(&dust);

    let chips = masked(coat(ts, "Chipped Edges", [0.6, 0.59, 0.56], 0.9, 0.0), ts, vec![fx(wear())]);
    r.value(&chips, BaseColor, "min(color * 1.6, 1)");
    r.value(&chips, Height, "0 - 0.02");
    r.bind_fx(&chips, 0, Prop::Level, "chips");
    r.bind(&chips, Prop::Opacity { channel: None }, "saturate(chips * 20)");
    r.follow_pattern(&chips);
    r.finish(folder(&format!("{PREFIX}Concrete"), ts, vec![base, stains, dust, chips]))
}

fn leather(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.2, 0.09, 0.045]).group("Leather"));
    r.param(unit("roughness", "Roughness", 0.5).group("Leather"));
    r.param(Param::float("grain_size", "Grain size", 0.25, 4.0, 1.0).group("Leather").tip("Above 1 is a coarser grain"));
    r.param(unit("grain_depth", "Grain depth", 0.5).group("Leather"));
    r.param(unit("wear", "Edge wear", 0.45).group("Wear").tip("Edges rubbed pale (reads the baked curvature)"));
    r.param(unit("creases", "Dark creases", 0.5).group("Wear").tip("Darkening in the folds (reads the baked ambient occlusion)"));
    r.pattern_params();

    let base = mat(ts, "Leather", vec![
        (BaseColor, proc(ProceduralKind::Cells, 72.0, 51, v(0.14, 0.06, 0.03), v(0.22, 0.10, 0.05))),
        (Roughness, proc(ProceduralKind::Cells, 72.0, 51, s(0.45), s(0.6))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Cells, 72.0, 51, s(0.0), s(0.04))),
    ], triplanar(1.0));
    r.color(&base, "color", "0.35");
    r.roughness(&base, "roughness", "0.2");
    r.range(&base, Height, "0", "grain_depth * 0.05");
    r.bind(&base, Prop::PatternScale { channel: None }, "72 / grain_size");
    r.bind(&base, Prop::PatternSeed { channel: None }, "seed + 51");
    r.bind(&base, Prop::Tiling, "1 * scale");

    let tone = masked(coat(ts, "Tone Variation", [0.1, 0.045, 0.02], 0.55, 0.0), ts, vec![fx(pattern(ProceduralKind::Clouds, 2.5, 52, 0.5, 0.4))]);
    r.value(&tone, BaseColor, "color * 0.55");
    r.value(&tone, Roughness, "clamp(roughness + 0.1, 0, 1)");
    r.bind(&tone, Prop::Opacity { channel: Some(BaseColor) }, "0.5");
    r.follow_pattern(&tone);

    let creases = masked(coat(ts, "Dark Creases", [0.04, 0.02, 0.01], 0.7, 0.0), ts, vec![fx(dirt_generator())]);
    r.value(&creases, BaseColor, "color * 0.25");
    r.bind_fx(&creases, 0, Prop::Level, "remap(creases, 0.5, 0)");
    r.bind(&creases, Prop::Opacity { channel: None }, "saturate(creases * 4)");
    r.follow_pattern(&creases);

    let worn = masked(coat(ts, "Worn Edges", [0.45, 0.28, 0.17], 0.4, 0.0), ts, vec![fx(wear())]);
    r.value(&worn, BaseColor, "mix(color, rgb(0.75, 0.62, 0.48), 0.45)");
    r.value(&worn, Roughness, "clamp(roughness + 0.15, 0, 1)");
    r.bind_fx(&worn, 0, Prop::Level, "wear");
    r.bind(&worn, Prop::Opacity { channel: None }, "saturate(wear * 20) * 0.85");
    r.follow_pattern(&worn);
    r.finish(folder(&format!("{PREFIX}Leather"), ts, vec![base, tone, creases, worn]))
}

fn wood(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("light", "Light color", [0.45, 0.25, 0.11]).group("Wood"));
    r.param(color("dark", "Dark color", [0.18, 0.09, 0.04]).group("Wood"));
    r.param(unit("roughness", "Roughness", 0.55).group("Wood"));
    r.param(Param::float("ring_size", "Ring size", 0.25, 4.0, 1.0).group("Wood").tip("Above 1 is wider growth rings"));
    r.param(unit("grain_depth", "Grain depth", 0.5).group("Wood"));
    r.param(Param::toggle("painted", "Painted", false).group("Paint").tip("A coat of paint over the wood, worn off the edges"));
    r.param(color("paint_color", "Color", [0.62, 0.66, 0.6]).group("Paint"));
    r.param(unit("paint_wear", "Paint wear", 0.6).group("Paint").tip("Paint worn off the edges and flaked off in patches"));
    dirt_params(&mut r, "dirt", "Dirt", [0.06, 0.045, 0.03]);
    r.pattern_params();

    let base = mat(ts, "Wood", vec![
        (BaseColor, proc(ProceduralKind::WoodRings, 3.0, 31, v(0.18, 0.09, 0.04), v(0.45, 0.25, 0.11))),
        (Roughness, proc(ProceduralKind::WoodRings, 3.0, 31, s(0.5), s(0.6))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::WoodRings, 3.0, 31, s(0.0), s(0.05))),
    ], triplanar(1.0));
    r.range(&base, BaseColor, "dark", "light");
    r.roughness(&base, "roughness", "0.2");
    r.range(&base, Height, "0", "grain_depth * 0.1");
    r.bind(&base, Prop::PatternScale { channel: None }, "3 / ring_size");
    r.bind(&base, Prop::PatternSeed { channel: None }, "seed + 31");
    r.bind(&base, Prop::Tiling, "1 * scale");

    let fibres = masked(coat(ts, "Fibres", [0.1, 0.05, 0.02], 0.7, 0.0), ts, vec![fx(pattern(ProceduralKind::Brushed, 10.0, 32, 0.6, 0.35))]);
    r.value(&fibres, BaseColor, "dark * 0.7");
    r.bind(&fibres, Prop::Opacity { channel: None }, "0.35 + grain_depth * 0.4");
    r.follow_pattern(&fibres);

    let paint = masked(
        mat(ts, "Paint", vec![
            (BaseColor, proc(ProceduralKind::Clouds, 3.0, 33, v(0.6, 0.6, 0.6), v(0.66, 0.66, 0.66))),
            (Roughness, proc(ProceduralKind::Dirt, 5.0, 34, s(0.5), s(0.65))),
            (Metallic, uni(0.0)),
            (Height, uni(0.012)),
        ], triplanar(2.0)),
        ts,
        vec![fx(all_white()), fx_blend(wear(), BlendMode::Subtract), fx_blend(pattern(ProceduralKind::Clouds, 6.0, 35, 0.6, 0.92), BlendMode::Subtract)],
    );
    r.bind(&paint, Prop::Visible, "painted");
    r.color(&paint, "paint_color", "0.12");
    r.bind_fx(&paint, 1, Prop::Level, "paint_wear");
    r.bind_fx(&paint, 1, Prop::Opacity { channel: None }, "saturate(paint_wear * 20)");
    r.bind_fx(&paint, 2, Prop::PatternBalance { channel: None }, "remap(paint_wear, 0.75, 0.5)");
    r.bind_fx(&paint, 2, Prop::Opacity { channel: None }, "saturate(paint_wear * 5)");
    r.follow_pattern(&paint);

    let dirt = dirt_layer(&mut r, ts, "Dirt", "dirt", 0.6);
    r.follow_pattern(&dirt);
    r.finish(folder(&format!("{PREFIX}Wood"), ts, vec![base, fibres, paint, occlusion(ts), dirt]))
}

/// The built-in procedural materials, in shelf order.
pub fn smart_materials() -> Vec<SmartMaterial> {
    let named = |name: &str, build: fn(&TextureSet) -> Layer| smart(&format!("{PREFIX}{name}"), build);
    vec![
        named("Painted Metal", painted_metal),
        named("Rusted Steel", rusted_steel),
        named("Brushed Metal", brushed_metal),
        named("Polished Metal", polished_metal),
        named("Hammered Metal", hammered_metal),
        named("Aged Copper", aged_copper),
        named("Worn Plastic", worn_plastic),
        named("Rubber", rubber),
        named("Concrete", concrete),
        named("Leather", leather),
        named("Wood", wood),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_doc::{ParamKind, ParamValue};

    fn built() -> Vec<(String, Layer)> {
        let ts = TextureSet::new("set", 0, 64);
        smart_materials().into_iter().map(|m| (m.name.clone(), m.build(&ts))).collect()
    }

    #[test]
    fn every_procedural_material_is_well_formed() {
        let all = built();
        assert_eq!(all.len(), 11);
        for (name, layer) in &all {
            assert!(name.starts_with(PREFIX), "{name}");
            let m = layer.material.as_ref().unwrap_or_else(|| panic!("{name} has parameters"));
            assert_eq!(m.validate(), Vec::<String>::new(), "{name}");
            assert_eq!(m.check(layer), vec![], "{name}: every binding has a target and a valid formula");
            assert!(m.params.len() >= 6, "{name} exposes a useful set of parameters");
            assert!(m.param("scale").is_some() && m.param("seed").is_some(), "{name} has Scale and Seed");
            assert!(m.is_default(), "{name} is built at its defaults");
            // Every parameter is read by at least one binding (no dead sliders).
            for p in &m.params {
                let used = m.bindings.iter().any(|b| subcraft_doc::material_expr::params_used(&b.formula).unwrap().contains(&p.id));
                assert!(used, "{name}: parameter '{}' drives nothing", p.id);
            }
        }
    }

    #[test]
    fn building_settles_the_layers_so_applying_again_changes_nothing() {
        for (name, mut layer) in built() {
            let before = layer.clone();
            assert!(!layer.apply_material(), "{name}");
            assert_eq!(layer, before, "{name}");
        }
    }

    #[test]
    fn every_parameter_changes_the_layers_and_reset_restores_them() {
        for (name, layer) in built() {
            let ids: Vec<_> = layer.material.as_ref().unwrap().params.iter().map(|p| (p.id.clone(), p.kind)).collect();
            for (id, kind) in ids {
                let mut edited = layer.clone();
                let value = match kind {
                    subcraft_doc::ParamKind::Float { min, max, default } => ParamValue::Number(if (default - max).abs() > (default - min).abs() { max } else { min }),
                    subcraft_doc::ParamKind::Color { .. } => ParamValue::Color([0.9, 0.05, 0.6]),
                    subcraft_doc::ParamKind::Toggle { default } => ParamValue::Toggle(!default),
                    subcraft_doc::ParamKind::Seed { default } => ParamValue::Number(default as f32 + 17.0),
                };
                assert!(Arc::make_mut(edited.material.as_mut().unwrap()).set(&id, value), "{name}.{id}");
                assert!(edited.apply_material(), "{name}: '{id}' changes a layer");
                assert_eq!(edited.material.as_ref().unwrap().check(&edited), vec![], "{name}.{id}");
                Arc::make_mut(edited.material.as_mut().unwrap()).reset();
                edited.apply_material();
                assert_eq!(edited, layer, "{name}: reset after '{id}' is exactly the built material");
            }
        }
    }

    #[test]
    fn two_applications_of_a_material_do_not_share_targets() {
        let ts = TextureSet::new("set", 0, 64);
        let material = &smart_materials()[0];
        let (a, b) = (material.build(&ts), material.build(&ts));
        let (ma, mb) = (a.material.as_ref().unwrap(), b.material.as_ref().unwrap());
        assert!(ma.bindings.iter().zip(&mb.bindings).all(|(x, y)| x.layer != y.layer));
        assert_eq!(ma.check(&b).len(), ma.bindings.len(), "bindings of one application find nothing in the other");
    }

    /// Authoring: every property the panel offers can be exposed on any material's layers.
    /// Exposing changes nothing (the control starts at the value the property has), the
    /// binding applies, and moving the control changes the layers.
    #[test]
    fn every_exposable_property_can_be_exposed_and_then_drives_its_layer() {
        let mut exposed = 0;
        for (name, built) in built() {
            let mut plain = built.clone();
            plain.material = None;
            let list = subcraft_doc::material::exposable(&plain);
            assert!(list.len() > 10, "{name}: {}", list.len());
            for e in &list {
                let mut controls = MaterialControls::default();
                let id = controls.expose(e).unwrap_or_else(|| panic!("{name}: {e:?}"));
                assert!(controls.validate().is_empty() && controls.check(&plain).is_empty(), "{name}: {e:?} {:?}", controls.check(&plain));
                assert!(controls.expose(e).is_none(), "a property is exposed once");
                let mut same = plain.clone();
                assert!(!controls.apply(&mut same), "{name}: exposing {} of {} changed the layer", e.label, e.layer_name);
                let other = match e.kind {
                    ParamKind::Float { min, max, default } => ParamValue::Number(if (default - min).abs() > (max - default).abs() { min } else { max }),
                    ParamKind::Color { default } => ParamValue::Color([1.0 - default[0].min(1.0), 0.3, 0.7]),
                    ParamKind::Toggle { default } => ParamValue::Toggle(!default),
                    ParamKind::Seed { default } => ParamValue::Number(((default + 7) % 1000) as f32),
                };
                assert!(controls.set(&id, other), "{name}: {e:?}");
                let mut moved = plain.clone();
                assert!(controls.apply(&mut moved), "{name}: {} of {} did not follow its control", e.label, e.layer_name);
                assert!(controls.remove_param(&id) && controls.params.is_empty() && controls.bindings.is_empty());
                exposed += 1;
            }
        }
        assert!(exposed > 300, "{exposed}");
    }

    /// One parameter driving several properties: linking keeps every property where it is,
    /// then all of them follow the parameter. Formulas can be rewritten; bad ones are refused.
    #[test]
    fn a_parameter_can_drive_several_properties() {
        for (name, built) in built() {
            let mut plain = built.clone();
            plain.material = None;
            let list = subcraft_doc::material::exposable(&plain);
            for kind in 0..3 {
                let same: Vec<_> = list
                    .iter()
                    .filter(|e| matches!((kind, e.kind), (0, ParamKind::Float { .. }) | (1, ParamKind::Color { .. }) | (2, ParamKind::Seed { .. })))
                    .collect();
                if same.len() < 3 {
                    continue;
                }
                let mut controls = MaterialControls::default();
                let id = controls.expose(same[0]).unwrap();
                let mut linked = 0;
                for e in &same[1..] {
                    assert!(controls.can_link(&id, e), "{name}: {e:?}");
                    assert!(controls.link(&id, e) && !controls.link(&id, e), "linked once");
                    linked += 1;
                }
                assert_eq!(controls.bindings_of(&id).len(), linked + 1);
                assert!(controls.check(&plain).is_empty(), "{name}: {:?}", controls.check(&plain));
                // Nothing moved by linking (ratios are written with 4 decimals, so allow that).
                let mut same_layer = plain.clone();
                controls.apply(&mut same_layer);
                for e in subcraft_doc::material::exposable(&same_layer).iter().zip(&list) {
                    let (now, before) = (e.0.kind.default_value().as_value(), e.1.kind.default_value().as_value());
                    for c in 0..3 {
                        assert!((now[c] - before[c]).abs() <= 2e-3 * before[c].abs().max(1.0), "{name}: {} of {} moved from {before:?} to {now:?}", e.1.label, e.1.layer_name);
                    }
                }
                // A different kind of control cannot be linked.
                if let Some(other) = list.iter().find(|e| std::mem::discriminant(&e.kind) != std::mem::discriminant(&same[0].kind)) {
                    assert!(!controls.can_link(&id, other) && !controls.link(&id, other));
                }
                // Formulas: rewritten, refused with a reason, removed.
                assert_eq!(controls.set_formula(1, &format!("{id} * 0.5")), Ok(true));
                assert_eq!(controls.set_formula(1, &format!("{id} * 0.5")), Ok(false));
                assert!(controls.set_formula(1, "nobody + 1").unwrap_err().contains("nobody"));
                assert!(controls.set_formula(1, "1 +").is_err());
                assert_eq!(controls.bindings[1].formula, format!("{id} * 0.5"), "a refused formula changes nothing");
                assert!(controls.remove_binding(1) && controls.bindings_of(&id).len() == linked);
            }
        }
    }
}
