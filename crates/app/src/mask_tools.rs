//! Mask actions of Painter's mask menu that need the evaluated mask: showing it alone in the
//! viewport, exporting it to a file. (Inverting the background and clearing are commands.)

use std::path::Path;

use subcraft_core::{LayerId, TextureSetId};
use subcraft_image::TiledImage;

use crate::{CommandError, Session};

/// A layer's mask evaluated with all its effects, at its texture set's resolution.
pub struct MaskImage {
    pub texture_set: TextureSetId,
    pub material_slot: u32,
    /// Two components: the mask value and an alpha of 1.
    pub image: TiledImage,
}

impl Session {
    /// The mask of `layer` evaluated on its own (base and effects; disabled masks too, so a
    /// mask can be inspected while it is switched off).
    pub fn mask_image(&self, layer: LayerId) -> Result<MaskImage, CommandError> {
        let set = self.texture_set_of(layer).ok_or(CommandError::LayerNotFound)?;
        let ts = self.document().texture_set(set).ok_or(CommandError::TextureSetNotFound)?;
        let mut mask = ts.find(layer).ok_or(CommandError::LayerNotFound)?.mask.clone().ok_or(CommandError::NoMask)?;
        mask.enabled = true;
        let geometry = self.scene().geometry.get(ts.material_slot, ts.resolution);
        let no_maps = subcraft_materials::MeshMaps::new();
        let maps = self.mesh_maps(set).unwrap_or(&no_maps);
        let image = crate::spp_stack::eval_mask_image(&mask, ts, &geometry, self.resources(), maps);
        Ok(MaskImage { texture_set: set, material_slot: ts.material_slot, image })
    }

    /// Painter's "Export mask to file": the evaluated mask as a grayscale image (the format
    /// comes from the extension: PNG 8 / 16-bit, TGA, EXR, TIFF). Mask values are data, written
    /// as they are (no colour transform).
    pub fn export_mask(&self, layer: LayerId, path: &Path, format: subcraft_image::FileFormat) -> Result<(), String> {
        let m = self.mask_image(layer).map_err(|e| e.to_string())?;
        let size = m.image.width();
        let dense = m.image.to_dense();
        let comps = m.image.comps();
        let gray: Vec<f32> = dense.chunks(comps).map(|p| p[0].clamp(0.0, 1.0)).collect();
        subcraft_image::save_image(path, size, size, 1, &gray, format).map_err(|e| e.to_string())
    }
}
