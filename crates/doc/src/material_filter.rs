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
        /// Hardness of the chip borders, 0..1: 0 is the soft worn band, 1 is hard-edged chips.
        /// Defaults to 0 so projects saved before this setting look the same.
        #[serde(default)]
        contrast: f32,
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
    /// Occlusion from the Height and Normal relief below: grooves and the foot of raised detail
    /// get darker, duller and are written to the Ambient Occlusion channel.
    Hbao {
        /// Reach of the occlusion, 0..1 (1 = 32 texels at 2048, scaled with the resolution).
        radius: f32,
        /// Strength of the occlusion, 0..1.
        intensity: f32,
        /// How much of it is written to the Ambient Occlusion channel, 0..1.
        ambient_occlusion: f32,
        /// How much it darkens the Base Color, 0..1.
        base_color: f32,
        /// How much it raises the Roughness, 0..1.
        roughness: f32,
    },
    /// Panel lines: whatever is painted on the layer becomes a carved groove with sloped walls.
    /// Paint the lines on a paint layer that carries this filter.
    ShutLine {
        /// Width of the groove's walls, 0..1 (1 = 24 texels at 2048, scaled with the resolution).
        width: f32,
        /// 0 = walls as wide as `width` allows, 1 = almost vertical walls.
        slope: f32,
        /// Raised lip along both sides of the groove, 0..1.
        emboss: f32,
        /// Depth of the groove written to Height, 0..1.
        height: f32,
        /// Colour inside the groove.
        color: [f32; 3],
        /// How much of `color` shows in the groove, 0..1.
        line_opacity: f32,
        /// How much of the painted material itself stays visible, 0..1 (0 keeps only the groove).
        source_opacity: f32,
    },
    /// Linear brushed finish: fine, straight, parallel brush lines.
    BrushedLinear {
        /// Brush lines across the model.
        scale: f32,
        /// Strength of the lines in Roughness, Height and tone, 0..1.
        intensity: f32,
        /// Direction of the lines in the surface, 0..1 = 0..180 degrees (0 = level around the model).
        direction: f32,
        seed: u32,
    },
    /// Fine even grain, as on sandblasted or cast metal.
    Grainy {
        /// Grains across the model.
        scale: f32,
        /// Strength of the grain in Roughness, Height and tone, 0..1.
        intensity: f32,
        seed: u32,
    },
    /// Perforated sheet: staggered rows of round holes.
    PerforatedCircles {
        /// Holes across the model.
        scale: f32,
        /// Size of a hole, 0..1 (1 = the holes almost touch).
        radius: f32,
        /// Depth of the holes written to Height, 0..1.
        depth: f32,
    },
    /// Lighting painted into the Base Color: a sun, a sky above and a ground bounce below, from
    /// the model's own normals and the baked Ambient Occlusion. For stylised and unlit targets.
    BakedLighting {
        /// Direction the sun shines from around the up axis, 0..1 = 0..360 degrees.
        sun_angle: f32,
        /// Height of the sun, 0..1 = the horizon to straight above.
        sun_height: f32,
        sun_intensity: f32,
        sun_color: [f32; 3],
        sky_intensity: f32,
        /// Light from above.
        sky_color: [f32; 3],
        /// Light bounced up from below.
        ground_color: [f32; 3],
        /// How much the baked Ambient Occlusion darkens the light, 0..1.
        ambient_occlusion: f32,
    },
}

/// One setting of a material filter, by name: what a material parameter can drive
/// ([`crate::Prop::Filter`]). A filter only has some of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterSetting {
    Scale,
    Amount,
    Depth,
    Roughness,
    RoughnessVariation,
    Relief,
    Spangle,
    Tint,
    Irregularity,
    Color,
    ColorAmount,
    OrangePeel,
    Grain,
    Spread,
    Metallic,
    Edges,
    Cavities,
    Drips,
    EdgeBias,
    Thickness,
    Curl,
    Rust,
    Seed,
    Intensity,
    GrungeScale,
    Brushing,
    Warp,
    Grunge,
    Level,
    Detail,
    Contrast,
    SizeVariation,
    Direction,
    Top,
    Sides,
    Underside,
    Height,
    Wet,
    Radius,
    AmbientOcclusion,
    BaseColor,
    Width,
    Slope,
    Emboss,
    LineOpacity,
    SourceOpacity,
    SunAngle,
    SunHeight,
    SunIntensity,
    SunColor,
    SkyIntensity,
    SkyColor,
    GroundColor,
}

impl FilterSetting {
    pub fn label(self) -> &'static str {
        match self {
            Self::Scale => "Scale",
            Self::Amount => "Amount",
            Self::Depth => "Depth",
            Self::Roughness => "Roughness",
            Self::RoughnessVariation => "Roughness variation",
            Self::Relief => "Relief",
            Self::Spangle => "Spangle",
            Self::Tint => "Tint",
            Self::Irregularity => "Irregularity",
            Self::Color => "Color",
            Self::ColorAmount => "Color amount",
            Self::OrangePeel => "Orange peel",
            Self::Grain => "Grain",
            Self::Spread => "Spread",
            Self::Metallic => "Metallic",
            Self::Edges => "Edges",
            Self::Cavities => "Cavities",
            Self::Drips => "Drips",
            Self::EdgeBias => "Edge bias",
            Self::Thickness => "Thickness",
            Self::Curl => "Curl",
            Self::Rust => "Rust",
            Self::Seed => "Seed",
            Self::Intensity => "Intensity",
            Self::GrungeScale => "Grunge scale",
            Self::Brushing => "Brushing",
            Self::Warp => "Warp",
            Self::Grunge => "Grunge",
            Self::Level => "Level",
            Self::Detail => "Painted detail",
            Self::Contrast => "Contrast",
            Self::SizeVariation => "Size variation",
            Self::Direction => "Direction",
            Self::Top => "Top",
            Self::Sides => "Sides",
            Self::Underside => "Underside",
            Self::Height => "Height",
            Self::Wet => "Wet darkening",
            Self::Radius => "Radius",
            Self::AmbientOcclusion => "Ambient Occlusion",
            Self::BaseColor => "Base Color",
            Self::Width => "Width",
            Self::Slope => "Slope",
            Self::Emboss => "Emboss",
            Self::LineOpacity => "Line opacity",
            Self::SourceOpacity => "Source material opacity",
            Self::SunAngle => "Sun angle",
            Self::SunHeight => "Sun height",
            Self::SunIntensity => "Sun intensity",
            Self::SunColor => "Sun color",
            Self::SkyIntensity => "Sky intensity",
            Self::SkyColor => "Sky color",
            Self::GroundColor => "Ground color",
        }
    }
}

/// A filter setting's value with its kind (and so its range).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FilterValue {
    /// 0..1.
    Unit(f32),
    /// 0.1..[`MAX_MATERIAL_FILTER_SCALE`].
    Scale(f32),
    /// 0..[`MAX_EDGE_DAMAGE_SPREAD`].
    Spread(f32),
    Color([f32; 3]),
    Seed(u32),
}

/// Largest Edge Damages reach, percent of the model's longest side.
pub const MAX_EDGE_DAMAGE_SPREAD: f32 = 12.5;

enum Slot<'a> {
    Unit(&'a mut f32),
    Scale(&'a mut f32),
    Spread(&'a mut f32),
    Color(&'a mut [f32; 3]),
    Seed(&'a mut u32),
}

impl MaterialFilter {
    fn slot(&mut self, setting: FilterSetting) -> Option<Slot<'_>> {
        use FilterSetting as S;
        Some(match (self, setting) {
            (Self::Galvanized { scale, .. } | Self::Hammered { scale, .. } | Self::PowderCoated { scale, .. } | Self::EdgeDamages { scale, .. } | Self::RustWeathering { scale, .. } | Self::PeelingPaint { scale, .. } | Self::Grinded { scale, .. } | Self::Rough { scale, .. } | Self::Raw { scale, .. } | Self::BrushedLinear { scale, .. } | Self::Grainy { scale, .. } | Self::PerforatedCircles { scale, .. }, S::Scale) => Slot::Scale(scale),
            (Self::Galvanized { seed, .. } | Self::Hammered { seed, .. } | Self::PowderCoated { seed, .. } | Self::EdgeDamages { seed, .. } | Self::RustWeathering { seed, .. } | Self::PeelingPaint { seed, .. } | Self::Grinded { seed, .. } | Self::Rough { seed, .. } | Self::Raw { seed, .. } | Self::DetailEdgeWear { seed, .. } | Self::BrushedLinear { seed, .. } | Self::Grainy { seed, .. }, S::Seed) => Slot::Seed(seed),
            (Self::Galvanized { roughness, .. } | Self::PowderCoated { roughness, .. } | Self::EdgeDamages { roughness, .. } | Self::RustWeathering { roughness, .. } | Self::PeelingPaint { roughness, .. } | Self::DetailEdgeWear { roughness, .. }, S::Roughness) => Slot::Unit(roughness),
            (Self::Galvanized { roughness_variation, .. } | Self::Hammered { roughness_variation, .. }, S::RoughnessVariation) => Slot::Unit(roughness_variation),
            (Self::Galvanized { relief, .. } | Self::RustWeathering { relief, .. }, S::Relief) => Slot::Unit(relief),
            (Self::Galvanized { spangle, .. }, S::Spangle) => Slot::Unit(spangle),
            (Self::Galvanized { tint, .. }, S::Tint) => Slot::Unit(tint),
            (Self::Hammered { depth, .. } | Self::EdgeDamages { depth, .. } | Self::PerforatedCircles { depth, .. }, S::Depth) => Slot::Unit(depth),
            (Self::Hammered { irregularity, .. }, S::Irregularity) => Slot::Unit(irregularity),
            (Self::PowderCoated { color, .. } | Self::EdgeDamages { color, .. } | Self::PeelingPaint { color, .. } | Self::DetailEdgeWear { color, .. }, S::Color) => Slot::Color(color),
            (Self::PowderCoated { color_amount, .. }, S::ColorAmount) => Slot::Unit(color_amount),
            (Self::PowderCoated { orange_peel, .. }, S::OrangePeel) => Slot::Unit(orange_peel),
            (Self::PowderCoated { grain, .. }, S::Grain) => Slot::Unit(grain),
            (Self::EdgeDamages { amount, .. } | Self::RustWeathering { amount, .. } | Self::PeelingPaint { amount, .. }, S::Amount) => Slot::Unit(amount),
            (Self::EdgeDamages { spread, .. }, S::Spread) => Slot::Spread(spread),
            (Self::EdgeDamages { metallic, .. } | Self::PeelingPaint { metallic, .. } | Self::DetailEdgeWear { metallic, .. }, S::Metallic) => Slot::Unit(metallic),
            (Self::RustWeathering { edges, .. }, S::Edges) => Slot::Unit(edges),
            (Self::RustWeathering { cavities, .. }, S::Cavities) => Slot::Unit(cavities),
            (Self::RustWeathering { drips, .. }, S::Drips) => Slot::Unit(drips),
            (Self::PeelingPaint { edge_bias, .. }, S::EdgeBias) => Slot::Unit(edge_bias),
            (Self::PeelingPaint { thickness, .. }, S::Thickness) => Slot::Unit(thickness),
            (Self::PeelingPaint { curl, .. }, S::Curl) => Slot::Unit(curl),
            (Self::PeelingPaint { rust, .. }, S::Rust) => Slot::Unit(rust),
            (Self::Grinded { intensity, .. } | Self::Raw { intensity, .. } | Self::BrushedLinear { intensity, .. } | Self::Grainy { intensity, .. }, S::Intensity) => Slot::Unit(intensity),
            (Self::Rough { grunge_scale, .. } | Self::DetailEdgeWear { grunge_scale, .. }, S::GrungeScale) => Slot::Scale(grunge_scale),
            (Self::Rough { brushing, .. }, S::Brushing) => Slot::Unit(brushing),
            (Self::Rough { warp, .. }, S::Warp) => Slot::Unit(warp),
            (Self::Rough { grunge, .. } | Self::DetailEdgeWear { grunge, .. }, S::Grunge) => Slot::Unit(grunge),
            (Self::DetailEdgeWear { level, .. }, S::Level) => Slot::Unit(level),
            (Self::DetailEdgeWear { detail, .. }, S::Detail) => Slot::Unit(detail),
            (Self::EdgeDamages { contrast, .. } | Self::DetailEdgeWear { contrast, .. }, S::Contrast) => Slot::Unit(contrast),
            (Self::WaterDrops { amount, .. }, S::Amount) => Slot::Unit(amount),
            (Self::WaterDrops { scale, .. }, S::Scale) => Slot::Scale(scale),
            (Self::WaterDrops { seed, .. }, S::Seed) => Slot::Seed(seed),
            (Self::WaterDrops { roughness, .. } | Self::Hbao { roughness, .. }, S::Roughness) => Slot::Unit(roughness),
            (Self::WaterDrops { size_variation, .. }, S::SizeVariation) => Slot::Unit(size_variation),
            (Self::WaterDrops { direction, .. } | Self::BrushedLinear { direction, .. }, S::Direction) => Slot::Unit(direction),
            (Self::WaterDrops { top, .. }, S::Top) => Slot::Unit(top),
            (Self::WaterDrops { sides, .. }, S::Sides) => Slot::Unit(sides),
            (Self::WaterDrops { underside, .. }, S::Underside) => Slot::Unit(underside),
            (Self::WaterDrops { height, .. } | Self::ShutLine { height, .. }, S::Height) => Slot::Unit(height),
            (Self::WaterDrops { wet, .. }, S::Wet) => Slot::Unit(wet),
            (Self::Hbao { radius, .. } | Self::PerforatedCircles { radius, .. }, S::Radius) => Slot::Unit(radius),
            (Self::Hbao { intensity, .. }, S::Intensity) => Slot::Unit(intensity),
            (Self::Hbao { ambient_occlusion, .. } | Self::BakedLighting { ambient_occlusion, .. }, S::AmbientOcclusion) => Slot::Unit(ambient_occlusion),
            (Self::BakedLighting { sun_angle, .. }, S::SunAngle) => Slot::Unit(sun_angle),
            (Self::BakedLighting { sun_height, .. }, S::SunHeight) => Slot::Unit(sun_height),
            (Self::BakedLighting { sun_intensity, .. }, S::SunIntensity) => Slot::Unit(sun_intensity),
            (Self::BakedLighting { sun_color, .. }, S::SunColor) => Slot::Color(sun_color),
            (Self::BakedLighting { sky_intensity, .. }, S::SkyIntensity) => Slot::Unit(sky_intensity),
            (Self::BakedLighting { sky_color, .. }, S::SkyColor) => Slot::Color(sky_color),
            (Self::BakedLighting { ground_color, .. }, S::GroundColor) => Slot::Color(ground_color),
            (Self::Hbao { base_color, .. }, S::BaseColor) => Slot::Unit(base_color),
            (Self::ShutLine { width, .. }, S::Width) => Slot::Unit(width),
            (Self::ShutLine { slope, .. }, S::Slope) => Slot::Unit(slope),
            (Self::ShutLine { emboss, .. }, S::Emboss) => Slot::Unit(emboss),
            (Self::ShutLine { color, .. }, S::Color) => Slot::Color(color),
            (Self::ShutLine { line_opacity, .. }, S::LineOpacity) => Slot::Unit(line_opacity),
            (Self::ShutLine { source_opacity, .. }, S::SourceOpacity) => Slot::Unit(source_opacity),
            _ => return None,
        })
    }

    /// The value of one setting, or `None` when this filter has no such setting.
    pub fn setting(mut self, setting: FilterSetting) -> Option<FilterValue> {
        Some(match self.slot(setting)? {
            Slot::Unit(v) => FilterValue::Unit(*v),
            Slot::Scale(v) => FilterValue::Scale(*v),
            Slot::Spread(v) => FilterValue::Spread(*v),
            Slot::Color(c) => FilterValue::Color(*c),
            Slot::Seed(s) => FilterValue::Seed(*s),
        })
    }

    /// This filter with one setting changed; `v` is clamped into the setting's range (a scalar
    /// reads `v[0]`), so the result stays [`MaterialFilter::valid`]. `None` when the filter has
    /// no such setting or `v` is not finite.
    pub fn with_setting(mut self, setting: FilterSetting, v: [f32; 3]) -> Option<Self> {
        if !v.iter().all(|c| c.is_finite()) {
            return None;
        }
        match self.slot(setting)? {
            Slot::Unit(s) => *s = v[0].clamp(0.0, 1.0),
            Slot::Scale(s) => *s = v[0].clamp(0.1, MAX_MATERIAL_FILTER_SCALE),
            Slot::Spread(s) => *s = v[0].clamp(0.0, MAX_EDGE_DAMAGE_SPREAD),
            Slot::Color(c) => *c = v.map(|x| x.clamp(0.0, 1.0)),
            Slot::Seed(s) => *s = v[0].round().clamp(0.0, u32::MAX as f32) as u32,
        }
        Some(self)
    }

    /// Every setting this filter has, in a fixed order.
    pub fn settings(self) -> Vec<(FilterSetting, FilterValue)> {
        use FilterSetting as S;
        [S::Amount, S::Scale, S::Spread, S::Depth, S::Relief, S::Spangle, S::Tint, S::Irregularity, S::Color, S::ColorAmount, S::Roughness, S::RoughnessVariation, S::Metallic, S::OrangePeel, S::Grain, S::Edges, S::Cavities, S::Drips, S::EdgeBias, S::Thickness, S::Curl, S::Rust, S::Intensity, S::Level, S::Detail, S::Contrast, S::Brushing, S::Warp, S::Grunge, S::GrungeScale, S::SizeVariation, S::Direction, S::Top, S::Sides, S::Underside, S::Height, S::Wet, S::Radius, S::AmbientOcclusion, S::BaseColor, S::Width, S::Slope, S::Emboss, S::LineOpacity, S::SourceOpacity, S::SunAngle, S::SunHeight, S::SunIntensity, S::SunColor, S::SkyIntensity, S::SkyColor, S::GroundColor, S::Seed]
            .into_iter()
            .filter_map(|s| self.setting(s).map(|v| (s, v)))
            .collect()
    }

    pub const ALL: [Self; 18] = [
        Self::Galvanized { scale: 40.0, spangle: 0.6, tint: 0.7, roughness: 0.32, roughness_variation: 0.5, relief: 0.25, seed: 1 },
        Self::Hammered { scale: 26.0, depth: 0.5, irregularity: 0.6, roughness_variation: 0.4, seed: 1 },
        Self::PowderCoated { color: [0.05, 0.05, 0.055], color_amount: 0.0, roughness: 0.48, orange_peel: 0.4, grain: 0.3, scale: 140.0, seed: 1 },
        Self::EdgeDamages { amount: 0.45, spread: 2.5, scale: 30.0, depth: 0.5, color: [0.56, 0.57, 0.58], roughness: 0.38, metallic: 1.0, contrast: 0.0, seed: 1 },
        Self::RustWeathering { amount: 0.45, edges: 0.5, cavities: 0.6, drips: 0.4, scale: 6.0, relief: 0.4, roughness: 0.88, seed: 1 },
        Self::PeelingPaint { amount: 0.25, scale: 6.0, edge_bias: 0.5, thickness: 0.5, curl: 0.5, color: [0.52, 0.53, 0.54], roughness: 0.45, metallic: 1.0, rust: 0.4, seed: 1 },
        Self::PbrValidate { strict: false, flag_partial_metal: true, flat: true },
        Self::Grinded { scale: 9.0, intensity: 0.6, seed: 1 },
        Self::Rough { scale: 60.0, grunge_scale: 9.0, brushing: 0.6, warp: 0.3, grunge: 0.5, seed: 1 },
        Self::Raw { scale: 5.0, intensity: 0.6, seed: 1 },
        Self::DetailEdgeWear { level: 0.5, detail: 0.7, contrast: 0.6, grunge: 0.5, grunge_scale: 24.0, color: [0.56, 0.57, 0.58], roughness: 0.3, metallic: 1.0, seed: 1 },
        Self::WaterDrops { amount: 0.5, scale: 32.0, size_variation: 0.6, direction: 0.4, top: 1.0, sides: 0.8, underside: 0.15, height: 0.6, roughness: 0.04, wet: 0.3, seed: 1 },
        Self::Hbao { radius: 0.35, intensity: 0.8, ambient_occlusion: 1.0, base_color: 0.5, roughness: 0.25 },
        Self::ShutLine { width: 0.25, slope: 0.4, emboss: 0.3, height: 0.6, color: [0.02, 0.02, 0.02], line_opacity: 0.8, source_opacity: 0.0 },
        Self::BrushedLinear { scale: 120.0, intensity: 0.6, direction: 0.0, seed: 1 },
        Self::Grainy { scale: 160.0, intensity: 0.6, seed: 1 },
        Self::PerforatedCircles { scale: 24.0, radius: 0.55, depth: 0.7 },
        Self::BakedLighting { sun_angle: 0.125, sun_height: 0.5, sun_intensity: 0.7, sun_color: [1.0, 0.95, 0.85], sky_intensity: 0.5, sky_color: [0.72, 0.84, 1.0], ground_color: [0.36, 0.30, 0.24], ambient_occlusion: 0.8 },
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
            Self::Hbao { .. } => "MatFx HBAO",
            Self::ShutLine { .. } => "MatFx Shut Line",
            Self::BrushedLinear { .. } => "MatFinish Brushed Linear",
            Self::Grainy { .. } => "MatFinish Grainy",
            Self::PerforatedCircles { .. } => "MatFinish Perforated Circles",
            Self::BakedLighting { .. } => "Baked Lighting Stylized",
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
            Self::Hbao { .. } => "Occlusion from the Height and Normal detail below. Add an Ambient Occlusion channel to store it. Does nothing on a flat material.",
            Self::ShutLine { .. } => "Turns what is painted on this layer into carved panel lines. Use it on a paint layer and paint the lines.",
            Self::BrushedLinear { .. } => "Fine straight brush lines in the Roughness and Height of the material below. Best on a metal fill.",
            Self::Grainy { .. } => "Fine even grain, as on sandblasted or cast metal.",
            Self::PerforatedCircles { .. } => "Staggered rows of round holes pressed into the material below.",
            Self::BakedLighting { .. } => "Paints a sun, sky and ground light into the Base Color. Put it on a Passthrough layer above everything. Bake Ambient Occlusion for shadowed cavities.",
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
            Self::Hammered { .. } | Self::Grinded { .. } | Self::Rough { .. } | Self::Raw { .. } | Self::WaterDrops { .. } | Self::BrushedLinear { .. } | Self::Grainy { .. } => matches!(kind, ChannelKind::BaseColor | ChannelKind::Roughness | ChannelKind::Height),
            Self::Hbao { .. } => matches!(kind, ChannelKind::BaseColor | ChannelKind::Roughness | ChannelKind::AmbientOcclusion),
            Self::BakedLighting { .. } => kind == ChannelKind::BaseColor,
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
            Self::Hbao { radius, intensity, .. } if intensity > 0.0 && self.valid() => {
                Some(crate::MicroDetails { enabled: true, amount: 1.0, ao_radius: radius, ao_depth: intensity, ..crate::MicroDetails::default() })
            }
            _ => None,
        }
    }

    /// Texels at 2048 over which a filter reads the blurred coverage of the layer's own Base
    /// Color content (Shut Line: the painted strokes), `None` when it reads none.
    pub fn coverage_blur(self) -> Option<f32> {
        match self {
            Self::ShutLine { width, .. } if self.valid() => Some(1.0 + width * (Self::MAX_SHUT_LINE_TEXELS - 1.0)),
            _ => None,
        }
    }
    /// Widest Shut Line wall in texels at 2048.
    pub const MAX_SHUT_LINE_TEXELS: f32 = 24.0;
    /// Texels beyond its own that a filter may read at any resolution up to 4096 (0 for most).
    pub fn support(self) -> u32 {
        let details = self.micro_details().map_or(0, |d| d.support());
        let blur = self.coverage_blur().map_or(0, |r| (r * 2.0).ceil() as u32 + 1);
        details.max(blur)
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
            | Self::WaterDrops { seed, .. }
            | Self::BrushedLinear { seed, .. }
            | Self::Grainy { seed, .. } => Some(seed),
            Self::PbrValidate { .. } | Self::Hbao { .. } | Self::ShutLine { .. } | Self::PerforatedCircles { .. } | Self::BakedLighting { .. } => None,
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
            Self::EdgeDamages { amount, spread, scale: s, depth, color: c, roughness, metallic, contrast, seed: _ } => {
                unit(amount) && spread.is_finite() && (0.0..=MAX_EDGE_DAMAGE_SPREAD).contains(&spread) && scale(s) && unit(depth) && color(c) && unit(roughness) && unit(metallic) && unit(contrast)
            }
            Self::RustWeathering { amount, edges, cavities, drips, scale: s, relief, roughness, seed: _ } => {
                unit(amount) && unit(edges) && unit(cavities) && unit(drips) && scale(s) && unit(relief) && unit(roughness)
            }
            Self::PeelingPaint { amount, scale: s, edge_bias, thickness, curl, color: c, roughness, metallic, rust, seed: _ } => {
                unit(amount) && scale(s) && unit(edge_bias) && unit(thickness) && unit(curl) && color(c) && unit(roughness) && unit(metallic) && unit(rust)
            }
            Self::PbrValidate { .. } => true,
            Self::Grinded { scale: s, intensity, seed: _ } | Self::Raw { scale: s, intensity, seed: _ } | Self::Grainy { scale: s, intensity, seed: _ } => scale(s) && unit(intensity),
            Self::BrushedLinear { scale: s, intensity, direction, seed: _ } => scale(s) && unit(intensity) && unit(direction),
            Self::PerforatedCircles { scale: s, radius, depth } => scale(s) && unit(radius) && unit(depth),
            Self::BakedLighting { sun_angle, sun_height, sun_intensity, sun_color, sky_intensity, sky_color, ground_color, ambient_occlusion } => {
                unit(sun_angle) && unit(sun_height) && unit(sun_intensity) && color(sun_color) && unit(sky_intensity) && color(sky_color) && color(ground_color) && unit(ambient_occlusion)
            }
            Self::Rough { scale: s, grunge_scale, brushing, warp, grunge, seed: _ } => scale(s) && scale(grunge_scale) && unit(brushing) && unit(warp) && unit(grunge),
            Self::DetailEdgeWear { level, detail, contrast, grunge, grunge_scale, color: c, roughness, metallic, seed: _ } => {
                unit(level) && unit(detail) && unit(contrast) && unit(grunge) && scale(grunge_scale) && color(c) && unit(roughness) && unit(metallic)
            }
            Self::WaterDrops { amount, scale: s, size_variation, direction, top, sides, underside, height, roughness, wet, seed: _ } => {
                unit(amount) && scale(s) && unit(size_variation) && unit(direction) && unit(top) && unit(sides) && unit(underside) && unit(height) && unit(roughness) && unit(wet)
            }
            Self::ShutLine { width, slope, emboss, height, color: c, line_opacity, source_opacity } => {
                unit(width) && unit(slope) && unit(emboss) && unit(height) && color(c) && unit(line_opacity) && unit(source_opacity)
            }
            Self::Hbao { radius, intensity, ambient_occlusion, base_color, roughness } => unit(radius) && unit(intensity) && unit(ambient_occlusion) && unit(base_color) && unit(roughness),
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
        // Edge Damages saved before Contrast existed still loads, as the same soft band.
        let old = r#"{"kind":"edge_damages","amount":0.45,"spread":2.5,"scale":30.0,"depth":0.5,"color":[0.56,0.57,0.58],"roughness":0.38,"metallic":1.0,"seed":1}"#;
        assert_eq!(serde_json::from_str::<MaterialFilter>(old).unwrap(), MaterialFilter::ALL[3]);
    }

    /// Every field of every filter is reachable as a setting, and a written setting is clamped
    /// so the filter stays valid.
    #[test]
    fn settings_cover_every_field_and_stay_valid() {
        for f in MaterialFilter::ALL {
            let settings = f.settings();
            let fields = serde_json::to_value(f).unwrap().as_object().unwrap().len() - 1;
            if matches!(f, MaterialFilter::PbrValidate { .. }) {
                assert!(settings.is_empty(), "toggles are not driven by formulas");
                continue;
            }
            assert_eq!(settings.len(), fields, "{}", f.label());
            for (s, _) in settings {
                for v in [[-5.0; 3], [0.37; 3], [1e9; 3]] {
                    let g = f.with_setting(s, v).unwrap();
                    assert!(g.valid(), "{} {s:?} {v:?}", f.label());
                }
                assert_ne!(f.with_setting(s, [0.37, 0.2, 0.9]).unwrap(), f, "{} {s:?} changes the filter", f.label());
                assert_eq!(f.with_setting(s, [f32::NAN; 3]), None);
            }
        }
        let h = MaterialFilter::ALL[1];
        assert_eq!(h.setting(FilterSetting::Depth), Some(FilterValue::Unit(0.5)));
        assert_eq!(h.setting(FilterSetting::Rust), None);
        assert_eq!(h.with_setting(FilterSetting::Rust, [0.5; 3]), None);
        assert_eq!(serde_json::to_string(&FilterSetting::RoughnessVariation).unwrap(), "\"roughness_variation\"");
    }

    #[test]
    fn out_of_range_parameters_are_rejected() {
        assert!(!MaterialFilter::Hammered { scale: 0.0, depth: 0.5, irregularity: 0.5, roughness_variation: 0.5, seed: 1 }.valid());
        assert!(!MaterialFilter::Hammered { scale: 8.0, depth: f32::NAN, irregularity: 0.5, roughness_variation: 0.5, seed: 1 }.valid());
        assert!(!MaterialFilter::EdgeDamages { amount: 0.5, spread: 40.0, scale: 30.0, depth: 0.5, color: [0.5; 3], roughness: 0.4, metallic: 1.0, contrast: 0.0, seed: 1 }.valid());
        assert!(!MaterialFilter::Grinded { scale: 9.0, intensity: 1.5, seed: 1 }.valid());
        assert!(!MaterialFilter::BrushedLinear { scale: 120.0, intensity: 0.6, direction: 1.2, seed: 1 }.valid());
        assert!(!MaterialFilter::Grainy { scale: 900.0, intensity: 0.6, seed: 1 }.valid());
        assert!(!MaterialFilter::PerforatedCircles { scale: 24.0, radius: f32::NAN, depth: 0.7 }.valid());
        let light = MaterialFilter::ALL[17];
        assert!(light.writes(ChannelKind::BaseColor) && !light.writes(ChannelKind::Roughness) && !light.writes(ChannelKind::Height) && light.seed().is_none());
        assert!(!light.with_setting(FilterSetting::SunColor, [0.5; 3]).unwrap().with_setting(FilterSetting::SunHeight, [0.2; 3]).is_none());
        assert!(!MaterialFilter::BakedLighting { sun_angle: 1.5, sun_height: 0.5, sun_intensity: 0.7, sun_color: [1.0; 3], sky_intensity: 0.5, sky_color: [1.0; 3], ground_color: [0.3; 3], ambient_occlusion: 0.8 }.valid());
        assert_eq!(serde_json::to_string(&MaterialFilter::ALL[16]).unwrap(), r#"{"kind":"perforated_circles","scale":24.0,"radius":0.55,"depth":0.7}"#);
        assert!(MaterialFilter::ALL[16].writes(ChannelKind::Metallic) && !MaterialFilter::ALL[14].writes(ChannelKind::Metallic) && MaterialFilter::ALL[16].seed().is_none());
        assert!(!MaterialFilter::Rough { scale: 60.0, grunge_scale: 0.0, brushing: 0.6, warp: 0.3, grunge: 0.5, seed: 1 }.valid());
        assert!(MaterialFilter::ALL[10].micro_details().is_some_and(|d| d.active() && d.amount == 0.7));
        assert!(MaterialFilter::DetailEdgeWear { level: 0.5, detail: 0.0, contrast: 0.6, grunge: 0.5, grunge_scale: 24.0, color: [0.5; 3], roughness: 0.3, metallic: 1.0, seed: 1 }.micro_details().is_none());
        assert!(MaterialFilter::ALL[3].micro_details().is_none());
        assert_eq!(MaterialFilter::ALL[13].coverage_blur(), Some(6.75));
        assert_eq!((MaterialFilter::ALL[13].support(), MaterialFilter::ALL[3].support(), MaterialFilter::ALL[10].support()), (15, 0, 3));
        assert!(MaterialFilter::ALL[12].micro_details().is_some_and(|d| d.active() && d.ao_radius == 0.35 && d.ao_depth == 0.8));
        assert!(MaterialFilter::ALL[12].writes(ChannelKind::AmbientOcclusion) && !MaterialFilter::ALL[12].writes(ChannelKind::Height));
        assert!(!MaterialFilter::Hbao { radius: 1.5, intensity: 0.8, ambient_occlusion: 1.0, base_color: 0.5, roughness: 0.25 }.valid());
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
