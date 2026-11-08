//! Applying shelf items *in place* (Alt+click / Alt+drag / drops on a mask): no new layer is
//! created, the item goes into the layer or mask that is already there.

use subcraft_assets::{BrushAlpha, MaterialPreset};
use subcraft_core::LayerId;
use subcraft_doc::{LayerContent, MaskEffectKind, Projection};

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
        let kind = MaskEffectKind::Image { resource: id, projection: Projection::default(), invert: false };
        self.execute(Command::ApplyMaskEffects { layer, effects: vec![kind], label: format!("Mask alpha: {}", alpha.name) })
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
