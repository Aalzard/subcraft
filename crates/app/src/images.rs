//! Importing the user's own images as project resources (fill-layer textures, masks).
//!
//! The original file bytes are kept so the project embeds the image losslessly and stays
//! portable; identity is the resource UUID, never the file path.

use std::path::Path;
use std::sync::Arc;

use subcraft_core::ResourceId;
use subcraft_doc::{Resource, ResourceKind};

use crate::session::Session;

/// Largest accepted image side (larger textures are almost always a mistake at this stage and
/// cost a lot of memory as decoded bitmaps).
pub const MAX_IMAGE_SIDE: u32 = 16384;

impl Session {
    /// Import an image file (PNG, JPEG, TGA, TIFF, BMP, EXR, HDR) as a texture resource.
    pub fn import_image(&mut self, path: &Path) -> Result<ResourceId, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("image").to_string();
        let id = self.import_image_bytes(&name, bytes)?;
        Ok(id)
    }

    /// Import encoded image bytes. `name` is shown in the UI and, for formats without a
    /// signature (TGA), its extension tells the decoder the format. Importing the same bytes
    /// twice returns the existing resource.
    pub fn import_image_bytes(&mut self, name: &str, bytes: Vec<u8>) -> Result<ResourceId, String> {
        if let Some(existing) = self
            .resources()
            .iter()
            .find(|r| r.kind == ResourceKind::Texture && r.encoded.as_deref().is_some_and(|e| *e == bytes))
        {
            return Ok(existing.id);
        }
        let bitmap = subcraft_image::io::decode_bitmap(&bytes, Some(name)).map_err(|e| format!("{name}: {e}"))?;
        if bitmap.width > MAX_IMAGE_SIDE || bitmap.height > MAX_IMAGE_SIDE {
            return Err(format!("{name}: {}x{} is larger than {MAX_IMAGE_SIDE} pixels", bitmap.width, bitmap.height));
        }
        let id = ResourceId::new();
        self.resources_mut().insert(Resource {
            id,
            name: name.to_string(),
            kind: ResourceKind::Texture,
            bitmap: Arc::new(bitmap),
            source_path: None,
            encoded: Some(Arc::new(bytes)),
        });
        log::info!("imported image {name}");
        Ok(id)
    }
}
