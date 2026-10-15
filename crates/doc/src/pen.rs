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
}

impl PenPath {
    pub const MAX_ANCHORS: usize = 256;

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.anchors.len() > Self::MAX_ANCHORS { return Err("a path supports at most 256 anchors"); }
        if !self.radius.is_finite() || !(0.00001..=0.5).contains(&self.radius)
            || !self.hardness.is_finite() || !(0.0..=1.0).contains(&self.hardness)
            || !self.opacity.is_finite() || !(0.0..=1.0).contains(&self.opacity)
        { return Err("invalid pen width, hardness or opacity"); }
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
        let segments = count.saturating_sub(1) + usize::from(self.closed && count > 1);
        for i in 0..segments {
            let a = &self.anchors[i % count]; let b = &self.anchors[(i + 1) % count];
            curve([a.position, a.position + a.outgoing, b.position + b.incoming, b.position], tolerance.max(1e-6), 0, &mut out)?;
        }
        Ok(out)
    }
}
