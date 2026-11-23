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

pub const STITCHES: [Stitch; 14] = [
    Stitch { name: "Stitch: Straight", tip: "Stitch Straight", size: 2.0, spacing: 1.0, alternate: 0.0, color: THREAD, roughness: 0.75, height: 0.5,
        description: "Running stitch: dashes of raised thread along the stroke or path" },
    Stitch { name: "Stitch: Cross", tip: "Stitch Cross", size: 2.0, spacing: 1.0, alternate: 0.0, color: THREAD, roughness: 0.75, height: 0.5,
        description: "Cross stitch: an X of raised thread every diameter" },
    Stitch { name: "Stitch: Zigzag", tip: "Stitch Zigzag", size: 2.0, spacing: 1.0, alternate: 0.0, color: THREAD, roughness: 0.75, height: 0.5,
        description: "Zigzag stitch: one continuous raised thread going from side to side" },
    Stitch { name: "Stitch: Staggered", tip: "Stitch Straight", size: 2.0, spacing: 0.8, alternate: 0.6, color: THREAD, roughness: 0.75, height: 0.5,
        description: "Straight stitches on alternating sides of the line" },
    Stitch { name: "Stitch: Saddle", tip: "Stitch Saddle", size: 2.5, spacing: 1.0, alternate: 0.0, color: THREAD, roughness: 0.75, height: 0.6,
        description: "Saddle stitch: thick slanted stitches, as on hand-sewn leather" },
    Stitch { name: "Stitch: Double", tip: "Stitch Double", size: 4.0, spacing: 1.0, alternate: 0.0, color: [0.80, 0.55, 0.18], roughness: 0.75, height: 0.5,
        description: "Double row: two parallel lines of stitches, like the topstitching on jeans" },
    Stitch { name: "Stitch: Whip", tip: "Stitch Whip", size: 3.0, spacing: 0.45, alternate: 0.0, color: THREAD, roughness: 0.75, height: 0.6,
        description: "Whip stitch: slanted threads close together that wrap over an edge or join two pieces" },
    Stitch { name: "Stitch: Blanket", tip: "Stitch Blanket", size: 3.5, spacing: 1.0, alternate: 0.0, color: THREAD, roughness: 0.75, height: 0.5,
        description: "Blanket stitch: a thread along the edge with a leg into the cloth at every stitch" },
    Stitch { name: "Stitch: Chain", tip: "Stitch Chain", size: 2.5, spacing: 1.0, alternate: 0.0, color: THREAD, roughness: 0.75, height: 0.5,
        description: "Chain stitch: a row of loops, each one held by the next (embroidery, jeans hems)" },
    Stitch { name: "Stitch: Herringbone", tip: "Stitch Herringbone", size: 4.0, spacing: 1.0, alternate: 0.0, color: THREAD, roughness: 0.75, height: 0.5,
        description: "Herringbone: long slanted threads that cross near both edges" },
    Stitch { name: "Stitch: Baseball", tip: "Stitch Baseball", size: 4.0, spacing: 0.6, alternate: 0.0, color: [0.72, 0.08, 0.06], roughness: 0.7, height: 0.6,
        description: "Baseball stitch: V shaped pairs of threads over a seam, as on a ball or a laced grip" },
    Stitch { name: "Stitch: Triple", tip: "Stitch Triple", size: 5.0, spacing: 1.0, alternate: 0.0, color: [0.80, 0.55, 0.18], roughness: 0.75, height: 0.5,
        description: "Triple row: three parallel lines of stitches, as on workwear seams" },
    Stitch { name: "Stitch: Cord", tip: "Stitch Cord", size: 2.5, spacing: 0.98, alternate: 0.0, color: THREAD, roughness: 0.8, height: 0.8,
        description: "Cord: one continuous twisted rope along the path (laces, piping, drawstrings)" },
    Stitch { name: "Stitch: Seam Holes", tip: "Stitch Needle Holes", size: 2.0, spacing: 1.0, alternate: 0.0, color: [0.06, 0.05, 0.04], roughness: 0.9, height: -0.4,
        description: "Needle-hole dents between stitches: paint it along the same path as Stitch: Straight for a seam" },
];

/// The stitch named `name`.
pub fn find(name: &str) -> Option<&'static Stitch> {
    STITCHES.iter().find(|s| s.name == name)
}

/// The pen style of `stitch` for new paths: its tip (from the built-in `tips`) as a resource of
/// the project, added when the project has no copy of it yet.
pub fn style(session: &mut crate::Session, stitch: &'static Stitch, tips: &[subcraft_assets::alphas::BrushAlpha]) -> crate::pen::PenStyle {
    let bitmap = tips.iter().find(|a| a.name == stitch.tip).and_then(|a| a.bitmap.clone());
    crate::pen::PenStyle { stitch, tip: bitmap.map(|b| session.tip_resource(b, stitch.tip)) }
}

/// The stitch `path` is drawn with: a stamped path of a stitch tip (`tip_name` is the name of
/// its tip resource). Straight and Staggered share a tip and differ by the alternate offset.
pub fn of_path(path: &PenPath, tip_name: Option<&str>) -> Option<&'static Stitch> {
    let stamp = path.stamp.filter(|_| path.kind == PathKind::Paint)?;
    let tip = tip_name?;
    STITCHES.iter().find(|s| s.tip == tip && (s.alternate != 0.0) == (stamp.alternate != 0.0))
}

/// Needle holes fit this stitch: one thread per stamp along the line, with a gap between two
/// stitches where a hole goes.
pub fn takes_holes(stitch: &Stitch) -> bool {
    matches!(stitch.tip, "Stitch Straight" | "Stitch Saddle") && stitch.alternate == 0.0
}

/// The needle holes of the stitched `path`: a path on the same points (with ids of their own)
/// at the same width and spacing, stamped with the holes of `holes` (the style of
/// Stitch: Seam Holes). Its stamps sit where the stitches do, and the holes tip has half a hole
/// on each side, so every hole lands in the gap between two stitches.
pub fn holes_path(path: &PenPath, holes: crate::pen::PenStyle, has_channel: impl Fn(ChannelKind) -> bool) -> PenPath {
    let mut out = path.clone();
    for a in &mut out.anchors {
        a.id = subcraft_core::AnchorId::new();
    }
    let spacing = path.stamp.map_or(holes.stitch.spacing, |s| s.spacing);
    let thread = std::mem::take(&mut out.values);
    holes.stitch.apply_to_path(&mut out, holes.tip, has_channel);
    if out.values.is_empty() {
        out.values = thread;
    }
    if let Some(stamp) = out.stamp.as_mut() {
        stamp.spacing = spacing;
        stamp.alternate = 0.0;
    }
    out.opacity = 1.0;
    out
}

/// How deep the seam groove presses the surface in (Height).
pub const GROOVE_DEPTH: f32 = -0.3;

/// The pressed seam under the stitched `path`: a path on the same points (with ids of their
/// own) at the same width, drawn as a soft continuous line on the surface. It changes Height
/// only, so the thread keeps its colour and sits in a shallow valley. `None` when the texture
/// set has no Height channel.
pub fn groove_path(path: &PenPath, has_channel: impl Fn(ChannelKind) -> bool) -> Option<PenPath> {
    if !has_channel(ChannelKind::Height) {
        return None;
    }
    let mut out = path.clone();
    for a in &mut out.anchors {
        a.id = subcraft_core::AnchorId::new();
    }
    out.kind = PathKind::Paint;
    out.hardness = 0.0;
    out.opacity = 1.0;
    // Round soft stamps close together: one smooth line that crosses UV seams like the stitches.
    out.stamp = Some(PathStamp { alpha: None, spacing: 0.05, follow: false, alternate: 0.0, surface: true });
    out.values = [(ChannelKind::Height, ChannelValue::scalar(GROOVE_DEPTH))].into_iter().collect();
    Some(out)
}

impl Stitch {
    /// Half the width of a new path of this stitch, in UV units (as [`PenPath::radius`]).
    pub fn radius(&self) -> f32 {
        (self.size * 0.005).clamp(0.00001, 0.5)
    }

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
        path.stamp = Some(PathStamp { alpha: tip, spacing: self.spacing, follow: true, alternate: self.alternate, surface: true });
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
        assert!(STITCHES.len() >= 14);
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
    fn every_stitch_is_recognised_on_the_path_it_made() {
        let anchors = vec![PenAnchor::new(Vec2::new(0.1, 0.5)), PenAnchor::new(Vec2::new(0.9, 0.5))];
        let plain = PenPath { anchors, closed: false, radius: 0.02, hardness: 0.3, opacity: 1.0, values: Default::default(), kind: PathKind::Paint, stamp: None, ribbon: None };
        assert_eq!(of_path(&plain, None), None, "a plain line is no stitch");
        for s in &STITCHES {
            let mut path = plain.clone();
            s.apply_to_path(&mut path, Some(ResourceId::new()), |_| true);
            assert_eq!(of_path(&path, Some(s.tip)).map(|f| f.name), Some(s.name));
            assert_eq!(of_path(&path, Some("Grunge")), None, "{}: another tip is not a stitch", s.name);
            path.kind = PathKind::Ribbon;
            assert_eq!(of_path(&path, Some(s.tip)), None, "only stamped paint paths");
            assert!((0.001..=0.05).contains(&s.radius()), "{}: a width that reads as a stitch", s.name);
        }
    }

    #[test]
    fn the_seam_groove_of_a_stitched_path_is_height_only_on_the_same_points() {
        let anchors = vec![PenAnchor::new(Vec2::new(0.1, 0.5)), PenAnchor::new(Vec2::new(0.9, 0.5))];
        let mut path = PenPath { anchors, closed: false, radius: 0.02, hardness: 0.3, opacity: 0.5, values: Default::default(), kind: PathKind::Paint, stamp: None, ribbon: None };
        find("Stitch: Saddle").unwrap().apply_to_path(&mut path, Some(ResourceId::new()), |_| true);
        assert_eq!(groove_path(&path, |k| k != ChannelKind::Height), None, "no Height channel, no groove");
        let groove = groove_path(&path, |_| true).unwrap();
        assert_eq!(groove.values.keys().copied().collect::<Vec<_>>(), vec![ChannelKind::Height]);
        assert_eq!(groove.values[&ChannelKind::Height], ChannelValue::scalar(GROOVE_DEPTH));
        let stamp = groove.stamp.unwrap();
        assert_eq!((groove.radius, groove.opacity, groove.hardness, stamp.alpha, stamp.surface), (path.radius, 1.0, 0.0, None, true));
        assert!(groove.anchors.iter().zip(&path.anchors).all(|(a, b)| a.id != b.id && a.position == b.position));
        assert!(groove.validate().is_ok(), "{:?}", groove.validate());
        assert_eq!(of_path(&groove, None), None, "the groove is a plain line, not a stitch");
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
        assert_eq!(path.stamp, Some(PathStamp { alpha: Some(tip), spacing: 0.8, follow: true, alternate: 0.6, surface: true }));
        assert_eq!(path.values[&ChannelKind::Height], ChannelValue::scalar(0.5));
        assert_eq!(path.values[&ChannelKind::BaseColor], ChannelValue::rgb(0.82, 0.76, 0.64));
        assert!(!path.values.contains_key(&ChannelKind::Roughness), "a channel the texture set lacks is left out");
        assert!(path.validate().is_ok(), "{:?}", path.validate());
    }
}
