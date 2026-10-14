//! Importing the user's own images as project resources (fill-layer textures, masks).
//!
//! The original file bytes are kept so the project embeds the image losslessly and stays
//! portable; identity is the resource UUID, never the file path.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use subcraft_assets::texture_maps::{Classified, MapConversion};
use subcraft_core::{ChannelKind, LayerId, ResourceId};
use subcraft_doc::{FillChannel, FillSource, Layer, LayerContent, Resource, ResourceKind};

use crate::command::Command;
use crate::session::Session;

/// Largest accepted image side (larger textures are almost always a mistake at this stage and
/// cost a lot of memory as decoded bitmaps).
pub const MAX_IMAGE_SIDE: u32 = 16384;

impl Session {
    /// Import an image file (PNG, JPEG, TGA, TIFF, BMP, EXR, HDR) as a texture resource.
    pub fn import_image(&mut self, path: &Path) -> Result<ResourceId, String> {
        let bytes = subcraft_core::vfs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
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

/// Outcome of [`Session::import_texture_set`], for the status line / dialog.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextureSetImport {
    pub layer_name: String,
    /// `(file name, channel)` that became fill-layer channels.
    pub used: Vec<(String, ChannelKind)>,
    /// `(file name, reason)` that were left out.
    pub skipped: Vec<(String, String)>,
}

impl TextureSetImport {
    pub fn summary(&self) -> String {
        let used: Vec<String> = self.used.iter().map(|(_, c)| c.display_name().to_string()).collect();
        let mut s = format!("Imported \"{}\": {}", self.layer_name, used.join(", "));
        if !self.skipped.is_empty() {
            s.push_str(&format!(" ({} file(s) skipped)", self.skipped.len()));
        }
        s
    }
}

impl Session {
    /// Import an image file, converting it first when needed: glossiness/smoothness inverted to
    /// roughness, DirectX normal maps to OpenGL (green flipped). Converted images are stored
    /// losslessly under a name saying what was done.
    pub fn import_converted(&mut self, path: &Path, conversion: MapConversion) -> Result<ResourceId, String> {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("image").to_string();
        let bytes = subcraft_core::vfs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if conversion == MapConversion::None {
            return self.import_image_bytes(&name, bytes);
        }
        let bitmap = subcraft_image::io::decode_bitmap(&bytes, Some(&name)).map_err(|e| format!("{name}: {e}"))?;
        let converted = match conversion {
            MapConversion::Invert => bitmap.map_color(|_, v| 1.0 - v),
            _ => bitmap.map_color(|c, v| if c == 1 { 1.0 - v } else { v }),
        };
        let (encoded, ext) = subcraft_image::encode_bitmap(&converted).map_err(|e| format!("{name}: {e}"))?;
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("image");
        let suffix = if conversion == MapConversion::Invert { "roughness" } else { "opengl" };
        self.import_image_bytes(&format!("{stem} ({suffix}).{ext}"), encoded)
    }

    /// Use an image file in one channel of a fill layer (drag a texture onto a channel row).
    /// The file name decides conversions only when it names that same channel (a `_gloss` map
    /// dropped on Roughness is inverted; a `_normal_dx` map dropped on Normal is flipped).
    /// Keeps the channel's strength; one undo step.
    pub fn set_fill_channel_image(&mut self, layer: LayerId, channel: ChannelKind, path: &Path) -> Result<(), String> {
        let current = match self.selected_texture_set().find(layer).map(|l| &l.content) {
            Some(LayerContent::Fill(f)) => f.channels.get(&channel).cloned(),
            Some(_) => return Err("drop textures onto a fill layer's channel".into()),
            None => return Err("layer not found".into()),
        };
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let conversion = match subcraft_assets::texture_maps::classify(name) {
            Classified::Map(m) if m.channel == channel => m.conversion,
            _ => MapConversion::None,
        };
        let resource = self.import_converted(path, conversion)?;
        let strength = current.map_or(1.0, |c| c.strength);
        let fill = FillChannel { strength, ..FillChannel::new(FillSource::Bitmap { resource }) };
        self.execute(Command::SetFillChannel { layer, channel, fill }).map_err(|e| e.to_string())
    }

    /// Add an image file as a new fill layer: in the channel its name suggests (with the usual
    /// conversions), or as Base Color when the name says nothing. Returns the layer name.
    pub fn add_texture_layer(&mut self, path: &Path) -> Result<String, String> {
        match self.import_texture_set(&[path.to_path_buf()]) {
            Ok(r) => Ok(r.layer_name),
            Err(_) => {
                let resource = self.import_image(path)?;
                let ts = self.selected_texture_set().clone();
                let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Texture").to_string();
                let mut layer = Layer::new_fill(name.clone(), &ts);
                let LayerContent::Fill(fill) = &mut layer.content else { unreachable!("fill layer") };
                for c in fill.channels.values_mut() {
                    c.enabled = false;
                }
                fill.channels.insert(ChannelKind::BaseColor, FillChannel::new(FillSource::Bitmap { resource }));
                self.execute(Command::InsertLayer { layer: Box::new(layer), label: format!("Texture: {name}") })
                    .map_err(|e| e.to_string())?;
                Ok(name)
            }
        }
    }

    /// Import several texture files as one fill layer on the selected texture set. Files are
    /// matched to channels by name (see `assets::texture_maps`); glossiness is inverted to
    /// roughness and DirectX normals are converted to OpenGL. One undo step.
    pub fn import_texture_set(&mut self, paths: &[PathBuf]) -> Result<TextureSetImport, String> {
        let plan = subcraft_assets::texture_maps::plan(paths);
        let file_name = |p: &Path| p.file_name().and_then(|n| n.to_str()).unwrap_or("image").to_string();
        let mut result = TextureSetImport {
            layer_name: plan.name.clone(),
            skipped: plan.skipped.iter().map(|(p, why)| (file_name(p), why.clone())).collect(),
            ..Default::default()
        };
        let ts = self.selected_texture_set().clone();
        let mut layer = Layer::new_fill(plan.name.clone(), &ts);
        let LayerContent::Fill(fill) = &mut layer.content else { unreachable!("new_fill makes a fill layer") };
        for c in fill.channels.values_mut() {
            c.enabled = false;
        }
        for (path, m) in &plan.maps {
            let name = file_name(path);
            if !ts.has_channel(m.channel) {
                result.skipped.push((name, format!("the texture set has no {} channel", m.channel.display_name())));
                continue;
            }
            let resource = self.import_converted(path, m.conversion)?;
            fill.channels.insert(m.channel, FillChannel::new(FillSource::Bitmap { resource }));
            result.used.push((name, m.channel));
        }
        if result.used.is_empty() {
            return Err(if plan.maps.is_empty() {
                "no texture maps recognized (file names need words like basecolor, roughness, normal, height, metallic, ao)".into()
            } else {
                "none of the maps matches a channel of this texture set".into()
            });
        }
        self.execute(Command::InsertLayer { layer: Box::new(layer), label: format!("Import {}", plan.name) })
            .map_err(|e| e.to_string())?;
        Ok(result)
    }
}
