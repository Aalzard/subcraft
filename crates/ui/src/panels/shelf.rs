//! Shelf: materials, smart materials, smart masks, brush alphas and environments, with rendered
//! thumbnails. Clicking applies the asset through the session (undoable where it edits layers).

use crate::i18n::tr;
use std::collections::HashMap;
use std::sync::Arc;

use egui::{Color32, RichText, Ui};
use glam::{Mat4, Vec3};
use subcraft_app::assets::{EnvironmentSource, Library};
use subcraft_app::thumbnails::PreviewScene;
use subcraft_core::{AssetId, ChannelKind, ChannelValue};
use subcraft_doc::{FillChannel, FillSource, Layer, LayerContent, Mask, MaskEffect};
use subcraft_render::{SHADED_CHANNELS, ViewParams, ViewportRenderer};

use crate::state::UiState;
use crate::theme;

/// A shelf item being dragged onto the viewport or the layer stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShelfDrag {
    Material(usize),
    SmartMaterial(usize),
    SmartMask(usize),
    /// An image from Shelf → Textures (drop on a channel row to use it there).
    Texture(usize),
    Starter(usize),
    /// A brush alpha (Shelf → Brushes): onto a mask it becomes the mask shape.
    Alpha(usize),
    /// A filter (index into `Filter::ALL`): into the stack it becomes a Passthrough filter layer.
    Filter(usize),
    /// A "SubCraft Materials" pack material (index into `UiState::materials_pack`): a new fill
    /// layer with all its maps.
    PackMaterial(usize),
}

/// An environment tile being dragged onto the 3D view or the Display Settings panel's
/// Environment map button (index into the library's environments).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvironmentDrag(pub usize);

/// A Shelf click: a material swaps the material of the selected fill layer (or adds one when
/// no fill layer is selected); everything else does what a drop does.
pub fn click_shelf_item(s: &mut UiState, item: ShelfDrag, now: f64) {
    if let ShelfDrag::Material(i) = item {
        let Some(m) = s.library.materials.get(i).cloned() else { return };
        match s.session.apply_material(&m) {
            Ok(swapped) => {
                if swapped { s.set_status(format!("Material swapped to {} (drag it onto the Layers panel to add a new layer)", m.name), now); }
                s.recovery.autosaver.request_soon();
            }
            Err(e) => s.set_status(e.to_string(), now),
        }
        return;
    }
    apply_shelf_item(s, item, now);
}

/// Apply a material / smart material (inserted above the selected layer of the selected
/// texture set) or a smart mask (on the selected layer). Used by drops.
pub fn apply_shelf_item(s: &mut UiState, item: ShelfDrag, now: f64) {
    apply_shelf_item_with(s, item, false, now);
}

/// What dropping `item` will do, for the drop-target highlight. `target` names the layer under
/// the cursor; `on_mask` is true over a mask thumbnail.
pub fn drop_label(item: ShelfDrag, alt: bool, target: Option<&str>, on_mask: bool) -> String {
    let t = target.map_or(String::new(), |n| format!(" \"{n}\""));
    match item {
        ShelfDrag::Material(_) if alt => format!("Replace the material of{t}"),
        ShelfDrag::Material(_) => format!("Add material layer above{t} (Ctrl: with a black mask)"),
        ShelfDrag::SmartMaterial(_) if alt => "Smart materials are added as a folder: release Alt".to_string(),
        ShelfDrag::SmartMaterial(_) => format!("Add smart material above{t}"),
        ShelfDrag::SmartMask(_) if alt => format!("Replace the mask effects of{t}"),
        ShelfDrag::SmartMask(_) => format!("Apply mask to{t} (Ctrl or Alt: replace its effects)"),
        ShelfDrag::Texture(_) | ShelfDrag::Starter(_) if alt => format!("Use this image in the active channel of{t}"),
        ShelfDrag::Texture(_) | ShelfDrag::Starter(_) => format!("Add texture layer above{t}"),
        ShelfDrag::Alpha(_) if on_mask || target.is_some() => format!("Use this alpha in the mask of{t}"),
        ShelfDrag::Alpha(_) => "Set as brush tip".to_string(),
        ShelfDrag::Filter(_) if alt || on_mask => format!("Add this filter to{t}"),
        ShelfDrag::Filter(_) => format!("Add a filter layer (Passthrough) above{t}"),
        ShelfDrag::PackMaterial(_) if alt => "Pack materials are added as a new layer: release Alt".to_string(),
        ShelfDrag::PackMaterial(_) => format!("Add material layer above{t} (Ctrl: with a black mask)"),
    }
}

/// `apply_shelf_item` with the Alt reading (Substance-Painter style): Alt applies the item
/// *in place* to the selected layer instead of creating a layer.
/// * Material: replaces the selected fill layer's channels (name, blending and mask are kept).
/// * Texture: sets the active channel of the selected fill layer.
/// * Alpha: becomes an image effect of the selected layer's mask (adds the mask); without Alt it
///   is the brush tip.
/// * Smart material: no in-place form; says so instead of doing nothing.
/// * Smart mask: always applies to the selected layer; with Alt the mask's previous effects are
///   removed first.
pub fn apply_shelf_item_with(s: &mut UiState, item: ShelfDrag, alt: bool, now: f64) {
    let layer = s.session.selection().layer;
    let selected_fill = s.session.selected_layer().is_some_and(|l| matches!(l.content, LayerContent::Fill(_)));
    let result = match item {
        ShelfDrag::Starter(i) => {s.asset_preview.queue(&s.session,i,None);Ok(())},
        ShelfDrag::Material(i) if alt => match (s.library.materials.get(i).cloned(), layer) {
            (Some(m), Some(l)) if selected_fill => s.session.apply_material_to_layer(l, &m).map_err(|e| e.to_string()),
            (Some(_), _) => Err("Alt+click replaces a fill layer's material: select a fill layer first (a click without Alt adds a new layer)".to_string()),
            (None, _) => Ok(()),
        },
        ShelfDrag::Material(i) => match s.library.materials.get(i).cloned() {
            Some(m) => s.session.add_material(&m).map_err(|e| e.to_string()),
            None => Ok(()),
        },
        ShelfDrag::SmartMaterial(_) if alt => Err("A smart material is a folder of layers, so Alt cannot apply it in place: click without Alt to add it".to_string()),
        ShelfDrag::SmartMaterial(i) => match s.library.smart_materials.get(i).cloned() {
            Some(m) => s.session.add_smart_material(&m).map_err(|e| e.to_string()),
            None => Ok(()),
        },
        ShelfDrag::SmartMask(i) => match s.library.smart_masks.get(i).cloned() {
            Some(m) => s.session.apply_smart_mask_with(&m, alt).map_err(|e| format!("{e} (select a layer first)")),
            None => Ok(()),
        },
        ShelfDrag::Texture(i) if alt => match (s.textures.items.get(i).map(|t| t.path.clone()), layer) {
            (Some(p), Some(l)) if selected_fill => {
                let channel = s.active_channel;
                s.session.set_fill_channel_image(l, channel, &p).map(|()| s.set_status(format!("Set {} from the image", channel.display_name()), now))
            }
            (Some(_), _) => Err("Alt+click puts the image in the active channel of the selected fill layer: select a fill layer first".to_string()),
            (None, _) => Ok(()),
        },
        ShelfDrag::Texture(i) => match s.textures.items.get(i).map(|t| t.path.clone()) {
            Some(p) => s.session.add_texture_layer(&p).map(|name| s.set_status(format!("Added \"{name}\" as a fill layer"), now)),
            None => Ok(()),
        },
        ShelfDrag::Alpha(i) => match s.library.alphas.get(i).cloned() {
            Some(a) if alt => match layer {
                Some(l) => s.session.apply_alpha_to_mask(l, &a).map(|()| s.set_status(format!("Using \"{}\" in the mask (Properties: projection)", a.name), now)).map_err(|e| e.to_string()),
                None => Err("Select a layer first: the alpha goes into its mask".to_string()),
            },
            Some(a) => { s.session.set_brush_alpha(&a); s.set_status(format!("Brush tip: {} (Alt+click or drag it onto a mask to use it there)", a.name), now); Ok(()) }
            None => Ok(()),
        },
        // Alt (or a drop on a mask): into the selected layer, like a click; else a filter layer.
        ShelfDrag::Filter(i) => match subcraft_doc::Filter::ALL.get(i).copied() {
            Some(f) if alt => match s.session.shelf_filter_command(f) {
                Some(cmd) => { s.run(vec![cmd], now); Ok(()) }
                None => Err(format!("{} does not fit the selected layer", f.label())),
            },
            Some(f) => s.session.add_filter_layer(f).map_err(|e| e.to_string()),
            None => Ok(()),
        },
        ShelfDrag::PackMaterial(_) if alt => Err("A pack material is added as a new fill layer: drop it without Alt".to_string()),
        ShelfDrag::PackMaterial(i) => match s.materials_pack.get(i).cloned() {
            Some(m) => s.session.apply_pack_material(&m).map(|r| s.set_status(format!("Added {}", r.layer_name), now)),
            None => Ok(()),
        },
    };
    match result {
        Ok(()) => s.recovery.autosaver.request_soon(),
        Err(e) => s.set_status(e, now),
    }
}

/// Whether `item` can go into a mask: Painter shows a ghost mask slot on mask-less rows while
/// it is dragged.
pub fn ghost_mask_accepts(item: ShelfDrag) -> bool {
    match item {
        ShelfDrag::Alpha(_) | ShelfDrag::SmartMask(_) | ShelfDrag::Texture(_) => true,
        ShelfDrag::Filter(i) => subcraft_doc::Filter::ALL.get(i).is_some_and(|f| f.supports_mask()),
        ShelfDrag::Material(_) | ShelfDrag::SmartMaterial(_) | ShelfDrag::Starter(_) | ShelfDrag::PackMaterial(_) => false,
    }
}

/// Painter's ghost mask slot: `item` dropped on it gives `layer` a black mask with the item
/// inside (an alpha or texture as an image effect, a filter, or a smart mask's effects), one
/// undo step, and selects the new mask.
pub fn apply_ghost_mask_drop(s: &mut UiState, layer: subcraft_core::LayerId, item: ShelfDrag, now: f64) {
    use subcraft_app::{Command, LayerTarget};
    use subcraft_doc::{MaskEffectKind, Projection};
    let image = |resource| MaskEffectKind::Image { resource, projection: Projection::default(), invert: false };
    let result: Result<(), String> = match item {
        ShelfDrag::Alpha(i) => match s.library.alphas.get(i).cloned() {
            Some(a) => s.session.apply_alpha_to_mask(layer, &a).map_err(|e| e.to_string()),
            None => Ok(()),
        },
        ShelfDrag::SmartMask(i) => match s.library.smart_masks.get(i).cloned() {
            Some(m) => {
                s.run(vec![Command::SelectLayer(Some(layer))], now);
                s.session.apply_smart_mask(&m).map_err(|e| e.to_string())
            }
            None => Ok(()),
        },
        ShelfDrag::Texture(i) => match s.textures.items.get(i).map(|t| t.path.clone()) {
            Some(p) => s.session.import_image(&p).and_then(|resource| {
                s.session.execute(Command::ApplyMaskEffects { layer, effects: vec![image(resource)], label: "Mask image".into() }).map_err(|e| e.to_string())
            }),
            None => Ok(()),
        },
        ShelfDrag::Filter(i) => match subcraft_doc::Filter::ALL.get(i).copied().filter(|f| f.supports_mask()) {
            Some(f) => s.session.execute(Command::ApplyMaskEffects { layer, effects: vec![MaskEffectKind::Filter(f)], label: format!("Mask filter: {}", f.label()) }).map_err(|e| e.to_string()),
            None => Err("This filter does not work on masks".to_string()),
        },
        ShelfDrag::Material(_) | ShelfDrag::SmartMaterial(_) | ShelfDrag::Starter(_) | ShelfDrag::PackMaterial(_) => Err("Materials do not go into a mask: drop them on the row".to_string()),
    };
    match result {
        Ok(()) => {
            s.run(vec![Command::SelectLayer(Some(layer)), Command::SelectLayerTarget { layer, target: LayerTarget::Mask }], now);
            s.recovery.autosaver.request_soon();
        }
        Err(e) => s.set_status(e, now),
    }
}

/// A Shelf item dropped into the layer stack (Painter):
/// * `with_mask` (Ctrl held): a material or texture becomes a new layer with a black mask, ready
///   to be painted in;
/// * `as_new_layer` (a smart mask dropped on the top zone instead of a row): a new fill layer
///   carrying that mask;
/// * `with_mask` or `alt` on a smart mask: it replaces the mask's effects instead of adding to them;
/// * otherwise the same as [`apply_shelf_item_with`].
pub fn apply_stack_drop(s: &mut UiState, item: ShelfDrag, alt: bool, with_mask: bool, as_new_layer: bool, now: f64) {
    use subcraft_app::{Command, LayerTarget, NewLayer};
    if as_new_layer && matches!(item, ShelfDrag::SmartMask(_)) {
        s.run(vec![Command::AddLayer(NewLayer::Fill)], now);
        apply_shelf_item_with(s, item, false, now);
        return;
    }
    let makes_layer = !alt && matches!(item, ShelfDrag::Material(_) | ShelfDrag::Texture(_) | ShelfDrag::PackMaterial(_));
    let before = s.session.selection().layer;
    let alt = alt || (with_mask && matches!(item, ShelfDrag::SmartMask(_)));
    apply_shelf_item_with(s, item, alt, now);
    if with_mask && makes_layer
        && let Some(layer) = s.session.selection().layer.filter(|l| Some(*l) != before)
    {
        s.run(vec![Command::AddMask { layer, white: false }, Command::SelectLayerTarget { layer, target: LayerTarget::Mask }], now);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShelfTab {
    Textures,
    /// Tangent-space normal maps (the starter pack's and the user's), apart from color textures.
    NormalMaps,
    Materials,
    SmartMaterials,
    SmartMasks,
    /// Filters: click adds one to the selected layer, or swaps the selected filter's type.
    Filters,
    /// Brush, tool and material presets.
    Brushes,
    /// Brush tips (Painter's Alphas type): the user's, the built-in ones and the starter pack's.
    Alphas,
    Environments,
}

impl ShelfTab {
    /// Usage keyword of the type (`u:` in a search).
    pub fn usage(self) -> &'static str {
        match self {
            ShelfTab::Materials => "basematerial",
            ShelfTab::SmartMaterials => "smartmaterial",
            ShelfTab::SmartMasks => "smartmask",
            ShelfTab::Filters => "filter",
            ShelfTab::Brushes => "brush",
            ShelfTab::Alphas => "alpha",
            ShelfTab::Textures => "texture",
            ShelfTab::NormalMaps => "normalmap",
            ShelfTab::Environments => "environment",
        }
    }

    pub fn from_usage(usage: &str) -> Option<Self> {
        CATEGORIES.iter().map(|c| c.0).find(|t| t.usage() == usage)
    }
}

/// Thumbnail sizes of the size menu (UI points), as Painter's Small / Medium / Large.
pub const PREVIEW_SIZES: [(&str, u32); 3] = [("Small", 60), ("Medium", 88), ("Large", 144)];

/// What the Shelf shows: the asset types, the search and the selected folders. The main Assets
/// panel and every sub-library tab have their own.
#[derive(Clone, Debug)]
pub struct ShelfNav {
    pub tab: ShelfTab,
    /// Types added with Ctrl+click, besides `tab`.
    pub extra_tabs: Vec<ShelfTab>,
    pub filter: crate::panels::shelf_filter::ShelfFilter,
    filter_tab: ShelfTab,
    /// Selected folders (`/library/` or `/library/folder/`, lowercase); empty is All libraries.
    pub paths: Vec<String>,
    /// Name of the saved search last applied, while the search still is that one.
    pub saved: Option<String>,
}

impl ShelfNav {
    fn new(tab: ShelfTab) -> Self {
        Self { tab, extra_tabs: Vec::new(), filter: Default::default(), filter_tab: tab, paths: Vec::new(), saved: None }
    }

    /// Selected types in Painter's icon order.
    pub fn tabs(&self) -> Vec<ShelfTab> {
        CATEGORIES.iter().map(|c| c.0).filter(|t| *t == self.tab || self.extra_tabs.contains(t)).collect()
    }

    /// Click on a type icon: only that type; with Ctrl, add or remove it (one always stays).
    pub fn click_type(&mut self, tab: ShelfTab, ctrl: bool) {
        if !ctrl {
            self.tab = tab;
            self.extra_tabs.clear();
        } else if tab == self.tab {
            if !self.extra_tabs.is_empty() {
                self.tab = self.extra_tabs.remove(0);
            }
        } else if let Some(i) = self.extra_tabs.iter().position(|t| *t == tab) {
            self.extra_tabs.remove(i);
        } else {
            self.extra_tabs.push(tab);
        }
    }

    /// Click on a folder: only that folder; with Ctrl, add or remove it.
    pub fn click_path(&mut self, path: String, ctrl: bool) {
        let path = path.to_lowercase();
        if !ctrl {
            self.paths = vec![path];
        } else if let Some(i) = self.paths.iter().position(|p| *p == path) {
            self.paths.remove(i);
        } else {
            self.paths.push(path);
        }
    }

    /// True when `e` is in a selected folder (or none is selected).
    pub fn in_paths(&self, e: &crate::panels::shelf_filter::Entry) -> bool {
        let path = e.path().to_lowercase();
        self.paths.is_empty() || self.paths.iter().any(|p| path.starts_with(p.as_str()))
    }

    /// The whole search as one query (Painter's saved-search form).
    pub fn query(&self) -> String {
        let usages: Vec<&str> = self.tabs().iter().map(|t| t.usage()).collect();
        crate::panels::shelf_filter::compose(&self.filter.query, &usages, &self.paths)
    }

    /// Show what a saved query selects: its types, folders and text.
    pub fn apply_query(&mut self, query: &str) {
        let parts = crate::panels::shelf_filter::decompose(query);
        let tabs: Vec<ShelfTab> = parts.usages.iter().filter_map(|u| ShelfTab::from_usage(u)).collect();
        if let Some((first, rest)) = tabs.split_first() {
            self.tab = *first;
            self.extra_tabs = rest.to_vec();
        }
        self.filter = crate::panels::shelf_filter::ShelfFilter { query: parts.text, ..Default::default() };
        self.filter_tab = self.tab;
        self.paths = parts.paths;
    }
}

/// A search saved in the Saved searches section (Painter's Shelf.ini `[filters]` entries).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SavedSearch {
    pub name: String,
    pub query: String,
}

/// A sub-library: a dock tab of its own showing one search.
pub struct SubLibrary {
    pub id: u32,
    pub name: String,
    /// The query it was made from: Reset returns to it.
    pub query: String,
    pub nav: ShelfNav,
}

/// Size of the grayscale alpha thumbnails.
const THUMB: u32 = 96;
/// Size of the rendered material / environment thumbnails (the composite behind them is
/// `subcraft_app::thumbnails::THUMB_RESOLUTION` square; both are sized to stay sharp in the
/// largest shelf tile, 160 points, without costing a visible frame: see `generate_one_thumbnail`).
const RENDER_THUMB: u32 = 160;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SceneKind {
    Sphere,
    /// Sphere with baked mesh maps (smart materials and masks).
    BakedSphere,
}

pub struct ShelfUi {
    pub tab: ShelfTab,
    /// Types added with Ctrl+click, besides `tab`.
    pub extra_tabs: Vec<ShelfTab>,
    pub filter: crate::panels::shelf_filter::ShelfFilter,
    filter_tab: ShelfTab,
    /// Selected folders (Breadcrumbs / Filter by path); empty is All libraries.
    pub paths: Vec<String>,
    /// Saved search last applied (its tag shows in the search field while it still applies).
    pub saved: Option<String>,
    /// Optional sections of the filter area.
    pub show_paths: bool,
    pub show_saved: bool,
    /// Filter by path: hide folders without a matching asset.
    pub hide_empty_folders: bool,
    /// Libraries folded in the Filter by path tree.
    pub folded: std::collections::HashSet<String>,
    pub sub_libraries: Vec<SubLibrary>,
    next_sub_library: u32,
    /// A sub-library tab to dock beside the Assets panel (the app picks it up).
    pub open_sub_library: Option<u32>,
    /// Sub-library being drawn (Reset shows in its header), `None` for the main panel.
    drawing_sub_library: Option<u32>,
    thumbs: HashMap<AssetId, egui::TextureHandle>,
    sphere: Option<Arc<PreviewScene>>,
    baked_sphere: Option<Arc<PreviewScene>>,
    /// Thumbnail being composited on a worker thread (the composite is far longer than a frame).
    compositing: Option<Compositing>,
    renderer: Option<(ViewportRenderer, Option<SceneKind>)>,
    /// Thumbnail whose pixels are on their way back from the GPU.
    pending: Option<(AssetId, subcraft_render::PendingReadback)>,
    /// Thumbnails of the visible tab are still being rendered (scripted screenshots wait).
    pub busy: bool,
    /// The Display Settings environment mini-shelf is open this frame: render environment
    /// thumbnails even when the Assets panel shows another type.
    pub want_environment_thumbs: bool,
}

/// A thumbnail composite running off the UI thread. The worker also builds the preview scene on
/// first use (its mesh bakes take a good fraction of a second at thumbnail resolution).
struct Compositing {
    id: AssetId,
    kind: SceneKind,
    env_index: Option<usize>,
    rx: std::sync::mpsc::Receiver<(Arc<PreviewScene>, subcraft_compositor::Composite)>,
}

type LayerBuilder = Box<dyn Fn(&subcraft_doc::TextureSet) -> Layer + Send>;

impl Default for ShelfUi {
    fn default() -> Self {
        Self {
            tab: ShelfTab::SmartMaterials,
            extra_tabs: Vec::new(),
            filter: Default::default(),
            filter_tab: ShelfTab::SmartMaterials,
            paths: Vec::new(),
            saved: None,
            show_paths: false,
            show_saved: false,
            hide_empty_folders: true,
            folded: Default::default(),
            sub_libraries: Vec::new(),
            next_sub_library: 1,
            open_sub_library: None,
            drawing_sub_library: None,
            thumbs: HashMap::new(),
            want_environment_thumbs: false,
            sphere: None,
            baked_sphere: None,
            compositing: None,
            renderer: None,
            pending: None,
            busy: false,
        }
    }
}

impl ShelfUi {
    /// Show `tab` with a search query (launch options; tab switches otherwise reset the search).
    pub fn open(&mut self, tab: ShelfTab, query: &str) {
        self.tab = tab;
        self.extra_tabs.clear();
        self.filter_tab = tab;
        self.filter = crate::panels::shelf_filter::ShelfFilter { query: query.into(), ..Default::default() };
        self.paths.clear();
        self.saved = None;
    }

    /// The navigation shown now, taken out of (and later put back into) the panel.
    fn take_nav(&mut self) -> ShelfNav {
        ShelfNav {
            tab: self.tab,
            extra_tabs: std::mem::take(&mut self.extra_tabs),
            filter: std::mem::take(&mut self.filter),
            filter_tab: self.filter_tab,
            paths: std::mem::take(&mut self.paths),
            saved: self.saved.take(),
        }
    }

    fn put_nav(&mut self, nav: ShelfNav) {
        self.tab = nav.tab;
        self.extra_tabs = nav.extra_tabs;
        self.filter = nav.filter;
        self.filter_tab = nav.filter_tab;
        self.paths = nav.paths;
        self.saved = nav.saved;
    }

    /// The panel's navigation as a [`ShelfNav`] (a copy).
    pub fn nav(&self) -> ShelfNav {
        ShelfNav {
            tab: self.tab,
            extra_tabs: self.extra_tabs.clone(),
            filter: self.filter.clone(),
            filter_tab: self.filter_tab,
            paths: self.paths.clone(),
            saved: self.saved.clone(),
        }
    }

    fn set_nav(&mut self, nav: ShelfNav) {
        self.put_nav(nav);
    }

    /// Selected types in Painter's icon order.
    pub fn tabs(&self) -> Vec<ShelfTab> {
        self.nav().tabs()
    }

    /// New sub-library tab showing `query`, named `name`. Returns its id.
    pub fn add_sub_library(&mut self, name: String, query: String) -> u32 {
        let id = self.next_sub_library;
        self.next_sub_library += 1;
        let mut nav = ShelfNav::new(self.tab);
        nav.apply_query(&query);
        nav.saved = Some(name.clone());
        self.sub_libraries.push(SubLibrary { id, name, query, nav });
        self.open_sub_library = Some(id);
        id
    }

    pub fn sub_library_name(&self, id: u32) -> Option<&str> {
        self.sub_libraries.iter().find(|s| s.id == id).map(|s| s.name.as_str())
    }

    /// A closed sub-library is gone (Painter: it has to be made again).
    pub fn close_sub_library(&mut self, id: u32) {
        self.sub_libraries.retain(|s| s.id != id);
    }
}

/// One asset of a type as the search, the breadcrumbs and Filter by path see it.
#[derive(Clone, Debug)]
pub struct Item {
    /// Index into the type's own list, or into the starter pack when `starter`.
    pub index: usize,
    pub starter: bool,
    pub name: String,
    pub usage: &'static str,
    /// Library id: `starter_assets`, `your_assets`, an extra library's name, or `session`.
    pub library: String,
    /// Folder inside the library: a sub-folder of a library file, else the asset's category.
    pub folder: Option<String>,
    pub tags: Vec<String>,
    pub tileable: bool,
}

impl Item {
    pub fn entry(&self) -> crate::panels::shelf_filter::Entry<'_> {
        crate::panels::shelf_filter::Entry { name: &self.name, usage: self.usage, library: &self.library, folder: self.folder.as_deref(), tags: &self.tags, tileable: self.tileable }
    }
}

/// Library id of the "SubCraft Materials" pack in the Assets panel (breadcrumbs, Filter by path).
pub const MATERIALS_PACK: &str = "SubCraft Materials";

/// Item indices of the pack's materials start here, apart from the built-in materials'.
pub const PACK_INDEX: usize = 1_000_000;

/// Every asset of type `tab`, in the order the Shelf draws them.
pub fn items(state: &UiState, tab: ShelfTab) -> Vec<Item> {
    use crate::panels::shelf_filter::STARTER;
    use subcraft_app::assets::UserAssetKind as K;
    let usage = tab.usage();
    let lib = |file: Option<&std::path::Path>| state.library_of(file);
    // A library file's sub-folder is its folder; otherwise its category.
    let folder = |kind: K, file: Option<&std::path::Path>, category: Option<&str>| state.library_subfolder(kind, file).or_else(|| category.map(String::from));
    let item = |index, name: &str, library: String, folder: Option<String>, tags: Vec<String>| Item { index, starter: false, name: name.to_string(), usage, library, folder, tags, tileable: false };
    let starters = |kind: subcraft_assets::starter::Kind| {
        subcraft_assets::starter::ENTRIES.iter().enumerate().filter(move |(_, e)| e.kind == kind).map(move |(i, e)| Item {
            index: i,
            starter: true,
            name: e.name.to_string(),
            usage,
            library: STARTER.to_string(),
            folder: Some("Starter pack".into()),
            tags: Vec::new(),
            tileable: e.tileable(),
        })
    };
    match tab {
        ShelfTab::Textures | ShelfTab::NormalMaps => {
            let normals = tab == ShelfTab::NormalMaps;
            let kind = if normals { subcraft_assets::starter::Kind::Normal } else { subcraft_assets::starter::Kind::Texture };
            let mut out: Vec<Item> = starters(kind).collect();
            out.extend(state.textures.items.iter().enumerate().filter(|(_, t)| (t.category == "Normal") == normals).map(|(i, t)| item(i, &t.name, lib(Some(t.path.as_path())), folder(K::Texture, Some(t.path.as_path()), Some(t.category)), vec![t.category.to_string()])));
            out
        }
        ShelfTab::Materials => {
            let mut out: Vec<Item> = state.library.materials.iter().enumerate().map(|(i, m)| item(i, &m.name, STARTER.to_string(), Some(m.category.to_string()), vec![m.category.to_string()])).collect();
            // The "SubCraft Materials" pack, by its own categories.
            out.extend(state.materials_pack.iter().enumerate().map(|(i, m)| {
                let mut tags = m.tags.clone();
                tags.push(m.category.clone());
                Item { tileable: true, ..item(PACK_INDEX + i, &m.name, MATERIALS_PACK.to_string(), Some(m.category.clone()), tags) }
            }));
            out
        }
        ShelfTab::SmartMaterials => state.library.smart_materials.iter().enumerate().map(|(i, m)| {
            let category = subcraft_app::assets::smart_material_info(&m.name).map(|i| i.category);
            item(i, &m.name, lib(m.user_file.as_deref()), folder(K::SmartMaterial, m.user_file.as_deref(), category), category.into_iter().map(String::from).collect())
        }).collect(),
        ShelfTab::SmartMasks => state.library.smart_masks.iter().enumerate().map(|(i, m)| {
            let category = subcraft_app::assets::smart_mask_info(&m.name).map(|i| i.category);
            item(i, &m.name, lib(m.user_file.as_deref()), folder(K::SmartMask, m.user_file.as_deref(), category), category.into_iter().map(String::from).collect())
        }).collect(),
        ShelfTab::Brushes => preset_order(&state.brush_presets, &state.favorites).into_iter().map(|i| {
            let p = &state.brush_presets[i];
            let folder = match p.kind {
                subcraft_app::brush_preset::PresetKind::Brush => "Brush presets",
                subcraft_app::brush_preset::PresetKind::Tool => "Tool presets",
                subcraft_app::brush_preset::PresetKind::Material => "Material presets",
            };
            item(i, &p.name, lib(p.user_file.as_deref()), Some(folder.to_string()), vec!["preset".into()])
        }).collect(),
        ShelfTab::Alphas => {
            let mut out: Vec<Item> = state.library.alphas.iter().enumerate().filter(|(_, a)| !a.name.starts_with("Starter: ")).map(|(i, a)| item(i, &a.name, lib(a.user_file.as_deref()), folder(K::Alpha, a.user_file.as_deref(), None), Vec::new())).collect();
            out.extend(starters(subcraft_assets::starter::Kind::Alpha));
            out
        }
        ShelfTab::Filters => subcraft_doc::Filter::ALL.iter().enumerate().map(|(i, f)| item(i, f.label(), STARTER.to_string(), None, Vec::new())).collect(),
        ShelfTab::Environments => state.library.environments.iter().enumerate().map(|(i, e)| item(i, &e.name, lib(e.user_file.as_deref()), folder(K::Environment, e.user_file.as_deref(), None), Vec::new())).collect(),
    }
}

/// The search of the panel being drawn, parsed once per frame.
pub struct Search {
    pub nav: ShelfNav,
    pub query: crate::panels::shelf_filter::Query,
}

impl Search {
    pub fn of(state: &UiState) -> Self {
        let nav = state.shelf.nav();
        let query = crate::panels::shelf_filter::Query::parse(&nav.filter.query);
        Self { nav, query }
    }

    /// Passes the chip and the text query (folders aside).
    pub fn matches(&self, item: &Item) -> bool {
        self.nav.filter.matches_entry(&item.entry(), &self.query)
    }

    /// Shown: passes the search and lies in a selected folder.
    pub fn shows(&self, item: &Item) -> bool {
        self.matches(item) && self.nav.in_paths(&item.entry())
    }
}

/// A folder of the libraries: how many assets it holds, and how many of them the selected types
/// and the search would show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    pub library: String,
    pub folder: Option<String>,
    pub assets: usize,
    pub applicable: usize,
}

impl Location {
    pub fn path(&self) -> String {
        crate::panels::shelf_filter::folder_path(&self.library, self.folder.as_deref()).to_lowercase()
    }
}

/// Every library and folder holding assets (libraries first, folders in first-seen order).
pub fn locations(state: &UiState, search: &Search) -> Vec<Location> {
    use crate::panels::shelf_filter::LIBRARIES;
    let selected = search.nav.tabs();
    // The fixed libraries, then the extra ones in the order of the settings.
    let mut out: Vec<Location> = Vec::new();
    for id in LIBRARIES.iter().map(|l| l.0.to_string()).chain(state.libraries.iter().map(|l| l.id.clone())) {
        if !out.iter().any(|l| l.library == id) {
            out.push(Location { library: id, folder: None, assets: 0, applicable: 0 });
        }
    }
    for tab in CATEGORIES.iter().map(|c| c.0) {
        let applies = selected.contains(&tab);
        for it in items(state, tab) {
            let hit = applies && search.matches(&it);
            match out.iter_mut().find(|l| l.library == it.library && l.folder.is_none()) {
                Some(l) => {
                    l.assets += 1;
                    l.applicable += usize::from(hit);
                }
                None => out.push(Location { library: it.library.clone(), folder: None, assets: 1, applicable: usize::from(hit) }),
            }
            if let Some(folder) = &it.folder {
                match out.iter_mut().find(|l| l.library == it.library && l.folder.as_deref().is_some_and(|f| f.eq_ignore_ascii_case(folder))) {
                    Some(l) => {
                        l.assets += 1;
                        l.applicable += usize::from(hit);
                    }
                    None => out.push(Location { library: it.library.clone(), folder: Some(folder.clone()), assets: 1, applicable: usize::from(hit) }),
                }
            }
        }
    }
    out
}

/// Draw sub-library `id`: the Shelf with that tab's own types, search and folders.
pub fn show_sub_library(state: &mut UiState, ui: &mut Ui, id: u32) {
    let Some(i) = state.shelf.sub_libraries.iter().position(|s| s.id == id) else {
        ui.label(RichText::new(tr("This sub-library was closed. Make a new one from the Assets panel.")).color(theme::TEXT_FAINT()));
        return;
    };
    let main = state.shelf.take_nav();
    let nav = state.shelf.sub_libraries[i].nav.clone();
    state.shelf.put_nav(nav);
    state.shelf.drawing_sub_library = Some(id);
    show(state, ui);
    state.shelf.drawing_sub_library = None;
    let nav = state.shelf.take_nav();
    if let Some(s) = state.shelf.sub_libraries.iter_mut().find(|s| s.id == id) {
        s.nav = nav;
    }
    state.shelf.put_nav(main);
}

/// Viewport environment selection.
pub struct EnvUi {
    pub index: usize,
    pub rotation_deg: f32,
    /// HDRI currently uploaded to the viewport renderer.
    pub uploaded: Option<AssetId>,
}

impl Default for EnvUi {
    fn default() -> Self {
        Self { index: 0, rotation_deg: 0.0, uploaded: None }
    }
}

/// Environment a new window starts with (Painter-like soft daylight).
pub const DEFAULT_ENVIRONMENT: &str = "Glasshouse Morning";
/// Azimuth (degrees, `atan2(x, z)`) the key light should come from in the default view: the
/// framing camera sits at 35°, so 65° puts the sun in front of the model, over the viewer's
/// right shoulder, as in Painter's default lighting (never from behind the model).
pub const KEY_LIGHT_AZIMUTH_DEG: f32 = 65.0;
/// Rotation of [`DEFAULT_ENVIRONMENT`] that puts its sun at [`KEY_LIGHT_AZIMUTH_DEG`]
/// (= `rotation_for_key_light` of its sun; fixed so startup does not decode the HDRI early,
/// checked by a test).
pub const DEFAULT_ENVIRONMENT_ROTATION_DEG: f32 = -165.6;

/// Environment rotation (degrees, -180..180 like the Rotation slider) that turns a sun at `sun`
/// (world direction) to [`KEY_LIGHT_AZIMUTH_DEG`].
pub fn rotation_for_key_light(sun: [f32; 3]) -> f32 {
    let azimuth = sun[0].atan2(sun[2]).to_degrees();
    wrap_rotation(KEY_LIGHT_AZIMUTH_DEG - azimuth)
}

/// Fold an environment rotation into the Rotation slider's -180..180 range.
pub fn wrap_rotation(deg: f32) -> f32 {
    (deg + 180.0).rem_euclid(360.0) - 180.0
}

impl EnvUi {
    /// Startup environment: [`DEFAULT_ENVIRONMENT`] turned so the model is lit from the front.
    pub fn startup(library: &Library) -> Self {
        match library.environments.iter().position(|e| e.name == DEFAULT_ENVIRONMENT) {
            Some(index) => Self { index, rotation_deg: DEFAULT_ENVIRONMENT_ROTATION_DEG, ..Self::default() },
            None => Self::default(),
        }
    }
}

/// The environment as the Display Settings describe it, for a camera turned `camera_yaw`
/// radians around the vertical axis: `(rotation in degrees, brightness multiplier)`. A
/// camera-aligned ("Local") environment turns with the view, so the lighting follows the camera.
pub fn environment_view(display: &subcraft_doc::DisplaySettings, rotation_deg: f32, camera_yaw: f32) -> (f32, f32) {
    let follow = if display.env_alignment == subcraft_doc::EnvAlignment::Local { camera_yaw.to_degrees() } else { 0.0 };
    (wrap_rotation(rotation_deg + follow), subcraft_doc::display::ev_to_multiplier(display.env_exposure.clamp(-10.0, 10.0)))
}

/// Configure `params` (and upload an HDRI when needed) for environment `index` of `library`.
pub fn apply_environment(
    library: &Library,
    index: usize,
    rotation_deg: f32,
    intensity: f32,
    params: &mut ViewParams,
    renderer: &mut ViewportRenderer,
    uploaded: &mut Option<AssetId>,
    device: &subcraft_render::wgpu_api::Device,
    queue: &subcraft_render::wgpu_api::Queue,
) {
    let Some(env) = library.environments.get(index) else { return };
    let rot = rotation_deg.to_radians();
    params.environment.rotation = rot;
    match &env.source {
        EnvironmentSource::Procedural(p) => {
            let e = &mut params.environment;
            e.use_hdri = false;
            e.ground = p.ground;
            e.horizon = p.horizon;
            e.zenith = p.zenith;
            e.irradiance_up = p.irradiance_up;
            e.irradiance_down = p.irradiance_down;
            e.panels = p.panels;
            e.top = p.top;
            e.key_softbox = p.key_softbox;
            params.light_intensity = p.light_intensity * intensity;
            params.light_color = p.light_color;
            params.environment_intensity = intensity;
        }
        EnvironmentSource::Hdri(_) | EnvironmentSource::Embedded(_) => {
            let Some(h) = env.source.hdri() else { return };
            if *uploaded != Some(env.id) {
                // On a low video memory adapter the widest levels stay on the processor side.
                let budget = if subcraft_render::caps::low_memory() { subcraft_render::pace::MemoryBudget::TIGHT } else { subcraft_render::pace::MemoryBudget::ROOMY };
                let skip = budget.environment_levels_skipped(&h.levels.iter().map(|l| l.width).collect::<Vec<_>>());
                let levels: Vec<(u32, u32, &[f32])> = h.levels.iter().skip(skip).map(|l| (l.width, l.height, l.rgba.as_slice())).collect();
                renderer.set_hdri(device, queue, Some(subcraft_render::HdriUpload { levels: &levels, sh: h.sh }));
                *uploaded = Some(env.id);
            }
            params.environment.use_hdri = true;
            params.environment.hdri_intensity = intensity;
            params.environment_intensity = 1.0;
            // World direction of the sun: inverse of the shader's environment rotation.
            let s = Vec3::from_array(h.sun_direction);
            let (sn, c) = rot.sin_cos();
            params.light_dir = Vec3::new(c * s.x + sn * s.z, s.y, -sn * s.x + c * s.z);
            params.light_color = h.sun_color;
            params.light_intensity = 1.2 * intensity;
        }
    }
}

/// Asset types whose thumbnails are rendered: the Assets panel's visible types, plus the
/// environments while the Display Settings mini-shelf shows them. The request lasts one frame.
pub(crate) fn thumbnail_tabs(shelf: &ShelfUi) -> Vec<ShelfTab> {
    let mut tabs = shelf.tabs();
    if shelf.want_environment_thumbs && !tabs.contains(&ShelfTab::Environments) {
        tabs.push(ShelfTab::Environments);
    }
    tabs
}

/// Thumbnail of an asset, once rendered.
pub(crate) fn thumbnail<'a>(shelf: &'a ShelfUi, id: &AssetId) -> Option<&'a egui::TextureHandle> {
    shelf.thumbs.get(id)
}

/// Add a pack material to the selected texture set as one fill layer.
pub fn apply_pack_material(state: &mut UiState, material: &subcraft_assets::materials_pack::PackMaterial, now: f64) {
    match state.session.apply_pack_material(material) {
        Ok(r) => {
            state.gpu_dirty = true;
            let skipped = if r.skipped.is_empty() { String::new() } else { format!(" ({} map(s) left out)", r.skipped.len()) };
            state.set_status(format!("Added {}{skipped}", r.layer_name), now);
        }
        Err(e) => state.set_status(format!("{}: {e}", material.name), now),
    }
}

/// Most pack previews decoded per frame (each is a small PNG) while the Materials tab shows.
const PACK_THUMBS_PER_FRAME: usize = 2;

/// The pack materials' `preview.png` files as thumbnails, a few per frame, while the Assets
/// panel shows materials. A preview that cannot be read gets an empty tile (no retry).
fn load_pack_thumbnails(state: &mut UiState, ctx: &egui::Context) {
    if !state.shelf.tabs().contains(&ShelfTab::Materials) {
        return;
    }
    let missing: Vec<(subcraft_core::AssetId, Option<std::path::PathBuf>)> = state.materials_pack.iter().filter(|m| !state.shelf.thumbs.contains_key(&m.id)).take(PACK_THUMBS_PER_FRAME).map(|m| (m.id, m.preview.clone())).collect();
    for (id, preview) in &missing {
        let image = preview.as_ref().and_then(|p| std::fs::read(p).ok()).and_then(|bytes| subcraft_image::io::decode_bitmap(&bytes, None).ok()).map(|b| {
            let (w, h, rgba) = b.preview_rgba8(RENDER_THUMB);
            egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba)
        });
        let image = image.unwrap_or_else(|| egui::ColorImage::new([1, 1], vec![Color32::TRANSPARENT]));
        state.shelf.thumbs.insert(*id, ctx.load_texture(format!("thumb-{id}"), image, egui::TextureOptions::LINEAR));
    }
    if !missing.is_empty() {
        ctx.request_repaint();
    }
}

/// Thumbnail tile: square image with a hairline frame (accent when hovered / selected), name below.
pub(crate) fn tile(ui: &mut Ui, side: f32, thumb: Option<&egui::TextureHandle>, name: &str, selected: bool, tooltip: &str) -> egui::Response {
    if ui.data(|d| d.get_temp::<bool>(egui::Id::new("shelf_list_view"))).unwrap_or(false) {
        return list_row(ui, thumb, name, selected, tooltip);
    }
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(side, side + 22.0), egui::Sense::click_and_drag());
    if resp.dragged()
        && egui::DragAndDrop::has_payload_of_type::<ShelfDrag>(ui.ctx())
        && let Some(p) = ui.ctx().pointer_latest_pos()
    {
        egui::Area::new(egui::Id::new("shelf-drag-ghost"))
            .order(egui::Order::Tooltip)
            .fixed_pos(p + egui::vec2(14.0, 10.0))
            .interactable(false)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(RichText::new(format!("{}  {name}", crate::icons::PLUS)).color(theme::TEXT()));
                    ui.label(RichText::new(tr("Drop on the model or a layer")).small().color(theme::TEXT_FAINT()));
                });
            });
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    let img = egui::Rect::from_min_size(rect.min, egui::vec2(side, side));
    let painter = ui.painter();
    painter.rect_filled(img, 0.0, theme::FIELD());
    match thumb {
        Some(t) => {
            painter.image(t.id(), img.shrink(1.0), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            let icon = ui.data(|d| d.get_temp::<&'static str>(egui::Id::new("shelf_fallback_icon"))).unwrap_or(crate::icons::IMAGE);
            painter.text(img.center(), egui::Align2::CENTER_CENTER, icon, theme::font(24.0), theme::TEXT_FAINT());
        }
    }
    let stroke = if selected {
        egui::Stroke::new(2.0, theme::ACCENT())
    } else if resp.hovered() {
        egui::Stroke::new(1.0, theme::ACCENT_HOVER())
    } else {
        egui::Stroke::new(1.0, theme::BORDER())
    };
    painter.rect_stroke(img, 0.0, stroke, egui::StrokeKind::Inside);
    if selected {
        // The active brush: an accent check badge in the corner, readable on any thumbnail.
        let badge = egui::pos2(img.right() - 11.0, img.top() + 11.0);
        painter.circle_filled(badge, 8.0, theme::ACCENT());
        painter.text(badge, egui::Align2::CENTER_CENTER, crate::icons::CHECK, theme::font(11.0), Color32::WHITE);
    }
    let color = if selected || resp.hovered() { theme::TEXT() } else { theme::TEXT_DIM() };
    let mut job = egui::text::LayoutJob::simple(name.to_string(), theme::font(11.0), color, side);
    job.wrap.max_rows = 1;
    let galley = painter.layout_job(job);
    let text_pos = egui::pos2(rect.center().x - galley.size().x / 2.0, img.bottom() + 5.0);
    painter.galley(text_pos, galley, color);
    resp.on_hover_text(tooltip).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Height of a List view row.
pub const LIST_ROW: f32 = 30.0;

/// List view: one full-width row per asset, a small thumbnail then the name.
fn list_row(ui: &mut Ui, thumb: Option<&egui::TextureHandle>, name: &str, selected: bool, tooltip: &str) -> egui::Response {
    let width = ui.max_rect().width().max(40.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, LIST_ROW), egui::Sense::click_and_drag());
    if resp.dragged() && egui::DragAndDrop::has_payload_of_type::<ShelfDrag>(ui.ctx()) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 2.0, theme::ACCENT_SOFT());
    } else if resp.hovered() {
        painter.rect_filled(rect, 2.0, theme::BG_3());
    }
    let img = egui::Rect::from_min_size(rect.min + egui::vec2(3.0, 3.0), egui::vec2(LIST_ROW - 6.0, LIST_ROW - 6.0));
    painter.rect_filled(img, 0.0, theme::FIELD());
    match thumb {
        Some(t) => {
            painter.image(t.id(), img, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            let icon = ui.data(|d| d.get_temp::<&'static str>(egui::Id::new("shelf_fallback_icon"))).unwrap_or(crate::icons::IMAGE);
            painter.text(img.center(), egui::Align2::CENTER_CENTER, icon, theme::font(13.0), theme::TEXT_FAINT());
        }
    }
    let color = if selected { theme::ACCENT() } else if resp.hovered() { theme::TEXT() } else { theme::TEXT_DIM() };
    let mut job = egui::text::LayoutJob::simple(name.to_string(), theme::font(11.5), color, (rect.width() - LIST_ROW - 8.0).max(1.0));
    job.wrap.max_rows = 1;
    let galley = painter.layout_job(job);
    painter.galley(egui::pos2(img.right() + 8.0, rect.center().y - galley.size().y / 2.0), galley, color);
    resp.on_hover_text(tooltip).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Right-click menu of an asset stored in the user library. Returns true if it was removed.
fn library_menu(
    resp: &egui::Response,
    lib: Option<&subcraft_app::assets::UserLibrary>,
    file: &std::path::Path,
    status: &mut Option<String>,
) -> bool {
    let mut removed = false;
    resp.context_menu(|ui| {
        if ui.button(format!("{}  Show in folder", crate::icons::FOLDER_OPEN)).clicked() {
            show_in_folder(file);
            ui.close();
        }
        if file.extension().is_some_and(|e| e.eq_ignore_ascii_case("scsmart"))
            && ui.button(format!("{}  Export smart material…", crate::icons::EXPORT)).clicked()
        {
            let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("smart material");
            if let Some(dest) = rfd::FileDialog::new().add_filter("SubCraft smart material", &["scsmart"]).set_file_name(format!("{stem}.scsmart")).save_file() {
                *status = Some(match subcraft_app::smart_import::export_smart_file(file, stem, &dest) {
                    Ok(()) => format!("Exported smart material to {}", dest.display()),
                    Err(e) => format!("Export smart material: {e}"),
                });
            }
            ui.close();
        }
        if ui.button(format!("{}  Remove from library", crate::icons::TRASH)).clicked() {
            match lib.map(|l| l.remove(file)) {
                Some(Ok(())) => removed = true,
                Some(Err(e)) => *status = Some(e),
                None => {}
            }
            ui.close();
        }
    });
    removed
}

/// File > Import smart material(s)…: `.scsmart` files and/or PBR texture images.
pub fn import_smart_dialog(ctx: &egui::Context) {
    crate::files::pick(ctx, crate::files::Purpose::SmartMaterials, crate::files::SMART_MATERIALS, true, Some("Import smart materials (.scsmart) or PBR texture maps"));
}

/// File > Import smart material folder…: a folder of `.scsmart` files or of PBR texture sets.
pub fn import_smart_folder_dialog(ctx: &egui::Context) {
    crate::files::pick_folder(ctx, crate::files::Purpose::SmartMaterialFolder, "Import smart materials from a folder (.scsmart files or PBR texture sets)");
}

/// Files dropped on the shelf: smart materials (`.scsmart`, folders) on any tab, textures too on
/// the Smart Materials tab. Draws a hint while files hover over it.
fn handle_drops(state: &mut UiState, ui: &Ui, now: f64) {
    let rect = ui.max_rect();
    // Not where the Import resources window floats over the panel: files dropped there are the
    // window's (QA 2026-11: on Smart Materials they were also imported straight away).
    let over = ui.ctx().input(|i| i.pointer.latest_pos()).is_some_and(|p| rect.contains(p) && !state.import.covers(p));
    if over && ui.ctx().input(|i| !i.raw.hovered_files.is_empty()) {
        ui.painter().rect_stroke(rect.shrink(2.0), 4.0, egui::Stroke::new(2.0, theme::ACCENT()), egui::StrokeKind::Inside);
    }
    let dropped: Vec<std::path::PathBuf> = ui.ctx().input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
    if !over || dropped.is_empty() {
        return;
    }
    state.drops_taken = true;
    let paths: Vec<std::path::PathBuf> = dropped
        .into_iter()
        .filter(|p| !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("spsm")))
        .collect();
    if paths.is_empty() {
    } else if state.shelf.tab == ShelfTab::SmartMaterials && state.shelf.extra_tabs.is_empty() {
        // On Smart Materials, .scsmart files, folders and PBR texture sets become smart materials.
        crate::files::import_smart_materials(state, &paths, now);
    } else {
        // Painter: files dropped on the Assets panel open Import resources with them listed.
        crate::import_resources::open_with(state, &paths);
    }
    // Painter smart materials go onto the selected texture set, as when dropped on the viewport.
    for path in ui.ctx().input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect::<Vec<_>>()) {
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("spsm")) {
            crate::files::import_painter_smart_material(state, &path, now);
        }
    }
}

/// Open the system file browser at `path` (selecting it when it is a file).
pub fn show_in_folder(path: &std::path::Path) {
    #[cfg(windows)]
    let r = if path.is_dir() {
        std::process::Command::new("explorer").arg(path).spawn()
    } else {
        std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn()
    };
    #[cfg(not(windows))]
    let r = std::process::Command::new(if cfg!(target_os = "macos") { "open" } else { "xdg-open" })
        .arg(if path.is_dir() { path } else { path.parent().unwrap_or(path) })
        .spawn();
    if let Err(e) = r {
        log::warn!("could not open {}: {e}", path.display());
    }
}

type ShelfAction = Box<dyn FnOnce(&mut UiState)>;

/// What the right-click menu of a preset asked for.
#[derive(Clone, Debug, PartialEq)]
pub enum PresetAction {
    /// Overwrite the preset with the current tool ("Update from current tool").
    Update,
    Rename(String),
    Duplicate,
    /// Add to / remove from the favorites.
    Favorite,
    Delete,
}

/// Favorites key of a preset.
pub fn preset_favorite_key(name: &str) -> String {
    subcraft_app::brush_preset::favorite_key("brush", name)
}

/// Shelf order of the presets: favorites first, each group in library order.
pub fn preset_order(presets: &[subcraft_app::brush_preset::BrushPreset], favorites: &subcraft_app::brush_preset::Favorites) -> Vec<usize> {
    let fav = |i: &usize| favorites.contains(&preset_favorite_key(&presets[*i].name));
    let mut order: Vec<usize> = (0..presets.len()).filter(fav).collect();
    order.extend((0..presets.len()).filter(|i| !fav(i)));
    order
}

/// Right-click menu of a preset tile. Built-in presets can only be duplicated or made a favorite.
fn preset_menu(resp: &egui::Response, p: &subcraft_app::brush_preset::BrushPreset, favorite: bool, has_library: bool) -> Option<PresetAction> {
    let mut action = None;
    let user = p.user_file.is_some();
    resp.context_menu(|ui| {
        ui.label(RichText::new(format!("{} · {}", p.name, if user { p.kind.label() } else { "built-in preset" })).small().color(theme::TEXT_FAINT()));
        ui.separator();
        if ui.add_enabled(user, egui::Button::new(format!("{}  Update from current tool", crate::icons::SAVE)))
            .on_hover_text("Overwrite this preset with what the tool Properties show now")
            .on_disabled_hover_text("Built-in presets cannot be changed: duplicate it first")
            .clicked()
        {
            action = Some(PresetAction::Update);
            ui.close();
        }
        ui.add_enabled_ui(user, |ui| {
            ui.menu_button("Rename", |ui| {
                let id = egui::Id::new(("preset-rename", &p.name));
                let mut name = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| p.name.clone());
                let edit = ui.add(egui::TextEdit::singleline(&mut name).desired_width(180.0));
                let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.data_mut(|d| d.insert_temp(id, name.clone()));
                if (ui.button("Rename").clicked() || enter) && !name.trim().is_empty() {
                    ui.data_mut(|d| d.remove::<String>(id));
                    action = Some(PresetAction::Rename(name));
                    ui.close();
                }
            });
        });
        if ui.add_enabled(has_library, egui::Button::new(format!("{}  Duplicate", crate::icons::COPY))).clicked() {
            action = Some(PresetAction::Duplicate);
            ui.close();
        }
        if ui.button(if favorite { "Remove from favorites" } else { "Add to favorites" }).clicked() {
            action = Some(PresetAction::Favorite);
            ui.close();
        }
        if let Some(file) = &p.user_file {
            ui.separator();
            if ui.button(format!("{}  Show in folder", crate::icons::FOLDER_OPEN)).clicked() {
                show_in_folder(file);
                ui.close();
            }
            if ui.button(format!("{}  Delete", crate::icons::TRASH)).clicked() {
                action = Some(PresetAction::Delete);
                ui.close();
            }
        }
    });
    action
}

/// Carry out a preset menu action on `state.brush_presets[index]`.
pub fn apply_preset_action(state: &mut UiState, index: usize, action: PresetAction, now: f64) {
    use subcraft_app::brush_preset as bp;
    let Some(preset) = state.brush_presets.get(index).cloned() else { return };
    let key = preset_favorite_key(&preset.name);
    if action == PresetAction::Favorite {
        let on = state.favorites.toggle(&key);
        if let Some(lib) = &state.user_library
            && let Err(e) = state.favorites.save(lib)
        {
            state.set_status(format!("Favorites: {e}"), now);
            return;
        }
        state.set_status(format!("\"{}\" {}", preset.name, if on { "added to favorites" } else { "removed from favorites" }), now);
        return;
    }
    let Some(lib) = state.user_library.clone() else {
        state.set_status("Presets need a user library folder", now);
        return;
    };
    let result: Result<String, String> = match action {
        PresetAction::Update => state.session.update_preset(&lib, &preset, state.tool.preset_tool()).map(|(p, tip)| {
            if let Some(tip) = tip { state.library.alphas.push(tip); }
            let msg = format!("Updated {} \"{}\" from the current tool", p.kind.label(), p.name);
            state.brush_presets[index] = p;
            msg
        }),
        PresetAction::Rename(name) => bp::rename_brush_preset(&lib, &preset, &name).map(|p| {
            state.favorites.rename(&key, &preset_favorite_key(&p.name));
            let _ = state.favorites.save(&lib);
            if state.selected_brush.as_ref().is_some_and(|(n, _)| *n == preset.name) {
                state.selected_brush = state.selected_brush.take().map(|(_, tip)| (p.name.clone(), tip));
            }
            let msg = format!("Renamed \"{}\" to \"{}\"", preset.name, p.name);
            state.brush_presets[index] = p;
            msg
        }),
        PresetAction::Duplicate => bp::duplicate_brush_preset(&lib, &preset).map(|p| {
            let msg = format!("Duplicated \"{}\" as \"{}\"", preset.name, p.name);
            state.brush_presets.insert(index + 1, p);
            msg
        }),
        PresetAction::Delete => match &preset.user_file {
            Some(file) => lib.remove(file).map(|()| {
                state.favorites.remove(&key);
                let _ = state.favorites.save(&lib);
                state.brush_presets.remove(index);
                format!("Deleted preset \"{}\"", preset.name)
            }),
            None => Err("built-in presets cannot be deleted".into()),
        },
        PresetAction::Favorite => unreachable!("handled above"),
    };
    state.set_status(result.unwrap_or_else(|e| e), now);
}

/// Small five-point star in the corner of a favorite's tile.
fn favorite_badge(painter: &egui::Painter, tile: egui::Rect) {
    let c = egui::pos2(tile.left() + 11.0, tile.top() + 11.0);
    let points: Vec<egui::Pos2> = (0..10).map(|k| {
        let r = if k % 2 == 0 { 6.5 } else { 2.8 };
        let a = std::f32::consts::TAU * k as f32 / 10.0 - std::f32::consts::FRAC_PI_2;
        c + egui::vec2(a.cos(), a.sin()) * r
    }).collect();
    painter.circle_filled(c, 8.5, Color32::from_black_alpha(150));
    // A star is concave: fan it from the center.
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(c, theme::ACCENT());
    for p in &points { mesh.colored_vertex(*p, theme::ACCENT()); }
    for k in 0..10u32 { mesh.add_triangle(0, 1 + k, 1 + (k + 1) % 10); }
    painter.add(egui::Shape::mesh(mesh));
}

/// Brush tiles: a click activates the brush; the second click of a double-click opens its
/// Brush settings instead (the first click already selected it, so no duplicate notice).
fn brush_click(resp: &egui::Response, ctx: &egui::Context, select: impl FnOnce(&mut UiState) + 'static) -> Option<ShelfAction> {
    if resp.double_clicked() {
        let ctx = ctx.clone();
        Some(Box::new(move |s: &mut UiState| { s.open_brush_editor = true; ctx.request_repaint(); }))
    } else if resp.clicked() {
        let ctx = ctx.clone();
        Some(Box::new(move |s: &mut UiState| { select(s); ctx.request_repaint(); }))
    } else { None }
}

impl ShelfUi {
    /// Thumbnail of an asset, once the Shelf has rendered it.
    pub(crate) fn thumb(&self, id: &AssetId) -> Option<&egui::TextureHandle> {
        self.thumbs.get(id)
    }
}

/// Thumbnail of a brush tip by name, once the Shelf has rendered it.
pub(crate) fn tip_thumbnail(state: &UiState, tip: &str) -> Option<egui::TextureId> {
    state.library.alphas.iter().find(|a| a.name == tip).and_then(|a| state.shelf.thumbs.get(&a.id)).map(|t| t.id())
}

/// Underlined text tab.
/// Icon of the Normal Maps tab. The icon font has no bumpy-surface glyph, so it is drawn: a
/// rounded tile with a wavy relief line, in the weight of the font icons.
const BUMPS: &str = "bumps-icon";
/// Icon of the Alphas type, drawn too: a soft round brush tip (bright centre fading out).
const ALPHA: &str = "alpha-icon";

fn draw_alpha(painter: &egui::Painter, center: egui::Pos2, color: Color32) {
    for (r, a) in [(7.0, 0.25), (5.0, 0.5), (3.0, 1.0)] {
        painter.circle_filled(center, r, color.gamma_multiply(a));
    }
}

/// A type icon: a font glyph, or one of the drawn ones.
fn draw_category_icon(painter: &egui::Painter, center: egui::Pos2, icon: &str, color: Color32) {
    match icon {
        BUMPS => draw_bumps(painter, center, color),
        ALPHA => draw_alpha(painter, center, color),
        glyph => {
            painter.text(center, egui::Align2::CENTER_CENTER, glyph, theme::font(16.0), color);
        }
    }
}

fn draw_bumps(painter: &egui::Painter, center: egui::Pos2, color: Color32) {
    let stroke = egui::Stroke::new(1.5, color);
    painter.rect_stroke(egui::Rect::from_center_size(center, egui::vec2(14.0, 14.0)), 3.0, stroke, egui::StrokeKind::Inside);
    let wave: Vec<egui::Pos2> = (0..=10).map(|i| {
        let t = i as f32 / 10.0;
        egui::pos2(center.x - 5.0 + 10.0 * t, center.y + 2.0 * (t * std::f32::consts::TAU * 1.5).sin())
    }).collect();
    painter.add(egui::Shape::line(wave, stroke));
}

fn shelf_tab(ui: &mut Ui, current: &mut ShelfTab, value: ShelfTab, icon: &str, label: &str) {
    let selected = *current == value;
    let drawn = icon == BUMPS;
    let text = if drawn { format!("      {label}") } else { format!("{icon}  {label}") };
    let color = if selected { theme::TEXT() } else { theme::TEXT_FAINT() };
    let galley = ui.painter().layout_no_wrap(text, theme::font_strong(12.0), color);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(galley.size().x + 16.0, 34.0), egui::Sense::click());
    let color = if resp.hovered() && !selected { theme::TEXT_DIM() } else { color };
    let text_pos = rect.center() - galley.size() / 2.0;
    if drawn { draw_bumps(ui.painter(), egui::pos2(text_pos.x + 7.0, rect.center().y), color); }
    ui.painter().galley_with_override_text_color(text_pos, galley, color);
    if selected {
        ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 2.0), rect.right_bottom()), 0.0, theme::ACCENT());
    }
    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        *current = value;
    }
}

fn shelf_hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(crate::icons::INFO).color(theme::TEXT_FAINT())).on_hover_text(text);
}

pub fn show(state: &mut UiState, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    handle_drops(state, ui, now);
    header(state, ui);
    env_controls(state, ui);
    filter_row(state, ui);
    generate_one_thumbnail(state, ui.ctx());
    load_pack_thumbnails(state, ui.ctx());
    let fallback = category_icon(state.shelf.tab);
    ui.data_mut(|d| d.insert_temp(egui::Id::new("shelf_fallback_icon"), fallback));
    let mut action: Option<Box<dyn FnOnce(&mut UiState)>> = None;
    let mut lib_status: Option<String> = None;
    let search = Search::of(state);
    let filter = search.nav.filter.clone();
    let tabs = search.nav.tabs();
    // What each selected type shows: its own assets, and the starter pack's.
    let visible: HashMap<ShelfTab, (std::collections::HashSet<usize>, std::collections::HashSet<usize>)> = tabs
        .iter()
        .map(|&tab| {
            let (starters, own): (Vec<Item>, Vec<Item>) = items(state, tab).into_iter().filter(|it| search.shows(it)).partition(|it| it.starter);
            (tab, (own.iter().map(|it| it.index).collect(), starters.iter().map(|it| it.index).collect()))
        })
        .collect();
    let list_view = state.recovery.prefs.shelf_list_view;
    ui.data_mut(|d| d.insert_temp(egui::Id::new("shelf_list_view"), list_view));
    let mut shown = 0usize;
    // Alt at the moment of the click: apply in place to the selected layer instead of adding one.
    let alt = ui.input(|i| i.modifiers.alt);
    let grid_width = ui.available_width();
    let tile_size = (state.recovery.prefs.shelf_preview_size.clamp(48, 160) as f32).min((grid_width - 2.0 * f32::from(theme::PAD)).max(1.0));
    let grid_height = (ui.available_height() - 56.0).max(0.0);
    ui.allocate_ui_with_layout(egui::vec2(grid_width, grid_height), egui::Layout::top_down(egui::Align::Min), |ui| {
    egui::ScrollArea::vertical().id_salt("shelf").auto_shrink([false, false]).min_scrolled_height(0.0).max_height(grid_height).show(ui, |ui| {
        egui::Frame::new().inner_margin(egui::Margin::same(theme::PAD)).show(ui, |ui| ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
            ui.set_max_width((grid_width - 2.0 * f32::from(theme::PAD)).max(0.0));
            for tab in tabs.iter().copied() {
            let (own, starters) = &visible[&tab];
            let pass = |i: usize| own.contains(&i);
            // Several types: a small heading names each one.
            if tabs.len() > 1 && (!own.is_empty() || !starters.is_empty()) {
                ui.end_row();
                ui.label(RichText::new(category_name(tab)).font(theme::font_strong(11.0)).color(theme::TEXT_DIM()));
                ui.end_row();
            }
            ui.data_mut(|d| d.insert_temp(egui::Id::new("shelf_fallback_icon"), category_icon(tab)));
            // Alphas: the user's and built-in tips come first, the CC0 starter tips after them.
            if tab != ShelfTab::Alphas { shown += crate::asset_preview::shelf(state, ui, tile_size, tab, starters); }
            let thumbs = &state.shelf.thumbs;
            match tab {
                ShelfTab::Textures | ShelfTab::NormalMaps => {
                    for (i, t) in state.textures.items.iter().enumerate() {
                        if !pass(i) {
                            continue;
                        }
                        let normals = tab == ShelfTab::NormalMaps;
                        shown += 1;
                        // Normal maps and height maps are stamp brushes on a click (Substance Painter:
                        // a click on a brush resource makes it the active brush).
                        let stamp = if normals { Some(subcraft_paint::StampKind::Normal) } else if t.category == subcraft_core::ChannelKind::Height.display_name() { Some(subcraft_paint::StampKind::Height) } else { None };
                        let selected = stamp.is_some_and(|k| state.session.brush.material.stamp.as_ref().is_some_and(|s| s.kind == k && s.name == t.name));
                        let tip = match stamp {
                            Some(subcraft_paint::StampKind::Normal) => format!("{} (normal map): click to use it as the normal-map brush, double-click to preview and adjust. Drag onto a channel, the model or a layer. Alt+click: use it in the active channel of the selected fill layer", t.name),
                            Some(subcraft_paint::StampKind::Height) => format!("{} (height map): click to use it as the height brush, double-click to preview and adjust. Drag onto a channel, the model or a layer. Alt+click: use it in the active channel of the selected fill layer", t.name),
                            None => format!("{} ({}): drag onto a channel, the model or a layer. Alt+click: use it in the active channel of the selected fill layer", t.name, t.category),
                        };
                        let resp = tile(ui, tile_size, t.thumb.as_ref(), &t.name, selected, &tip);
                        resp.dnd_set_drag_payload(ShelfDrag::Texture(i));
                        if library_menu(&resp, state.user_library.as_ref(), &t.path, &mut lib_status) {
                            let path = t.path.clone();
                            action = Some(Box::new(move |s: &mut UiState| s.textures.remove(&path)));
                        }
                        if resp.clicked() && alt {
                            action = Some(Box::new(move |s: &mut UiState| apply_shelf_item_with(s, ShelfDrag::Texture(i), true, now)));
                        } else if let Some(kind) = stamp.filter(|_| resp.clicked() && !resp.double_clicked()) {
                            let name=t.name.clone();let path=t.path.clone();let ctx=ui.ctx().clone();let thumb=t.thumb_bitmap.clone();
                            let image_kind = if kind == subcraft_paint::StampKind::Normal { subcraft_assets::starter::Kind::Normal } else { subcraft_assets::starter::Kind::Alpha };
                            action = Some(Box::new(move |s: &mut UiState| {
                                let UiState { asset_preview, session, .. } = s;
                                asset_preview.start_stamp(&ctx,session,name,image_kind,crate::asset_preview::Source::File(path),kind,thumb);
                                s.tool = crate::state::Tool::Brush;
                                s.show_selected_brush(None);
                            }));
                        } else if resp.clicked() {
                            let name=t.name.clone();let path=t.path.clone();let ctx=ui.ctx().clone();
                            let kind=if t.category=="Normal" {subcraft_assets::starter::Kind::Normal} else {subcraft_assets::starter::Kind::Texture};
                            action = Some(Box::new(move |s: &mut UiState| s.asset_preview.start(&ctx,name,kind,crate::asset_preview::Source::File(path),true)));
                        }
                    }
                }
                ShelfTab::Materials => {
                    for (i, m) in state.library.materials.iter().enumerate() {
                        if !pass(i) {
                            continue;
                        }
                        shown += 1;
                        let resp = tile(ui, tile_size, thumbs.get(&m.id), &m.name, false, &format!("{} ({}): click or drag onto the model / a layer (adds a layer). Alt+click: replace the selected fill layer's material", m.name, m.category));
                        resp.dnd_set_drag_payload(ShelfDrag::Material(i));
                        if resp.clicked() {
                            action = Some(Box::new(move |s: &mut UiState| if alt { apply_shelf_item_with(s, ShelfDrag::Material(i), alt, now) } else { click_shelf_item(s, ShelfDrag::Material(i), now) }));
                        }
                    }
                    // The "SubCraft Materials" pack: each one a fill layer with all its maps.
                    for (i, m) in state.materials_pack.iter().enumerate() {
                        if !pass(PACK_INDEX + i) {
                            continue;
                        }
                        shown += 1;
                        let resp = tile(ui, tile_size, thumbs.get(&m.id), &m.name, false, &format!("{} ({}, SubCraft Materials): click or drag onto the model / a layer to add it as a fill layer with all its maps (Ctrl in the layer stack: with a black mask)", m.name, m.category));
                        resp.dnd_set_drag_payload(ShelfDrag::PackMaterial(i));
                        if resp.clicked() {
                            let material = m.clone();
                            action = Some(Box::new(move |s: &mut UiState| apply_pack_material(s, &material, now)));
                        }
                    }
                }
                ShelfTab::SmartMaterials => {
                    for (i, m) in state.library.smart_materials.iter().enumerate() {
                        let info = subcraft_app::assets::smart_material_info(&m.name);
                        if !pass(i) {
                            continue;
                        }
                        shown += 1;
                        let tip = match info {
                            Some(i) => format!("{} ({}): {}. Click or drag onto the model / a layer", m.name, i.category, i.description),
                            None => format!("{}: click or drag onto the model / a layer", m.name),
                        };
                        let resp = tile(ui, tile_size, thumbs.get(&m.id), &m.name, false, &tip);
                        resp.dnd_set_drag_payload(ShelfDrag::SmartMaterial(i));
                        if let Some(file) = &m.user_file
                            && library_menu(&resp, state.user_library.as_ref(), file, &mut lib_status)
                        {
                            action = Some(Box::new(move |s: &mut UiState| {
                                s.library.smart_materials.remove(i);
                            }));
                        }
                        if resp.clicked() {
                            action = Some(Box::new(move |s: &mut UiState| apply_shelf_item_with(s, ShelfDrag::SmartMaterial(i), alt, now)));
                        }
                    }
                }
                ShelfTab::SmartMasks => {
                    for (i, m) in state.library.smart_masks.iter().enumerate() {
                        let info = subcraft_app::assets::smart_mask_info(&m.name);
                        if !pass(i) {
                            continue;
                        }
                        shown += 1;
                        let tip = match info {
                            Some(i) => format!("{} ({}): {}. Click to mask the selected layer, or drag onto a layer", m.name, i.category, i.description),
                            None => format!("{}: click to mask the selected layer, or drag onto a layer", m.name),
                        };
                        let resp = tile(ui, tile_size, thumbs.get(&m.id), &m.name, false, &tip);
                        resp.dnd_set_drag_payload(ShelfDrag::SmartMask(i));
                        if let Some(file) = &m.user_file
                            && library_menu(&resp, state.user_library.as_ref(), file, &mut lib_status)
                        {
                            action = Some(Box::new(move |s: &mut UiState| {
                                s.library.smart_masks.remove(i);
                            }));
                        }
                        if resp.clicked() {
                            action = Some(Box::new(move |s: &mut UiState| apply_shelf_item(s, ShelfDrag::SmartMask(i), now)));
                        }
                    }
                }
                ShelfTab::Brushes => {
                    for i in preset_order(&state.brush_presets, &state.favorites) {
                        let p = &state.brush_presets[i];
                        if !pass(i) {
                            continue;
                        }
                        shown += 1;
                        let tip_id = state.library.alphas.iter().find(|a| Some(&a.name) == p.tip.as_ref() || (p.tip.is_none() && a.bitmap.is_none())).map(|a| a.id);
                        let thumb = tip_id.and_then(|id| thumbs.get(&id));
                        let tip = match subcraft_app::brush_builtin::describe(&p.name).filter(|_| p.user_file.is_none()) {
                            Some(d) => format!("{}: {d}", p.name),
            None => {
                                use subcraft_app::brush_preset::PresetKind;
                                match (p.kind, p.tool) {
                                    (PresetKind::Tool, Some(tool)) => format!("Tool preset ({}): brush, alpha, stencil and material; selecting it switches to that tool. Right-click for options", tool.label()),
                                    (PresetKind::Tool, None) => "Tool preset: brush, alpha and material (right-click for options)".to_string(),
                                    (PresetKind::Brush, _) => "Brush preset: brush parameters, alpha and stencil; the material in use is kept (right-click for options)".to_string(),
                                    (PresetKind::Material, _) => "Material preset: the material only; the brush in use is kept (right-click for options)".to_string(),
                                }
                            }
                        };
                        let favorite = state.favorites.contains(&preset_favorite_key(&p.name));
                        let resp = tile(ui, tile_size, thumb, &p.name, state.is_selected_shelf_brush(&p.name), &tip);
                        if favorite {
                            favorite_badge(ui.painter(), resp.rect);
                        }
                        if let Some(chosen) = preset_menu(&resp, p, favorite, state.user_library.is_some()) {
                            action = Some(Box::new(move |s: &mut UiState| apply_preset_action(s, i, chosen, now)));
                        }
                        if let Some(a) = brush_click(&resp, ui.ctx(), move |s: &mut UiState| {
                            let p = s.brush_presets[i].clone();
                            s.select_shelf_brush(subcraft_app::brush_preset::ShelfBrush::Preset(&p), now);
                        }) {
                            action = Some(a);
                        }
                    }
                }
                ShelfTab::Alphas => {
                    for (i, a) in state.library.alphas.iter().enumerate() {
                        if !pass(i) {
                            continue;
                        }
                        shown += 1;
                        let tip = if a.user_file.is_some() { "Brush tip (your library: right-click for options). Drag onto a layer's mask, or Alt+click, to use it as the mask shape" } else { "Brush tip. Drag onto a layer's mask, or Alt+click, to use it as the mask shape" };
                        let resp = tile(ui, tile_size, thumbs.get(&a.id), &a.name, state.is_selected_shelf_brush(&a.name), tip);
                        if a.bitmap.is_some() { resp.dnd_set_drag_payload(ShelfDrag::Alpha(i)); }
                        if let Some(file) = &a.user_file
                            && library_menu(&resp, state.user_library.as_ref(), file, &mut lib_status)
                        {
                            action = Some(Box::new(move |s: &mut UiState| {
                                s.library.alphas.remove(i);
                            }));
                        }
                        if alt && resp.clicked() {
                            action = Some(Box::new(move |s: &mut UiState| apply_shelf_item_with(s, ShelfDrag::Alpha(i), true, now)));
                        } else if let Some(a) = brush_click(&resp, ui.ctx(), move |s: &mut UiState| {
                            let a = s.library.alphas[i].clone();
                            s.select_shelf_brush(subcraft_app::brush_preset::ShelfBrush::Tip(&a), now);
                        }) {
                            action = Some(a);
                        }
                    }
                }
                ShelfTab::Filters => {
                    let selected = selected_filter(&state.session);
                    for (i, f) in subcraft_doc::Filter::ALL.into_iter().enumerate() {
                        let label = f.label();
                        if !pass(i) {
                            continue;
                        }
                        shown += 1;
                        let fits = state.session.shelf_filter_command(f).is_some();
                        let tip = if fits { "Click: add to the selected layer, or swap the selected filter to this one. Drag into the layer stack: a new filter layer (Passthrough) that filters everything below" } else { "Select a layer first (some filters work on color channels or masks only). Drag into the layer stack: a new filter layer" };
                        let resp = tile(ui, tile_size, None, label, selected.as_deref() == Some(label), tip);
                        resp.dnd_set_drag_payload(ShelfDrag::Filter(i));
                        if resp.clicked() && fits {
                            action = Some(Box::new(move |s: &mut UiState| {
                                if let Some(cmd) = s.session.shelf_filter_command(f) {
                                    s.run(vec![cmd], now);
                                }
                            }));
                        }
                    }
                }
                ShelfTab::Environments => {
                    for (i, e) in state.library.environments.iter().enumerate() {
                        if !pass(i) {
                            continue;
                        }
                        shown += 1;
                        let tip = if e.user_file.is_some() { "Viewport lighting (your library: right-click for options)" } else { "Viewport lighting" };
                        let resp = tile(ui, tile_size, thumbs.get(&e.id), &e.name, i == state.env.index, tip);
                        if let Some(file) = &e.user_file
                            && library_menu(&resp, state.user_library.as_ref(), file, &mut lib_status)
                        {
                            action = Some(Box::new(move |s: &mut UiState| {
                                s.library.environments.remove(i);
                                if s.env.index >= i {
                                    s.env.index = s.env.index.saturating_sub(1);
                                }
                                s.env.uploaded = None;
                            }));
                        }
                        resp.dnd_set_drag_payload(EnvironmentDrag(i));
                        if resp.dragged() && let Some(p) = ui.ctx().pointer_latest_pos() {
                            egui::Area::new(egui::Id::new("environment-drag-ghost")).order(egui::Order::Tooltip).fixed_pos(p + egui::vec2(14.0, 10.0)).interactable(false).show(ui.ctx(), |ui| {
                                egui::Frame::popup(ui.style()).show(ui, |ui| {
                                    ui.label(RichText::new(&e.name).color(theme::TEXT()));
                                    ui.label(RichText::new(tr("Drop on the 3D view to light the model with it")).small().color(theme::TEXT_FAINT()));
                                });
                            });
                        }
                        if resp.clicked() {
                            action = Some(Box::new(move |s: &mut UiState| s.env.index = i));
                        }
                    }
                }
            }
            if tab == ShelfTab::Alphas { shown += crate::asset_preview::shelf(state, ui, tile_size, tab, starters); }
            }
            if shown == 0 {
                let msg = if filter.is_active() || !search.nav.paths.is_empty() { "No matches. Clear the search or pick another filter." } else { "Nothing here yet." };
                ui.label(RichText::new(msg).color(theme::TEXT_FAINT()));
            }
        }));
    });
    });
    bottom_toolbar(state, ui);
    if let Some(a) = action {
        a(state);
    }
    if let Some(msg) = lib_status {
        state.set_status(msg, now);
    }
    if !matches!(state.shelf.tab, ShelfTab::Brushes | ShelfTab::Alphas) || state.shelf.thumbs.len() < state.library.alphas.len() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
    }
}

/// Painter's category icon row, in its order. The shelf has no filter, generator or text assets
/// (those are made from the layer menus), so those categories are not listed.
const CATEGORIES: [(ShelfTab, &str, &str); 9] = [
    (ShelfTab::Materials, crate::icons::DROP, "Materials"),
    (ShelfTab::SmartMaterials, crate::icons::SPARKLE, "Smart Materials"),
    (ShelfTab::SmartMasks, crate::icons::MASK, "Smart Masks"),
    (ShelfTab::Filters, crate::icons::SLIDERS, "Filters"),
    (ShelfTab::Brushes, crate::icons::PAINT_BRUSH, "Brushes"),
    (ShelfTab::Alphas, ALPHA, "Alphas"),
    (ShelfTab::Textures, crate::icons::IMAGE, "Textures"),
    (ShelfTab::NormalMaps, BUMPS, "Normal Maps"),
    (ShelfTab::Environments, crate::icons::SUN, "Environments"),
];

fn category_icon(tab: ShelfTab) -> &'static str {
    match tab {
        ShelfTab::NormalMaps => crate::icons::GRID,
        ShelfTab::Alphas => crate::icons::PAINT_BRUSH,
        other => CATEGORIES.iter().find(|c| c.0 == other).map_or(crate::icons::IMAGE, |c| c.1),
    }
}

fn category_name(tab: ShelfTab) -> &'static str {
    match tab {
        ShelfTab::Materials => tr("Materials"),
        ShelfTab::SmartMaterials => tr("Smart Materials"),
        ShelfTab::SmartMasks => tr("Smart Masks"),
        ShelfTab::Filters => tr("Filters"),
        ShelfTab::Brushes => tr("Brushes"),
        ShelfTab::Alphas => tr("Alphas"),
        ShelfTab::Textures => tr("Textures"),
        ShelfTab::NormalMaps => tr("Normal Maps"),
        ShelfTab::Environments => tr("Environments"),
    }
}

/// Breadcrumbs, search box (with the tag of an applied saved search), the row of type icons
/// (Ctrl+click selects several) and the optional Filter by path / Saved searches sections.
fn header(state: &mut UiState, ui: &mut Ui) {
    let search = Search::of(state);
    let places = locations(state, &search);
    let mut nav = search.nav.clone();
    let ctrl = ui.input(|i| i.modifiers.command);
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        breadcrumbs(ui, &mut nav, &places);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(6.0);
            let mut closed = false;
            if let Some(id) = state.shelf.drawing_sub_library
                && crate::widgets::icon_button(ui, crate::icons::X, tr("Close this sub-library")).clicked()
            {
                state.shelf.close_sub_library(id);
                closed = true;
            }
            if !closed
                && let Some(id) = state.shelf.drawing_sub_library
                && crate::widgets::icon_button(ui, crate::icons::UNDO, tr("Reset: back to the search this tab was made from")).clicked()
                && let Some(s) = state.shelf.sub_libraries.iter().find(|s| s.id == id)
            {
                let (name, query) = (s.name.clone(), s.query.clone());
                nav.apply_query(&query);
                nav.saved = Some(name);
            }
            section_toggle(ui, &mut state.shelf.show_saved, crate::icons::SAVE, tr("Saved searches"));
            section_toggle(ui, &mut state.shelf.show_paths, crate::icons::FOLDER, tr("Filter by path"));
        });
    });
    ui.add_space(4.0);
    egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 0)).show(ui, |ui| {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
            let stroke = egui::Stroke::new(1.6, theme::TEXT_DIM());
            ui.painter().circle_stroke(r.center() - egui::vec2(1.5, 1.5), 5.0, stroke);
            ui.painter().line_segment([r.center() + egui::vec2(2.0, 2.0), r.center() + egui::vec2(6.5, 6.5)], stroke);
            // The saved search in use, as a tag: hover shows what it holds, a click drops it.
            if let Some(name) = nav.saved.clone() {
                let tag = egui::Button::new(RichText::new(format!("{name}  {}", crate::icons::X)).font(theme::font(11.0)).color(theme::ACCENT())).fill(theme::ACCENT_SOFT()).stroke(egui::Stroke::NONE);
                let query = nav.query();
                if ui.add(tag).on_hover_text(format!("{}\n{query}", tr("Saved search (click to clear it):"))).clicked() {
                    nav = ShelfNav::new(nav.tab);
                }
            }
            ui.add(egui::TextEdit::singleline(&mut nav.filter.query).hint_text(tr("Search…")).desired_width(ui.available_width() - 28.0))
                .on_hover_text(tr("n: name, s: library, p: path, u: usage. Suffix ! contains, = equal, ^ starts, & ends, # regex; - excludes; quotes keep spaces"));
            if !nav.filter.query.is_empty() && crate::widgets::icon_button(ui, crate::icons::X, "Clear search").clicked() {
                nav.filter.query.clear();
            }
        });
    });
    ui.add_space(4.0);
    egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 0)).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            let selected_tabs = nav.tabs();
            for (tab, icon, _) in CATEGORIES {
                let selected = selected_tabs.contains(&tab);
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(28.0, 26.0), egui::Sense::click());
                let painter = ui.painter();
                if selected {
                    painter.rect_filled(rect, 2.0, theme::BG_4());
                } else if resp.hovered() {
                    painter.rect_filled(rect, 2.0, theme::BG_3());
                }
                let color = if selected { theme::TEXT() } else { theme::TEXT_DIM() };
                draw_category_icon(painter, rect.center(), icon, color);
                let hint = format!("{}\n{}", category_name(tab), tr("Ctrl+click: show several types"));
                if resp.on_hover_text(hint).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    nav.click_type(tab, ctrl);
                }
            }
        });
    });
    ui.add_space(4.0);
    if state.shelf.show_paths {
        filter_by_path(state, ui, &mut nav, &places, ctrl);
    }
    if state.shelf.show_saved {
        saved_searches(state, ui, &mut nav);
    }
    // A saved search stays tagged only while the search still is that one.
    if let Some(name) = nav.saved.clone() {
        let query = nav.query();
        let saved = state.recovery.prefs.shelf_saved_searches.iter().any(|s| s.name == name && same_search(&s.query, &query));
        let tab = state.shelf.drawing_sub_library.and_then(|id| state.shelf.sub_libraries.iter().find(|s| s.id == id)).is_some_and(|s| s.name == name && same_search(&s.query, &query));
        if !saved && !tab {
            nav.saved = None;
        }
    }
    state.shelf.set_nav(nav);
}

/// The Shelf still shows saved query `saved`: same folders and text, and the same types unless
/// the saved query names none (then it leaves the types as they were).
pub fn same_search(saved: &str, now: &str) -> bool {
    use crate::panels::shelf_filter::decompose;
    let (mut a, mut b) = (decompose(saved), decompose(now));
    for p in [&mut a, &mut b] {
        p.usages.sort();
        p.paths.sort();
    }
    (a.usages.is_empty() || a.usages == b.usages) && a.paths == b.paths && a.text.trim() == b.text.trim()
}

/// A small toggle button for an optional section of the filter area.
fn section_toggle(ui: &mut Ui, on: &mut bool, icon: &str, label: &str) {
    let text = RichText::new(icon).font(theme::font(13.0)).color(if *on { theme::ACCENT() } else { theme::TEXT_DIM() });
    let b = egui::Button::new(text).fill(if *on { theme::ACCENT_SOFT() } else { Color32::TRANSPARENT }).stroke(egui::Stroke::NONE);
    if ui.add(b).on_hover_text(label).clicked() {
        *on = !*on;
    }
}

/// Library and folder of a selected path (lowercased, see [`Location::path`]) as the places name
/// them. Compared lowercased like the path: an ASCII-only comparison missed a library named
/// with other letters ("Été"), and the breadcrumbs showed "All libraries" (QA 2026-11).
fn crumb_place(path: &str, places: &[Location]) -> (Option<String>, Option<String>) {
    let p = path.trim_matches('/');
    let (l, f) = p.split_once('/').map_or((p, None), |(l, f)| (l, Some(f)));
    let library = places.iter().find(|x| x.library.to_lowercase() == l.to_lowercase()).map(|x| x.library.clone());
    let folder = f.map(|f| {
        library.as_deref()
            .and_then(|lib| places.iter().find(|x| x.library == lib && x.folder.as_deref().is_some_and(|x| x.to_lowercase() == f.to_lowercase())))
            .and_then(|x| x.folder.clone())
            .unwrap_or_else(|| f.to_string())
    });
    (library, folder)
}

/// `All libraries ▸ library ▸ folder`: each crumb goes back to that level, each arrow lists the
/// places below it (greyed when none of their assets is of a selected type).
fn breadcrumbs(ui: &mut Ui, nav: &mut ShelfNav, places: &[Location]) {
    use crate::panels::shelf_filter::library_name;
    let current = (nav.paths.len() == 1).then(|| nav.paths[0].clone());
    let (library, folder): (Option<String>, Option<String>) = match current.as_deref() {
        Some(p) => crumb_place(p, places),
        None => (None, None),
    };
    let crumb = |ui: &mut Ui, text: &str, last: bool| {
        let color = if last { theme::TEXT() } else { theme::TEXT_DIM() };
        let font = if last { theme::font_strong(12.0) } else { theme::font(12.0) };
        ui.add(egui::Label::new(RichText::new(text).font(font).color(color)).sense(egui::Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
    };
    if crumb(ui, tr("All libraries"), library.is_none()) {
        nav.paths.clear();
    }
    crumb_menu(ui, "root", places.iter().filter(|p| p.folder.is_none() && p.assets > 0).collect(), nav, |p| library_name(&p.library).to_string());
    if let Some(lib) = library.as_deref() {
        if crumb(ui, library_name(lib), folder.is_none()) {
            nav.paths = vec![crate::panels::shelf_filter::folder_path(lib, None).to_lowercase()];
        }
        crumb_menu(ui, lib, places.iter().filter(|p| p.library == lib && p.folder.is_some()).collect(), nav, |p| p.folder.clone().unwrap_or_default());
        if let Some(f) = &folder {
            crumb(ui, f, true);
        }
    } else if nav.paths.len() > 1 {
        ui.label(RichText::new(format!("({} {})", nav.paths.len(), tr("places"))).font(theme::font(11.0)).color(theme::TEXT_FAINT()));
    }
}

/// The arrow after a crumb and the menu of the places below it.
fn crumb_menu(ui: &mut Ui, id: &str, children: Vec<&Location>, nav: &mut ShelfNav, label: impl Fn(&Location) -> String) {
    let resp = ui.add(egui::Label::new(RichText::new(crate::icons::CARET_RIGHT).font(theme::font(11.0)).color(theme::TEXT_DIM())).sense(egui::Sense::click()));
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    egui::Popup::menu(&resp).id(ui.make_persistent_id(("shelf-crumb", id))).show(|ui| {
        if children.is_empty() {
            ui.label(RichText::new(tr("No folders")).color(theme::TEXT_FAINT()));
        }
        for place in children {
            let color = if place.applicable > 0 { theme::TEXT() } else { theme::TEXT_FAINT() };
            if ui.button(RichText::new(label(place)).color(color)).clicked() {
                nav.paths = vec![place.path()];
                ui.close();
            }
        }
    });
}

/// Filter by path: libraries and their folders. Click selects one place, Ctrl+click several;
/// Shift+click on a library's arrow folds or unfolds every library.
fn filter_by_path(state: &mut UiState, ui: &mut Ui, nav: &mut ShelfNav, places: &[Location], ctrl: bool) {
    use crate::panels::shelf_filter::library_name;
    egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 2)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("Filter by path")).font(theme::font_strong(11.0)).color(theme::TEXT_DIM()));
            ui.checkbox(&mut state.shelf.hide_empty_folders, RichText::new(tr("Hide non-applicable folders")).font(theme::font(11.0)));
        });
        let hide = state.shelf.hide_empty_folders;
        let shift = ui.input(|i| i.modifiers.shift);
        for lib in places.iter().filter(|p| p.folder.is_none() && (!hide || p.applicable > 0)) {
            let folded = state.shelf.folded.contains(&lib.library);
            ui.horizontal(|ui| {
                let caret = if folded { crate::icons::CARET_RIGHT } else { crate::icons::CARET_DOWN };
                if ui.add(egui::Label::new(RichText::new(caret).font(theme::font(11.0)).color(theme::TEXT_DIM())).sense(egui::Sense::click())).clicked() {
                    if shift {
                        // Shift: every library follows this one.
                        for l in places.iter().filter(|p| p.folder.is_none()) {
                            if folded {
                                state.shelf.folded.remove(&l.library);
                            } else {
                                state.shelf.folded.insert(l.library.clone());
                            }
                        }
                    } else if folded {
                        state.shelf.folded.remove(&lib.library);
                    } else {
                        state.shelf.folded.insert(lib.library.clone());
                    }
                }
                place_row(ui, nav, lib, library_name(&lib.library), ctrl);
            });
            if folded {
                continue;
            }
            for folder in places.iter().filter(|p| p.library == lib.library && p.folder.is_some() && (!hide || p.applicable > 0)) {
                ui.horizontal(|ui| {
                    ui.add_space(18.0);
                    place_row(ui, nav, folder, folder.folder.as_deref().unwrap_or_default(), ctrl);
                });
            }
        }
    });
    crate::widgets::divider(ui);
}

/// One library or folder of Filter by path: selected rows are highlighted; the count is what the
/// selected types and the search show there.
fn place_row(ui: &mut Ui, nav: &mut ShelfNav, place: &Location, name: &str, ctrl: bool) {
    let path = place.path();
    let selected = nav.paths.contains(&path);
    let color = if place.applicable > 0 { theme::TEXT() } else { theme::TEXT_FAINT() };
    let text = RichText::new(format!("{name}  ({})", place.applicable)).font(theme::font(11.5)).color(if selected { theme::ACCENT() } else { color });
    if ui.selectable_label(selected, text).clicked() {
        nav.click_path(path, ctrl);
    }
}

/// Saved searches: save the current one, click to apply, right-click to open it as a tab,
/// rename or delete it.
fn saved_searches(state: &mut UiState, ui: &mut Ui, nav: &mut ShelfNav) {
    let mut remove = None;
    let mut rename: Option<(usize, String)> = None;
    let mut open_tab = None;
    egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 2)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("Saved searches")).font(theme::font_strong(11.0)).color(theme::TEXT_DIM()));
            ui.menu_button(RichText::new(format!("{}  {}", crate::icons::PLUS, tr("Save current search"))).font(theme::font(11.0)), |ui| {
                let id = egui::Id::new("shelf-save-search");
                let mut name = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| default_search_name(nav));
                let edit = ui.add(egui::TextEdit::singleline(&mut name).desired_width(180.0));
                let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.data_mut(|d| d.insert_temp(id, name.clone()));
                if (ui.button(tr("Save")).clicked() || enter) && !name.trim().is_empty() {
                    ui.data_mut(|d| d.remove::<String>(id));
                    nav.saved = Some(save_search(&mut state.recovery.prefs.shelf_saved_searches, name.trim(), nav.query()));
                    ui.close();
                }
            });
        });
        if state.recovery.prefs.shelf_saved_searches.is_empty() {
            ui.label(RichText::new(tr("No saved searches yet.")).font(theme::font(11.0)).color(theme::TEXT_FAINT()));
        }
        for (i, s) in state.recovery.prefs.shelf_saved_searches.iter().enumerate() {
            let selected = nav.saved.as_deref() == Some(s.name.as_str());
            let resp = ui.selectable_label(selected, RichText::new(&s.name).font(theme::font(11.5))).on_hover_text(&s.query);
            if resp.clicked() {
                nav.apply_query(&s.query);
                nav.saved = Some(s.name.clone());
            }
            resp.context_menu(|ui| {
                if ui.button(tr("Create new tab")).clicked() {
                    open_tab = Some(i);
                    ui.close();
                }
                ui.menu_button(tr("Rename"), |ui| {
                    let id = egui::Id::new(("shelf-rename-search", i));
                    let mut name = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| s.name.clone());
                    let edit = ui.add(egui::TextEdit::singleline(&mut name).desired_width(180.0));
                    let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.data_mut(|d| d.insert_temp(id, name.clone()));
                    if (ui.button(tr("Rename")).clicked() || enter) && !name.trim().is_empty() {
                        ui.data_mut(|d| d.remove::<String>(id));
                        rename = Some((i, name.trim().to_string()));
                        ui.close();
                    }
                });
                if ui.button(format!("{}  {}", crate::icons::TRASH, tr("Delete"))).clicked() {
                    remove = Some(i);
                    ui.close();
                }
            });
        }
    });
    let saved = &mut state.recovery.prefs.shelf_saved_searches;
    if let Some((i, name)) = rename {
        let renamed = rename_search(saved, i, &name);
        if nav.saved.as_ref().is_some_and(|n| renamed.as_ref().is_some_and(|(old, _)| old == n)) {
            nav.saved = renamed.map(|(_, new)| new);
        }
    }
    if let Some(i) = remove.filter(|i| *i < saved.len()) {
        // Sub-library tabs made from it stay open (Painter keeps them until closed).
        saved.remove(i);
    }
    if let Some(s) = open_tab.and_then(|i| saved.get(i)).cloned() {
        state.shelf.add_sub_library(s.name, s.query);
    }
    crate::widgets::divider(ui);
}

/// Add a saved search named `name` (or `name 2`... when taken). Returns the name it got.
pub fn save_search(saved: &mut Vec<SavedSearch>, name: &str, query: String) -> String {
    let name = unique_search_name(saved, name);
    saved.push(SavedSearch { name: name.clone(), query });
    name
}

/// Rename saved search `i` (kept distinct from the others). Returns the old and new names.
pub fn rename_search(saved: &mut [SavedSearch], i: usize, name: &str) -> Option<(String, String)> {
    let others: Vec<SavedSearch> = saved.iter().enumerate().filter(|(k, _)| *k != i).map(|(_, s)| s.clone()).collect();
    let new = unique_search_name(&others, name);
    let s = saved.get_mut(i)?;
    let old = std::mem::replace(&mut s.name, new.clone());
    Some((old, new))
}

/// Name proposed for a new saved search: the typed text, else the selected types.
fn default_search_name(nav: &ShelfNav) -> String {
    let text = nav.filter.query.trim();
    if !text.is_empty() {
        return text.to_string();
    }
    nav.tabs().iter().map(|t| category_name(*t)).collect::<Vec<_>>().join(" + ")
}

/// `name`, or `name 2`, `name 3`... when a saved search already has it.
fn unique_search_name(saved: &[SavedSearch], name: &str) -> String {
    let taken = |n: &str| saved.iter().any(|s| s.name.eq_ignore_ascii_case(n));
    if !taken(name) {
        return name.to_string();
    }
    (2..).map(|k| format!("{name} {k}")).find(|n| !taken(n)).unwrap_or_default()
}

/// Rotation / intensity of the viewport environment (only on the Environments category).
fn env_controls(state: &mut UiState, ui: &mut Ui) {
    if state.shelf.tab != ShelfTab::Environments { return; }
    egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 2)).show(ui, |ui| {
        let width = ui.available_width();
        // Exposure is in EV (stops) like Painter's Environment Exposure: +1 doubles the light.
        for (label, value, range) in [("Rotation", &mut state.env.rotation_deg, -180.0..=180.0), ("Exposure", &mut state.recovery.prefs.viewport.display.env_exposure, -5.0..=5.0)] {
            ui.allocate_ui_with_layout(egui::vec2(width, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add_sized([60.0, 18.0], egui::Label::new(RichText::new(label).color(theme::TEXT_DIM())));
                // Room for the value box: "-180°" is the widest rotation (whole degrees).
                ui.spacing_mut().slider_width = (ui.available_width() - 72.0).max(20.0);
                let slider = egui::Slider::new(value, range);
                ui.add(if label == "Rotation" { slider.suffix("°").fixed_decimals(0) } else { slider.suffix(" EV").fixed_decimals(1) });
            });
        }
    });
}

/// Bottom icon toolbar, as Painter's: the thumbnail size menu first, then the type's import /
/// library-folder actions and the new sub-library tab button.
fn bottom_toolbar(state: &mut UiState, ui: &mut Ui) {
    use crate::widgets::icon_button;
    crate::widgets::divider(ui);
    type Pick = (crate::files::Purpose, (&'static str, &'static [&'static str]), bool);
    let mut pick: Option<Pick> = None;
    let mut folder: Option<subcraft_app::assets::UserAssetKind> = None;
    egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 3)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            size_menu(ui, &mut state.recovery.prefs);
            if icon_button(ui, crate::icons::PLUS, tr("Import resources…")).clicked() {
                state.import.open = true;
            }
            match state.shelf.tab {
                ShelfTab::Textures | ShelfTab::NormalMaps => {
                    if icon_button(ui, crate::icons::DOWNLOAD, "Import textures…").clicked() { pick = Some((crate::files::Purpose::ShelfTextures, crate::files::IMAGES, true)); }
                    folder = Some(subcraft_app::assets::UserAssetKind::Texture);
                }
                ShelfTab::Materials => {
                    if icon_button(ui, crate::icons::DOWNLOAD, tr("Select a texture's maps (color, roughness, normal…) to make one fill layer.")).clicked() { crate::panels::layers::import_texture_set_dialog(ui.ctx()); }
                    if icon_button(ui, crate::icons::ARROW_DOWN, tr("Download SubCraft Materials: a pack of tileable PBR materials (1K or 2K)")).clicked() { crate::materials_download::open(state); }
                }
                ShelfTab::Alphas => {
                    if icon_button(ui, crate::icons::DOWNLOAD, "Import alpha…").clicked() { pick = Some((crate::files::Purpose::Alpha, crate::files::ALPHAS, false)); }
                    folder = Some(subcraft_app::assets::UserAssetKind::Alpha);
                }
                ShelfTab::Environments => {
                    if icon_button(ui, crate::icons::DOWNLOAD, "Import HDRI…").clicked() { pick = Some((crate::files::Purpose::Hdri, crate::files::PANORAMAS, false)); }
                    folder = Some(subcraft_app::assets::UserAssetKind::Environment);
                }
                ShelfTab::SmartMaterials => {
                    if icon_button(ui, crate::icons::DOWNLOAD, tr("Import smart materials (.scsmart) or PBR texture sets (Color, NormalGL, Roughness…)")).clicked() { import_smart_dialog(ui.ctx()); }
                    if icon_button(ui, crate::icons::FOLDER_OPEN, tr("Import a folder of .scsmart files or PBR texture sets")).clicked() { import_smart_folder_dialog(ui.ctx()); }
                }
                ShelfTab::SmartMasks | ShelfTab::Filters | ShelfTab::Brushes => {}
            }
            if let Some(kind) = folder && let Some(lib) = &state.user_library {
                let dir = lib.folder(kind);
                if icon_button(ui, crate::icons::FOLDER, &format!("Library folder: {}\nFiles here appear in the shelf at startup.", dir.display())).clicked() {
                    if let Err(e) = std::fs::create_dir_all(&dir) { log::warn!("could not create {}: {e}", dir.display()); }
                    show_in_folder(&dir);
                }
            }
            if icon_button(ui, crate::icons::SQUARES, tr("New sub-library tab: the current search in a tab of its own")).clicked() {
                let nav = state.shelf.nav();
                let name = nav.saved.clone().unwrap_or_else(|| default_search_name(&nav));
                state.shelf.add_sub_library(name, nav.query());
            }
        });
    });
    if let Some((purpose, filter, multi)) = pick { crate::files::pick(ui.ctx(), purpose, filter, multi, None); }
}

/// Name of the thumbnail size the preferences hold (the nearest preset for an older free size).
pub fn preview_size_name(prefs: &crate::recovery::Preferences) -> &'static str {
    if prefs.shelf_list_view {
        return "List";
    }
    PREVIEW_SIZES.iter().min_by_key(|(_, s)| s.abs_diff(prefs.shelf_preview_size)).map_or("Medium", |p| p.0)
}

/// Pick a size of the size menu: Small, Medium, Large or List.
pub fn set_preview_size(prefs: &mut crate::recovery::Preferences, name: &str) {
    match PREVIEW_SIZES.iter().find(|p| p.0 == name) {
        Some((_, size)) => {
            prefs.shelf_preview_size = *size;
            prefs.shelf_list_view = false;
        }
        None => prefs.shelf_list_view = true,
    }
}

/// A size of the size menu in the interface language.
fn size_label(name: &str) -> &'static str {
    match name {
        "Small" => tr("Small"),
        "Medium" => tr("Medium"),
        "Large" => tr("Large"),
        _ => tr("List"),
    }
}

/// The thumbnail size menu (Painter's left-most bottom icon): Small, Medium (default), Large, List.
fn size_menu(ui: &mut Ui, prefs: &mut crate::recovery::Preferences) {
    let current = preview_size_name(prefs);
    let label = RichText::new(format!("{}  {}", crate::icons::GRID, size_label(current))).font(theme::font(11.0)).color(theme::TEXT_DIM());
    let resp = ui.menu_button(label, |ui| {
        for name in ["Small", "Medium", "Large", "List"] {
            if ui.radio(current == name, size_label(name)).clicked() {
                set_preview_size(prefs, name);
                ui.close();
            }
        }
    });
    resp.response.on_hover_text(tr("Thumbnail size"));
}

/// Grayscale bitmap → egui image.
fn alpha_image(b: &subcraft_image::Bitmap) -> egui::ColorImage {
    let n = THUMB as usize;
    let mut px = Vec::with_capacity(n * n);
    for y in 0..n {
        for x in 0..n {
            let t = b.sample((x as f32 + 0.5) / n as f32, (y as f32 + 0.5) / n as f32, subcraft_image::Wrap::Clamp, false);
            let v = (t[0] * t[3]).clamp(0.0, 1.0);
            let g = (40.0 + v * 215.0) as u8;
            px.push(Color32::from_gray(g));
        }
    }
    egui::ColorImage { size: [n, n], pixels: px, source_size: egui::vec2(n as f32, n as f32) }
}

/// Render one missing thumbnail of the visible tab per frame (keeps the UI responsive).
pub(crate) fn generate_one_thumbnail(state: &mut UiState, ctx: &egui::Context) {
    // Finish the thumbnail in flight first (the GPU copy is never waited on).
    state.shelf.busy = state.shelf.pending.is_some() || state.shelf.compositing.is_some();
    if let Some((id, pending)) = &state.shelf.pending {
        let Some(rs) = state.render_state.clone() else { return };
        match pending.try_take(&rs.device) {
            None => {
                ctx.request_repaint();
                return;
            }
            Some(bytes) => {
                if let Some(bytes) = bytes {
                    let img = egui::ColorImage::from_rgba_unmultiplied([RENDER_THUMB as usize, RENDER_THUMB as usize], &bytes);
                    let tex = ctx.load_texture(format!("thumb-{id}"), img, egui::TextureOptions::LINEAR);
                    state.shelf.thumbs.insert(*id, tex);
                }
                state.shelf.pending = None;
            }
        }
    }
    // A composite finished on the worker: draw it. At most one step of work per frame.
    if state.shelf.compositing.is_some() {
        let Some(rs) = state.render_state.clone() else { return };
        let message = state.shelf.compositing.as_ref().map(|c| c.rx.try_recv());
        match message {
            Some(Ok((scene, composite))) => {
                if let Some(done) = state.shelf.compositing.take() {
                    render_composited(state, ctx, &rs, done, scene, composite);
                }
            }
            Some(Err(std::sync::mpsc::TryRecvError::Empty)) => ctx.request_repaint_after(std::time::Duration::from_millis(8)),
            _ => {
                // The worker died without a result: show an empty tile instead of retrying forever.
                if let Some(done) = state.shelf.compositing.take() {
                    log::warn!("shelf thumbnail worker failed for {}", done.id);
                    let blank = egui::ColorImage::new([1, 1], vec![Color32::TRANSPARENT]);
                    state.shelf.thumbs.insert(done.id, ctx.load_texture(format!("thumb-{}", done.id), blank, egui::TextureOptions::LINEAR));
                }
            }
        }
        return;
    }
    let lib = &state.library;
    let missing = |id: &AssetId| !state.shelf.thumbs.contains_key(id);
    enum Job {
        Alpha(usize),
        /// The layer builder and the images the layer uses.
        Layer(AssetId, SceneKind, LayerBuilder, Vec<std::sync::Arc<subcraft_doc::Resource>>),
        Environment(usize),
    }
    let job = thumbnail_tabs(&state.shelf).into_iter().find_map(|tab| match tab {
        // Texture thumbnails come from their own background loader.
        ShelfTab::Textures | ShelfTab::NormalMaps | ShelfTab::Filters => None,
        // Brush presets show their tip's thumbnail.
        ShelfTab::Brushes | ShelfTab::Alphas => lib.alphas.iter().position(|a| missing(&a.id)).map(Job::Alpha),
        ShelfTab::Materials => lib.materials.iter().find(|m| missing(&m.id)).map(|m| {
            let m = m.clone();
            let (ids, images) = subcraft_assets::presets::decode_images(&m.images);
            Job::Layer(m.id, SceneKind::Sphere, Box::new(move |ts| preview_layer(m.build_with(ts, &ids))), images)
        }),
        ShelfTab::SmartMaterials => lib.smart_materials.iter().find(|m| missing(&m.id)).map(|m| {
            let m = m.clone();
            let (ids, decoded) = subcraft_assets::presets::decode_images(m.images());
            let images = m.resources().iter().cloned().chain(decoded).collect();
            Job::Layer(m.id, SceneKind::BakedSphere, Box::new(move |ts| preview_layer(m.build_with(ts, &ids))), images)
        }),
        ShelfTab::SmartMasks => lib.smart_masks.iter().find(|m| missing(&m.id)).map(|m| {
            let m = m.clone();
            Job::Layer(m.id, SceneKind::BakedSphere, Box::new(move |ts| mask_preview_layer(ts, &m.effects)), Vec::new())
        }),
        ShelfTab::Environments => lib.environments.iter().position(|e| missing(&e.id)).map(Job::Environment),
    });
    state.shelf.busy |= job.is_some();
    // The mini-shelf asks again every frame it is open.
    state.shelf.want_environment_thumbs = false;
    let Some(job) = job else { return };
    if let Job::Alpha(i) = job {
        let a = &state.library.alphas[i];
        let img = match &a.bitmap {
            Some(b) => alpha_image(b),
            None => alpha_image(&subcraft_image::Bitmap::from_fn_gray(64, 64, |u, v| {
                let d = ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt() * 2.0;
                subcraft_paint::falloff(d, 0.5)
            })),
        };
        let tex = ctx.load_texture(format!("thumb-{}", a.id), img, egui::TextureOptions::LINEAR);
        state.shelf.thumbs.insert(a.id, tex);
        return;
    }
    if state.render_state.is_none() { return; }
    let (id, kind, layer_fn, images, env_index) = match job {
        Job::Layer(id, k, f, images) => (id, k, Some(f), images, None),
        Job::Environment(i) => (state.library.environments[i].id, SceneKind::Sphere, None, Vec::new(), Some(i)),
        Job::Alpha(_) => unreachable!(),
    };
    // Composite on a worker thread: at thumbnail resolution it takes 10-90 ms, several frames.
    let existing = match kind {
        SceneKind::Sphere => state.shelf.sphere.clone(),
        SceneKind::BakedSphere => state.shelf.baked_sphere.clone(),
    };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let scene = existing.unwrap_or_else(|| Arc::new(match kind {
            SceneKind::Sphere => PreviewScene::sphere(),
            SceneKind::BakedSphere => PreviewScene::baked_sphere(),
        }));
        let composite = match &layer_fn {
            Some(f) => scene.composite_with(f(scene.texture_set()), &images),
            None => {
                // Environment preview: a glossy metal sphere.
                let ts = scene.texture_set();
                let mut l = Layer::new_fill("Chrome", ts);
                if let LayerContent::Fill(f) = &mut l.content {
                    let set = |k| FillChannel::new(FillSource::uniform(k));
                    f.channels.insert(ChannelKind::BaseColor, set(ChannelValue::rgb(0.9, 0.9, 0.9)));
                    f.channels.insert(ChannelKind::Metallic, set(ChannelValue::scalar(1.0)));
                    f.channels.insert(ChannelKind::Roughness, set(ChannelValue::scalar(0.12)));
                }
                scene.composite(l)
            }
        };
        let _ = tx.send((scene, composite));
    });
    state.shelf.compositing = Some(Compositing { id, kind, env_index, rx });
    state.shelf.busy = true;
    ctx.request_repaint_after(std::time::Duration::from_millis(8));
}

/// Second half of a thumbnail: the composite is ready, draw it (the GPU copy is never waited on).
fn render_composited(state: &mut UiState, ctx: &egui::Context, rs: &subcraft_render::egui_wgpu::RenderState, done: Compositing, scene: Arc<PreviewScene>, composite: subcraft_compositor::Composite) {
    let Compositing { id, kind, env_index, .. } = done;
    match kind {
        SceneKind::Sphere => state.shelf.sphere = Some(scene.clone()),
        SceneKind::BakedSphere => state.shelf.baked_sphere = Some(scene.clone()),
    }
    let mesh = scene.mesh.clone();
    let (renderer, current) = state.shelf.renderer.get_or_insert_with(|| (ViewportRenderer::new(&rs.device, &rs.queue), None));
    if *current != Some(kind) {
        renderer.set_mesh(&rs.device, &mesh);
        *current = Some(kind);
    }
    for k in SHADED_CHANNELS {
        if let Some(img) = composite.channel(k) {
            renderer.upload_channel(&rs.device, &rs.queue, 0, k, img, None);
        }
    }
    let eye = Vec3::new(0.0, 0.3, 3.4);
    let view = Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y);
    let mut params = ViewParams::new(view, Mat4::perspective_rh(38f32.to_radians(), 1.0, 0.1, 50.0));
    params.background = [0.035, 0.036, 0.04];
    if let Some(i) = env_index {
        let mut uploaded = None;
        apply_environment(&state.library, i, 0.0, 1.0, &mut params, renderer, &mut uploaded, &rs.device, &rs.queue);
    }
    let pending = renderer.render(&rs.device, &rs.queue, [RENDER_THUMB, RENDER_THUMB], &params).read_rgba8_async(&rs.device, &rs.queue);
    state.shelf.pending = Some((id, pending));
    ctx.request_repaint();
}

/// A material as the shelf thumbnail shows it: same layer, tiling boosted for the small sphere
/// (the saved asset is not changed).
fn preview_layer(mut layer: Layer) -> Layer {
    subcraft_app::thumbnails::zoom_out_tiling(&mut layer);
    layer
}

/// White paint shown through a smart mask (for mask thumbnails).
fn mask_preview_layer(ts: &subcraft_doc::TextureSet, effects: &[subcraft_doc::MaskEffectKind]) -> Layer {
    let mut l = Layer::new_fill("Mask preview", ts);
    if let LayerContent::Fill(f) = &mut l.content {
        for c in f.channels.values_mut() {
            c.enabled = false;
        }
        f.channels.insert(ChannelKind::BaseColor, FillChannel::new(FillSource::uniform(ChannelValue::rgb(0.95, 0.55, 0.2))));
    }
    let mut mask = Mask::new(ts.resolution, false);
    mask.effects = effects.iter().cloned().map(|k| Arc::new(MaskEffect::new(k))).collect();
    l.mask = Some(mask);
    l
}

/// "Open library folder" (creates it on first use so the user can drop files in).
fn library_folder_button(ui: &mut Ui, state: &UiState, kind: subcraft_app::assets::UserAssetKind) {
    let Some(lib) = &state.user_library else { return };
    let dir = lib.folder(kind);
    if ui
        .button(format!("{}  Library folder", crate::icons::FOLDER_OPEN))
        .on_hover_text(format!("{}\nFiles here appear in the shelf at startup.", dir.display()))
        .clicked()
    {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::warn!("could not create {}: {e}", dir.display());
        }
        show_in_folder(&dir);
    }
}

/// Search box + category chips for the visible tab (chips reset when the tab changes).
fn filter_row(state: &mut UiState, ui: &mut Ui) {
    use crate::panels::shelf_filter::{Chip, categories};
    if state.shelf.filter_tab != state.shelf.tab {
        state.shelf.filter_tab = state.shelf.tab;
        state.shelf.filter.chip = Chip::All;
    }
    let mut chips = vec![(Chip::All, "All".to_string()), (Chip::Mine, "Mine".to_string())];
    if state.shelf.tab == ShelfTab::Materials {
        for c in categories(state.library.materials.iter().map(|m| m.category)) {
            chips.push((Chip::Category(c.clone()), c));
        }
    }
    if state.shelf.tab == ShelfTab::SmartMaterials {
        for c in categories(state.library.smart_materials.iter().filter_map(|m| subcraft_app::assets::smart_material_info(&m.name)).map(|i| i.category)) {
            chips.push((Chip::Category(c.clone()), c));
        }
    }
    if state.shelf.tab == ShelfTab::SmartMasks {
        for c in categories(state.library.smart_masks.iter().filter_map(|m| subcraft_app::assets::smart_mask_info(&m.name)).map(|i| i.category)) {
            chips.push((Chip::Category(c.clone()), c));
        }
    }
    if matches!(state.shelf.tab, ShelfTab::Textures | ShelfTab::NormalMaps) {
        chips.push((Chip::Tileable,"Tileable".into()));
        let normals = state.shelf.tab == ShelfTab::NormalMaps;
        for c in categories(state.textures.items.iter().filter(|t| (t.category == "Normal") == normals).map(|t| t.category)) {
            chips.push((Chip::Category(c.clone()), c));
        }
    }
    if matches!(state.shelf.tab,ShelfTab::NormalMaps|ShelfTab::Alphas) {chips.push((Chip::Category("Starter pack".into()),"Starter pack".into()));}
    egui::Frame::new().inner_margin(egui::Margin { left: theme::PAD, right: theme::PAD, top: 4, bottom: 6 }).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            let filter = &mut state.shelf.filter;
            ui.spacing_mut().item_spacing.x = 4.0;
            for (chip, label) in chips {
                let selected = filter.chip == chip;
                let text = RichText::new(label).font(theme::font(11.5)).color(if selected { theme::ACCENT() } else { theme::TEXT_DIM() });
                let b = egui::Button::new(text).fill(if selected { theme::ACCENT_SOFT() } else { theme::BG_3() }).stroke(egui::Stroke::NONE);
                if ui.add(b).clicked() {
                    filter.chip = if selected { Chip::All } else { chip };
                }
            }
        });
    });
}

/// Label of the selected filter effect (content or mask), to highlight its Shelf tile.
fn selected_filter(session: &subcraft_app::Session) -> Option<String> {
    let layer = session.selected_layer()?;
    let id = session.selected_effect()?;
    if let Some(e) = layer.content_effects.iter().find(|e| e.id == id)
        && let subcraft_doc::ContentEffectKind::Filter { filter, .. } = &e.kind
    {
        return Some(filter.label().to_string());
    }
    let e = layer.mask.as_ref()?.effects.iter().find(|e| e.id == id)?;
    match &e.kind {
        subcraft_doc::MaskEffectKind::Filter(f) => Some(f.label().to_string()),
        _ => None,
    }
}

#[cfg(test)]
#[path = "shelf_nav_tests.rs"]
mod nav_tests;

#[cfg(test)]
mod materials_pack_tests {
    use super::*;

    #[test]
    fn a_materials_pack_folder_fills_the_materials_tab_and_applies_as_a_fill_layer() {
        let mut s = UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::cube(2.0), 128), None);
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/tests/fixtures/materials_pack");
        s.recovery.prefs.materials_pack = fixtures.display().to_string();
        s.reload_materials_pack();
        let pack: Vec<Item> = items(&s, ShelfTab::Materials).into_iter().filter(|i| i.library == MATERIALS_PACK).collect();
        let names: Vec<&str> = pack.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["Metal Test Plate", "Wood Test Planks"]);
        assert_eq!(pack[1].folder.as_deref(), Some("Wood"), "the pack's category is the folder");
        assert!(pack[1].tags.iter().any(|t| t == "oak") && pack.iter().all(|i| i.index >= PACK_INDEX && i.tileable));
        assert!(items(&s, ShelfTab::Materials).iter().any(|i| i.library != MATERIALS_PACK), "the built-in materials stay");
        // Applying one adds its fill layer.
        let before = s.session.selected_texture_set().layers.len();
        let wood = s.materials_pack.iter().find(|m| m.key == "wood_test").unwrap().clone();
        apply_pack_material(&mut s, &wood, 0.0);
        assert_eq!(s.session.selected_texture_set().layers.len(), before + 1);
        assert!(s.session.selected_texture_set().layers.iter().any(|l| l.name == "Wood Test Planks"));
        // Dropped into the layer stack with Ctrl: a new layer with a black mask, selected.
        let metal = s.materials_pack.iter().position(|m| m.key == "metal_test").unwrap();
        apply_stack_drop(&mut s, ShelfDrag::PackMaterial(metal), false, true, false, 0.0);
        let layer = s.session.selected_layer().expect("the dropped material is selected");
        assert_eq!(layer.name, "Metal Test Plate");
        assert!(layer.mask.is_some(), "Ctrl: with a black mask");
        assert_eq!(s.session.selected_texture_set().layers.len(), before + 2);
        // Alt does not replace in place; masks refuse it.
        apply_shelf_item_with(&mut s, ShelfDrag::PackMaterial(metal), true, 0.0);
        assert_eq!(s.session.selected_texture_set().layers.len(), before + 2, "Alt adds nothing");
        assert!(!ghost_mask_accepts(ShelfDrag::PackMaterial(metal)));
        assert!(drop_label(ShelfDrag::PackMaterial(metal), false, Some("Base"), false).starts_with("Add material layer above"));
        // No pack: nothing listed.
        s.recovery.prefs.materials_pack.clear();
        s.reload_materials_pack();
        assert!(items(&s, ShelfTab::Materials).iter().all(|i| i.library != MATERIALS_PACK));
    }
}

#[cfg(test)]
mod environment_tests {
    use super::*;

    #[test]
    fn the_environment_mini_shelf_asks_for_environment_thumbnails() {
        let mut shelf = ShelfUi::default();
        shelf.tab = ShelfTab::Materials;
        shelf.extra_tabs.clear();
        assert!(!thumbnail_tabs(&shelf).contains(&ShelfTab::Environments), "only the panel's types");
        shelf.want_environment_thumbs = true;
        let tabs = thumbnail_tabs(&shelf);
        assert!(tabs.contains(&ShelfTab::Environments) && tabs.contains(&ShelfTab::Materials));
        shelf.tab = ShelfTab::Environments;
        assert_eq!(thumbnail_tabs(&shelf).iter().filter(|t| **t == ShelfTab::Environments).count(), 1, "listed once");
    }

    #[test]
    fn startup_environment_is_glasshouse_lit_from_the_front() {
        let library = Library::builtin();
        let env = EnvUi::startup(&library);
        let chosen = &library.environments[env.index];
        assert_eq!(chosen.name, DEFAULT_ENVIRONMENT);
        let sun = chosen.source.hdri().expect("Glasshouse is a bundled HDRI").sun_direction;
        let wanted = rotation_for_key_light(sun);
        eprintln!("glasshouse sun {sun:?}, rotation for a front key light {wanted:.1} deg");
        let off = (DEFAULT_ENVIRONMENT_ROTATION_DEG - wanted + 180.0).rem_euclid(360.0) - 180.0;
        assert!(off.abs() < 2.0, "default rotation {DEFAULT_ENVIRONMENT_ROTATION_DEG} should be {wanted:.1}");
        assert!((-180.0..=180.0).contains(&DEFAULT_ENVIRONMENT_ROTATION_DEG), "inside the Rotation slider's range");
        // The light apply_environment derives faces the framing camera (35° yaw), never the model's back.
        let (s, c) = env.rotation_deg.to_radians().sin_cos();
        let light = Vec3::new(c * sun[0] + s * sun[2], sun[1], -s * sun[0] + c * sun[2]);
        let camera = Vec3::new(35f32.to_radians().sin(), 0.0, 35f32.to_radians().cos());
        assert!(Vec3::new(light.x, 0.0, light.z).normalize().dot(camera) > 0.7, "light {light} vs camera {camera}");
    }

    #[test]
    fn a_new_window_opens_the_shelf_on_smart_materials_with_glasshouse() {
        let state = UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::cube(2.0), 64), None);
        assert_eq!(state.shelf.tab, ShelfTab::SmartMaterials);
        assert_eq!(state.library.environments[state.env.index].name, DEFAULT_ENVIRONMENT);
        assert_eq!(state.env.rotation_deg, DEFAULT_ENVIRONMENT_ROTATION_DEG);
    }

    #[test]
    fn key_light_rotation_moves_any_sun_to_the_target_azimuth() {
        for az in [0.0f32, 90.0, 200.0, 359.0] {
            let sun = [az.to_radians().sin(), 0.4, az.to_radians().cos()];
            let r = rotation_for_key_light(sun).to_radians();
            let x = r.cos() * sun[0] + r.sin() * sun[2];
            let z = -r.sin() * sun[0] + r.cos() * sun[2];
            assert!((x.atan2(z).to_degrees() - KEY_LIGHT_AZIMUTH_DEG).abs() < 1e-3, "{az}");
            assert!((-180.0..=180.0).contains(&rotation_for_key_light(sun)));
        }
        for (deg, wrapped) in [(194.4f32, -165.6f32), (-190.0, 170.0), (90.0, 90.0), (540.0, -180.0)] {
            assert!((wrap_rotation(deg) - wrapped).abs() < 1e-3, "{deg} -> {}", wrap_rotation(deg));
        }
    }
}

#[cfg(test)]
mod stack_drop_tests {
    use super::*;
    use subcraft_app::{LayerTarget, Session};

    fn state() -> UiState {
        UiState::new(Session::new(subcraft_mesh::primitives::cube(2.0), 64), None)
    }

    #[test]
    fn ctrl_dropping_a_material_adds_its_layer_with_a_black_mask_selected() {
        let mut s = state();
        assert!(!s.library.materials.is_empty());
        let layers = s.session.selected_texture_set().layers.len();
        apply_stack_drop(&mut s, ShelfDrag::Material(0), false, true, false, 0.0);
        let ts = s.session.selected_texture_set();
        assert_eq!(ts.layers.len(), layers + 1);
        let new = s.session.selected_layer().unwrap();
        let mask = new.mask.as_ref().expect("Ctrl adds a mask");
        assert_eq!(mask.base.fill()[0], 0.0, "a black mask");
        assert_eq!(s.session.layer_target(), LayerTarget::Mask);

        // Without Ctrl: a plain material layer.
        apply_stack_drop(&mut s, ShelfDrag::Material(0), false, false, false, 0.0);
        assert!(s.session.selected_layer().unwrap().mask.is_none());
        assert_eq!(s.session.selected_texture_set().layers.len(), layers + 2);
        // Alt replaces in place: Ctrl adds nothing then.
        let count = s.session.selected_texture_set().layers.len();
        apply_stack_drop(&mut s, ShelfDrag::Material(0), true, true, false, 0.0);
        assert_eq!(s.session.selected_texture_set().layers.len(), count);
        assert!(s.session.selected_layer().unwrap().mask.is_none());
    }

    #[test]
    fn a_smart_mask_dropped_above_the_stack_makes_a_fill_layer_with_that_mask() {
        let mut s = state();
        if s.library.smart_masks.is_empty() {
            return;
        }
        let layers = s.session.selected_texture_set().layers.len();
        let below = s.session.selection().layer.unwrap();
        apply_stack_drop(&mut s, ShelfDrag::SmartMask(0), false, false, true, 0.0);
        let ts = s.session.selected_texture_set();
        assert_eq!(ts.layers.len(), layers + 1, "a new layer");
        let new = s.session.selected_layer().unwrap();
        assert!(matches!(new.content, LayerContent::Fill(_)));
        assert!(new.mask.as_ref().is_some_and(|m| !m.effects.is_empty()), "it carries the smart mask");
        assert!(ts.find(below).unwrap().mask.is_none(), "the layer below is untouched");
        // Dropped on a row instead: that row gets the mask, no new layer.
        apply_stack_drop(&mut s, ShelfDrag::SmartMask(0), false, false, false, 0.0);
        assert_eq!(s.session.selected_texture_set().layers.len(), layers + 1);
    }

    #[test]
    fn the_ghost_mask_slot_takes_mask_items_and_gives_the_layer_a_black_mask_with_them() {
        use subcraft_doc::MaskEffectKind;
        assert!(ghost_mask_accepts(ShelfDrag::Alpha(0)));
        assert!(ghost_mask_accepts(ShelfDrag::SmartMask(0)));
        assert!(ghost_mask_accepts(ShelfDrag::Texture(0)));
        assert!(!ghost_mask_accepts(ShelfDrag::Material(0)));
        assert!(!ghost_mask_accepts(ShelfDrag::SmartMaterial(0)));
        let mask_filter = subcraft_doc::Filter::ALL.iter().position(|f| f.supports_mask()).unwrap();
        assert!(ghost_mask_accepts(ShelfDrag::Filter(mask_filter)));
        if let Some(i) = subcraft_doc::Filter::ALL.iter().position(|f| !f.supports_mask()) {
            assert!(!ghost_mask_accepts(ShelfDrag::Filter(i)));
        }
        let mut s = state();
        let layer = s.session.selection().layer.unwrap();
        assert!(s.session.selected_layer().unwrap().mask.is_none());
        let steps = s.session.history().entries().len();
        apply_ghost_mask_drop(&mut s, layer, ShelfDrag::Filter(mask_filter), 0.0);
        let l = s.session.selected_layer().unwrap();
        let mask = l.mask.as_ref().expect("a mask was created");
        assert_eq!(mask.base.get(0, 0)[0], 0.0, "a black mask");
        assert!(matches!(mask.effects[..], [ref e] if matches!(e.kind, MaskEffectKind::Filter(_))));
        assert_eq!(s.session.layer_target(), subcraft_app::LayerTarget::Mask, "the new mask is selected");
        assert_eq!(s.session.history().entries().len(), steps + 1, "one undo step");
        // A smart mask on another mask-less layer.
        if !s.library.smart_masks.is_empty() {
            s.session.execute(subcraft_app::Command::AddLayer(subcraft_app::NewLayer::Fill)).unwrap();
            let other = s.session.selection().layer.unwrap();
            s.session.execute(subcraft_app::Command::SelectLayer(Some(layer))).unwrap();
            apply_ghost_mask_drop(&mut s, other, ShelfDrag::SmartMask(0), 0.0);
            assert_eq!(s.session.selection().layer, Some(other));
            let own = s.library.smart_masks[0].effects.len();
            assert_eq!(s.session.selected_layer().unwrap().mask.as_ref().unwrap().effects.len(), own);
        }
        // Materials are refused with a message and change nothing.
        let steps = s.session.history().entries().len();
        apply_ghost_mask_drop(&mut s, layer, ShelfDrag::Material(0), 1.0);
        assert_eq!(s.session.history().entries().len(), steps);
        assert!(s.status.as_ref().is_some_and(|(m, _)| m.contains("mask")));
    }

    #[test]
    fn a_smart_mask_dropped_with_ctrl_or_alt_replaces_the_effects_of_the_mask() {
        let mut s = state();
        if s.library.smart_masks.is_empty() {
            return;
        }
        let own = s.library.smart_masks[0].effects.len();
        let count = |s: &UiState| s.session.selected_layer().unwrap().mask.as_ref().map_or(0, |m| m.effects.len());
        apply_stack_drop(&mut s, ShelfDrag::SmartMask(0), false, false, false, 0.0);
        assert_eq!(count(&s), own);
        // A plain drop adds to what is there.
        apply_stack_drop(&mut s, ShelfDrag::SmartMask(0), false, false, false, 0.0);
        assert_eq!(count(&s), own * 2);
        // Ctrl replaces the whole stack, and so does Alt.
        apply_stack_drop(&mut s, ShelfDrag::SmartMask(0), false, true, false, 0.0);
        assert_eq!(count(&s), own);
        apply_stack_drop(&mut s, ShelfDrag::SmartMask(0), false, false, false, 0.0);
        apply_stack_drop(&mut s, ShelfDrag::SmartMask(0), true, false, false, 0.0);
        assert_eq!(count(&s), own);
        // One undo step brings the replaced effects back.
        s.session.execute(subcraft_app::Command::Undo).unwrap();
        assert_eq!(count(&s), own * 2);
    }
}
