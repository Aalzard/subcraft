//! The normal-map (and height) stamp brush: choosing the image, the brush-state bookkeeping and
//! the "turn on the layer's Normal channel on the first stroke" rule. Tool state only: nothing
//! here changes the document until a stroke starts.

use std::sync::Arc;

use subcraft_core::{ChannelKind, ResourceId};
use subcraft_doc::{BlendMode, ChannelBlend, ChannelDef, LayerContent};
use subcraft_image::Bitmap;
use subcraft_paint::{BrushChannel, ImageStamp, StampKind};

use crate::session::Session;

/// Spacing (in brush diameters) and edge hardness a stamp brush starts with: one stamp per
/// diameter with a soft rim, so a click places a single screw / nail.
pub const STAMP_SPACING: f32 = 1.0;
pub const STAMP_HARDNESS: f32 = 0.85;

impl Session {
    /// Make a decoded image the active stamp. Normal maps paint the Normal channel, height /
    /// grayscale images the Height channel. The brush keeps only that channel switched on (the
    /// previous selection is restored by [`Session::clear_brush_stamp`]); other channels the user
    /// switches on afterwards paint flat values under the stamp's footprint. Identical images
    /// share one resource.
    pub fn set_brush_stamp(&mut self, name: &str, kind: StampKind, bitmap: Bitmap) -> Result<(), String> {
        let name = name.strip_prefix("Starter: ").unwrap_or(name);
        let (encoded, ext) = subcraft_image::encode_bitmap(&bitmap).map_err(|e| e.to_string())?;
        let existing = self
            .resources()
            .iter()
            .find(|r| r.kind == subcraft_doc::ResourceKind::Texture && r.encoded.as_deref().is_some_and(|e| *e == encoded))
            .map(|r| r.id);
        let id = existing.unwrap_or_else(ResourceId::new);
        if existing.is_none() {
            self.resources_mut().insert(subcraft_doc::Resource {
                id,
                name: format!("{name}.{ext}"),
                kind: subcraft_doc::ResourceKind::Texture,
                bitmap: Arc::new(bitmap),
                source_path: None,
                encoded: Some(Arc::new(encoded)),
            });
        }
        self.activate_stamp(name, kind, id);
        Ok(())
    }

    /// Activate stamp `resource` (already in the store) with fresh strength / flip settings.
    pub fn activate_stamp(&mut self, name: &str, kind: StampKind, resource: ResourceId) {
        let previous = self.brush.material.stamp.take();
        let mut stamp = ImageStamp { kind, resource: Some(resource), name: name.to_string(), tiled: previous.as_ref().is_some_and(|p| p.tiled), ..ImageStamp::default() };
        let settings = &mut self.brush.settings;
        match previous {
            // Switching stamps keeps what to restore.
            Some(p) => {
                stamp.restore_channels = p.restore_channels;
                stamp.restore_settings = p.restore_settings;
            }
            None => {
                stamp.restore_channels = self.brush.material.channels.iter().filter(|(_, c)| c.enabled).map(|(k, _)| *k).collect();
                stamp.restore_settings = Some(*settings);
                settings.spacing = STAMP_SPACING;
                settings.hardness = STAMP_HARDNESS;
                settings.follow_path = false;
            }
        }
        let channel = kind.channel();
        for c in self.brush.material.channels.values_mut() {
            c.enabled = false;
        }
        self.brush.material.channels.entry(channel).or_insert(BrushChannel { enabled: false, value: channel.default_paint_value() }).enabled = true;
        self.brush.material.stamp = Some(stamp);
        self.brush.mode = subcraft_paint::StrokeMode::Paint;
    }

    /// Back to an ordinary brush: restores the channels and spacing / edge settings from before.
    pub fn clear_brush_stamp(&mut self) {
        let Some(stamp) = self.brush.material.stamp.take() else { return };
        for (kind, c) in self.brush.material.channels.iter_mut() {
            c.enabled = stamp.restore_channels.contains(kind);
        }
        for kind in &stamp.restore_channels {
            self.brush.material.channels.entry(*kind).or_insert(BrushChannel { enabled: true, value: kind.default_paint_value() });
        }
        if let Some(s) = stamp.restore_settings {
            self.brush.settings.spacing = s.spacing;
            self.brush.settings.hardness = s.hardness;
            self.brush.settings.follow_path = s.follow_path;
        }
    }

    /// The active stamp and its decoded image (`None` for an ordinary brush or a missing image).
    pub fn brush_stamp(&self) -> Option<(&ImageStamp, Arc<Bitmap>)> {
        let stamp = self.brush.material.stamp.as_ref()?;
        let bitmap = self.resources().bitmap(stamp.resource?)?.clone();
        Some((stamp, bitmap))
    }

    /// Stamp that the next stroke on the paint target will use.
    pub fn stroke_stamp(&self) -> Option<(ImageStamp, Arc<Bitmap>)> {
        let b = &self.brush;
        if b.mode != subcraft_paint::StrokeMode::Paint || b.target != subcraft_paint::PaintTarget::Content {
            return None;
        }
        self.brush_stamp().map(|(s, bmp)| (s.clone(), bmp))
    }

    /// The layer "turns on" the stamp's channel: the texture set gets the channel if it lacked it
    /// and the layer's blend for it becomes active. Part of the stroke's document change, so it is
    /// undone with the stroke. Other channels are untouched. Returns whether anything changed.
    pub(crate) fn enable_layer_channel(&mut self, layer: subcraft_core::LayerId, kind: ChannelKind) -> bool {
        let ts_id = self.selection.texture_set;
        let Some(ts) = self.doc.texture_set_mut(ts_id) else { return false };
        let paintable = ts.find(layer).is_some_and(|l| matches!(&l.content, LayerContent::Paint(p) if p.path.is_none() && p.text.is_none()));
        if !paintable {
            return false;
        }
        let mut changed = false;
        if !ts.has_channel(kind) {
            ts.channels.push(ChannelDef::default_for(kind));
            changed = true;
        }
        if let Some(l) = ts.find_mut(layer) {
            if !l.blend.get(&kind).is_some_and(|b| b.is_active()) {
                let mode = if kind == ChannelKind::Normal { BlendMode::NormalMapCombine } else { BlendMode::Normal };
                l.blend.insert(kind, ChannelBlend::new(mode));
                changed = true;
            }
        }
        if changed {
            self.revision += 1;
        }
        changed
    }
}
