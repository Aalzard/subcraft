//! More built-in smart materials and smart masks: stylized, game-art looks built only from this
//! project's own fills, procedural generators and mask effects (no third-party assets).
//! Edges / Cavity read baked curvature / AO, so bake first; without maps those layers stay clean.

use subcraft_core::{ChannelKind::*, ChannelValue};
use subcraft_doc::{BlendMode, GeneratorKind, Layer, MaskEffectKind, ProceduralKind, ProceduralParams, Projection, TextureSet};

use crate::presets::{SmartMask, generator, mask, proc, rgb, triplanar, uni};
use crate::smart::{SmartMaterial, coat, fx, fx_blend, folder, masked, smart};

type V = ChannelValue;

fn v(r: f32, g: f32, b: f32) -> V { ChannelValue::rgb(r, g, b) }
fn s(x: f32) -> V { ChannelValue::scalar(x) }

/// A fill layer with these channel sources.
fn mat(ts: &TextureSet, name: &str, channels: Vec<(subcraft_core::ChannelKind, subcraft_doc::FillSource)>, projection: Projection) -> Layer {
    crate::presets::preset(name, "", channels, projection).build(ts)
}

/// A procedural mask fill: `contrast` / `balance` shape it, projected in 3D so it ignores UV seams.
fn pattern(kind: ProceduralKind, scale: f32, seed: u32, balance: f32, contrast: f32) -> MaskEffectKind {
    MaskEffectKind::Fill {
        value: 1.0,
        procedural: Some(ProceduralParams { scale, seed, balance, contrast, ..ProceduralParams::new(kind) }),
        projection: triplanar(2.0),
    }
}

fn all_white() -> MaskEffectKind {
    MaskEffectKind::Fill { value: 1.0, procedural: None, projection: Projection::default() }
}

/// Wear on edges: legacy threshold on baked curvature (no natural spread), so it stays a thin chip line.
fn edges(level: f32, softness: f32, noise: f32) -> MaskEffectKind {
    let mut g = match generator(GeneratorKind::Edges, level, softness, noise, false) {
        MaskEffectKind::Generator(g) => g,
        _ => unreachable!(),
    };
    g.edge_spread = None;
    MaskEffectKind::Generator(g)
}

fn cavity(level: f32, softness: f32, noise: f32) -> MaskEffectKind { generator(GeneratorKind::Cavity, level, softness, noise, false) }
fn top(level: f32, softness: f32, noise: f32) -> MaskEffectKind { generator(GeneratorKind::Directional, level, softness, noise, false) }

// ----- smart materials -----------------------------------------------------------------------

fn worn_leather(ts: &TextureSet) -> Layer {
    let base = mat(ts, "Leather", vec![
        (BaseColor, proc(ProceduralKind::Cells, 28.0, 61, v(0.12, 0.055, 0.03), v(0.25, 0.115, 0.06))),
        (Roughness, proc(ProceduralKind::Clouds, 10.0, 62, s(0.5), s(0.75))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Pores, 26.0, 63, s(0.0), s(0.05))),
    ], triplanar(1.5));
    let wear = masked(coat(ts, "Worn Edges", [0.38, 0.22, 0.11], 0.3, 0.0), ts, vec![fx(edges(0.6, 0.18, 0.5)), fx_blend(pattern(ProceduralKind::Grunge, 7.0, 9, 0.5, 0.3), BlendMode::Multiply)]);
    let creases = masked(coat(ts, "Creases", [0.04, 0.025, 0.015], 0.8, 0.0), ts, vec![fx(cavity(0.3, 0.3, 0.3))]);
    folder("Worn Leather", ts, vec![base, creases, wear])
}

fn chipped_painted_metal(ts: &TextureSet) -> Layer {
    let steel = mat(ts, "Bare Steel", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 9.0, 71, v(0.42, 0.42, 0.43), v(0.58, 0.58, 0.59))),
        (Roughness, proc(ProceduralKind::Grunge, 8.0, 72, s(0.35), s(0.65))),
        (Metallic, uni(1.0)),
    ], triplanar(2.0));
    // Paint everywhere, minus chips along edges and a few flaked patches.
    let paint = masked(coat(ts, "Paint", [0.5, 0.07, 0.05], 0.42, 0.0), ts, vec![
        fx(all_white()),
        fx_blend(edges(0.55, 0.12, 0.55), BlendMode::Subtract),
        fx_blend(pattern(ProceduralKind::Grunge, 5.0, 14, 0.62, 0.7), BlendMode::Subtract),
    ]);
    let grime = masked(coat(ts, "Grime", [0.05, 0.04, 0.035], 0.85, 0.0), ts, vec![fx(cavity(0.22, 0.3, 0.3))]);
    folder("Chipped Painted Metal", ts, vec![steel, paint, grime])
}

fn mossy_stone(ts: &TextureSet) -> Layer {
    let stone = mat(ts, "Stone", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 7.0, 81, v(0.2, 0.2, 0.19), v(0.42, 0.41, 0.38))),
        (Roughness, proc(ProceduralKind::Grunge, 14.0, 82, s(0.8), s(0.97))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Cells, 10.0, 83, s(0.0), s(0.12))),
    ], triplanar(1.5));
    let moss = masked(
        mat(ts, "Moss", vec![
            (BaseColor, proc(ProceduralKind::Clouds, 14.0, 84, v(0.04, 0.12, 0.025), v(0.14, 0.3, 0.05))),
            (Roughness, uni(0.95)),
            (Metallic, uni(0.0)),
            (Height, proc(ProceduralKind::Clouds, 30.0, 85, s(0.02), s(0.09))),
        ], triplanar(2.0)),
        ts,
        vec![fx(top(0.55, 0.3, 0.45)), fx_blend(cavity(0.4, 0.3, 0.3), BlendMode::LinearDodge), fx_blend(pattern(ProceduralKind::Clouds, 9.0, 86, 0.45, 0.4), BlendMode::Multiply)],
    );
    folder("Mossy Stone", ts, vec![stone, moss])
}

fn brushed_steel(ts: &TextureSet) -> Layer {
    let steel = mat(ts, "Brushed Steel", vec![
        (BaseColor, proc(ProceduralKind::Brushed, 6.0, 91, v(0.3, 0.31, 0.33), v(0.44, 0.45, 0.47))),
        (Roughness, proc(ProceduralKind::Brushed, 6.0, 92, s(0.2), s(0.4))),
        (Metallic, uni(1.0)),
        (Height, proc(ProceduralKind::Brushed, 6.0, 93, s(0.0), s(0.015))),
    ], triplanar(2.0));
    let scratches = masked(coat(ts, "Scratches", [0.78, 0.79, 0.8], 0.45, 1.0), ts, vec![fx(pattern(ProceduralKind::Scratches, 5.0, 94, 0.5, 0.6))]);
    let smudge = masked(coat(ts, "Smudges", [0.35, 0.35, 0.36], 0.6, 1.0), ts, vec![fx_blend(pattern(ProceduralKind::Dirt, 4.0, 95, 0.55, 0.1), BlendMode::Multiply), fx(cavity(0.3, 0.4, 0.2))]);
    folder("Brushed Steel", ts, vec![steel, smudge, scratches])
}

fn dusty_concrete(ts: &TextureSet) -> Layer {
    let concrete = mat(ts, "Concrete", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 8.0, 101, v(0.28, 0.28, 0.27), v(0.46, 0.45, 0.43))),
        (Roughness, proc(ProceduralKind::Grunge, 12.0, 102, s(0.82), s(0.97))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Speckles, 12.0, 103, s(0.0), s(0.05))),
    ], triplanar(2.0));
    let dust = masked(coat(ts, "Dust", [0.52, 0.49, 0.44], 0.98, 0.0), ts, vec![fx(top(0.62, 0.35, 0.45)), fx_blend(pattern(ProceduralKind::Dirt, 5.0, 104, 0.45, 0.2), BlendMode::Multiply)]);
    let chips = masked(coat(ts, "Chipped Corners", [0.6, 0.59, 0.56], 0.9, 0.0), ts, vec![fx(edges(0.6, 0.15, 0.5))]);
    let stains = masked(coat(ts, "Stains", [0.2, 0.19, 0.18], 0.9, 0.0), ts, vec![fx(pattern(ProceduralKind::Streaks, 4.0, 105, 0.68, 0.4))]);
    folder("Dusty Concrete", ts, vec![concrete, stains, dust, chips])
}

fn oxidized_copper(ts: &TextureSet) -> Layer {
    let copper = mat(ts, "Copper", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 8.0, 111, v(0.6, 0.29, 0.19), v(0.8, 0.42, 0.27))),
        (Roughness, proc(ProceduralKind::Clouds, 10.0, 112, s(0.25), s(0.5))),
        (Metallic, uni(1.0)),
    ], triplanar(2.0));
    let verdigris = masked(
        mat(ts, "Verdigris", vec![
            (BaseColor, proc(ProceduralKind::Clouds, 12.0, 113, v(0.08, 0.38, 0.3), v(0.2, 0.62, 0.5))),
            (Roughness, proc(ProceduralKind::Grunge, 16.0, 114, s(0.7), s(0.9))),
            (Metallic, uni(0.0)),
            (Height, proc(ProceduralKind::Grunge, 20.0, 115, s(0.0), s(0.06))),
        ], triplanar(2.0)),
        ts,
        vec![fx(cavity(0.4, 0.3, 0.45)), fx_blend(pattern(ProceduralKind::Grunge, 7.0, 116, 0.62, 0.5), BlendMode::LinearDodge), fx_blend(pattern(ProceduralKind::Clouds, 8.0, 117, 0.3, 0.4), BlendMode::Multiply)],
    );
    folder("Oxidized Copper", ts, vec![copper, verdigris])
}

fn scratched_plastic(ts: &TextureSet) -> Layer {
    let plastic = mat(ts, "Plastic", vec![
        (BaseColor, rgb(0.04, 0.17, 0.5)),
        (Roughness, proc(ProceduralKind::Clouds, 12.0, 121, s(0.2), s(0.32))),
        (Metallic, uni(0.0)),
    ], Projection::default());
    let scratches = masked(coat(ts, "Scratches", [0.75, 0.78, 0.85], 0.55, 0.0), ts, vec![fx(pattern(ProceduralKind::Scratches, 6.0, 122, 0.5, 0.7))]);
    let scuffs = masked(coat(ts, "Scuffs", [0.3, 0.33, 0.45], 0.65, 0.0), ts, vec![fx(edges(0.55, 0.2, 0.6)), fx_blend(pattern(ProceduralKind::Speckles, 5.0, 123, 0.5, 0.2), BlendMode::LinearDodge)]);
    folder("Scratched Plastic", ts, vec![plastic, scuffs, scratches])
}

fn cracked_clay(ts: &TextureSet) -> Layer {
    let clay = mat(ts, "Clay", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 6.0, 131, v(0.4, 0.17, 0.09), v(0.58, 0.29, 0.16))),
        (Roughness, proc(ProceduralKind::Clouds, 14.0, 132, s(0.75), s(0.95))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Cells, 6.0, 133, s(0.0), s(0.1))),
    ], triplanar(1.5));
    let cracks = masked(coat(ts, "Cracks", [0.06, 0.03, 0.02], 0.95, 0.0), ts, vec![fx(pattern(ProceduralKind::Cracks, 5.0, 134, 0.5, 0.9))]);
    let dust = masked(coat(ts, "Dry Dust", [0.62, 0.45, 0.32], 0.98, 0.0), ts, vec![fx(top(0.6, 0.35, 0.4)), fx_blend(pattern(ProceduralKind::Dirt, 6.0, 135, 0.5, 0.2), BlendMode::Multiply)]);
    folder("Cracked Clay", ts, vec![clay, dust, cracks])
}

fn frosted_ice(ts: &TextureSet) -> Layer {
    let ice = mat(ts, "Ice", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 5.0, 141, v(0.28, 0.52, 0.72), v(0.5, 0.75, 0.9))),
        (Roughness, proc(ProceduralKind::Clouds, 9.0, 142, s(0.06), s(0.2))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Clouds, 7.0, 143, s(0.0), s(0.05))),
    ], triplanar(1.5));
    let frost = masked(
        mat(ts, "Frost", vec![
            (BaseColor, rgb(0.85, 0.92, 0.98)),
            (Roughness, proc(ProceduralKind::Speckles, 10.0, 144, s(0.6), s(0.9))),
            (Metallic, uni(0.0)),
            (Height, proc(ProceduralKind::Speckles, 14.0, 145, s(0.0), s(0.04))),
        ], triplanar(2.0)),
        ts,
        vec![fx(pattern(ProceduralKind::Clouds, 7.0, 146, 0.6, 0.5)), fx_blend(edges(0.5, 0.3, 0.5), BlendMode::LinearDodge), fx_blend(cavity(0.4, 0.3, 0.3), BlendMode::LinearDodge)],
    );
    let fractures = masked(coat(ts, "Fractures", [0.82, 0.92, 0.98], 0.3, 0.0), ts, vec![fx(pattern(ProceduralKind::Cracks, 4.0, 147, 0.5, 0.9))]);
    folder("Frosted Ice", ts, vec![ice, fractures, frost])
}

fn rough_wood(ts: &TextureSet) -> Layer {
    let wood = mat(ts, "Rough Wood", vec![
        (BaseColor, proc(ProceduralKind::WoodRings, 3.0, 151, v(0.15, 0.075, 0.035), v(0.4, 0.22, 0.1))),
        (Roughness, proc(ProceduralKind::Brushed, 3.0, 152, s(0.65), s(0.9))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::WoodRings, 3.0, 151, s(0.0), s(0.08))),
    ], triplanar(1.0));
    let weathered = masked(coat(ts, "Weathering", [0.34, 0.29, 0.24], 0.97, 0.0), ts, vec![fx(pattern(ProceduralKind::Streaks, 3.0, 153, 0.45, 0.3)), fx_blend(top(0.6, 0.4, 0.4), BlendMode::LinearDodge)]);
    let splinters = masked(coat(ts, "Splinters", [0.55, 0.4, 0.25], 0.9, 0.0), ts, vec![fx(edges(0.6, 0.2, 0.6))]);
    folder("Rough Wood", ts, vec![wood, weathered, splinters])
}

fn peeling_paint_wood(ts: &TextureSet) -> Layer {
    let wood = mat(ts, "Bare Wood", vec![
        (BaseColor, proc(ProceduralKind::WoodRings, 3.0, 161, v(0.2, 0.1, 0.05), v(0.5, 0.3, 0.15))),
        (Roughness, uni(0.75)),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::WoodRings, 3.0, 161, s(0.0), s(0.05))),
    ], triplanar(1.0));
    let paint = masked(coat(ts, "Paint", [0.08, 0.32, 0.4], 0.5, 0.0), ts, vec![
        fx(all_white()),
        fx_blend(pattern(ProceduralKind::Grunge, 4.0, 162, 0.6, 0.8), BlendMode::Subtract),
        fx_blend(edges(0.5, 0.15, 0.6), BlendMode::Subtract),
    ]);
    let dirt = masked(coat(ts, "Dirt", [0.06, 0.05, 0.04], 0.9, 0.0), ts, vec![fx(cavity(0.25, 0.3, 0.3))]);
    folder("Peeling Painted Wood", ts, vec![wood, paint, dirt])
}

fn hammered_brass(ts: &TextureSet) -> Layer {
    let brass = mat(ts, "Brass", vec![
        (BaseColor, proc(ProceduralKind::Cells, 12.0, 171, v(0.62, 0.45, 0.16), v(0.85, 0.66, 0.28))),
        (Roughness, proc(ProceduralKind::Cells, 12.0, 171, s(0.22), s(0.4))),
        (Metallic, uni(1.0)),
        (Height, proc(ProceduralKind::Cells, 12.0, 171, s(0.0), s(0.1))),
    ], triplanar(2.0));
    let tarnish = masked(coat(ts, "Tarnish", [0.14, 0.09, 0.04], 0.6, 0.6), ts, vec![fx(cavity(0.35, 0.35, 0.4)), fx_blend(pattern(ProceduralKind::Dirt, 5.0, 172, 0.5, 0.2), BlendMode::Multiply)]);
    let polish = masked(coat(ts, "Polished Edges", [1.0, 0.8, 0.4], 0.15, 1.0), ts, vec![fx(edges(0.55, 0.15, 0.4))]);
    folder("Hammered Brass", ts, vec![brass, tarnish, polish])
}

fn sandstone(ts: &TextureSet) -> Layer {
    let sand = mat(ts, "Sandstone", vec![
        (BaseColor, proc(ProceduralKind::Brushed, 3.0, 181, v(0.48, 0.34, 0.2), v(0.7, 0.54, 0.34))),
        (Roughness, uni(0.95)),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Speckles, 14.0, 182, s(0.0), s(0.08))),
    ], triplanar(1.2));
    let strata = masked(coat(ts, "Strata", [0.55, 0.36, 0.22], 0.95, 0.0), ts, vec![fx(pattern(ProceduralKind::Brushed, 3.0, 183, 0.5, 0.6)), fx_blend(pattern(ProceduralKind::Clouds, 6.0, 184, 0.5, 0.4), BlendMode::Multiply)]);
    let dust = masked(coat(ts, "Sand Drift", [0.76, 0.62, 0.42], 0.98, 0.0), ts, vec![fx(top(0.65, 0.35, 0.4)), fx_blend(cavity(0.4, 0.3, 0.3), BlendMode::LinearDodge)]);
    folder("Sandstone", ts, vec![sand, strata, dust])
}

fn dry_mud(ts: &TextureSet) -> Layer {
    let mud = mat(ts, "Mud", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 5.0, 191, v(0.1, 0.06, 0.035), v(0.24, 0.15, 0.08))),
        (Roughness, proc(ProceduralKind::Clouds, 9.0, 192, s(0.55), s(0.9))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Clouds, 6.0, 193, s(0.0), s(0.12))),
    ], triplanar(1.5));
    let cracks = masked(coat(ts, "Dry Cracks", [0.04, 0.025, 0.015], 0.95, 0.0), ts, vec![fx(pattern(ProceduralKind::Cracks, 4.0, 194, 0.5, 0.85)), fx_blend(pattern(ProceduralKind::Clouds, 5.0, 195, 0.4, 0.4), BlendMode::Multiply)]);
    let dried = masked(coat(ts, "Dried Crust", [0.5, 0.38, 0.26], 0.97, 0.0), ts, vec![fx(top(0.6, 0.4, 0.5)), fx_blend(pattern(ProceduralKind::Dirt, 5.0, 196, 0.5, 0.2), BlendMode::Multiply)]);
    folder("Dry Mud", ts, vec![mud, dried, cracks])
}

fn worn_rubber(ts: &TextureSet) -> Layer {
    let rubber = mat(ts, "Rubber", vec![
        (BaseColor, rgb(0.025, 0.025, 0.027)),
        (Roughness, proc(ProceduralKind::Clouds, 12.0, 201, s(0.8), s(0.95))),
        (Metallic, uni(0.0)),
        (Height, proc(ProceduralKind::Pores, 20.0, 202, s(0.0), s(0.03))),
    ], triplanar(2.0));
    let scuffs = masked(coat(ts, "Scuffs", [0.14, 0.14, 0.15], 0.6, 0.0), ts, vec![fx(edges(0.55, 0.2, 0.6)), fx_blend(pattern(ProceduralKind::Scratches, 5.0, 203, 0.5, 0.6), BlendMode::LinearDodge)]);
    let chalk = masked(coat(ts, "Road Dust", [0.4, 0.39, 0.37], 0.95, 0.0), ts, vec![fx(pattern(ProceduralKind::Speckles, 5.0, 204, 0.5, 0.25)), fx_blend(top(0.6, 0.4, 0.4), BlendMode::Multiply)]);
    folder("Worn Rubber", ts, vec![rubber, scuffs, chalk])
}

pub fn smart_materials() -> Vec<SmartMaterial> {
    vec![
        smart("Worn Leather", worn_leather),
        smart("Chipped Painted Metal", chipped_painted_metal),
        smart("Mossy Stone", mossy_stone),
        smart("Brushed Steel", brushed_steel),
        smart("Dusty Concrete", dusty_concrete),
        smart("Oxidized Copper", oxidized_copper),
        smart("Scratched Plastic", scratched_plastic),
        smart("Cracked Clay", cracked_clay),
        smart("Frosted Ice", frosted_ice),
        smart("Rough Wood", rough_wood),
        smart("Peeling Painted Wood", peeling_paint_wood),
        smart("Hammered Brass", hammered_brass),
        smart("Sandstone", sandstone),
        smart("Dry Mud", dry_mud),
        smart("Worn Rubber", worn_rubber),
    ]
}

// ----- smart masks ---------------------------------------------------------------------------

pub fn smart_masks() -> Vec<SmartMask> {
    // A smart mask is a list of plain effect kinds laid over a black mask (a later Normal effect
    // replaces an earlier one), so each recipe is one effect, shaped by its own breakup noise.
    vec![
        mask("Edge Dirt", vec![edges(0.45, 0.4, 0.65)]),
        mask("Top-Down Dust", vec![top(0.7, 0.3, 0.45)]),
        mask("Scratches", vec![pattern(ProceduralKind::Scratches, 5.0, 302, 0.5, 0.65)]),
        mask("Moss in Crevices", vec![cavity(0.3, 0.35, 0.5)]),
        mask("Water Streaks", vec![pattern(ProceduralKind::Streaks, 4.0, 303, 0.45, 0.35)]),
        mask("Drip Stains", vec![pattern(ProceduralKind::Streaks, 7.0, 304, 0.55, 0.5)]),
        mask("Chipped Edges", vec![edges(0.6, 0.12, 0.6)]),
        mask("Cracks", vec![pattern(ProceduralKind::Cracks, 5.0, 305, 0.5, 0.9)]),
        mask("Speckled Dirt", vec![pattern(ProceduralKind::Speckles, 5.0, 306, 0.5, 0.3)]),
        mask("Rust Bleed", vec![cavity(0.28, 0.3, 0.75)]),
        mask("Bottom Splash", vec![generator(GeneratorKind::PositionGradient, 0.35, 0.4, 0.5, true)]),
        mask("Sun Fade", vec![top(0.55, 0.5, 0.35)]),
        mask("Soot Crevices", vec![cavity(0.15, 0.4, 0.3)]),
        mask("Peeling Edges", vec![edges(0.45, 0.35, 0.85)]),
        mask("Pitting", vec![pattern(ProceduralKind::Pores, 6.0, 309, 0.5, 0.6)]),
        mask("Grime Blotches", vec![pattern(ProceduralKind::Dirt, 5.0, 310, 0.5, 0.4)]),
    ]
}
