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
    // ----- camera ------------------------------------------------------------------------------
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
    fn resetting_the_post_effects_keeps_the_visualisation_settings() {
        let mut d = DisplaySettings { bloom: 1.0, dof: true, tone_map: ToneMap::Linear, wireframe: true, grid: false, antialias: false, ..Default::default() };
        d.reset_post_effects();
        assert_eq!(d, DisplaySettings { wireframe: true, grid: false, antialias: false, ..Default::default() });
    }
}
