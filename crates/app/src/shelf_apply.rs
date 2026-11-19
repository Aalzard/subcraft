//! Applying shelf items *in place* (Alt+click / Alt+drag / drops on a mask): no new layer is
//! created, the item goes into the layer or mask that is already there.

use subcraft_assets::{BrushAlpha, MaterialPreset};
use subcraft_core::{ChannelKind, LayerId};
use subcraft_doc::{FillChannel, FillSource, LayerContent, MaskEffectKind, Projection};

use crate::command::{Command, CommandError};
use crate::session::Session;

fn invalid(s: impl Into<String>) -> CommandError {
    CommandError::Invalid(s.into())
}

impl Session {
    /// Use a brush alpha as an image effect of `layer`'s mask (adds a mask when there is none).
    /// The alpha's picture becomes the mask shape (projected like any image mask); one undo step.
    pub fn apply_alpha_to_mask(&mut self, layer: LayerId, alpha: &BrushAlpha) -> Result<(), CommandError> {
        if self.selected_texture_set().find(layer).is_none() { return Err(CommandError::LayerNotFound); }
        let bitmap = alpha.bitmap.as_ref().ok_or_else(|| invalid("the round tip has no picture; pick an image alpha"))?;
        let (encoded, ext) = subcraft_image::encode_bitmap(bitmap).map_err(|e| invalid(e.to_string()))?;
        let id = self.import_image_bytes(&format!("{}.{ext}", alpha.name), encoded).map_err(invalid)?;
        let kind = MaskEffectKind::Image { resource: id, projection: Projection::default(), invert: false, balance: 0.5, contrast: 0.0 };
        self.execute(Command::ApplyMaskEffects { layer, effects: vec![kind], label: format!("Mask alpha: {}", alpha.name) })
    }

    /// Use a brush alpha as the image of one channel of fill layer `layer` (an alpha dragged onto
    /// a channel row): the alpha's picture becomes that channel's source, through the layer's
    /// projection like any other image. Keeps the channel's strength; one undo step. Normal
    /// channels are refused: a gray picture is not a normal map.
    pub fn set_fill_channel_alpha(&mut self, layer: LayerId, channel: ChannelKind, alpha: &BrushAlpha) -> Result<(), CommandError> {
        let current = match self.selected_texture_set().find(layer).map(|l| &l.content) {
            Some(LayerContent::Fill(f)) => f.channels.get(&channel).cloned(),
            Some(_) => return Err(invalid("drop alphas onto a fill layer's channel")),
            None => return Err(CommandError::LayerNotFound),
        };
        if matches!(channel, ChannelKind::Normal | ChannelKind::CoatNormal) {
            return Err(invalid("an alpha is a gray picture, not a normal map; drop it on Height instead"));
        }
        let bitmap = alpha.bitmap.as_ref().ok_or_else(|| invalid("the round tip has no picture; pick an image alpha"))?;
        let (encoded, ext) = subcraft_image::encode_bitmap(bitmap).map_err(|e| invalid(e.to_string()))?;
        let resource = self.import_image_bytes(&format!("{}.{ext}", alpha.name), encoded).map_err(invalid)?;
        let strength = current.map_or(1.0, |c| c.strength);
        let fill = FillChannel { strength, ..FillChannel::new(FillSource::Bitmap { resource }) };
        self.execute(Command::SetFillChannel { layer, channel, fill })
    }

    /// Replace the channels of fill layer `layer` with those of a material preset, keeping its
    /// name, blending, mask and effects (one undo step).
    pub fn apply_material_to_layer(&mut self, layer: LayerId, preset: &MaterialPreset) -> Result<(), CommandError> {
        let current = self.selected_texture_set().find(layer).cloned().ok_or(CommandError::LayerNotFound)?;
        if !matches!(current.content, LayerContent::Fill(_)) {
            return Err(invalid("only a fill layer can take a material in place"));
        }
        let ids = self.import_preset_images(&preset.images)?;
        let built = preset.build_with(self.selected_texture_set(), &ids);
        let mut replacement = current;
        replacement.content = built.content;
        self.execute(Command::ReplaceLayer { layer, replacement: Box::new(replacement), label: format!("Material: {}", preset.name) })
    }
}
