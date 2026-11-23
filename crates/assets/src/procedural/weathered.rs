//! Weathered materials: a clean procedural base that the material filters (edge damages, rust
//! weathering, peeling paint) then age, with Mask Builder dirt on top. Each has three macro
//! sliders (Wear, Age, Dirt) that drive several filter settings and masks at once; at 0 the
//! surface is new. Same rules as the parent module: built at the parameter defaults, bake first.

use super::*;
use subcraft_core::EffectId;
use subcraft_doc::{ContentEffect, ContentEffectKind, FilterSetting as S, MaskBuilder, MaterialFilter};

/// Put a material filter on `layer` (a fill or a folder: it rewrites what the layer shows).
fn filter(layer: &mut Layer, ts: &TextureSet, f: MaterialFilter) -> EffectId {
    let effect = ContentEffect::new(ContentEffectKind::Material(f), ts);
    let id = effect.id;
    layer.content_effects.push(Arc::new(effect));
    id
}

impl Recipe {
    /// One setting of a filter of `layer` follows a formula.
    fn drive(&mut self, layer: &Layer, effect: EffectId, setting: S, formula: &str) {
        self.bindings.push(Binding::effect(layer.id, effect, Prop::Filter { setting }, formula));
    }

    /// A filter's pattern follows the material's Scale and Seed.
    fn follow_filter(&mut self, layer: &Layer, effect: EffectId) {
        let Some(e) = layer.content_effects.iter().find(|e| e.id == effect) else { return };
        let ContentEffectKind::Material(f) = e.kind else { return };
        if let Some(subcraft_doc::FilterValue::Scale(scale)) = f.setting(S::Scale) {
            self.drive(layer, effect, S::Scale, &format!("{scale} * scale"));
        }
        if let Some(subcraft_doc::FilterValue::Seed(seed)) = f.setting(S::Seed) {
            self.drive(layer, effect, S::Seed, &format!("seed + {seed}"));
        }
    }

    /// The three macros every weathered material has.
    fn macros(&mut self, wear: (f32, &str), age: (f32, &str), dirt: (f32, &str)) {
        self.param(unit("wear", "Wear", wear.0).group("Weathering").tip(wear.1));
        self.param(unit("age", "Age", age.0).group("Weathering").tip(age.1));
        self.param(unit("dirt", "Dirt", dirt.0).group("Weathering").tip(dirt.1));
    }
}

/// A Mask Builder generator with these contributions, seeded.
fn builder(seed: u32, b: MaskBuilder) -> MaskEffectKind {
    let mut g = Generator::new(GeneratorKind::MaskBuilder);
    g.noise.seed = seed;
    g.builder = Some(b);
    MaskEffectKind::Generator(g)
}

/// A coat spread by a Mask Builder: `amount` raises the builder's Level from `none` to `full`
/// (0.5 is the summed contributions as they are) and fades the layer in from 0.
#[allow(clippy::too_many_arguments)]
fn built_coat(r: &mut Recipe, ts: &TextureSet, name: &str, seed: u32, b: MaskBuilder, amount: &str, none: f32, full: f32, strength: f32) -> Layer {
    let l = masked(coat(ts, name, [0.5, 0.5, 0.5], 0.5, 0.0), ts, vec![fx(builder(seed, b))]);
    r.bind_fx(&l, 0, Prop::Level, &format!("remap({amount}, {none}, {full})"));
    r.bind_fx(&l, 0, Prop::BreakupSeed, &format!("seed + {seed}"));
    r.bind(&l, Prop::Opacity { channel: None }, &format!("saturate({amount} * 4) * {strength}"));
    l
}

fn steel_under(color: [f32; 3]) -> (MaterialFilter, MaterialFilter) {
    (
        MaterialFilter::PeelingPaint { amount: 0.2, scale: 5.0, edge_bias: 0.6, thickness: 0.6, curl: 0.5, color, roughness: 0.42, metallic: 1.0, rust: 0.5, seed: 11 },
        MaterialFilter::EdgeDamages { amount: 0.4, spread: 2.2, scale: 30.0, depth: 0.6, color, roughness: 0.4, metallic: 1.0, contrast: 0.0, seed: 12 },
    )
}

// ----- materials ------------------------------------------------------------------------------

fn rusty_painted_steel(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("paint_color", "Color", [0.1, 0.22, 0.17]).group("Paint"));
    r.param(unit("paint_roughness", "Roughness", 0.45).group("Paint"));
    r.param(color("metal_color", "Steel color", [0.5, 0.5, 0.52]).group("Paint").tip("The steel that shows where the paint is gone"));
    r.macros(
        (0.5, "Paint chipped off the edges down to the steel (reads the baked curvature)"),
        (0.5, "Paint fades and flakes, and rust grows out of the creases and runs down"),
        (0.4, "Dark dirt in the creases and lower down (reads the baked ambient occlusion)"),
    );
    r.param(color("dirt_color", "Dirt color", [0.05, 0.04, 0.03]).group("Weathering"));
    r.pattern_params();

    let mut paint = mat(ts, "Paint", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 3.0, 501, v(0.09, 0.2, 0.16), v(0.11, 0.24, 0.18))),
        (Roughness, proc(ProceduralKind::Dirt, 4.0, 502, s(0.4), s(0.5))),
        (Metallic, uni(0.0)),
        (Height, uni(0.0)),
    ], triplanar(2.0));
    // Old paint chalks: paler, flatter in colour and matte.
    r.color(&paint, "mix(paint_color, paint_color * 0.6 + 0.2, age * 0.6)", "0.1 + age * 0.25");
    r.roughness(&paint, "clamp(paint_roughness + age * 0.3, 0, 1)", "0.12 + age * 0.15");
    r.follow_pattern(&paint);

    let (peel, chips) = steel_under([0.5, 0.5, 0.52]);
    let peel = filter(&mut paint, ts, peel);
    r.drive(&paint, peel, S::Amount, "age * age * 0.4");
    r.drive(&paint, peel, S::EdgeBias, "0.3 + wear * 0.7");
    r.drive(&paint, peel, S::Rust, "age * 0.85");
    r.drive(&paint, peel, S::Color, "metal_color");
    r.follow_filter(&paint, peel);
    let chips = filter(&mut paint, ts, chips);
    r.drive(&paint, chips, S::Amount, "wear * 0.65");
    r.drive(&paint, chips, S::Spread, "0.8 + wear * 1.4");
    r.drive(&paint, chips, S::Color, "metal_color");
    r.follow_filter(&paint, chips);
    let rust = filter(&mut paint, ts, MaterialFilter::RustWeathering { amount: 0.35, edges: 0.5, cavities: 0.6, drips: 0.6, scale: 5.0, relief: 0.5, roughness: 0.9, seed: 13 });
    r.drive(&paint, rust, S::Amount, "age * 0.5");
    r.drive(&paint, rust, S::Edges, "0.25 + wear * 0.6");
    r.drive(&paint, rust, S::Drips, "0.3 + age * 0.6");
    r.follow_filter(&paint, rust);

    let dirt = built_coat(&mut r, ts, "Soiling", 504, MaskBuilder { grunge: 0.5, ao: 0.9, curvature: 0.0, gradient: 0.25, gradient_invert: true, ao_noisiness: 0.4, ..MaskBuilder::default() }, "dirt", 0.05, 0.8, 0.9);
    r.value(&dirt, BaseColor, "dirt_color");
    r.value(&dirt, Roughness, "0.9");
    r.finish(folder(&format!("{PREFIX}Rusty Painted Steel"), ts, vec![paint, dirt]))
}

fn chipped_enamel(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("enamel_color", "Color", [0.78, 0.76, 0.68]).group("Enamel").tip("Glassy fired enamel: cream by default"));
    r.param(unit("gloss", "Gloss", 0.85).group("Enamel"));
    r.param(color("ground_color", "Ground coat", [0.025, 0.028, 0.04]).group("Enamel").tip("The dark first coat that rims every chip"));
    r.macros(
        (0.45, "Enamel knocked off the edges: a dark rim, then bare steel (reads the baked curvature)"),
        (0.4, "Chips on the faces, rust in the bare steel, a duller glaze and fine crazing"),
        (0.35, "Brown staining in the creases (reads the baked ambient occlusion)"),
    );
    r.param(color("dirt_color", "Dirt color", [0.09, 0.06, 0.035]).group("Weathering"));
    r.pattern_params();

    let enamel = mat(ts, "Enamel", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 2.0, 511, v(0.76, 0.74, 0.66), v(0.8, 0.78, 0.7))),
        (Roughness, proc(ProceduralKind::Dirt, 3.0, 512, s(0.1), s(0.2))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Clouds, 14.0, 513, s(0.0), s(0.006))),
    ], triplanar(2.0));
    r.color(&enamel, "enamel_color", "0.05");
    r.roughness(&enamel, "clamp(1 - gloss * 0.95 + age * 0.12, 0, 1)", "0.08 + age * 0.15");
    r.follow_pattern(&enamel);

    // Crazing: hairline cracks in the glaze that pick up dirt as it ages.
    let crazing = masked(coat(ts, "Crazing", [0.2, 0.15, 0.1], 0.6, 0.0), ts, vec![
        fx(pattern(ProceduralKind::Cracks, 22.0, 514, 0.5, 0.8)),
        fx_blend(pattern(ProceduralKind::Clouds, 2.5, 515, 0.5, 0.6), BlendMode::Multiply),
    ]);
    r.value(&crazing, BaseColor, "mix(enamel_color, dirt_color, 0.75)");
    r.bind_fx(&crazing, 0, Prop::PatternBalance { channel: None }, "remap(age, 0.97, 0.6)");
    r.bind_fx(&crazing, 1, Prop::PatternBalance { channel: None }, "remap(age, 0.8, 0.35)");
    r.bind(&crazing, Prop::Opacity { channel: None }, "saturate(age * 3) * 0.7");
    r.follow_pattern(&crazing);

    // The filters sit on the folder, so chips cut through the glaze and its crazing together.
    let mut surface = folder("Enamel Surface", ts, vec![enamel, crazing]);
    // The same edge damage twice: the ground coat reaches further than the bare steel, so every
    // chip has a dark rim.
    let rim = filter(&mut surface, ts, MaterialFilter::EdgeDamages { amount: 0.4, spread: 2.4, scale: 22.0, depth: 0.5, color: [0.025, 0.028, 0.04], roughness: 0.6, metallic: 0.0, contrast: 0.0, seed: 21 });
    r.drive(&surface, rim, S::Amount, "wear * 0.85");
    r.drive(&surface, rim, S::Spread, "0.8 + wear * 1.5");
    r.drive(&surface, rim, S::Color, "ground_color");
    r.follow_filter(&surface, rim);
    let steel = filter(&mut surface, ts, MaterialFilter::EdgeDamages { amount: 0.3, spread: 2.4, scale: 22.0, depth: 0.9, color: [0.42, 0.42, 0.44], roughness: 0.45, metallic: 1.0, contrast: 0.0, seed: 21 });
    r.drive(&surface, steel, S::Amount, "wear * 0.55");
    r.drive(&surface, steel, S::Spread, "0.8 + wear * 1.5");
    r.follow_filter(&surface, steel);
    let flakes = filter(&mut surface, ts, MaterialFilter::PeelingPaint { amount: 0.1, scale: 9.0, edge_bias: 0.8, thickness: 0.7, curl: 0.0, color: [0.3, 0.3, 0.32], roughness: 0.5, metallic: 1.0, rust: 0.5, seed: 22 });
    r.drive(&surface, flakes, S::Amount, "age * 0.3");
    r.drive(&surface, flakes, S::Rust, "saturate(0.2 + age)");
    r.follow_filter(&surface, flakes);
    let rust = filter(&mut surface, ts, MaterialFilter::RustWeathering { amount: 0.2, edges: 0.9, cavities: 0.5, drips: 0.5, scale: 7.0, relief: 0.3, roughness: 0.88, seed: 23 });
    r.drive(&surface, rust, S::Amount, "age * 0.5");
    r.drive(&surface, rust, S::Edges, "wear * 1.6");
    r.follow_filter(&surface, rust);

    let dirt = built_coat(&mut r, ts, "Staining", 516, MaskBuilder { grunge: 0.45, ao: 1.0, curvature: 0.0, ao_noisiness: 0.5, ..MaskBuilder::default() }, "dirt", 0.0, 0.75, 0.85);
    r.value(&dirt, BaseColor, "dirt_color");
    r.value(&dirt, Roughness, "0.8");
    r.finish(folder(&format!("{PREFIX}Chipped Enamel"), ts, vec![surface, dirt]))
}

fn scuffed_gun_metal(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.075, 0.08, 0.09]).group("Finish").tip("The dark finish of the metal"));
    r.param(unit("roughness", "Roughness", 0.36).group("Finish"));
    r.param(color("steel_color", "Bare steel", [0.6, 0.61, 0.63]).group("Finish").tip("What shows where the finish is rubbed through"));
    r.macros(
        (0.5, "Finish rubbed through on the edges, with scuffs and scratches around them (reads the baked curvature)"),
        (0.3, "The finish thins to grey in patches and rust starts in the creases"),
        (0.4, "Dry grime and dust packed into the creases (reads the baked ambient occlusion)"),
    );
    r.pattern_params();

    let metal = mat(ts, "Gun Metal", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 5.0, 521, v(0.07, 0.075, 0.085), v(0.085, 0.09, 0.1))),
        (Roughness, proc(ProceduralKind::Dirt, 4.0, 522, s(0.28), s(0.45))),
        (Metallic, uni(1.0)),
        (Height, uni(0.0)),
    ], triplanar(2.0));
    r.color(&metal, "color", "0.3");
    r.roughness(&metal, "roughness", "0.2 + age * 0.2");
    r.follow_pattern(&metal);

    let thin = more::pattern_coat(&mut r, ts, "Thin Finish", ProceduralKind::Clouds, 3.5, 523, 0.45, 1.0, "age", 0.95, 0.4, 0.8);
    r.value(&thin, BaseColor, "mix(color, steel_color, 0.25)");
    r.value(&thin, Roughness, "clamp(roughness + 0.12, 0, 1)");

    // Scuffs: scratches that gather around the edges and in a few grubby patches.
    let scuffs = built_coat(&mut r, ts, "Scuffs", 524, MaskBuilder { grunge: 0.25, ao: 0.0, curvature: 0.55, scratches: 0.65, scratches_amount: 0.5, scratches_scale: 1.5, curvature_boost: 0.4, curvature_noisiness: 0.5, ..MaskBuilder::default() }, "wear", 0.0, 0.42, 0.9);
    r.bind_fx(&scuffs, 0, Prop::Softness, "0.35");
    r.value(&scuffs, BaseColor, "mix(color, steel_color, 0.55)");
    r.value(&scuffs, Roughness, "clamp(roughness + 0.08, 0, 1)");
    if let LayerContent::Fill(f) = &scuffs.content
        && f.channels.contains_key(&Metallic)
    {
        r.value(&scuffs, Metallic, "1");
    }

    let mut surface = folder("Metal Surface", ts, vec![metal, thin, scuffs]);
    let bright = filter(&mut surface, ts, MaterialFilter::EdgeDamages { amount: 0.4, spread: 1.8, scale: 40.0, depth: 0.15, color: [0.6, 0.61, 0.63], roughness: 0.3, metallic: 1.0, contrast: 0.0, seed: 31 });
    r.drive(&surface, bright, S::Amount, "wear * 0.8");
    r.drive(&surface, bright, S::Spread, "0.6 + wear * 1.0");
    r.drive(&surface, bright, S::Color, "steel_color");
    r.drive(&surface, bright, S::Roughness, "clamp(roughness - 0.06, 0.05, 1)");
    r.follow_filter(&surface, bright);
    let rust = filter(&mut surface, ts, MaterialFilter::RustWeathering { amount: 0.15, edges: 0.3, cavities: 0.9, drips: 0.0, scale: 9.0, relief: 0.25, roughness: 0.85, seed: 32 });
    r.drive(&surface, rust, S::Amount, "age * 0.42");
    r.follow_filter(&surface, rust);

    let grime = built_coat(&mut r, ts, "Dry Grime", 525, MaskBuilder { grunge: 0.45, ao: 1.0, curvature: 0.5, convex_range: 0.0, concave_range: 1.0, ao_noisiness: 0.3, ..MaskBuilder::default() }, "dirt", 0.0, 0.75, 0.9);
    r.value(&grime, BaseColor, "rgb(0.1, 0.085, 0.065)");
    r.value(&grime, Roughness, "0.85");
    if ts.has_channel(Metallic) {
        r.value(&grime, Metallic, "0");
    }
    r.finish(folder(&format!("{PREFIX}Scuffed Gun Metal"), ts, vec![surface, grime]))
}

fn worn_leather_strap(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.19, 0.085, 0.04]).group("Leather"));
    r.param(unit("roughness", "Roughness", 0.5).group("Leather"));
    r.param(unit("grain_depth", "Grain depth", 0.5).group("Leather"));
    r.macros(
        (0.5, "Edges rubbed through the dye to the pale raw hide, with scuffs around them (reads the baked curvature)"),
        (0.4, "The dye dries out and fades, and cracks open"),
        (0.4, "Dark sweat and grime worked into the creases (reads the baked ambient occlusion)"),
    );
    r.pattern_params();

    let base = mat(ts, "Leather", vec![
        (BaseColor, proc(ProceduralKind::Cells, 66.0, 531, v(0.14, 0.06, 0.03), v(0.22, 0.10, 0.05))),
        (Roughness, proc(ProceduralKind::Cells, 66.0, 531, s(0.45), s(0.6))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Cells, 66.0, 531, s(0.0), s(0.04))),
    ], triplanar(1.0));
    r.color(&base, "mix(color, color * 0.7 + 0.08, age * 0.5)", "0.35");
    r.roughness(&base, "clamp(roughness + age * 0.2, 0, 1)", "0.2");
    r.range(&base, Height, "0", "grain_depth * 0.05");
    r.follow_pattern(&base);

    let tone = masked(coat(ts, "Tone Variation", [0.1, 0.045, 0.02], 0.55, 0.0), ts, vec![fx(pattern(ProceduralKind::Clouds, 2.5, 532, 0.5, 0.4))]);
    r.value(&tone, BaseColor, "color * 0.55");
    r.value(&tone, Roughness, "clamp(roughness + 0.1, 0, 1)");
    r.bind(&tone, Prop::Opacity { channel: Some(BaseColor) }, "0.5");
    r.follow_pattern(&tone);

    // Handling: the high spots are burnished darker and smoother before they wear through.
    let burnish = built_coat(&mut r, ts, "Burnished", 533, MaskBuilder { grunge: 0.35, ao: 0.0, curvature: 0.7, curvature_boost: 0.5, curvature_noisiness: 0.6, ..MaskBuilder::default() }, "wear", 0.1, 0.7, 0.85);
    r.value(&burnish, BaseColor, "color * 0.45");
    r.value(&burnish, Roughness, "clamp(roughness - 0.2, 0.1, 1)");

    let crack = |r: &mut Recipe, name: &str, contrast: f32, none: f32, full: f32| {
        let l = with_height(masked(coat(ts, name, [0.3, 0.2, 0.14], 0.8, 0.0), ts, vec![
            fx(pattern(ProceduralKind::Cracks, 9.0, 534, 0.5, contrast)),
            fx_blend(pattern(ProceduralKind::Clouds, 2.0, 535, 0.42, 0.75), BlendMode::Multiply),
        ]), ts);
        r.bind_fx(&l, 0, Prop::PatternBalance { channel: None }, &format!("remap(age, {none}, {full})"));
        r.bind_fx(&l, 1, Prop::PatternBalance { channel: None }, "remap(age, 0.8, 0.4)");
        r.bind(&l, Prop::Opacity { channel: None }, "saturate(age * 4)");
        r.follow_pattern(&l);
        l
    };
    let rims = crack(&mut r, "Crack Rims", 0.2, 0.8, 0.25);
    r.value(&rims, BaseColor, "mix(color, rgb(0.6, 0.48, 0.36), 0.45)");
    r.value(&rims, Roughness, "clamp(roughness + 0.25, 0, 1)");
    r.value(&rims, Height, "0.004");
    let cracks = crack(&mut r, "Cracks", 0.7, 0.93, 0.5);
    r.value(&cracks, BaseColor, "color * 0.3");
    r.value(&cracks, Roughness, "0.9");
    r.value(&cracks, Height, "0 - 0.03");

    let mut surface = folder("Leather Surface", ts, vec![base, tone, burnish, rims, cracks]);
    let hide = filter(&mut surface, ts, MaterialFilter::EdgeDamages { amount: 0.4, spread: 2.0, scale: 26.0, depth: 0.35, color: [0.5, 0.36, 0.24], roughness: 0.85, metallic: 0.0, contrast: 0.0, seed: 41 });
    r.drive(&surface, hide, S::Amount, "wear * 0.8");
    r.drive(&surface, hide, S::Spread, "0.7 + wear * 1.2");
    r.drive(&surface, hide, S::Color, "mix(color, rgb(0.6, 0.46, 0.33), 0.6)");
    r.follow_filter(&surface, hide);

    let grime = built_coat(&mut r, ts, "Sweat and Grime", 536, MaskBuilder { grunge: 0.5, ao: 1.0, curvature: 0.4, convex_range: 0.0, concave_range: 1.0, ao_noisiness: 0.4, ..MaskBuilder::default() }, "dirt", 0.0, 0.75, 0.9);
    r.value(&grime, BaseColor, "color * 0.18");
    r.value(&grime, Roughness, "clamp(roughness - 0.1, 0.1, 1)");
    r.finish(folder(&format!("{PREFIX}Worn Leather Strap"), ts, vec![surface, grime]))
}

fn dusty_plastic(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.06, 0.065, 0.07]).group("Plastic"));
    r.param(unit("roughness", "Roughness", 0.4).group("Plastic"));
    r.param(unit("grain", "Molded grain", 0.35).group("Plastic").tip("Fine bumps of textured plastic; 0 is smooth"));
    r.macros(
        (0.4, "Corners rubbed pale and smooth, with scuffs and scratches around them (reads the baked curvature)"),
        (0.25, "The plastic fades and yellows in the light, unevenly, and loses its sheen"),
        (0.3, "Dust on everything that faces up and in the corners"),
    );
    r.param(color("dust_color", "Dust color", [0.5, 0.48, 0.44]).group("Weathering"));
    r.pattern_params();

    let plastic = mat(ts, "Plastic", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 4.0, 541, v(0.055, 0.06, 0.065), v(0.065, 0.07, 0.075))),
        (Roughness, proc(ProceduralKind::Dirt, 4.0, 542, s(0.3), s(0.5))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Speckles, 40.0, 543, s(0.0), s(0.01))),
    ], triplanar(2.0));
    r.color(&plastic, "color", "0.06");
    r.roughness(&plastic, "clamp(roughness + age * 0.15, 0, 1)", "0.2 + age * 0.2");
    r.range(&plastic, Height, "0", "grain * 0.03");
    r.follow_pattern(&plastic);

    // Sun fade: strongest on the open faces that saw the light, weak in the corners.
    let yellow = built_coat(&mut r, ts, "Sun Fade", 544, MaskBuilder { grunge: 0.45, ao: 0.6, ao_invert: true, curvature: 0.0, ..MaskBuilder::default() }, "age", 0.0, 0.35, 0.45);
    r.value(&yellow, BaseColor, "mix(color, rgb(0.62, 0.55, 0.4), 0.4)");
    r.value(&yellow, Roughness, "clamp(roughness + 0.2, 0, 1)");

    let scuffs = built_coat(&mut r, ts, "Scuffs", 545, MaskBuilder { grunge: 0.25, ao: 0.0, curvature: 0.5, scratches: 0.7, scratches_amount: 0.5, scratches_scale: 1.4, curvature_boost: 0.4, curvature_noisiness: 0.5, ..MaskBuilder::default() }, "wear", 0.0, 0.4, 0.55);
    r.bind_fx(&scuffs, 0, Prop::Softness, "0.35");
    r.value(&scuffs, BaseColor, "mix(color, rgb(0.85, 0.85, 0.85), 0.3)");
    r.value(&scuffs, Roughness, "clamp(roughness + 0.3, 0, 1)");

    let mut surface = folder("Plastic Surface", ts, vec![plastic, yellow, scuffs]);
    let rubbed = filter(&mut surface, ts, MaterialFilter::EdgeDamages { amount: 0.3, spread: 1.6, scale: 34.0, depth: 0.1, color: [0.3, 0.3, 0.3], roughness: 0.3, metallic: 0.0, contrast: 0.0, seed: 51 });
    r.drive(&surface, rubbed, S::Amount, "wear * 0.7");
    r.drive(&surface, rubbed, S::Spread, "0.6 + wear * 1.6");
    r.drive(&surface, rubbed, S::Color, "mix(color, rgb(0.9, 0.9, 0.9), 0.22)");
    r.drive(&surface, rubbed, S::Roughness, "clamp(roughness - 0.12, 0.05, 1)");
    r.follow_filter(&surface, rubbed);

    let dust = built_coat(&mut r, ts, "Dust", 546, MaskBuilder { grunge: 0.3, ao: 0.6, curvature: 0.0, world_normal: 0.75, ao_noisiness: 0.3, ..MaskBuilder::default() }, "dirt", 0.1, 0.5, 0.85);
    // A harder threshold than the plain sum, so dust stays on what faces up.
    r.bind_fx(&dust, 0, Prop::Softness, "0.5");
    r.value(&dust, BaseColor, "dust_color");
    r.value(&dust, Roughness, "0.97");
    r.finish(folder(&format!("{PREFIX}Dusty Plastic"), ts, vec![surface, dust]))
}

/// The weathered family, in shelf order.
pub(super) fn smart_materials() -> Vec<SmartMaterial> {
    let named = |name: &str, build: fn(&TextureSet) -> Layer| smart(&format!("{PREFIX}{name}"), build);
    vec![
        named("Rusty Painted Steel", rusty_painted_steel),
        named("Chipped Enamel", chipped_enamel),
        named("Scuffed Gun Metal", scuffed_gun_metal),
        named("Worn Leather Strap", worn_leather_strap),
        named("Dusty Plastic", dusty_plastic),
    ]
}
