//! Material filters: Painter's MatFinish / MatFX family and PBR Validate. Unlike [`crate::Filter`]
//! (one channel at a time), a material filter rewrites the material below it as a whole: Base
//! Color, Roughness, Metallic and Height together, from one shared pattern. Kernels live in
//! `subcraft_materials::matfx`; this file is only the serializable parameters.
use serde::{Deserialize, Serialize};
use subcraft_core::ChannelKind;

/// Features across the model's longest side (every pattern is sampled in object space, so it
/// never follows UV seams).
pub const MAX_MATERIAL_FILTER_SCALE: f32 = 256.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MaterialFilter {
    /// Hot-dip zinc: a mosaic of crystalline spangles, each reflecting a little differently.
    Galvanized {
        /// Spangles across the model.
        scale: f32,
        /// Brightness / reflection contrast between spangles, 0..1.
        spangle: f32,
        /// How much of the Base Color below is replaced by zinc gray (0 keeps the metal below).
        tint: f32,
        roughness: f32,
        /// Roughness difference between spangles, 0..1.
        roughness_variation: f32,
        /// Grain-boundary relief written to Height, 0..1.
        relief: f32,
        seed: u32,
    },
    /// Hand-hammered sheet: overlapping round dimples with ridges between them.
    Hammered {
        /// Dimples across the model.
        scale: f32,
        /// Dimple depth written to Height, 0..1.
        depth: f32,
        /// 0 = an even mosaic, 1 = irregular hand work.
        irregularity: f32,
        /// Ridges polish, dimples stay duller, 0..1.
        roughness_variation: f32,
        seed: u32,
    },
    /// Powder coat: a dielectric film with fine orange peel.
    PowderCoated {
        color: [f32; 3],
        /// 0 keeps the Base Color below, 1 replaces it by `color`.
        color_amount: f32,
        roughness: f32,
        /// Orange-peel relief written to Height, 0..1.
        orange_peel: f32,
        /// Fine sparkle of the powder grains, 0..1.
        grain: f32,
        /// Orange-peel waves across the model.
        scale: f32,
        seed: u32,
    },
    /// Chipped edges exposing the material underneath (uses the edge wear model, so it follows
    /// the mesh's real edges and the baked curvature).
    EdgeDamages {
        /// Amount of damage, 0..1 (the edge wear Level).
        amount: f32,
        /// Reach from the edges, percent of the model's longest side.
        spread: f32,
        /// Chips across the model.
        scale: f32,
        /// Depth of the chips written to Height, 0..1.
        depth: f32,
        /// The exposed material.
        color: [f32; 3],
        roughness: f32,
        metallic: f32,
        seed: u32,
    },
    /// Rust growing from cavities, edges and random patches, with stains and runs.
    RustWeathering {
        /// Overall amount, 0..1.
        amount: f32,
        /// Extra rust along convex edges, 0..1.
        edges: f32,
        /// Extra rust in occluded cavities, 0..1.
        cavities: f32,
        /// Vertical runs below the patches, 0..1.
        drips: f32,
        /// Patches across the model.
        scale: f32,
        /// Rust scale relief written to Height, 0..1.
        relief: f32,
        roughness: f32,
        seed: u32,
    },
    /// Paint flaking off in angular chips with curled lips and hairline cracks.
    PeelingPaint {
        /// Fraction of the paint that has come off, 0..1.
        amount: f32,
        /// Flakes across the model.
        scale: f32,
        /// How much more the paint peels along edges, 0..1.
        edge_bias: f32,
        /// Paint film thickness written to Height, 0..1.
        thickness: f32,
        /// Raised, curled lip around each peeled area, 0..1.
        curl: f32,
        /// The exposed material.
        color: [f32; 3],
        roughness: f32,
        metallic: f32,
        /// Rust on the exposed material, 0..1.
        rust: f32,
        seed: u32,
    },
    /// Colour-codes the Base Color by physical plausibility: green is in range, red too dark,
    /// blue too bright, yellow a metallic value between dielectric and metal.
    PbrValidate {
        /// Dielectrics may go down to 30 sRGB (off) or only to 50 sRGB (on).
        strict: bool,
        /// Flag Metallic values that are neither 0 nor 1.
        flag_partial_metal: bool,
        /// Show the result unlit-like (Metallic 0, Roughness 1) so reflections do not tint it.
        flat: bool,
    },
    /// Angle-grinder finish: overlapping fans of fine arc scratches.
    Grinded {
        /// Grinder passes across the model.
        scale: f32,
        /// Strength of the scratches in Roughness, Height and tone, 0..1.
        intensity: f32,
        seed: u32,
    },
    /// Coarse brushed finish: long warped brush lines, dulled in patches by grunge.
    Rough {
        /// Brush lines across the model.
        scale: f32,
        /// Grunge patches across the model.
        grunge_scale: f32,
        /// Strength of the brush lines, 0..1.
        brushing: f32,
        /// How much the brush lines wander, 0..1.
        warp: f32,
        /// Strength of the dull grunge patches, 0..1.
        grunge: f32,
        seed: u32,
    },
    /// Raw, unfinished stock: mottled dark mill scale with small pits.
    Raw {
        /// Scale patches across the model.
        scale: f32,
        /// Strength of the mottling, 0..1.
        intensity: f32,
        seed: u32,
    },
    /// A thin, sharp line of wear on the edges, broken up by grunge (finer than Edge Damages,
    /// and almost flat).
    DetailEdgeWear {
        /// Amount of wear, 0..1 (the edge wear Level).
        level: f32,
        /// How much the Height and Normal detail below (painted panel lines, stamps, a normal
        /// map) is worn like a mesh edge, 0..1. 0 follows the mesh edges only.
        detail: f32,
        /// 0 = soft fade, 1 = hard border.
        contrast: f32,
        /// How much the grunge breaks the wear up, 0..1.
        grunge: f32,
        /// Grunge patches across the model.
        grunge_scale: f32,
        /// The exposed material.
        color: [f32; 3],
        roughness: f32,
        metallic: f32,
        seed: u32,
    },
    /// Water drops sitting on the surface: glossy raised lenses, big ones with fine droplets
    /// between them.
    WaterDrops {
        /// Share of the possible drops that are there, 0..1.
        amount: f32,
        /// Big drops across the model.
        scale: f32,
        /// 0 = all drops one size, 1 = very mixed sizes.
        size_variation: f32,
        /// How much drops sag downward on walls, 0..1.
        direction: f32,
        /// Share of the drops kept on faces looking up, sideways and down, 0..1 each.
        top: f32,
        sides: f32,
        underside: f32,
        /// Drop relief written to Height, 0..1.
        height: f32,
        /// Roughness of the water.
        roughness: f32,
        /// How much a drop darkens the Base Color below, 0..1.
        wet: f32,
        seed: u32,
    },
}

impl MaterialFilter {
    pub const ALL: [Self; 12] = [
        Self::Galvanized { scale: 40.0, spangle: 0.6, tint: 0.7, roughness: 0.32, roughness_variation: 0.5, relief: 0.25, seed: 1 },
        Self::Hammered { scale: 26.0, depth: 0.5, irregularity: 0.6, roughness_variation: 0.4, seed: 1 },
        Self::PowderCoated { color: [0.05, 0.05, 0.055], color_amount: 0.0, roughness: 0.48, orange_peel: 0.4, grain: 0.3, scale: 140.0, seed: 1 },
        Self::EdgeDamages { amount: 0.45, spread: 2.5, scale: 30.0, depth: 0.5, color: [0.56, 0.57, 0.58], roughness: 0.38, metallic: 1.0, seed: 1 },
        Self::RustWeathering { amount: 0.45, edges: 0.5, cavities: 0.6, drips: 0.4, scale: 6.0, relief: 0.4, roughness: 0.88, seed: 1 },
        Self::PeelingPaint { amount: 0.25, scale: 6.0, edge_bias: 0.5, thickness: 0.5, curl: 0.5, color: [0.52, 0.53, 0.54], roughness: 0.45, metallic: 1.0, rust: 0.4, seed: 1 },
        Self::PbrValidate { strict: false, flag_partial_metal: true, flat: true },
        Self::Grinded { scale: 9.0, intensity: 0.6, seed: 1 },
        Self::Rough { scale: 60.0, grunge_scale: 9.0, brushing: 0.6, warp: 0.3, grunge: 0.5, seed: 1 },
        Self::Raw { scale: 5.0, intensity: 0.6, seed: 1 },
        Self::DetailEdgeWear { level: 0.5, detail: 0.7, contrast: 0.6, grunge: 0.5, grunge_scale: 24.0, color: [0.56, 0.57, 0.58], roughness: 0.3, metallic: 1.0, seed: 1 },
        Self::WaterDrops { amount: 0.5, scale: 48.0, size_variation: 0.6, direction: 0.4, top: 1.0, sides: 0.8, underside: 0.15, height: 0.6, roughness: 0.04, wet: 0.3, seed: 1 },
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Galvanized { .. } => "MatFinish Galvanized",
            Self::Hammered { .. } => "MatFinish Hammered",
            Self::PowderCoated { .. } => "MatFinish Powder Coated",
            Self::EdgeDamages { .. } => "MatFx Edge Damages",
            Self::RustWeathering { .. } => "MatFx Rust Weathering",
            Self::PeelingPaint { .. } => "MatFx Peeling Paint",
            Self::PbrValidate { .. } => "PBR Validate",
            Self::Grinded { .. } => "MatFinish Grinded",
            Self::Rough { .. } => "MatFinish Rough",
            Self::Raw { .. } => "MatFinish Raw",
            Self::DetailEdgeWear { .. } => "MatFx Detail Edge Wear",
            Self::WaterDrops { .. } => "MatFx Water Drops",
        }
    }

    /// One line for the editor.
    pub fn hint(self) -> &'static str {
        match self {
            Self::Galvanized { .. } => "Zinc spangles on the metal below. Best on a metal fill.",
            Self::Hammered { .. } => "Hammered dimples in the Height of the material below.",
            Self::PowderCoated { .. } => "Turns the material below into a powder-coated film with orange peel.",
            Self::EdgeDamages { .. } => "Chips the edges and exposes the material set here. Bake Curvature for finer detail.",
            Self::RustWeathering { .. } => "Rust from cavities, edges and patches. Bake Ambient Occlusion and Curvature for best placement.",
            Self::PeelingPaint { .. } => "Flakes the material below off the surface set here.",
            Self::PbrValidate { .. } => "Green: plausible. Red: too dark. Blue: too bright. Yellow: Metallic between 0 and 1.",
            Self::Grinded { .. } => "Angle-grinder arcs in the Roughness and Height of the material below. Best on a metal fill.",
            Self::Rough { .. } => "Coarse brush lines and dull grunge patches on the material below.",
            Self::Raw { .. } => "Mottled mill scale and pits, as on unfinished stock. Best on a metal fill.",
            Self::DetailEdgeWear { .. } => "A thin worn line on the mesh edges and on the Height and Normal detail below, exposing the material set here.",
            Self::WaterDrops { .. } => "Glossy water drops on the material below, fewer on walls and undersides.",
        }
    }

    /// The default-valued filter of the same kind (reset gestures, the Add menu).
    pub fn default_like(self) -> Self {
        Self::ALL.into_iter().find(|f| std::mem::discriminant(f) == std::mem::discriminant(&self)).unwrap_or(self)
    }

    /// Channels this filter rewrites. Normal is never written: the relief goes to Height, which
    /// the viewport and the export turn into normals.
    pub fn writes(self, kind: ChannelKind) -> bool {
        match self {
            Self::Hammered { .. } | Self::Grinded { .. } | Self::Rough { .. } | Self::Raw { .. } | Self::WaterDrops { .. } => matches!(kind, ChannelKind::BaseColor | ChannelKind::Roughness | ChannelKind::Height),
            Self::PbrValidate { flat, .. } => kind == ChannelKind::BaseColor || (flat && matches!(kind, ChannelKind::Roughness | ChannelKind::Metallic)),
            _ => matches!(kind, ChannelKind::BaseColor | ChannelKind::Roughness | ChannelKind::Metallic | ChannelKind::Height),
        }
    }

    /// Filters whose Base Color result depends on the Metallic below.
    pub fn reads_metallic(self) -> bool {
        matches!(self, Self::PbrValidate { .. })
    }

    /// Micro details a filter reads from the Height and Normal below it (the ridges of painted
    /// detail count as edges), `None` when it reads none. Only the curvature is used.
    pub fn micro_details(self) -> Option<crate::MicroDetails> {
        match self {
            Self::DetailEdgeWear { detail, .. } if detail > 0.0 && self.valid() => {
                Some(crate::MicroDetails { enabled: true, amount: detail, ao_radius: 0.0, ao_depth: 0.0, ..crate::MicroDetails::default() })
            }
            _ => None,
        }
    }

    /// Filters placed by the mesh's edges (and so by the baked curvature).
    pub fn uses_edges(self) -> bool {
        match self {
            Self::EdgeDamages { .. } | Self::DetailEdgeWear { .. } => true,
            Self::RustWeathering { edges, .. } => edges > 0.0,
            Self::PeelingPaint { edge_bias, .. } => edge_bias > 0.0,
            _ => false,
        }
    }

    pub fn seed(self) -> Option<u32> {
        match self {
            Self::Galvanized { seed, .. }
            | Self::Hammered { seed, .. }
            | Self::PowderCoated { seed, .. }
            | Self::EdgeDamages { seed, .. }
            | Self::RustWeathering { seed, .. }
            | Self::PeelingPaint { seed, .. }
            | Self::Grinded { seed, .. }
            | Self::Rough { seed, .. }
            | Self::Raw { seed, .. }
            | Self::DetailEdgeWear { seed, .. }
            | Self::WaterDrops { seed, .. } => Some(seed),
            Self::PbrValidate { .. } => None,
        }
    }

    /// Every parameter finite and inside its range (project files, MCP input).
    pub fn valid(self) -> bool {
        let unit = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
        let scale = |v: f32| v.is_finite() && (0.1..=MAX_MATERIAL_FILTER_SCALE).contains(&v);
        let color = |c: [f32; 3]| c.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v));
        match self {
            Self::Galvanized { scale: s, spangle, tint, roughness, roughness_variation, relief, seed: _ } => {
                scale(s) && unit(spangle) && unit(tint) && unit(roughness) && unit(roughness_variation) && unit(relief)
            }
            Self::Hammered { scale: s, depth, irregularity, roughness_variation, seed: _ } => scale(s) && unit(depth) && unit(irregularity) && unit(roughness_variation),
            Self::PowderCoated { color: c, color_amount, roughness, orange_peel, grain, scale: s, seed: _ } => {
                color(c) && unit(color_amount) && unit(roughness) && unit(orange_peel) && unit(grain) && scale(s)
            }
            Self::EdgeDamages { amount, spread, scale: s, depth, color: c, roughness, metallic, seed: _ } => {
                unit(amount) && spread.is_finite() && (0.0..=12.5).contains(&spread) && scale(s) && unit(depth) && color(c) && unit(roughness) && unit(metallic)
            }
            Self::RustWeathering { amount, edges, cavities, drips, scale: s, relief, roughness, seed: _ } => {
                unit(amount) && unit(edges) && unit(cavities) && unit(drips) && scale(s) && unit(relief) && unit(roughness)
            }
            Self::PeelingPaint { amount, scale: s, edge_bias, thickness, curl, color: c, roughness, metallic, rust, seed: _ } => {
                unit(amount) && scale(s) && unit(edge_bias) && unit(thickness) && unit(curl) && color(c) && unit(roughness) && unit(metallic) && unit(rust)
            }
            Self::PbrValidate { .. } => true,
            Self::Grinded { scale: s, intensity, seed: _ } | Self::Raw { scale: s, intensity, seed: _ } => scale(s) && unit(intensity),
            Self::Rough { scale: s, grunge_scale, brushing, warp, grunge, seed: _ } => scale(s) && scale(grunge_scale) && unit(brushing) && unit(warp) && unit(grunge),
            Self::DetailEdgeWear { level, detail, contrast, grunge, grunge_scale, color: c, roughness, metallic, seed: _ } => {
                unit(level) && unit(detail) && unit(contrast) && unit(grunge) && scale(grunge_scale) && color(c) && unit(roughness) && unit(metallic)
            }
            Self::WaterDrops { amount, scale: s, size_variation, direction, top, sides, underside, height, roughness, wet, seed: _ } => {
                unit(amount) && scale(s) && unit(size_variation) && unit(direction) && unit(top) && unit(sides) && unit(underside) && unit(height) && unit(roughness) && unit(wet)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_distinct_and_round_trip_as_tagged_json() {
        let mut labels = std::collections::BTreeSet::new();
        for f in MaterialFilter::ALL {
            assert!(f.valid(), "{}", f.label());
            assert!(labels.insert(f.label()));
            assert_eq!(f.default_like(), f);
            let json = serde_json::to_string(&f).unwrap();
            assert!(json.starts_with("{\"kind\":\""), "{json}");
            assert_eq!(serde_json::from_str::<MaterialFilter>(&json).unwrap(), f);
            assert!(!f.writes(ChannelKind::Normal), "relief goes to Height, never Normal");
            assert!(f.writes(ChannelKind::BaseColor));
        }
        assert_eq!(serde_json::to_string(&MaterialFilter::ALL[1]).unwrap(), r#"{"kind":"hammered","scale":26.0,"depth":0.5,"irregularity":0.6,"roughness_variation":0.4,"seed":1}"#);
    }

    #[test]
    fn out_of_range_parameters_are_rejected() {
        assert!(!MaterialFilter::Hammered { scale: 0.0, depth: 0.5, irregularity: 0.5, roughness_variation: 0.5, seed: 1 }.valid());
        assert!(!MaterialFilter::Hammered { scale: 8.0, depth: f32::NAN, irregularity: 0.5, roughness_variation: 0.5, seed: 1 }.valid());
        assert!(!MaterialFilter::EdgeDamages { amount: 0.5, spread: 40.0, scale: 30.0, depth: 0.5, color: [0.5; 3], roughness: 0.4, metallic: 1.0, seed: 1 }.valid());
        assert!(!MaterialFilter::Grinded { scale: 9.0, intensity: 1.5, seed: 1 }.valid());
        assert!(!MaterialFilter::Rough { scale: 60.0, grunge_scale: 0.0, brushing: 0.6, warp: 0.3, grunge: 0.5, seed: 1 }.valid());
        assert!(MaterialFilter::ALL[10].micro_details().is_some_and(|d| d.active() && d.amount == 0.7));
        assert!(MaterialFilter::DetailEdgeWear { level: 0.5, detail: 0.0, contrast: 0.6, grunge: 0.5, grunge_scale: 24.0, color: [0.5; 3], roughness: 0.3, metallic: 1.0, seed: 1 }.micro_details().is_none());
        assert!(MaterialFilter::ALL[3].micro_details().is_none());
        assert!(!MaterialFilter::DetailEdgeWear { level: 0.5, detail: 0.7, contrast: -0.1, grunge: 0.5, grunge_scale: 24.0, color: [0.5; 3], roughness: 0.3, metallic: 1.0, seed: 1 }.valid());
        assert!(!MaterialFilter::PowderCoated { color: [2.0, 0.0, 0.0], color_amount: 1.0, roughness: 0.5, orange_peel: 0.4, grain: 0.3, scale: 60.0, seed: 1 }.valid());
    }

    #[test]
    fn validate_writes_the_shading_channels_only_when_flat() {
        let flat = MaterialFilter::PbrValidate { strict: false, flag_partial_metal: true, flat: true };
        let lit = MaterialFilter::PbrValidate { strict: false, flag_partial_metal: true, flat: false };
        assert!(flat.writes(ChannelKind::Metallic) && flat.writes(ChannelKind::Roughness));
        assert!(!lit.writes(ChannelKind::Metallic) && !lit.writes(ChannelKind::Height));
        assert!(flat.reads_metallic());
    }
}
