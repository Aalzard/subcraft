//! A third batch of built-in smart materials and smart masks, plus the category and one-line
//! description the Shelf shows for every smart asset. Built only from this project's own fills,
//! procedural patterns and generators. Edges / Cavity read baked curvature / AO, so bake first.

use subcraft_core::ChannelKind::*;
use subcraft_doc::{BlendMode, GeneratorKind, GradientAxis, Layer, MaskEffectKind, ProceduralKind, TextureSet};

use crate::presets::{dirt_generator, SmartMask, generator, mask, proc, rgb, triplanar, uni};
use crate::smart::{SmartMaterial, coat, occlusion, fx, fx_blend, folder, masked, smart};
use crate::smart_more::{all_white, cavity, edges, mat, pattern, s, top, v};

fn steel_base(ts: &TextureSet, seed: u32) -> Layer {
    mat(ts, "Steel", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 9.0, seed, v(0.5, 0.51, 0.52), v(0.6, 0.61, 0.62))),
        (Roughness, proc(ProceduralKind::Grunge, 8.0, seed + 1, s(0.3), s(0.55))),
        (Metallic, uni(1.0)),
    ], triplanar(2.0))
}

fn rusted_painted_metal(ts: &TextureSet) -> Layer {
    let rust = masked(
        mat(ts, "Rust", vec![
            (BaseColor, proc(ProceduralKind::Grunge, 7.0, 401, v(0.2, 0.065, 0.02), v(0.42, 0.16, 0.05))),
            (Roughness, proc(ProceduralKind::Clouds, 10.0, 402, s(0.8), s(0.97))),
            (Metallic, uni(0.0)),
            (Height, proc(ProceduralKind::Clouds, 18.0, 403, s(0.0), s(0.07))),
        ], triplanar(2.0)),
        ts,
        vec![fx(all_white())],
    );
    let paint = masked(coat(ts, "Paint", [0.06, 0.2, 0.24], 0.4, 0.0), ts, vec![
        fx(all_white()),
        fx_blend(pattern(ProceduralKind::Grunge, 5.0, 404, 0.58, 0.6), BlendMode::Subtract),
        fx_blend(edges(0.55, 0.15, 0.6), BlendMode::Subtract),
    ]);
    let bleed = masked(coat(ts, "Rust Bleed", [0.25, 0.09, 0.03], 0.85, 0.0), ts, vec![fx(dirt_generator())]);
    folder("Rusted Painted Metal", ts, vec![steel_base(ts, 405), rust, paint, bleed])
}

fn oily_metal(ts: &TextureSet) -> Layer {
    let metal = mat(ts, "Dark Steel", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 8.0, 411, v(0.28, 0.285, 0.29), v(0.42, 0.425, 0.43))),
        (Roughness, proc(ProceduralKind::Clouds, 10.0, 412, s(0.3), s(0.5))),
        (Metallic, uni(1.0)),
    ], triplanar(2.0));
    let oil = masked(coat(ts, "Oil Film", [0.02, 0.018, 0.015], 0.1, 0.0), ts, vec![fx(dirt_generator()), fx_blend(pattern(ProceduralKind::Dirt, 5.0, 413, 0.8, 0.1), BlendMode::LinearDodge)]);
    let grime = masked(coat(ts, "Greasy Dust", [0.07, 0.06, 0.05], 0.7, 0.0), ts, vec![fx(pattern(ProceduralKind::Streaks, 5.0, 414, 0.95, 0.05))]);
    folder("Oily Metal", ts, vec![metal, oil, grime])
}

fn painted_steel_edge_wear(ts: &TextureSet) -> Layer {
    let paint = coat(ts, "Safety Paint", [0.62, 0.45, 0.02], 0.38, 0.0);
    let wear = masked(
        mat(ts, "Bare Steel", vec![
            (BaseColor, rgb(0.56, 0.57, 0.58)),
            (Roughness, proc(ProceduralKind::Scratches, 6.0, 421, s(0.35), s(0.6))),
            (Metallic, uni(1.0)),
        ], triplanar(2.0)),
        ts,
        vec![fx(edges(0.5, 0.2, 0.65)), fx_blend(pattern(ProceduralKind::Grunge, 8.0, 422, 0.45, 0.4), BlendMode::Multiply)],
    );
    let dirt = masked(coat(ts, "Dirt", [0.05, 0.04, 0.03], 0.88, 0.0), ts, vec![fx(dirt_generator())]);
    folder("Painted Steel Edge Wear", ts, vec![steel_base(ts, 423), paint, wear, occlusion(ts), dirt])
}

fn sandblasted_aluminium(ts: &TextureSet) -> Layer {
    let alu = mat(ts, "Aluminium", vec![
        (BaseColor, proc(ProceduralKind::Speckles, 30.0, 451, v(0.82, 0.83, 0.83), v(0.92, 0.93, 0.93))),
        (Roughness, proc(ProceduralKind::Speckles, 30.0, 452, s(0.5), s(0.68))),
        (Metallic, uni(1.0)),
        (Height, proc(ProceduralKind::Speckles, 40.0, 453, s(0.0), s(0.015))),
    ], triplanar(2.0));
    let polish = masked(coat(ts, "Polished Edges", [0.91, 0.92, 0.92], 0.25, 1.0), ts, vec![fx(edges(0.55, 0.15, 0.4))]);
    let scuffs = masked(coat(ts, "Light Scuffs", [0.7, 0.71, 0.72], 0.4, 1.0), ts, vec![fx(pattern(ProceduralKind::Scratches, 9.0, 454, 0.5, 0.6))]);
    folder("Sandblasted Aluminium", ts, vec![alu, scuffs, polish])
}

fn galvanized_steel(ts: &TextureSet) -> Layer {
    let zinc = mat(ts, "Galvanized Zinc", vec![
        (BaseColor, proc(ProceduralKind::Cells, 5.0, 491, v(0.55, 0.57, 0.58), v(0.7, 0.72, 0.73))),
        (Roughness, proc(ProceduralKind::Clouds, 10.0, 492, s(0.3), s(0.5))),
        (Metallic, uni(1.0)),
        (Height, proc(ProceduralKind::Cells, 5.0, 491, s(0.0), s(0.02))),
    ], triplanar(2.0));
    let white_rust = masked(coat(ts, "White Rust", [0.62, 0.63, 0.6], 0.85, 0.0), ts, vec![fx(dirt_generator()), fx_blend(pattern(ProceduralKind::Grunge, 6.0, 493, 0.55, 0.4), BlendMode::Multiply)]);
    let rust = masked(coat(ts, "Rust Spots", [0.25, 0.09, 0.035], 0.85, 0.0), ts, vec![fx(edges(0.6, 0.15, 0.8)), fx_blend(pattern(ProceduralKind::Speckles, 7.0, 494, 0.6, 0.2), BlendMode::LinearDodge)]);
    folder("Galvanized Steel", ts, vec![zinc, white_rust, rust])
}

fn pitted_cast_iron(ts: &TextureSet) -> Layer {
    let iron = mat(ts, "Cast Iron", vec![
        (BaseColor, proc(ProceduralKind::Clouds, 12.0, 531, v(0.2, 0.2, 0.205), v(0.32, 0.32, 0.33))),
        (Roughness, proc(ProceduralKind::Grunge, 14.0, 532, s(0.5), s(0.75))),
        (Metallic, uni(1.0)),
        (Height, proc(ProceduralKind::Pores, 24.0, 533, s(0.0), s(0.05))),
    ], triplanar(2.0));
    let rust = masked(coat(ts, "Surface Rust", [0.28, 0.1, 0.04], 0.88, 0.0), ts, vec![fx(pattern(ProceduralKind::Pores, 12.0, 534, 0.5, 0.5)), fx_blend(dirt_generator(), BlendMode::LinearDodge)]);
    let wear = masked(coat(ts, "Handled Edges", [0.55, 0.55, 0.56], 0.3, 1.0), ts, vec![fx(edges(0.6, 0.12, 0.5))]);
    folder("Pitted Cast Iron", ts, vec![iron, rust, wear])
}

pub fn smart_materials() -> Vec<SmartMaterial> {
    vec![
        smart("Rusted Painted Metal", rusted_painted_metal),
        smart("Oily Metal", oily_metal),
        smart("Painted Steel Edge Wear", painted_steel_edge_wear),
        smart("Sandblasted Aluminium", sandblasted_aluminium),
        smart("Galvanized Steel", galvanized_steel),
        smart("Pitted Cast Iron", pitted_cast_iron),
    ]
}

fn axis_gradient(axis: GradientAxis, level: f32, softness: f32) -> MaskEffectKind {
    let MaskEffectKind::Generator(mut g) = generator(GeneratorKind::PositionGradient, level, softness, 0.2, false) else { unreachable!() };
    g.axis = axis;
    MaskEffectKind::Generator(g)
}

pub fn smart_masks() -> Vec<SmartMask> {
    vec![
        mask("Cavity Dust", vec![cavity(0.45, 0.4, 0.5)]),
        mask("Heavy Drips", vec![pattern(ProceduralKind::Streaks, 9.0, 601, 0.62, 0.35)]),
        mask("Scratch Wear", vec![pattern(ProceduralKind::Scratches, 9.0, 602, 0.45, 0.5)]),
        mask("Edge Chips", vec![edges(0.5, 0.08, 0.85)]),
        mask("Rain Streaks", vec![pattern(ProceduralKind::Streaks, 14.0, 603, 0.5, 0.2)]),
        mask("Smudges", vec![pattern(ProceduralKind::Dirt, 12.0, 604, 0.5, 0.15)]),
        mask("Burn Marks", vec![pattern(ProceduralKind::Clouds, 3.0, 605, 0.68, 0.1)]),
        mask("Corner Wear", vec![edges(0.8, 0.3, 0.3)]),
        mask("Moss on Top", vec![top(0.5, 0.3, 0.6)]),
        mask("Random Grime", vec![pattern(ProceduralKind::Grunge, 11.0, 606, 0.5, 0.35)]),
        mask("Fine Dust Speckle", vec![pattern(ProceduralKind::Speckles, 14.0, 607, 0.5, 0.25)]),
        mask("Oil Film", vec![pattern(ProceduralKind::Clouds, 6.0, 608, 0.45, 0.25)]),
        mask("Bottom Mud", vec![generator(GeneratorKind::PositionGradient, 0.2, 0.3, 0.6, true)]),
        mask("Lichen Patches", vec![pattern(ProceduralKind::Cells, 10.0, 609, 0.5, 0.2)]),
        mask("Brushed Wear", vec![pattern(ProceduralKind::Brushed, 8.0, 610, 0.5, 0.5)]),
        mask("Crevice Grime", vec![generator(GeneratorKind::Concave, 0.55, 0.3, 0.35, false)]),
        mask("Thin Areas", vec![generator(GeneratorKind::MeshMap(subcraft_core::MeshMapKind::Thickness), 0.6, 0.3, 0.2, false)]),
        mask("Left Side Fade", vec![axis_gradient(GradientAxis::X, 0.5, 0.5)]),
        mask("Front Side Fade", vec![axis_gradient(GradientAxis::Z, 0.5, 0.5)]),
    ]
}

/// What the Shelf shows for a built-in smart asset: a category chip and a one-line description.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetInfo {
    pub category: &'static str,
    pub description: &'static str,
}

const fn info(category: &'static str, description: &'static str) -> Option<AssetInfo> {
    Some(AssetInfo { category, description })
}

pub fn smart_material_info(name: &str) -> Option<AssetInfo> {
    match name {
        // Procedural materials: parameters stay editable in Properties after applying.
        "Procedural Painted Metal" => info("Procedural", "Painted metal with sliders for paint colour, edge wear, chips and dirt"),
        "Procedural Rusted Steel" => info("Procedural", "Steel with sliders for rust amount, colour, edge sharpness and pitting"),
        "Procedural Brushed Metal" => info("Procedural", "Brushed metal with sliders for colour, brushing, direction, scratches and smudges"),
        "Procedural Polished Metal" => info("Procedural", "Polished metal (gold by default) with sliders for colour, handling marks, scratches and tarnish"),
        "Procedural Hammered Metal" => info("Procedural", "Hammered metal (brass by default) with sliders for dent size and depth, tarnish and polished edges"),
        "Procedural Aged Copper" => info("Procedural", "Copper with sliders for patina amount, colour and relief, and polished edges"),
        "Procedural Worn Plastic" => info("Procedural", "Plastic with sliders for colour, molded grain, scuffs, edge wear and dust"),
        "Procedural Rubber" => info("Procedural", "Rubber with sliders for colour, surface texture, edge wear and dust"),
        "Procedural Concrete" => info("Procedural", "Concrete with sliders for colour, pores, stains, chipped edges and dust"),
        "Procedural Leather" => info("Procedural", "Leather with sliders for colour, grain size and depth, edge wear and dark creases"),
        "Procedural Wood" => info("Procedural", "Wood with sliders for colours, ring size and grain, an optional worn coat of paint, and dirt"),
        "Procedural Enamel Paint" => info("Procedural", "Glossy machine paint over primer and metal, with sliders for edge wear, chips, scratches, rust and dirt"),
        "Procedural Cast Iron" => info("Procedural", "Cast iron with sliders for the casting texture, rust, polished edges and soot"),
        "Procedural Anodized Aluminium" => info("Procedural", "Coloured anodized aluminium with sliders for the dye, satin finish, edge wear, scratches and fingerprints"),
        "Procedural Blued Steel" => info("Procedural", "Blue-black gun steel with sliders for oil marks, worn edges, thin patches and grime"),
        "Procedural Aged Bronze" => info("Procedural", "Bronze with sliders for dark patina, verdigris and polished edges"),
        "Procedural Glossy Plastic" => info("Procedural", "Glossy plastic with sliders for colour, fingerprints, hairline scratches, edge wear and dust"),
        "Procedural Carbon Fibre" => info("Procedural", "Woven carbon fibre under clear coat, with sliders for weave size, contrast, relief and wear"),
        "Procedural Canvas" => info("Procedural", "Woven canvas with sliders for colour, weave size, fuzz, fading, stains and dirt"),
        "Procedural Denim" => info("Procedural", "Denim with sliders for the indigo, white cross threads, fading, stains and dirt"),
        "Procedural Granite" => info("Procedural", "Granite with sliders for its three grain colours, grain size, polish, pitting, dirt and dust"),
        "Procedural Marble" => info("Procedural", "Marble with sliders for colour, veins, clouding, polish and dull marks"),
        "Procedural Sandstone" => info("Procedural", "Sandstone with sliders for sediment layers and their angle, sand grain, eroded edges and holes"),
        "Procedural Rough Rock" => info("Procedural", "Broken rock with sliders for facet size, relief, cracks, pale edges and moss"),
        "Procedural Painted Plaster" => info("Procedural", "A painted plaster wall with sliders for peeling, cracks, chipped corners, water streaks and dirt"),
        "Procedural Rusty Painted Steel" => info("Procedural", "Old painted steel built on the Peeling Paint, Edge Damages and Rust Weathering filters, with Wear, Age and Dirt sliders"),
        "Procedural Chipped Enamel" => info("Procedural", "Glossy fired enamel chipped to a dark rim and rusting steel, with Wear, Age and Dirt sliders"),
        "Procedural Scuffed Gun Metal" => info("Procedural", "Dark gun metal rubbed bright on the edges, scuffed, lightly rusted and grimy, with Wear, Age and Dirt sliders"),
        "Procedural Worn Leather Strap" => info("Procedural", "A used leather strap: burnished, rubbed to raw hide on the edges, cracked and grimy, with Wear, Age and Dirt sliders"),
        "Procedural Dusty Plastic" => info("Procedural", "Old sun-faded plastic with rubbed corners, scuffs and settled dust, with Wear, Age and Dirt sliders"),
        "Procedural Galvanized Steel" => info("Procedural", "Zinc-coated steel with crystal spangles, built on the Galvanized filter, with Wear, Age (white and red rust) and Dirt sliders"),
        "Procedural Powder Coated Metal" => info("Procedural", "A baked powder coat with orange peel that chips off the edges in hard flakes, with Wear, Age and Dirt sliders"),
        "Procedural Hammered Copper" => info("Procedural", "Hand-hammered copper with polished high spots, brown tarnish and verdigris, with Wear, Age and Dirt sliders"),
        "Procedural Raw Steel Plate" => info("Procedural", "Hot-rolled steel with mill scale, ground edges, flash rust and shop dirt, with Wear, Age and Dirt sliders"),
        "Procedural Weathered Painted Wood" => info("Procedural", "Old painted wood: paint rubbed off the edges, cracked and flaked to silvered grain, with Wear, Age and Dirt sliders"),
        "Procedural Worn Canvas" => info("Procedural", "Used cotton canvas: frayed edges, threadbare patches, sun bleach, stains and grime, with Wear, Age and Dirt sliders"),
        "Procedural Weathered Concrete" => info("Procedural", "Outdoor concrete with broken edges, cracks, water and rust streaks, moss and soot, with Wear, Age and Dirt sliders"),
        "Procedural Perished Rubber" => info("Procedural", "Old rubber: shiny rubbed edges, torn chunks, chalky bloom, fine cracks and dust, with Wear, Age and Dirt sliders"),
        "Procedural Chipped Painted Plastic" => info("Procedural", "Painted plastic chipped through primer to the bare shell, scuffed, faded and grimy, with Wear, Age and Dirt sliders"),
        "Procedural Worn Denim" => info("Procedural", "Used indigo denim: pale rubbed edges and high spots, washed-out dye, white weft, stains and grime, with Wear, Age and Dirt sliders"),
        "Procedural Worn Wool" => info("Procedural", "Thick woven wool with a nap that rubs off and pills, fading dye, stains and grime, with Wear, Age and Dirt sliders"),
        "Procedural Stylized Metal" => info("Procedural", "Hand-painted metal for game art: flat colour patches, bold brush strokes, a painted edge highlight, cavity shade and baked light, each on a slider"),
        "Procedural Stylized Wood" => info("Procedural", "Hand-painted wood for game art: bold grain lines, a painted edge highlight, cavity shade and baked light, each on a slider"),
        "Procedural Stylized Gold" => info("Procedural", "Hand-painted gold for game art: flat colour patches, bold brush strokes, a painted edge highlight, cavity shade and baked light, each on a slider"),
        "Procedural Stylized Cloth" => info("Procedural", "Hand-painted cloth for game art: soft painted folds, weave strokes, a painted edge highlight, cavity shade and baked light, each on a slider"),
        "Procedural Stylized Leather" => info("Procedural", "Hand-painted leather for game art: bold creases, pale scuffs, a painted edge highlight, cavity shade and baked light, each on a slider"),
        "Procedural Stylized Stone" => info("Procedural", "Hand-painted stone for game art: chiselled facets, a few bold cracks, a painted edge highlight, cavity shade and baked light, each on a slider"),
        "Procedural Weathered Limestone" => info("Procedural", "Cut limestone with chipped edges, cracks, water streaks, lichen, moss and soot, with Wear, Age and Dirt sliders"),
        "Procedural Cracked Leather" => info("Procedural", "Old dry leather with sliders for cracks, scuffs, folds, edge wear and dark creases"),
        "Procedural Wood Planks" => info("Procedural", "Boards with gaps, with sliders for board size, grain, knots, weathering, worn edges and dirt"),
        "Procedural Varnished Wood" => info("Procedural", "Varnished furniture wood with sliders for grain, figure, gloss, scratches and worn edges"),
        // PBR pack (textured, with wear and dirt layers)
        "Black Basalt" => info("Stone", "Dark volcanic basalt with dirt in the pores and dust on top"),
        "Travertine" => info("Stone", "Pale pitted travertine with dirt in the holes and settled dust"),
        "Terrazzo" => info("Stone", "Polished terrazzo chips with grime in the gaps and dust on top"),
        "Cobblestone" => info("Stone", "Rounded cobbles with dirt between the stones and dust on top"),
        "Cracked Asphalt" => info("Stone", "Weathered asphalt with dirt in the cracks and road dust"),
        "Dry Mud PBR" => info("Nature", "Cracked dry mud, darker in the fissures, dusty on top"),
        "Desert Sand" => info("Nature", "Fine rippled sand with darker hollows and a dusty crust"),
        "Mossy Stone PBR" => info("Nature", "Stone with moss, dark in the crevices and dusty on top"),
        "Tree Bark" => info("Wood", "Deep ridged bark with worn ridges and grime in the furrows"),
        "Cork" => info("Wood", "Granular cork board with worn edges and grime in the pores"),
        "Bamboo Flooring" => info("Wood", "Bamboo strips with worn edges and dirt in the seams"),
        "Woven Linen" => info("Fabric", "Woven linen cloth, rubbed pale on edges, grimy in the folds"),
        "Worn Leather PBR" => info("Fabric", "Creased leather, pale on worn edges and dark in the creases"),
        "Industrial Rubber" => info("Plastic", "Textured rubber with scuffed edges and dirt in the grooves"),
        "Brushed Aluminum" => info("Metal", "Brushed aluminium with polished edges and dirt in recesses"),
        "Oxidized Copper PBR" => info("Metal", "Green-patinated copper, polished on edges, dirty in recesses"),
        "Galvanized Steel PBR" => info("Metal", "Spangled galvanized steel with bright edges and dirt in recesses"),
        "Hex Armor" => info("Metal", "Hexagonal armour plating with polished edges and dirt in the seams"),
        "Worn Painted Metal" => info("Metal", "Blue paint worn through to steel on edges, dirt in crevices"),
        "Rusty Iron" => info("Metal", "Rough iron with rust gathering in the cavities"),
        "Dusty Plastic" => info("Plastic", "Glossy red plastic with dust settling on upward faces"),
        "Tarnished Gold" => info("Metal", "Gold darkened by tarnish in recesses"),
        "Chipped Painted Metal" => info("Paint", "Red paint chipping off bare steel along edges and in flakes"),
        "Brushed Steel" => info("Metal", "Brushed steel with scratches and smudges"),
        "Dusty Concrete" => info("Stone", "Concrete with dust on top, stains and chipped corners"),
        "Oxidized Copper" => info("Metal", "Copper with green verdigris growing in recesses"),
        "Frosted Ice" => info("Nature", "Blue ice with white frost on edges and in crevices"),
        "Hammered Brass" => info("Metal", "Dimpled brass, tarnished in dents, polished on edges"),
        "Rusted Painted Metal" => info("Metal", "Teal paint over steel; rust shows where it chipped"),
        "Oily Metal" => info("Metal", "Dark steel with glossy oil in the cavities and greasy streaks"),
        "Painted Steel Edge Wear" => info("Paint", "Yellow safety paint rubbed down to bare steel on edges"),
        "Sandblasted Aluminium" => info("Metal", "Matte, finely pitted aluminium with polished edges"),
        "Galvanized Steel" => info("Metal", "Spangled zinc coating with white rust and orange spots"),
        "Pitted Cast Iron" => info("Metal", "Dark pitted iron with surface rust and polished edges"),
        // Open pack: built on its scanned image materials.
        "Painted Steel Plates" => info("Paint", "Scanned metal plates with green paint on the edges and dirt"),
        "Rusted Steel" => info("Metal", "Scanned stained steel with rust in the cavities"),
        "Brown Leather" => info("Fabric", "Scanned brown leather with worn edges and grime"),
        "Bricks" => info("Stone", "Scanned bricks with grime in the joints"),
        "Dusty Smooth Concrete" => info("Stone", "Scanned concrete with stains from below and dust on top"),
        _ => None,
    }
}

pub fn smart_mask_info(name: &str) -> Option<AssetInfo> {
    match name {
        "Edge Wear" | "Edge Dirt" | "Chipped Edges" | "Peeling Edges" | "Edge Chips" | "Corner Wear" => info("Wear", "Selects the convex edges (needs a curvature bake)"),
        "Heavy Edge Wear" => info("Wear", "Wide selection of the convex edges (needs a curvature bake)"),
        "Soft Cavity Dirt" => info("Dirt", "Soft, wide selection of crevices (needs an ambient-occlusion bake)"),
        "Rising Damp" => info("Water", "Fades up from the bottom of the model like rising damp"),
        "Cloudy Variation" => info("Breakup", "Large soft clouds to vary a flat layer"),
        "Cavity Dirt" | "Soot Crevices" | "Rust Bleed" | "Moss in Crevices" | "Cavity Dust" => info("Dirt", "Selects crevices (needs an ambient-occlusion bake)"),
        "Top Dust" | "Top-Down Dust" | "Sun Fade" | "Moss on Top" => info("Dust", "Selects faces turned upward"),
        "Bottom Grime" | "Bottom Splash" | "Bottom Mud" => info("Dirt", "Selects the lower part of the model"),
        "Water Streaks" | "Drip Stains" | "Heavy Drips" | "Rain Streaks" => info("Water", "Vertical streaks like running water"),
        "Scratches" | "Scratch Wear" | "Brushed Wear" => info("Wear", "Fine scratch marks across the surface"),
        "Cracks" | "Pitting" | "Burn Marks" => info("Damage", "Cracks, pits or scorch patches"),
        "Crevice Grime" => info("Dirt", "Selects concave creases (needs a curvature bake)"),
        "Dust Occlusion" => info("Dust", "Soft dust in the occluded areas, broken by grunge; a Mask Builder with a slider for each part (needs an ambient-occlusion bake)"),
        "Sharp Dirt" => info("Dirt", "Hard-edged dirt packed into creases; a Mask Builder with a slider for each part (needs ambient-occlusion and curvature bakes)"),
        "Ground Dirt" => info("Dirt", "Dirt that climbs from the bottom of the model and gathers in creases; a Mask Builder with a slider for each part"),
        "Dust from Above" => info("Dust", "Dust settled on faces turned upward and in corners; a Mask Builder with a slider for each part"),
        "Moss" => info("Dirt", "Damp creases and upward faces where moss grows; a Mask Builder with a slider for each part (needs ambient-occlusion and curvature bakes)"),
        "Edges Scratched" => info("Wear", "Worn edges with scratches spreading from them; a Mask Builder with a slider for each part (needs a curvature bake)"),
        "Surface Worn" => info("Wear", "Patchy wear across the faces, a little stronger on edges; a Mask Builder with a slider for each part"),
        "Stains and Scratches" => info("Wear", "Blotchy stains crossed by scratches; a Mask Builder with a slider for each part"),
        "Oxidation" => info("Damage", "Patches of corrosion that favour creases and edges; a Mask Builder with a slider for each part"),
        "Spots" => info("Breakup", "Scattered round spots; a Mask Builder with sliders for density, size and variation"),
        "Fibre Glass Wear" => info("Wear", "Edge wear that frays into a woven glass mat: the Fiber Glass Edge Wear generator, with sliders for wear, weave size and loose strands"),
        "Frayed Edges" => info("Wear", "Cloth edges worn down to loose threads: a fine, irregular weave shows where the wear thins out"),
        "Threadbare Cloth" => info("Wear", "Wide wear that leaves only single threads: for cloth rubbed thin well beyond its edges"),
        "Edge Drips" => info("Water", "Rust on edges and creases with streaks hanging below it: the Dripping Rust generator, with sliders for spreading, drip length and smoothness"),
        "Long Leaks" => info("Water", "Thin, long streaks running down from edges and creases: the Dripping Rust generator set for little rust and long drips"),
        "Soft Run-off Stains" => info("Water", "Wide soft stains under edges and creases, as rain leaves: the Dripping Rust generator with merged drips"),
        "Leaks" => info("Water", "Streaks that run down from the creases: a streak pattern multiplied by an occlusion Mask Builder"),
        "Dirt and Leaks" => info("Dirt", "Crevice dirt plus broken streaks running down: three effects blended together"),
        "Paint Old" => info("Wear", "What is left of an old coat: everything, minus the edges, minus flaked patches"),
        "Thin Areas" => info("Wear", "Selects thin parts of the model (needs a thickness bake)"),
        "Left Side Fade" | "Front Side Fade" => info("Breakup", "Fades across the model along the X or Z axis"),
        "Grunge Breakup" | "Speckled Dirt" | "Grime Blotches" | "Random Grime" | "Smudges" | "Fine Dust Speckle" | "Oil Film" | "Lichen Patches" => info("Breakup", "Irregular blotches to break up a flat layer"),
        _ => None,
    }
}
