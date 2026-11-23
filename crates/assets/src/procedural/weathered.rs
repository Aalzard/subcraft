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
        MaterialFilter::EdgeDamages { amount: 0.4, spread: 2.2, scale: 30.0, depth: 0.6, color, roughness: 0.4, metallic: 1.0, contrast: 0.85, seed: 12 },
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
    let rim = filter(&mut surface, ts, MaterialFilter::EdgeDamages { amount: 0.4, spread: 2.4, scale: 22.0, depth: 0.5, color: [0.025, 0.028, 0.04], roughness: 0.6, metallic: 0.0, contrast: 0.95, seed: 21 });
    r.drive(&surface, rim, S::Amount, "wear * 0.85");
    r.drive(&surface, rim, S::Spread, "0.8 + wear * 1.5");
    r.drive(&surface, rim, S::Color, "ground_color");
    r.follow_filter(&surface, rim);
    let steel = filter(&mut surface, ts, MaterialFilter::EdgeDamages { amount: 0.3, spread: 2.4, scale: 22.0, depth: 0.9, color: [0.42, 0.42, 0.44], roughness: 0.45, metallic: 1.0, contrast: 0.95, seed: 21 });
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
    let bright = filter(&mut surface, ts, MaterialFilter::EdgeDamages { amount: 0.4, spread: 1.8, scale: 40.0, depth: 0.15, color: [0.6, 0.61, 0.63], roughness: 0.3, metallic: 1.0, contrast: 0.45, seed: 31 });
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
    let hide = filter(&mut surface, ts, MaterialFilter::EdgeDamages { amount: 0.4, spread: 2.0, scale: 26.0, depth: 0.35, color: [0.5, 0.36, 0.24], roughness: 0.85, metallic: 0.0, contrast: 0.35, seed: 41 });
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
    let rubbed = filter(&mut surface, ts, MaterialFilter::EdgeDamages { amount: 0.3, spread: 1.6, scale: 34.0, depth: 0.1, color: [0.3, 0.3, 0.3], roughness: 0.3, metallic: 0.0, contrast: 0.4, seed: 51 });
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

fn galvanized_steel(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(Param::float("spangle_size", "Spangle size", 0.25, 4.0, 1.0).group("Zinc").tip("Above 1 is larger zinc crystals"));
    r.param(unit("spangle", "Spangle contrast", 0.6).group("Zinc").tip("How different the zinc crystals look from each other"));
    r.param(unit("roughness", "Roughness", 0.32).group("Zinc"));
    r.macros(
        (0.35, "Zinc rubbed through to steel on the edges (reads the baked curvature)"),
        (0.4, "The zinc dulls, white rust blooms in patches, and red rust starts where the steel is open"),
        (0.4, "Grey dirt in the creases and lower down (reads the baked ambient occlusion)"),
    );
    r.pattern_params();

    let steel = mat(ts, "Steel", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 6.0, 551, v(0.5, 0.51, 0.52), v(0.58, 0.59, 0.6))),
        (Roughness, proc(ProceduralKind::Dirt, 4.0, 552, s(0.3), s(0.45))),
        (Metallic, uni(1.0)),
        (Height, uni(0.0)),
    ], triplanar(2.0));
    r.follow_pattern(&steel);

    let mut surface = folder("Zinc Surface", ts, vec![steel]);
    let zinc = filter(&mut surface, ts, MaterialFilter::Galvanized { scale: 40.0, spangle: 0.6, tint: 0.8, roughness: 0.32, roughness_variation: 0.5, relief: 0.25, seed: 61 });
    r.drive(&surface, zinc, S::Scale, "40 * scale / spangle_size");
    r.drive(&surface, zinc, S::Seed, "seed + 61");
    r.drive(&surface, zinc, S::Spangle, "spangle * (1 - age * 0.5)");
    r.drive(&surface, zinc, S::Roughness, "clamp(roughness + age * 0.3, 0, 1)");
    let rubbed = filter(&mut surface, ts, MaterialFilter::EdgeDamages { amount: 0.3, spread: 1.4, scale: 36.0, depth: 0.15, color: [0.2, 0.2, 0.22], roughness: 0.5, metallic: 1.0, contrast: 0.55, seed: 62 });
    r.drive(&surface, rubbed, S::Amount, "wear * 0.75");
    r.drive(&surface, rubbed, S::Spread, "0.6 + wear * 1.3");
    r.follow_filter(&surface, rubbed);
    let rust = filter(&mut surface, ts, MaterialFilter::RustWeathering { amount: 0.1, edges: 0.6, cavities: 0.7, drips: 0.5, scale: 8.0, relief: 0.25, roughness: 0.88, seed: 63 });
    r.drive(&surface, rust, S::Amount, "age * age * 0.5");
    r.drive(&surface, rust, S::Edges, "0.2 + wear * 1.2");
    r.follow_filter(&surface, rust);

    // White rust: the chalky zinc oxide that blooms where water sat.
    let bloom = built_coat(&mut r, ts, "White Rust", 564, MaskBuilder { grunge: 0.7, ao: 0.5, curvature: 0.0, ao_noisiness: 0.5, ..MaskBuilder::default() }, "age", 0.0, 0.42, 0.75);
    r.bind_fx(&bloom, 0, Prop::Softness, "0.5");
    r.value(&bloom, BaseColor, "rgb(0.72, 0.73, 0.72)");
    r.value(&bloom, Roughness, "0.85");

    let dirt = built_coat(&mut r, ts, "Soiling", 565, MaskBuilder { grunge: 0.45, ao: 0.9, curvature: 0.0, gradient: 0.25, gradient_invert: true, ao_noisiness: 0.4, ..MaskBuilder::default() }, "dirt", 0.0, 0.75, 0.85);
    r.value(&dirt, BaseColor, "rgb(0.07, 0.065, 0.055)");
    r.value(&dirt, Roughness, "0.9");
    r.finish(folder(&format!("{PREFIX}Galvanized Steel"), ts, vec![surface, bloom, dirt]))
}

fn powder_coated_metal(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.6, 0.12, 0.02]).group("Powder coat").tip("Signal orange by default"));
    r.param(unit("roughness", "Roughness", 0.45).group("Powder coat"));
    r.param(unit("orange_peel", "Orange peel", 0.45).group("Powder coat").tip("The fine waviness of a baked powder coat"));
    r.param(color("metal_color", "Metal color", [0.72, 0.73, 0.74]).group("Powder coat").tip("The aluminium or steel under the coat"));
    r.macros(
        (0.4, "The coat chipped off the edges in hard flakes, down to the metal (reads the baked curvature)"),
        (0.3, "The colour chalks and fades in the sun, and the coat lets go in a few places"),
        (0.35, "Dark dirt in the creases and lower down (reads the baked ambient occlusion)"),
    );
    r.pattern_params();

    let base = mat(ts, "Coat", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 3.0, 571, v(0.58, 0.11, 0.02), v(0.62, 0.13, 0.02))),
        (Roughness, uni(0.45)),
        (Metallic, uni(0.0)),
        (Height, uni(0.0)),
    ], triplanar(2.0));
    r.color(&base, "mix(color, color * 0.65 + 0.2, age * 0.55)", "0.05 + age * 0.2");
    r.follow_pattern(&base);

    let mut surface = folder("Coat Surface", ts, vec![base]);
    let coat = filter(&mut surface, ts, MaterialFilter::PowderCoated { color: [0.05, 0.05, 0.055], color_amount: 0.0, roughness: 0.45, orange_peel: 0.45, grain: 0.3, scale: 140.0, seed: 71 });
    r.drive(&surface, coat, S::Roughness, "clamp(roughness + age * 0.25, 0, 1)");
    r.drive(&surface, coat, S::OrangePeel, "orange_peel");
    r.follow_filter(&surface, coat);
    let flakes = filter(&mut surface, ts, MaterialFilter::PeelingPaint { amount: 0.05, scale: 8.0, edge_bias: 0.9, thickness: 0.8, curl: 0.2, color: [0.72, 0.73, 0.74], roughness: 0.35, metallic: 1.0, rust: 0.0, seed: 72 });
    r.drive(&surface, flakes, S::Amount, "age * age * 0.2");
    r.drive(&surface, flakes, S::Color, "metal_color");
    r.follow_filter(&surface, flakes);
    let chips = filter(&mut surface, ts, MaterialFilter::EdgeDamages { amount: 0.3, spread: 1.6, scale: 26.0, depth: 0.8, color: [0.72, 0.73, 0.74], roughness: 0.35, metallic: 1.0, contrast: 0.95, seed: 73 });
    r.drive(&surface, chips, S::Amount, "wear * 0.7");
    r.drive(&surface, chips, S::Spread, "0.7 + wear * 1.5");
    r.drive(&surface, chips, S::Color, "metal_color");
    r.follow_filter(&surface, chips);

    let dirt = built_coat(&mut r, ts, "Soiling", 574, MaskBuilder { grunge: 0.45, ao: 0.9, curvature: 0.0, gradient: 0.2, gradient_invert: true, ao_noisiness: 0.4, ..MaskBuilder::default() }, "dirt", 0.0, 0.75, 0.85);
    r.value(&dirt, BaseColor, "rgb(0.05, 0.04, 0.03)");
    r.value(&dirt, Roughness, "0.9");
    r.finish(folder(&format!("{PREFIX}Powder Coated Metal"), ts, vec![surface, dirt]))
}

fn hammered_copper(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.85, 0.4, 0.24]).group("Copper"));
    r.param(unit("roughness", "Roughness", 0.22).group("Copper"));
    r.param(Param::float("dimple_size", "Dimple size", 0.25, 4.0, 1.0).group("Copper").tip("Above 1 is larger hammer marks"));
    r.param(unit("depth", "Dimple depth", 0.55).group("Copper"));
    r.macros(
        (0.4, "Edges and high spots polished bright by handling (reads the baked curvature)"),
        (0.4, "The copper tarnishes brown, then green verdigris grows in the creases"),
        (0.3, "Dark grime in the creases (reads the baked ambient occlusion)"),
    );
    r.pattern_params();

    let copper = mat(ts, "Copper", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 5.0, 581, v(0.81, 0.38, 0.23), v(0.89, 0.42, 0.25))),
        (Roughness, proc(ProceduralKind::Dirt, 4.0, 582, s(0.15), s(0.3))),
        (Metallic, uni(1.0)),
        (Height, uni(0.0)),
    ], triplanar(2.0));
    r.color(&copper, "color", "0.08");
    r.roughness(&copper, "roughness", "0.15");
    r.follow_pattern(&copper);

    let tarnish = built_coat(&mut r, ts, "Tarnish", 583, MaskBuilder { grunge: 0.8, ao: 0.6, curvature: 0.0, ao_noisiness: 0.4, ..MaskBuilder::default() }, "age", 0.05, 0.75, 0.85);
    r.value(&tarnish, BaseColor, "color * rgb(0.32, 0.26, 0.22)");
    r.value(&tarnish, Roughness, "clamp(roughness + 0.3, 0, 1)");
    if ts.has_channel(Metallic) {
        r.value(&tarnish, Metallic, "1");
    }
    let verdigris = built_coat(&mut r, ts, "Verdigris", 584, MaskBuilder { grunge: 0.5, ao: 1.0, curvature: 0.4, convex_range: 0.0, concave_range: 1.0, ao_noisiness: 0.6, ..MaskBuilder::default() }, "saturate(age * 1.6 - 0.6)", 0.0, 0.5, 0.9);
    r.bind_fx(&verdigris, 0, Prop::Softness, "0.45");
    r.value(&verdigris, BaseColor, "rgb(0.22, 0.5, 0.42)");
    r.value(&verdigris, Roughness, "0.9");

    let mut surface = folder("Copper Surface", ts, vec![copper, tarnish, verdigris]);
    let hammer = filter(&mut surface, ts, MaterialFilter::Hammered { scale: 26.0, depth: 0.55, irregularity: 0.6, roughness_variation: 0.4, seed: 81 });
    r.drive(&surface, hammer, S::Scale, "26 * scale / dimple_size");
    r.drive(&surface, hammer, S::Seed, "seed + 81");
    r.drive(&surface, hammer, S::Depth, "depth");
    let bright = filter(&mut surface, ts, MaterialFilter::EdgeDamages { amount: 0.3, spread: 1.6, scale: 30.0, depth: 0.0, color: [0.95, 0.6, 0.45], roughness: 0.14, metallic: 1.0, contrast: 0.25, seed: 82 });
    r.drive(&surface, bright, S::Amount, "wear * 0.75");
    r.drive(&surface, bright, S::Spread, "0.7 + wear * 1.5");
    r.drive(&surface, bright, S::Color, "min(color * 1.08 + 0.04, 1)");
    r.drive(&surface, bright, S::Roughness, "clamp(roughness - 0.08, 0.04, 1)");
    r.follow_filter(&surface, bright);

    let grime = built_coat(&mut r, ts, "Dark Grime", 585, MaskBuilder { grunge: 0.4, ao: 1.0, curvature: 0.4, convex_range: 0.0, concave_range: 1.0, ao_noisiness: 0.3, ..MaskBuilder::default() }, "dirt", 0.0, 0.85, 0.9);
    r.value(&grime, BaseColor, "rgb(0.03, 0.022, 0.016)");
    r.value(&grime, Roughness, "0.75");
    r.finish(folder(&format!("{PREFIX}Hammered Copper"), ts, vec![surface, grime]))
}

fn raw_steel_plate(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.2, 0.21, 0.23]).group("Steel").tip("Dark hot-rolled steel with its mill scale"));
    r.param(unit("roughness", "Roughness", 0.5).group("Steel"));
    r.param(unit("scale_marks", "Mill scale", 0.6).group("Steel").tip("The mottled dark skin of unfinished steel"));
    r.macros(
        (0.4, "Edges ground and rubbed to bright steel (reads the baked curvature)"),
        (0.35, "Flash rust spreads from the creases and edges"),
        (0.35, "Oil and shop dirt in the creases (reads the baked ambient occlusion)"),
    );
    r.pattern_params();

    let steel = mat(ts, "Steel", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 4.0, 591, v(0.18, 0.19, 0.21), v(0.22, 0.23, 0.25))),
        (Roughness, proc(ProceduralKind::Grunge, 6.0, 592, s(0.4), s(0.6))),
        (Metallic, uni(1.0)),
        (Height, uni(0.0)),
    ], triplanar(2.0));
    r.color(&steel, "color", "0.2");
    r.roughness(&steel, "roughness", "0.25");
    r.follow_pattern(&steel);

    let mut surface = folder("Steel Surface", ts, vec![steel]);
    let raw = filter(&mut surface, ts, MaterialFilter::Raw { scale: 5.0, intensity: 0.6, seed: 91 });
    r.drive(&surface, raw, S::Intensity, "scale_marks");
    r.follow_filter(&surface, raw);
    let ground = filter(&mut surface, ts, MaterialFilter::EdgeDamages { amount: 0.3, spread: 1.8, scale: 30.0, depth: 0.3, color: [0.62, 0.63, 0.65], roughness: 0.3, metallic: 1.0, contrast: 0.6, seed: 92 });
    r.drive(&surface, ground, S::Amount, "wear * 0.8");
    r.drive(&surface, ground, S::Spread, "0.8 + wear * 1.6");
    r.follow_filter(&surface, ground);
    let rust = filter(&mut surface, ts, MaterialFilter::RustWeathering { amount: 0.2, edges: 0.5, cavities: 0.8, drips: 0.2, scale: 7.0, relief: 0.3, roughness: 0.88, seed: 93 });
    r.drive(&surface, rust, S::Amount, "age * 0.6");
    r.follow_filter(&surface, rust);

    let grime = built_coat(&mut r, ts, "Shop Dirt", 594, MaskBuilder { grunge: 0.45, ao: 1.0, curvature: 0.4, convex_range: 0.0, concave_range: 1.0, ao_noisiness: 0.3, ..MaskBuilder::default() }, "dirt", 0.0, 0.7, 0.9);
    r.value(&grime, BaseColor, "rgb(0.035, 0.03, 0.025)");
    r.value(&grime, Roughness, "0.5");
    r.finish(folder(&format!("{PREFIX}Raw Steel Plate"), ts, vec![surface, grime]))
}

// ----- beyond metals --------------------------------------------------------------------------

fn weathered_painted_wood(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("paint_color", "Color", [0.1, 0.26, 0.3]).group("Paint").tip("Old oil paint: a faded blue-green by default"));
    r.param(unit("paint_roughness", "Roughness", 0.5).group("Paint"));
    r.param(color("wood_color", "Wood color", [0.42, 0.25, 0.12]).group("Wood"));
    r.param(unit("grain_depth", "Grain depth", 0.5).group("Wood"));
    r.macros(
        (0.45, "Paint rubbed off the edges and corners down to the wood (reads the baked curvature)"),
        (0.45, "The paint chalks, cracks and flakes off in patches, and the bare wood turns silver-grey"),
        (0.35, "Dark dirt in the creases and lower down (reads the baked ambient occlusion)"),
    );
    r.param(color("dirt_color", "Dirt color", [0.05, 0.04, 0.03]).group("Weathering"));
    r.pattern_params();

    let wood = mat(ts, "Wood", vec![
        (BaseColor, proc(ProceduralKind::Brushed, 2.0, 601, v(0.2, 0.12, 0.06), v(0.42, 0.25, 0.12))),
        (Roughness, proc(ProceduralKind::Clouds, 5.0, 602, s(0.6), s(0.75))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Brushed, 2.0, 601, s(0.0), s(0.03))),
    ], triplanar(2.0));
    r.range(&wood, BaseColor, "wood_color * 0.35", "wood_color");
    r.range(&wood, Height, "0", "grain_depth * 0.06");
    r.follow_pattern(&wood);

    let fibres = masked(coat(ts, "Fibres", [0.1, 0.05, 0.02], 0.75, 0.0), ts, vec![fx(pattern(ProceduralKind::Brushed, 10.0, 603, 0.6, 0.35))]);
    r.value(&fibres, BaseColor, "wood_color * 0.3");
    r.bind(&fibres, Prop::Opacity { channel: None }, "0.35 + grain_depth * 0.4");
    r.follow_pattern(&fibres);

    // Bare wood left in the weather goes silver-grey.
    let grey = more::pattern_coat(&mut r, ts, "Silvered Wood", ProceduralKind::Clouds, 3.0, 604, 0.2, 0.0, "age", 0.9, 0.1, 0.85);
    r.value(&grey, BaseColor, "rgb(0.27, 0.27, 0.26)");
    r.value(&grey, Roughness, "0.88");

    // The paint: everything, minus the rubbed edges, minus flaked patches, minus cracks.
    let paint = masked(
        mat(ts, "Paint", vec![
            (BaseColor, proc(ProceduralKind::Clouds, 3.0, 605, v(0.09, 0.24, 0.28), v(0.11, 0.28, 0.32))),
            (Roughness, proc(ProceduralKind::Dirt, 5.0, 606, s(0.45), s(0.6))),
            (Metallic, uni(0.0)),
            (Height, uni(0.014)),
        ], triplanar(2.0)),
        ts,
        vec![
            fx(all_white()),
            fx_blend(builder(607, MaskBuilder { grunge: 0.45, ao: 0.0, curvature: 0.7, curvature_boost: 0.4, curvature_noisiness: 0.6, ..MaskBuilder::default() }), BlendMode::Subtract),
            fx_blend(pattern(ProceduralKind::Dirt, 6.0, 608, 0.6, 0.94), BlendMode::Subtract),
            fx_blend(pattern(ProceduralKind::Cracks, 7.0, 609, 0.9, 0.85), BlendMode::Subtract),
        ],
    );
    r.color(&paint, "mix(paint_color, paint_color * 0.6 + 0.25, age * 0.6)", "0.1 + age * 0.2");
    r.roughness(&paint, "clamp(paint_roughness + age * 0.3, 0, 1)", "0.15");
    r.bind_fx(&paint, 1, Prop::Level, "remap(wear, 0.05, 0.6)");
    r.bind_fx(&paint, 1, Prop::Softness, "0.3");
    r.bind_fx(&paint, 1, Prop::Opacity { channel: None }, "saturate(wear * 5)");
    r.bind_fx(&paint, 2, Prop::PatternBalance { channel: None }, "remap(age, 0.85, 0.55)");
    r.bind_fx(&paint, 2, Prop::Opacity { channel: None }, "saturate(age * 5)");
    r.bind_fx(&paint, 3, Prop::PatternBalance { channel: None }, "remap(age, 0.97, 0.6)");
    r.bind_fx(&paint, 3, Prop::Opacity { channel: None }, "saturate(age * 4)");
    r.follow_pattern(&paint);

    let mut surface = folder("Wood Surface", ts, vec![wood, fibres, grey, paint]);
    // The very edge is splintered back to fresh pale wood.
    let raw = filter(&mut surface, ts, MaterialFilter::EdgeDamages { amount: 0.3, spread: 1.2, scale: 28.0, depth: 0.5, color: [0.6, 0.42, 0.25], roughness: 0.85, metallic: 0.0, contrast: 0.6, seed: 101 });
    r.drive(&surface, raw, S::Amount, "wear * 0.45");
    r.drive(&surface, raw, S::Spread, "0.5 + wear * 0.9");
    r.drive(&surface, raw, S::Color, "min(wood_color * 1.15, 1)");
    r.follow_filter(&surface, raw);

    let dirt = built_coat(&mut r, ts, "Soiling", 610, MaskBuilder { grunge: 0.5, ao: 0.9, curvature: 0.0, gradient: 0.25, gradient_invert: true, ao_noisiness: 0.4, ..MaskBuilder::default() }, "dirt", 0.05, 0.8, 0.9);
    r.value(&dirt, BaseColor, "dirt_color");
    r.value(&dirt, Roughness, "0.9");
    r.finish(folder(&format!("{PREFIX}Weathered Painted Wood"), ts, vec![surface, dirt]))
}

fn worn_canvas(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.2, 0.22, 0.12]).group("Canvas").tip("Dyed cotton canvas: olive drab by default"));
    r.param(unit("roughness", "Roughness", 0.9).group("Canvas"));
    r.param(Param::float("weave_size", "Weave size", 0.25, 4.0, 1.0).group("Canvas").tip("Above 1 is thicker threads"));
    r.param(unit("relief", "Weave relief", 0.5).group("Canvas"));
    r.macros(
        (0.45, "Edges frayed to pale thread, with threadbare patches and loose fibres around them (reads the baked curvature)"),
        (0.4, "The dye bleaches in the sun, unevenly, and the cloth creases"),
        (0.4, "Stains on the cloth and dark grime in the folds (reads the baked ambient occlusion)"),
    );
    r.pattern_params();

    let weave = mat(ts, "Weave", vec![
        (BaseColor, proc(ProceduralKind::Weave, 7.0, 621, v(0.1, 0.1, 0.06), v(0.22, 0.24, 0.13))),
        (Roughness, proc(ProceduralKind::Clouds, 6.0, 622, s(0.85), s(0.95))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Weave, 7.0, 621, s(0.0), s(0.03))),
    ], triplanar(2.0));
    r.range(&weave, BaseColor, "color * 0.45", "color * 1.1");
    r.roughness(&weave, "roughness", "0.1");
    r.range(&weave, Height, "0", "relief * 0.06");
    r.bind(&weave, Prop::PatternScale { channel: Some(BaseColor) }, "7 / weave_size");
    r.bind(&weave, Prop::PatternScale { channel: Some(Height) }, "7 / weave_size");
    r.follow_pattern(&weave);

    let folds = mat(ts, "Creases", vec![(Height, proc(ProceduralKind::Clouds, 3.0, 623, s(0.0), s(0.1)))], triplanar(2.0));
    r.range(&folds, Height, "0", "0.04 + age * 0.12");
    r.follow_pattern(&folds);

    // Sun bleach: strongest on the open faces that saw the light.
    let bleach = built_coat(&mut r, ts, "Sun Bleach", 624, MaskBuilder { grunge: 0.55, ao: 0.6, ao_invert: true, curvature: 0.0, ..MaskBuilder::default() }, "age", 0.0, 0.5, 0.6);
    r.value(&bleach, BaseColor, "mix(color, rgb(0.62, 0.6, 0.5), 0.5)");
    r.value(&bleach, Roughness, "clamp(roughness + 0.05, 0, 1)");

    // Threadbare: the dye is rubbed out of the high spots and the pale core of the thread shows.
    let bare = built_coat(&mut r, ts, "Threadbare", 625, MaskBuilder { grunge: 0.5, ao: 0.0, curvature: 0.65, curvature_boost: 0.5, curvature_noisiness: 0.7, ..MaskBuilder::default() }, "wear", 0.0, 0.55, 0.6);
    r.bind_fx(&bare, 0, Prop::Softness, "0.8");
    r.value(&bare, BaseColor, "mix(color, rgb(0.72, 0.7, 0.62), 0.35)");
    r.value(&bare, Roughness, "1");

    let fuzz = more::pattern_coat(&mut r, ts, "Loose Fibres", ProceduralKind::Speckles, 14.0, 626, 0.3, 0.0, "wear", 0.95, 0.4, 0.5);
    r.value(&fuzz, BaseColor, "mix(color, rgb(0.8, 0.8, 0.75), 0.55)");
    r.value(&fuzz, Roughness, "1");

    let stains = more::pattern_coat(&mut r, ts, "Stains", ProceduralKind::Dirt, 3.0, 627, 0.2, 0.0, "dirt", 0.95, 0.5, 0.55);
    r.value(&stains, BaseColor, "color * 0.4");
    r.value(&stains, Roughness, "clamp(roughness - 0.15, 0, 1)");

    let mut surface = folder("Canvas Surface", ts, vec![weave, folds, bleach, bare, fuzz, stains]);
    // A thin frayed line of pale thread right on the edges.
    let fray = filter(&mut surface, ts, MaterialFilter::DetailEdgeWear { level: 0.4, detail: 0.0, contrast: 0.15, grunge: 0.8, grunge_scale: 8.0, color: [0.7, 0.68, 0.6], roughness: 1.0, metallic: 0.0, seed: 111 });
    r.drive(&surface, fray, S::Level, "wear * 0.5");
    r.drive(&surface, fray, S::Color, "mix(color, rgb(0.8, 0.78, 0.7), 0.3)");
    r.drive(&surface, fray, S::GrungeScale, "8 * scale");
    r.drive(&surface, fray, S::Seed, "seed + 111");

    let grime = built_coat(&mut r, ts, "Fold Grime", 628, MaskBuilder { grunge: 0.5, ao: 1.0, curvature: 0.4, convex_range: 0.0, concave_range: 1.0, ao_noisiness: 0.4, ..MaskBuilder::default() }, "dirt", 0.0, 0.75, 0.85);
    r.value(&grime, BaseColor, "rgb(0.05, 0.043, 0.035)");
    r.value(&grime, Roughness, "0.9");
    r.finish(folder(&format!("{PREFIX}Worn Canvas"), ts, vec![surface, grime]))
}

fn weathered_concrete(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.27, 0.265, 0.25]).group("Concrete"));
    r.param(unit("roughness", "Roughness", 0.9).group("Concrete"));
    r.param(unit("pores", "Pores", 0.5).group("Concrete").tip("Depth of the small pits in the surface"));
    r.param(unit("aggregate", "Aggregate", 0.4).group("Concrete").tip("The small stones and sand showing in the surface"));
    r.macros(
        (0.5, "Edges and corners broken off in hard chips, showing the pale stony inside (reads the baked curvature)"),
        (0.4, "Cracks open, dark water streaks run down, and moss starts in the damp creases"),
        (0.35, "Dark soot and dirt in the creases and lower down (reads the baked ambient occlusion)"),
    );
    r.param(color("moss_color", "Moss color", [0.05, 0.075, 0.025]).group("Weathering"));
    r.pattern_params();

    let base = mat(ts, "Concrete", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 8.0, 631, v(0.26, 0.26, 0.25), v(0.4, 0.4, 0.38))),
        (Roughness, proc(ProceduralKind::Grunge, 12.0, 632, s(0.82), s(0.97))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Pores, 14.0, 633, s(0.0), s(-0.05))),
    ], triplanar(2.0));
    r.color(&base, "color", "0.35");
    r.roughness(&base, "roughness", "0.15");
    r.range(&base, Height, "0", "0 - pores * 0.1");
    r.follow_pattern(&base);

    let patches = masked(coat(ts, "Patches", [0.3, 0.3, 0.29], 0.9, 0.0), ts, vec![fx(pattern(ProceduralKind::Clouds, 2.0, 634, 0.5, 0.2))]);
    r.value(&patches, BaseColor, "color * 0.72");
    r.value(&patches, Roughness, "roughness");
    r.bind(&patches, Prop::Opacity { channel: None }, "0.3 + age * 0.2");
    r.follow_pattern(&patches);

    let dark_grit = masked(coat(ts, "Dark Aggregate", [0.2, 0.2, 0.19], 0.85, 0.0), ts, vec![fx(pattern(ProceduralKind::Speckles, 9.0, 635, 0.5, 0.5))]);
    r.value(&dark_grit, BaseColor, "color * 0.5");
    r.bind(&dark_grit, Prop::Opacity { channel: None }, "aggregate * 0.8");
    r.follow_pattern(&dark_grit);
    let pale_grit = masked(coat(ts, "Pale Aggregate", [0.55, 0.54, 0.5], 0.85, 0.0), ts, vec![fx(pattern(ProceduralKind::Speckles, 6.0, 636, 0.55, 0.5))]);
    r.value(&pale_grit, BaseColor, "min(color * 1.5, 1)");
    r.bind(&pale_grit, Prop::Opacity { channel: None }, "aggregate * 0.6");
    r.follow_pattern(&pale_grit);

    // Water streaks only where a soft cloud lets them through, so they start and stop.
    let streaks = masked(coat(ts, "Water Streaks", [0.2, 0.19, 0.18], 0.85, 0.0), ts, vec![
        fx(pattern(ProceduralKind::Streaks, 7.0, 637, 0.68, 0.15)),
        fx_blend(pattern(ProceduralKind::Clouds, 3.0, 638, 0.45, 0.4), BlendMode::Multiply),
    ]);
    r.value(&streaks, BaseColor, "color * 0.5");
    r.bind_fx(&streaks, 0, Prop::PatternBalance { channel: None }, "remap(age, 0.95, 0.4)");
    r.bind(&streaks, Prop::Opacity { channel: None }, "saturate(age * 3) * 0.55");
    r.follow_pattern(&streaks);

    let cracks = with_height(masked(coat(ts, "Cracks", [0.12, 0.12, 0.11], 0.95, 0.0), ts, vec![
        fx(pattern(ProceduralKind::Cracks, 3.0, 639, 0.6, 0.85)),
        fx_blend(pattern(ProceduralKind::Clouds, 2.0, 640, 0.5, 0.7), BlendMode::Multiply),
    ]), ts);
    r.value(&cracks, BaseColor, "color * 0.25");
    r.value(&cracks, Height, "0 - 0.04");
    r.bind_fx(&cracks, 1, Prop::PatternBalance { channel: None }, "remap(age, 0.85, 0.2)");
    r.bind(&cracks, Prop::Opacity { channel: None }, "saturate(age * 5) * 0.9");
    r.follow_pattern(&cracks);

    let mut surface = folder("Concrete Surface", ts, vec![base, patches, dark_grit, pale_grit, streaks, cracks]);
    let chips = filter(&mut surface, ts, MaterialFilter::EdgeDamages { amount: 0.4, spread: 2.0, scale: 16.0, depth: 0.9, color: [0.5, 0.49, 0.46], roughness: 0.97, metallic: 0.0, contrast: 0.9, seed: 122 });
    r.drive(&surface, chips, S::Amount, "wear * 0.8");
    r.drive(&surface, chips, S::Spread, "0.8 + wear * 1.8");
    r.drive(&surface, chips, S::Color, "min(color * 1.3, 1)");
    r.follow_filter(&surface, chips);

    // Moss: only late, in the damp creases and on what faces up.
    let moss = with_height(built_coat(&mut r, ts, "Moss", 641, MaskBuilder { grunge: 0.5, ao: 0.9, curvature: 0.4, convex_range: 0.0, concave_range: 1.0, world_normal: 0.35, ao_noisiness: 0.6, ..MaskBuilder::default() }, "saturate(age * 1.6 - 0.5)", 0.0, 0.36, 0.85), ts);
    r.bind_fx(&moss, 0, Prop::Softness, "0.4");
    r.value(&moss, BaseColor, "moss_color");
    r.value(&moss, Roughness, "0.95");
    r.value(&moss, Height, "0.02");

    let dirt = built_coat(&mut r, ts, "Soot", 642, MaskBuilder { grunge: 0.5, ao: 0.9, curvature: 0.0, gradient: 0.25, gradient_invert: true, ao_noisiness: 0.4, ..MaskBuilder::default() }, "dirt", 0.0, 0.75, 0.85);
    r.value(&dirt, BaseColor, "rgb(0.045, 0.04, 0.035)");
    r.value(&dirt, Roughness, "0.92");
    r.finish(folder(&format!("{PREFIX}Weathered Concrete"), ts, vec![surface, moss, dirt]))
}

fn perished_rubber(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.03, 0.03, 0.032]).group("Rubber"));
    r.param(unit("roughness", "Roughness", 0.75).group("Rubber"));
    r.param(unit("texture", "Surface texture", 0.4).group("Rubber").tip("Fine pitted relief; 0 is smooth"));
    r.macros(
        (0.45, "Edges rubbed smooth and shiny, with grey scuffs; past half, chunks tear out of the edges (reads the baked curvature)"),
        (0.4, "The rubber dries out: it greys, a chalky bloom spreads and fine cracks open"),
        (0.35, "Pale dust on what faces up and in the creases"),
    );
    r.param(color("dust_color", "Dust color", [0.3, 0.27, 0.22]).group("Weathering"));
    r.pattern_params();

    let base = mat(ts, "Rubber", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 5.0, 651, v(0.03, 0.03, 0.03), v(0.04, 0.04, 0.04))),
        (Roughness, proc(ProceduralKind::Dirt, 5.0, 652, s(0.65), s(0.85))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Pores, 30.0, 653, s(0.0), s(-0.01))),
    ], triplanar(2.0));
    r.color(&base, "mix(color, color * 0.6 + rgb(0.05, 0.047, 0.043), age * 0.4)", "0.4");
    r.roughness(&base, "clamp(roughness + age * 0.15, 0, 1)", "0.2");
    r.range(&base, Height, "0", "0 - texture * 0.03");
    r.follow_pattern(&base);

    let bloom = built_coat(&mut r, ts, "Bloom", 654, MaskBuilder { grunge: 0.75, ao: 0.4, ao_invert: true, curvature: 0.0, ..MaskBuilder::default() }, "age", 0.0, 0.42, 0.35);
    r.value(&bloom, BaseColor, "mix(color, rgb(0.42, 0.41, 0.38), 0.4)");
    r.value(&bloom, Roughness, "0.95");

    let scuffs = built_coat(&mut r, ts, "Scuffs", 655, MaskBuilder { grunge: 0.25, ao: 0.0, curvature: 0.5, scratches: 0.45, scratches_amount: 0.4, scratches_scale: 1.4, curvature_boost: 0.4, curvature_noisiness: 0.5, ..MaskBuilder::default() }, "wear", 0.0, 0.4, 0.45);
    r.bind_fx(&scuffs, 0, Prop::Softness, "0.35");
    r.value(&scuffs, BaseColor, "mix(color, rgb(0.4, 0.4, 0.4), 0.3)");
    r.value(&scuffs, Roughness, "clamp(roughness - 0.3, 0.1, 1)");

    // Ozone cracking: fine, close cracks that open with age.
    let crack = |r: &mut Recipe, name: &str, scale: f32, seed: u32, none: f32, full: f32| {
        let l = with_height(masked(coat(ts, name, [0.01, 0.01, 0.01], 0.9, 0.0), ts, vec![
            fx(pattern(ProceduralKind::Cracks, scale, seed, 0.6, 0.8)),
            fx_blend(pattern(ProceduralKind::Clouds, 2.5, seed + 1, 0.5, 0.6), BlendMode::Multiply),
        ]), ts);
        r.value(&l, BaseColor, "color * 0.25");
        r.value(&l, Height, "0 - 0.03");
        r.bind_fx(&l, 1, Prop::PatternBalance { channel: None }, &format!("remap(age, {none}, {full})"));
        r.bind(&l, Prop::Opacity { channel: None }, "saturate(age * 4) * 0.9");
        r.follow_pattern(&l);
        l
    };
    let fine = crack(&mut r, "Fine Cracks", 16.0, 656, 0.9, 0.45);
    let deep = crack(&mut r, "Deep Cracks", 6.0, 658, 1.0, 0.55);

    let mut surface = folder("Rubber Surface", ts, vec![base, bloom, scuffs, fine, deep]);
    let polished = filter(&mut surface, ts, MaterialFilter::EdgeDamages { amount: 0.35, spread: 1.6, scale: 30.0, depth: 0.0, color: [0.06, 0.06, 0.06], roughness: 0.4, metallic: 0.0, contrast: 0.3, seed: 131 });
    r.drive(&surface, polished, S::Amount, "wear * 0.9");
    r.drive(&surface, polished, S::Spread, "0.7 + wear * 1.8");
    r.drive(&surface, polished, S::Color, "color * 2 + 0.03");
    r.drive(&surface, polished, S::Roughness, "clamp(roughness - 0.45, 0.1, 1)");
    r.follow_filter(&surface, polished);
    let torn = filter(&mut surface, ts, MaterialFilter::EdgeDamages { amount: 0.0, spread: 1.4, scale: 14.0, depth: 1.0, color: [0.02, 0.02, 0.02], roughness: 0.95, metallic: 0.0, contrast: 0.9, seed: 132 });
    r.drive(&surface, torn, S::Amount, "saturate(wear * 2 - 1) * 0.55");
    r.drive(&surface, torn, S::Color, "color * 0.6");
    r.drive(&surface, torn, S::Roughness, "roughness");
    r.follow_filter(&surface, torn);

    let dust = built_coat(&mut r, ts, "Dust", 660, MaskBuilder { grunge: 0.3, ao: 0.6, curvature: 0.0, world_normal: 0.75, ao_noisiness: 0.3, ..MaskBuilder::default() }, "dirt", 0.1, 0.42, 0.8);
    r.bind_fx(&dust, 0, Prop::Softness, "0.5");
    r.value(&dust, BaseColor, "dust_color");
    r.value(&dust, Roughness, "0.97");
    r.finish(folder(&format!("{PREFIX}Perished Rubber"), ts, vec![surface, dust]))
}

fn chipped_painted_plastic(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("paint_color", "Color", [0.7, 0.42, 0.02]).group("Paint").tip("Sprayed paint over a plastic shell: safety yellow by default"));
    r.param(unit("gloss", "Gloss", 0.6).group("Paint"));
    r.param(color("primer_color", "Primer", [0.42, 0.42, 0.4]).group("Paint").tip("The grey undercoat that rims every chip"));
    r.param(color("plastic_color", "Plastic color", [0.035, 0.035, 0.04]).group("Paint").tip("The moulded plastic under the paint"));
    r.macros(
        (0.45, "Paint chipped off the edges: a rim of primer, then bare plastic, with scuffs around them (reads the baked curvature)"),
        (0.3, "The paint fades and dulls in the sun and lets go in small flakes"),
        (0.35, "Dark grime in the creases and lower down (reads the baked ambient occlusion)"),
    );
    r.pattern_params();

    let paint = mat(ts, "Paint", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 3.0, 671, v(0.68, 0.4, 0.02), v(0.72, 0.44, 0.02))),
        (Roughness, proc(ProceduralKind::Dirt, 4.0, 672, s(0.35), s(0.5))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Clouds, 30.0, 673, s(0.0), s(0.004))),
    ], triplanar(2.0));
    r.color(&paint, "mix(paint_color, paint_color * 0.65 + 0.2, age * 0.4)", "0.05 + age * 0.15");
    r.roughness(&paint, "clamp(1 - gloss * 0.95 + age * 0.3, 0, 1)", "0.12 + age * 0.15");
    r.follow_pattern(&paint);

    let scuffs = built_coat(&mut r, ts, "Scuffs", 674, MaskBuilder { grunge: 0.25, ao: 0.0, curvature: 0.5, scratches: 0.7, scratches_amount: 0.5, scratches_scale: 1.4, curvature_boost: 0.4, curvature_noisiness: 0.5, ..MaskBuilder::default() }, "wear", 0.0, 0.4, 0.6);
    r.bind_fx(&scuffs, 0, Prop::Softness, "0.35");
    r.value(&scuffs, BaseColor, "mix(paint_color, primer_color, 0.4)");
    r.value(&scuffs, Roughness, "clamp(1 - gloss * 0.95 + 0.3, 0, 1)");

    let mut surface = folder("Paint Surface", ts, vec![paint, scuffs]);
    // The same edge damage twice: the primer reaches further than the bare plastic.
    let rim = filter(&mut surface, ts, MaterialFilter::EdgeDamages { amount: 0.4, spread: 2.0, scale: 28.0, depth: 0.3, color: [0.42, 0.42, 0.4], roughness: 0.7, metallic: 0.0, contrast: 0.9, seed: 141 });
    r.drive(&surface, rim, S::Amount, "wear * 0.8");
    r.drive(&surface, rim, S::Spread, "0.7 + wear * 1.5");
    r.drive(&surface, rim, S::Color, "primer_color");
    r.follow_filter(&surface, rim);
    let bare = filter(&mut surface, ts, MaterialFilter::EdgeDamages { amount: 0.3, spread: 2.0, scale: 28.0, depth: 0.6, color: [0.035, 0.035, 0.04], roughness: 0.45, metallic: 0.0, contrast: 0.9, seed: 141 });
    r.drive(&surface, bare, S::Amount, "wear * 0.55");
    r.drive(&surface, bare, S::Spread, "0.7 + wear * 1.5");
    r.drive(&surface, bare, S::Color, "plastic_color");
    r.follow_filter(&surface, bare);
    let flakes = filter(&mut surface, ts, MaterialFilter::PeelingPaint { amount: 0.03, scale: 9.0, edge_bias: 0.7, thickness: 0.5, curl: 0.3, color: [0.035, 0.035, 0.04], roughness: 0.45, metallic: 0.0, rust: 0.0, seed: 142 });
    r.drive(&surface, flakes, S::Amount, "age * age * 0.07");
    r.drive(&surface, flakes, S::Color, "plastic_color");
    r.follow_filter(&surface, flakes);

    let grime = built_coat(&mut r, ts, "Dark Grime", 675, MaskBuilder { grunge: 0.45, ao: 1.0, curvature: 0.0, gradient: 0.2, gradient_invert: true, ao_noisiness: 0.4, ..MaskBuilder::default() }, "dirt", 0.0, 0.75, 0.85);
    r.value(&grime, BaseColor, "rgb(0.04, 0.034, 0.028)");
    r.value(&grime, Roughness, "0.85");
    r.finish(folder(&format!("{PREFIX}Chipped Painted Plastic"), ts, vec![surface, grime]))
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
        named("Galvanized Steel", galvanized_steel),
        named("Powder Coated Metal", powder_coated_metal),
        named("Hammered Copper", hammered_copper),
        named("Raw Steel Plate", raw_steel_plate),
        named("Weathered Painted Wood", weathered_painted_wood),
        named("Worn Canvas", worn_canvas),
        named("Weathered Concrete", weathered_concrete),
        named("Perished Rubber", perished_rubber),
        named("Chipped Painted Plastic", chipped_painted_plastic),
    ]
}
