//! PSD round trip of a texture set's layer stack for "Edit in PhotoCraft" (ADR-0040).
//!
//! One file per channel: folders become groups, paint layers become 16-bit raster layers with
//! their painted mask as a user mask. Fill layers and effects are not pixels and are left out.
//! Color channels are sRGB-encoded, data channels are written raw and Height is remapped
//! `h · 0.5 + 0.5`. Per-tile hashes of the written samples let [`apply_channel`] write back only
//! the tiles an editor actually changed, so untouched texels stay bit-exact.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use photocraft_psd::layer::{CHANNEL_TRANSPARENCY, LayerRecord, Rect};
use photocraft_psd::pixels::GrayImage;
use photocraft_psd::tree::LayerNode;
use photocraft_psd::{BlendMode as PsdBlend, ColorMode, GroupSpec, LayerSpec, MaskSpec, PixelData, PsdBuilder, PsdFile};
use subcraft_core::color::{linear_to_srgb, srgb_to_linear};
use subcraft_core::{ChannelKind, LayerId};
use subcraft_doc::{BlendMode, Layer, LayerContent, Mask, TextureSet, empty_paint_image};
use subcraft_image::{TILE_SIZE, TILE_TEXELS, TiledImage};

pub use photocraft_psd::PsdError;

/// What was written for one PSD layer, keyed by its `lyid` in [`ChannelBaseline`].
#[derive(Clone, Debug, PartialEq)]
pub struct LayerBaseline {
    pub layer: LayerId,
    pub name: String,
    pub visible: bool,
    /// Per-tile hashes of the channel samples (`None` for folders).
    pub content: Option<Vec<u64>>,
    /// Per-tile hashes of the mask samples (`None` without a mask).
    pub mask: Option<Vec<u64>>,
}

/// The state of one channel file as last written or applied.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChannelBaseline {
    pub layers: HashMap<u32, LayerBaseline>,
}

pub struct ChannelExport {
    pub bytes: Vec<u8>,
    pub baseline: ChannelBaseline,
}

/// Result of applying an edited channel file.
#[derive(Clone, Debug)]
pub struct ChannelEdit {
    /// The texture set's new layer list (bottom-to-top).
    pub layers: Vec<Arc<Layer>>,
    pub baseline: ChannelBaseline,
    pub report: EditReport,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditReport {
    pub changed_tiles: usize,
    pub renamed: usize,
    pub visibility: usize,
    pub added: Vec<LayerId>,
    /// Layers sent to the editor but missing from the file (left untouched).
    pub missing: usize,
}

impl EditReport {
    pub fn is_empty(&self) -> bool {
        self.changed_tiles == 0 && self.renamed == 0 && self.visibility == 0 && self.added.is_empty()
    }
}

/// True if the layer travels to the PSD (folders and paint layers).
pub fn is_exchanged(layer: &Layer) -> bool {
    matches!(layer.content, LayerContent::Paint(_) | LayerContent::Group(_))
}

// ----- encoding -------------------------------------------------------------------------------

fn encode(kind: ChannelKind, v: f32) -> u16 {
    let v = if kind.is_color() { linear_to_srgb(v) } else if kind.is_signed() { v * 0.5 + 0.5 } else { v };
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}

fn decode(kind: ChannelKind, s: u16) -> f32 {
    let v = f32::from(s) / 65535.0;
    if kind.is_color() { srgb_to_linear(v) } else if kind.is_signed() { v * 2.0 - 1.0 } else { v }
}

fn encode_alpha(v: f32) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}

fn encode_mask(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Samples per texel in the file: value components + alpha.
fn spp(kind: ChannelKind) -> usize {
    kind.components() + 1
}

/// Hash of one tile's samples; color under zero alpha is ignored (editors may rewrite it).
fn hash_samples(samples: &[u16], spp: usize) -> u64 {
    let mut h = DefaultHasher::new();
    for px in samples.chunks_exact(spp) {
        if px[spp - 1] == 0 { 0u16.hash(&mut h); } else { px.hash(&mut h); }
    }
    h.finish()
}

fn hash_mask(samples: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    samples.hash(&mut h);
    h.finish()
}

/// Encoded samples of tile `index` (`TILE_TEXELS * spp`).
fn image_tile_samples(image: &TiledImage, kind: ChannelKind, index: usize, buf: &mut Vec<f32>) -> Vec<u16> {
    let n = spp(kind);
    buf.resize(TILE_TEXELS * n, 0.0);
    image.read_tile(index, buf);
    buf.chunks_exact(n)
        .flat_map(|px| (0..n).map(move |c| if c + 1 == n { encode_alpha(px[c]) } else { encode(kind, px[c]) }))
        .collect()
}

fn mask_tile_samples(image: &TiledImage, index: usize, buf: &mut Vec<f32>) -> Vec<u8> {
    buf.resize(TILE_TEXELS, 0.0);
    image.read_tile(index, buf);
    buf.iter().map(|&v| encode_mask(v)).collect()
}

fn content_hashes(image: &TiledImage, kind: ChannelKind) -> Vec<u64> {
    let mut buf = Vec::new();
    (0..image.tile_count()).map(|i| hash_samples(&image_tile_samples(image, kind, i, &mut buf), spp(kind))).collect()
}

fn mask_hashes(image: &TiledImage) -> Vec<u64> {
    let mut buf = Vec::new();
    (0..image.tile_count()).map(|i| hash_mask(&mask_tile_samples(image, i, &mut buf))).collect()
}

/// Bounds of the tiles that differ from a transparent/default fill, or the whole image.
fn content_rect(image: &TiledImage, fill_matters: bool) -> Option<(u32, u32, u32, u32)> {
    if fill_matters {
        return Some((0, 0, image.width(), image.height()));
    }
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for i in (0..image.tile_count()).filter(|&i| image.is_allocated(i)) {
        let r = image.tile_rect(i);
        x0 = x0.min(r.x0 as u32);
        y0 = y0.min(r.y0 as u32);
        x1 = x1.max(r.x1 as u32);
        y1 = y1.max(r.y1 as u32);
    }
    (x0 < x1).then(|| (x0, y0, x1 - x0, y1 - y0))
}

/// Interleaved samples of `rect` from per-tile sample buffers.
fn gather<T: Copy + Default>(image: &TiledImage, rect: (u32, u32, u32, u32), n: usize, mut tile: impl FnMut(usize) -> Vec<T>) -> Vec<T> {
    let (x0, y0, w, h) = rect;
    let mut out = vec![T::default(); w as usize * h as usize * n];
    for i in 0..image.tile_count() {
        let r = image.tile_rect(i);
        let (rx0, ry0, rx1, ry1) = (r.x0 as u32, r.y0 as u32, r.x1 as u32, r.y1 as u32);
        if rx1 <= x0 || ry1 <= y0 || rx0 >= x0 + w || ry0 >= y0 + h {
            continue;
        }
        let data = tile(i);
        for y in ry0.max(y0)..ry1.min(y0 + h) {
            for x in rx0.max(x0)..rx1.min(x0 + w) {
                let src = ((y - ry0) * TILE_SIZE + (x - rx0)) as usize * n;
                let dst = ((y - y0) * w + (x - x0)) as usize * n;
                out[dst..dst + n].copy_from_slice(&data[src..src + n]);
            }
        }
    }
    out
}

fn psd_blend(mode: BlendMode) -> PsdBlend {
    match mode {
        BlendMode::Multiply => PsdBlend::Multiply,
        BlendMode::Screen => PsdBlend::Screen,
        BlendMode::Overlay => PsdBlend::Overlay,
        BlendMode::Darken => PsdBlend::Darken,
        BlendMode::Lighten => PsdBlend::Lighten,
        BlendMode::LinearDodge => PsdBlend::LinearDodge,
        BlendMode::LinearBurn => PsdBlend::LinearBurn,
        BlendMode::ColorBurn => PsdBlend::ColorBurn,
        BlendMode::ColorDodge => PsdBlend::ColorDodge,
        BlendMode::SoftLight => PsdBlend::SoftLight,
        BlendMode::HardLight => PsdBlend::HardLight,
        BlendMode::VividLight => PsdBlend::VividLight,
        BlendMode::LinearLight => PsdBlend::LinearLight,
        BlendMode::PinLight => PsdBlend::PinLight,
        BlendMode::Difference => PsdBlend::Difference,
        BlendMode::Exclusion => PsdBlend::Exclusion,
        BlendMode::Subtract => PsdBlend::Subtract,
        BlendMode::Divide => PsdBlend::Divide,
        BlendMode::Saturation => PsdBlend::Saturation,
        BlendMode::Color => PsdBlend::Color,
        BlendMode::Passthrough => PsdBlend::PassThrough,
        _ => PsdBlend::Normal,
    }
}

// ----- export ---------------------------------------------------------------------------------

/// Writes the exchanged layers of `set` for channel `kind` as a 16-bit PSD.
pub fn write_channel(set: &TextureSet, kind: ChannelKind) -> Result<ChannelExport, PsdError> {
    let res = set.resolution;
    let mode = if kind.components() == 3 { ColorMode::Rgb } else { ColorMode::Grayscale };
    let mut builder = PsdBuilder::new(res, res).color_mode(mode).depth(16);
    let mut emitted = Vec::new();
    emit(&mut builder, &set.layers, kind, res, &mut emitted)?;
    let file = builder.build()?;
    // The builder numbers every record; read the ids back in tree order (same as emission).
    let mut ids = Vec::new();
    collect_ids(&file, &file.layer_tree(), &mut ids);
    let baseline = ChannelBaseline { layers: ids.into_iter().zip(emitted).collect() };
    Ok(ChannelExport { bytes: file.to_bytes()?, baseline })
}

fn emit(builder: &mut PsdBuilder, layers: &[Arc<Layer>], kind: ChannelKind, res: u32, out: &mut Vec<LayerBaseline>) -> Result<(), PsdError> {
    for layer in layers.iter().filter(|l| is_exchanged(l)) {
        let blend = layer.blend_for(kind);
        let opacity = if blend.mode == BlendMode::Disable { 0 } else { (blend.opacity.clamp(0.0, 1.0) * 255.0).round() as u8 };
        match &layer.content {
            LayerContent::Group(group) => {
                out.push(LayerBaseline { layer: layer.id, name: layer.name.clone(), visible: layer.visible, content: None, mask: None });
                let mut spec = GroupSpec::new(layer.name.clone());
                spec.visible = layer.visible;
                spec.open = group.expanded;
                spec.opacity = opacity;
                spec.blend_mode = if blend.mode == BlendMode::Passthrough { PsdBlend::PassThrough } else { psd_blend(blend.mode) };
                builder.begin_group(spec);
                emit(builder, &group.children, kind, res, out)?;
                builder.end_group()?;
            }
            LayerContent::Paint(paint) => {
                let empty;
                let image = match paint.channels.get(&kind) {
                    Some(image) => image,
                    None => { empty = empty_paint_image(kind, res); &empty }
                };
                let n = spp(kind);
                let mut buf = Vec::new();
                let (left, top, width, height, pixels) = match content_rect(image, image.fill()[n - 1] != 0.0) {
                    Some(rect) => {
                        let samples = gather(image, rect, n, |i| image_tile_samples(image, kind, i, &mut buf));
                        let pixels = if n == 4 { PixelData::Rgba16(samples) } else { PixelData::GrayA16(samples) };
                        (rect.0 as i32, rect.1 as i32, rect.2, rect.3, pixels)
                    }
                    None => (0, 0, 0, 0, if n == 4 { PixelData::Rgba16(Vec::new()) } else { PixelData::GrayA16(Vec::new()) }),
                };
                let mut spec = LayerSpec::new(layer.name.clone(), left, top, width, height, pixels);
                spec.visible = layer.visible;
                spec.opacity = opacity;
                spec.blend_mode = psd_blend(blend.mode);
                spec.mask = layer.mask.as_ref().map(mask_spec);
                out.push(LayerBaseline {
                    layer: layer.id,
                    name: layer.name.clone(),
                    visible: layer.visible,
                    content: Some(content_hashes(image, kind)),
                    mask: layer.mask.as_ref().map(|m| mask_hashes(&m.base)),
                });
                builder.push_layer(spec);
            }
            LayerContent::Fill(_) => unreachable!("filtered by is_exchanged"),
        }
    }
    Ok(())
}

fn mask_spec(mask: &Mask) -> MaskSpec {
    let default_color = encode_mask(mask.base.fill()[0]);
    let mut buf = Vec::new();
    let rect = content_rect(&mask.base, false).unwrap_or((0, 0, 0, 0));
    let data = gather(&mask.base, rect, 1, |i| mask_tile_samples(&mask.base, i, &mut buf));
    MaskSpec { rect: Rect::from_xywh(rect.0 as i32, rect.1 as i32, rect.2, rect.3), data, default_color, disabled: !mask.enabled }
}

fn collect_ids(file: &PsdFile, nodes: &[LayerNode], out: &mut Vec<u32>) {
    for node in nodes {
        if let LayerNode::Group { children, .. } = node {
            out.push(file.layers()[node.index()].layer_id().unwrap_or(0));
            collect_ids(file, children, out);
        } else {
            out.push(file.layers()[node.index()].layer_id().unwrap_or(0));
        }
    }
}

// ----- import ---------------------------------------------------------------------------------

/// A decoded raster layer: planes at 16 bits, in the record's rectangle.
struct Planes {
    rect: (i64, i64, i64, i64),
    planes: Vec<Vec<u16>>,
}

impl Planes {
    fn decode(file: &PsdFile, record: &LayerRecord, kind: ChannelKind) -> Result<Self, PsdError> {
        let r = record.rect;
        let (w, h) = r.size()?;
        let depth = file.header.depth;
        let mut planes = Vec::new();
        for id in (0..kind.components() as i16).chain([CHANNEL_TRANSPARENCY]) {
            let plane = if record.channel(id).is_some() && w * h > 0 {
                to_u16(&record.decode_channel(id, depth, file.header.version)?, depth)?
            } else if id == CHANNEL_TRANSPARENCY {
                vec![u16::MAX; w * h]
            } else {
                vec![0; w * h]
            };
            if plane.len() != w * h {
                return Err(PsdError::Invalid("channel size mismatch".into()));
            }
            planes.push(plane);
        }
        Ok(Self { rect: (i64::from(r.left), i64::from(r.top), w as i64, h as i64), planes })
    }

    /// Samples of canvas tile `index` (transparent outside the layer rectangle).
    fn tile(&self, image: &TiledImage, index: usize) -> Vec<u16> {
        let n = self.planes.len();
        let mut out = vec![0u16; TILE_TEXELS * n];
        let r = image.tile_rect(index);
        let (lx, ly, lw, lh) = self.rect;
        for y in r.y0..r.y1 {
            for x in r.x0..r.x1 {
                let (px, py) = (i64::from(x) - lx, i64::from(y) - ly);
                if px < 0 || py < 0 || px >= lw || py >= lh {
                    continue;
                }
                let src = (py * lw + px) as usize;
                let dst = ((y - r.y0) as u32 * TILE_SIZE + (x - r.x0) as u32) as usize * n;
                for (c, plane) in self.planes.iter().enumerate() {
                    out[dst + c] = plane[src];
                }
            }
        }
        out
    }
}

fn to_u16(bytes: &[u8], depth: u16) -> Result<Vec<u16>, PsdError> {
    Ok(match depth {
        8 => bytes.iter().map(|&b| u16::from(b) * 257).collect(),
        16 => bytes.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect(),
        32 => bytes.as_chunks::<4>().0.iter().map(|c| (f32::from_be_bytes(*c).clamp(0.0, 1.0) * 65535.0).round() as u16).collect(),
        d => return Err(PsdError::Unsupported(format!("bit depth {d}"))),
    })
}

fn mask_tile(mask: &GrayImage, image: &TiledImage, index: usize) -> Vec<u8> {
    let mut out = vec![mask.default_value; TILE_TEXELS];
    let r = image.tile_rect(index);
    for y in r.y0..r.y1 {
        for x in r.x0..r.x1 {
            let (px, py) = (i64::from(x) - i64::from(mask.left), i64::from(y) - i64::from(mask.top));
            if px < 0 || py < 0 || px >= i64::from(mask.width) || py >= i64::from(mask.height) {
                continue;
            }
            out[((y - r.y0) as u32 * TILE_SIZE + (x - r.x0) as u32) as usize] = mask.data[(py * i64::from(mask.width) + px) as usize];
        }
    }
    out
}

/// Writes the tiles of `planes` whose hash differs from `hashes` into `image`.
fn apply_tiles(image: &mut TiledImage, kind: ChannelKind, planes: &Planes, hashes: &mut [u64]) -> usize {
    let n = spp(kind);
    let mut changed = 0;
    for (i, hash) in hashes.iter_mut().enumerate() {
        let samples = planes.tile(image, i);
        let h = hash_samples(&samples, n);
        if h == *hash {
            continue;
        }
        let data: Vec<f32> = samples
            .chunks_exact(n)
            .flat_map(|px| (0..n).map(move |c| if c + 1 == n { f32::from(px[c]) / 65535.0 } else { decode(kind, px[c]) }))
            .collect();
        image.write_tile(i, &data);
        *hash = h;
        changed += 1;
    }
    changed
}

fn apply_mask(layer: &mut Layer, file_mask: &GrayImage, res: u32, hashes: &mut Option<Vec<u64>>) -> usize {
    let mask = layer.mask.get_or_insert_with(|| Mask::new(res, true));
    let hashes = hashes.get_or_insert_with(|| mask_hashes(&mask.base));
    let mut changed = 0;
    for (i, hash) in hashes.iter_mut().enumerate() {
        let samples = mask_tile(file_mask, &mask.base, i);
        let h = hash_mask(&samples);
        if h == *hash {
            continue;
        }
        let data: Vec<f32> = samples.iter().map(|&v| f32::from(v) / 255.0).collect();
        mask.base.write_tile(i, &data);
        *hash = h;
        changed += 1;
    }
    changed
}

/// Applies an edited channel file to `set`. Known layers (by `lyid`) get changed tiles, names and
/// visibility; unknown raster layers become new paint layers next to their nearest known
/// sibling below. Nothing is deleted.
pub fn apply_channel(set: &TextureSet, kind: ChannelKind, bytes: &[u8], baseline: &ChannelBaseline) -> Result<ChannelEdit, PsdError> {
    let file = PsdFile::from_bytes(bytes)?;
    let res = set.resolution;
    if file.header.width != res || file.header.height != res {
        return Err(PsdError::Invalid(format!("canvas is {}×{}, the texture set is {res}×{res}; resizing is not supported", file.header.width, file.header.height)));
    }
    let expected = if kind.components() == 3 { ColorMode::Rgb } else { ColorMode::Grayscale };
    if file.header.color_mode != expected {
        return Err(PsdError::Invalid(format!("color mode changed to {:?}; {} needs {expected:?}", file.header.color_mode, kind.display_name())));
    }
    let mut ctx = Apply { file: &file, kind, res, set: set.clone(), baseline: baseline.clone(), report: EditReport::default(), seen: Vec::new() };
    ctx.nodes(&file.layer_tree(), None, None)?;
    ctx.report.missing = ctx.baseline.layers.iter().filter(|(id, b)| !ctx.seen.contains(id) && ctx.set.find(b.layer).is_some()).count();
    Ok(ChannelEdit { layers: ctx.set.layers, baseline: ctx.baseline, report: ctx.report })
}

struct Apply<'a> {
    file: &'a PsdFile,
    kind: ChannelKind,
    res: u32,
    set: TextureSet,
    baseline: ChannelBaseline,
    report: EditReport,
    seen: Vec<u32>,
}

impl Apply<'_> {
    /// Walks one level bottom-to-top. Returns the last known or added layer at this level.
    fn nodes(&mut self, nodes: &[LayerNode], parent: Option<LayerId>, mut below: Option<LayerId>) -> Result<Option<LayerId>, PsdError> {
        for node in nodes {
            let record = &self.file.layers()[node.index()];
            let psd_id = record.layer_id();
            let known = psd_id.and_then(|id| self.baseline.layers.get(&id).map(|b| (id, b.layer))).filter(|(_, l)| self.set.find(*l).is_some());
            if let Some((id, _)) = known { self.seen.push(id); }
            match (node, known) {
                (LayerNode::Group { children, .. }, Some((id, layer))) => {
                    self.properties(id, layer, record);
                    self.nodes(children, Some(layer), None)?;
                    below = Some(layer);
                }
                // A folder made in the editor: its layers join the current level.
                (LayerNode::Group { children, .. }, None) => below = self.nodes(children, parent, below)?.or(below),
                (LayerNode::Layer { .. }, Some((id, layer))) => {
                    self.properties(id, layer, record);
                    if self.baseline.layers[&id].content.is_some() {
                        self.pixels(id, layer, record)?;
                    }
                    below = Some(layer);
                }
                (LayerNode::Layer { .. }, None) => below = Some(self.add(psd_id, record, parent, below)?),
            }
        }
        Ok(below)
    }

    fn properties(&mut self, id: u32, layer: LayerId, record: &LayerRecord) {
        let b = self.baseline.layers.get_mut(&id).expect("known");
        let l = self.set.find_mut(layer).expect("known");
        let name = record.name();
        if name != b.name {
            l.name = name.clone();
            b.name = name;
            self.report.renamed += 1;
        }
        if record.is_visible() != b.visible {
            l.visible = record.is_visible();
            b.visible = l.visible;
            self.report.visibility += 1;
        }
    }

    fn pixels(&mut self, id: u32, layer: LayerId, record: &LayerRecord) -> Result<(), PsdError> {
        let planes = Planes::decode(self.file, record, self.kind)?;
        let file_mask = self.file.layer(self.file.layers().iter().position(|r| std::ptr::eq(r, record)).expect("record of file")).and_then(|l| l.user_mask()).transpose()?;
        let (kind, res) = (self.kind, self.res);
        let b = self.baseline.layers.get_mut(&id).expect("known");
        let l = self.set.find_mut(layer).expect("known");
        let LayerContent::Paint(paint) = &mut l.content else { return Ok(()) };
        // Text and pen-path layers are redrawn from their text / path: pixels edited in the PSD
        // were taken in and then thrown away by the next text or path edit, without a word. Their
        // pixels are left alone (the mask below still comes back).
        if paint.text.is_none() && paint.path.is_none() {
            // A channel the layer never painted is only created when the file put pixels in it.
            let mut image = paint.channels.get(&kind).cloned().unwrap_or_else(|| empty_paint_image(kind, res));
            let changed = apply_tiles(&mut image, kind, &planes, b.content.as_mut().expect("paint baseline"));
            if changed > 0 {
                paint.channels.insert(kind, image);
                self.report.changed_tiles += changed;
            }
        }
        if let Some(m) = file_mask {
            self.report.changed_tiles += apply_mask(l, &m, res, &mut b.mask);
        }
        Ok(())
    }

    fn add(&mut self, psd_id: Option<u32>, record: &LayerRecord, parent: Option<LayerId>, below: Option<LayerId>) -> Result<LayerId, PsdError> {
        let planes = Planes::decode(self.file, record, self.kind)?;
        let mut layer = Layer::new_paint(record.name(), &self.set);
        layer.visible = record.is_visible();
        let mut image = empty_paint_image(self.kind, self.res);
        let mut hashes = content_hashes(&image, self.kind);
        self.report.changed_tiles += apply_tiles(&mut image, self.kind, &planes, &mut hashes);
        if let LayerContent::Paint(paint) = &mut layer.content {
            paint.channels.insert(self.kind, image);
        }
        let index = self.file.layers().iter().position(|r| std::ptr::eq(r, record)).expect("record of file");
        let mut mask_hashes_out = None;
        if let Some(m) = self.file.layer(index).and_then(|l| l.user_mask()).transpose()? {
            self.report.changed_tiles += apply_mask(&mut layer, &m, self.res, &mut mask_hashes_out);
        }
        let id = layer.id;
        if let Some(psd_id) = psd_id {
            self.seen.push(psd_id);
            self.baseline.layers.insert(psd_id, LayerBaseline {
                layer: id,
                name: layer.name.clone(),
                visible: layer.visible,
                content: Some(hashes),
                mask: mask_hashes_out,
            });
        }
        match below {
            Some(reference) => { self.set.insert_above(Some(reference), layer); }
            None => { let _ = self.set.insert(parent, 0, layer); }
        }
        self.report.added.push(id);
        Ok(id)
    }
}

#[cfg(test)]
#[path = "tests/psd.rs"]
mod tests;
