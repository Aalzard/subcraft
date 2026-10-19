//! Material presets (fill layers) and smart masks (mask effect stacks).

use std::collections::HashMap;
use std::sync::Arc;

use subcraft_core::{AssetId, ChannelKind, ChannelValue, ResourceId};
use subcraft_doc::{
    FillChannel, FillSource, Generator, GeneratorKind, Layer, LayerContent, MaskEffectKind, ProceduralKind, ProceduralParams, Projection,
    ProjectionMode, Resource, ResourceKind, TextureSet,
};

/// An embedded image a built-in material fills one channel with. It is decoded only when the
/// material is applied or previewed, so listing the shelf costs nothing.
#[derive(Clone, Copy)]
pub struct PresetImage {
    pub channel: ChannelKind,
    /// Unique file name; also the key in [`ImageIds`].
    pub name: &'static str,
    /// Fill-channel strength (Height maps are 0..1 images, used as a gentle bump).
    pub strength: f32,
    pub bytes: &'static [u8],
}

impl std::fmt::Debug for PresetImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PresetImage").field("channel", &self.channel).field("name", &self.name).field("bytes", &self.bytes.len()).finish()
    }
}

/// Resource ids of preset images in the project they are applied to, keyed by image name.
pub type ImageIds = HashMap<&'static str, ResourceId>;

/// Decode images into fresh resources (shelf previews; the session imports its own copies).
/// Images that fail to decode are left out, so their channels stay off.
pub fn decode_images(images: &[PresetImage]) -> (ImageIds, Vec<Arc<Resource>>) {
    let mut ids = ImageIds::new();
    let mut resources = Vec::new();
    for img in images {
        match subcraft_image::io::decode_bitmap(img.bytes, Some(img.name)) {
            Ok(bitmap) => {
                let id = ResourceId::new();
                ids.insert(img.name, id);
                resources.push(Arc::new(Resource {
                    id,
                    name: img.name.into(),
                    kind: ResourceKind::Texture,
                    bitmap: Arc::new(bitmap),
                    source_path: None,
                    encoded: Some(Arc::new(img.bytes.to_vec())),
                }));
            }
            Err(e) => log::warn!("built-in image {}: {e}", img.name),
        }
    }
    (ids, resources)
}

/// A fill layer recipe: channel sources (others disabled), embedded images and a projection.
#[derive(Clone, Debug)]
pub struct MaterialPreset {
    pub id: AssetId,
    pub name: String,
    pub category: &'static str,
    pub channels: Vec<(ChannelKind, FillSource)>,
    /// Image channels (they win over `channels` for the same channel once imported).
    pub images: Vec<PresetImage>,
    pub projection: Projection,
}

impl MaterialPreset {
    /// Fill layer for `ts`: preset channels enabled (when the set has them), all others off.
    /// Image channels need [`MaterialPreset::build_with`].
    pub fn build(&self, ts: &TextureSet) -> Layer {
        self.build_with(ts, &ImageIds::new())
    }

    /// [`MaterialPreset::build`] with the project's resource ids for the preset's images.
    pub fn build_with(&self, ts: &TextureSet, ids: &ImageIds) -> Layer {
        let mut layer = Layer::new_fill(self.name.clone(), ts);
        if let LayerContent::Fill(f) = &mut layer.content {
            f.projection = self.projection;
            for ch in f.channels.values_mut() {
                ch.enabled = false;
            }
            for (kind, source) in &self.channels {
                if ts.has_channel(*kind) {
                    f.channels.insert(*kind, FillChannel::new(source.clone()));
                }
            }
            for img in &self.images {
                if let Some(&resource) = ids.get(img.name).filter(|_| ts.has_channel(img.channel)) {
                    f.channels.insert(img.channel, FillChannel { strength: img.strength, ..FillChannel::new(FillSource::Bitmap { resource }) });
                }
            }
        }
        layer
    }
}

fn uni(v: f32) -> FillSource {
    FillSource::uniform(ChannelValue::scalar(v))
}

fn rgb(r: f32, g: f32, b: f32) -> FillSource {
    FillSource::uniform(ChannelValue::rgb(r, g, b))
}

fn proc(kind: ProceduralKind, scale: f32, seed: u32, low: ChannelValue, high: ChannelValue) -> FillSource {
    FillSource::Procedural { params: ProceduralParams { scale, seed, ..ProceduralParams::new(kind) }, low, high }
}

fn triplanar(scale: f32) -> Projection {
    Projection { mode: ProjectionMode::Triplanar, scale: [scale, scale], ..Projection::default() }
}

fn preset(name: &str, category: &'static str, channels: Vec<(ChannelKind, FillSource)>, projection: Projection) -> MaterialPreset {
    MaterialPreset { id: AssetId::from_name(&format!("material/{name}")), name: name.into(), category, channels, images: Vec::new(), projection }
}

fn metal(name: &str, color: [f32; 3], rough: f32) -> MaterialPreset {
    use ChannelKind::*;
    preset(name, "Metal", vec![(BaseColor, rgb(color[0], color[1], color[2])), (Roughness, uni(rough)), (Metallic, uni(1.0))], Projection::default())
}

fn dielectric(name: &str, category: &'static str, color: [f32; 3], rough: f32) -> MaterialPreset {
    use ChannelKind::*;
    preset(name, category, vec![(BaseColor, rgb(color[0], color[1], color[2])), (Roughness, uni(rough)), (Metallic, uni(0.0))], Projection::default())
}

/// Built-in materials (linear base colors from measured reflectance where applicable).
pub fn materials() -> Vec<MaterialPreset> {
    use ChannelKind::*;
    let v = ChannelValue::rgb;
    let s = ChannelValue::scalar;
    vec![
        metal("Steel", [0.56, 0.57, 0.58], 0.35),
        metal("Aluminium", [0.91, 0.92, 0.92], 0.3),
        metal("Chrome", [0.55, 0.556, 0.554], 0.05),
        metal("Gold", [1.0, 0.766, 0.336], 0.22),
        metal("Copper", [0.955, 0.638, 0.538], 0.3),
        preset(
            "Iron Rough",
            "Metal",
            vec![
                (BaseColor, proc(ProceduralKind::Clouds, 6.0, 3, v(0.42, 0.41, 0.40), v(0.56, 0.55, 0.53))),
                (Roughness, proc(ProceduralKind::Grunge, 8.0, 4, s(0.45), s(0.8))),
                (Metallic, uni(1.0)),
            ],
            triplanar(2.0),
        ),
        dielectric("Plastic Glossy Red", "Plastic", [0.6, 0.03, 0.03], 0.18),
        dielectric("Plastic Matte Gray", "Plastic", [0.32, 0.32, 0.33], 0.6),
        dielectric("Rubber Black", "Plastic", [0.03, 0.03, 0.03], 0.9),
        dielectric("Paint White", "Paint", [0.8, 0.8, 0.78], 0.45),
        dielectric("Paint Military Green", "Paint", [0.12, 0.16, 0.08], 0.55),
        dielectric("Ceramic White", "Paint", [0.85, 0.85, 0.83], 0.08),
        preset(
            "Concrete",
            "Stone",
            vec![
                (BaseColor, proc(ProceduralKind::Clouds, 8.0, 21, v(0.30, 0.30, 0.29), v(0.48, 0.47, 0.45))),
                (Roughness, proc(ProceduralKind::Grunge, 12.0, 22, s(0.82), s(0.97))),
                (Metallic, uni(0.0)),
                (Height, proc(ProceduralKind::Cells, 24.0, 23, s(0.0), s(0.12))),
            ],
            triplanar(2.0),
        ),
        preset(
            "Wood",
            "Wood",
            vec![
                (BaseColor, proc(ProceduralKind::WoodRings, 3.0, 31, v(0.18, 0.09, 0.04), v(0.45, 0.25, 0.11))),
                (Roughness, uni(0.6)),
                (Metallic, uni(0.0)),
                (Height, proc(ProceduralKind::WoodRings, 3.0, 31, s(0.0), s(0.05))),
            ],
            triplanar(1.0),
        ),
        preset(
            "Rust",
            "Metal",
            vec![
                (BaseColor, proc(ProceduralKind::Grunge, 6.0, 41, v(0.22, 0.07, 0.02), v(0.55, 0.24, 0.07))),
                (Roughness, proc(ProceduralKind::Clouds, 8.0, 42, s(0.75), s(0.95))),
                (Metallic, uni(0.0)),
                (Height, proc(ProceduralKind::Clouds, 16.0, 43, s(0.0), s(0.06))),
            ],
            triplanar(2.0),
        ),
        preset(
            "Leather",
            "Fabric",
            vec![
                (BaseColor, proc(ProceduralKind::Cells, 32.0, 51, v(0.14, 0.06, 0.03), v(0.22, 0.10, 0.05))),
                (Roughness, uni(0.55)),
                (Metallic, uni(0.0)),
                (Height, proc(ProceduralKind::Cells, 32.0, 51, s(0.0), s(0.04))),
            ],
            triplanar(1.0),
        ),
    ]
    .into_iter()
    .chain(crate::open_pack::materials())
    .collect()
}

/// A mask recipe: effects added on top of a black mask.
#[derive(Clone, Debug)]
pub struct SmartMask {
    pub id: AssetId,
    pub name: String,
    pub effects: Vec<MaskEffectKind>,
    /// Images used by image mask effects (saved smart masks).
    pub resources: Vec<std::sync::Arc<subcraft_doc::Resource>>,
    /// File in the user library (`None` for built-in smart masks).
    pub user_file: Option<std::path::PathBuf>,
}

pub(crate) fn generator(kind: GeneratorKind, level: f32, softness: f32, noise: f32, invert: bool) -> MaskEffectKind {
    let mut g = Generator::new(kind);
    g.level = level;
    g.softness = softness;
    g.noise_amount = noise;
    g.invert = invert;
    MaskEffectKind::Generator(g)
}

fn mask(name: &str, effects: Vec<MaskEffectKind>) -> SmartMask {
    SmartMask { id: AssetId::from_name(&format!("smart_mask/{name}")), name: name.into(), effects, resources: Vec::new(), user_file: None }
}

/// Built-in smart masks. Edges / Cavity need baked curvature / AO.
pub fn smart_masks() -> Vec<SmartMask> {
    vec![
        mask("Edge Wear", vec![generator(GeneratorKind::Edges, 0.62, 0.08, 0.35, false)]),
        mask("Cavity Dirt", vec![generator(GeneratorKind::Cavity, 0.25, 0.25, 0.3, false)]),
        mask("Top Dust", vec![generator(GeneratorKind::Directional, 0.72, 0.25, 0.4, false)]),
        mask("Bottom Grime", vec![generator(GeneratorKind::PositionGradient, 0.3, 0.35, 0.3, true)]),
        mask(
            "Grunge Breakup",
            vec![MaskEffectKind::Fill {
                value: 1.0,
                procedural: Some(ProceduralParams { scale: 6.0, contrast: 0.4, ..ProceduralParams::new(ProceduralKind::Grunge) }),
                projection: triplanar(2.0),
            }],
        ),
        mask("Heavy Edge Wear", vec![generator(GeneratorKind::Edges, 0.45, 0.15, 0.6, false)]),
        mask("Soft Cavity Dirt", vec![generator(GeneratorKind::Cavity, 0.4, 0.45, 0.25, false)]),
        mask("Moss on Top", vec![generator(GeneratorKind::Directional, 0.55, 0.3, 0.7, false)]),
        mask("Rising Damp", vec![generator(GeneratorKind::PositionGradient, 0.18, 0.2, 0.6, true)]),
        mask("Scratches", vec![procedural_mask(ProceduralKind::Scratches, 3.0, 0.5)]),
        mask("Cloudy Variation", vec![procedural_mask(ProceduralKind::Clouds, 4.0, 0.2)]),
    ]
}

fn procedural_mask(kind: ProceduralKind, scale: f32, contrast: f32) -> MaskEffectKind {
    MaskEffectKind::Fill { value: 1.0, procedural: Some(ProceduralParams { scale, contrast, ..ProceduralParams::new(kind) }), projection: triplanar(2.0) }
}
