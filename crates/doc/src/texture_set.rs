//! Texture sets: the paintable region of one imported material slot.
//!
//! Each texture set owns its channel list, resolution and layer stack (bottom-to-top), and is
//! exported independently — mirroring the central abstraction of the target workflow.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use subcraft_core::{ChannelKind, LayerId, TextureSetId};

use crate::layer::{Layer, LayerContent};

/// Storage/export precision of a channel (internal working storage is always f16).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelFormat {
    Bit8,
    Bit16,
    Float16,
    Float32,
}

impl ChannelFormat {
    pub fn label(self) -> &'static str {
        match self {
            ChannelFormat::Bit8 => "8-bit",
            ChannelFormat::Bit16 => "16-bit",
            ChannelFormat::Float16 => "16F",
            ChannelFormat::Float32 => "32F",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelDef {
    pub kind: ChannelKind,
    pub format: ChannelFormat,
}

impl ChannelDef {
    pub fn default_for(kind: ChannelKind) -> Self {
        let format = match kind {
            ChannelKind::BaseColor | ChannelKind::Roughness | ChannelKind::Metallic => ChannelFormat::Bit8,
            ChannelKind::Height | ChannelKind::Normal => ChannelFormat::Float16,
            _ => ChannelFormat::Bit8,
        };
        Self { kind, format }
    }
}

/// How the viewport draws a texture set's opacity (Painter's alpha shader variants:
/// `pbr-metal-rough` = Opaque, `...-with-alpha-test`, `...-with-alpha-blending`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlphaMode {
    /// Opacity is ignored in the viewport (it still exports).
    #[default]
    Opaque,
    /// Texels below the threshold are cut out; the rest is drawn solid (depth-correct).
    Test,
    /// Blended over what is behind it (no depth write, drawn after the opaque sets).
    Blend,
}

impl AlphaMode {
    pub const ALL: [AlphaMode; 3] = [AlphaMode::Opaque, AlphaMode::Test, AlphaMode::Blend];

    pub fn label(self) -> &'static str {
        match self {
            AlphaMode::Opaque => "Opaque",
            AlphaMode::Test => "Alpha test",
            AlphaMode::Blend => "Alpha blending",
        }
    }
}

/// Viewport treatment of a texture set's opacity (Painter's shader "Alpha threshold" and
/// "Alpha dithering" common parameters). Display only: export never reads it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AlphaSettings {
    #[serde(default)]
    pub mode: AlphaMode,
    /// Alpha test cut-off, 0..1 (Painter's default 0.33).
    #[serde(default = "default_alpha_threshold")]
    pub threshold: f32,
    /// Replace blending/testing by an ordered (Bayer) dither: depth-correct, no sorting.
    #[serde(default)]
    pub dithering: bool,
}

fn default_alpha_threshold() -> f32 {
    0.33
}

impl Default for AlphaSettings {
    fn default() -> Self {
        Self { mode: AlphaMode::Opaque, threshold: default_alpha_threshold(), dithering: false }
    }
}

impl AlphaSettings {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

pub const RESOLUTIONS: [u32; 6] = [128, 256, 512, 1024, 2048, 4096];

/// Texture Set Settings > Channels: how the Normal channel takes the normal made from Height.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NormalMixing {
    /// The height detail is added on top of the Normal channel.
    #[default]
    Combine,
    /// The Normal channel alone: height does not change the normals.
    Replace,
}

/// How the Height channel is turned into normals.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeightToNormal {
    /// 3x3 Sobel filter: smoother slopes.
    #[default]
    Smooth,
    /// Central differences of the direct neighbours: crisper edges.
    Sharp,
}

/// How the Ambient Occlusion channel and the baked ambient occlusion map are put together.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AoMixing {
    /// Channel times baked map.
    #[default]
    Multiply,
    /// The channel alone (the baked map is used only when the set has no AO channel).
    Replace,
}

/// How texels outside the UV islands are filled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UvPadding {
    /// Painter's default: from the neighbour across the seam on the mesh.
    #[default]
    Space3d,
    /// From the nearest texel in the texture.
    Space2d,
}

/// Mixing settings of a texture set (Painter's Texture Set Settings > Channels dropdowns).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MixingSettings {
    #[serde(default)]
    pub normal: NormalMixing,
    #[serde(default)]
    pub height_to_normal: HeightToNormal,
    #[serde(default)]
    pub ao: AoMixing,
    #[serde(default)]
    pub uv_padding: UvPadding,
}

impl MixingSettings {
    /// What SubCraft did before these settings existed; projects saved without them load with it,
    /// so their exports do not change.
    pub const LEGACY: MixingSettings = MixingSettings {
        normal: NormalMixing::Combine,
        height_to_normal: HeightToNormal::Sharp,
        ao: AoMixing::Replace,
        uv_padding: UvPadding::Space2d,
    };
}

impl NormalMixing {
    pub const ALL: [NormalMixing; 2] = [NormalMixing::Combine, NormalMixing::Replace];
    pub fn label(self) -> &'static str { match self { NormalMixing::Combine => "Combine", NormalMixing::Replace => "Replace" } }
}

impl HeightToNormal {
    pub const ALL: [HeightToNormal; 2] = [HeightToNormal::Smooth, HeightToNormal::Sharp];
    pub fn label(self) -> &'static str { match self { HeightToNormal::Smooth => "Smooth (Sobel)", HeightToNormal::Sharp => "Sharp" } }
}

impl AoMixing {
    pub const ALL: [AoMixing; 2] = [AoMixing::Multiply, AoMixing::Replace];
    pub fn label(self) -> &'static str { match self { AoMixing::Multiply => "Multiply", AoMixing::Replace => "Replace" } }
}

impl UvPadding {
    pub const ALL: [UvPadding; 2] = [UvPadding::Space3d, UvPadding::Space2d];
    pub fn label(self) -> &'static str { match self { UvPadding::Space3d => "3D Space Neighbor", UvPadding::Space2d => "2D Space Neighbor" } }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum LayerOpError {
    NotFound,
    NotAFolder,
    /// Moving a folder into itself or one of its descendants.
    Cycle,
}

/// Location of a node: parent folder (None = root) and index among siblings (0 = bottom).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerLocation {
    pub parent: Option<LayerId>,
    pub index: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextureSet {
    pub id: TextureSetId,
    pub name: String,
    /// Free text shown under the name in the Texture Set List (Painter's description). Empty by default.
    pub description: String,
    /// Index of the mesh material slot this texture set paints.
    pub material_slot: u32,
    pub resolution: u32,
    /// Normal / height / AO mixing and UV padding (Texture Set Settings > Channels).
    pub mixing: MixingSettings,
    /// Base Color bitmap reconstruction, used for compositing, painting and display.
    pub albedo_filter: subcraft_image::TextureFilter,
    pub channels: Vec<ChannelDef>,
    /// Bottom-to-top.
    pub layers: Vec<Arc<Layer>>,
    /// The UDIM tile (`[column, row]` in stored UV space, see `subcraft_mesh::regroup`) this set
    /// paints; `None` for an ordinary set. Its mesh slot was moved into the 0..1 square.
    pub udim_tile: Option<[i32; 2]>,
    /// Start empty: every channel's stack begins transparent (alpha 0) instead of the opaque
    /// channel default, so unpainted texels export with alpha 0 (issue #145).
    pub transparent: bool,
    /// How the viewport draws opacity.
    pub alpha: AlphaSettings,
    /// ID map colors the user changed in the ID color editor, applied over the baked / imported
    /// ID map (`from` is the color in that map). Part of the document, so undo restores the map
    /// colors together with the ID Color masks that were updated with them.
    pub id_color_edits: Vec<IdColorEdit>,
}

/// One edited ID color: texels of `from` in the baked ID map show `to`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IdColorEdit {
    pub from: [f32; 3],
    pub to: [f32; 3],
}

impl TextureSet {
    pub fn new(name: impl Into<String>, material_slot: u32, resolution: u32) -> Self {
        Self {
            id: TextureSetId::new(),
            name: name.into(),
            description: String::new(),
            material_slot,
            resolution,
            mixing: MixingSettings::default(),
            albedo_filter: Default::default(),
            channels: ChannelKind::METAL_ROUGH.iter().map(|&k| ChannelDef::default_for(k)).collect(),
            layers: Vec::new(),
            udim_tile: None,
            transparent: false,
            alpha: AlphaSettings::default(),
            id_color_edits: Vec::new(),
        }
    }

    /// The 1001-style UDIM number of this set's tile.
    pub fn udim_number(&self) -> Option<u32> {
        self.udim_tile.map(|[col, row]| (1001 + col as i64 - 10 * row as i64).clamp(0, u32::MAX as i64) as u32)
    }

    /// The name without a trailing `.1002` tile suffix (what exported file names start with).
    pub fn base_name(&self) -> &str {
        match (self.udim_number(), self.name.rsplit_once('.')) {
            (Some(_), Some((base, tail))) if tail.len() == 4 && tail.bytes().all(|b| b.is_ascii_digit()) => base,
            _ => &self.name,
        }
    }

    /// Alpha the layer stack starts from: 0 for a transparent set, 1 otherwise.
    pub fn base_alpha(&self) -> f32 {
        if self.transparent { 0.0 } else { 1.0 }
    }

    pub fn has_channel(&self, kind: ChannelKind) -> bool {
        self.channels.iter().any(|c| c.kind == kind)
    }

    /// Change the working resolution, resampling every painted raster (paint channels, masks)
    /// so strokes stay where they were. Fill layers are evaluated at the new size anyway.
    pub fn set_resolution(&mut self, resolution: u32) {
        if resolution == self.resolution {
            return;
        }
        self.resolution = resolution;
        let filter = self.albedo_filter;
        for layer in &mut self.layers {
            Arc::make_mut(layer).map_channel_rasters(&|kind, img| img.resampled_filtered(resolution, resolution,
                if kind == Some(ChannelKind::BaseColor) { filter } else { subcraft_image::TextureFilter::Bilinear }));
        }
    }

    pub fn channel_kinds(&self) -> impl Iterator<Item = ChannelKind> + '_ {
        self.channels.iter().map(|c| c.kind)
    }

    // ----- tree queries -------------------------------------------------------------------------

    pub fn find(&self, id: LayerId) -> Option<&Layer> {
        fn rec(list: &[Arc<Layer>], id: LayerId) -> Option<&Layer> {
            for l in list {
                if l.id == id {
                    return Some(l);
                }
                if let Some(f) = rec(l.children(), id) {
                    return Some(f);
                }
            }
            None
        }
        rec(&self.layers, id)
    }

    fn path_of(&self, id: LayerId) -> Option<Vec<usize>> {
        fn rec(list: &[Arc<Layer>], id: LayerId, path: &mut Vec<usize>) -> bool {
            for (i, l) in list.iter().enumerate() {
                path.push(i);
                if l.id == id || rec(l.children(), id, path) {
                    return true;
                }
                path.pop();
            }
            false
        }
        let mut path = Vec::new();
        rec(&self.layers, id, &mut path).then_some(path)
    }

    /// Copy-on-write mutable access to a node.
    pub fn find_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        let path = self.path_of(id)?;
        let mut list = &mut self.layers;
        for (depth, &i) in path.iter().enumerate() {
            let node = Arc::make_mut(&mut list[i]);
            if depth + 1 == path.len() {
                return Some(node);
            }
            list = node.children_mut()?;
        }
        None
    }

    fn list_mut(&mut self, parent: Option<LayerId>) -> Result<&mut Vec<Arc<Layer>>, LayerOpError> {
        match parent {
            None => Ok(&mut self.layers),
            Some(p) => self.find_mut(p).ok_or(LayerOpError::NotFound)?.children_mut().ok_or(LayerOpError::NotAFolder),
        }
    }

    pub fn location(&self, id: LayerId) -> Option<LayerLocation> {
        fn rec(list: &[Arc<Layer>], parent: Option<LayerId>, id: LayerId) -> Option<LayerLocation> {
            for (index, l) in list.iter().enumerate() {
                if l.id == id {
                    return Some(LayerLocation { parent, index });
                }
                if let Some(loc) = rec(l.children(), Some(l.id), id) {
                    return Some(loc);
                }
            }
            None
        }
        rec(&self.layers, None, id)
    }

    /// True if `id` is `ancestor` or nested inside it.
    pub fn is_descendant_or_self(&self, id: LayerId, ancestor: LayerId) -> bool {
        if id == ancestor {
            return true;
        }
        self.find(ancestor).is_some_and(|a| {
            fn rec(list: &[Arc<Layer>], id: LayerId) -> bool {
                list.iter().any(|l| l.id == id || rec(l.children(), id))
            }
            rec(a.children(), id)
        })
    }

    /// Depth-first walk (bottom-to-top within each level); `f(layer, depth)`.
    pub fn walk(&self, mut f: impl FnMut(&Layer, usize)) {
        fn rec(list: &[Arc<Layer>], depth: usize, f: &mut dyn FnMut(&Layer, usize)) {
            for l in list {
                f(l, depth);
                rec(l.children(), depth + 1, f);
            }
        }
        rec(&self.layers, 0, &mut f);
    }

    pub fn layer_count(&self) -> usize {
        let mut n = 0;
        self.walk(|_, _| n += 1);
        n
    }

    // ----- structural edits ---------------------------------------------------------------------

    /// Insert at `index` within `parent` (clamped). Returns the new id.
    pub fn insert(&mut self, parent: Option<LayerId>, index: usize, layer: Layer) -> Result<LayerId, LayerOpError> {
        let id = layer.id;
        let list = self.list_mut(parent)?;
        let index = index.min(list.len());
        list.insert(index, Arc::new(layer));
        Ok(id)
    }

    /// Insert directly above `reference` (same parent), or at the top of the root if None.
    pub fn insert_above(&mut self, reference: Option<LayerId>, layer: Layer) -> LayerId {
        match reference.and_then(|r| self.location(r)) {
            Some(loc) => self.insert(loc.parent, loc.index + 1, layer).expect("parent exists"),
            None => {
                let n = self.layers.len();
                self.insert(None, n, layer).expect("root insert")
            }
        }
    }

    pub fn remove(&mut self, id: LayerId) -> Result<Arc<Layer>, LayerOpError> {
        let loc = self.location(id).ok_or(LayerOpError::NotFound)?;
        let list = self.list_mut(loc.parent)?;
        Ok(list.remove(loc.index))
    }

    /// Move `id` to `index` within `parent`. `index` refers to the list *after* removal.
    pub fn move_to(&mut self, id: LayerId, parent: Option<LayerId>, index: usize) -> Result<(), LayerOpError> {
        if let Some(p) = parent {
            if self.is_descendant_or_self(p, id) {
                return Err(LayerOpError::Cycle);
            }
            if !self.find(p).ok_or(LayerOpError::NotFound)?.is_folder() {
                return Err(LayerOpError::NotAFolder);
            }
        }
        let node = self.remove(id)?;
        let list = self.list_mut(parent)?;
        let index = index.min(list.len());
        list.insert(index, node);
        Ok(())
    }

    /// Duplicate above the original; returns the copy's id.
    pub fn duplicate(&mut self, id: LayerId) -> Result<LayerId, LayerOpError> {
        let loc = self.location(id).ok_or(LayerOpError::NotFound)?;
        let mut copy = self.find(id).ok_or(LayerOpError::NotFound)?.duplicate_with_new_ids();
        copy.name = format!("{} copy", copy.name);
        self.insert(loc.parent, loc.index + 1, copy)
    }

    /// Wrap a node into a new folder at its position; returns the folder id.
    pub fn group(&mut self, id: LayerId, folder_name: &str) -> Result<LayerId, LayerOpError> {
        let loc = self.location(id).ok_or(LayerOpError::NotFound)?;
        let node = self.remove(id)?;
        let mut folder = Layer::new_folder(folder_name, self);
        if let LayerContent::Group(g) = &mut folder.content {
            g.children.push(node);
        }
        self.insert(loc.parent, loc.index, folder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts() -> TextureSet {
        TextureSet::new("Body", 0, 256)
    }

    #[test]
    fn nearest_resolution_changes_preserve_only_albedo_raster_edges() {
        use crate::{Layer, LayerContent};
        use subcraft_image::{TextureFilter, TiledImage};
        let mut set = TextureSet::new("T", 0, 2);
        set.albedo_filter = TextureFilter::Nearest;
        let mut layer = Layer::new_paint("P", &set);
        if let LayerContent::Paint(p) = &mut layer.content {
            let mut base = TiledImage::new(2, 2, 4, [0., 0., 0., 1.]); base.set(1, 0, [1.; 4]); base.set(1, 1, [1.; 4]);
            let mut rough = TiledImage::new(2, 2, 2, [0., 1., 0., 0.]); rough.set(1, 0, [1.; 4]); rough.set(1, 1, [1.; 4]);
            p.channels.insert(ChannelKind::BaseColor, base); p.channels.insert(ChannelKind::Roughness, rough);
        }
        set.insert_above(None, layer); set.set_resolution(4);
        let LayerContent::Paint(p) = &set.layers[0].content else { panic!() };
        assert_eq!(p.channels[&ChannelKind::BaseColor].get(1, 1)[0], 0.);
        assert_eq!(p.channels[&ChannelKind::BaseColor].get(2, 1)[0], 1.);
        assert_eq!(p.channels[&ChannelKind::Roughness].get(1, 1)[0], 0.25);
    }

    #[test]
    fn insert_find_remove() {
        let mut t = ts();
        let a = t.insert_above(None, Layer::new_paint("A", &t.clone()));
        let b = t.insert_above(Some(a), Layer::new_fill("B", &t.clone()));
        assert_eq!(t.layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["A", "B"]);
        assert_eq!(t.location(b), Some(LayerLocation { parent: None, index: 1 }));
        t.remove(a).unwrap();
        assert!(t.find(a).is_none());
        assert_eq!(t.remove(a), Err(LayerOpError::NotFound));
    }

    #[test]
    fn move_into_folder_and_reject_cycles() {
        let mut t = ts();
        let tc = t.clone();
        let f = t.insert_above(None, Layer::new_folder("F", &tc));
        let inner = t.insert(Some(f), 0, Layer::new_folder("Inner", &tc)).unwrap();
        let p = t.insert_above(None, Layer::new_paint("P", &tc));
        t.move_to(p, Some(inner), 0).unwrap();
        assert_eq!(t.location(p), Some(LayerLocation { parent: Some(inner), index: 0 }));
        assert_eq!(t.move_to(f, Some(inner), 0), Err(LayerOpError::Cycle));
        assert_eq!(t.move_to(f, Some(f), 0), Err(LayerOpError::Cycle));
        let q = t.insert_above(None, Layer::new_paint("Q", &tc));
        assert_eq!(t.move_to(f, Some(q), 0), Err(LayerOpError::NotAFolder));
        assert_eq!(t.layer_count(), 4);
    }

    #[test]
    fn reorder_within_root() {
        let mut t = ts();
        let tc = t.clone();
        let a = t.insert_above(None, Layer::new_paint("A", &tc));
        let _b = t.insert_above(None, Layer::new_paint("B", &tc));
        let _c = t.insert_above(None, Layer::new_paint("C", &tc));
        t.move_to(a, None, 2).unwrap();
        assert_eq!(t.layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["B", "C", "A"]);
    }

    #[test]
    fn copy_on_write_isolates_snapshots() {
        let mut t = ts();
        let tc = t.clone();
        let f = t.insert_above(None, Layer::new_folder("F", &tc));
        let p = t.insert(Some(f), 0, Layer::new_paint("P", &tc)).unwrap();
        let snapshot = t.clone();
        t.find_mut(p).unwrap().name = "Renamed".into();
        assert_eq!(snapshot.find(p).unwrap().name, "P");
        assert_eq!(t.find(p).unwrap().name, "Renamed");
    }

    #[test]
    fn duplicate_gets_new_ids_and_shares_rasters() {
        let mut t = ts();
        let tc = t.clone();
        let p = t.insert_above(None, Layer::new_paint("P", &tc));
        if let LayerContent::Paint(pc) = &mut t.find_mut(p).unwrap().content {
            pc.channel_mut(ChannelKind::BaseColor, 256).set(1, 1, [1.0, 0.0, 0.0, 1.0]);
        }
        let d = t.duplicate(p).unwrap();
        assert_ne!(d, p);
        let (a, b) = (t.find(p).unwrap(), t.find(d).unwrap());
        let (LayerContent::Paint(pa), LayerContent::Paint(pb)) = (&a.content, &b.content) else { panic!() };
        assert!(pa.channels[&ChannelKind::BaseColor].same_storage(&pb.channels[&ChannelKind::BaseColor]));
        assert_eq!(t.location(d).unwrap().index, 1);
    }

    #[test]
    fn group_wraps_in_place() {
        let mut t = ts();
        let tc = t.clone();
        let _a = t.insert_above(None, Layer::new_paint("A", &tc));
        let b = t.insert_above(None, Layer::new_paint("B", &tc));
        let g = t.group(b, "Folder 1").unwrap();
        assert_eq!(t.location(g), Some(LayerLocation { parent: None, index: 1 }));
        assert_eq!(t.location(b), Some(LayerLocation { parent: Some(g), index: 0 }));
    }
}
