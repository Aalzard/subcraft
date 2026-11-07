//! Text tool: editable UV-space text decals, text brush tips and projected text stamps.
//!
//! A text layer is a paint layer whose `PaintContent::text` holds the editable parameters; its
//! channel rasters are a cache rendered from the glyph outlines at the texture set's resolution
//! (same pattern as pen paths). All edits go through `ReplaceLayer` / `InsertLayer`, so each is
//! one undo step. Projected text reuses the stroke machinery: the text is a brush tip stamped once
//! with camera alignment, which is an ordinary (undoable) paint stroke on the selected layer.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use glam::Vec2;
use subcraft_core::{ChannelKind, ResourceId};
use subcraft_doc::{Document, Layer, LayerContent, Resource, ResourceKind, ResourceStore, TextDecal};
use subcraft_paint::Camera;
use subcraft_text::{Font, FontLibrary, FontRef, TextStyle};

use crate::command::{Command, CommandError};
use crate::session::Session;

/// Largest accepted font file.
pub const MAX_FONT_BYTES: usize = 64 << 20;
/// Resolution of the square brush tip rendered from text.
pub const TIP_SIZE: u32 = 512;

fn invalid(s: impl Into<String>) -> CommandError {
    CommandError::Invalid(s.into())
}

/// Process-wide font library (system fonts are scanned on first use).
pub fn font_library() -> &'static FontLibrary {
    static LIB: OnceLock<FontLibrary> = OnceLock::new();
    LIB.get_or_init(FontLibrary::new)
}

fn resolve(resources: &ResourceStore, font: &FontRef) -> Result<Font, String> {
    font_library()
        .resolve(font, &|id: ResourceId| resources.get(id).and_then(|r| r.encoded.clone()))
        .map_err(|e| e.to_string())
}

/// Name shown for a text layer.
pub fn layer_name(text: &str) -> String {
    let first = text.lines().next().unwrap_or("").trim();
    let short: String = first.chars().take(24).collect();
    if short.is_empty() { "Text".into() } else { format!("Text: {short}") }
}

/// Re-render the raster cache of every text layer in `texture_sets` (after a resolution change,
/// where resampling the old raster would blur the text).
pub(crate) fn refresh_text_rasters(doc: &mut Document, resources: &ResourceStore, texture_sets: &[subcraft_core::TextureSetId]) {
    fn walk(layers: &mut [Arc<Layer>], resolution: u32, resources: &ResourceStore) {
        for layer in layers {
            let needs = match &layer.content {
                LayerContent::Paint(p) => p.text.is_some(),
                LayerContent::Group(g) => !g.children.is_empty(),
                LayerContent::Fill(_) => false,
            };
            if !needs { continue; }
            match &mut Arc::make_mut(layer).content {
                LayerContent::Paint(p) => {
                    let Some(decal) = p.text.clone() else { continue };
                    match resolve(resources, &decal.font).and_then(|f| subcraft_paint::text_decal::rasterize_decal(&decal, &f, resolution).map_err(|e| e.to_string())) {
                        Ok(images) => p.channels = images,
                        Err(e) => log::warn!("text layer not re-rendered: {e}"),
                    }
                }
                LayerContent::Group(g) => walk(&mut g.children, resolution, resources),
                LayerContent::Fill(_) => {}
            }
        }
    }
    for ts in doc.texture_sets.iter_mut().filter(|ts| texture_sets.contains(&ts.id)) {
        let resolution = ts.resolution;
        walk(&mut ts.layers, resolution, resources);
    }
}

impl Session {
    /// Import a `.ttf` / `.otf` / `.ttc` file; the font is embedded in the project (a resource,
    /// identified by UUID — the path is not kept). The same bytes return the existing font.
    pub fn import_font(&mut self, path: &Path) -> Result<FontRef, String> {
        let bytes = subcraft_core::vfs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("font").to_string();
        self.import_font_bytes(&name, bytes)
    }

    pub fn import_font_bytes(&mut self, name: &str, bytes: Vec<u8>) -> Result<FontRef, String> {
        if bytes.len() > MAX_FONT_BYTES {
            return Err(format!("{name}: font files over {} MB are not supported", MAX_FONT_BYTES >> 20));
        }
        if let Some(existing) = self.resources().iter().find(|r| r.kind == ResourceKind::Font && r.encoded.as_deref().is_some_and(|e| *e == bytes)) {
            return Ok(FontRef::Embedded { resource: existing.id });
        }
        let font = FontLibrary::load_bytes(bytes.clone()).map_err(|e| format!("{name}: {e}"))?;
        let id = ResourceId::new();
        let display = font.family_name().map(|f| format!("{f} ({name})")).unwrap_or_else(|| name.to_string());
        self.resources_mut().insert(Resource {
            id,
            name: display,
            kind: ResourceKind::Font,
            bitmap: Arc::new(subcraft_image::Bitmap::new_f32(1, 1, 1, vec![0.0])),
            source_path: None,
            encoded: Some(Arc::new(bytes)),
        });
        Ok(FontRef::Embedded { resource: id })
    }

    /// Fonts embedded in the project, for the font picker.
    pub fn embedded_fonts(&self) -> Vec<(ResourceId, String)> {
        let mut v: Vec<_> = self.resources().iter().filter(|r| r.kind == ResourceKind::Font).map(|r| (r.id, r.name.clone())).collect();
        v.sort_by(|a, b| a.1.cmp(&b.1));
        v
    }

    pub fn resolve_font(&self, font: &FontRef) -> Result<Font, String> {
        resolve(self.resources(), font)
    }

    /// A decal for new text using the brush's enabled channel values, centred on the texture.
    pub fn new_text_decal(&self, text: &str) -> TextDecal {
        let ts = self.selected_texture_set();
        let values: BTreeMap<ChannelKind, _> = self.brush.material.channels.iter().filter(|(k, v)| v.enabled && ts.has_channel(**k)).map(|(&k, v)| (k, v.value)).collect();
        let values = if values.is_empty() { ts.channels.first().map(|c| (c.kind, c.kind.default_paint_value())).into_iter().collect() } else { values };
        TextDecal::new(text, Vec2::splat(0.5), values)
    }

    /// The selected layer's text decal, if it is a text layer.
    pub fn selected_text(&self) -> Option<&TextDecal> {
        match &self.selected_layer()?.content {
            LayerContent::Paint(p) => p.text.as_ref(),
            _ => None,
        }
    }

    fn text_layer(&self, decal: &TextDecal, mut layer: Layer) -> Result<Layer, CommandError> {
        decal.validate().map_err(invalid)?;
        let font = self.resolve_font(&decal.font).map_err(invalid)?;
        let images = subcraft_paint::text_decal::rasterize_decal(decal, &font, self.selected_texture_set().resolution).map_err(|e| invalid(e.to_string()))?;
        let LayerContent::Paint(p) = &mut layer.content else { return Err(invalid("text needs a paint layer")) };
        p.path = None;
        p.text = Some(decal.clone());
        p.channels = images;
        Ok(layer)
    }

    /// Add `decal` as a new text layer above the selection (one undo step).
    pub fn add_text_layer(&mut self, decal: TextDecal) -> Result<(), CommandError> {
        if self.is_stroking() { return Err(CommandError::StrokeInProgress); }
        let mut layer = Layer::new_paint(layer_name(&decal.style.text), self.selected_texture_set());
        // Channels the decal does not write must not blend (a text layer only touches its channels).
        for (kind, blend) in layer.blend.iter_mut() {
            if !decal.values.contains_key(kind) { blend.mode = subcraft_doc::BlendMode::Disable; }
        }
        let layer = self.text_layer(&decal, layer)?;
        self.execute(Command::InsertLayer { layer: Box::new(layer), label: "Add Text".into() })
    }

    /// Replace the selected text layer's parameters and re-render it (one undo step).
    pub fn update_selected_text(&mut self, decal: TextDecal) -> Result<(), CommandError> {
        if self.is_stroking() { return Err(CommandError::StrokeInProgress); }
        let current = self.selected_layer().filter(|l| matches!(&l.content, LayerContent::Paint(p) if p.text.is_some())).ok_or_else(|| invalid("select a text layer"))?.clone();
        let old_auto = match &current.content { LayerContent::Paint(p) => p.text.as_ref().map(|t| layer_name(&t.style.text)), _ => None };
        let mut layer = self.text_layer(&decal, current.clone())?;
        if old_auto.as_deref() == Some(current.name.as_str()) { layer.name = layer_name(&decal.style.text); }
        for (kind, blend) in layer.blend.iter_mut() {
            if decal.values.contains_key(kind) && blend.mode == subcraft_doc::BlendMode::Disable { blend.mode = subcraft_doc::BlendMode::Normal; }
        }
        let id = layer.id;
        self.execute(Command::ReplaceLayer { layer: id, replacement: Box::new(layer), label: "Edit Text".into() })
    }

    /// UV (inside its own tile) of the surface point under `screen`, on the selected texture set.
    fn text_uv_at(&self, camera: &Camera, screen: Vec2) -> Result<Vec2, CommandError> {
        let pick = self.pick(camera, screen).ok_or_else(|| invalid("click the model to place the text"))?;
        if pick.material_slot != self.selected_texture_set().material_slot {
            return Err(invalid("that point belongs to another texture set; select it first"));
        }
        let mesh = &self.scene().mesh;
        let i = &mesh.indices[pick.triangle as usize * 3..][..3];
        let (_, offset) = subcraft_mesh::uv_raster::wrap_to_tile([mesh.uvs[i[0] as usize], mesh.uvs[i[1] as usize], mesh.uvs[i[2] as usize]]);
        Ok(pick.uv - Vec2::new(offset[0] as f32, offset[1] as f32))
    }

    /// Move the selected text layer to the surface point under `screen` (same texture set only).
    pub fn place_selected_text_at(&mut self, camera: &Camera, screen: Vec2) -> Result<(), CommandError> {
        let mut decal = self.selected_text().ok_or_else(|| invalid("select a text layer"))?.clone();
        decal.center = self.text_uv_at(camera, screen)?;
        self.update_selected_text(decal)
    }

    /// Like [`Self::update_selected_text`], but consecutive calls coalesce into one undo step until
    /// [`Self::end_text_gesture`]: used while dragging the on-model handle.
    pub fn update_selected_text_live(&mut self, decal: TextDecal) -> Result<(), CommandError> {
        if self.is_stroking() { return Err(CommandError::StrokeInProgress); }
        let current = self.selected_layer().filter(|l| matches!(&l.content, LayerContent::Paint(p) if p.text.is_some())).ok_or_else(|| invalid("select a text layer"))?.clone();
        let layer = self.text_layer(&decal, current)?;
        let id = layer.id;
        self.edit_layer_set(id, "Move Text", Some(format!("text-gesture:{id:?}")), |ts| {
            *Self::layer_mut(ts, id)? = layer;
            Ok(())
        })
    }

    /// Move the selected text to the surface point under `screen` as part of a drag (coalesced).
    pub fn drag_selected_text_to(&mut self, camera: &Camera, screen: Vec2) -> Result<(), CommandError> {
        let mut decal = self.selected_text().ok_or_else(|| invalid("select a text layer"))?.clone();
        decal.center = self.text_uv_at(camera, screen)?;
        self.update_selected_text_live(decal)
    }

    /// End a drag: the next edit starts a new undo step.
    pub fn end_text_gesture(&mut self) { let _ = self.execute(Command::Seal); }

    /// Use the text as the brush tip: strokes then paint the text along the path.
    pub fn set_text_brush_tip(&mut self, font: &FontRef, style: &TextStyle) -> Result<(), CommandError> {
        let font = self.resolve_font(font).map_err(invalid)?;
        let tip = subcraft_paint::text_decal::text_tip(&font, style, TIP_SIZE).map_err(|e| invalid(e.to_string()))?;
        self.brush.alpha = Some(tip);
        self.brush.alpha_name = layer_name(&style.text);
        Ok(())
    }

    /// Stamp the text once, projected through the camera at `screen`, onto the selected paint
    /// layer: a camera-aligned dab with the text as tip, `size` as the brush size (percent of the
    /// mesh diagonal) and `angle_deg` rotation. One undo step; the brush is restored afterwards.
    pub fn stamp_text_projected(&mut self, camera: &Camera, screen: Vec2, font: &FontRef, style: &TextStyle, size: f32, angle_deg: f32) -> Result<(), CommandError> {
        if self.is_stroking() { return Err(CommandError::StrokeInProgress); }
        let saved = self.brush.clone();
        let result = (|| {
            self.set_text_brush_tip(font, style)?;
            let s = &mut self.brush.settings;
            s.alignment = subcraft_paint::Alignment::Camera;
            s.size = size.clamp(0.1, 100.0);
            s.angle_deg = angle_deg;
            s.hardness = 1.0;
            s.flow = 1.0;
            s.stroke_opacity = 1.0;
            s.follow_path = false;
            s.size_jitter = 0.0;
            s.angle_jitter_deg = 0.0;
            s.position_jitter = 0.0;
            s.scatter_count = 0;
            self.begin_stroke().map_err(|e| invalid(e.to_string()))?;
            let painted = self.stroke_to(camera, screen, false).map_err(|e| invalid(e.to_string()));
            self.end_stroke();
            if painted?.is_empty() { return Err(invalid("the text landed off the model")); }
            Ok(())
        })();
        self.brush = saved;
        result
    }
}
