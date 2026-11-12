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

/// How HDR light is mapped to the display.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToneMap {
    /// Film-like curve with soft highlights (the original look).
    #[default]
    Aces,
    /// Hable's filmic curve: punchier contrast, warm highlights.
    Filmic,
    /// Simple Reinhard: gentle, a little flat.
    Reinhard,
    /// No curve: values above 1 clip. Useful to judge raw material colors.
    Linear,
}

impl ToneMap {
    /// Storage order of old preference files (an index); new entries are only ever appended.
    pub const ALL: [ToneMap; 4] = [ToneMap::Aces, ToneMap::Filmic, ToneMap::Reinhard, ToneMap::Linear];

    pub fn label(self) -> &'static str {
        match self {
            ToneMap::Aces => "ACES",
            ToneMap::Filmic => "Filmic",
            ToneMap::Reinhard => "Reinhard",
            ToneMap::Linear => "Linear (clip)",
        }
    }
}

/// Direction of the streaks that bright emission throws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlareShape {
    #[default]
    Horizontal,
    Vertical,
    Both,
    /// Four streaks (horizontal, vertical and both diagonals): a starburst.
    Star,
}

impl GlareShape {
    /// Storage order of old preference files (an index); new entries are only ever appended.
    pub const ALL: [GlareShape; 4] = [GlareShape::Horizontal, GlareShape::Vertical, GlareShape::Both, GlareShape::Star];

    pub fn label(self) -> &'static str {
        match self {
            GlareShape::Horizontal => "Horizontal",
            GlareShape::Vertical => "Vertical",
            GlareShape::Both => "Horizontal + vertical",
            GlareShape::Star => "Starburst",
        }
    }
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

fn tone_map_compat<'de, D: Deserializer<'de>>(d: D) -> Result<ToneMap, D::Error> {
    compat(d, &ToneMap::ALL)
}

fn glare_compat<'de, D: Deserializer<'de>>(d: D) -> Result<GlareShape, D::Error> {
    compat(d, &GlareShape::ALL)
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
    #[serde(deserialize_with = "tone_map_compat")]
    pub tone_map: ToneMap,
    /// Brightness multiplier before tone mapping.
    pub exposure: f32,
    pub bloom: f32,
    pub bloom_threshold: f32,
    pub ssao: f32,
    /// Ambient-occlusion reach in percent of the model's size.
    pub ssao_radius: f32,
    /// Glow around emissive areas (0 = off) and its reach multiplier.
    pub emission_glow: f32,
    pub glow_radius: f32,
    /// Streaks from bright emission (0 = off), their shape and length 0..1.
    pub glare: f32,
    #[serde(deserialize_with = "glare_compat")]
    pub glare_mode: GlareShape,
    pub glare_length: f32,

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
            tone_map: ToneMap::Aces,
            exposure: 1.0,
            bloom: 0.0,
            bloom_threshold: 0.8,
            ssao: 0.0,
            ssao_radius: 3.0,
            emission_glow: 0.35,
            glow_radius: 1.0,
            glare: 0.0,
            glare_mode: GlareShape::Horizontal,
            glare_length: 0.5,
            wireframe: false,
            wire_color: [0.85, 0.88, 0.95],
            wire_opacity: 0.6,
            wire_angle: 30.0,
            grid: true,
            grid_axes: [true, false, true],
        }
    }
}

impl DisplaySettings {
    /// Put the post effects back to their defaults (the panel's "Reset" of that group); camera
    /// and visualisation settings are kept.
    pub fn reset_post_effects(&mut self) {
        let d = Self::default();
        *self = Self {
            env_map: self.env_map.take(),
            env_opacity: self.env_opacity,
            env_exposure: self.env_exposure,
            env_rotation: self.env_rotation,
            env_blur: self.env_blur,
            env_alignment: self.env_alignment,
            fov: self.fov,
            antialias: self.antialias,
            wireframe: self.wireframe,
            wire_color: self.wire_color,
            wire_opacity: self.wire_opacity,
            wire_angle: self.wire_angle,
            grid: self.grid,
            grid_axes: self.grid_axes,
            ..d
        };
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
    fn resetting_the_post_effects_keeps_the_visualisation_settings() {
        let mut d = DisplaySettings { bloom: 1.0, dof: true, tone_map: ToneMap::Linear, wireframe: true, grid: false, antialias: false, ..Default::default() };
        d.reset_post_effects();
        assert_eq!(d, DisplaySettings { wireframe: true, grid: false, antialias: false, ..Default::default() });
    }
}
