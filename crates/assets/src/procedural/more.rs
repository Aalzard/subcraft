//! More built-in procedural materials: metals, plastics, fabrics, stone and painted surfaces.
//! Same rules as the parent module: built at the parameter defaults, bake first for wear.

use super::*;
use subcraft_doc::Projection;

/// A coat on the edges (reads the baked curvature): `amount` is its parameter.
fn edge_coat(r: &mut Recipe, ts: &TextureSet, name: &str, color: &str, roughness: &str, metal: f32, amount: &str) -> Layer {
    let l = masked(coat(ts, name, [0.5, 0.5, 0.5], 0.5, metal), ts, vec![fx(wear())]);
    r.value(&l, BaseColor, color);
    r.value(&l, Roughness, roughness);
    r.bind_fx(&l, 0, Prop::Level, amount);
    r.bind(&l, Prop::Opacity { channel: None }, &format!("saturate({amount} * 20)"));
    r.follow_pattern(&l);
    l
}

/// A coat spread by a pattern: its balance follows `amount` from `none` (nothing) to `full`.
#[allow(clippy::too_many_arguments)]
fn pattern_coat(r: &mut Recipe, ts: &TextureSet, name: &str, kind: ProceduralKind, scale: f32, seed: u32, contrast: f32, metal: f32, amount: &str, none: f32, full: f32, strength: f32) -> Layer {
    let l = masked(coat(ts, name, [0.5, 0.5, 0.5], 0.5, metal), ts, vec![fx(pattern(kind, scale, seed, 0.5, contrast))]);
    r.bind_fx(&l, 0, Prop::PatternBalance { channel: None }, &format!("remap({amount}, {none}, {full})"));
    r.bind(&l, Prop::Opacity { channel: None }, &format!("saturate({amount} * 4) * {strength}"));
    r.follow_pattern(&l);
    l
}

/// Dust on the faces that look up.
fn dust_layer(r: &mut Recipe, ts: &TextureSet, strength: f32) -> Layer {
    let dust = masked(coat(ts, "Dust", [0.5, 0.48, 0.44], 0.98, 0.0), ts, vec![fx(top(0.62, 0.45, 0.45)), fx_blend(pattern(ProceduralKind::Clouds, 3.0, 904, 0.4, 0.0), BlendMode::Multiply)]);
    r.value(&dust, BaseColor, "dust_color");
    r.bind_fx(&dust, 0, Prop::Level, "remap(dust, 0.95, 0.35)");
    r.bind(&dust, Prop::Opacity { channel: None }, &format!("saturate(dust * 3) * {strength}"));
    r.follow_pattern(&dust);
    dust
}

fn dust_params(r: &mut Recipe, default: f32) {
    r.param(unit("dust", "Amount", default).group("Dust").tip("Dust settled on the surfaces that face up"));
    r.param(color("dust_color", "Color", [0.5, 0.48, 0.44]).group("Dust"));
}

fn turned(scale: f32, degrees: f32) -> Projection {
    Projection { rotation_deg: degrees, ..triplanar(scale) }
}

// ----- metals ---------------------------------------------------------------------------------

fn cast_iron(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.06, 0.06, 0.065]).group("Iron"));
    r.param(unit("roughness", "Roughness", 0.62).group("Iron"));
    r.param(unit("casting", "Casting texture", 0.5).group("Iron").tip("The sandy, pitted skin a mould leaves"));
    r.param(unit("rust", "Rust", 0.35).group("Wear").tip("Rust blooming in patches"));
    r.param(color("rust_color", "Rust color", [0.17, 0.052, 0.018]).group("Wear"));
    r.param(unit("polish", "Polished edges", 0.3).group("Wear").tip("Edges rubbed to bare bright iron (reads the baked curvature)"));
    dirt_params(&mut r, "soot", "Soot", [0.012, 0.011, 0.01]);
    r.pattern_params();

    let iron = mat(ts, "Cast Iron", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 12.0, 301, v(0.05, 0.05, 0.05), v(0.07, 0.07, 0.07))),
        (Roughness, proc(ProceduralKind::Grunge, 10.0, 302, s(0.5), s(0.75))),
        (Metallic, uni(1.0)),
        (Height, proc(ProceduralKind::Clouds, 48.0, 303, s(0.0), s(0.03))),
    ], triplanar(2.0));
    r.color(&iron, "color", "0.4");
    r.roughness(&iron, "roughness", "0.25");
    r.range(&iron, Height, "0", "casting * 0.06");
    r.follow_pattern(&iron);

    let pits = with_height(masked(coat(ts, "Casting Pits", [0.02, 0.02, 0.02], 0.9, 0.6), ts, vec![fx(pattern(ProceduralKind::Pores, 9.0, 304, 0.5, 0.5))]), ts);
    r.value(&pits, BaseColor, "color * 0.4");
    r.value(&pits, Height, "0 - casting * 0.05");
    r.bind(&pits, Prop::Opacity { channel: None }, "saturate(casting * 3)");
    r.follow_pattern(&pits);

    let rust = pattern_coat(&mut r, ts, "Rust", ProceduralKind::Grunge, 5.0, 305, 0.7, 0.0, "rust", 1.2, 0.4, 1.0);
    r.value(&rust, BaseColor, "rust_color");
    r.value(&rust, Roughness, "0.92");

    let polish = edge_coat(&mut r, ts, "Polished Edges", "min(color * 4 + 0.1, 1)", "clamp(roughness - 0.3, 0.05, 1)", 1.0, "polish");
    let soot = dirt_layer(&mut r, ts, "Soot", "soot", 0.9);
    r.follow_pattern(&soot);
    r.finish(folder(&format!("{PREFIX}Cast Iron"), ts, vec![iron, pits, rust, polish, soot]))
}

fn anodized_aluminium(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.55, 0.06, 0.07]).group("Anodizing").tip("The dye of the anodized skin"));
    r.param(unit("roughness", "Roughness", 0.32).group("Anodizing"));
    r.param(unit("satin", "Satin finish", 0.4).group("Anodizing").tip("The fine brushed lines under the colour"));
    r.param(unit("wear", "Edge wear", 0.4).group("Wear").tip("Colour rubbed off the edges down to bare aluminium (reads the baked curvature)"));
    r.param(unit("scratches", "Scratches", 0.3).group("Wear").tip("Scratches through the colour"));
    r.param(unit("smudges", "Fingerprints", 0.4).group("Wear"));
    r.pattern_params();

    let skin = mat(ts, "Anodized Skin", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 3.0, 311, v(0.5, 0.05, 0.06), v(0.6, 0.07, 0.08))),
        (Roughness, proc(ProceduralKind::Brushed, 6.0, 312, s(0.25), s(0.4))),
        (Metallic, uni(1.0)),
        (Height, uni(0.0)),
    ], triplanar(2.0));
    r.color(&skin, "color", "0.08");
    r.roughness(&skin, "roughness", "satin * 0.4");
    r.follow_pattern(&skin);

    let smudges = pattern_coat(&mut r, ts, "Fingerprints", ProceduralKind::Dirt, 4.0, 313, 0.2, 1.0, "smudges", 0.95, 0.45, 1.0);
    r.value(&smudges, BaseColor, "color * 0.85");
    r.value(&smudges, Roughness, "clamp(roughness + 0.22, 0, 1)");

    let scratches = pattern_coat(&mut r, ts, "Scratches", ProceduralKind::Scratches, 12.0, 314, 0.6, 1.0, "scratches", 0.95, 0.55, 0.7);
    r.value(&scratches, BaseColor, "rgb(0.82, 0.83, 0.85)");
    r.value(&scratches, Roughness, "0.4");

    let bare = edge_coat(&mut r, ts, "Bare Aluminium", "rgb(0.82, 0.83, 0.85)", "0.35", 1.0, "wear");
    r.finish(folder(&format!("{PREFIX}Anodized Aluminium"), ts, vec![skin, smudges, scratches, bare]))
}

fn blued_steel(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.025, 0.03, 0.045]).group("Bluing").tip("The blue-black oxide of gun steel"));
    r.param(unit("roughness", "Roughness", 0.3).group("Bluing"));
    r.param(unit("variation", "Oil marks", 0.4).group("Bluing").tip("Uneven sheen from oil and handling"));
    r.param(unit("wear", "Edge wear", 0.5).group("Wear").tip("Bluing rubbed through to bright steel on the edges (reads the baked curvature)"));
    r.param(unit("thinning", "Thin patches", 0.3).group("Wear").tip("Greyer patches where the bluing is wearing thin"));
    r.param(unit("scratches", "Scratches", 0.25).group("Wear"));
    dirt_params(&mut r, "grime", "Grime", [0.03, 0.025, 0.02]);
    r.pattern_params();

    let steel = mat(ts, "Blued Steel", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 5.0, 321, v(0.02, 0.025, 0.04), v(0.03, 0.035, 0.05))),
        (Roughness, proc(ProceduralKind::Dirt, 4.0, 322, s(0.2), s(0.45))),
        (Metallic, uni(1.0)),
    ], triplanar(2.0));
    r.color(&steel, "color", "0.3");
    r.roughness(&steel, "roughness", "variation * 0.5");
    r.follow_pattern(&steel);

    let thin = pattern_coat(&mut r, ts, "Thin Bluing", ProceduralKind::Clouds, 4.0, 323, 0.5, 1.0, "thinning", 0.95, 0.45, 1.0);
    r.value(&thin, BaseColor, "mix(color, rgb(0.3, 0.3, 0.32), 0.55)");
    r.value(&thin, Roughness, "clamp(roughness + 0.1, 0, 1)");

    let scratches = pattern_coat(&mut r, ts, "Scratches", ProceduralKind::Scratches, 12.0, 324, 0.6, 1.0, "scratches", 0.95, 0.55, 0.6);
    r.value(&scratches, BaseColor, "rgb(0.6, 0.61, 0.63)");
    r.value(&scratches, Roughness, "0.38");

    let bright = edge_coat(&mut r, ts, "Bright Edges", "rgb(0.62, 0.63, 0.65)", "clamp(roughness + 0.05, 0, 1)", 1.0, "wear");
    let grime = dirt_layer(&mut r, ts, "Grime", "grime", 0.8);
    r.follow_pattern(&grime);
    r.finish(folder(&format!("{PREFIX}Blued Steel"), ts, vec![steel, thin, scratches, bright, occlusion(ts), grime]))
}

fn aged_bronze(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.55, 0.33, 0.14]).group("Bronze"));
    r.param(unit("roughness", "Roughness", 0.35).group("Bronze"));
    dirt_params(&mut r, "patina", "Dark patina", [0.05, 0.035, 0.02]);
    r.param(unit("verdigris", "Verdigris", 0.3).group("Verdigris").tip("Green crust, starting in the recesses"));
    r.param(color("verdigris_color", "Color", [0.16, 0.42, 0.33]).group("Verdigris"));
    r.param(unit("polish", "Polished edges", 0.5).group("Wear").tip("Edges and high spots rubbed bright (reads the baked curvature)"));
    r.pattern_params();

    let bronze = mat(ts, "Bronze", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 6.0, 331, v(0.45, 0.27, 0.1), v(0.6, 0.37, 0.16))),
        (Roughness, proc(ProceduralKind::Grunge, 7.0, 332, s(0.25), s(0.45))),
        (Metallic, uni(1.0)),
    ], triplanar(2.0));
    r.color(&bronze, "color", "0.3");
    r.roughness(&bronze, "roughness", "0.25");
    r.follow_pattern(&bronze);

    let tone = pattern_coat(&mut r, ts, "Brown Tone", ProceduralKind::Clouds, 3.0, 333, 0.3, 1.0, "patina", 0.9, 0.3, 1.0);
    r.value(&tone, BaseColor, "color * 0.45");
    r.value(&tone, Roughness, "clamp(roughness + 0.2, 0, 1)");

    let patina = dirt_layer(&mut r, ts, "Dark Patina", "patina", 0.9);
    r.value(&patina, Metallic, "0.3");
    r.follow_pattern(&patina);

    let verdigris = masked(
        mat(ts, "Verdigris", vec![
            (BaseColor, proc(ProceduralKind::Clouds, 16.0, 334, v(0.1, 0.3, 0.25), v(0.25, 0.5, 0.42))),
            (Roughness, uni(0.9)),
            (Metallic, uni(0.0)),
            (Height, proc(ProceduralKind::Clouds, 24.0, 335, s(0.0), s(0.03))),
        ], triplanar(2.0)),
        ts,
        vec![fx(dirt_generator()), fx_blend(pattern(ProceduralKind::Grunge, 6.0, 336, 0.5, 0.6), BlendMode::Multiply)],
    );
    r.color(&verdigris, "verdigris_color", "0.6");
    r.bind_fx(&verdigris, 1, Prop::PatternBalance { channel: None }, "remap(verdigris, 0.85, 0.2)");
    r.bind(&verdigris, Prop::Opacity { channel: None }, "saturate(verdigris * 5)");
    r.follow_pattern(&verdigris);

    let polish = edge_coat(&mut r, ts, "Polished Edges", "min(color * 1.45, 1)", "clamp(roughness - 0.18, 0.04, 1)", 1.0, "polish");
    r.finish(folder(&format!("{PREFIX}Aged Bronze"), ts, vec![bronze, tone, patina, verdigris, polish]))
}

// ----- plastics -------------------------------------------------------------------------------

fn glossy_plastic(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.02, 0.02, 0.022]).group("Plastic"));
    r.param(unit("roughness", "Roughness", 0.08).group("Plastic"));
    r.param(unit("smudges", "Fingerprints", 0.45).group("Wear").tip("Greasy marks that dull the gloss"));
    r.param(unit("scratches", "Hairline scratches", 0.35).group("Wear"));
    r.param(unit("edge_wear", "Edge wear", 0.2).group("Wear").tip("Edges rubbed matte (reads the baked curvature)"));
    dust_params(&mut r, 0.15);
    r.pattern_params();

    let plastic = mat(ts, "Glossy Plastic", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 3.0, 341, v(0.02, 0.02, 0.02), v(0.022, 0.022, 0.022))),
        (Roughness, proc(ProceduralKind::Clouds, 5.0, 342, s(0.06), s(0.1))),
        (Metallic, uni(0.0)),
    ], triplanar(2.0));
    r.color(&plastic, "color", "0.06");
    r.roughness(&plastic, "roughness", "0.04");
    r.follow_pattern(&plastic);

    let smudges = pattern_coat(&mut r, ts, "Fingerprints", ProceduralKind::Dirt, 5.0, 343, 0.15, 0.0, "smudges", 0.95, 0.42, 1.0);
    r.value(&smudges, BaseColor, "mix(color, rgb(0.3, 0.3, 0.3), 0.12)");
    r.value(&smudges, Roughness, "clamp(roughness + 0.3, 0, 1)");

    let scratches = pattern_coat(&mut r, ts, "Hairline Scratches", ProceduralKind::Scratches, 8.0, 344, 0.55, 0.0, "scratches", 0.8, 0.3, 1.0);
    r.value(&scratches, BaseColor, "mix(color, rgb(0.6, 0.6, 0.6), 0.3)");
    r.value(&scratches, Roughness, "clamp(roughness + 0.4, 0, 1)");

    let edges = edge_coat(&mut r, ts, "Worn Edges", "mix(color, rgb(0.5, 0.5, 0.5), 0.25)", "clamp(roughness + 0.35, 0, 1)", 0.0, "edge_wear");
    let dust = dust_layer(&mut r, ts, 0.4);
    r.finish(folder(&format!("{PREFIX}Glossy Plastic"), ts, vec![plastic, smudges, scratches, edges, dust]))
}

fn carbon_fibre(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Fibre color", [0.035, 0.035, 0.04]).group("Carbon"));
    r.param(unit("contrast", "Weave contrast", 0.6).group("Carbon").tip("How strongly the crossing bundles differ in brightness"));
    r.param(Param::float("weave_size", "Weave size", 0.25, 4.0, 1.0).group("Carbon").tip("Above 1 is a larger weave"));
    r.param(unit("relief", "Weave relief", 0.2).group("Carbon").tip("0 is a flat, resin-filled surface"));
    r.param(unit("roughness", "Clear coat roughness", 0.12).group("Clear coat"));
    r.param(unit("scratches", "Scratches", 0.25).group("Wear"));
    r.param(unit("edge_wear", "Edge wear", 0.25).group("Wear").tip("Clear coat rubbed dull on the edges (reads the baked curvature)"));
    dust_params(&mut r, 0.1);
    r.pattern_params();

    let weave = mat(ts, "Carbon Weave", vec![
        (BaseColor, proc(ProceduralKind::Weave, 4.0, 351, v(0.01, 0.01, 0.012), v(0.06, 0.06, 0.07))),
        (Roughness, proc(ProceduralKind::Clouds, 4.0, 352, s(0.1), s(0.16))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Weave, 4.0, 351, s(0.0), s(0.01))),
    ], turned(2.0, 45.0));
    r.range(&weave, BaseColor, "color * (1 - contrast * 0.8)", "color * (1 + contrast * 1.6)");
    r.roughness(&weave, "roughness", "0.06");
    r.range(&weave, Height, "0", "relief * 0.05");
    r.bind(&weave, Prop::PatternScale { channel: Some(BaseColor) }, "4 / weave_size");
    r.bind(&weave, Prop::PatternScale { channel: Some(Height) }, "4 / weave_size");
    r.follow_pattern(&weave);

    let scratches = pattern_coat(&mut r, ts, "Scratches", ProceduralKind::Scratches, 7.0, 353, 0.55, 0.0, "scratches", 0.8, 0.3, 1.0);
    r.value(&scratches, BaseColor, "mix(color, rgb(0.5, 0.5, 0.5), 0.3)");
    r.value(&scratches, Roughness, "clamp(roughness + 0.4, 0, 1)");

    let edges = edge_coat(&mut r, ts, "Dull Edges", "mix(color, rgb(0.4, 0.4, 0.4), 0.2)", "clamp(roughness + 0.4, 0, 1)", 0.0, "edge_wear");
    let dust = dust_layer(&mut r, ts, 0.6);
    r.finish(folder(&format!("{PREFIX}Carbon Fibre"), ts, vec![weave, scratches, edges, dust]))
}

// ----- fabrics --------------------------------------------------------------------------------

/// A woven cloth: thread colours, weave relief, fuzz, fading, stains.
#[allow(clippy::too_many_arguments)]
fn cloth(ts: &TextureSet, name: &str, kind: ProceduralKind, base_seed: u32, thread: [f32; 3], cross: f32, rough: f32, weave_scale: f32, fade_default: f32, fade_tip: &str) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", thread).group("Cloth"));
    r.param(unit("cross", "Cross thread", cross).group("Cloth").tip("How much the pale crossing threads show between the coloured ones: 0 is one flat colour, 1 is the white specks of denim"));
    r.param(unit("roughness", "Roughness", rough).group("Cloth"));
    r.param(Param::float("weave_size", "Weave size", 0.25, 4.0, 1.0).group("Cloth").tip("Above 1 is thicker threads"));
    r.param(unit("relief", "Weave relief", 0.5).group("Cloth"));
    r.param(unit("wrinkles", "Wrinkles", 0.4).group("Cloth").tip("Soft folds and creases in the cloth"));
    r.param(unit("fuzz", "Fuzz", 0.35).group("Cloth").tip("Loose pale fibres on the surface"));
    // Sheen needs the texture set's sheen channels (applying the material from the shelf adds them).
    let sheen = ts.has_channel(SheenColor) && ts.has_channel(SheenOpacity);
    if sheen {
        r.param(unit("sheen", "Sheen", 0.4).group("Cloth").tip("The soft glow of the fibres when you look along the cloth"));
    }
    r.param(unit("fading", "Fading", fade_default).group("Wear").tip(fade_tip));
    r.param(unit("stains", "Stains", 0.25).group("Wear"));
    dirt_params(&mut r, "dirt", "Dirt", [0.06, 0.05, 0.04]);
    r.pattern_params();

    let weave = mat(ts, "Weave", vec![
        (BaseColor, proc(kind, weave_scale, base_seed, v(0.1, 0.1, 0.1), v(0.2, 0.2, 0.2))),
        (Roughness, proc(ProceduralKind::Clouds, 6.0, base_seed + 1, s(0.8), s(0.95))),
        (Metallic, uni(0.0)),
        (Height, proc(kind, weave_scale, base_seed, s(0.0), s(0.03))),
        (SheenColor, FillSource::uniform(v(0.6, 0.6, 0.6))),
        (SheenOpacity, uni(0.4)),
        (SheenRoughness, uni(0.5)),
    ], triplanar(2.0));
    if sheen {
        r.value(&weave, SheenColor, "mix(color, rgb(1, 1, 1), 0.35)");
        r.value(&weave, SheenOpacity, "sheen");
    }
    // The gaps between threads are the dark end; the tops of the threads carry the colour,
    // paler where the cross threads show.
    r.range(&weave, BaseColor, "color * 0.45", "color * 1.1");
    r.roughness(&weave, "roughness", "0.12");
    r.range(&weave, Height, "0", "relief * 0.06");
    r.bind(&weave, Prop::PatternScale { channel: Some(BaseColor) }, &format!("{weave_scale} / weave_size"));
    r.bind(&weave, Prop::PatternScale { channel: Some(Height) }, &format!("{weave_scale} / weave_size"));
    r.follow_pattern(&weave);

    // The crossing threads show in the low parts of the weave (the white dots of denim).
    let cross_mask = MaskEffectKind::Fill {
        value: 1.0,
        procedural: Some(subcraft_doc::ProceduralParams { scale: weave_scale, seed: base_seed, balance: 0.42, contrast: 0.6, invert: true, ..subcraft_doc::ProceduralParams::new(kind) }),
        projection: triplanar(2.0),
    };
    let cross = masked(coat(ts, "Cross Threads", [0.8, 0.8, 0.78], 0.9, 0.0), ts, vec![fx(cross_mask)]);
    r.value(&cross, BaseColor, "mix(color, rgb(0.85, 0.85, 0.82), 0.85)");
    r.value(&cross, Roughness, "roughness");
    r.bind(&cross, Prop::Opacity { channel: None }, "cross");
    r.bind_fx(&cross, 0, Prop::PatternScale { channel: None }, &format!("{weave_scale} / weave_size"));
    r.bind_fx(&cross, 0, Prop::PatternSeed { channel: None }, &format!("seed + {base_seed}"));

    // Folds: height only, large and soft.
    let wrinkles = mat(ts, "Wrinkles", vec![(Height, proc(ProceduralKind::Clouds, 3.0, base_seed + 6, s(0.0), s(0.1)))], triplanar(2.0));
    r.range(&wrinkles, Height, "0", "wrinkles * 0.3");
    r.follow_pattern(&wrinkles);

    let tone = masked(coat(ts, "Tone Variation", [0.1, 0.1, 0.1], 0.9, 0.0), ts, vec![fx(pattern(ProceduralKind::Clouds, 2.5, base_seed + 2, 0.5, 0.3))]);
    r.value(&tone, BaseColor, "color * 0.75");
    r.value(&tone, Roughness, "roughness");
    r.bind(&tone, Prop::Opacity { channel: None }, "0.12");
    r.follow_pattern(&tone);

    let fuzz = pattern_coat(&mut r, ts, "Fuzz", ProceduralKind::Speckles, 14.0, base_seed + 3, 0.3, 0.0, "fuzz", 0.9, 0.35, 0.5);
    r.value(&fuzz, BaseColor, "mix(color, rgb(0.8, 0.8, 0.78), 0.5)");
    r.value(&fuzz, Roughness, "1");

    let faded = masked(coat(ts, "Faded Edges", [0.5, 0.5, 0.5], 0.95, 0.0), ts, vec![fx(wear()), fx_blend(pattern(ProceduralKind::Clouds, 4.0, base_seed + 4, 0.35, 0.2), BlendMode::LinearDodge)]);
    r.value(&faded, BaseColor, "mix(color, rgb(0.8, 0.8, 0.78), 0.6)");
    r.value(&faded, Roughness, "clamp(roughness + 0.05, 0, 1)");
    r.bind_fx(&faded, 0, Prop::Level, "fading");
    r.bind_fx(&faded, 1, Prop::PatternBalance { channel: None }, "remap(fading, 1.3, 0.55)");
    r.bind(&faded, Prop::Opacity { channel: None }, "saturate(fading * 6) * 0.55");
    r.follow_pattern(&faded);

    let stains = pattern_coat(&mut r, ts, "Stains", ProceduralKind::Dirt, 3.0, base_seed + 5, 0.2, 0.0, "stains", 0.95, 0.45, 0.6);
    r.value(&stains, BaseColor, "color * 0.5");
    r.value(&stains, Roughness, "clamp(roughness - 0.1, 0, 1)");

    let dirt = dirt_layer(&mut r, ts, "Dirt", "dirt", 0.7);
    r.follow_pattern(&dirt);
    r.finish(folder(&format!("{PREFIX}{name}"), ts, vec![weave, cross, wrinkles, tone, fuzz, faded, stains, occlusion(ts), dirt]))
}

fn canvas(ts: &TextureSet) -> Layer {
    cloth(ts, "Canvas", ProceduralKind::Weave, 361, [0.45, 0.38, 0.27], 0.15, 0.88, 7.0, 0.25, "Sun-bleached, rubbed areas on edges and in patches")
}

fn denim(ts: &TextureSet) -> Layer {
    cloth(ts, "Denim", ProceduralKind::Twill, 371, [0.03, 0.06, 0.2], 0.6, 0.92, 18.0, 0.2, "The pale wear of denim on edges, seams and in patches")
}

// ----- stone ----------------------------------------------------------------------------------

fn granite(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.42, 0.4, 0.39]).group("Granite"));
    r.param(color("dark_color", "Dark grains", [0.03, 0.03, 0.035]).group("Granite"));
    r.param(color("light_color", "Light grains", [0.8, 0.76, 0.72]).group("Granite"));
    r.param(unit("grains", "Grain amount", 0.55).group("Granite"));
    r.param(Param::float("grain_size", "Grain size", 0.25, 4.0, 1.0).group("Granite").tip("Above 1 is coarser crystals"));
    r.param(unit("roughness", "Roughness", 0.22).group("Finish").tip("Low is polished stone, high is a rough-cut face"));
    r.param(unit("pitting", "Pitting", 0.25).group("Finish"));
    dirt_params(&mut r, "dirt", "Dirt", [0.05, 0.045, 0.04]);
    dust_params(&mut r, 0.2);
    r.pattern_params();

    let base = mat(ts, "Granite", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 14.0, 381, v(0.35, 0.33, 0.32), v(0.5, 0.48, 0.46))),
        (Roughness, proc(ProceduralKind::Clouds, 20.0, 382, s(0.18), s(0.28))),
        (Metallic, uni(0.0)),
    ], triplanar(2.0));
    r.color(&base, "color", "0.35");
    r.roughness(&base, "roughness", "0.1");
    r.follow_pattern(&base);

    let grain = |r: &mut Recipe, name: &str, kind: ProceduralKind, scale: f32, seed: u32, colour: &str, none: f32, full: f32| {
        let l = masked(coat(ts, name, [0.5, 0.5, 0.5], 0.2, 0.0), ts, vec![fx(pattern(kind, scale, seed, 0.5, 0.75))]);
        r.value(&l, BaseColor, colour);
        r.value(&l, Roughness, "roughness");
        r.bind_fx(&l, 0, Prop::PatternBalance { channel: None }, &format!("remap(grains, {none}, {full})"));
        r.bind_fx(&l, 0, Prop::PatternScale { channel: None }, &format!("{scale} * scale / grain_size"));
        r.bind_fx(&l, 0, Prop::PatternSeed { channel: None }, &format!("seed + {seed}"));
        r.bind(&l, Prop::Opacity { channel: None }, "saturate(grains * 4)");
        l
    };
    let feldspar = grain(&mut r, "Light Grains", ProceduralKind::Clouds, 34.0, 383, "light_color", 0.95, 0.56);
    let quartz = grain(&mut r, "Grey Grains", ProceduralKind::Clouds, 40.0, 384, "color * 0.55", 0.85, 0.5);
    let mica = grain(&mut r, "Dark Grains", ProceduralKind::Clouds, 55.0, 385, "dark_color", 0.9, 0.58);

    let pits = with_height(pattern_coat(&mut r, ts, "Pits", ProceduralKind::Pores, 16.0, 386, 0.5, 0.0, "pitting", 0.9, 0.4, 1.0), ts);
    r.value(&pits, BaseColor, "color * 0.4");
    r.value(&pits, Roughness, "0.9");
    r.value(&pits, Height, "0 - 0.03");

    let dirt = dirt_layer(&mut r, ts, "Dirt", "dirt", 0.8);
    r.follow_pattern(&dirt);
    let dust = dust_layer(&mut r, ts, 0.7);
    r.finish(folder(&format!("{PREFIX}Granite"), ts, vec![base, feldspar, quartz, mica, pits, occlusion(ts), dirt, dust]))
}

fn marble(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.86, 0.85, 0.82]).group("Marble"));
    r.param(color("vein_color", "Vein color", [0.2, 0.2, 0.22]).group("Veins"));
    r.param(unit("veins", "Veins", 0.5).group("Veins").tip("How strong and how wide the veins are"));
    r.param(Param::float("vein_size", "Vein size", 0.25, 4.0, 1.0).group("Veins").tip("Above 1 is a larger, sparser net of veins"));
    r.param(unit("clouding", "Clouding", 0.5).group("Marble").tip("Soft grey clouds in the stone"));
    r.param(unit("roughness", "Roughness", 0.12).group("Finish").tip("Low is polished, high is honed"));
    r.param(unit("etching", "Dull marks", 0.3).group("Finish").tip("Dull patches where the polish is worn or etched"));
    dirt_params(&mut r, "dirt", "Dirt", [0.08, 0.07, 0.055]);
    r.pattern_params();

    let base = mat(ts, "Marble", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 3.0, 391, v(0.75, 0.74, 0.72), v(0.9, 0.89, 0.86))),
        (Roughness, proc(ProceduralKind::Dirt, 3.0, 392, s(0.08), s(0.2))),
        (Metallic, uni(0.0)),
    ], triplanar(2.0));
    r.range(&base, BaseColor, "color * (1 - clouding * 0.3)", "min(color * 1.04, 1)");
    r.roughness(&base, "roughness", "etching * 0.5");
    r.follow_pattern(&base);

    // A soft halo around every vein, then the vein itself.
    let vein = |r: &mut Recipe, name: &str, contrast: f32, strength: f32, none: f32, full: f32| {
        let l = masked(coat(ts, name, [0.2, 0.2, 0.2], 0.12, 0.0), ts, vec![fx(pattern(ProceduralKind::Veins, 2.0, 393, 0.5, contrast))]);
        r.value(&l, BaseColor, "vein_color");
        r.value(&l, Roughness, "roughness");
        r.bind_fx(&l, 0, Prop::PatternBalance { channel: None }, &format!("remap(veins, {none}, {full})"));
        r.bind_fx(&l, 0, Prop::PatternScale { channel: None }, "2 * scale / vein_size");
        r.bind_fx(&l, 0, Prop::PatternSeed { channel: None }, "seed + 393");
        r.bind(&l, Prop::Opacity { channel: None }, &format!("saturate(veins * 4) * {strength}"));
        l
    };
    let wide = vein(&mut r, "Vein Halo", 0.0, 0.35, 0.5, 0.12);
    let thin = vein(&mut r, "Veins", 0.5, 0.9, 0.75, 0.3);

    let dirt = dirt_layer(&mut r, ts, "Dirt", "dirt", 0.5);
    r.follow_pattern(&dirt);
    r.finish(folder(&format!("{PREFIX}Marble"), ts, vec![base, wide, thin, occlusion(ts), dirt]))
}

fn sandstone(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.55, 0.4, 0.24]).group("Sandstone"));
    r.param(unit("layers", "Layering", 0.5).group("Sandstone").tip("The bands of sediment the stone was laid down in"));
    r.param(Param::float("layer_angle", "Layer angle", -90.0, 90.0, 0.0).group("Sandstone"));
    r.param(unit("grain", "Sand grain", 0.5).group("Sandstone"));
    r.param(unit("roughness", "Roughness", 0.85).group("Sandstone"));
    r.param(unit("erosion", "Eroded edges", 0.45).group("Wear").tip("Edges crumbled pale and rough (reads the baked curvature)"));
    r.param(unit("holes", "Weathered holes", 0.3).group("Wear"));
    dirt_params(&mut r, "dirt", "Dirt", [0.07, 0.055, 0.035]);
    dust_params(&mut r, 0.3);
    r.pattern_params();

    let base = mat(ts, "Sandstone", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 5.0, 401, v(0.45, 0.32, 0.18), v(0.62, 0.46, 0.28))),
        (Roughness, proc(ProceduralKind::Clouds, 12.0, 402, s(0.78), s(0.92))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Clouds, 60.0, 403, s(0.0), s(0.02))),
    ], triplanar(2.0));
    r.color(&base, "color", "0.3");
    r.roughness(&base, "roughness", "0.14");
    r.range(&base, Height, "0", "grain * 0.04");
    r.follow_pattern(&base);

    let bands = mat(ts, "Sediment Bands", vec![
        (BaseColor, proc(ProceduralKind::Brushed, 2.0, 404, v(0.3, 0.2, 0.1), v(0.7, 0.55, 0.36))),
        (Height, proc(ProceduralKind::Brushed, 2.0, 404, s(0.0), s(0.03))),
    ], turned(1.0, 0.0));
    r.range(&bands, BaseColor, "color * 0.6", "min(color * 1.35, 1)");
    r.range(&bands, Height, "0", "layers * 0.06");
    r.bind(&bands, Prop::Opacity { channel: Some(BaseColor) }, "layers * 0.7");
    r.bind(&bands, Prop::Rotation, "layer_angle");
    r.follow_pattern(&bands);

    let holes = with_height(pattern_coat(&mut r, ts, "Weathered Holes", ProceduralKind::Pores, 7.0, 405, 0.4, 0.0, "holes", 0.95, 0.4, 1.0), ts);
    r.value(&holes, BaseColor, "color * 0.45");
    r.value(&holes, Roughness, "0.95");
    r.value(&holes, Height, "0 - 0.06");

    let eroded = with_height(edge_coat(&mut r, ts, "Eroded Edges", "min(color * 1.35, 1)", "0.95", 0.0, "erosion"), ts);
    r.value(&eroded, Height, "0 - 0.02");
    let dirt = dirt_layer(&mut r, ts, "Dirt", "dirt", 0.7);
    r.follow_pattern(&dirt);
    let dust = dust_layer(&mut r, ts, 0.7);
    r.finish(folder(&format!("{PREFIX}Sandstone"), ts, vec![base, bands, holes, eroded, occlusion(ts), dirt, dust]))
}

fn rough_rock(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.2, 0.19, 0.18]).group("Rock"));
    r.param(unit("roughness", "Roughness", 0.82).group("Rock"));
    r.param(Param::float("facet_size", "Facet size", 0.25, 4.0, 1.0).group("Rock").tip("Above 1 is larger broken faces"));
    r.param(unit("relief", "Relief", 0.6).group("Rock"));
    r.param(unit("cracks", "Cracks", 0.4).group("Rock"));
    r.param(unit("edges", "Pale edges", 0.4).group("Wear").tip("Edges chipped pale (reads the baked curvature)"));
    r.param(unit("moss", "Amount", 0.35).group("Moss").tip("Moss on the faces that look up"));
    r.param(color("moss_color", "Color", [0.07, 0.12, 0.03]).group("Moss"));
    dirt_params(&mut r, "dirt", "Dirt", [0.04, 0.035, 0.03]);
    r.pattern_params();

    let base = mat(ts, "Rock", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 6.0, 411, v(0.15, 0.14, 0.13), v(0.26, 0.25, 0.24))),
        (Roughness, proc(ProceduralKind::Clouds, 10.0, 412, s(0.72), s(0.92))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Cells, 5.0, 413, s(0.0), s(0.2))),
    ], triplanar(2.0));
    r.color(&base, "color", "0.5");
    r.roughness(&base, "roughness", "0.2");
    r.range(&base, Height, "0", "relief * 0.35");
    r.bind(&base, Prop::PatternScale { channel: Some(Height) }, "5 / facet_size");
    r.follow_pattern(&base);

    let grit = mat(ts, "Grit", vec![(Height, proc(ProceduralKind::Clouds, 40.0, 414, s(0.0), s(0.03)))], triplanar(2.0));
    r.range(&grit, Height, "0", "relief * 0.06");
    r.follow_pattern(&grit);

    let cracks = with_height(pattern_coat(&mut r, ts, "Cracks", ProceduralKind::Cracks, 4.0, 415, 0.5, 0.0, "cracks", 0.85, 0.35, 1.0), ts);
    r.value(&cracks, BaseColor, "color * 0.25");
    r.value(&cracks, Roughness, "0.95");
    r.value(&cracks, Height, "0 - 0.08");

    let pale = edge_coat(&mut r, ts, "Pale Edges", "min(color * 1.9, 1)", "clamp(roughness + 0.05, 0, 1)", 0.0, "edges");

    let moss = masked(
        mat(ts, "Moss", vec![
            (BaseColor, proc(ProceduralKind::Clouds, 18.0, 416, v(0.04, 0.08, 0.02), v(0.1, 0.16, 0.04))),
            (Roughness, uni(0.95)),
            (Metallic, uni(0.0)),
            (Height, proc(ProceduralKind::Clouds, 30.0, 417, s(0.0), s(0.05))),
        ], triplanar(2.0)),
        ts,
        vec![fx(top(0.6, 0.35, 0.5)), fx_blend(pattern(ProceduralKind::Grunge, 4.0, 418, 0.5, 0.5), BlendMode::Multiply)],
    );
    r.color(&moss, "moss_color", "0.7");
    r.bind_fx(&moss, 0, Prop::Level, "remap(moss, 0.95, 0.3)");
    r.bind_fx(&moss, 1, Prop::PatternBalance { channel: None }, "remap(moss, 0.8, 0.25)");
    r.bind(&moss, Prop::Opacity { channel: None }, "saturate(moss * 5)");
    r.follow_pattern(&moss);

    let dirt = dirt_layer(&mut r, ts, "Dirt", "dirt", 0.8);
    r.follow_pattern(&dirt);
    r.finish(folder(&format!("{PREFIX}Rough Rock"), ts, vec![base, grit, cracks, pale, occlusion(ts), dirt, moss]))
}

// ----- painted surfaces -----------------------------------------------------------------------

fn painted_plaster(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("paint_color", "Color", [0.3, 0.5, 0.46]).group("Paint"));
    r.param(unit("paint_roughness", "Roughness", 0.7).group("Paint"));
    r.param(unit("roller", "Roller texture", 0.4).group("Paint").tip("The fine orange-peel a paint roller leaves"));
    r.param(color("plaster_color", "Color", [0.62, 0.57, 0.5]).group("Plaster"));
    r.param(unit("peeling", "Peeling", 0.35).group("Damage").tip("Paint flaked off in patches, showing the plaster"));
    r.param(unit("cracks", "Cracks", 0.4).group("Damage"));
    r.param(unit("chipped", "Chipped edges", 0.35).group("Damage").tip("Corners knocked back to the plaster (reads the baked curvature)"));
    r.param(unit("streaks", "Water streaks", 0.3).group("Dirt").tip("Dirty runs down the wall"));
    dirt_params(&mut r, "dirt", "Dirt", [0.07, 0.06, 0.045]);
    r.pattern_params();

    let plaster = mat(ts, "Plaster", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 6.0, 421, v(0.62, 0.6, 0.56), v(0.78, 0.76, 0.72))),
        (Roughness, proc(ProceduralKind::Clouds, 14.0, 422, s(0.85), s(0.95))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Clouds, 40.0, 423, s(0.0), s(0.015))),
    ], triplanar(2.0));
    r.color(&plaster, "plaster_color", "0.2");
    r.follow_pattern(&plaster);

    let paint = masked(
        mat(ts, "Paint", vec![
            (BaseColor, proc(ProceduralKind::Clouds, 3.0, 424, v(0.6, 0.64, 0.58), v(0.64, 0.68, 0.62))),
            (Roughness, proc(ProceduralKind::Dirt, 4.0, 425, s(0.62), s(0.78))),
            (Metallic, uni(0.0)),
            (Height, proc(ProceduralKind::Clouds, 70.0, 426, s(0.03), s(0.04))),
        ], triplanar(2.0)),
        ts,
        vec![
            fx(all_white()),
            fx_blend(wear(), BlendMode::Subtract),
            fx_blend(pattern(ProceduralKind::Clouds, 5.0, 427, 0.66, 0.92), BlendMode::Subtract),
            fx_blend(pattern(ProceduralKind::Cracks, 4.0, 428, 0.5, 0.6), BlendMode::Subtract),
        ],
    );
    r.color(&paint, "paint_color", "0.08");
    r.roughness(&paint, "paint_roughness", "0.16");
    r.range(&paint, Height, "0.03", "0.03 + roller * 0.02");
    r.bind_fx(&paint, 1, Prop::Level, "chipped");
    r.bind_fx(&paint, 1, Prop::Opacity { channel: None }, "saturate(chipped * 20)");
    r.bind_fx(&paint, 2, Prop::PatternBalance { channel: None }, "remap(peeling, 0.8, 0.5)");
    r.bind_fx(&paint, 2, Prop::Opacity { channel: None }, "saturate(peeling * 5)");
    r.bind_fx(&paint, 3, Prop::PatternBalance { channel: None }, "remap(cracks, 0.9, 0.35)");
    r.bind_fx(&paint, 3, Prop::Opacity { channel: None }, "saturate(cracks * 5)");
    r.follow_pattern(&paint);

    let streaks = pattern_coat(&mut r, ts, "Water Streaks", ProceduralKind::Streaks, 3.0, 429, 0.1, 0.0, "streaks", 0.95, 0.4, 0.45);
    r.value(&streaks, BaseColor, "dirt_color * 1.5");
    r.value(&streaks, Roughness, "0.8");

    let dirt = dirt_layer(&mut r, ts, "Dirt", "dirt", 0.7);
    r.follow_pattern(&dirt);
    r.finish(folder(&format!("{PREFIX}Painted Plaster"), ts, vec![plaster, paint, streaks, occlusion(ts), dirt]))
}

fn enamel_paint(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("paint_color", "Color", [0.75, 0.52, 0.03]).group("Paint").tip("Machine yellow by default"));
    r.param(unit("gloss", "Gloss", 0.7).group("Paint"));
    r.param(unit("orange_peel", "Orange peel", 0.3).group("Paint").tip("The slight waviness of sprayed enamel"));
    r.param(color("primer_color", "Primer color", [0.3, 0.09, 0.06]).group("Under the paint").tip("Shows as a rim around chips"));
    r.param(color("metal_color", "Metal color", [0.5, 0.5, 0.52]).group("Under the paint"));
    r.param(unit("wear", "Edge wear", 0.45).group("Wear").tip("Paint worn off the edges (reads the baked curvature)"));
    r.param(unit("chips", "Chips", 0.3).group("Wear"));
    r.param(unit("scratches", "Scratches", 0.3).group("Wear"));
    r.param(unit("rust", "Rust in chips", 0.4).group("Wear").tip("Rust where the metal is bare"));
    dirt_params(&mut r, "dirt", "Dirt", [0.05, 0.04, 0.035]);
    dust_params(&mut r, 0.2);
    r.pattern_params();

    let metal = mat(ts, "Bare Metal", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 9.0, 431, v(0.42, 0.42, 0.43), v(0.58, 0.58, 0.59))),
        (Roughness, proc(ProceduralKind::Grunge, 8.0, 432, s(0.35), s(0.6))),
        (Metallic, uni(1.0)),
    ], triplanar(2.0));
    r.color(&metal, "metal_color", "0.3");
    r.follow_pattern(&metal);

    let rust = pattern_coat(&mut r, ts, "Rust", ProceduralKind::Grunge, 9.0, 433, 0.6, 0.0, "rust", 1.0, 0.3, 1.0);
    r.value(&rust, BaseColor, "rgb(0.17, 0.052, 0.018)");
    r.value(&rust, Roughness, "0.9");

    // The same wear twice: the primer reaches a little further than the paint, so every chip
    // and worn edge gets a rim of primer.
    let wear_mask = |primer: bool| {
        let (level_chip, level_scratch) = if primer { (0.69, 0.53) } else { (0.66, 0.5) };
        vec![
            fx(all_white()),
            fx_blend(wear(), BlendMode::Subtract),
            fx_blend(pattern(ProceduralKind::Clouds, 7.0, 434, level_chip, 0.92), BlendMode::Subtract),
            fx_blend(pattern(ProceduralKind::Scratches, 5.0, 435, level_scratch, 0.7), BlendMode::Subtract),
        ]
    };
    let bind_wear = |r: &mut Recipe, l: &Layer, shrink: f32| {
        r.bind_fx(l, 1, Prop::Level, &format!("wear * {}", 1.0 - shrink));
        r.bind_fx(l, 1, Prop::Opacity { channel: None }, "saturate(wear * 20)");
        r.bind_fx(l, 2, Prop::PatternBalance { channel: None }, &format!("remap(chips, 0.75, 0.5) + {}", shrink * 0.12));
        r.bind_fx(l, 2, Prop::Opacity { channel: None }, "saturate(chips * 5)");
        r.bind_fx(l, 3, Prop::PatternBalance { channel: None }, &format!("remap(scratches, 0.8, 0.4) + {}", shrink * 0.12));
        r.bind_fx(l, 3, Prop::Opacity { channel: None }, "saturate(scratches * 5)");
        r.follow_pattern(l);
    };
    let primer = with_height(masked(coat(ts, "Primer", [0.3, 0.09, 0.06], 0.75, 0.0), ts, wear_mask(true)), ts);
    r.value(&primer, BaseColor, "primer_color");
    r.value(&primer, Height, "0.015");
    bind_wear(&mut r, &primer, 0.25);

    let paint = masked(
        mat(ts, "Enamel", vec![
            (BaseColor, proc(ProceduralKind::Clouds, 3.0, 436, v(0.7, 0.5, 0.03), v(0.78, 0.54, 0.03))),
            (Roughness, proc(ProceduralKind::Dirt, 4.0, 437, s(0.2), s(0.35))),
            (Metallic, uni(0.0)),
            (Height, proc(ProceduralKind::Clouds, 30.0, 438, s(0.03), s(0.035))),
        ], triplanar(2.0)),
        ts,
        wear_mask(false),
    );
    r.color(&paint, "paint_color", "0.08");
    r.roughness(&paint, "1 - gloss * 0.95", "0.12");
    r.range(&paint, Height, "0.03", "0.03 + orange_peel * 0.015");
    bind_wear(&mut r, &paint, 0.0);

    let dirt = dirt_layer(&mut r, ts, "Dirt", "dirt", 0.9);
    r.follow_pattern(&dirt);
    let dust = dust_layer(&mut r, ts, 0.6);
    r.finish(folder(&format!("{PREFIX}Enamel Paint"), ts, vec![metal, rust, primer, paint, occlusion(ts), dirt, dust]))
}

// ----- leather and wood -------------------------------------------------------------------------

fn cracked_leather(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("color", "Color", [0.11, 0.042, 0.02]).group("Leather"));
    r.param(unit("roughness", "Roughness", 0.55).group("Leather"));
    r.param(unit("grain_depth", "Grain depth", 0.5).group("Leather"));
    r.param(unit("cracks", "Cracks", 0.5).group("Age").tip("Dry cracks through the finish, paler underneath"));
    r.param(Param::float("crack_size", "Crack size", 0.25, 4.0, 1.0).group("Age").tip("Above 1 is a larger, sparser net of cracks"));
    r.param(unit("scuffs", "Scuffs", 0.25).group("Age").tip("Pale rubbed scratches"));
    r.param(unit("wear", "Edge wear", 0.3).group("Age").tip("Edges rubbed down to the pale raw hide (reads the baked curvature)"));
    r.param(unit("folds", "Folds", 0.4).group("Age").tip("Soft dents and folds of used leather"));
    dirt_params(&mut r, "creases", "Dark creases", [0.025, 0.014, 0.008]);
    r.pattern_params();

    let base = mat(ts, "Leather", vec![
        (BaseColor, proc(ProceduralKind::Cells, 60.0, 441, v(0.1, 0.05, 0.03), v(0.15, 0.08, 0.05))),
        (Roughness, proc(ProceduralKind::Cells, 60.0, 441, s(0.48), s(0.62))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Cells, 60.0, 441, s(0.0), s(0.04))),
    ], triplanar(1.0));
    r.color(&base, "color", "0.35");
    r.roughness(&base, "roughness", "0.2");
    r.range(&base, Height, "0", "grain_depth * 0.05");
    r.follow_pattern(&base);

    let folds = mat(ts, "Folds", vec![(Height, proc(ProceduralKind::Clouds, 3.0, 442, s(0.0), s(0.1)))], triplanar(2.0));
    r.range(&folds, Height, "0", "folds * 0.3");
    r.follow_pattern(&folds);

    let tone = masked(coat(ts, "Tone Variation", [0.06, 0.03, 0.02], 0.6, 0.0), ts, vec![fx(pattern(ProceduralKind::Clouds, 2.5, 443, 0.5, 0.4))]);
    r.value(&tone, BaseColor, "color * 0.55");
    r.value(&tone, Roughness, "clamp(roughness + 0.1, 0, 1)");
    r.bind(&tone, Prop::Opacity { channel: Some(BaseColor) }, "0.5");
    r.follow_pattern(&tone);

    // Cracks: a pale rim (the finish lifting), then the dark crack inside it. Each set only
    // shows inside its own soft patches, and a finer set lives in other patches, so the cracks
    // gather in places instead of covering the hide as one even net.
    let crack = |r: &mut Recipe, name: &str, contrast: f32, none: f32, full: f32, scale: f32, seed: u32| {
        let l = with_height(masked(coat(ts, name, [0.3, 0.2, 0.14], 0.8, 0.0), ts, vec![
            fx(pattern(ProceduralKind::Cracks, scale, seed, 0.5, contrast)),
            fx_blend(pattern(ProceduralKind::Clouds, 2.0, seed + 1, 0.42, 0.75), BlendMode::Multiply),
        ]), ts);
        r.bind_fx(&l, 0, Prop::PatternBalance { channel: None }, &format!("remap(cracks, {none}, {full})"));
        r.bind_fx(&l, 0, Prop::PatternScale { channel: None }, &format!("{scale} * scale / crack_size"));
        r.bind_fx(&l, 0, Prop::PatternSeed { channel: None }, &format!("seed + {seed}"));
        r.bind_fx(&l, 1, Prop::PatternBalance { channel: None }, "remap(cracks, 0.75, 0.4)");
        r.bind_fx(&l, 1, Prop::PatternSeed { channel: None }, &format!("seed + {}", seed + 1));
        r.bind(&l, Prop::Opacity { channel: None }, "saturate(cracks * 5)");
        l
    };
    let rim = crack(&mut r, "Crack Rims", 0.2, 0.75, 0.2, 7.0, 444);
    r.value(&rim, BaseColor, "mix(color, rgb(0.6, 0.48, 0.36), 0.5)");
    r.value(&rim, Roughness, "clamp(roughness + 0.25, 0, 1)");
    r.value(&rim, Height, "0.004");
    let cracks = crack(&mut r, "Cracks", 0.7, 0.9, 0.45, 7.0, 444);
    r.value(&cracks, BaseColor, "color * 0.3");
    r.value(&cracks, Roughness, "0.9");
    r.value(&cracks, Height, "0 - 0.03");
    let fine = crack(&mut r, "Fine Cracks", 0.75, 0.95, 0.55, 19.0, 448);
    r.value(&fine, BaseColor, "color * 0.4");
    r.value(&fine, Roughness, "0.85");
    r.value(&fine, Height, "0 - 0.012");

    let scuffs = pattern_coat(&mut r, ts, "Scuffs", ProceduralKind::Scratches, 5.0, 446, 0.4, 0.0, "scuffs", 0.9, 0.5, 0.6);
    r.value(&scuffs, BaseColor, "mix(color, rgb(0.5, 0.36, 0.25), 0.4)");
    r.value(&scuffs, Roughness, "clamp(roughness + 0.25, 0, 1)");

    let creases = dirt_layer(&mut r, ts, "Dark Creases", "creases", 0.85);
    r.follow_pattern(&creases);

    let worn = masked(coat(ts, "Worn Edges", [0.45, 0.33, 0.22], 0.75, 0.0), ts, vec![fx(wear()), fx_blend(pattern(ProceduralKind::Clouds, 5.0, 447, 0.45, 0.3), BlendMode::Multiply)]);
    r.value(&worn, BaseColor, "mix(color, rgb(0.5, 0.36, 0.24), 0.5)");
    r.value(&worn, Roughness, "clamp(roughness + 0.2, 0, 1)");
    r.bind_fx(&worn, 0, Prop::Level, "wear");
    r.bind(&worn, Prop::Opacity { channel: None }, "saturate(wear * 20) * 0.9");
    r.follow_pattern(&worn);
    r.finish(folder(&format!("{PREFIX}Cracked Leather"), ts, vec![base, folds, tone, rim, cracks, fine, scuffs, creases, worn]))
}

fn wood_planks(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("light", "Light boards", [0.42, 0.26, 0.13]).group("Wood"));
    r.param(color("dark", "Dark boards", [0.2, 0.11, 0.05]).group("Wood"));
    r.param(unit("roughness", "Roughness", 0.65).group("Wood"));
    r.param(unit("grain", "Grain", 0.6).group("Wood").tip("The dark lines running along every board"));
    r.param(unit("knots", "Knots", 0.3).group("Wood"));
    r.param(Param::float("plank_size", "Board size", 0.25, 4.0, 1.0).group("Boards").tip("Above 1 is wider, longer boards"));
    r.param(unit("gaps", "Gap depth", 0.6).group("Boards").tip("How deep the gaps between boards are, and how much boards differ in height"));
    r.param(unit("weathering", "Weathering", 0.15).group("Age").tip("Sun and rain turning the wood silver-grey"));
    r.param(unit("wear", "Worn edges", 0.35).group("Age").tip("Edges rubbed pale (reads the baked curvature)"));
    dirt_params(&mut r, "dirt", "Dirt", [0.045, 0.035, 0.025]);
    r.pattern_params();

    // Boards: the gap is the dark end of the ramp, each board sits somewhere towards the light.
    let boards = mat(ts, "Boards", vec![
        (BaseColor, proc(ProceduralKind::Planks, 1.0, 451, v(0.03, 0.02, 0.01), v(0.42, 0.26, 0.13))),
        (Roughness, proc(ProceduralKind::Clouds, 6.0, 452, s(0.55), s(0.75))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Planks, 1.0, 451, s(0.0), s(0.1))),
    ], triplanar(4.0));
    r.range(&boards, BaseColor, "dark * 0.12", "light");
    r.roughness(&boards, "roughness", "0.2");
    r.range(&boards, Height, "0", "gaps * 0.2");
    r.bind(&boards, Prop::Tiling, "4 * scale / plank_size");
    r.bind(&boards, Prop::PatternSeed { channel: None }, "seed + 451");

    // The dark boards' own colour, in soft patches, so not every board is a mix with the gap.
    let tone = pattern_coat(&mut r, ts, "Dark Patches", ProceduralKind::Clouds, 1.5, 453, 0.3, 0.0, "grain", 0.75, 0.45, 0.45);
    r.value(&tone, BaseColor, "dark");
    r.value(&tone, Roughness, "roughness");

    let grain = masked(coat(ts, "Grain", [0.1, 0.05, 0.02], 0.7, 0.0), ts, vec![fx(pattern(ProceduralKind::Brushed, 6.0, 454, 0.58, 0.45))]);
    let grain = with_height(grain, ts);
    r.value(&grain, BaseColor, "dark * 0.55");
    r.value(&grain, Roughness, "clamp(roughness + 0.1, 0, 1)");
    r.value(&grain, Height, "0 - grain * 0.015");
    r.bind(&grain, Prop::Opacity { channel: None }, "grain * 0.75");
    r.follow_pattern(&grain);

    let knots = pattern_coat(&mut r, ts, "Knots", ProceduralKind::Knots, 3.0, 455, 0.6, 0.0, "knots", 0.9, 0.3, 0.9);
    r.value(&knots, BaseColor, "dark * 0.4");
    r.value(&knots, Roughness, "clamp(roughness - 0.1, 0, 1)");

    let grey = pattern_coat(&mut r, ts, "Weathered Grey", ProceduralKind::Clouds, 3.0, 456, 0.2, 0.0, "weathering", 0.85, 0.2, 0.75);
    r.value(&grey, BaseColor, "rgb(0.33, 0.31, 0.28)");
    r.value(&grey, Roughness, "0.85");

    let worn = edge_coat(&mut r, ts, "Worn Edges", "min(light * 1.5, 1)", "clamp(roughness + 0.1, 0, 1)", 0.0, "wear");
    let dirt = dirt_layer(&mut r, ts, "Dirt", "dirt", 0.8);
    r.follow_pattern(&dirt);
    r.finish(folder(&format!("{PREFIX}Wood Planks"), ts, vec![boards, tone, grain, knots, grey, worn, occlusion(ts), dirt]))
}

fn varnished_wood(ts: &TextureSet) -> Layer {
    let mut r = Recipe::default();
    r.param(color("light", "Light color", [0.36, 0.16, 0.06]).group("Wood").tip("Mahogany by default; oak and walnut are two colours away"));
    r.param(color("dark", "Dark color", [0.12, 0.045, 0.018]).group("Wood"));
    r.param(unit("grain", "Grain", 0.6).group("Wood").tip("The fine straight lines of the grain"));
    r.param(unit("figure", "Figure", 0.5).group("Wood").tip("The broad growth-ring bands crossing the grain"));
    r.param(Param::float("grain_angle", "Grain angle", -90.0, 90.0, 0.0).group("Wood"));
    r.param(unit("gloss", "Gloss", 0.8).group("Varnish"));
    r.param(unit("pores", "Open pores", 0.3).group("Varnish").tip("Grain lines that still show as fine grooves through the varnish"));
    r.param(unit("scratches", "Scratches", 0.3).group("Wear"));
    r.param(unit("wear", "Worn edges", 0.4).group("Wear").tip("Varnish rubbed through to dull bare wood on the edges (reads the baked curvature)"));
    dust_params(&mut r, 0.2);
    dirt_params(&mut r, "dirt", "Dirt", [0.03, 0.02, 0.012]);
    r.pattern_params();

    let wood = mat(ts, "Wood", vec![
        (BaseColor, proc(ProceduralKind::Brushed, 2.0, 461, v(0.12, 0.045, 0.018), v(0.36, 0.16, 0.06))),
        (Roughness, proc(ProceduralKind::Clouds, 5.0, 462, s(0.12), s(0.22))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Brushed, 2.0, 461, s(0.0), s(0.01))),
    ], turned(2.0, 0.0));
    r.range(&wood, BaseColor, "mix(light, dark, 0.3 + grain * 0.7)", "light");
    r.roughness(&wood, "1 - gloss * 0.95", "0.08");
    r.range(&wood, Height, "0 - pores * 0.02", "0");
    r.bind(&wood, Prop::Rotation, "grain_angle");
    r.follow_pattern(&wood);

    let figure = mat(ts, "Figure", vec![(BaseColor, proc(ProceduralKind::WoodRings, 1.0, 463, v(0.12, 0.045, 0.018), v(0.36, 0.16, 0.06)))], turned(1.0, 0.0));
    r.range(&figure, BaseColor, "dark", "light");
    r.bind(&figure, Prop::Opacity { channel: Some(BaseColor) }, "figure * 0.55");
    r.bind(&figure, Prop::Rotation, "grain_angle");
    r.follow_pattern(&figure);

    let scratches = pattern_coat(&mut r, ts, "Scratches", ProceduralKind::Scratches, 9.0, 464, 0.55, 0.0, "scratches", 0.92, 0.5, 0.7);
    r.value(&scratches, BaseColor, "mix(light, rgb(0.7, 0.62, 0.5), 0.4)");
    r.value(&scratches, Roughness, "clamp(1 - gloss * 0.95 + 0.35, 0, 1)");

    let bare = masked(coat(ts, "Bare Wood", [0.4, 0.26, 0.15], 0.7, 0.0), ts, vec![fx(wear()), fx_blend(pattern(ProceduralKind::Clouds, 5.0, 465, 0.45, 0.3), BlendMode::Multiply)]);
    r.value(&bare, BaseColor, "mix(light, rgb(0.62, 0.5, 0.36), 0.5)");
    r.value(&bare, Roughness, "0.72");
    r.bind_fx(&bare, 0, Prop::Level, "wear");
    r.bind(&bare, Prop::Opacity { channel: None }, "saturate(wear * 20) * 0.9");
    r.follow_pattern(&bare);

    let dirt = dirt_layer(&mut r, ts, "Dirt", "dirt", 0.7);
    r.follow_pattern(&dirt);
    let dust = dust_layer(&mut r, ts, 0.5);
    r.finish(folder(&format!("{PREFIX}Varnished Wood"), ts, vec![wood, figure, scratches, bare, occlusion(ts), dirt, dust]))
}

/// The second family of built-in procedural materials, in shelf order.
pub(super) fn smart_materials() -> Vec<SmartMaterial> {
    let named = |name: &str, build: fn(&TextureSet) -> Layer| smart(&format!("{PREFIX}{name}"), build);
    vec![
        named("Enamel Paint", enamel_paint),
        named("Cast Iron", cast_iron),
        named("Anodized Aluminium", anodized_aluminium),
        named("Blued Steel", blued_steel),
        named("Aged Bronze", aged_bronze),
        named("Glossy Plastic", glossy_plastic),
        named("Carbon Fibre", carbon_fibre),
        named("Canvas", canvas),
        named("Denim", denim),
        named("Granite", granite),
        named("Marble", marble),
        named("Sandstone", sandstone),
        named("Rough Rock", rough_rock),
        named("Painted Plaster", painted_plaster),
        named("Cracked Leather", cracked_leather),
        named("Wood Planks", wood_planks),
        named("Varnished Wood", varnished_wood),
    ]
}
