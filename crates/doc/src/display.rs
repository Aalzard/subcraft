//! Display settings: how the 3D view shows the project (environment, camera, post effects,
//! visualisation aids). Plain data, saved with the project and never part of the undo history.
//! The renderer and the Display Settings panel both read this one struct.
//!
//! Every field has a default, so a project (or a preference file) written by an older version
//! loads unchanged, and unknown enum values fall back to the default instead of failing the load.

use serde::{Deserialize, Deserializer, Serialize};

/// A value written either by name (current files) or as the list index older preference files
/// used; anything unreadable becomes the default.
#[derive(Deserialize)]
#[serde(untagged)]
enum Compat<T> {
    Value(T),
    Index(u64),
    Other(serde::de::IgnoredAny),
}

fn compat<'de, D: Deserializer<'de>, T: Deserialize<'de> + Default + Copy>(d: D, all: &[T]) -> Result<T, D::Error> {
    Ok(match Compat::<T>::deserialize(d)? {
        Compat::Value(v) => v,
        Compat::Index(i) => all.get(i as usize).copied().unwrap_or_default(),
        Compat::Other(_) => T::default(),
    })
}

/// How HDR light is mapped to the display (the Tone mapping effect's Function).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToneMap {
    /// Film-like curve with soft highlights (the original look).
    #[default]
    Aces,
    /// Hable's filmic curve: punchier contrast, warm highlights.
    Filmic,
    /// Reinhard per color channel: gentle, a little flat.
    Reinhard,
    /// No curve and no clamp of its own (the display still ends at white).
    Linear,
    /// Picks the function for you: the sensitometric curve.
    Auto,
    /// No curve, values clamped to 0..1.
    LinearSat,
    /// Film response (an exponential shoulder), no Mapping Factor.
    Sensitometric,
    /// Reinhard on the luminance only: hues are kept while highlights compress.
    ReinhardLum,
    /// Logarithmic compression per color channel.
    Log,
    /// Logarithmic compression of the luminance only.
    LogLum,
}

impl ToneMap {
    /// Storage order of old preference files (an index); new entries are only ever appended.
    pub const ALL: [ToneMap; 10] = [
        ToneMap::Aces, ToneMap::Filmic, ToneMap::Reinhard, ToneMap::Linear, ToneMap::Auto, ToneMap::LinearSat, ToneMap::Sensitometric,
        ToneMap::ReinhardLum, ToneMap::Log, ToneMap::LogLum,
    ];

    /// The Function dropdown: Painter's list in its order, then SubCraft's extra curve.
    pub const MENU: [ToneMap; 10] = [
        ToneMap::Auto, ToneMap::Linear, ToneMap::LinearSat, ToneMap::Sensitometric, ToneMap::Reinhard, ToneMap::ReinhardLum, ToneMap::Log,
        ToneMap::LogLum, ToneMap::Aces, ToneMap::Filmic,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ToneMap::Aces => "ACES",
            ToneMap::Filmic => "Filmic",
            ToneMap::Reinhard => "Reinhard",
            ToneMap::Linear => "Linear",
            ToneMap::Auto => "Auto",
            ToneMap::LinearSat => "LinearSat",
            ToneMap::Sensitometric => "Sensitometric",
            ToneMap::ReinhardLum => "ReinhardLum",
            ToneMap::Log => "Log",
            ToneMap::LogLum => "LogLum",
        }
    }

    /// The function actually evaluated (Auto chooses the sensitometric curve).
    pub fn resolved(self) -> ToneMap {
        if self == ToneMap::Auto { ToneMap::Sensitometric } else { self }
    }

    /// Whether the Mapping Factor changes this function.
    pub fn uses_mapping_factor(self) -> bool {
        matches!(self, ToneMap::Reinhard | ToneMap::ReinhardLum | ToneMap::Log | ToneMap::LogLum)
    }
}

/// Shape of the glare thrown by bright areas (Painter's Glare > Shape list).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlareShape {
    /// One horizontal streak.
    #[default]
    #[serde(alias = "horizontal_streak")]
    Horizontal,
    /// One vertical streak.
    #[serde(alias = "vertical_streak")]
    Vertical,
    /// Cross screen filter: a horizontal and a vertical streak (four points).
    #[serde(alias = "cross_screen")]
    Both,
    /// Sunny cross filter: four streaks, eight points.
    #[serde(alias = "sunny_cross")]
    Star,
    /// A round glow, no streaks.
    Bloom,
    /// A round glow with ghost images mirrored through the view centre.
    LensFlare,
    /// A round glow with a four-point cross.
    Standard,
    /// A small diagonal cross with one ghost: the look of a simple lens.
    CheapLens,
    /// A long, faint horizontal trail.
    AfterImage,
    /// Cross screen filter with rainbow fringes.
    CrossScreenSpectral,
    /// Snow cross filter: three streaks, six points.
    SnowCross,
    SnowCrossSpectral,
    SunnyCrossSpectral,
}

impl GlareShape {
    /// Storage order of old preference files (an index); new entries are only ever appended.
    pub const ALL: [GlareShape; 13] = [
        GlareShape::Horizontal, GlareShape::Vertical, GlareShape::Both, GlareShape::Star, GlareShape::Bloom, GlareShape::LensFlare,
        GlareShape::Standard, GlareShape::CheapLens, GlareShape::AfterImage, GlareShape::CrossScreenSpectral, GlareShape::SnowCross,
        GlareShape::SnowCrossSpectral, GlareShape::SunnyCrossSpectral,
    ];

    /// The Shape dropdown in Painter's order.
    pub const MENU: [GlareShape; 13] = [
        GlareShape::Bloom, GlareShape::LensFlare, GlareShape::Standard, GlareShape::CheapLens, GlareShape::AfterImage, GlareShape::Both,
        GlareShape::CrossScreenSpectral, GlareShape::SnowCross, GlareShape::SnowCrossSpectral, GlareShape::Star, GlareShape::SunnyCrossSpectral,
        GlareShape::Horizontal, GlareShape::Vertical,
    ];

    pub fn label(self) -> &'static str {
        match self {
            GlareShape::Horizontal => "Horizontal Streak",
            GlareShape::Vertical => "Vertical Streak",
            GlareShape::Both => "Filter Cross Screen",
            GlareShape::Star => "Filter Sunny Cross",
            GlareShape::Bloom => "Bloom",
            GlareShape::LensFlare => "Lens Flare",
            GlareShape::Standard => "Standard",
            GlareShape::CheapLens => "Cheap Lens",
            GlareShape::AfterImage => "After Image",
            GlareShape::CrossScreenSpectral => "Filter Cross Screen Spectral",
            GlareShape::SnowCross => "Filter Snow Cross",
            GlareShape::SnowCrossSpectral => "Filter Snow Cross Spectral",
            GlareShape::SunnyCrossSpectral => "Filter Sunny Cross Spectral",
        }
    }
}

/// Film grain look.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrainType {
    /// The same noise on red, green and blue.
    #[default]
    Monochromatic,
    /// Independent noise per color channel.
    Colored,
}

impl GrainType {
    pub const ALL: [GrainType; 2] = [GrainType::Monochromatic, GrainType::Colored];

    pub fn label(self) -> &'static str {
        match self {
            GrainType::Monochromatic => "Monochromatic",
            GrainType::Colored => "Colored",
        }
    }
}

fn grain_compat<'de, D: Deserializer<'de>>(d: D) -> Result<GrainType, D::Error> {
    compat(d, &GrainType::ALL)
}

/// Lens of the Lens flare effect: how far the ghosts spread.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlareCamera {
    /// Ghosts spread over the whole view.
    #[default]
    Panoramic,
    /// Ghosts stay close to the view centre.
    Telephoto,
}

impl FlareCamera {
    pub const ALL: [FlareCamera; 2] = [FlareCamera::Panoramic, FlareCamera::Telephoto];

    pub fn label(self) -> &'static str {
        match self {
            FlareCamera::Panoramic => "Panoramic Lens",
            FlareCamera::Telephoto => "Telephoto Lens",
        }
    }
}

fn flare_camera_compat<'de, D: Deserializer<'de>>(d: D) -> Result<FlareCamera, D::Error> {
    compat(d, &FlareCamera::ALL)
}

/// Which components of a channel shown alone are displayed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelComponent {
    /// The channel as it is: RGBA for color channels, grayscale + alpha for the others.
    #[default]
    All,
    R,
    G,
    B,
    Alpha,
}

impl ChannelComponent {
    pub const ALL: [ChannelComponent; 5] = [ChannelComponent::All, ChannelComponent::R, ChannelComponent::G, ChannelComponent::B, ChannelComponent::Alpha];

    /// Entry of the Color channels list; the first one is named after the kind of channel.
    pub fn label(self, color: bool) -> &'static str {
        match self {
            ChannelComponent::All if color => "RGBA",
            ChannelComponent::All => "Grayscale + Alpha",
            ChannelComponent::R => "R",
            ChannelComponent::G => "G",
            ChannelComponent::B => "B",
            ChannelComponent::Alpha => "Alpha",
        }
    }

    /// Name written over the view while a single component is shown.
    pub fn overlay(self) -> Option<&'static str> {
        match self {
            ChannelComponent::All => None,
            ChannelComponent::R => Some("Red"),
            ChannelComponent::G => Some("Green"),
            ChannelComponent::B => Some("Blue"),
            ChannelComponent::Alpha => Some("Alpha"),
        }
    }
}

fn component_compat<'de, D: Deserializer<'de>>(d: D) -> Result<ChannelComponent, D::Error> {
    compat(d, &ChannelComponent::ALL)
}

/// The axis the grid plane is perpendicular to (the "up" of the grid).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GridAxis {
    X,
    #[default]
    Y,
    Z,
}

impl GridAxis {
    pub const ALL: [GridAxis; 3] = [GridAxis::X, GridAxis::Y, GridAxis::Z];

    pub fn label(self) -> &'static str {
        match self {
            GridAxis::X => "X",
            GridAxis::Y => "Y",
            GridAxis::Z => "Z",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

fn grid_axis_compat<'de, D: Deserializer<'de>>(d: D) -> Result<GridAxis, D::Error> {
    // Index 1 (Y) is the default, so an unknown value falls back to the usual floor.
    compat(d, &GridAxis::ALL)
}

/// Anisotropic texture filtering quality (samples per pixel at grazing angles).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anisotropy {
    Disabled,
    Low,
    #[default]
    Medium,
    High,
    VeryHigh,
}

impl Anisotropy {
    pub const ALL: [Anisotropy; 5] = [Anisotropy::Disabled, Anisotropy::Low, Anisotropy::Medium, Anisotropy::High, Anisotropy::VeryHigh];

    pub fn label(self) -> &'static str {
        match self {
            Anisotropy::Disabled => "Disabled",
            Anisotropy::Low => "Low (2 spp)",
            Anisotropy::Medium => "Medium (4 spp)",
            Anisotropy::High => "High (8 spp)",
            Anisotropy::VeryHigh => "Very High (16 spp)",
        }
    }

    /// Samples per pixel (1 = off).
    pub fn samples(self) -> u16 {
        match self {
            Anisotropy::Disabled => 1,
            Anisotropy::Low => 2,
            Anisotropy::Medium => 4,
            Anisotropy::High => 8,
            Anisotropy::VeryHigh => 16,
        }
    }
}

fn anisotropy_compat<'de, D: Deserializer<'de>>(d: D) -> Result<Anisotropy, D::Error> {
    Ok(match Compat::<Anisotropy>::deserialize(d)? {
        Compat::Value(v) => v,
        Compat::Index(i) => Anisotropy::ALL.get(i as usize).copied().unwrap_or_default(),
        Compat::Other(_) => Anisotropy::default(),
    })
}

/// Names of the MipMap bias steps: 0 Soft .. 3 Very Sharp (a texture level bias of 0 .. -3).
pub const MIP_BIAS_LABELS: [&str; 4] = ["0 - Soft", "1 - Medium Soft", "2 - Sharp", "3 - Very Sharp"];

/// Grid opacity that draws the lines at their original strength.
pub const DEFAULT_GRID_OPACITY: f32 = 0.25;
/// Default grid line color (sRGB), the gray the grid always had.
pub const DEFAULT_GRID_COLOR: [f32; 3] = [0.4245, 0.4370, 0.4614];

fn tone_map_compat<'de, D: Deserializer<'de>>(d: D) -> Result<ToneMap, D::Error> {
    compat(d, &ToneMap::ALL)
}

fn glare_compat<'de, D: Deserializer<'de>>(d: D) -> Result<GlareShape, D::Error> {
    compat(d, &GlareShape::ALL)
}

/// What the environment map is attached to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvAlignment {
    /// Fixed in the scene: orbiting the camera shows another part of the map.
    #[default]
    World,
    /// Fixed to the camera: the lighting follows the view while orbiting.
    Local,
}

impl EnvAlignment {
    pub const ALL: [EnvAlignment; 2] = [EnvAlignment::World, EnvAlignment::Local];

    pub fn label(self) -> &'static str {
        match self {
            EnvAlignment::World => "World",
            EnvAlignment::Local => "Local",
        }
    }
}

fn alignment_compat<'de, D: Deserializer<'de>>(d: D) -> Result<EnvAlignment, D::Error> {
    compat(d, &EnvAlignment::ALL)
}

/// Sensor height (mm) behind the focal-length readout: a full-frame 36 x 24 mm sensor.
pub const SENSOR_HEIGHT_MM: f32 = 24.0;
/// Field-of-view range of the camera (vertical, degrees).
pub const FOV_RANGE: std::ops::RangeInclusive<f32> = 5.0..=120.0;

/// Focal length (mm) of a lens with vertical field of view `fov_deg` on the reference sensor.
pub fn focal_length_mm(fov_deg: f32) -> f32 {
    0.5 * SENSOR_HEIGHT_MM / (fov_deg.to_radians() * 0.5).tan().max(1e-6)
}

/// Vertical field of view (degrees) of a lens of focal length `mm` on the reference sensor.
pub fn fov_from_focal_length(mm: f32) -> f32 {
    2.0 * (0.5 * SENSOR_HEIGHT_MM / mm.max(1e-3)).atan().to_degrees()
}

/// Brightness multiplier of an exposure offset in EV (stops): +1 EV doubles the light.
pub fn ev_to_multiplier(ev: f32) -> f32 {
    2f32.powf(ev)
}

/// Read a project's display settings keeping every value that parses. A value this build cannot
/// read (a `null` written for a non-finite number, a wrong type in a hand-edited file) used to
/// fail the whole document, so the project could not be opened at all; it takes its default now.
pub fn lenient_display<'de, D: Deserializer<'de>>(d: D) -> Result<Option<DisplaySettings>, D::Error> {
    let Some(value) = Option::<serde_json::Value>::deserialize(d)? else { return Ok(None) };
    if let Ok(settings) = serde_json::from_value::<DisplaySettings>(value.clone()) {
        return Ok(Some(settings));
    }
    let serde_json::Value::Object(stored) = value else { return Ok(Some(DisplaySettings::default())) };
    let serde_json::Value::Object(mut merged) = serde_json::to_value(DisplaySettings::default()).map_err(serde::de::Error::custom)? else {
        return Ok(Some(DisplaySettings::default()));
    };
    // Every stored key is tried, including the ones the defaults do not write at all (an
    // environment map: `None` is skipped when saving), which used to be dropped here.
    for (key, value) in stored {
        let previous = merged.insert(key.clone(), value);
        if serde_json::from_value::<DisplaySettings>(serde_json::Value::Object(merged.clone())).is_err() {
            match previous {
                Some(default) => merged.insert(key, default),
                None => merged.remove(&key),
            };
        }
    }
    Ok(Some(serde_json::from_value(serde_json::Value::Object(merged)).unwrap_or_default()))
}

/// Everything the Display Settings panel edits. Field names are the file format: rename with a
/// `#[serde(alias)]`, never in place.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplaySettings {
    // ----- environment -------------------------------------------------------------------------
    /// Name of the environment map (an asset name, never a path). `None` = not chosen yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env_map: Option<String>,
    /// How much of the environment map shows behind the model (0 = the plain background color).
    pub env_opacity: f32,
    /// Environment brightness in EV (stops): 0 is the map as authored, +1 doubles it.
    pub env_exposure: f32,
    /// Rotation of the environment around the vertical axis, degrees (-180..180).
    pub env_rotation: f32,
    /// Blur of the environment shown behind the model (0 = sharp, 1 = fully diffused).
    pub env_blur: f32,
    #[serde(deserialize_with = "alignment_compat")]
    pub env_alignment: EnvAlignment,

    // ----- camera ------------------------------------------------------------------------------
    /// Vertical field of view, degrees.
    pub fov: f32,
    /// 4x MSAA.
    pub antialias: bool,
    /// Depth of field (off by default): blur amount 0..1, focus distance as a multiple of the
    /// orbit distance (1 = the point the camera orbits), sharp band in percent of the model size.
    pub dof: bool,
    pub dof_amount: f32,
    pub dof_focus: f32,
    pub dof_range: f32,

    // ----- post effects ------------------------------------------------------------------------
    /// Master switch ("Activate Post Effects"): off shows the plain render with the default
    /// tone curve, whatever the effects below are set to.
    pub post_effects: bool,
    /// Tone mapping switch: off shows linear, clamped light at exposure 1.
    pub tone_mapping_on: bool,
    #[serde(deserialize_with = "tone_map_compat")]
    pub tone_map: ToneMap,
    /// Brightness multiplier before tone mapping.
    pub exposure: f32,
    /// Display gamma applied after the curve (1 = none).
    pub gamma: f32,
    /// Strength of the Reinhard / Log compression (1 = the plain curve).
    pub mapping_factor: f32,
    pub bloom_on: bool,
    pub bloom: f32,
    pub bloom_threshold: f32,
    /// How far the bloom spreads, 0 (tight) .. 1 (wide).
    pub bloom_falloff: f32,
    pub ssao: f32,
    /// Ambient-occlusion reach in percent of the model's size.
    pub ssao_radius: f32,
    /// Glow around emissive areas (0 = off) and its reach multiplier.
    pub emission_glow: f32,
    pub glow_radius: f32,
    pub ssao_on: bool,
    pub glow_on: bool,
    /// Glare from bright emission: luminance (0 = off), shape and streak length 0..1.
    pub glare_on: bool,
    pub glare: f32,
    #[serde(deserialize_with = "glare_compat")]
    pub glare_mode: GlareShape,
    pub glare_length: f32,
    /// Light below this brightness throws no glare.
    pub glare_threshold: f32,
    /// Response of the glare to brightness above the threshold (1 = proportional).
    pub glare_remap: f32,
    /// Lens flare: ghost images of bright areas mirrored through the view centre.
    pub flare_on: bool,
    #[serde(deserialize_with = "flare_camera_compat")]
    pub flare_camera: FlareCamera,
    pub flare_amount: f32,
    pub flare_threshold: f32,
    /// Number of ghost images (1..8).
    pub flare_ghosts: u32,
    /// Spacing of the ghosts along the line through the view centre.
    pub flare_position_scale: f32,
    /// Lateral (chromatic) aberration: color fringes growing away from the centre.
    pub aberration_on: bool,
    pub aberration_amount: f32,
    /// Distance from the centre (0..1 of the half diagonal) where the fringes reach half strength.
    pub aberration_radius: f32,
    pub aberration_softness: f32,
    /// Horizontal stretch of the unaffected zone (1 = circular).
    pub aberration_ratio: f32,
    /// Centre of the effect in the view, 0..1.
    pub aberration_center: [f32; 2],
    /// Blur radially instead of splitting the colors.
    pub aberration_achromatic: bool,
    pub vignette_on: bool,
    pub vignette_amount: f32,
    pub sharpen_on: bool,
    pub sharpen_amount: f32,
    /// Sharpen diameter in pixels.
    pub sharpen_diameter: f32,
    pub grain_on: bool,
    pub grain_amount: f32,
    /// Grain size in pixels.
    pub grain_radius: f32,
    #[serde(deserialize_with = "grain_compat")]
    pub grain_type: GrainType,
    pub color_correction_on: bool,
    pub cc_saturation: f32,
    pub cc_contrast: f32,
    pub cc_brightness: f32,
    pub cc_bias: f32,
    pub cc_sepia: f32,
    /// White balance temperature, kelvin (6500 = neutral).
    pub cc_temperature: f32,

    // ----- visualisation -----------------------------------------------------------------------
    /// Polygon wireframe over the model: on/off, sRGB color, opacity and the largest angle (degrees)
    /// between two triangles that still count as one polygon.
    pub wireframe: bool,
    pub wire_color: [f32; 3],
    pub wire_opacity: f32,
    pub wire_angle: f32,
    /// Display-only floor grid and axis guides.
    pub grid: bool,
    pub grid_axes: [bool; 3],
    /// Axis the grid plane is perpendicular to, line color (sRGB) and opacity 0..1.
    #[serde(deserialize_with = "grid_axis_compat")]
    pub grid_plane: GridAxis,
    pub grid_color: [f32; 3],
    pub grid_opacity: f32,
    /// Channel display (while a single channel is shown): no lighting on the solo view.
    pub solo_unlit: bool,
    /// HDR values are divided by this before they are shown.
    pub hdr_scale: f32,
    /// Show positive and negative values of grayscale channels in two colors (sRGB).
    pub hdr_signed_colors: bool,
    pub hdr_positive: [f32; 3],
    pub hdr_negative: [f32; 3],
    #[serde(deserialize_with = "component_compat")]
    pub channel_component: ChannelComponent,
    /// Texture filtering of the 3D view.
    #[serde(deserialize_with = "anisotropy_compat")]
    pub anisotropy: Anisotropy,
    /// MipMap bias step 0..3 (see [`MIP_BIAS_LABELS`]).
    pub mip_bias: u32,
    /// Camera frame: outline of a picture of `camera_frame_ratio` (width / height) in the view,
    /// with what is outside dimmed by the gate mask opacity.
    pub camera_frame: bool,
    pub camera_frame_ratio: f32,
    pub gate_opacity: f32,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            env_map: None,
            env_opacity: 0.0,
            env_exposure: 0.0,
            env_rotation: 0.0,
            env_blur: 0.5,
            env_alignment: EnvAlignment::World,
            fov: 40.0,
            antialias: true,
            dof: false,
            dof_amount: 0.5,
            dof_focus: 1.0,
            dof_range: 10.0,
            post_effects: true,
            tone_mapping_on: true,
            tone_map: ToneMap::Aces,
            exposure: 1.0,
            gamma: 1.0,
            mapping_factor: 1.0,
            bloom_on: true,
            bloom: 0.0,
            bloom_threshold: 0.8,
            bloom_falloff: 0.5,
            ssao: 0.0,
            ssao_radius: 3.0,
            emission_glow: 0.35,
            glow_radius: 1.0,
            ssao_on: true,
            glow_on: true,
            glare_on: true,
            glare: 0.0,
            glare_mode: GlareShape::Horizontal,
            glare_length: 0.5,
            glare_threshold: 0.0,
            glare_remap: 1.0,
            flare_on: false,
            flare_camera: FlareCamera::Panoramic,
            flare_amount: 0.5,
            flare_threshold: 0.8,
            flare_ghosts: 4,
            flare_position_scale: 1.0,
            aberration_on: false,
            aberration_amount: 0.5,
            aberration_radius: 0.5,
            aberration_softness: 0.3,
            aberration_ratio: 1.0,
            aberration_center: [0.5, 0.5],
            aberration_achromatic: false,
            vignette_on: false,
            vignette_amount: 0.5,
            sharpen_on: false,
            sharpen_amount: 0.5,
            sharpen_diameter: 2.0,
            grain_on: false,
            grain_amount: 0.2,
            grain_radius: 1.0,
            grain_type: GrainType::Monochromatic,
            color_correction_on: false,
            cc_saturation: 1.0,
            cc_contrast: 1.0,
            cc_brightness: 1.0,
            cc_bias: 0.0,
            cc_sepia: 0.0,
            cc_temperature: 6500.0,
            wireframe: false,
            wire_color: [0.85, 0.88, 0.95],
            wire_opacity: 0.6,
            wire_angle: 30.0,
            grid: true,
            grid_axes: [true, false, true],
            grid_plane: GridAxis::Y,
            grid_color: DEFAULT_GRID_COLOR,
            grid_opacity: DEFAULT_GRID_OPACITY,
            solo_unlit: true,
            hdr_scale: 1.0,
            hdr_signed_colors: false,
            hdr_positive: [0.25, 0.85, 0.30],
            hdr_negative: [0.90, 0.25, 0.20],
            channel_component: ChannelComponent::All,
            anisotropy: Anisotropy::Medium,
            mip_bias: 0,
            camera_frame: false,
            camera_frame_ratio: 16.0 / 9.0,
            gate_opacity: 0.5,
        }
    }
}

impl DisplaySettings {
    /// Put the post effects back to their defaults (the panel's "Reset" of that group); camera
    /// and visualisation settings are kept.
    pub fn reset_post_effects(&mut self) {
        let mut d = Self::default();
        // Everything that is not a post effect stays as the user set it.
        macro_rules! keep {
            ($($field:ident),* $(,)?) => { $( d.$field = std::mem::take(&mut self.$field); )* };
        }
        keep!(
            env_map, env_opacity, env_exposure, env_rotation, env_blur, env_alignment, fov, antialias, wireframe, wire_color, wire_opacity,
            wire_angle, grid, grid_axes, grid_plane, grid_color, grid_opacity, solo_unlit, hdr_scale, hdr_signed_colors, hdr_positive,
            hdr_negative, channel_component, anisotropy, mip_bias, camera_frame, camera_frame_ratio, gate_opacity,
        );
        *self = d;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_object_is_the_default_and_values_round_trip() {
        let d: DisplaySettings = serde_json::from_str("{}").unwrap();
        assert_eq!(d, DisplaySettings::default());
        let edited = DisplaySettings { tone_map: ToneMap::Reinhard, glare_mode: GlareShape::Star, bloom: 0.7, grid_axes: [false, true, false], wire_color: [0.1, 0.2, 0.3], ..Default::default() };
        let text = serde_json::to_string(&edited).unwrap();
        assert!(text.contains(r#""tone_map":"reinhard""#), "enums are written by name: {text}");
        // Shapes written under Painter's names by another build read as the same shape.
        let named: DisplaySettings = serde_json::from_str(r#"{"glare_mode":"sunny_cross"}"#).unwrap();
        assert_eq!(named.glare_mode, GlareShape::Star);
        assert_eq!(serde_json::from_str::<DisplaySettings>(&text).unwrap(), edited);
    }

    #[test]
    fn old_index_values_and_unknown_names_still_load() {
        let d: DisplaySettings = serde_json::from_str(r#"{"tone_map":2,"glare_mode":3,"bloom":0.5}"#).unwrap();
        assert_eq!((d.tone_map, d.glare_mode, d.bloom), (ToneMap::Reinhard, GlareShape::Star, 0.5));
        let d: DisplaySettings = serde_json::from_str(r#"{"tone_map":99,"glare_mode":"from_the_future","exposure":2.0}"#).unwrap();
        assert_eq!((d.tone_map, d.glare_mode, d.exposure), (ToneMap::Aces, GlareShape::Horizontal, 2.0));
    }

    #[test]
    fn focal_length_and_field_of_view_are_inverses_and_ev_is_in_stops() {
        // 24 mm sensor height: a 50 mm lens sees 2 * atan(12 / 50) = 26.99 degrees.
        assert!((fov_from_focal_length(50.0) - 26.991).abs() < 1e-2);
        assert!((focal_length_mm(90.0) - 12.0).abs() < 1e-4, "90 degrees is half the sensor height");
        for fov in [5.0f32, 27.0, 40.0, 75.0, 120.0] {
            assert!((fov_from_focal_length(focal_length_mm(fov)) - fov).abs() < 1e-3);
        }
        assert_eq!((ev_to_multiplier(0.0), ev_to_multiplier(1.0), ev_to_multiplier(-2.0)), (1.0, 2.0, 0.25));
        let d: DisplaySettings = serde_json::from_str(r#"{"env_alignment":"local","env_map":"Studio","fov":55}"#).unwrap();
        assert_eq!((d.env_alignment, d.env_map.as_deref(), d.fov, d.env_opacity), (EnvAlignment::Local, Some("Studio"), 55.0, 0.0));
        assert!(!serde_json::to_string(&DisplaySettings::default()).unwrap().contains("env_map"), "no map chosen: nothing written");
    }

    #[test]
    fn menus_list_every_function_and_shape_once_in_painters_order() {
        use std::collections::HashSet;
        assert_eq!(ToneMap::MENU.iter().collect::<HashSet<_>>(), ToneMap::ALL.iter().collect::<HashSet<_>>());
        assert_eq!(GlareShape::MENU.iter().collect::<HashSet<_>>(), GlareShape::ALL.iter().collect::<HashSet<_>>());
        assert_eq!(ToneMap::MENU.map(|t| t.label())[..8], ["Auto", "Linear", "LinearSat", "Sensitometric", "Reinhard", "ReinhardLum", "Log", "LogLum"]);
        assert_eq!(GlareShape::MENU.map(|s| s.label()), [
            "Bloom", "Lens Flare", "Standard", "Cheap Lens", "After Image", "Filter Cross Screen", "Filter Cross Screen Spectral", "Filter Snow Cross",
            "Filter Snow Cross Spectral", "Filter Sunny Cross", "Filter Sunny Cross Spectral", "Horizontal Streak", "Vertical Streak",
        ]);
        // Old files stored list indices: the first four entries never move.
        assert_eq!(ToneMap::ALL[..4], [ToneMap::Aces, ToneMap::Filmic, ToneMap::Reinhard, ToneMap::Linear]);
        assert_eq!(GlareShape::ALL[..4], [GlareShape::Horizontal, GlareShape::Vertical, GlareShape::Both, GlareShape::Star]);
        assert_eq!(ToneMap::Auto.resolved(), ToneMap::Sensitometric);
        assert!(ToneMap::Log.uses_mapping_factor() && !ToneMap::Sensitometric.uses_mapping_factor() && !ToneMap::Linear.uses_mapping_factor());
    }

    #[test]
    fn visualisation_settings_have_painters_options_and_defaults() {
        let d = DisplaySettings::default();
        assert_eq!((d.grid_plane, d.anisotropy, d.mip_bias, d.channel_component, d.solo_unlit, d.hdr_scale), (GridAxis::Y, Anisotropy::Medium, 0, ChannelComponent::All, true, 1.0));
        assert_eq!(Anisotropy::ALL.map(|a| a.samples()), [1, 2, 4, 8, 16]);
        assert_eq!(Anisotropy::Medium.label(), "Medium (4 spp)");
        assert_eq!(MIP_BIAS_LABELS.len(), 4);
        assert_eq!((ChannelComponent::All.label(true), ChannelComponent::All.label(false)), ("RGBA", "Grayscale + Alpha"));
        assert_eq!(ChannelComponent::ALL.map(|c| c.overlay()), [None, Some("Red"), Some("Green"), Some("Blue"), Some("Alpha")]);
        assert_eq!(GridAxis::ALL.map(|a| a.index()), [0, 1, 2]);
        let read: DisplaySettings = serde_json::from_str(r#"{"grid_plane":"z","anisotropy":"very_high","channel_component":"alpha","mip_bias":3}"#).unwrap();
        assert_eq!((read.grid_plane, read.anisotropy, read.channel_component, read.mip_bias), (GridAxis::Z, Anisotropy::VeryHigh, ChannelComponent::Alpha, 3));
        let odd: DisplaySettings = serde_json::from_str(r#"{"grid_plane":"w","anisotropy":77}"#).unwrap();
        assert_eq!((odd.grid_plane, odd.anisotropy), (GridAxis::Y, Anisotropy::Medium));
    }

    #[test]
    fn resetting_the_post_effects_keeps_the_visualisation_settings() {
        let kept = DisplaySettings {
            wireframe: true, grid: false, antialias: false, env_map: Some("Studio".into()), env_opacity: 0.6, fov: 55.0, grid_plane: GridAxis::Z,
            anisotropy: Anisotropy::High, camera_frame: true, hdr_scale: 4.0, gate_opacity: 0.9, ..Default::default()
        };
        let mut d = DisplaySettings { bloom: 1.0, dof: true, tone_map: ToneMap::Linear, vignette_on: true, cc_sepia: 0.4, post_effects: false, ..kept.clone() };
        d.reset_post_effects();
        assert_eq!(d, kept);
    }
}

#[cfg(test)]
mod lenient_tests {
    use super::*;

    #[derive(Deserialize)]
    struct Holder {
        #[serde(default, deserialize_with = "lenient_display")]
        display: Option<DisplaySettings>,
    }

    /// QA 2026-11: a non-finite exposure is written as `null`, and one such value made the whole
    /// project unopenable ("invalid type: null, expected f32").
    #[test]
    fn one_unreadable_display_value_takes_its_default_and_keeps_the_rest() {
        let h: Holder = serde_json::from_str(r#"{"display": {"exposure": null, "bloom": 0.75, "flare_ghosts": -1, "gamma": "x"}}"#).unwrap();
        let d = h.display.unwrap();
        let default = DisplaySettings::default();
        assert_eq!(d.bloom, 0.75, "readable values are kept");
        assert_eq!((d.exposure, d.flare_ghosts, d.gamma), (default.exposure, default.flare_ghosts, default.gamma));
        let none: Holder = serde_json::from_str("{}").unwrap();
        assert!(none.display.is_none(), "absent stays absent (the user's own defaults)");
        let full = DisplaySettings { bloom: 0.3, exposure: 2.0, ..Default::default() };
        let back: Holder = serde_json::from_str(&format!(r#"{{"display": {}}}"#, serde_json::to_string(&full).unwrap())).unwrap();
        assert_eq!(back.display, Some(full), "a valid block round-trips unchanged");
    }

    /// QA 2026-11: the fallback started from the defaults, which never write `env_map`, so the
    /// chosen environment map was dropped whenever another value had to take its default (and
    /// the next save lost it for good).
    #[test]
    fn the_environment_map_survives_an_unreadable_neighbour() {
        let h: Holder = serde_json::from_str(r#"{"display": {"env_map": "Studio", "exposure": null, "bloom": 0.5}}"#).unwrap();
        let d = h.display.unwrap();
        assert_eq!(d.env_map.as_deref(), Some("Studio"));
        assert_eq!((d.bloom, d.exposure), (0.5, DisplaySettings::default().exposure));
        let bad: Holder = serde_json::from_str(r#"{"display": {"env_map": 7, "exposure": null}}"#).unwrap();
        assert_eq!(bad.display.unwrap().env_map, None, "an unreadable map name is left out, not fatal");
    }
}
