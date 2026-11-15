//! Stitch brushes: the procedural stitch tips of `subcraft_assets::alphas` stamped one diameter
//! apart, turned along the stroke or pen path, with a raised (or, for needle holes, sunken)
//! Height so the stitches read in 3D. They are brush presets (Shelf) and pen path presets
//! (Properties of a stamped path).

use subcraft_core::{ChannelKind, ChannelValue, ResourceId};
use subcraft_doc::{PathKind, PathStamp, PenPath};
use subcraft_paint::BrushSettings;

/// One stitch style.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stitch {
    /// Preset name (Shelf and the pen path's stitch menu).
    pub name: &'static str,
    /// Built-in tip (`subcraft_assets::alphas::builtin`).
    pub tip: &'static str,
    /// Brush size (percent of the model's size).
    pub size: f32,
    /// Distance between stamps in diameters: 1 joins the tips into a line.
    pub spacing: f32,
    /// Every second stamp moves across the line by this fraction of the radius.
    pub alternate: f32,
    pub color: [f32; 3],
    pub roughness: f32,
    /// Raised thread (positive) or sunken holes (negative).
    pub height: f32,
    pub description: &'static str,
}

const THREAD: [f32; 3] = [0.82, 0.76, 0.64];

pub const STITCHES: [Stitch; 5] = [
    Stitch { name: "Stitch: Straight", tip: "Stitch Straight", size: 2.0, spacing: 1.0, alternate: 0.0, color: THREAD, roughness: 0.75, height: 0.5,
        description: "Running stitch: dashes of raised thread along the stroke or path" },
    Stitch { name: "Stitch: Cross", tip: "Stitch Cross", size: 2.0, spacing: 1.0, alternate: 0.0, color: THREAD, roughness: 0.75, height: 0.5,
        description: "Cross stitch: an X of raised thread every diameter" },
    Stitch { name: "Stitch: Zigzag", tip: "Stitch Zigzag", size: 2.0, spacing: 1.0, alternate: 0.0, color: THREAD, roughness: 0.75, height: 0.5,
        description: "Zigzag stitch: one continuous raised thread going from side to side" },
    Stitch { name: "Stitch: Staggered", tip: "Stitch Straight", size: 2.0, spacing: 0.8, alternate: 0.6, color: THREAD, roughness: 0.75, height: 0.5,
        description: "Straight stitches on alternating sides of the line" },
    Stitch { name: "Stitch: Seam Holes", tip: "Stitch Needle Holes", size: 2.0, spacing: 1.0, alternate: 0.0, color: [0.06, 0.05, 0.04], roughness: 0.9, height: -0.4,
        description: "Needle-hole dents between stitches: paint it along the same path as Stitch: Straight for a seam" },
];

/// The stitch named `name`.
pub fn find(name: &str) -> Option<&'static Stitch> {
    STITCHES.iter().find(|s| s.name == name)
}

impl Stitch {
    /// Brush settings of the preset: hard tip, one stamp per `spacing`, turned along the stroke.
    pub fn brush_settings(&self) -> BrushSettings {
        BrushSettings {
            size: self.size,
            hardness: 1.0,
            flow: 1.0,
            spacing: self.spacing,
            follow_path: true,
            alternate_offset: self.alternate,
            pressure_size: 0.0,
            ..BrushSettings::default()
        }
    }

    /// Base Color, Roughness and Height of the thread (or of the holes).
    pub fn channels(&self) -> [(ChannelKind, ChannelValue); 3] {
        let [r, g, b] = self.color;
        [
            (ChannelKind::BaseColor, ChannelValue::rgb(r, g, b)),
            (ChannelKind::Roughness, ChannelValue::scalar(self.roughness)),
            (ChannelKind::Height, ChannelValue::scalar(self.height)),
        ]
    }

    /// Make `path` a stamped paint path of this stitch: `tip` (the project resource of
    /// [`Stitch::tip`]) stamped at the stitch spacing, turned along the curve, with the stitch's
    /// values on the channels the texture set has (`has_channel`). The points and width stay.
    pub fn apply_to_path(&self, path: &mut PenPath, tip: Option<ResourceId>, has_channel: impl Fn(ChannelKind) -> bool) {
        path.kind = PathKind::Paint;
        path.hardness = 1.0;
        path.stamp = Some(PathStamp { alpha: tip, spacing: self.spacing, follow: true, alternate: self.alternate });
        for (kind, value) in self.channels() {
            if has_channel(kind) {
                path.values.insert(kind, value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec2;
    use subcraft_doc::PenAnchor;

    #[test]
    fn stitches_use_existing_tips_and_carry_height() {
        let tips = subcraft_assets::alphas::builtin();
        let mut names: Vec<_> = STITCHES.iter().map(|s| s.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), STITCHES.len(), "unique names");
        for s in &STITCHES {
            assert!(tips.iter().any(|t| t.name == s.tip && t.bitmap.is_some()), "{}: tip {} exists", s.name, s.tip);
            let settings = s.brush_settings();
            assert!(settings.follow_path && settings.hardness == 1.0, "{}", s.name);
            assert!(s.channels().iter().any(|(k, v)| *k == ChannelKind::Height && v.0[0] != 0.0), "{} has relief", s.name);
            assert!(BrushSettings::ALTERNATE_OFFSET.contains(&s.alternate) && PathStamp::SPACING.contains(&s.spacing));
        }
        assert!(find("Stitch: Seam Holes").is_some_and(|s| s.height < 0.0), "needle holes are dents");
        assert!(find("Stitch: Staggered").is_some_and(|s| s.alternate != 0.0));
        // The brush presets list carries them, with their tip and material.
        let presets = crate::brush_preset::builtin();
        for s in &STITCHES {
            let p = presets.iter().find(|p| p.name == s.name).unwrap_or_else(|| panic!("{} in the presets", s.name));
            assert_eq!(p.tip.as_deref(), Some(s.tip));
            assert_eq!(p.settings, s.brush_settings());
            assert!(p.material.channels.contains_key(&ChannelKind::Height));
        }
    }

    #[test]
    fn a_stitch_makes_a_stamped_path_on_the_channels_that_exist() {
        let anchors = vec![PenAnchor::new(Vec2::new(0.1, 0.5)), PenAnchor::new(Vec2::new(0.9, 0.5))];
        let values = [(ChannelKind::BaseColor, ChannelValue::rgb(1.0, 0.0, 0.0))].into_iter().collect();
        let mut path = PenPath { anchors, closed: false, radius: 0.02, hardness: 0.3, opacity: 1.0, values, kind: PathKind::Ribbon, stamp: None, ribbon: None };
        let tip = ResourceId::new();
        let staggered = find("Stitch: Staggered").unwrap();
        staggered.apply_to_path(&mut path, Some(tip), |k| k != ChannelKind::Roughness);
        assert_eq!(path.kind, PathKind::Paint);
        assert_eq!(path.stamp, Some(PathStamp { alpha: Some(tip), spacing: 0.8, follow: true, alternate: 0.6 }));
        assert_eq!(path.values[&ChannelKind::Height], ChannelValue::scalar(0.5));
        assert_eq!(path.values[&ChannelKind::BaseColor], ChannelValue::rgb(0.82, 0.76, 0.64));
        assert!(!path.values.contains_key(&ChannelKind::Roughness), "a channel the texture set lacks is left out");
        assert!(path.validate().is_ok(), "{:?}", path.validate());
    }
}
