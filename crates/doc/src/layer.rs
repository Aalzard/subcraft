//! Layer nodes: paint layers, fill layers and folders.
//!
//! Every node has per-channel blending (mode + opacity), an optional mask, and its content.
//! Children are `Arc`-shared so that document snapshots are cheap; mutation goes through
//! `Arc::make_mut` (copy-on-write along the path to the edited node).

use std::collections::BTreeMap;
use std::sync::Arc;

use subcraft_core::{ChannelKind, LayerId};
use subcraft_image::TiledImage;

use crate::blend::BlendMode;
use crate::effect::{ContentEffect, ContentEffectKind};
use crate::fill::{FillSource, Projection};
use crate::mask::Mask;
use crate::texture_set::TextureSet;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChannelBlend {
    pub mode: BlendMode,
    pub opacity: f32,
}

impl ChannelBlend {
    pub fn new(mode: BlendMode) -> Self {
        Self { mode, opacity: 1.0 }
    }
    pub fn is_active(&self) -> bool {
        self.mode != BlendMode::Disable && self.opacity > 0.0
    }
}

/// Raster content: per channel an image of `components + 1` values (value..., alpha).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct PaintContent {
    pub channels: BTreeMap<ChannelKind, TiledImage>,
    /// Editable pen source; channel rasters are its derived, tiled cache.
    pub path: Option<crate::pen::PenPath>,
    /// Editable text decal; channel rasters are its derived cache (exclusive with `path`).
    pub text: Option<crate::text::TextDecal>,
}

impl PaintContent {
    /// Image for a channel, creating an empty (transparent) one on first use.
    pub fn channel_mut(&mut self, kind: ChannelKind, resolution: u32) -> &mut TiledImage {
        self.channels.entry(kind).or_insert_with(|| empty_paint_image(kind, resolution))
    }
}

pub fn empty_paint_image(kind: ChannelKind, resolution: u32) -> TiledImage {
    let comps = kind.components() + 1;
    let d = kind.default_value().0;
    let mut fill = [0.0f32; 4];
    fill[..kind.components()].copy_from_slice(&d[..kind.components()]);
    fill[comps - 1] = 0.0;
    TiledImage::new(resolution, resolution, comps, fill)
}

#[derive(Clone, Debug, PartialEq)]
pub struct FillChannel {
    pub enabled: bool,
    pub source: FillSource,
    /// Strength of the channel's result (1 = as is). Height: bump scale; Normal: tilt scale
    /// (0 = flat); Base Color / Emissive: brightness; other channels: value multiplier.
    pub strength: f32,
}

impl FillChannel {
    pub fn new(source: FillSource) -> Self {
        Self { enabled: true, source, strength: 1.0 }
    }

    /// What a fill layer gets when `kind` is added to it. Emission starts white at Intensity 3
    /// (a visible glow from the default viewport glow); the rest use their usual paint value.
    pub fn default_for(kind: ChannelKind) -> Self {
        match kind {
            ChannelKind::Emissive => Self { enabled: true, source: FillSource::uniform(subcraft_core::ChannelValue::rgb(1.0, 1.0, 1.0)), strength: 3.0 },
            _ => Self::new(FillSource::uniform(kind.default_paint_value())),
        }
    }

    pub fn disabled(source: FillSource) -> Self {
        Self { enabled: false, source, strength: 1.0 }
    }
}

/// Parametric content covering the whole surface.
#[derive(Clone, Debug, PartialEq)]
pub struct FillContent {
    pub projection: Projection,
    pub channels: BTreeMap<ChannelKind, FillChannel>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct GroupContent {
    pub children: Vec<Arc<Layer>>,
    pub expanded: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LayerContent {
    Paint(PaintContent),
    Fill(FillContent),
    Group(GroupContent),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerKind {
    Paint,
    Path,
    Text,
    Fill,
    Folder,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    pub blend: BTreeMap<ChannelKind, ChannelBlend>,
    pub mask: Option<Mask>,
    /// Show the layer only on these mesh parts (like a geometry mask); `None` = everywhere.
    pub parts: Option<PartMask>,
    /// Colour tag shown on the layer's row (Painter's Set color). Organisation only: it never
    /// changes the composite.
    pub color: Option<LayerColor>,
    /// The layer this one is an instance of (see `crate::instance`): its content, content
    /// effects and mask follow that layer.
    pub instance_of: Option<LayerId>,
    pub content: LayerContent,
    /// Non-destructive effects on the content, bottom to top (separate from the mask's stack).
    pub content_effects: Vec<Arc<ContentEffect>>,
}

/// Colour tags a layer or folder can carry. A folder's tag also marks the rows inside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerColor {
    Red,
    Orange,
    Yellow,
    Green,
    Teal,
    Blue,
    Violet,
    Magenta,
    Grey,
}

impl LayerColor {
    pub const ALL: [LayerColor; 9] = [
        LayerColor::Red, LayerColor::Orange, LayerColor::Yellow, LayerColor::Green, LayerColor::Teal,
        LayerColor::Blue, LayerColor::Violet, LayerColor::Magenta, LayerColor::Grey,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LayerColor::Red => "Red",
            LayerColor::Orange => "Orange",
            LayerColor::Yellow => "Yellow",
            LayerColor::Green => "Green",
            LayerColor::Teal => "Teal",
            LayerColor::Blue => "Blue",
            LayerColor::Violet => "Violet",
            LayerColor::Magenta => "Magenta",
            LayerColor::Grey => "Grey",
        }
    }

    /// Display colour (sRGB bytes).
    pub fn rgb(self) -> [u8; 3] {
        match self {
            LayerColor::Red => [224, 82, 82],
            LayerColor::Orange => [232, 144, 62],
            LayerColor::Yellow => [226, 200, 70],
            LayerColor::Green => [102, 187, 106],
            LayerColor::Teal => [64, 182, 172],
            LayerColor::Blue => [84, 148, 232],
            LayerColor::Violet => [150, 116, 226],
            LayerColor::Magenta => [214, 104, 190],
            LayerColor::Grey => [150, 150, 156],
        }
    }
}

/// The mesh parts (connected pieces, see `subcraft_mesh::topology::mesh_elements`) a layer is
/// visible on. Multiplies the layer's mask; independent of it.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct PartMask {
    pub enabled: bool,
    pub kind: subcraft_core::PartKind,
    /// Visible part ids. Ids are stable for a given mesh (the mesh is stored in the project).
    pub parts: std::collections::BTreeSet<u32>,
}

impl PartMask {
    pub fn new(parts: impl IntoIterator<Item = u32>) -> Self {
        Self::for_kind(subcraft_core::PartKind::Connected, parts)
    }

    pub fn for_kind(kind: subcraft_core::PartKind, parts: impl IntoIterator<Item = u32>) -> Self {
        Self { enabled: true, kind, parts: parts.into_iter().collect() }
    }

    /// Visibility factor for texels of `part` (texels outside every triangle have no part).
    #[inline]
    pub fn factor(&self, part: Option<u32>) -> f32 {
        if !self.enabled || part.is_some_and(|p| self.parts.contains(&p)) { 1.0 } else { 0.0 }
    }
}

fn default_blends(ts: &TextureSet, folder: bool) -> BTreeMap<ChannelKind, ChannelBlend> {
    ts.channels.iter().map(|c| (c.kind, ChannelBlend::new(BlendMode::default_for(c.kind, folder)))).collect()
}

impl Layer {
    pub fn new_paint(name: impl Into<String>, ts: &TextureSet) -> Self {
        Self {
            id: LayerId::new(),
            name: name.into(),
            visible: true,
            blend: default_blends(ts, false),
            mask: None,
            parts: None,
            color: None,
            instance_of: None,
            content_effects: Vec::new(),
            content: LayerContent::Paint(PaintContent::default()),
        }
    }

    /// Fill layer with every texture-set channel enabled at its default paint value.
    pub fn new_fill(name: impl Into<String>, ts: &TextureSet) -> Self {
        let channels = ts
            .channels
            .iter()
            .map(|c| (c.kind, FillChannel::new(FillSource::uniform(c.kind.default_paint_value()))))
            .collect();
        Self {
            id: LayerId::new(),
            name: name.into(),
            visible: true,
            blend: default_blends(ts, false),
            mask: None,
            parts: None,
            color: None,
            instance_of: None,
            content_effects: Vec::new(),
            content: LayerContent::Fill(FillContent { projection: Projection::default(), channels }),
        }
    }

    pub fn new_folder(name: impl Into<String>, ts: &TextureSet) -> Self {
        Self {
            id: LayerId::new(),
            name: name.into(),
            visible: true,
            blend: default_blends(ts, true),
            mask: None,
            parts: None,
            color: None,
            instance_of: None,
            content_effects: Vec::new(),
            content: LayerContent::Group(GroupContent { children: Vec::new(), expanded: true }),
        }
    }

    pub fn kind(&self) -> LayerKind {
        match &self.content {
            LayerContent::Paint(p) if p.path.is_some() => LayerKind::Path,
            LayerContent::Paint(p) if p.text.is_some() => LayerKind::Text,
            LayerContent::Paint(_) => LayerKind::Paint,
            LayerContent::Fill(_) => LayerKind::Fill,
            LayerContent::Group(_) => LayerKind::Folder,
        }
    }

    pub fn is_folder(&self) -> bool {
        matches!(self.content, LayerContent::Group(_))
    }

    pub fn children(&self) -> &[Arc<Layer>] {
        match &self.content {
            LayerContent::Group(g) => &g.children,
            _ => &[],
        }
    }

    pub fn children_mut(&mut self) -> Option<&mut Vec<Arc<Layer>>> {
        match &mut self.content {
            LayerContent::Group(g) => Some(&mut g.children),
            _ => None,
        }
    }

    pub fn blend_for(&self, kind: ChannelKind) -> ChannelBlend {
        self.blend.get(&kind).copied().unwrap_or(ChannelBlend::new(BlendMode::Disable))
    }

    /// Deep copy with fresh ids for this node, its children and mask effects.
    /// Raster data stays shared (copy-on-write).
    pub fn duplicate_with_new_ids(&self) -> Layer {
        let mut map = std::collections::HashMap::new();
        let mut l = self.duplicate_ids(&mut map);
        // References inside the copy read the copied anchors (ADR-0045).
        crate::anchor::remap_inputs(&mut l, &map);
        l
    }

    fn duplicate_ids(&self, map: &mut std::collections::HashMap<subcraft_core::EffectId, subcraft_core::EffectId>) -> Layer {
        let mut l = self.clone();
        l.id = LayerId::new();
        if let LayerContent::Paint(p) = &mut l.content && let Some(path) = &mut p.path {
            for a in &mut path.anchors { a.id = subcraft_core::AnchorId::new(); }
        }
        if let Some(mask) = &mut l.mask {
            for e in mask.effects.iter_mut() {
                let new = subcraft_core::EffectId::new();
                map.insert(e.id, new);
                Arc::make_mut(e).id = new;
            }
        }
        for e in l.content_effects.iter_mut() {
            let new = subcraft_core::EffectId::new();
            map.insert(e.id, new);
            Arc::make_mut(e).id = new;
        }
        if let LayerContent::Group(g) = &mut l.content {
            for c in g.children.iter_mut() {
                *c = Arc::new(c.duplicate_ids(map));
            }
        }
        l
    }

    /// Replace every raster in this subtree (paint channels, mask base, mask and content paint
    /// effects) with `f(raster)`, e.g. to resample a saved smart material to another resolution.
    pub fn map_rasters(&mut self, f: &dyn Fn(&TiledImage) -> TiledImage) {
        self.map_channel_rasters(&|_, image| f(image));
    }

    pub(crate) fn map_channel_rasters(&mut self, f: &dyn Fn(Option<ChannelKind>, &TiledImage) -> TiledImage) {
        for e in self.content_effects.iter_mut() {
            if let ContentEffectKind::Paint { channels } = &e.kind {
                let channels = channels.iter().map(|(k, img)| (*k, f(Some(*k), img))).collect();
                Arc::make_mut(e).kind = ContentEffectKind::Paint { channels };
            }
        }
        if let Some(m) = &mut self.mask {
            m.base = f(None, &m.base);
            for e in m.effects.iter_mut() {
                if let crate::mask::MaskEffectKind::Paint { image } = &e.kind {
                    let image = f(None, image);
                    Arc::make_mut(e).kind = crate::mask::MaskEffectKind::Paint { image };
                }
            }
        }
        match &mut self.content {
            LayerContent::Paint(p) => {
                for (kind, img) in &mut p.channels {
                    *img = f(Some(*kind), img);
                }
            }
            LayerContent::Group(g) => {
                for c in g.children.iter_mut() {
                    Arc::make_mut(c).map_channel_rasters(f);
                }
            }
            LayerContent::Fill(_) => {}
        }
    }

    /// Image resources this subtree uses (fill-channel images and image mask effects), in order,
    /// without duplicates, appended to `out`.
    pub fn collect_resources(&self, out: &mut Vec<subcraft_core::ResourceId>) {
        let mut add = |r: subcraft_core::ResourceId| {
            if !out.contains(&r) {
                out.push(r);
            }
        };
        if let LayerContent::Fill(f) = &self.content {
            for c in f.channels.values() {
                if let FillSource::Bitmap { resource } = c.source {
                    add(resource);
                }
            }
        }
        if let LayerContent::Paint(p) = &self.content && let Some(r) = p.text.as_ref().and_then(|t| t.resource()) {
            add(r);
        }
        for e in &self.content_effects {
            if let ContentEffectKind::Filter { filter, .. } = e.kind {
                filter.resource().into_iter().for_each(&mut add);
            }
            if let ContentEffectKind::Generator(g) = &e.kind {
                g.resources().into_iter().for_each(&mut add);
            }
            if let ContentEffectKind::Fill(f) = &e.kind {
                for c in f.channels.values() {
                    if let FillSource::Bitmap { resource } = c.source {
                        add(resource);
                    }
                }
            }
        }
        for e in self.mask.iter().flat_map(|m| &m.effects) {
            if let crate::mask::MaskEffectKind::Image { resource, .. } = e.kind {
                add(resource);
            }
            if let crate::mask::MaskEffectKind::Generator(g) = &e.kind {
                g.resources().into_iter().for_each(&mut add);
            }
            if let crate::mask::MaskEffectKind::Filter(f) = e.kind {
                f.resource().into_iter().for_each(&mut add);
            }
        }
        for c in self.children() {
            c.collect_resources(out);
        }
    }

    /// Equality ignoring raster contents (used for dirty-region diffing).
    pub fn same_parameters(&self, other: &Layer) -> bool {
        if self.id != other.id || self.visible != other.visible || self.blend != other.blend || self.parts != other.parts {
            return false;
        }
        let masks_same = match (&self.mask, &other.mask) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                a.enabled == b.enabled
                    && a.base.same_layout(&b.base)
                    && a.base.fill() == b.base.fill()
                    && a.effects.len() == b.effects.len()
                    && a.effects.iter().zip(&b.effects).all(|(x, y)| Arc::ptr_eq(x, y) || x.same_parameters(y))
            }
            _ => false,
        };
        if !masks_same {
            return false;
        }
        if self.content_effects.len() != other.content_effects.len()
            || !self.content_effects.iter().zip(&other.content_effects).all(|(x, y)| Arc::ptr_eq(x, y) || x.same_parameters(y))
        {
            return false;
        }
        match (&self.content, &other.content) {
            // A channel image present on one side only is a raster change (absent = transparent,
            // e.g. created lazily by the first stroke) as long as it has the paint-image layout.
            (LayerContent::Paint(a), LayerContent::Paint(b)) => a.text == b.text && same_paint_layout(&a.channels, &b.channels),
            (LayerContent::Fill(a), LayerContent::Fill(b)) => a == b,
            (LayerContent::Group(a), LayerContent::Group(b)) => {
                a.children.len() == b.children.len() && a.children.iter().zip(&b.children).all(|(x, y)| x.id == y.id)
            }
            _ => false,
        }
    }
}

/// Copies of several layers with fresh ids (see [`Layer::duplicate_with_new_ids`]); references
/// between the copied layers read the copied anchors.
pub fn duplicate_layers_with_new_ids<'a>(layers: impl IntoIterator<Item = &'a Layer>) -> Vec<Layer> {
    let mut map = std::collections::HashMap::new();
    let mut out: Vec<Layer> = layers.into_iter().map(|l| l.duplicate_ids(&mut map)).collect();
    for l in &mut out {
        crate::anchor::remap_inputs(l, &map);
    }
    out
}

/// Two per-channel paint raster sets differ only in texels (absent = transparent, e.g. created
/// lazily by the first stroke), so a diff can refresh just their changed tiles.
pub fn same_paint_layout(a: &BTreeMap<ChannelKind, TiledImage>, b: &BTreeMap<ChannelKind, TiledImage>) -> bool {
    let keys: std::collections::BTreeSet<_> = a.keys().chain(b.keys()).collect();
    keys.into_iter().all(|k| match (a.get(k), b.get(k)) {
        (Some(ia), Some(ib)) => ia.same_layout(ib),
        (Some(i), None) | (None, Some(i)) => i.same_layout(&empty_paint_image(*k, i.width())) && i.width() == i.height(),
        (None, None) => true,
    })
}

/// Tiles whose texels differ between two per-channel paint raster sets (see [`same_paint_layout`]).
pub fn changed_paint_tiles(a: &BTreeMap<ChannelKind, TiledImage>, b: &BTreeMap<ChannelKind, TiledImage>, out: &mut impl Extend<usize>) {
    for (kind, ia) in a {
        match b.get(kind) {
            Some(ib) => out.extend(ia.changed_tiles(ib)),
            None => out.extend(ia.changed_tiles(&empty_paint_image(*kind, ia.width()))),
        }
    }
    for (kind, ib) in b.iter().filter(|(k, _)| !a.contains_key(k)) {
        out.extend(ib.changed_tiles(&empty_paint_image(*kind, ib.width())));
    }
}
