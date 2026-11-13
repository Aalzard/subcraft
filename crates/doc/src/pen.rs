//! Editable cubic paths in canonical texture UV space. Paint channels cache their raster result;
//! anchors, relative handles and style are the persistent source of truth.

use std::collections::{BTreeMap, BTreeSet};
use glam::Vec2;
use subcraft_core::{AnchorId, ChannelKind, ChannelValue};

#[derive(Clone, Debug, PartialEq)]
pub struct PenAnchor {
    pub id: AnchorId,
    pub position: Vec2,
    /// Relative to the anchor, so moving an anchor preserves both tangents.
    pub incoming: Vec2,
    pub outgoing: Vec2,
}

impl PenAnchor {
    pub fn new(position: Vec2) -> Self {
        Self { id: AnchorId::new(), position, incoming: Vec2::ZERO, outgoing: Vec2::ZERO }
    }
}

/// What a path paints (Painter's path types).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathKind {
    /// A stroke of the path's width along the curve.
    #[default]
    Paint,
    /// The interior of the path (always closed), filled with the path's values; width and
    /// hardness are not used.
    Filled,
    /// An image laid along the curve over the path's width ([`PenPath::ribbon`]).
    Ribbon,
}

impl PathKind {
    pub const ALL: [PathKind; 3] = [PathKind::Paint, PathKind::Filled, PathKind::Ribbon];
    pub fn label(self) -> &'static str {
        match self {
            PathKind::Paint => "Paint along path",
            PathKind::Filled => "Filled path",
            PathKind::Ribbon => "Ribbon path",
        }
    }
}

/// How a ribbon's image runs along the path (Painter's Stretching & Tiling mode).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RibbonMode {
    /// `tiles` copies of the image over the whole length.
    #[default]
    Stretch,
    /// Copies keep the image's aspect ratio at the path's width (scaled by `tiles`).
    KeepAspect,
}

impl RibbonMode {
    pub const ALL: [RibbonMode; 2] = [RibbonMode::Stretch, RibbonMode::KeepAspect];
    pub fn label(self) -> &'static str {
        match self {
            RibbonMode::Stretch => "Stretch along path",
            RibbonMode::KeepAspect => "Keep aspect ratio",
        }
    }
}

/// Ribbon path settings: the image laid along the curve and how it repeats.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PathRibbon {
    /// The image (a project resource); `None` is a solid strip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<subcraft_core::ResourceId>,
    pub mode: RibbonMode,
    /// Stretch: copies over the length. Keep aspect ratio: aspect-ratio factor of each copy.
    pub tiles: f32,
    /// Mirror every second copy along the path.
    pub mirror: bool,
    /// Flip the image across the path.
    pub flip: bool,
    /// Base Color takes the image's colour (otherwise the path's value tinted by nothing).
    pub color_from_image: bool,
}

impl PathRibbon {
    pub const TILES: std::ops::RangeInclusive<f32> = 0.05..=256.0;
}

impl Default for PathRibbon {
    fn default() -> Self {
        Self { image: None, mode: RibbonMode::Stretch, tiles: 1.0, mirror: false, flip: false, color_from_image: true }
    }
}

/// Paint along path with a brush tip: stamps of the tip placed along the curve (Painter's
/// brush, alpha and spacing on a path). Without it a path is one continuous stroke.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PathStamp {
    /// Tip image (an Alpha resource of the project); `None` is the round tip with the path's hardness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpha: Option<subcraft_core::ResourceId>,
    /// Distance between stamps as a fraction of the stamp diameter.
    pub spacing: f32,
    /// Turn each stamp along the curve (Follow path); otherwise the tip stays upright.
    pub follow: bool,
}

impl PathStamp {
    pub const SPACING: std::ops::RangeInclusive<f32> = 0.02..=4.0;
}

impl Default for PathStamp {
    fn default() -> Self {
        Self { alpha: None, spacing: 0.25, follow: true }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PenPath {
    pub anchors: Vec<PenAnchor>,
    pub closed: bool,
    /// Radius as a fraction of texture width (independent of resolution).
    pub radius: f32,
    pub hardness: f32,
    pub opacity: f32,
    /// Linear color / encoded data-channel values. Absent channels remain transparent.
    pub values: BTreeMap<ChannelKind, ChannelValue>,
    /// Stroke along the curve, or its filled interior.
    pub kind: PathKind,
    /// Brush tip stamped along the curve (Paint along path only).
    pub stamp: Option<PathStamp>,
    /// Ribbon settings (Ribbon path only; `None` uses the defaults).
    pub ribbon: Option<PathRibbon>,
}

impl PenPath {
    pub const MAX_ANCHORS: usize = 256;

    /// The image resource the path draws with: the stamped tip, or the ribbon image.
    pub fn image_resource(&self) -> Option<subcraft_core::ResourceId> {
        match self.kind {
            PathKind::Paint => self.stamp.and_then(|s| s.alpha),
            PathKind::Ribbon => self.ribbon.and_then(|r| r.image),
            PathKind::Filled => None,
        }
    }

    /// Whether the curve returns to its first anchor (a filled path always does).
    pub fn is_closed(&self) -> bool {
        self.closed || self.kind == PathKind::Filled
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.anchors.len() > Self::MAX_ANCHORS { return Err("a path supports at most 256 anchors"); }
        if !self.radius.is_finite() || !(0.00001..=0.5).contains(&self.radius)
            || !self.hardness.is_finite() || !(0.0..=1.0).contains(&self.hardness)
            || !self.opacity.is_finite() || !(0.0..=1.0).contains(&self.opacity)
        { return Err("invalid pen width, hardness or opacity"); }
        if self.stamp.is_some_and(|s| !s.spacing.is_finite() || !PathStamp::SPACING.contains(&s.spacing)) { return Err("pen stamp spacing must be between 2 % and 400 % of the tip"); }
        if self.ribbon.is_some_and(|r| !r.tiles.is_finite() || !PathRibbon::TILES.contains(&r.tiles)) { return Err("ribbon tiling must be between 0.05 and 256"); }
        if self.values.is_empty() || self.values.values().any(|v| !v.0.iter().all(|x| x.is_finite() && x.abs() <= 65504.0)) { return Err("pen channels must contain finite, representable values"); }
        let mut ids = BTreeSet::new();
        for a in &self.anchors {
            if a.id == AnchorId::nil() || !ids.insert(a.id) { return Err("pen anchor UUIDs must be unique and nonzero"); }
            for p in [a.position, a.incoming, a.outgoing] {
                if !p.is_finite() || p.abs().max_element() > 4.0 { return Err("pen coordinates must be finite and within four UV tiles"); }
            }
        }
        Ok(())
    }

    /// Resolution-aware adaptive subdivision. Bounded depth and segment count prevent an
    /// untrusted project or extreme handle positions from creating unbounded work.
    pub fn polyline(&self, tolerance: f32) -> Result<Vec<Vec2>, &'static str> {
        self.validate()?;
        let mut out = Vec::new();
        let Some(first) = self.anchors.first() else { return Ok(out); };
        out.push(first.position);
        fn curve(p: [Vec2; 4], tolerance: f32, depth: u32, out: &mut Vec<Vec2>) -> Result<(), &'static str> {
            if out.len() >= 8192 { return Err("path is too complex at this resolution"); }
            let d = p[3] - p[0];
            let distance = |q: Vec2| {
                let t = if d.length_squared() > 1e-12 { ((q - p[0]).dot(d) / d.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
                q.distance(p[0] + d * t)
            };
            if depth >= 12 || distance(p[1]).max(distance(p[2])) <= tolerance {
                out.push(p[3]); return Ok(());
            }
            let a = (p[0] + p[1]) * 0.5; let b = (p[1] + p[2]) * 0.5; let c = (p[2] + p[3]) * 0.5;
            let d = (a + b) * 0.5; let e = (b + c) * 0.5; let f = (d + e) * 0.5;
            curve([p[0], a, d, f], tolerance, depth + 1, out)?;
            curve([f, e, c, p[3]], tolerance, depth + 1, out)
        }
        let count = self.anchors.len();
        let segments = count.saturating_sub(1) + usize::from(self.is_closed() && count > 1);
        for i in 0..segments {
            let a = &self.anchors[i % count]; let b = &self.anchors[(i + 1) % count];
            curve([a.position, a.position + a.outgoing, b.position + b.incoming, b.position], tolerance.max(1e-6), 0, &mut out)?;
        }
        Ok(out)
    }
}
