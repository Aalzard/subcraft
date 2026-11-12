//! Content effects: an ordered, non-destructive stack applied to a layer's own content.
//!
//! The stack runs bottom to top on top of the layer's base content (its paint rasters or fill
//! material), before the layer's mask, blend and opacity. The base content is never modified by
//! an effect, so removing, disabling or reordering effects restores the earlier result exactly.
//! The mask has its own, separate stack ([`crate::MaskEffect`]); see ADR-0017.

use std::collections::BTreeMap;

use subcraft_core::{ChannelKind, EffectId};
use subcraft_image::TiledImage;

use crate::blend::BlendMode;
use crate::layer::{ChannelBlend, FillContent, empty_paint_image};
use crate::mask::Levels;
use crate::texture_set::TextureSet;

#[derive(Clone, Debug, PartialEq)]
pub enum ContentEffectKind {
    /// Additional paint rasters per channel (`components + 1` values: value…, alpha). Strokes go
    /// here when the effect is selected, so even fill layers can carry painted detail.
    Paint { channels: BTreeMap<ChannelKind, TiledImage> },
    /// A material fill (every enabled channel) through its own projection.
    Fill(FillContent),
    /// Remaps one channel of the content below it. Adjusts values only: coverage is unchanged,
    /// so transparent paint stays transparent. Not valid on the Normal channel (vector data).
    Levels { channel: ChannelKind, levels: Levels },
    Filter { channel: ChannelKind, filter: crate::Filter },
    /// Anchor point (ADR-0045): captures the content evaluated up to here, per channel, under
    /// the effect's name, for references elsewhere. Changes nothing itself.
    Anchor,
}

impl ContentEffectKind {
    pub fn label(&self) -> String {
        match self {
            ContentEffectKind::Paint { .. } => "Paint".into(),
            ContentEffectKind::Fill(_) => "Fill".into(),
            ContentEffectKind::Levels { channel, .. } => format!("Levels ({})", channel.display_name()),
            ContentEffectKind::Filter { channel, filter } => format!("{} ({})", filter.label(), channel.display_name()),
            ContentEffectKind::Anchor => "Anchor Point".into(),
        }
    }

    /// Adjustments transform the content below instead of adding new content on top of it.
    pub fn is_adjustment(&self) -> bool {
        matches!(self, ContentEffectKind::Levels { .. } | ContentEffectKind::Filter { .. })
    }

    /// Paint image for a channel, creating an empty (transparent) one on first use.
    pub fn paint_channel_mut(&mut self, kind: ChannelKind, resolution: u32) -> Option<&mut TiledImage> {
        match self {
            ContentEffectKind::Paint { channels } => Some(channels.entry(kind).or_insert_with(|| empty_paint_image(kind, resolution))),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContentEffect {
    pub id: EffectId,
    pub name: String,
    pub visible: bool,
    /// Per-channel blend of this effect onto the content below it in the stack.
    pub blend: BTreeMap<ChannelKind, ChannelBlend>,
    pub kind: ContentEffectKind,
    /// Filters only: further channels the filter processes besides its own `channel` (Painter's
    /// per-channel toggles). Channels the filter cannot process are ignored.
    pub extra_channels: std::collections::BTreeSet<ChannelKind>,
}

impl ContentEffect {
    /// New effect blending every texture-set channel normally (normals combine).
    pub fn new(kind: ContentEffectKind, ts: &TextureSet) -> Self {
        let blend = ts
            .channels
            .iter()
            .map(|c| {
                let mode = if c.kind == ChannelKind::Normal && !kind.is_adjustment() { BlendMode::NormalMapCombine } else { BlendMode::Normal };
                (c.kind, ChannelBlend::new(mode))
            })
            .collect();
        Self { id: EffectId::new(), name: kind.label(), visible: true, blend, kind, extra_channels: Default::default() }
    }

    /// Equality ignoring paint texels (dirty-region diffing: painted tiles are diffed instead).
    pub fn same_parameters(&self, other: &ContentEffect) -> bool {
        let kinds = match (&self.kind, &other.kind) {
            (ContentEffectKind::Paint { channels: a }, ContentEffectKind::Paint { channels: b }) => crate::layer::same_paint_layout(a, b),
            (a, b) => a == b,
        };
        kinds && self.id == other.id && self.visible == other.visible && self.blend == other.blend && self.extra_channels == other.extra_channels
    }

    /// Whether this effect works on `kind`: a filter on its own channel and the extra ones it
    /// supports, Levels on its one channel, anything else on every channel.
    pub fn applies_to(&self, kind: ChannelKind) -> bool {
        match &self.kind {
            ContentEffectKind::Filter { channel, filter } => {
                (*channel == kind || self.extra_channels.contains(&kind)) && filter.supports_channel(kind)
            }
            ContentEffectKind::Levels { channel, .. } => *channel == kind,
            _ => true,
        }
    }

    /// The channels a filter effect processes (its own first), filtered by what it supports.
    pub fn filter_channels(&self) -> Vec<ChannelKind> {
        let ContentEffectKind::Filter { channel, filter } = &self.kind else { return Vec::new() };
        std::iter::once(*channel)
            .chain(self.extra_channels.iter().copied().filter(|k| k != channel))
            .filter(|k| filter.supports_channel(*k))
            .collect()
    }

    pub fn blend_for(&self, kind: ChannelKind) -> ChannelBlend {
        self.blend.get(&kind).copied().unwrap_or(ChannelBlend::new(BlendMode::Disable))
    }

    /// Resample every raster to `resolution` (paste / texture-set resolution change).
    pub fn resampled(&self, resolution: u32) -> Self {
        let mut out = self.clone();
        if let ContentEffectKind::Paint { channels } = &mut out.kind {
            for image in channels.values_mut() {
                *image = image.resampled(resolution, resolution);
            }
        }
        out
    }
}
