//! The editing session: scene (mesh + acceleration structures), document + history, selection,
//! brush state, the active stroke and the composite caches.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use glam::{Vec2, Vec3};
use subcraft_compositor::{Composite, CompositeCache, CompositeWorker, EvalContext, SetSnapshot, Snapshot};
use subcraft_core::{ChannelKind, EffectId, LayerId, TextureSetId};
use subcraft_doc::{ChannelDef, ContentEffect, ContentEffectKind, Document, History, Layer, LayerContent, LayerLocation, Mask, MaskEffect, ResourceStore, TextureSet};
use subcraft_materials::{GeometryCache, MeshMaps};
use subcraft_mesh::{Bvh, Mesh, import_mesh, primitives};
use subcraft_paint::{
    BrushMaterial, BrushSettings, Camera, DepthCache, PaintContext, PaintError, PaintTarget, PolygonFillMode, Stroke, StrokeMode,
    StrokeOptions,
};

use crate::actions::Action;
use crate::command::{Command, CommandError, NewLayer};

/// Run `$body` on the content or mask effect stack of a layer (`$s`: `&mut Vec<Arc<_>>`).
macro_rules! on_stack {
    ($layer:expr, $target:expr, |$s:ident| $body:expr) => {
        match $target {
            LayerTarget::Content => {
                let $s = &mut $layer.content_effects;
                $body
            }
            LayerTarget::Mask => {
                let $s = &mut $layer.mask.as_mut().ok_or(CommandError::NoMask)?.effects;
                $body
            }
        }
    };
}

/// Default texture-set resolution for new sessions.
pub const DEFAULT_RESOLUTION: u32 = 2048;

/// Shown when an edit targets an instance's content.
pub const INSTANCE_READ_ONLY: &str = "This layer is an instance: edit its source layer (its name, visibility, blending and colour are its own)";

/// Guide geometry of the active symmetry, in object space (see [`Session::symmetry_guides`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SymmetryGuides {
    /// Where each enabled mirror plane cuts the surface.
    pub intersections: Vec<[Vec3; 2]>,
    /// Corners of each enabled mirror plane's rectangle (a little larger than the mesh).
    pub planes: Vec<[Vec3; 4]>,
    /// Radial axis line, then one spoke per copy.
    pub axis: Vec<[Vec3; 2]>,
}

impl SymmetryGuides {
    /// The guide lines to draw: the chosen kinds, plane rectangles as their outlines.
    pub fn segments(&self, intersections: bool, planes: bool, axis: bool) -> Vec<[Vec3; 2]> {
        let mut lines = Vec::new();
        if intersections { lines.extend(self.intersections.iter().copied()); }
        if planes { lines.extend(self.planes.iter().flat_map(crate::guides::outline)); }
        if axis { lines.extend(self.axis.iter().copied()); }
        lines
    }
}

/// Immutable geometry of the session and everything derived from it.
pub struct Scene {
    /// Mirror guides for the last plane positions (bits of the three plane coordinates).
    symmetry_sections: std::sync::Mutex<Option<([u32; 3], Arc<[crate::guides::MirrorGuide; 3]>)>>,
    pub mesh: Arc<Mesh>,
    pub bvh: Bvh,
    pub geometry: GeometryCache,
    pub depth: DepthCache,
}

impl Scene {
    pub fn new(mesh: Mesh) -> Self {
        size_field_cache();
        let mesh = Arc::new(mesh);
        Self { symmetry_sections: Default::default(), bvh: Bvh::build(&mesh), geometry: GeometryCache::new(mesh.clone()), depth: DepthCache::new(), mesh }
    }
}

/// Size the procedural field cache from this machine's memory, once per process: at 4096² a stack
/// with a dozen generator noises needs about 0.9 GB of cached fields, more than the conservative
/// default, and every field that does not fit is recomputed on every slider step.
fn size_field_cache() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let mut system = sysinfo::System::new();
        system.refresh_memory();
        let total = system.total_memory();
        if total > 0 {
            subcraft_materials::sample::field::set_budget(subcraft_materials::sample::field::budget_for_memory(total));
        }
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub texture_set: TextureSetId,
    pub layer: Option<LayerId>,
}

/// The selected layer's editable domain. Transient tool state, never project data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerTarget { Content, Mask }

/// What the Clone tool's source does between strokes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CloneSourceMode {
    /// The source keeps its distance to the brush: each stroke continues where the last one left
    /// the copy (the source follows the brush).
    #[default]
    Relative,
    /// Every stroke starts copying from the point that was set as the source.
    Absolute,
}

/// Current brush configuration (not undoable).
#[derive(Clone, Debug)]
pub struct BrushState {
    /// Tip image (`None` = round procedural tip) and its display name.
    pub alpha: Option<Arc<subcraft_image::Bitmap>>,
    pub alpha_name: String,
    pub settings: BrushSettings,
    pub options: StrokeOptions,
    pub material: BrushMaterial,
    pub mode: StrokeMode,
    pub target: PaintTarget,
    /// Screen-space stencil (white paints, black blocks); `None` = no stencil.
    pub stencil: Option<Arc<subcraft_paint::Stencil>>,
    /// Temporarily ignore the stencil (held N key), without removing it.
    pub stencil_bypass: bool,
    /// Painter's Projection tool: strokes cast the material's images from the view, tiled over
    /// it, whatever the brush material's own projection settings.
    pub projection: bool,
    /// Clone tool: whether the source follows the brush or returns to where it was set.
    pub clone_mode: CloneSourceMode,
}

/// A surface point under the cursor.
#[derive(Clone, Copy, Debug)]
pub struct Pick {
    pub uv: Vec2,
    pub position: Vec3,
    pub normal: Vec3,
    pub triangle: u32,
    pub material_slot: u32,
    /// View depth of the hit.
    pub depth: f32,
}

/// Tiles of one texture set's composite that changed in an update.
#[derive(Clone, Debug, PartialEq)]
pub struct CompositeUpdate {
    pub texture_set: TextureSetId,
    pub material_slot: u32,
    /// Re-evaluated tiles.
    pub tiles: Vec<usize>,
    /// Per channel, the re-evaluated tiles whose texels actually changed (what to upload).
    pub changed: subcraft_compositor::ChangedTiles,
}

struct ActiveStroke {
    stroke: Stroke,
    texture_set: TextureSetId,
    label: &'static str,
    /// Last view camera, for the catch-up end of the stroke.
    camera: Option<Camera>,
    /// The stroke paints the quick mask (in [`Session::quick_scratch`]), not the document.
    quick: bool,
    /// What the stroke was painted with, when a later stroke may continue it (paint and erase
    /// strokes); `None` for strokes that cannot be continued.
    key: Option<ContinueKey>,
}

/// What must stay the same for a stroke to continue the previous one.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ContinueKey {
    texture_set: TextureSetId,
    layer: LayerId,
    mode: StrokeMode,
    target: PaintTarget,
    settings: subcraft_paint::BrushSettings,
    options: subcraft_paint::StrokeOptions,
    /// Painted in the 2D view.
    uv: bool,
}

/// The last finished stroke, kept so the next one can continue it (Painter's A key).
struct LastStroke {
    active: ActiveStroke,
    /// History position and length right after the stroke's step: any later edit, undo or
    /// other change means the stroke can no longer be continued.
    cursor: usize,
    entries: usize,
}

/// Painter's quick mask: a temporary selection painted with the usual tools (U), which then
/// restricts every stroke and polygon fill to its white areas. It belongs to one texture set and
/// is view state: not saved, not in the history.
#[derive(Clone, Debug)]
pub struct QuickMask {
    pub texture_set: TextureSetId,
    /// One channel at the texture set's resolution: 1 paints, 0 protects.
    pub image: subcraft_image::TiledImage,
    /// Strokes paint the mask itself (U again applies it).
    pub editing: bool,
    /// Bumped on every change (overlay caches key on it).
    pub revision: u64,
}

pub struct Session {
    pub(crate) part_view: crate::part_workflow::PartView,
    pub(crate) scene: Scene,
    resources: ResourceStore,
    mesh_maps: HashMap<TextureSetId, MeshMaps>,
    /// ID maps as baked / imported, with the document edits currently applied over them in
    /// `mesh_maps` (see `crate::id_color`).
    id_baked: HashMap<TextureSetId, (subcraft_image::TiledImage, Vec<subcraft_doc::IdColorEdit>)>,
    no_maps: MeshMaps,
    pub(crate) history: History,
    /// Which instances are in step with their sources (see `subcraft_doc::instance`).
    instances: subcraft_doc::InstanceCache,
    /// Live document: equals the current history entry except while a stroke is in progress.
    pub(crate) doc: Document,
    pub(crate) selection: Selection,
    mask_clipboard: Option<Mask>,
    hidden_sets: std::collections::BTreeSet<TextureSetId>,
    /// Texture Set List Focus mode: only the selected set is shown (view state).
    focus_set: bool,
    /// Selected effect of the selected layer's current domain (see [`Session::selected_effect`]).
    effect: Option<EffectId>,
    pub brush: BrushState,
    /// Settings > General > Default UV scale for materials: the UV tiling of new fill layers.
    pub default_uv_scale: f32,
    stroke: Option<ActiveStroke>,
    /// The last finished paint / erase stroke (see [`Session::set_continue_stroke`]).
    last_stroke: Option<LastStroke>,
    /// The next stroke continues the last one (A held when it starts).
    continue_stroke: bool,
    /// Clone tool: object-space source point, and the screen offset the first stroke fixed.
    clone_source: Option<Vec3>,
    clone_offset: Option<Vec2>,
    /// The clone source as a texture position of a texture set (2D-view clone strokes), and the
    /// UV offset the last 2D clone stroke fixed.
    clone_source_uv: Option<(TextureSetId, Vec2)>,
    clone_offset_uv: Option<Vec2>,
    quick_mask: Option<QuickMask>,
    /// While a quick mask stroke runs: a one-layer texture set whose mask base is the quick mask.
    quick_scratch: Option<TextureSet>,
    /// Composite caches while compositing runs on the calling thread (until the first
    /// interactive update starts the worker).
    composites: HashMap<TextureSetId, CompositeCache>,
    /// Background compositing (desktop, from the first interactive update on): owns the caches.
    worker: Option<CompositeWorker>,
    /// No worker thread can run here: composite on the calling thread.
    inline_only: bool,
    /// Newest composites published by the worker.
    shown: HashMap<TextureSetId, Composite>,
    /// Inputs last submitted to the worker (submissions happen only when they change).
    submitted: Option<Snapshot>,
    /// Texture sets to re-evaluate completely on the next update (mesh maps changed).
    invalidated: BTreeSet<TextureSetId>,
    /// Changes whenever newly evaluated composites are installed, including synchronous callers.
    composite_generation: u64,
    /// Bumped on every document change (UI caches key off it).
    pub(crate) revision: u64,
    layer_counter: u32,
    project_path: Option<PathBuf>,
    /// Document as of the last save / load / new (for the unsaved-changes indicator).
    saved_doc: Document,
    /// Saved with the project but outside the undo history (export choices, normal format).
    pub(crate) settings: subcraft_doc::ProjectSettings,
    saved_settings: subcraft_doc::ProjectSettings,
    /// Mesh maps changed (baked) since the last save.
    maps_dirty: bool,
    /// Bumped whenever baked mesh maps change (autosave change tracking).
    maps_generation: u64,
    /// Opened from a recovery copy: unsaved until the user saves.
    recovered: bool,
    /// The mesh was reloaded since the last save.
    mesh_dirty: bool,
    /// Symmetry axes the L key (Painter's Symmetry on / off) switched off, restored by the next L.
    saved_symmetry: Option<([bool; 3], bool)>,
    /// Painter's Pause engine computation: the incremental compositing waits while set.
    engine_paused: bool,
}

/// Everything a project file holds, captured cheaply (shared tiles and resources) so it can be
/// written on a background thread while editing continues.
pub struct ProjectSnapshot {
    pub mesh: Arc<Mesh>,
    pub document: Document,
    pub resources: ResourceStore,
    pub mesh_maps: subcraft_project::MeshMapSet,
    pub settings: subcraft_doc::ProjectSettings,
}

impl ProjectSnapshot {
    pub fn save(&self, path: &Path) -> Result<(), String> {
        subcraft_project::save_project_with(path, &self.mesh, &self.document, &self.resources, &self.mesh_maps, &self.settings).map_err(|e| e.to_string())
    }
}

/// Whether two project settings differ only in their saved view (which is not an edit).
fn same_settings_but_view(a: &subcraft_doc::ProjectSettings, b: &subcraft_doc::ProjectSettings) -> bool {
    if a.view == b.view {
        return a == b;
    }
    let mut a = a.clone();
    a.view = b.view;
    a == *b
}

impl Session {
    /// One texture set per material slot, each with an empty paint layer selected.
    pub fn new(mesh: Mesh, resolution: u32) -> Self {
        let names: Vec<String> = mesh.materials.iter().map(|m| m.name.clone()).collect();
        let mut doc = Document::from_material_slots(&names, resolution);
        for ts in doc.texture_sets.iter_mut() {
            let tc = ts.clone();
            ts.insert_above(None, Layer::new_paint("Layer 1", &tc));
        }
        Self::from_parts(mesh, doc, ResourceStore::default(), "New Project")
    }

    /// Session over an existing document (project load).
    pub fn from_parts(mesh: Mesh, doc: Document, resources: ResourceStore, label: &str) -> Self {
        assert!(!doc.texture_sets.is_empty(), "a document has at least one texture set");
        let first = &doc.texture_sets[0];
        let selection = Selection { texture_set: first.id, layer: first.layers.last().map(|l| l.id) };
        let material = BrushMaterial::new(first);
        let layer_counter = doc.texture_sets.iter().map(|t| t.layer_count() as u32).max().unwrap_or(1);
        Self {
            scene: Scene::new(mesh),
            part_view: Default::default(),
            resources,
            mesh_maps: HashMap::new(),
            id_baked: HashMap::new(),
            no_maps: MeshMaps::new(),
            history: History::new(doc.clone(), label),
            instances: Default::default(),
            saved_doc: doc.clone(),
            settings: Default::default(),
            saved_settings: Default::default(),
            doc,
            selection,
            mask_clipboard: None,
            default_uv_scale: 1.0,
            hidden_sets: Default::default(),
            focus_set: false,
            effect: None,
            brush: BrushState {
                alpha: None,
                alpha_name: "Round".into(),
                settings: BrushSettings::default(),
                options: StrokeOptions::default(),
                material,
                mode: StrokeMode::Paint,
                target: PaintTarget::Content,
                stencil: None,
                stencil_bypass: false,
                projection: false,
                clone_mode: CloneSourceMode::Relative,
            },
            stroke: None,
            last_stroke: None,
            continue_stroke: false,
            clone_source: None,
            clone_offset: None,
            clone_source_uv: None,
            clone_offset_uv: None,
            quick_mask: None,
            quick_scratch: None,
            composites: HashMap::new(),
            worker: None,
            inline_only: false,
            shown: HashMap::new(),
            submitted: None,
            invalidated: BTreeSet::new(),
            composite_generation: 0,
            revision: 0,
            layer_counter,
            project_path: None,
            maps_dirty: false,
            maps_generation: 0,
            recovered: false,
            mesh_dirty: false,
            saved_symmetry: None,
            engine_paused: false,
        }
    }

    /// Open a `.subcraft` project.
    pub fn open_project(path: &Path) -> Result<Self, String> {
        Self::open_project_labeled(path, "Open Project")
    }

    pub(crate) fn open_project_labeled(path: &Path, label: &str) -> Result<Self, String> {
        let data = subcraft_project::load_project(path).map_err(|e| e.to_string())?;
        if data.document.texture_sets.is_empty() {
            return Err("project has no texture sets".into());
        }
        let mut s = Self::from_parts(data.mesh, data.document, data.resources, label);
        s.project_path = Some(path.to_path_buf());
        s.saved_settings = data.settings.clone();
        s.settings = data.settings;
        for (ts, maps) in data.mesh_maps {
            s.mesh_maps.insert(ts, MeshMaps::from_map(maps));
        }
        s.capture_baked_ids();
        s.sync_id_maps();
        Ok(s)
    }

    /// Save to `path` (or the current project path). Returns the path written.
    pub fn save(&mut self, path: Option<&Path>) -> Result<PathBuf, String> {
        if self.stroke.is_some() {
            return Err("finish the stroke before saving".into());
        }
        let path = match path.or(self.project_path.as_deref()) {
            Some(p) => p.to_path_buf(),
            None => return Err("no project path: use Save As".into()),
        };
        let maps: subcraft_project::MeshMapSet = self.mesh_maps.keys().map(|id| (*id, self.stored_mesh_maps(*id))).collect();
        subcraft_project::save_project_with(&path, &self.scene.mesh, &self.doc, &self.resources, &maps, &self.settings).map_err(|e| e.to_string())?;
        self.project_path = Some(path.clone());
        self.saved_doc = self.doc.clone();
        self.saved_settings = self.settings.clone();
        self.maps_dirty = false;
        self.recovered = false;
        self.mesh_dirty = false;
        Ok(path)
    }

    /// File > Save as copy: write the project to `path` and keep working on the current file. The
    /// project path, the unsaved-changes state and the recent-files list are left as they were.
    pub fn save_copy(&self, path: &Path) -> Result<(), String> {
        if self.stroke.is_some() {
            return Err("finish the stroke before saving".into());
        }
        if self.project_path.as_deref() == Some(path) {
            return Err("a copy needs another file than the project itself: use Save".into());
        }
        let maps: subcraft_project::MeshMapSet = self.mesh_maps.keys().map(|id| (*id, self.stored_mesh_maps(*id))).collect();
        subcraft_project::save_project_with(path, &self.scene.mesh, &self.doc, &self.resources, &maps, &self.settings).map_err(|e| e.to_string())
    }

    /// The last committed state (no half-finished stroke) for a background save.
    pub fn snapshot(&self) -> ProjectSnapshot {
        ProjectSnapshot {
            mesh: self.scene.mesh.clone(),
            document: self.history.current().clone(),
            resources: self.resources.clone(),
            mesh_maps: self.mesh_maps.keys().map(|id| (*id, self.stored_mesh_maps(*id))).collect(),
            settings: self.settings.clone(),
        }
    }

    /// Changes whenever baked mesh maps change.
    pub fn maps_generation(&self) -> u64 {
        self.maps_generation
    }

    /// After opening a recovery copy: point at the user's own file again and count as unsaved.
    pub(crate) fn mark_recovered(&mut self, project_path: Option<PathBuf>) {
        self.project_path = project_path;
        self.recovered = true;
    }

    pub(crate) fn restore_after_update(&mut self, project_path: Option<PathBuf>, modified: bool) {
        self.project_path = project_path;
        self.recovered = modified;
    }

    /// Opened from a recovery copy and not saved since.
    pub fn is_recovered(&self) -> bool {
        self.recovered
    }

    pub fn project_path(&self) -> Option<&Path> {
        self.project_path.as_deref()
    }

    /// True if the document differs from the last save / load.
    pub fn is_modified(&self) -> bool {
        crate::unsaved::is_dirty(crate::unsaved::DirtyInputs {
            document_changed: self.doc != self.saved_doc,
            // The saved view (camera, 2D pan) travels with the project but moving around is
            // not an edit.
            settings_changed: !same_settings_but_view(&self.settings, &self.saved_settings),
            maps_changed: self.maps_dirty,
            mesh_changed: self.mesh_dirty,
            from_recovery: self.recovered,
        })
    }

    /// Project-wide settings (export choices, normal format), saved with the project.
    pub fn settings(&self) -> &subcraft_doc::ProjectSettings {
        &self.settings
    }

    /// Store the view (3D camera, 2D pan and zoom) to save with the project. Never marks the
    /// project as modified.
    pub fn set_view_state(&mut self, view: Option<subcraft_doc::ViewState>) {
        self.settings.view = view;
    }

    /// Replace the project settings. Not undoable; marks the project as modified.
    pub fn set_settings(&mut self, settings: subcraft_doc::ProjectSettings) {
        self.settings = settings;
    }

    // ----- baking --------------------------------------------------------------------------------

    /// Baked mesh maps of a texture set.
    pub fn mesh_maps(&self, ts: TextureSetId) -> Option<&MeshMaps> {
        self.mesh_maps.get(&ts).filter(|m| !m.is_empty())
    }

    /// Install a rebuilt Painter stack: per texture set the folder goes above the flattened layer
    /// (which takes the visibility chosen by the quality check). Layers the user added meanwhile
    /// stay where they are. While the project is still as opened, the result replaces the opened
    /// state (so undo cannot go back to a half-imported project); otherwise it is one undo step.
    /// Returns the stack's notes.
    pub fn apply_spp_stack(&mut self, stack: crate::spp_job::SppStack) -> Vec<String> {
        for r in stack.resources.iter() {
            self.resources.insert_shared(r.clone());
        }
        let pristine = self.history.entries().len() == 1 && self.history.cursor() == 0;
        for u in stack.updates {
            let Some(ts) = self.doc.texture_set_mut(u.texture_set) else { continue };
            let Some(pos) = ts.layers.iter().position(|l| l.id == u.flat.id) else { continue };
            ts.layers[pos] = u.flat;
            ts.layers.insert(pos + 1, u.folder);
        }
        if pristine {
            self.history.reset(self.doc.clone(), "Open Painter project");
            self.saved_doc = self.doc.clone();
            self.revision += 1;
        } else {
            self.commit("Rebuild Painter stack", None);
        }
        stack.notes
    }

    /// Install baked maps (merged over existing ones) and refresh that set's composite.
    pub fn set_mesh_maps(&mut self, ts: TextureSetId, maps: std::collections::BTreeMap<subcraft_core::MeshMapKind, subcraft_image::TiledImage>) {
        self.settings.stale_mesh_maps.retain(|id| *id != ts);
        if let Some(id) = maps.get(&subcraft_core::MeshMapKind::Id) {
            self.id_baked.insert(ts, (id.clone(), Vec::new()));
        }
        let entry = self.mesh_maps.entry(ts).or_default();
        let mut merged = entry.as_map().clone();
        merged.extend(maps);
        *entry = MeshMaps::from_map(merged);
        self.invalidated.insert(ts);
        self.maps_dirty = true;
        self.maps_generation += 1;
        self.revision += 1;
        // A new bake keeps the user's ID color edits (they apply to colors it still has).
        self.sync_id_maps();
    }

    /// Maps as they are saved: the ID map as baked / imported (its edits are in the document).
    fn stored_mesh_maps(&self, ts: TextureSetId) -> std::collections::BTreeMap<subcraft_core::MeshMapKind, subcraft_image::TiledImage> {
        let mut maps = self.mesh_maps.get(&ts).map(|m| m.as_map().clone()).unwrap_or_default();
        if let Some((baked, _)) = self.id_baked.get(&ts) && maps.contains_key(&subcraft_core::MeshMapKind::Id) {
            maps.insert(subcraft_core::MeshMapKind::Id, baked.clone());
        }
        maps
    }

    /// Take the installed ID maps as the baked ones (after loading or replacing all maps).
    fn capture_baked_ids(&mut self) {
        self.id_baked = self.mesh_maps.iter()
            .filter_map(|(ts, m)| Some((*ts, (m.get(subcraft_core::MeshMapKind::Id)?.clone(), Vec::new()))))
            .collect();
    }

    /// Show each set's ID map with the document's ID color edits (after edits, undo and redo).
    pub(crate) fn sync_id_maps(&mut self) {
        let mut changed = Vec::new();
        for ts in &self.doc.texture_sets {
            if let Some((baked, applied)) = self.id_baked.get(&ts.id) && *applied != ts.id_color_edits {
                changed.push((ts.id, crate::id_color::apply_id_edits(baked, &ts.id_color_edits), ts.id_color_edits.clone()));
            }
        }
        for (ts, image, edits) in changed {
            if let Some(entry) = self.id_baked.get_mut(&ts) { entry.1 = edits; }
            let entry = self.mesh_maps.entry(ts).or_default();
            let mut maps = entry.as_map().clone();
            maps.insert(subcraft_core::MeshMapKind::Id, image);
            *entry = MeshMaps::from_map(maps);
            self.invalidated.insert(ts);
            self.maps_dirty = true;
            self.maps_generation += 1;
            self.revision += 1;
        }
    }

    /// Start baking on a background thread; poll the job each frame.
    pub fn start_bake(&self, request: crate::bake_job::BakeRequest) -> crate::bake_job::BakeJob {
        crate::bake_job::BakeJob::start(self.scene.mesh.clone(), Arc::new(self.scene.bvh.clone()), request)
    }

    /// Reuse the high-poly mesh already loaded for the bake preview.
    pub fn start_bake_prepared(&self, request: crate::bake_job::BakeRequest, high: Option<crate::bake_job::BakePreviewMesh>) -> crate::bake_job::BakeJob {
        crate::bake_job::BakeJob::start_prepared(self.scene.mesh.clone(), Arc::new(self.scene.bvh.clone()), request, high)
    }

    /// [`Self::start_bake_prepared`] with more high-poly files and a custom cage file.
    pub fn start_bake_files(&self, request: crate::bake_job::BakeRequest, high: Option<crate::bake_job::BakePreviewMesh>, files: crate::bake_job::BakeFiles) -> crate::bake_job::BakeJob {
        crate::bake_job::BakeJob::start_with_files(self.scene.mesh.clone(), Arc::new(self.scene.bvh.clone()), request, high, files)
    }

    /// Expose a high-to-low normal bake in the existing layer workflow, without changing
    /// authored materials. A live mesh-map reference updates on rebakes and saves normally.
    pub(crate) fn show_baked_normal(&mut self, ts_id: TextureSetId) {
        use subcraft_doc::{BlendMode, ChannelBlend, FillChannel, FillSource};
        use subcraft_core::MeshMapKind;
        fn uses_bake(l: &Layer) -> bool {
            let source = |s: &FillSource| matches!(s, FillSource::MeshMap { map: MeshMapKind::Normal });
            let channels = |c: &std::collections::BTreeMap<ChannelKind, FillChannel>| {
                c.get(&ChannelKind::Normal).is_some_and(|f| f.enabled && source(&f.source))
            };
            matches!(&l.content, LayerContent::Fill(f) if channels(&f.channels))
                || l.content_effects.iter().any(|e| matches!(&e.kind, ContentEffectKind::Fill(f) if channels(&f.channels)))
                || l.children().iter().any(|l| uses_bake(l))
        }
        let Some(ts) = self.doc.texture_set_mut(ts_id) else { return };
        if ts.layers.iter().any(|l| uses_bake(l)) { return; }
        if !ts.has_channel(ChannelKind::Normal) { ts.channels.push(ChannelDef::default_for(ChannelKind::Normal)); }
        let mut layer = Layer::new_fill("Baked normal · high poly", ts);
        if let LayerContent::Fill(fill) = &mut layer.content {
            fill.channels.clear();
            fill.channels.insert(ChannelKind::Normal, FillChannel::new(FillSource::MeshMap { map: MeshMapKind::Normal }));
        }
        // The bake is the base orientation; authored normals beneath it remain detail.
        layer.blend.insert(ChannelKind::Normal, ChannelBlend::new(BlendMode::NormalMapInverseDetail));
        ts.layers.push(Arc::new(layer));
        self.commit("Show baked normal", None);
    }

    /// The model SubCraft starts with and the "sample model" action opens: the bundled
    /// `SubCraft_base.fbx` (see `subcraft_assets::default_model`). Falls back to
    /// [`Session::sample`] if the bundled file ever fails to import.
    pub fn default_model() -> Self {
        match subcraft_mesh::import_fbx_bytes(subcraft_assets::default_model::FBX, subcraft_assets::default_model::NAME) {
            Ok(report) if report.mesh.triangle_count() > 0 => Self::with_default_material(report.mesh, DEFAULT_RESOLUTION),
            other => {
                log::error!("bundled default model failed to import ({:?}); using the sample crate", other.err());
                Self::sample()
            }
        }
    }

    /// Like [`Session::new`], but every texture set starts with the default smart material
    /// (`subcraft_assets::default_model::SMART_MATERIAL`, a folder of layers) below its empty
    /// paint layer. Built into the starting document, so it is not an edit: a fresh start has
    /// nothing to save and nothing to undo.
    fn with_default_material(mesh: Mesh, resolution: u32) -> Self {
        let smart = subcraft_assets::smart::smart_materials().into_iter().find(|m| m.name == subcraft_assets::default_model::SMART_MATERIAL);
        let Some(smart) = smart else { return Self::new(mesh, resolution) };
        let names: Vec<String> = mesh.materials.iter().map(|m| m.name.clone()).collect();
        let mut doc = Document::from_material_slots(&names, resolution);
        let mut resources = ResourceStore::default();
        for r in smart.resources() {
            resources.insert_shared(r.clone());
        }
        let ids = subcraft_assets::ImageIds::new();
        for ts in doc.texture_sets.iter_mut() {
            let tc = ts.clone();
            ts.insert_above(None, smart.build_with(&tc, &ids));
            ts.insert_above(None, Layer::new_paint("Layer 1", &tc));
        }
        Self::from_parts(mesh, doc, resources, "New Project")
    }

    /// The built-in two-material sample crate (kept as a small fixture for tests and examples).
    pub fn sample() -> Self {
        Self::new(primitives::sample_crate(), DEFAULT_RESOLUTION)
    }

    /// New session from a mesh file (OBJ / glTF / FBX / Substance Painter .spp).
    pub fn open_mesh(path: &Path, resolution: u32) -> Result<(Self, Vec<String>), String> {
        if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("spp")) {
            return Self::open_spp(path);
        }
        let mut report = match import_mesh(path) {
            Err(subcraft_mesh::ImportError::Build(subcraft_mesh::mesh::BuildError::NoUvs)) => subcraft_mesh::import_mesh_for_baking(path).map_err(|e| e.to_string())?,
            other => other.map_err(|e| e.to_string())?,
        };
        if report.mesh.triangle_count() == 0 {
            return Err("mesh has no triangles".into());
        }
        report.mesh = subcraft_mesh::unwrap::unwrap_if_missing(std::mem::take(&mut report.mesh), &mut report.warnings);
        let mut warnings = report.warnings;
        let tiles = report.mesh.udim_tiles(None).len();
        let mut session = if tiles > 1 {
            // Folding the tiles onto one would make their islands paint over each other.
            let settings = crate::new_project::NewProjectSettings { resolution, ..Default::default() };
            warnings.push(format!("The model uses {tiles} UDIM tiles; each tile became its own texture set (use File > New project to choose differently)."));
            Self::from_mesh_with(report.mesh, &settings)?
        } else {
            Self::new(report.mesh, resolution)
        };
        session.set_mesh_source(path);
        Ok((session, warnings))
    }

    /// Remember the model file the mesh came from (default for File > Reload mesh). Not an
    /// edit: does not mark the project modified.
    pub fn set_mesh_source(&mut self, path: &Path) {
        let source = Some(path.display().to_string());
        let unchanged = same_settings_but_view(&self.settings, &self.saved_settings);
        self.settings.mesh_source = source.clone();
        if unchanged {
            self.saved_settings.mesh_source = source;
        }
    }

    /// Import the cameras of the mesh file at `path` into the project, replacing the ones it had
    /// (New project > Import cameras, or a mesh reload with that option). Returns how many were
    /// found; glTF, GLB and FBX carry cameras, other formats give 0. Like the mesh source, this
    /// does not mark an unchanged project modified: it is part of creating or reloading it.
    pub fn import_mesh_cameras(&mut self, path: &Path) -> Result<usize, String> {
        let specs = subcraft_mesh::camera::cameras_in_mesh_file(path).map_err(|e| format!("cameras of {}: {e}", path.display()))?;
        let cameras: Vec<subcraft_doc::SceneCamera> = specs
            .iter()
            .map(|c| subcraft_doc::SceneCamera {
                name: c.name.clone(),
                position: c.position.to_array(),
                rotation: c.rotation.to_array(),
                fov_y_deg: c.fov_y_deg.clamp(1.0, 170.0),
                orthographic: c.orthographic,
                aspect: c.aspect,
            })
            .filter(subcraft_doc::SceneCamera::is_valid)
            .collect();
        let count = cameras.len();
        let unchanged = same_settings_but_view(&self.settings, &self.saved_settings);
        self.settings.cameras = cameras.clone();
        if unchanged {
            self.saved_settings.cameras = cameras;
        }
        Ok(count)
    }

    /// Cameras imported from the mesh file, in file order.
    pub fn scene_cameras(&self) -> &[subcraft_doc::SceneCamera] {
        &self.settings.cameras
    }

    /// The model file the mesh was imported or last reloaded from, if known.
    pub fn mesh_source(&self) -> Option<&Path> {
        self.settings.mesh_source.as_deref().map(Path::new)
    }

    /// Whether a texture set's baked maps were made for an earlier version of the mesh.
    pub fn mesh_maps_stale(&self, ts: TextureSetId) -> bool {
        self.settings.stale_mesh_maps.contains(&ts)
    }

    // ----- mesh reload ---------------------------------------------------------------------------

    /// Step 1 of File > Reload mesh, in the background: import `path` and match it to the
    /// texture sets (see [`crate::mesh_reload`]).
    pub fn start_mesh_reload_check(&self, path: &Path) -> crate::mesh_reload::MeshReloadJob<crate::mesh_reload::PreparedReload> {
        let (path, mesh, bvh, doc) = (path.to_path_buf(), self.scene.mesh.clone(), self.scene.bvh.clone(), self.history.current().clone());
        crate::mesh_reload::MeshReloadJob::spawn(move |p| crate::mesh_reload::prepare(&path, mesh, &bvh, &doc, p))
    }

    /// [`Session::start_mesh_reload_check`] for an already imported mesh (tests, scripts).
    pub fn check_mesh_reload(&self, path: &Path, mesh: Mesh) -> Result<crate::mesh_reload::PreparedReload, String> {
        let progress = crate::mesh_reload::ReloadProgress::default();
        crate::mesh_reload::prepare_mesh(path, mesh, Vec::new(), self.scene.mesh.clone(), &self.scene.bvh, self.history.current(), &progress)
    }

    /// Step 2, in the background: build the reloaded document with `mode`.
    pub fn start_mesh_reload(&self, prepared: Arc<crate::mesh_reload::PreparedReload>, mode: crate::mesh_reload::ReloadMode) -> crate::mesh_reload::MeshReloadJob<crate::mesh_reload::ReloadOutput> {
        let (bvh, doc, maps) = (self.scene.bvh.clone(), self.history.current().clone(), self.mesh_map_set());
        crate::mesh_reload::MeshReloadJob::spawn(move |p| crate::mesh_reload::transfer(&prepared, mode, &bvh, &doc, &maps, p))
    }

    /// Both steps on the calling thread for an already prepared reload (tests, scripts).
    pub fn apply_mesh_reload(&mut self, prepared: Arc<crate::mesh_reload::PreparedReload>, mode: crate::mesh_reload::ReloadMode) -> Result<crate::mesh_reload::MeshReloadReport, String> {
        let output = self.start_mesh_reload(prepared, mode).wait()?;
        self.finish_mesh_reload(output)
    }

    /// Import `path` and reload it with `mode`, waiting for the result.
    pub fn reload_mesh(&mut self, path: &Path, mode: crate::mesh_reload::ReloadMode) -> Result<crate::mesh_reload::MeshReloadReport, String> {
        let prepared = Arc::new(self.start_mesh_reload_check(path).wait()?);
        self.apply_mesh_reload(prepared, mode)
    }

    pub(crate) fn mesh_map_set(&self) -> crate::mesh_reload::MapSet {
        self.mesh_maps.keys().map(|id| (*id, self.stored_mesh_maps(*id))).collect()
    }

    /// Install a finished reload: new mesh, texture sets and maps. Not undoable: the history is
    /// cleared (the mesh is not part of it, ADR-0042). Fails if the project changed meanwhile.
    pub fn finish_mesh_reload(&mut self, output: crate::mesh_reload::ReloadOutput) -> Result<crate::mesh_reload::MeshReloadReport, String> {
        if self.stroke.is_some() {
            return Err("finish the stroke before reloading the mesh".into());
        }
        if !Arc::ptr_eq(&output.base_mesh, &self.scene.mesh) || output.base_doc != *self.history.current() || self.doc != *self.history.current() {
            return Err("the project changed while the new mesh was being prepared; reload it again".into());
        }
        if output.document.texture_sets.is_empty() {
            return Err("the reloaded mesh has no texture sets".into());
        }
        log::info!("action: Reload Mesh ({})", output.source.display());
        // Compositing restarts from scratch on the new geometry.
        self.worker = None;
        self.composites.clear();
        self.shown.clear();
        self.submitted = None;
        self.invalidated.clear();
        self.scene = Scene::new(output.mesh);
        self.part_view.reset_for_new_mesh();
        self.doc = output.document;
        self.history.reset(self.doc.clone(), "Reload Mesh");
        self.mesh_maps = output.maps.into_iter().map(|(id, m)| (id, MeshMaps::from_map(m))).collect();
        self.capture_baked_ids();
        self.sync_id_maps();
        let doc = &self.doc;
        self.settings.stale_mesh_maps.retain(|id| doc.texture_set(*id).is_some());
        for id in output.stale {
            if !self.settings.stale_mesh_maps.contains(&id) {
                self.settings.stale_mesh_maps.push(id);
            }
        }
        self.settings.mesh_source = Some(output.source.display().to_string());
        if self.doc.texture_set(self.selection.texture_set).is_none() {
            self.selection.texture_set = self.doc.texture_sets[0].id;
            self.selection.layer = None;
        }
        if self.selection.layer.is_none_or(|l| self.selected_texture_set().find(l).is_none()) {
            self.selection.layer = self.selected_texture_set().layers.last().map(|l| l.id);
        }
        self.clone_source = None;
        self.clone_offset = None;
        self.clone_source_uv = None;
        self.clone_offset_uv = None;
        self.quick_mask = None;
        self.normalize_target();
        self.mesh_dirty = true;
        self.maps_generation += 1;
        self.revision += 1;
        Ok(output.report)
    }

    // ----- queries -------------------------------------------------------------------------------

    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Guides of the active symmetry, at the planes and axis the stroke paints with: where each
    /// enabled mirror plane cuts the surface, each plane's rectangle, and the radial axis with
    /// one spoke per copy.
    pub fn symmetry_guides(&self) -> SymmetryGuides {
        let options = &self.brush.options;
        let mut guides = SymmetryGuides::default();
        if !options.has_symmetry() { return guides; }
        if options.symmetry.contains(&true) {
            let center = self.scene.mesh.bounds.center();
            let planes: [f32; 3] = std::array::from_fn(|axis| options.mirror_plane(axis, center));
            let key = planes.map(f32::to_bits);
            let sections = {
                let mut cache = self.scene.symmetry_sections.lock().unwrap_or_else(|e| e.into_inner());
                match &*cache {
                    Some((k, s)) if *k == key => s.clone(),
                    _ => {
                        let s = Arc::new(crate::guides::symmetry_sections(&self.scene.mesh, planes));
                        *cache = Some((key, s.clone()));
                        s
                    }
                }
            };
            for (guide, _) in sections.iter().zip(options.symmetry).filter(|(_, enabled)| *enabled) {
                guides.intersections.extend(guide.intersection.iter().copied());
                guides.planes.push(guide.plane);
            }
        }
        let mirrors = options.symmetry.iter().filter(|&&m| m).count();
        guides.axis = crate::guides::radial_guides(&self.scene.mesh, &options.radial, options.radial.effective_count(mirrors));
        guides
    }

    /// Every guide line of the active symmetry: plane intersections, plane outlines, radial axis.
    pub fn symmetry_guide_segments(&self) -> Vec<[Vec3; 2]> {
        self.symmetry_guides().segments(true, true, true)
    }

    /// Where the active symmetry also paints a stroke point `p` (object space), one point per
    /// copy, without `p` itself.
    pub fn symmetry_copies(&self, p: Vec3) -> Vec<Vec3> {
        let options = &self.brush.options;
        if !options.has_symmetry() { return Vec::new(); }
        options.copy_transforms(self.scene.mesh.bounds.center()).iter().skip(1).map(|t| t.transform_point3(p)).collect()
    }
    pub fn document(&self) -> &Document {
        &self.doc
    }
    /// Fold the last `n` history steps into one undo step (see [`History::squash_last`]).
    pub fn squash_history(&mut self, n: usize) -> bool {
        self.history.squash_last(n)
    }

    pub fn history(&self) -> &History {
        &self.history
    }
    pub fn selection(&self) -> Selection {
        self.selection
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn layer_target(&self) -> LayerTarget {
        if self.brush.target.is_mask() && self.selected_layer().is_some_and(|l| l.mask.is_some()) {
            LayerTarget::Mask
        } else { LayerTarget::Content }
    }

    /// The selected effect, if it still exists in the selected layer's current domain stack.
    pub fn selected_effect(&self) -> Option<EffectId> {
        let id = self.effect?;
        let layer = self.selected_layer()?;
        let exists = match self.layer_target() {
            LayerTarget::Content => layer.content_effects.iter().any(|e| e.id == id),
            LayerTarget::Mask => layer.mask.as_ref().is_some_and(|m| m.effects.iter().any(|e| e.id == id)),
        };
        exists.then_some(id)
    }

    /// The selected effect when it sits in `layer`'s `target` stack: new and pasted effects go
    /// right above it (Painter).
    fn selected_effect_in(&self, layer: LayerId, target: LayerTarget) -> Option<EffectId> {
        (self.selection.layer == Some(layer) && self.layer_target() == target).then(|| self.selected_effect()).flatten()
    }

    /// Effect ids of one of a layer's stacks, bottom to top.
    fn stack_ids(layer: &Layer, target: LayerTarget) -> Result<Vec<EffectId>, CommandError> {
        Ok(match target {
            LayerTarget::Content => layer.content_effects.iter().map(|e| e.id).collect(),
            LayerTarget::Mask => layer.mask.as_ref().ok_or(CommandError::NoMask)?.effects.iter().map(|e| e.id).collect(),
        })
    }

    /// Brush target for selecting `effect` (or the domain itself) in `target` of `layer`.
    fn target_for(layer: &Layer, target: LayerTarget, effect: Option<EffectId>) -> Result<PaintTarget, CommandError> {
        let missing = || CommandError::Invalid("No such effect in this stack".into());
        Ok(match (target, effect) {
            (LayerTarget::Content, None) => PaintTarget::Content,
            (LayerTarget::Mask, None) => {
                layer.mask.as_ref().ok_or(CommandError::NoMask)?;
                PaintTarget::MaskBase
            }
            (LayerTarget::Content, Some(id)) => match &layer.content_effects.iter().find(|e| e.id == id).ok_or_else(missing)?.kind {
                ContentEffectKind::Paint { .. } => PaintTarget::ContentEffect(id),
                _ => PaintTarget::Content,
            },
            (LayerTarget::Mask, Some(id)) => {
                let mask = layer.mask.as_ref().ok_or(CommandError::NoMask)?;
                match &mask.effects.iter().find(|e| e.id == id).ok_or_else(missing)?.kind {
                    subcraft_doc::MaskEffectKind::Paint { .. } => PaintTarget::MaskEffect(id),
                    _ => PaintTarget::MaskBase,
                }
            }
        })
    }

    /// Keep the brush target and effect selection valid after any change (undo, removal…).
    fn normalize_target(&mut self) {
        let Some(layer) = self.selected_layer() else {
            self.brush.target = PaintTarget::Content;
            self.effect = None;
            return;
        };
        let domain = if self.brush.target.is_mask() && layer.mask.is_some() { LayerTarget::Mask } else { LayerTarget::Content };
        let effect = self.selected_effect();
        let target = Self::target_for(layer, domain, effect).unwrap_or(PaintTarget::Content);
        self.effect = effect;
        self.brush.target = target;
    }
    pub fn can_paste_mask(&self) -> bool { self.mask_clipboard.is_some() }

    /// Texture sets hidden in the viewport (view state). In Focus mode every set but the selected one is hidden.
    pub fn is_set_hidden(&self, ts: TextureSetId) -> bool {
        self.hidden_sets.contains(&ts) || (self.focus_set && ts != self.selection.texture_set)
    }

    /// Whether the Texture Set List Focus mode is on.
    pub fn texture_set_focus(&self) -> bool { self.focus_set }

    /// The name of the mesh material a texture set paints: what "Reset name" goes back to.
    pub fn original_set_name(&self, ts: TextureSetId) -> Option<String> {
        let slot = self.doc.texture_set(ts)?.material_slot as usize;
        self.scene().mesh.materials.get(slot).map(|m| m.name.trim().to_string()).filter(|n| !n.is_empty())
    }

    /// Copy layers of the selected texture set into a clip for the in-app clipboard.
    pub fn copy_layers(&self, layers: &[LayerId]) -> Result<crate::clipboard::LayerClip, CommandError> {
        let first = *layers.first().ok_or(CommandError::LayerNotFound)?;
        let ts = self.doc.texture_set(self.texture_set_of(first).ok_or(CommandError::LayerNotFound)?).ok_or(CommandError::TextureSetNotFound)?;
        crate::clipboard::LayerClip::capture(ts, layers, &self.resources, crate::clipboard::mesh_signature(&self.scene.mesh)).ok_or(CommandError::LayerNotFound)
    }

    /// Copy effects of one layer's content stack (`LayerTarget::Content`) or mask stack.
    pub fn copy_effects(&self, layer: LayerId, target: LayerTarget, effects: &[EffectId]) -> Result<crate::clipboard::EffectClip, CommandError> {
        let ts = self.doc.texture_set(self.texture_set_of(layer).ok_or(CommandError::LayerNotFound)?).ok_or(CommandError::TextureSetNotFound)?;
        let clip = match target {
            LayerTarget::Content => crate::clipboard::EffectClip::capture_content(ts, layer, effects, &self.resources),
            LayerTarget::Mask => crate::clipboard::EffectClip::capture_mask(ts, layer, effects, &self.resources),
        };
        clip.ok_or_else(|| CommandError::Invalid("Select an effect to copy".into()))
    }

    /// Sets the layers of a multi-select operation live in; all must share one.
    fn common_set(&self, layers: &[LayerId]) -> Result<TextureSetId, CommandError> {
        let first = *layers.first().ok_or(CommandError::LayerNotFound)?;
        let set = self.texture_set_of(first).ok_or(CommandError::LayerNotFound)?;
        if layers.iter().any(|&l| self.texture_set_of(l) != Some(set)) {
            return Err(CommandError::Invalid("The selected layers are in different texture sets".into()));
        }
        // Layers inside an instance are its source's: the next sync undoes any change to them
        // (QA 2026-11: hiding or deleting several left an empty undo step; grouping one out of
        // an instance gave two layers with the same id). The instance itself is fine.
        if layers.iter().any(|&l| self.inside_instance_content(l)) {
            return Err(CommandError::Invalid(INSTANCE_READ_ONLY.into()));
        }
        Ok(set)
    }

    /// Whether `layer` is inside an instance (not the instance layer itself).
    fn inside_instance_content(&self, layer: LayerId) -> bool {
        self.texture_set_of(layer)
            .and_then(|ts| self.doc.texture_set(ts))
            .and_then(|ts| subcraft_doc::instance::root_of(ts, layer))
            .is_some_and(|root| root != layer)
    }
    pub fn resources(&self) -> &ResourceStore {
        &self.resources
    }

    pub(crate) fn resources_mut(&mut self) -> &mut ResourceStore {
        &mut self.resources
    }
    pub fn is_stroking(&self) -> bool {
        self.stroke.is_some()
    }

    pub fn selected_texture_set(&self) -> &TextureSet {
        self.doc.texture_set(self.selection.texture_set).unwrap_or(&self.doc.texture_sets[0])
    }

    pub fn selected_layer(&self) -> Option<&Layer> {
        self.selection.layer.and_then(|id| self.selected_texture_set().find(id))
    }

    /// The newest composite of a texture set. With background compositing it can trail the
    /// document by a few frames; [`Session::update_composites`] makes it exact.
    pub fn composite(&self, ts: TextureSetId) -> Option<&Composite> {
        match self.worker {
            Some(_) => self.shown.get(&ts),
            None => self.composites.get(&ts).and_then(|c| c.composite()),
        }
    }

    /// Size and hit counters of the per-layer result caches (inline compositing; with the
    /// background worker the caches live on its thread and this is empty).
    pub fn layer_cache_stats(&self) -> subcraft_compositor::PrefixStats {
        let mut total = subcraft_compositor::PrefixStats::default();
        for c in self.composites.values() {
            let s = c.layer_cache_stats();
            total.entries += s.entries;
            total.bytes += s.bytes;
            total.hits += s.hits;
            total.misses += s.misses;
        }
        total
    }

    /// Publication generation, so consumers can detect results installed by another caller
    /// (for example export or the eyedropper) even if it consumed the tile notifications.
    pub fn composite_generation(&self) -> u64 {
        self.composite_generation
    }

    pub(crate) fn texture_set_of(&self, layer: LayerId) -> Option<TextureSetId> {
        self.doc.texture_sets.iter().find(|t| t.find(layer).is_some()).map(|t| t.id)
    }

    /// Surface point under a screen position.
    pub fn pick(&self, camera: &Camera, screen: Vec2) -> Option<Pick> {
        let (o, d) = camera.ray(screen);
        let hit = self.viewport_scene().bvh.intersect(o, d, 0.0, f32::INFINITY)?;
        let sp = self.scene.mesh.interpolate(hit.tri as usize, hit.u, hit.v);
        Some(Pick {
            uv: sp.uv,
            position: sp.position,
            normal: sp.normal,
            triangle: hit.tri,
            material_slot: self.scene.mesh.tri_material[hit.tri as usize],
            depth: camera.view_depth(sp.position),
        })
    }

    /// World radius of the current brush in Object size space (see [`Session::brush_radius_in`]).
    pub fn brush_radius(&self) -> f32 {
        self.brush.settings.radius(self.scene.mesh.bounds.diagonal())
    }

    /// World radius of the current brush in its size space: `pixels_per_unit` and
    /// `viewport_height` describe the view under the cursor, `uv_per_unit` the painted texture
    /// set's texel density (0 = unknown; that space then uses the object size).
    pub fn brush_radius_in(&self, pixels_per_unit: f32, viewport_height: f32, uv_per_unit: f32) -> f32 {
        self.brush.settings.radius_in(&subcraft_paint::SizeContext { diagonal: self.scene.mesh.bounds.diagonal(), pixels_per_unit, viewport_height, uv_per_unit })
    }

    /// Surface position corresponding to a texture UV, for displaying editable path overlays.
    /// The geometry cache shares the compositor's UV wrapping and texel convention.
    pub fn pen_uv_position(&self, uv: Vec2) -> Option<Vec3> {
        if !uv.is_finite() || uv.min_element() < 0.0 || uv.max_element() > 1.0 { return None; }
        let ts = self.selected_texture_set(); let r = ts.resolution;
        let xy = (uv * r as f32).as_uvec2().min(glam::UVec2::splat(r - 1));
        let size = subcraft_image::TILE_SIZE;
        let geometry = self.scene.geometry.get(ts.material_slot, r);
        let tile = geometry.tile((xy.y / size * r.div_ceil(size) + xy.x / size) as usize);
        let i = (xy.y % size * size + xy.x % size) as usize;
        if tile.coverage(i) == 0.0 { return None; }
        let bounds = &self.scene.mesh.bounds;
        Some(bounds.min + Vec3::from_array(tile.position(i)) * (bounds.max - bounds.min).max_element())
    }

    // ----- commands ------------------------------------------------------------------------------

    /// Refresh the instances from their sources (and drop those whose source was just deleted):
    /// called before every history entry, so one undo step covers both.
    fn sync_instances(&mut self) {
        if subcraft_doc::instance::sync(&mut self.doc, Some(self.history.current()), &mut self.instances)
            && let Some(l) = self.selection.layer
            && self.doc.texture_set(self.selection.texture_set).is_some_and(|ts| ts.find(l).is_none())
        {
            self.selection.layer = None;
        }
    }

    /// The layers to instantiate, each with its texture set, in document order; layers inside
    /// another listed layer come with it.
    fn instance_sources(&self, ids: &[LayerId]) -> Vec<(TextureSetId, Layer)> {
        let mut out = Vec::new();
        for ts in &self.doc.texture_sets {
            for id in crate::clipboard::top_level_in_order(ts, ids) {
                if let Some(l) = ts.find(id) {
                    out.push((ts.id, l.clone()));
                }
            }
        }
        out
    }

    /// Channels these layers blend on that `texture_set` does not have: an instance there
    /// cannot show them (Painter's warning mark in the Instantiate dialog).
    pub fn instance_channel_gaps(&self, layers: &[LayerId], texture_set: TextureSetId) -> Vec<ChannelKind> {
        fn walk(l: &Layer, ts: &TextureSet, out: &mut Vec<ChannelKind>) {
            for (&k, b) in &l.blend {
                if b.is_active() && !ts.has_channel(k) && !out.contains(&k) {
                    out.push(k);
                }
            }
            for c in l.children() {
                walk(c, ts, out);
            }
        }
        let mut out = Vec::new();
        if let Some(ts) = self.doc.texture_set(texture_set) {
            for (_, l) in self.instance_sources(layers) {
                walk(&l, ts, &mut out);
            }
        }
        out
    }

    /// Whether `layer` is an instance or inside one: its content cannot be edited.
    pub fn instance_locked(&self, layer: LayerId) -> bool {
        self.texture_set_of(layer).and_then(|ts| self.doc.texture_set(ts)).is_some_and(|ts| subcraft_doc::instance::root_of(ts, layer).is_some())
    }

    /// Instance link of `layer` (`None`: an ordinary layer).
    pub fn instance_state(&self, layer: LayerId) -> Option<subcraft_doc::InstanceState> {
        let ts = self.doc.texture_set(self.texture_set_of(layer)?)?;
        subcraft_doc::instance::state(&self.doc, ts.find(layer)?)
    }

    fn commit(&mut self, label: &str, merge_key: Option<String>) {
        self.sync_instances();
        // Recent actions end up in the log (and in crash reports). Slider drags merge into one
        // history entry; log only the first step.
        if merge_key.is_none() || self.history.current_merge_key() != merge_key.as_deref() {
            log::info!("action: {label}");
        }
        self.history.push_merge(label, self.doc.clone(), merge_key);
        self.revision += 1;
    }

    /// Apply `f` to the texture set containing `layer`, then record history.
    pub(crate) fn edit_layer_set<R>(
        &mut self,
        layer: LayerId,
        label: &str,
        merge_key: Option<String>,
        f: impl FnOnce(&mut TextureSet) -> Result<R, CommandError>,
    ) -> Result<R, CommandError> {
        let ts_id = self.texture_set_of(layer).ok_or(CommandError::LayerNotFound)?;
        let guard = self.instance_guard(ts_id, layer);
        let ts = self.doc.texture_set_mut(ts_id).ok_or(CommandError::TextureSetNotFound)?;
        let r = f(ts)?;
        self.check_instance_guard(ts_id, guard)?;
        self.commit(label, merge_key);
        Ok(r)
    }

    /// `reference` for an insertion, moved out of any instance it is in (to the outermost
    /// instance): a layer added inside one was wiped by the next sync with its source, leaving an
    /// empty undo step (QA 2026-11).
    fn outside_instances(ts: &TextureSet, reference: Option<LayerId>) -> Option<LayerId> {
        reference.map(|r| subcraft_doc::instance::root_of(ts, r).unwrap_or(r))
    }

    /// Whether a layer moved into `parent` would land in an instance (and be replaced by the
    /// source's content on the next sync, or, the source itself, take the instance with it).
    fn inside_instance(ts: &TextureSet, parent: Option<LayerId>) -> bool {
        parent.is_some_and(|p| subcraft_doc::instance::root_of(ts, p).is_some())
    }

    /// The layers of `ts_id` as they are, when `layer` is an instance or inside one.
    fn instance_guard(&self, ts_id: TextureSetId, layer: LayerId) -> Option<Vec<Arc<Layer>>> {
        let ts = self.doc.texture_set(ts_id)?;
        subcraft_doc::instance::root_of(ts, layer).map(|_| ts.layers.clone())
    }

    /// An edit of an instance that its source undoes right away changed nothing: refuse it with
    /// a message instead of recording an empty history step.
    fn check_instance_guard(&mut self, ts_id: TextureSetId, guard: Option<Vec<Arc<Layer>>>) -> Result<(), CommandError> {
        let Some(before) = guard else { return Ok(()) };
        self.sync_instances();
        if self.doc.texture_set(ts_id).is_some_and(|ts| ts.layers == before) {
            return Err(CommandError::Invalid(INSTANCE_READ_ONLY.into()));
        }
        Ok(())
    }

    /// [`Self::edit_layer_set`] that adds no history entry when `f` changed nothing (a no-op
    /// click must not wipe the redo branch or mark the project modified).
    pub(crate) fn edit_layer_set_if_changed<R>(
        &mut self,
        layer: LayerId,
        label: &str,
        f: impl FnOnce(&mut TextureSet) -> Result<R, CommandError>,
    ) -> Result<R, CommandError> {
        let ts_id = self.texture_set_of(layer).ok_or(CommandError::LayerNotFound)?;
        let ts = self.doc.texture_set_mut(ts_id).ok_or(CommandError::TextureSetNotFound)?;
        let before = ts.layers.clone();
        let r = f(ts)?;
        if ts.layers != before {
            // An edit of an instance's content is undone by its source: nothing to record then.
            self.sync_instances();
            if self.doc.texture_set(ts_id).is_some_and(|ts| ts.layers == before) {
                return Err(CommandError::Invalid(INSTANCE_READ_ONLY.into()));
            }
            self.commit(label, None);
        }
        Ok(r)
    }

    pub(crate) fn layer_mut(ts: &mut TextureSet, id: LayerId) -> Result<&mut Layer, CommandError> {
        ts.find_mut(id).ok_or(CommandError::LayerNotFound)
    }

    fn sync_after_history_move(&mut self) {
        self.doc = self.history.current().clone();
        self.sync_id_maps();
        self.fit_quick_mask();
        if self.doc.texture_set(self.selection.texture_set).is_none() {
            self.selection.texture_set = self.doc.texture_sets[0].id;
        }
        if let Some(l) = self.selection.layer
            && self.selected_texture_set().find(l).is_none()
        {
            self.selection.layer = self.selected_texture_set().layers.last().map(|l| l.id);
        }
        self.revision += 1;
    }

    pub fn execute(&mut self, cmd: Command) -> Result<(), CommandError> {
        if self.stroke.is_some() && !matches!(cmd, Command::Seal) {
            return Err(CommandError::StrokeInProgress);
        }
        match cmd {
            Command::SetBrushSettings(mut settings) => {
                if [settings.size, settings.flow, settings.stroke_opacity, settings.hardness, settings.scatter_radius, settings.scatter_size_random,
                    settings.scatter_rotation_deg, settings.scatter_opacity_random, settings.scatter_fall,
                    settings.smudge_strength, settings.smudge_carry, settings.smudge_falloff, settings.smudge_material].iter().any(|v| !v.is_finite()) {
                    return Err(CommandError::Invalid("brush values must be finite".into()));
                }
                settings.size = settings.size.clamp(0.1, 100.0);
                settings.flow = settings.flow.clamp(0.0, 1.0);
                settings.stroke_opacity = settings.stroke_opacity.clamp(0.0, 1.0);
                settings.hardness = settings.hardness.clamp(0.0, 1.0);
                settings.scatter_count = settings.scatter_count.min(subcraft_paint::MAX_SCATTER);
                settings.scatter_radius = settings.scatter_radius.clamp(0.0, 10.0);
                settings.scatter_size_random = settings.scatter_size_random.clamp(0.0, 1.0);
                settings.scatter_opacity_random = settings.scatter_opacity_random.clamp(0.0, 1.0);
                settings.scatter_fall = settings.scatter_fall.clamp(0.0, 20.0);
                settings.smudge_strength = settings.smudge_strength.clamp(0.0, 1.0);
                settings.smudge_carry = settings.smudge_carry.clamp(0.0, 0.99);
                settings.smudge_falloff = settings.smudge_falloff.clamp(0.25, 8.0);
                settings.smudge_material = settings.smudge_material.clamp(0.0, 1.0);
                self.brush.settings = settings;
            }
            Command::SetStrokeOptions(mut options) => {
                if !options.lazy_distance_px.is_finite() {
                    return Err(CommandError::Invalid("lazy mouse distance must be finite".into()));
                }
                if !options.radial.center_offset.iter().all(|v| v.is_finite()) {
                    return Err(CommandError::Invalid("radial symmetry center must be finite".into()));
                }
                if !options.mirror_offset.iter().all(|v| v.is_finite()) {
                    return Err(CommandError::Invalid("mirror plane position must be finite".into()));
                }
                if !options.smoothing.is_finite() {
                    return Err(CommandError::Invalid("smoothing must be finite".into()));
                }
                options.lazy_distance_px = options.lazy_distance_px.clamp(0.0, 100.0);
                options.smoothing = options.smoothing.clamp(0.0, subcraft_paint::MAX_SMOOTHING);
                options.radial.axis = options.radial.axis.min(2);
                options.radial.count = options.radial.count.clamp(2, subcraft_paint::MAX_SYMMETRY_COPIES);
                self.brush.options = options;
            }
            Command::SetBrushMaterial(material) => self.brush.material = material,
            Command::InsertPartLayers { layers, label } => {
                if layers.iter().any(|(id, _)| self.doc.texture_set(*id).is_none()) { return Err(CommandError::TextureSetNotFound); }
                let mut doc = self.doc.clone();
                for (id, layer) in layers {
                    let ts = doc.texture_set_mut(id).expect("validated");
                    let reference = if id == self.selection.texture_set { self.selection.layer } else { None };
                    let inserted = ts.insert_above(reference, *layer);
                    if id == self.selection.texture_set { self.selection.layer = Some(inserted); }
                }
                self.doc = doc; self.commit(&label, None);
            }
            Command::ReplaceTextureSetLayers { texture_set, layers, label } => {
                let mut doc = self.doc.clone();
                doc.texture_set_mut(texture_set).ok_or(CommandError::TextureSetNotFound)?.layers = layers;
                self.doc = doc;
                if self.selection.texture_set == texture_set && self.selection.layer.is_some_and(|l| self.doc.texture_set(texture_set).and_then(|ts| ts.find(l)).is_none()) {
                    self.selection.layer = None;
                    self.effect = None;
                }
                self.commit(&label, None);
            }
            Command::ReplaceLayer { layer, replacement, label } => {
                if layer != replacement.id { return Err(CommandError::Invalid("replacement must keep layer UUID".into())); }
                self.edit_layer_set(layer, &label, None, |ts| { *Self::layer_mut(ts, layer)? = *replacement; Ok(()) })?;
            }
            Command::Undo => {
                if self.history.undo().is_some() {
                    self.sync_after_history_move();
                }
            }
            Command::Redo => {
                if self.history.redo().is_some() {
                    self.sync_after_history_move();
                }
            }
            Command::JumpToHistory(i) => {
                if self.history.jump(i).is_some() {
                    self.sync_after_history_move();
                }
            }
            Command::Seal => self.history.seal(),
            Command::SelectTextureSet(id) => {
                let ts = self.doc.texture_set(id).ok_or(CommandError::TextureSetNotFound)?;
                self.selection = Selection { texture_set: id, layer: ts.layers.last().map(|l| l.id) };
                self.brush.target = PaintTarget::Content;
                self.effect = None;
                self.revision += 1;
            }
            Command::SelectLayer(layer) => {
                if let Some(l) = layer {
                    self.selection.texture_set = self.texture_set_of(l).ok_or(CommandError::LayerNotFound)?;
                }
                self.selection.layer = layer;
                self.brush.target = PaintTarget::Content;
                self.effect = None;
                self.revision += 1;
            }
            Command::SelectLayerTarget { layer, target } => {
                let ts = self.texture_set_of(layer).ok_or(CommandError::LayerNotFound)?;
                if target == LayerTarget::Mask && self.doc.texture_set(ts).and_then(|ts| ts.find(layer)).is_none_or(|l| l.mask.is_none()) {
                    return Err(CommandError::NoMask);
                }
                self.selection = Selection { texture_set: ts, layer: Some(layer) };
                self.brush.target = if target == LayerTarget::Mask { PaintTarget::MaskBase } else { PaintTarget::Content };
                self.effect = None;
                self.revision += 1;
            }
            Command::SelectEffect { layer, target, effect } => {
                let ts = self.texture_set_of(layer).ok_or(CommandError::LayerNotFound)?;
                let l = self.doc.texture_set(ts).and_then(|t| t.find(layer)).ok_or(CommandError::LayerNotFound)?;
                let paint_target = Self::target_for(l, target, effect)?;
                self.selection = Selection { texture_set: ts, layer: Some(layer) };
                self.brush.target = paint_target;
                self.effect = effect;
                self.revision += 1;
            }
            Command::AddContentEffect { layer, kind } => {
                Self::check_content_effect(&kind)?;
                let above = self.selected_effect_in(layer, LayerTarget::Content);
                let id = self.edit_layer_set(layer, "Add Effect", None, |ts| {
                    let mut effect = ContentEffect::new(kind, ts);
                    if matches!(effect.kind, ContentEffectKind::Anchor) {
                        effect.name = Self::new_anchor_name(ts, layer);
                    }
                    let l = Self::layer_mut(ts, layer)?;
                    if l.is_folder() && !effect.kind.is_adjustment() && !matches!(effect.kind, ContentEffectKind::Anchor) {
                        return Err(CommandError::Invalid("Folders support adjustment effects only".into()));
                    }
                    let id = effect.id;
                    let at = above.and_then(|a| l.content_effects.iter().position(|e| e.id == a)).map_or(l.content_effects.len(), |i| i + 1);
                    l.content_effects.insert(at, Arc::new(effect));
                    Ok(id)
                })?;
                return self.execute(Command::SelectEffect { layer, target: LayerTarget::Content, effect: Some(id) });
            }
            Command::RemoveContentEffect { layer, effect } => {
                self.edit_layer_set(layer, "Remove Effect", None, |ts| {
                    let l = Self::layer_mut(ts, layer)?;
                    let index = l.content_effects.iter().position(|e| e.id == effect).ok_or_else(|| CommandError::Invalid("No such effect".into()))?;
                    l.content_effects.remove(index);
                    Ok(())
                })?;
            }
            Command::SetContentEffect { layer, effect } => {
                Self::check_content_effect(&effect.kind)?;
                let key = format!("content-effect:{layer}:{}", effect.id);
                self.edit_layer_set(layer, "Effect", Some(key), |ts| {
                    let l = Self::layer_mut(ts, layer)?;
                    if l.is_folder() && !effect.kind.is_adjustment() && !matches!(effect.kind, ContentEffectKind::Anchor) { return Err(CommandError::Invalid("Folders support adjustment effects only".into())); }
                    let slot = l.content_effects.iter_mut().find(|e| e.id == effect.id).ok_or_else(|| CommandError::Invalid("No such effect".into()))?;
                    *slot = Arc::new(effect);
                    Ok(())
                })?;
            }
            Command::MoveContentEffect { layer, effect, index } => {
                self.edit_layer_set(layer, "Move Effect", None, |ts| {
                    let l = Self::layer_mut(ts, layer)?;
                    if index >= l.content_effects.len() { return Err(CommandError::Invalid("Effect destination is outside the stack".into())); }
                    let from = l.content_effects.iter().position(|e| e.id == effect).ok_or_else(|| CommandError::Invalid("No such effect".into()))?;
                    let moved = l.content_effects.remove(from);
                    l.content_effects.insert(index, moved);
                    Ok(())
                })?;
            }
            Command::DropEffect { source, target, side } => {
                if source.texture_set != self.selection.texture_set {
                    return Err(CommandError::Invalid("the dragged texture set is no longer selected".into()));
                }
                let ts=self.doc.texture_set(source.texture_set).ok_or(CommandError::TextureSetNotFound)?;
                if let Some(index)=crate::effect_stack::drop_destination(ts,source,target,side) {
                    let moved=match source.domain {
                        LayerTarget::Content=>Command::MoveContentEffect {layer:source.layer,effect:source.effect,index},
                        LayerTarget::Mask=>Command::MoveMaskEffect {layer:source.layer,effect:source.effect,index},
                    };
                    self.execute(moved)?;
                    return self.execute(Command::SelectEffect {layer:source.layer,target:source.domain,effect:Some(source.effect)});
                }
            }
            Command::DuplicateContentEffect { layer, effect } => {
                self.edit_layer_set(layer, "Duplicate Effect", None, |ts| {
                    let l = Self::layer_mut(ts, layer)?;
                    let index = l.content_effects.iter().position(|e| e.id == effect).ok_or_else(|| CommandError::Invalid("No such effect".into()))?;
                    let mut copy = (*l.content_effects[index]).clone();
                    copy.id = subcraft_core::EffectId::new();
                    copy.name = format!("{} copy", copy.name);
                    l.content_effects.insert(index + 1, Arc::new(copy));
                    Ok(())
                })?;
            }
            Command::RemoveEffects { layer, target, effects } => {
                let label = if effects.len() == 1 { "Remove Effect" } else { "Remove Effects" };
                self.edit_layer_set(layer, label, None, |ts| {
                    let l = Self::layer_mut(ts, layer)?;
                    let removed = on_stack!(l, target, |s| crate::effect_stack::take(s, &effects).len());
                    if removed == 0 { Err(CommandError::Invalid("No such effect".into())) } else { Ok(()) }
                })?;
            }
            Command::DuplicateEffects { layer, target, effects } => {
                let label = if effects.len() == 1 { "Duplicate Effect" } else { "Duplicate Effects" };
                let new = self.edit_layer_set(layer, label, None, |ts| {
                    let l = Self::layer_mut(ts, layer)?;
                    let new = on_stack!(l, target, |s| crate::effect_stack::duplicate_in_place(s, &effects));
                    if new.is_empty() { Err(CommandError::Invalid("No such effect".into())) } else { Ok(new) }
                })?;
                return self.execute(Command::SelectEffect { layer, target, effect: new.last().copied() });
            }
            Command::NudgeEffects { layer, target, effects, up } => {
                let ts = self.texture_set_of(layer).and_then(|t| self.doc.texture_set(t)).ok_or(CommandError::LayerNotFound)?;
                let ids = Self::stack_ids(ts.find(layer).ok_or(CommandError::LayerNotFound)?, target)?;
                if !effects.iter().any(|e| ids.contains(e)) {
                    return Err(CommandError::Invalid("No such effect".into()));
                }
                if let Some(order) = crate::effect_stack::nudged(&ids, &effects, up) {
                    self.edit_layer_set(layer, "Move Effect", None, |ts| {
                        let l = Self::layer_mut(ts, layer)?;
                        on_stack!(l, target, |s| crate::effect_stack::apply_order(s, &order));
                        Ok(())
                    })?;
                }
            }
            Command::RenameEffect { layer, target, effect, name } => {
                let name = name.trim().to_string();
                if name.is_empty() { return Err(CommandError::Invalid("An effect needs a name".into())); }
                self.edit_layer_set_if_changed(layer, "Rename Effect", |ts| {
                    use crate::effect_stack::StackEffect;
                    let l = Self::layer_mut(ts, layer)?;
                    let found = on_stack!(l, target, |s| match s.iter_mut().find(|e| e.id() == effect) {
                        Some(e) => { *Arc::make_mut(e).name_mut() = name; true }
                        None => false,
                    });
                    if found { Ok(()) } else { Err(CommandError::Invalid("No such effect".into())) }
                })?;
            }
            Command::DropEffects { source, effects, to, copy } => {
                use crate::effect_stack::{EffectDrop, DropSide};
                if source.texture_set != self.selection.texture_set {
                    return Err(CommandError::Invalid("the dragged texture set is no longer selected".into()));
                }
                let ts = self.doc.texture_set(source.texture_set).ok_or(CommandError::TextureSetNotFound)?;
                // Stale drags (the layer, its mask or the grabbed effect is gone) do nothing.
                let Some(from) = ts.find(source.layer) else { return Ok(()) };
                let Ok(ids) = Self::stack_ids(from, source.domain) else { return Ok(()) };
                if !ids.contains(&source.effect) { return Ok(()); }
                let moving: Vec<EffectId> = ids.iter().copied().filter(|id| *id == source.effect || effects.contains(id)).collect();
                let (layer, domain, grabbed) = (source.layer, source.domain, source.effect);
                let label = match (copy, moving.len() == 1) {
                    (false, true) => "Move Effect",
                    (false, false) => "Move Effects",
                    (true, true) => "Duplicate Effect",
                    (true, false) => "Duplicate Effects",
                };
                // Moving to the top of its own stack is a reorder like any other.
                let to = match to {
                    EffectDrop::Layer(dest) if dest == layer && !copy => match ids.iter().rev().find(|id| !moving.contains(id)) {
                        Some(top) => EffectDrop::Beside { effect: *top, side: DropSide::Above },
                        None => return Ok(()),
                    },
                    other => other,
                };
                let (dest, selected) = match to {
                    EffectDrop::Beside { effect: anchor, side } if !copy => {
                        let Some(order) = crate::effect_stack::reordered(&ids, &moving, anchor, side) else { return Ok(()) };
                        self.edit_layer_set(layer, label, None, |ts| {
                            let l = Self::layer_mut(ts, layer)?;
                            on_stack!(l, domain, |s| crate::effect_stack::apply_order(s, &order));
                            Ok(())
                        })?;
                        (layer, grabbed)
                    }
                    EffectDrop::Beside { effect: anchor, side } => {
                        if !ids.contains(&anchor) { return Ok(()); }
                        let selected = self.edit_layer_set(layer, label, None, |ts| {
                            let l = Self::layer_mut(ts, layer)?;
                            Ok(on_stack!(l, domain, |s| {
                                let (items, selected) = crate::effect_stack::lift(s, &moving, true, grabbed);
                                let at = crate::effect_stack::stack_ids(s).iter().position(|id| *id == anchor).expect("checked above") + usize::from(side == DropSide::Above);
                                s.splice(at..at, items);
                                selected
                            }))
                        })?;
                        (layer, selected)
                    }
                    EffectDrop::Layer(dest) => {
                        let Some(to_layer) = ts.find(dest) else { return Ok(()) };
                        if domain == LayerTarget::Content && to_layer.is_folder()
                            && from.content_effects.iter().any(|e| moving.contains(&e.id) && !e.kind.is_adjustment() && !matches!(e.kind, ContentEffectKind::Anchor))
                        {
                            return Err(CommandError::Invalid("Folders support adjustment effects only".into()));
                        }
                        let selected = self.edit_layer_set(layer, label, None, |ts| {
                            let res = ts.resolution;
                            Ok(match domain {
                                LayerTarget::Content => {
                                    let (items, selected) = crate::effect_stack::lift(&mut Self::layer_mut(ts, layer)?.content_effects, &moving, copy, grabbed);
                                    Self::layer_mut(ts, dest)?.content_effects.extend(items);
                                    selected
                                }
                                LayerTarget::Mask => {
                                    let (items, selected) = crate::effect_stack::lift(&mut Self::layer_mut(ts, layer)?.mask.as_mut().ok_or(CommandError::NoMask)?.effects, &moving, copy, grabbed);
                                    // Mask effects dropped on a layer without a mask give it one.
                                    Self::layer_mut(ts, dest)?.mask.get_or_insert_with(|| Mask::new(res, true)).effects.extend(items);
                                    selected
                                }
                            })
                        })?;
                        (dest, selected)
                    }
                };
                return self.execute(Command::SelectEffect { layer: dest, target: domain, effect: Some(selected) });
            }
            Command::AddLayer(kind) => {
                self.layer_counter += 1;
                let ts_id = self.selection.texture_set;
                let reference = self.selection.layer;
                let ts = self.doc.texture_set_mut(ts_id).ok_or(CommandError::TextureSetNotFound)?;
                let tc = ts.clone();
                let (layer, label) = match kind {
                    NewLayer::Paint => (Layer::new_paint(format!("Layer {}", self.layer_counter), &tc), "New Paint Layer"),
                    NewLayer::Fill => {
                        let mut fill = Layer::new_fill(format!("Fill {}", self.layer_counter), &tc);
                        let s = if self.default_uv_scale.is_finite() && self.default_uv_scale > 0.0 { self.default_uv_scale } else { 1.0 };
                        if let subcraft_doc::LayerContent::Fill(f) = &mut fill.content {
                            f.projection.scale = [s, s];
                        }
                        (fill, "New Fill Layer")
                    }
                    NewLayer::Folder => (Layer::new_folder(format!("Folder {}", self.layer_counter), &tc), "New Folder"),
                };
                // Never inside an instance (its content is its source's): above it instead.
                let locked = reference.and_then(|r| subcraft_doc::instance::root_of(ts, r));
                let reference = locked.or(reference);
                // Painter: with an empty folder selected, the new layer goes inside it.
                let empty_folder = reference.filter(|&r| locked.is_none() && ts.find(r).is_some_and(|l| l.is_folder() && l.children().is_empty()));
                let id = match empty_folder {
                    Some(folder) => ts.insert(Some(folder), 0, layer).map_err(|_| CommandError::LayerNotFound)?,
                    None => ts.insert_above(reference, layer),
                };
                self.selection.layer = Some(id);
                self.commit(label, None);
            }
            Command::InsertLayer { layer, label } => {
                let ts_id = self.selection.texture_set;
                let reference = self.selection.layer;
                let ts = self.doc.texture_set_mut(ts_id).ok_or(CommandError::TextureSetNotFound)?;
                let mut layer = *layer;
                if ts.find(layer.id).is_some() {
                    layer = layer.duplicate_with_new_ids();
                }
                let reference = Self::outside_instances(ts, reference);
                let id = ts.insert_above(reference, layer);
                self.selection.layer = Some(id);
                self.commit(&label, None);
            }
            Command::SetTransparent { texture_set, transparent } => {
                let ts = self.doc.texture_set_mut(texture_set).ok_or(CommandError::TextureSetNotFound)?;
                if ts.transparent != transparent {
                    ts.transparent = transparent;
                    self.commit(if transparent { "Transparent texture set" } else { "Opaque texture set" }, None);
                }
            }
            Command::SetAlphaSettings { texture_set, alpha } => {
                // NaN survives clamp, is saved as null and the project no longer opens.
                if !alpha.threshold.is_finite() { return Err(CommandError::Invalid("the alpha threshold must be a number".into())); }
                let ts = self.doc.texture_set_mut(texture_set).ok_or(CommandError::TextureSetNotFound)?;
                let alpha = subcraft_doc::AlphaSettings { threshold: alpha.threshold.clamp(0.0, 1.0), ..alpha };
                if ts.alpha != alpha {
                    ts.alpha = alpha;
                    self.commit("Alpha settings", None);
                }
            }
            Command::SetAlbedoFilter { texture_set, filter } => {
                let ts = self.doc.texture_set_mut(texture_set).ok_or(CommandError::TextureSetNotFound)?;
                if ts.albedo_filter != filter {
                    ts.albedo_filter = filter;
                    self.commit("Albedo filtering", None);
                }
            }
            Command::SetResolution { texture_set, resolution } => {
                if !subcraft_doc::RESOLUTIONS.contains(&resolution) {
                    return Err(CommandError::Invalid(format!("unsupported resolution {resolution}")));
                }
                if texture_set.is_some_and(|id| self.doc.texture_set(id).is_none()) {
                    return Err(CommandError::TextureSetNotFound);
                }
                let mut changed = false;
                let mut resized = Vec::new();
                for ts in self.doc.texture_sets.iter_mut().filter(|ts| texture_set.is_none_or(|id| ts.id == id)) {
                    if ts.resolution != resolution { resized.push(ts.id); }
                    changed |= ts.resolution != resolution;
                    ts.set_resolution(resolution);
                }
                // Text layers are re-rendered from their outlines instead of resampled.
                crate::text::refresh_text_rasters(&mut self.doc, &self.resources, &resized);
                // Pen paths too: their raster is a cache of the path.
                crate::pen::refresh_path_rasters(&mut self.doc, &self.resources, &resized);
                if changed {
                    self.commit(&format!("Resolution {resolution}"), None);
                    self.fit_quick_mask();
                }
            }
            Command::AddChannel { texture_set, kind } => {
                let ts = self.doc.texture_set_mut(texture_set).ok_or(CommandError::TextureSetNotFound)?;
                if ts.has_channel(kind) {
                    return Ok(());
                }
                ts.channels.push(ChannelDef::default_for(kind));
                // Existing layers take part in the new channel at once (Painter: an added
                // Opacity channel is paintable on the current layer straight away).
                add_blend(&mut ts.layers, kind);
                self.brush.material.set(kind, kind.default_paint_value());
                self.commit(&format!("Add {} Channel", kind.display_name()), None);
            }
            Command::AddChannels { texture_set, kinds } => {
                let ts = self.doc.texture_set_mut(texture_set).ok_or(CommandError::TextureSetNotFound)?;
                let mut added: Vec<ChannelKind> = Vec::new();
                for kind in kinds {
                    if !ts.has_channel(kind) {
                        ts.channels.push(ChannelDef::default_for(kind));
                        add_blend(&mut ts.layers, kind);
                        added.push(kind);
                    }
                }
                if added.is_empty() {
                    return Ok(());
                }
                for &kind in &added {
                    self.brush.material.set(kind, kind.default_paint_value());
                }
                let label = match added.as_slice() {
                    [one] => format!("Add {} Channel", one.display_name()),
                    many => format!("Add {} Channels", many.len()),
                };
                self.commit(&label, None);
            }
            Command::RemoveChannel { texture_set, kind } => {
                let ts = self.doc.texture_set_mut(texture_set).ok_or(CommandError::TextureSetNotFound)?;
                if !ts.has_channel(kind) {
                    return Ok(());
                }
                if ts.channels.len() == 1 {
                    return Err(CommandError::Invalid("a texture set needs at least one channel".into()));
                }
                ts.channels.retain(|c| c.kind != kind);
                self.commit(&format!("Remove {} Channel", kind.display_name()), None);
            }
            Command::SetChannelFormat { texture_set, kind, format } => {
                let ts = self.doc.texture_set_mut(texture_set).ok_or(CommandError::TextureSetNotFound)?;
                let Some(def) = ts.channels.iter_mut().find(|c| c.kind == kind) else { return Err(CommandError::Invalid(format!("no {} channel", kind.display_name()))) };
                if def.format == format {
                    return Ok(());
                }
                def.format = format;
                self.commit(&format!("{} Format", kind.display_name()), None);
            }
            Command::RenameUserChannel { texture_set, kind, name } => {
                if !kind.is_user() {
                    return Err(CommandError::Invalid(format!("{} is not a user channel and keeps its name", kind.display_name())));
                }
                let ts = self.doc.texture_set_mut(texture_set).ok_or(CommandError::TextureSetNotFound)?;
                let name = name.trim().to_string();
                let changed = if name.is_empty() || name == kind.display_name() {
                    ts.user_channel_names.remove(&kind).is_some()
                } else {
                    ts.user_channel_names.insert(kind, name.clone()).as_deref() != Some(name.as_str())
                };
                if changed {
                    self.commit("Rename Channel", None);
                }
            }
            Command::InsertLayerWithChannels { layer, channels, label } => {
                let ts = self.doc.texture_set_mut(self.selection.texture_set).ok_or(CommandError::TextureSetNotFound)?;
                for kind in channels {
                    if !ts.has_channel(kind) {
                        ts.channels.push(ChannelDef::default_for(kind));
                        // Like Add Channel: without a blend entry a layer ignores the channel, and a
                        // folder without one switches it off for everything inside it.
                        add_blend(&mut ts.layers, kind);
                    }
                }
                // InsertLayer commits the document including the new channels: one history entry.
                return self.execute(Command::InsertLayer { layer, label });
            }
            Command::ApplyMaskEffects { layer, effects, label } => {
                for kind in &effects { Self::check_mask_effect(kind)?; }
                self.edit_layer_set(layer, &label, None, |ts| {
                    let res = ts.resolution;
                    let l = Self::layer_mut(ts, layer)?;
                    let mask = l.mask.get_or_insert_with(|| Mask::new(res, false));
                    mask.effects.extend(effects.into_iter().map(|k| Arc::new(MaskEffect::new(k))));
                    Ok(())
                })?;
            }
            Command::ReplaceMaskEffects { layer, effects, label } => {
                for kind in &effects { Self::check_mask_effect(kind)?; }
                self.edit_layer_set(layer, &label, None, |ts| {
                    let res = ts.resolution;
                    let l = Self::layer_mut(ts, layer)?;
                    let mask = l.mask.get_or_insert_with(|| Mask::new(res, false));
                    mask.effects = effects.into_iter().map(|k| Arc::new(MaskEffect::new(k))).collect();
                    Ok(())
                })?;
            }
            Command::DeleteLayer(id) => {
                let loc = self.edit_layer_set(id, "Delete Layer", None, |ts| {
                    let loc = ts.location(id);
                    ts.remove(id).map_err(|_| CommandError::LayerNotFound)?;
                    Ok(loc)
                })?;
                // The selected layer may have been inside the deleted folder.
                if self.selection.layer.is_some_and(|l| l == id || self.selected_texture_set().find(l).is_none()) {
                    self.selection.layer = self.neighbour_after_removal(loc);
                }
            }
            Command::DuplicateLayer(id) => {
                let copy = self.edit_layer_set(id, "Duplicate Layer", None, |ts| ts.duplicate(id).map_err(|_| CommandError::LayerNotFound))?;
                // A copy in another texture set cannot be the selection without switching sets.
                if self.selected_texture_set().find(copy).is_some() {
                    self.selection.layer = Some(copy);
                }
            }
            Command::PasteAsInstance(sources) => {
                let ts_id = self.selection.texture_set;
                // The copied layers, wherever they are now (another texture set too).
                let found = self.instance_sources(&sources);
                let ts = self.doc.texture_set_mut(ts_id).ok_or(CommandError::TextureSetNotFound)?;
                // Inside an instance nothing can be added: the new layers go above it.
                let mut at = self.selection.layer.map(|a| subcraft_doc::instance::root_of(ts, a).unwrap_or(a));
                if found.is_empty() {
                    return Err(CommandError::Invalid("Paste as instance: the copied layer is no longer in the project".into()));
                }
                // Put back when a later layer is refused: the instances already inserted used to
                // stay in the document, outside any undo step.
                let before = ts.layers.clone();
                for (set, source) in found {
                    let same = set == ts_id;
                    // An instance of an instance follows that one's source: the new layer must stay
                    // out of that layer too.
                    let follows = source.instance_of.unwrap_or(source.id);
                    if at.is_some_and(|a| (same && a != source.id && ts.is_descendant_or_self(a, source.id)) || (a != follows && ts.is_descendant_or_self(a, follows))) {
                        ts.layers = before;
                        return Err(CommandError::Invalid("An instance cannot go inside its own source: select a layer outside it first".into()));
                    }
                    let mut instance = subcraft_doc::instance::new_instance(&source, ts, same);
                    instance.name = crate::clipboard::pasted_name(ts, &format!("{} instance", source.name));
                    at = Some(ts.insert_above(at, instance));
                }
                self.selection.layer = at;
                self.commit("Paste as Instance", None);
            }
            Command::BreakInstances(layers) => {
                let layers = if layers.is_empty() { self.all_instances() } else { layers };
                let mut broken = 0;
                for ts in self.doc.texture_sets.iter_mut() {
                    for &id in &layers {
                        // Layers inside an instance belong to its copy: only the instance itself has a link to break.
                        if subcraft_doc::instance::root_of(ts, id) == Some(id)
                            && let Some(l) = ts.find_mut(id)
                        {
                            // The copied layers become plain too: an instance inside the source
                            // was copied as a live instance, and the next refresh found it as a
                            // new root, still locked, after "make every instance independent".
                            fn unlink(l: &mut Layer) {
                                l.instance_of = None;
                                if let Some(children) = l.children_mut() {
                                    for c in children.iter_mut() {
                                        unlink(Arc::make_mut(c));
                                    }
                                }
                            }
                            unlink(l);
                            broken += 1;
                        }
                    }
                }
                if broken == 0 {
                    return Err(CommandError::Invalid("No instance to make independent".into()));
                }
                self.commit(if broken == 1 { "Break Instance Link" } else { "Break Instance Links" }, None);
            }
            Command::InstantiateAcross { layers, texture_sets } => {
                let found = self.instance_sources(&layers);
                if found.is_empty() {
                    return Err(CommandError::LayerNotFound);
                }
                // Checked before anything is added: a texture set that is gone (the dialog stays
                // open over a mesh reload) used to stop the command after instances had already
                // been put into the sets before it, outside any undo step.
                if texture_sets.iter().any(|&t| self.doc.texture_set(t).is_none()) {
                    return Err(CommandError::TextureSetNotFound);
                }
                let mut made = false;
                let mut done = Vec::new();
                for target in texture_sets {
                    // A set listed twice gets one instance.
                    if done.contains(&target) {
                        continue;
                    }
                    done.push(target);
                    let ts = self.doc.texture_set_mut(target).ok_or(CommandError::TextureSetNotFound)?;
                    for (set, source) in &found {
                        if *set == target {
                            continue;
                        }
                        let mut instance = subcraft_doc::instance::new_instance(source, ts, false);
                        instance.name = crate::clipboard::pasted_name(ts, &source.name);
                        ts.insert_above(None, instance);
                        made = true;
                    }
                }
                if !made {
                    return Err(CommandError::Invalid("Instantiate across texture sets: tick at least one other texture set".into()));
                }
                self.commit("Instantiate Across Texture Sets", None);
            }
            Command::DeleteLayers(layers) => {
                let set = self.common_set(&layers)?;
                let ts = self.doc.texture_set_mut(set).ok_or(CommandError::TextureSetNotFound)?;
                for id in crate::clipboard::top_level_in_order(ts, &layers) {
                    ts.remove(id).map_err(|_| CommandError::LayerNotFound)?;
                }
                self.commit("Delete Layers", None);
                if self.selection.layer.is_none_or(|l| self.selected_texture_set().find(l).is_none()) {
                    self.selection.layer = self.selected_texture_set().layers.last().map(|l| l.id);
                }
            }
            Command::DuplicateLayers(layers) => {
                let set = self.common_set(&layers)?;
                let ts = self.doc.texture_set_mut(set).ok_or(CommandError::TextureSetNotFound)?;
                let copies = Self::duplicate_in_set(ts, &layers)?;
                self.commit("Duplicate Layers", None);
                if set == self.selection.texture_set {
                    self.selection.layer = copies.last().copied().or(self.selection.layer);
                }
            }
            Command::SetLayersVisible { layers, visible } => {
                let set = self.common_set(&layers)?;
                let ts = self.doc.texture_set_mut(set).ok_or(CommandError::TextureSetNotFound)?;
                let mut changed = false;
                for id in layers {
                    let l = ts.find_mut(id).ok_or(CommandError::LayerNotFound)?;
                    changed |= l.visible != visible;
                    l.visible = visible;
                    if visible { changed |= Self::show_ancestors(ts, id); }
                }
                if changed { self.commit(if visible { "Show Layers" } else { "Hide Layers" }, None); }
            }
            Command::PasteLayers(clip) => {
                let ts_id = self.selection.texture_set;
                let reference = self.selection.layer;
                let mesh = crate::clipboard::mesh_signature(&self.scene.mesh);
                let ts = self.doc.texture_set_mut(ts_id).ok_or(CommandError::TextureSetNotFound)?;
                for kind in clip.missing_channels(ts) {
                    ts.channels.push(ChannelDef::default_for(kind));
                    add_blend(&mut ts.layers, kind);
                }
                let mut at = Self::outside_instances(ts, reference);
                // Resources first: a pasted text or path layer may be drawn again with them.
                for r in &clip.resources {
                    self.resources.insert_shared(r.clone());
                }
                let layers = redraw_pasted(clip.instantiate(ts, mesh), clip.resolution, ts.resolution, &self.resources);
                for mut layer in layers {
                    layer.name = crate::clipboard::pasted_name(ts, &layer.name);
                    at = Some(ts.insert_above(at, layer));
                }
                self.selection.layer = at;
                self.commit("Paste Layers", None);
            }
            Command::PasteEffects { layer, clip } => {
                for e in &clip.content { Self::check_content_effect(&e.kind)?; }
                for e in &clip.mask { Self::check_mask_effect(&e.kind)?; }
                let (content, mask) = (!clip.content.is_empty(), !clip.mask.is_empty());
                let above_content = self.selected_effect_in(layer, LayerTarget::Content);
                let above_mask = self.selected_effect_in(layer, LayerTarget::Mask);
                self.edit_layer_set(layer, "Paste Effects", None, |ts| {
                    let res = ts.resolution;
                    let l = Self::layer_mut(ts, layer)?;
                    if content {
                        if l.is_folder() && clip.content.iter().any(|e| !e.kind.is_adjustment() && !matches!(e.kind, ContentEffectKind::Anchor)) {
                            return Err(CommandError::Invalid("Folders support adjustment effects only".into()));
                        }
                        let at = above_content.and_then(|a| l.content_effects.iter().position(|e| e.id == a)).map_or(l.content_effects.len(), |i| i + 1);
                        l.content_effects.splice(at..at, clip.instantiate_content(res).into_iter().map(Arc::new));
                    }
                    if mask {
                        let m = l.mask.get_or_insert_with(|| Mask::new(res, true));
                        let at = above_mask.and_then(|a| m.effects.iter().position(|e| e.id == a)).map_or(m.effects.len(), |i| i + 1);
                        m.effects.splice(at..at, clip.instantiate_mask(res).into_iter().map(Arc::new));
                    }
                    Ok(())
                })?;
                for r in &clip.resources {
                    self.resources.insert_shared(r.clone());
                }
            }
            Command::RenameTextureSet { texture_set, name } => {
                let name = name.trim().to_string();
                if name.is_empty() { return Err(CommandError::Invalid("A texture set needs a name".into())); }
                let ts = self.doc.texture_set_mut(texture_set).ok_or(CommandError::TextureSetNotFound)?;
                if ts.name != name {
                    ts.name = name;
                    self.commit("Rename Texture Set", None);
                }
            }
            Command::ResetTextureSetName(texture_set) => {
                if self.doc.texture_set(texture_set).is_none() { return Err(CommandError::TextureSetNotFound); }
                let name = self.original_set_name(texture_set)
                    .ok_or_else(|| CommandError::Invalid("the mesh material of this texture set has no name".into()))?;
                return self.execute(Command::RenameTextureSet { texture_set, name });
            }
            Command::SetTextureSetDescription { texture_set, description } => {
                let description = description.trim_end().to_string();
                let ts = self.doc.texture_set_mut(texture_set).ok_or(CommandError::TextureSetNotFound)?;
                if ts.description != description {
                    ts.description = description;
                    self.commit("Edit Texture Set Description", None);
                }
            }
            Command::SetTextureSetsVisibility(op) => {
                let all: std::collections::BTreeSet<TextureSetId> = self.doc.texture_sets.iter().map(|t| t.id).collect();
                let hidden = match op {
                    crate::command::SetsVisibility::ShowAll => Default::default(),
                    crate::command::SetsVisibility::HideAll => all,
                    crate::command::SetsVisibility::Invert => all.difference(&self.hidden_sets).copied().collect(),
                };
                if self.focus_set || hidden != self.hidden_sets {
                    self.hidden_sets = hidden;
                    self.focus_set = false;
                    self.revision += 1;
                    self.refresh_part_view();
                }
            }
            Command::SetTextureSetFocus(on) => {
                if self.focus_set != on {
                    self.focus_set = on;
                    self.revision += 1;
                    self.refresh_part_view();
                }
            }
            Command::SetMixing { texture_set, mixing } => {
                let ts = self.doc.texture_set_mut(texture_set).ok_or(CommandError::TextureSetNotFound)?;
                if ts.mixing != mixing {
                    ts.mixing = mixing;
                    self.commit("Mixing Settings", None);
                }
            }
            Command::SetTextureSetHidden { texture_set, hidden } => {
                if self.doc.texture_set(texture_set).is_none() { return Err(CommandError::TextureSetNotFound); }
                let changed = if hidden { self.hidden_sets.insert(texture_set) } else { self.hidden_sets.remove(&texture_set) };
                if changed { self.revision += 1; self.refresh_part_view(); }
            }
            Command::ClearMeshMap { texture_set, kind } => {
                if self.doc.texture_set(texture_set).is_none() { return Err(CommandError::TextureSetNotFound); }
                if kind == subcraft_core::MeshMapKind::Id { self.id_baked.remove(&texture_set); }
                if self.mesh_maps.get_mut(&texture_set).is_some_and(|m| m.remove(kind).is_some()) {
                    self.invalidated.insert(texture_set);
                    self.maps_dirty = true;
                    self.maps_generation += 1;
                    self.revision += 1;
                }
            }
            Command::FlattenLayers(layers) => return crate::flatten::flatten_layers(self, &layers),
            Command::GroupLayer(id) => {
                self.edit_layer_set(id, "Group Layer", None, |ts| ts.group(id, "Folder").map_err(|_| CommandError::LayerNotFound))?;
            }
            Command::IsolateTextureSet(texture_set) => {
                if self.doc.texture_set(texture_set).is_none() { return Err(CommandError::TextureSetNotFound); }
                let others: std::collections::BTreeSet<TextureSetId> = self.doc.texture_sets.iter().map(|t| t.id).filter(|&id| id != texture_set).collect();
                let hidden = if self.hidden_sets == others { Default::default() } else { others };
                if hidden != self.hidden_sets {
                    self.hidden_sets = hidden;
                    self.revision += 1;
                    self.refresh_part_view();
                }
            }
            Command::GroupLayers(ids) => {
                let first = *ids.first().ok_or(CommandError::LayerNotFound)?;
                let set = self.texture_set_of(first).ok_or(CommandError::LayerNotFound)?;
                if ids.iter().any(|&id| self.texture_set_of(id) != Some(set)) {
                    return Err(CommandError::Invalid("the layers to group are in different texture sets".into()));
                }
                // The guard below covers `first` only.
                if ids.iter().any(|&id| self.inside_instance_content(id)) {
                    return Err(CommandError::Invalid(INSTANCE_READ_ONLY.into()));
                }
                let folder = self.edit_layer_set(first, "Group Layers", None, |ts| {
                    // Bottom to top, as stored. A layer inside another selected folder moves with that folder.
                    let mut order = Vec::new();
                    ts.walk(|l, _| if ids.contains(&l.id) { order.push(l.id); });
                    let nested = |ts: &TextureSet, id: LayerId| {
                        let mut parent = ts.location(id).and_then(|l| l.parent);
                        while let Some(p) = parent {
                            if ids.contains(&p) { return true; }
                            parent = ts.location(p).and_then(|l| l.parent);
                        }
                        false
                    };
                    let order: Vec<LayerId> = order.iter().copied().filter(|&id| !nested(ts, id)).collect();
                    let (&top, below) = order.split_last().ok_or(CommandError::LayerNotFound)?;
                    let folder = ts.group(top, "Folder").map_err(|_| CommandError::LayerNotFound)?;
                    for (i, &id) in below.iter().enumerate() {
                        ts.move_to(id, Some(folder), i).map_err(|e| CommandError::Invalid(format!("{e:?}")))?;
                    }
                    Ok(folder)
                })?;
                self.selection = Selection { texture_set: set, layer: Some(folder) };
                self.brush.target = PaintTarget::Content;
                self.effect = None;
            }
            Command::MoveLayer { layer, parent, index } => {
                self.edit_layer_set(layer, "Move Layer", None, |ts| {
                    ts.move_to(layer, parent, index).map_err(|e| CommandError::Invalid(format!("{e:?}")))
                })?;
            }
            Command::DropLayer { texture_set, layer, target } => {
                // A drag belongs to the texture set where it started, even if selection changes.
                if texture_set != self.selection.texture_set { return Err(CommandError::Invalid("the dragged texture set is no longer selected".into())); }
                let ts = self.doc.texture_set(texture_set).ok_or(CommandError::TextureSetNotFound)?;
                if ts.find(layer).is_none() { return Err(CommandError::LayerNotFound); }
                if let Some(destination) = crate::layer_tree::drop_destination(ts, layer, target) {
                    if Self::inside_instance(ts, destination.parent) { return Err(CommandError::Invalid(INSTANCE_READ_ONLY.into())); }
                    self.edit_layer_set(layer, "Move Layer", None, |ts| {
                        ts.move_to(layer, destination.parent, destination.index).map_err(|e| CommandError::Invalid(format!("{e:?}")))?;
                        if let Some(parent) = destination.parent
                            && let LayerContent::Group(folder) = &mut Self::layer_mut(ts, parent)?.content
                        { folder.expanded = true; }
                        Ok(())
                    })?;
                    self.selection.layer = Some(layer);
                }
            }
            Command::DropLayers { texture_set, layers, target, copy } => {
                use crate::layer_tree::DropTarget;
                if texture_set != self.selection.texture_set { return Err(CommandError::Invalid("the dragged texture set is no longer selected".into())); }
                let ts = self.doc.texture_set_mut(texture_set).ok_or(CommandError::TextureSetNotFound)?;
                let originals = crate::clipboard::top_level_in_order(ts, &layers);
                if originals.is_empty() { return Err(CommandError::LayerNotFound); }
                let anchor = match target {
                    DropTarget::Above(id) | DropTarget::Below(id) | DropTarget::Inside(id) => Some(id),
                    DropTarget::RootTop | DropTarget::RootBottom => None,
                };
                if let Some(anchor) = anchor {
                    if ts.find(anchor).is_none() { return Err(CommandError::LayerNotFound); }
                    if matches!(target, DropTarget::Inside(_)) && !ts.find(anchor).is_some_and(|l| l.is_folder()) { return Ok(()); }
                    // Moved layers cannot land on or inside themselves; copies can land anywhere.
                    if !copy && originals.iter().any(|&l| ts.is_descendant_or_self(anchor, l)) { return Ok(()); }
                }
                let before = ts.layers.clone();
                let moved = (|| {
                    let moving = if copy { Self::duplicate_in_set(ts, &originals)? } else { originals };
                    // The first one goes to the target; each next one right above the one before,
                    // so the group keeps its stacking order.
                    let mut at = target;
                    for &id in &moving {
                        if let Some(destination) = crate::layer_tree::drop_destination(ts, id, at) {
                            if Self::inside_instance(ts, destination.parent) { return Err(CommandError::Invalid(INSTANCE_READ_ONLY.into())); }
                            ts.move_to(id, destination.parent, destination.index).map_err(|e| CommandError::Invalid(format!("{e:?}")))?;
                        }
                        at = DropTarget::Above(id);
                    }
                    if let DropTarget::Inside(folder) = target
                        && let Some(LayerContent::Group(group)) = ts.find_mut(folder).map(|l| &mut l.content)
                    { group.expanded = true; }
                    Ok(moving)
                })();
                let moving = match moved {
                    Ok(moving) => moving,
                    // Leave the document as it was when a step is refused.
                    Err(e) => { ts.layers = before; return Err(e); }
                };
                if ts.layers != before {
                    self.commit(match (copy, moving.len() == 1) {
                        (true, true) => "Duplicate Layer",
                        (true, false) => "Duplicate Layers",
                        (false, true) => "Move Layer",
                        (false, false) => "Move Layers",
                    }, None);
                    if copy || !self.selection.layer.is_some_and(|l| moving.contains(&l)) {
                        self.selection.layer = moving.last().copied();
                    }
                }
            }
            Command::SetBlendAllChannels { layer, from, mode, opacity } => {
                self.edit_layer_set_if_changed(layer, "Blend All Channels", |ts| {
                    let l = Self::layer_mut(ts, layer)?;
                    let source = *l.blend.get(&from).ok_or(CommandError::Invalid("channel not blended by this layer".into()))?;
                    let folder = l.is_folder();
                    for (kind, blend) in l.blend.iter_mut() {
                        if mode && subcraft_doc::BlendMode::available(*kind == ChannelKind::Normal, folder).any(|m| m == source.mode) { blend.mode = source.mode; }
                        if opacity { blend.opacity = source.opacity; }
                    }
                    Ok(())
                })?;
            }
            Command::PasteBlending { layers, blend } => {
                let set = self.common_set(&layers)?;
                let ts = self.doc.texture_set_mut(set).ok_or(CommandError::TextureSetNotFound)?;
                let before = ts.layers.clone();
                for id in layers {
                    let l = ts.find_mut(id).ok_or(CommandError::LayerNotFound)?;
                    let folder = l.is_folder();
                    for (kind, target) in l.blend.iter_mut() {
                        let Some(source) = blend.get(kind) else { continue };
                        if subcraft_doc::BlendMode::available(*kind == ChannelKind::Normal, folder).any(|m| m == source.mode) { target.mode = source.mode; }
                        target.opacity = source.opacity.clamp(0.0, 1.0);
                    }
                }
                if ts.layers != before { self.commit("Paste Blending Options", None); }
            }
            Command::SetLayerColor { layers, color } => {
                let set = self.common_set(&layers)?;
                let ts = self.doc.texture_set_mut(set).ok_or(CommandError::TextureSetNotFound)?;
                let mut changed = false;
                for id in layers {
                    let l = ts.find_mut(id).ok_or(CommandError::LayerNotFound)?;
                    changed |= l.color != color;
                    l.color = color;
                }
                if changed { self.commit("Layer Color", None); }
            }
            Command::PasteLayerContent { layer, clip } => {
                let [source] = clip.layers.as_slice() else {
                    return Err(CommandError::Invalid("Copy the content of one layer first".into()));
                };
                if source.is_folder() { return Err(CommandError::Invalid("A folder has no content to paste".into())); }
                let mesh = crate::clipboard::mesh_signature(&self.scene.mesh);
                let ts = self.texture_set_of(layer).and_then(|t| self.doc.texture_set(t)).ok_or(CommandError::LayerNotFound)?;
                if ts.find(layer).ok_or(CommandError::LayerNotFound)?.is_folder() {
                    return Err(CommandError::Invalid("Layer content cannot be pasted into a folder".into()));
                }
                // Fresh effect ids and rasters at this set's resolution, like a pasted layer.
                let (pasted, resolution) = (clip.instantiate(ts, mesh), ts.resolution);
                for r in &clip.resources {
                    self.resources.insert_shared(r.clone());
                }
                let pasted = redraw_pasted(pasted, clip.resolution, resolution, &self.resources).pop().ok_or(CommandError::LayerNotFound)?;
                self.edit_layer_set(layer, "Paste Layer Content", None, |ts| {
                    let l = Self::layer_mut(ts, layer)?;
                    l.content = pasted.content;
                    l.content_effects = pasted.content_effects;
                    Ok(())
                })?;
            }
            Command::MoveUp(id) | Command::MoveDown(id) => {
                let up = matches!(cmd, Command::MoveUp(_));
                self.edit_layer_set(id, "Move Layer", None, |ts| {
                    let loc = ts.location(id).ok_or(CommandError::LayerNotFound)?;
                    let siblings = match loc.parent {
                        None => ts.layers.len(),
                        Some(p) => ts.find(p).map_or(0, |p| p.children().len()),
                    };
                    let target = if up { loc.index + 1 } else { loc.index.checked_sub(1).ok_or(CommandError::Invalid("already at bottom".into()))? };
                    if target >= siblings {
                        return Err(CommandError::Invalid("already at top".into()));
                    }
                    ts.move_to(id, loc.parent, target).map_err(|e| CommandError::Invalid(format!("{e:?}")))
                })?;
            }
            Command::SetVisible { layer, visible } => {
                self.edit_layer_set(layer, if visible { "Show Layer" } else { "Hide Layer" }, None, |ts| {
                    Self::layer_mut(ts, layer)?.visible = visible;
                    if visible { Self::show_ancestors(ts, layer); }
                    Ok(())
                })?;
            }
            Command::SetFolderExpanded { layer, expanded } => {
                self.edit_layer_set(layer, if expanded { "Expand Folder" } else { "Collapse Folder" }, None, |ts| {
                    let LayerContent::Group(folder) = &mut Self::layer_mut(ts, layer)?.content else { return Err(CommandError::Invalid("only folders can be expanded".into())); };
                    folder.expanded = expanded;
                    Ok(())
                })?;
            }
            Command::Rename { layer, name } => {
                self.edit_layer_set(layer, "Rename Layer", None, |ts| {
                    Self::layer_mut(ts, layer)?.name = name;
                    Ok(())
                })?;
            }
            Command::SetBlendMode { layer, channel, mode } => {
                self.edit_layer_set(layer, "Blend Mode", None, |ts| {
                    Self::layer_mut(ts, layer)?.blend.entry(channel).or_insert(subcraft_doc::ChannelBlend::new(mode)).mode = mode;
                    Ok(())
                })?;
            }
            Command::SetBlendOpacity { layer, channel, opacity } => {
                let key = format!("opacity:{layer}:{channel:?}");
                self.edit_layer_set(layer, "Opacity", Some(key), |ts| {
                    let l = Self::layer_mut(ts, layer)?;
                    let b = l.blend.get_mut(&channel).ok_or(CommandError::Invalid("channel not blended by this layer".into()))?;
                    // `clamp` keeps NaN, and a NaN in the document is saved as `null`: a file that never opens again.
                    if !opacity.is_finite() { return Err(CommandError::Invalid("Opacity must be a number".into())); }
                    b.opacity = opacity.clamp(0.0, 1.0);
                    Ok(())
                })?;
            }
            Command::SetFillChannel { layer, channel, fill } => {
                let key = format!("fill:{layer}:{channel:?}");
                self.edit_layer_set(layer, "Fill", Some(key), |ts| {
                    let LayerContent::Fill(f) = &mut Self::layer_mut(ts, layer)?.content else { return Err(CommandError::NotAFillLayer) };
                    if !fill.strength.is_finite() { return Err(CommandError::Invalid("Strength must be a number".into())); }
                    f.channels.insert(channel, fill);
                    Ok(())
                })?;
            }
            Command::ToggleBrushChannel { channel, solo } => {
                let channels = &mut self.brush.material.channels;
                let on = solo || !channels.get(&channel).is_some_and(|c| c.enabled);
                if solo { for c in channels.values_mut() { c.enabled = false; } }
                channels.entry(channel).or_insert(subcraft_paint::BrushChannel { enabled: false, value: channel.default_paint_value() }).enabled = on;
            }
            Command::ToggleFillChannel { layer, effect, channel, solo } => {
                self.edit_layer_set(layer, if solo { "Solo Channel" } else { "Toggle Channel" }, None, |ts| {
                    let l = Self::layer_mut(ts, layer)?;
                    let fill = if let Some(effect) = effect {
                        let e = l.content_effects.iter_mut().find(|e| e.id == effect).ok_or_else(|| CommandError::Invalid("effect not found".into()))?;
                        let subcraft_doc::ContentEffectKind::Fill(f) = &mut Arc::make_mut(e).kind else { return Err(CommandError::NotAFillLayer) }; f
                    } else { let LayerContent::Fill(f) = &mut l.content else { return Err(CommandError::NotAFillLayer) }; f };
                    let on = solo || !fill.channels.get(&channel).is_some_and(|c| c.enabled);
                    if solo { for c in fill.channels.values_mut() { c.enabled = false; } }
                    fill.channels.entry(channel).or_insert(subcraft_doc::FillChannel { enabled: false, ..subcraft_doc::FillChannel::default_for(channel) }).enabled = on;
                    // A layer made before the set had this channel has no blend for it, which means "off".
                    if on { l.blend.entry(channel).or_insert(subcraft_doc::ChannelBlend::new(subcraft_doc::BlendMode::Normal)); }
                    Ok(())
                })?;
            }
            Command::AddFillChannel { layer, channel } => {
                if !matches!(self.selected_texture_set().find(layer).map(|l| &l.content), Some(LayerContent::Fill(_))) {
                    return Err(CommandError::NotAFillLayer);
                }
                self.edit_layer_set(layer, "Add Channel", None, |ts| {
                    if !ts.has_channel(channel) {
                        ts.channels.push(ChannelDef::default_for(channel));
                        add_blend(&mut ts.layers, channel);
                    }
                    let l = Self::layer_mut(ts, layer)?;
                    let LayerContent::Fill(f) = &mut l.content else { return Err(CommandError::NotAFillLayer) };
                    f.channels.insert(channel, subcraft_doc::FillChannel::default_for(channel));
                    l.blend.entry(channel).or_insert(subcraft_doc::ChannelBlend::new(subcraft_doc::BlendMode::Normal));
                    Ok(())
                })?;
            }
            Command::SetFillProjection { layer, projection } => {
                if !projection.valid() { return Err(CommandError::Invalid("Projection values are invalid".into())); }
                let key = format!("projection:{layer}");
                self.edit_layer_set(layer, "Projection", Some(key), |ts| {
                    let LayerContent::Fill(f) = &mut Self::layer_mut(ts, layer)?.content else { return Err(CommandError::NotAFillLayer) };
                    f.projection = projection;
                    Ok(())
                })?;
            }
            Command::AddMask { layer, white } => {
                self.edit_layer_set(layer, "Add Mask", None, |ts| {
                    let res = ts.resolution;
                    Self::layer_mut(ts, layer)?.mask = Some(Mask::new(res, white));
                    Ok(())
                })?;
            }
            Command::AddHeightCombinationMask(layer) => {
                self.edit_layer_set(layer, "Add Mask with Height Combination", None, |ts| {
                    let res = ts.resolution;
                    let l = Self::layer_mut(ts, layer)?;
                    let mut mask = Mask::new(res, false);
                    mask.effects.push(Arc::new(MaskEffect::new(subcraft_doc::MaskEffectKind::Compare(subcraft_doc::CompareMask::height_combination()))));
                    l.mask = Some(mask);
                    let opacity = l.blend.get(&ChannelKind::Height).map_or(1.0, |b| b.opacity);
                    l.blend.insert(ChannelKind::Height, subcraft_doc::ChannelBlend { mode: subcraft_doc::BlendMode::Normal, opacity });
                    Ok(())
                })?;
            }
            Command::RemoveMask(layer) => {
                self.edit_layer_set(layer, "Remove Mask", None, |ts| {
                    Self::layer_mut(ts, layer)?.mask.take().map(|_| ()).ok_or(CommandError::NoMask)
                })?;
            }
            Command::CopyMask(layer) => {
                let layer = self.doc.texture_sets.iter().find_map(|ts| ts.find(layer)).ok_or(CommandError::LayerNotFound)?;
                self.mask_clipboard = Some(layer.mask.clone().ok_or(CommandError::NoMask)?);
            }
            Command::PasteMask(layer) => {
                let mut mask = self.mask_clipboard.clone().ok_or_else(|| CommandError::Invalid("Copy a mask first".into()))?;
                self.edit_layer_set(layer, "Paste Mask", None, |ts| {
                    mask.base = mask.base.resampled(ts.resolution, ts.resolution);
                    let mut ids = std::collections::HashMap::new();
                    mask.effects = mask.effects.iter().map(|effect| {
                        let mut effect = (**effect).clone();
                        let old = effect.id;
                        effect.id = subcraft_core::EffectId::new();
                        ids.insert(old, effect.id);
                        if let subcraft_doc::MaskEffectKind::Paint { image } = &mut effect.kind {
                            *image = image.resampled(ts.resolution, ts.resolution);
                        }
                        Arc::new(effect)
                    }).collect();
                    subcraft_doc::anchor::remap_mask_inputs(&mut mask.effects, &ids);
                    Self::layer_mut(ts, layer)?.mask = Some(mask);
                    Ok(())
                })?;
            }
            Command::InvertMask(layer) => {
                self.edit_layer_set(layer, "Invert Mask", None, |ts| {
                    let mask = Self::layer_mut(ts, layer)?.mask.as_mut().ok_or(CommandError::NoMask)?;
                    mask.effects.push(Arc::new(MaskEffect::new(subcraft_doc::MaskEffectKind::Invert)));
                    Ok(())
                })?;
            }
            Command::SetParts { layer, parts } => {
                self.edit_layer_set_if_changed(layer, "Layer Parts", |ts| {
                    Self::layer_mut(ts, layer)?.parts = parts;
                    Ok(())
                })?;
            }
            Command::InvertMaskBase(layer) => {
                self.edit_layer_set(layer, "Invert Mask Background", None, |ts| {
                    let mask = Self::layer_mut(ts, layer)?.mask.as_mut().ok_or(CommandError::NoMask)?;
                    let base = &mask.base;
                    let fill = base.fill();
                    let mut out = subcraft_image::TiledImage::new(base.width(), base.height(), base.comps(), [1.0 - fill[0], 0.0, 0.0, 0.0]);
                    let mut buf = vec![0.0f32; subcraft_image::TILE_TEXELS * base.comps()];
                    // Only painted tiles carry data; untouched ones are the (inverted) fill.
                    for t in 0..base.tile_count() {
                        if base.tile(t).is_none() { continue; }
                        base.read_tile(t, &mut buf);
                        for v in buf.iter_mut().step_by(base.comps()) {
                            *v = 1.0 - *v;
                        }
                        out.write_tile(t, &buf);
                    }
                    mask.base = out;
                    Ok(())
                })?;
            }
            Command::ClearMask(layer) => {
                // Clearing a mask with nothing painted adds no undo step (it wiped redo).
                self.edit_layer_set_if_changed(layer, "Clear Mask", |ts| {
                    let mask = Self::layer_mut(ts, layer)?.mask.as_mut().ok_or(CommandError::NoMask)?;
                    let (w, h, comps, fill) = (mask.base.width(), mask.base.height(), mask.base.comps(), mask.base.fill());
                    mask.base = subcraft_image::TiledImage::new(w, h, comps, fill);
                    Ok(())
                })?;
            }
            Command::SetMaskEnabled { layer, enabled } => {
                self.edit_layer_set(layer, "Mask Enabled", None, |ts| {
                    Self::layer_mut(ts, layer)?.mask.as_mut().ok_or(CommandError::NoMask)?.enabled = enabled;
                    Ok(())
                })?;
            }
            Command::AddMaskEffect { layer, kind } => {
                Self::check_mask_effect(&kind)?;
                let above = self.selected_effect_in(layer, LayerTarget::Mask);
                let id = self.edit_layer_set(layer, "Add Mask Effect", None, |ts| {
                    let name = matches!(kind, subcraft_doc::MaskEffectKind::Anchor).then(|| Self::new_anchor_name(ts, layer));
                    let mask = Self::layer_mut(ts, layer)?.mask.as_mut().ok_or(CommandError::NoMask)?;
                    let mut effect = MaskEffect::new(kind);
                    if let Some(name) = name {
                        effect.name = name;
                    }
                    let id = effect.id;
                    let at = above.and_then(|a| mask.effects.iter().position(|e| e.id == a)).map_or(mask.effects.len(), |i| i + 1);
                    mask.effects.insert(at, Arc::new(effect));
                    Ok(id)
                })?;
                return self.execute(Command::SelectEffect { layer, target: LayerTarget::Mask, effect: Some(id) });
            }
            Command::RemoveMaskEffect { layer, effect } => {
                self.edit_layer_set(layer, "Remove Mask Effect", None, |ts| {
                    let mask = Self::layer_mut(ts, layer)?.mask.as_mut().ok_or(CommandError::NoMask)?;
                    let before = mask.effects.len();
                    mask.effects.retain(|e| e.id != effect);
                    if mask.effects.len() == before { Err(CommandError::Invalid("no such effect".into())) } else { Ok(()) }
                })?;
            }
            Command::SetMaskEffect { layer, effect } => {
                Self::check_mask_effect(&effect.kind)?;
                let key = format!("effect:{layer}:{}", effect.id);
                self.edit_layer_set(layer, "Mask Effect", Some(key), |ts| {
                    let mask = Self::layer_mut(ts, layer)?.mask.as_mut().ok_or(CommandError::NoMask)?;
                    let slot = mask.effects.iter_mut().find(|e| e.id == effect.id).ok_or(CommandError::Invalid("no such effect".into()))?;
                    *slot = Arc::new(effect);
                    Ok(())
                })?;
            }
            Command::EditIdColor { texture_set, from, to } => {
                if from.iter().chain(&to).any(|v| !v.is_finite()) { return Err(CommandError::Invalid("invalid ID color".into())); }
                let baked_has_from = self.id_baked.get(&texture_set).is_some_and(|(baked, _)| crate::id_color::image_has_color(baked, from));
                let ts = self.doc.texture_set_mut(texture_set).ok_or(CommandError::TextureSetNotFound)?;
                let before = ts.clone();
                crate::id_color::edit_id_color(ts, from, to, baked_has_from);
                // An edit that changes nothing adds no undo step (it used to, and wiped redo).
                if *ts != before {
                    self.commit("Edit ID Color", None);
                    self.sync_id_maps();
                }
            }
            Command::SetIdColors { layer, effect, colors } => {
                self.edit_layer_set(layer, "ID Color", None, |ts| {
                    let mask = Self::layer_mut(ts, layer)?.mask.as_mut().ok_or(CommandError::NoMask)?;
                    let slot = mask.effects.iter_mut().find(|e| e.id == effect).ok_or(CommandError::Invalid("no such effect".into()))?;
                    let subcraft_doc::MaskEffectKind::IdColor(id) = &mut Arc::make_mut(slot).kind else {
                        return Err(CommandError::Invalid("not an ID Color Mask".into()));
                    };
                    let candidate = subcraft_doc::IdColorMask { colors, ..id.clone() };
                    if !candidate.valid() { return Err(CommandError::Invalid("invalid ID colors".into())); }
                    *id = candidate;
                    Ok(())
                })?;
            }
            Command::MoveMaskEffect { layer, effect, index } => {
                self.edit_layer_set(layer, "Move Mask Effect", None, |ts| {
                    let mask = Self::layer_mut(ts, layer)?.mask.as_mut().ok_or(CommandError::NoMask)?;
                    if index >= mask.effects.len() { return Err(CommandError::Invalid("Effect destination is outside the stack".into())); }
                    let from = mask.effects.iter().position(|e| e.id == effect).ok_or_else(|| CommandError::Invalid("No such effect".into()))?;
                    let moved = mask.effects.remove(from);
                    mask.effects.insert(index, moved);
                    Ok(())
                })?;
            }
            Command::DuplicateMaskEffect { layer, effect } => {
                self.edit_layer_set(layer, "Duplicate Mask Effect", None, |ts| {
                    let mask = Self::layer_mut(ts, layer)?.mask.as_mut().ok_or(CommandError::NoMask)?;
                    let index = mask.effects.iter().position(|e| e.id == effect).ok_or_else(|| CommandError::Invalid("No such effect".into()))?;
                    let mut copy = (*mask.effects[index]).clone();
                    copy.id = subcraft_core::EffectId::new();
                    copy.name = format!("{} copy", copy.name);
                    mask.effects.insert(index + 1, Arc::new(copy));
                    Ok(())
                })?;
            }
        }
        self.normalize_target();
        Ok(())
    }

    /// Duplicate `layers` (those not inside another of them), each copy right above its
    /// original. Returns the copies, bottom to top.
    fn duplicate_in_set(ts: &mut TextureSet, layers: &[LayerId]) -> Result<Vec<LayerId>, CommandError> {
        let mut copies = Vec::new();
        for id in crate::clipboard::top_level_in_order(ts, layers) {
            let copy = ts.duplicate(id).map_err(|_| CommandError::LayerNotFound)?;
            copies.push((id, copy));
        }
        // Copies read the copied anchors when both were duplicated together (ADR-0045).
        let mut map = HashMap::new();
        for (original, copy) in &copies {
            if let (Some(a), Some(b)) = (ts.find(*original), ts.find(*copy)) {
                subcraft_doc::anchor::copied_effect_ids(a, b, &mut map);
            }
        }
        if subcraft_doc::anchor::has_references(&ts.layers) {
            for (_, copy) in &copies {
                if let Some(l) = ts.find_mut(*copy) {
                    subcraft_doc::anchor::remap_inputs(l, &map);
                }
            }
        }
        Ok(copies.into_iter().map(|(_, copy)| copy).collect())
    }

    /// Make every folder around `layer` visible (Painter: un-hiding a layer inside a hidden
    /// folder shows the folder too). Returns whether anything changed.
    fn show_ancestors(ts: &mut TextureSet, layer: LayerId) -> bool {
        let mut changed = false;
        let mut at = ts.location(layer).and_then(|l| l.parent);
        while let Some(id) = at {
            if let Some(folder) = ts.find_mut(id) {
                changed |= !folder.visible;
                folder.visible = true;
            }
            at = ts.location(id).and_then(|l| l.parent);
        }
        changed
    }

    /// Default name of a new anchor point in `layer`: "<layer> anchor", numbered when taken, so
    /// pickers can tell anchors apart (ADR-0045).
    fn new_anchor_name(ts: &TextureSet, layer: LayerId) -> String {
        let base = format!("{} anchor", ts.find(layer).map_or("Layer", |l| l.name.as_str()));
        let taken: std::collections::HashSet<String> = subcraft_doc::AnchorIndex::build(&ts.layers).anchors.into_iter().map(|a| a.name).collect();
        (1..).map(|n| if n == 1 { base.clone() } else { format!("{base} {n}") }).find(|n| !taken.contains(n)).expect("unbounded")
    }

    fn check_content_effect(kind: &ContentEffectKind) -> Result<(), CommandError> {
        if let ContentEffectKind::Filter { channel, filter } = kind
            && (!filter.valid() || !filter.supports_channel(*channel)) {
            return Err(CommandError::Invalid("Filter parameters or target channel are invalid".into()));
        }
        if matches!(kind, ContentEffectKind::Levels { channel: subcraft_core::ChannelKind::Normal, .. }) {
            return Err(CommandError::Invalid("Levels cannot adjust the Normal channel".into()));
        }
        if let ContentEffectKind::Levels { levels, .. } = kind && !levels.valid() {
            return Err(CommandError::Invalid("Levels parameters are invalid".into()));
        }
        // The project file refuses these: accepted here, the project saved and never opened again.
        if let ContentEffectKind::Fill(f) = kind && !f.projection.valid() {
            return Err(CommandError::Invalid("Fill projection parameters are invalid".into()));
        }
        Ok(())
    }

    fn check_mask_effect(kind: &subcraft_doc::MaskEffectKind) -> Result<(), CommandError> {
        if let subcraft_doc::MaskEffectKind::Filter(filter) = kind
            && (!filter.valid() || !filter.supports_mask()) {
            return Err(CommandError::Invalid("Filter parameters or mask target are invalid".into()));
        }
        if let subcraft_doc::MaskEffectKind::IdColor(id) = kind && !id.valid() {
            return Err(CommandError::Invalid("ID Color Mask parameters are invalid".into()));
        }
        if let subcraft_doc::MaskEffectKind::Compare(c) = kind && !c.valid() {
            return Err(CommandError::Invalid("Compare Mask parameters are invalid".into()));
        }
        if let subcraft_doc::MaskEffectKind::Levels(l) | subcraft_doc::MaskEffectKind::AnchorRef { levels: Some(l), .. } = kind && !l.valid() {
            return Err(CommandError::Invalid("Levels parameters are invalid".into()));
        }
        // The project file refuses these: accepted here, the project saved and never opened again.
        if let subcraft_doc::MaskEffectKind::Fill { projection, .. } | subcraft_doc::MaskEffectKind::Image { projection, .. } = kind && !projection.valid() {
            return Err(CommandError::Invalid("Fill projection parameters are invalid".into()));
        }
        Ok(())
    }

    fn neighbour_after_removal(&self, loc: Option<LayerLocation>) -> Option<LayerId> {
        let ts = self.selected_texture_set();
        let loc = loc?;
        let siblings: &[Arc<Layer>] = match loc.parent {
            None => &ts.layers,
            Some(p) => ts.find(p).map_or(&[], |l| l.children()),
        };
        siblings.get(loc.index.saturating_sub(1)).or(siblings.last()).map(|l| l.id).or(loc.parent)
    }

    /// Toolkit-independent actions. Returns false for actions the front end must handle itself
    /// (e.g. camera actions).
    pub fn perform(&mut self, action: Action) -> Result<bool, CommandError> {
        let sel = self.selection.layer;
        match action {
            Action::Undo => self.execute(Command::Undo)?,
            Action::Redo => self.execute(Command::Redo)?,
            Action::NewPaintLayer => self.execute(Command::AddLayer(NewLayer::Paint))?,
            Action::NewFillLayer => self.execute(Command::AddLayer(NewLayer::Fill))?,
            // Ctrl+G is Painter's "Group layers": wrap the selection; with nothing selected, an empty folder.
            Action::NewFolder => match sel {
                Some(l) => self.execute(Command::GroupLayers(vec![l]))?,
                None => self.execute(Command::AddLayer(NewLayer::Folder))?,
            },
            Action::NewTextLayer => { let d = self.new_text_decal("Text"); self.add_text_layer(d)? }
            // With an effect selected, Delete and Ctrl+D act on the effect, not its layer (Painter).
            Action::DeleteLayer => match (sel, self.selected_effect()) {
                (Some(l), Some(e)) => self.execute(Command::RemoveEffects { layer: l, target: self.layer_target(), effects: vec![e] })?,
                (Some(l), None) => self.execute(Command::DeleteLayer(l))?,
                _ => {}
            },
            Action::DuplicateLayer => match (sel, self.selected_effect()) {
                (Some(l), Some(e)) => self.execute(Command::DuplicateEffects { layer: l, target: self.layer_target(), effects: vec![e] })?,
                (Some(l), None) => self.execute(Command::DuplicateLayer(l))?,
                _ => {}
            },
            Action::BrushTool => self.brush.mode = StrokeMode::Paint,
            Action::EraserTool => self.brush.mode = StrokeMode::Erase,
            Action::BrushSmaller => self.brush.settings.size = (self.brush.settings.size / 1.15).max(0.1),
            Action::BrushLarger => self.brush.settings.size = (self.brush.settings.size * 1.15).min(100.0),
            Action::ToggleSymmetryX => self.brush.options.symmetry[0] = !self.brush.options.symmetry[0],
            Action::ToggleSymmetry => self.brush.options.toggle_symmetry(&mut self.saved_symmetry),
            Action::PauseEngine => self.engine_paused = !self.engine_paused,
            Action::ToggleLazyMouse => {
                let o = &mut self.brush.options;
                o.lazy_distance_px = if o.lazy_distance_px > 0.0 { 0.0 } else { 20.0 };
            }
            Action::InvertMaskGray => self.brush.material.mask_value = 1.0 - self.brush.material.mask_value.clamp(0.0, 1.0),
            Action::IsolateTextureSet => self.execute(Command::IsolateTextureSet(self.selection.texture_set))?,
            Action::HideExcludedGeometry => self.toggle_hide_excluded(),
            Action::QuickMaskToggle => self.toggle_quick_mask(),
            Action::QuickMaskClear => self.clear_quick_mask(),
            Action::QuickMaskInvert => return Ok(self.invert_quick_mask()),
            Action::HideUi | Action::PaintingMode | Action::CloseProject | Action::View3dAnd2d | Action::View3dOnly | Action::View2dOnly
            | Action::SwapViews | Action::NextMeshMap | Action::PrevMeshMap | Action::NextCamera | Action::PrevCamera => return Ok(false),
            Action::PenTool | Action::PolygonFillTool | Action::SmudgeTool | Action::CloneTool | Action::EyedropperTool | Action::ProjectionTool | Action::ResetCamera
            | Action::PerspectiveCamera | Action::OrthographicCamera | Action::NextChannel | Action::PrevChannel | Action::DisplayMaterial
            | Action::NewProject | Action::Quit | Action::Save | Action::SaveAs | Action::OpenProject | Action::Export | Action::Bake
            | Action::TogglePathTracing | Action::SelectAllLayers | Action::ReimportMesh => return Ok(false),
        }
        Ok(true)
    }

    // ----- painting ------------------------------------------------------------------------------

    /// Before a paint stroke on layer content: if the selection cannot take paint (a fill layer,
    /// a folder, or nothing selected), add a paint layer above it and select it, so the brush
    /// always draws (a fill layer's material stays as it is). Mask strokes, erasing and layers
    /// that already take paint are left alone. Returns the new layer's name when one was added.
    pub fn ensure_paint_layer(&mut self) -> Option<String> {
        if self.brush.target != PaintTarget::Content || self.brush.mode == StrokeMode::Erase || self.quick_mask_editing() {
            return None;
        }
        // A clone stroke without a source fails to start; it must not leave a new empty layer and
        // an undo step behind.
        if self.brush.mode == StrokeMode::Clone && self.clone_source.is_none() {
            return None;
        }
        let paintable = self.selected_layer().is_some_and(|l| matches!(&l.content, subcraft_doc::LayerContent::Paint(p) if p.path.is_none() && p.text.is_none()));
        if paintable { return None; }
        self.execute(Command::AddLayer(NewLayer::Paint)).ok()?;
        self.selected_layer().map(|l| l.name.clone())
    }

    /// Start a stroke on the selected layer with the current brush.
    pub fn begin_stroke(&mut self) -> Result<(), PaintError> {
        self.begin_stroke_with(None)
    }

    /// Start a 2D-view stroke (continued with [`Session::stroke_to_uv`]). `view` is where the
    /// texture is drawn in the 2D view: a stencil masks the stroke through it. Clone strokes copy
    /// from [`Session::clone_source_uv`].
    pub fn begin_uv_stroke(&mut self, view: Option<subcraft_paint::UvView>) -> Result<(), PaintError> {
        self.begin_stroke_with(Some(view))
    }

    /// Painter's "continue the previous stroke" (A held when a stroke starts): the next stroke,
    /// when nothing changed since the last one (same layer, brush and settings, no other edit
    /// or undo), carries on that stroke. Its coverage builds on the old one's, so stroke opacity
    /// still caps both together instead of each stroke on its own. It is still its own undo step.
    pub fn set_continue_stroke(&mut self, on: bool) {
        self.continue_stroke = on;
    }

    /// Whether a stroke starting now in the 3D view (`uv` false) or the 2D view would continue
    /// the last one.
    pub fn can_continue_stroke(&self, uv: bool) -> bool {
        self.last_stroke.as_ref().is_some_and(|last| self.continuable(last, uv))
    }

    fn continuable(&self, last: &LastStroke, uv: bool) -> bool {
        last.active.key.is_some()
            && last.active.key == self.continue_key(uv)
            && self.history.cursor() == last.cursor
            && self.history.entries().len() == last.entries
            && self.doc == *self.history.current()
    }

    /// The key of a stroke starting now (`None` when such a stroke cannot be continued later).
    fn continue_key(&self, uv: bool) -> Option<ContinueKey> {
        let b = &self.brush;
        if !matches!(b.mode, StrokeMode::Paint | StrokeMode::Erase) || self.stroke_stamp().is_some() || self.quick_mask_editing() {
            return None;
        }
        Some(ContinueKey {
            texture_set: self.selection.texture_set,
            layer: self.selection.layer?,
            mode: b.mode,
            target: b.target,
            settings: b.settings,
            options: b.options,
            uv,
        })
    }

    fn begin_stroke_with(&mut self, uv: Option<Option<subcraft_paint::UvView>>) -> Result<(), PaintError> {
        if self.stroke.is_some() {
            return Ok(());
        }
        if std::mem::take(&mut self.continue_stroke)
            && let Some(last) = self.last_stroke.take()
            && self.continuable(&last, uv.is_some())
        {
            let mut active = last.active;
            active.stroke.lift_pen();
            active.camera = None;
            self.stroke = Some(active);
            return Ok(());
        }
        if self.quick_mask_editing() {
            return self.begin_quick_mask_stroke(uv.flatten());
        }
        let layer = self.selection.layer.ok_or(PaintError::LayerNotFound)?;
        if self.instance_locked(layer) {
            return Err(PaintError::Instance);
        }
        // Before anything below changes the document: a refused stroke leaves no trace.
        if self.brush.mode == StrokeMode::Clone {
            let missing = if uv.is_some() { self.clone_source_uv().is_none() } else { self.clone_source.is_none() };
            if missing {
                return Err(PaintError::NoCloneSource);
            }
        }
        // A normal-map / height stamp turns on its channel on the layer with the first stroke.
        let stamp = self.stroke_stamp();
        if let Some((s, _)) = &stamp {
            self.enable_layer_channel(layer, s.kind.channel());
        }
        // Absolute clone source: the first dab of every stroke copies from the source point again.
        if self.brush.mode == StrokeMode::Clone && self.brush.clone_mode == CloneSourceMode::Absolute && self.clone_source.is_some() {
            self.clone_offset = None;
        }
        if self.brush.mode == StrokeMode::Clone && self.brush.clone_mode == CloneSourceMode::Absolute {
            self.clone_offset_uv = None;
        }
        let uv_clone = self.clone_source_uv();
        let ts = self.selected_texture_set();
        let b = &self.brush;
        let mut material = b.material.clone();
        if b.projection {
            material.camera_projection = true;
            material.texture_tiling = true;
        }
        if let Some((s, _)) = &stamp {
            let channel = s.kind.channel();
            material.channels.entry(channel).or_insert(subcraft_paint::BrushChannel { enabled: true, value: channel.default_paint_value() }).enabled = true;
        }
        let mut stroke = Stroke::begin(ts, layer, b.target, b.mode, b.settings, b.options, &material)?
            .with_resources(self.resources())
            .with_alpha(if stamp.is_some() { None } else { b.alpha.clone() })
            .with_stamp(stamp.as_ref().map(|(s, _)| s.clone()), stamp.as_ref().map(|(_, i)| i.clone()))
            .with_stencil(if b.stencil_bypass { None } else { b.stencil.clone() })
            .with_quick_mask(self.applied_quick_mask().cloned());
        if let Some(view) = uv {
            stroke = stroke.with_uv_view(view);
            if b.mode == StrokeMode::Clone {
                stroke = stroke.with_uv_clone(uv_clone.ok_or(PaintError::NoCloneSource)?, self.clone_offset_uv);
            }
        } else if b.mode == StrokeMode::Clone {
            let point = self.clone_source.ok_or(PaintError::NoCloneSource)?;
            stroke = stroke.with_clone(point, self.clone_offset);
        }
        let b = &self.brush;
        let label = match (b.mode, b.target) {
            (StrokeMode::Smudge, _) => "Smudge Stroke",
            (StrokeMode::Clone, _) => "Clone Stroke",
            (StrokeMode::Paint, PaintTarget::Content) if stamp.as_ref().is_some_and(|(s, _)| s.kind == subcraft_paint::StampKind::Normal) => "Normal Map Stamp Stroke",
            (StrokeMode::Paint, PaintTarget::Content) if stamp.is_some() => "Height Stamp Stroke",
            (StrokeMode::Paint, PaintTarget::Content) => "Paint Stroke",
            (StrokeMode::Erase, PaintTarget::Content) => "Erase Stroke",
            (StrokeMode::Paint, PaintTarget::ContentEffect(_)) => "Paint Effect Stroke",
            (StrokeMode::Erase, PaintTarget::ContentEffect(_)) => "Erase Effect Stroke",
            (_, PaintTarget::MaskBase | PaintTarget::MaskEffect(_)) => "Mask Stroke",
        };
        let key = self.continue_key(uv.is_some());
        let ts_id = self.selected_texture_set().id;
        self.stroke = Some(ActiveStroke { stroke, texture_set: ts_id, label, camera: None, quick: false, key });
        Ok(())
    }

    /// A stroke (or polygon fill) on the quick mask being edited: brush, polygon fill, smudge and
    /// clone add white, the eraser black, with the brush's tip, stencil and symmetry.
    fn begin_quick_mask_stroke(&mut self, view: Option<subcraft_paint::UvView>) -> Result<(), PaintError> {
        let qm = self.quick_mask.as_ref().ok_or(PaintError::LayerNotFound)?;
        let mut scratch = self.selected_texture_set().clone();
        scratch.layers.clear();
        let mut layer = Layer::new_paint("Quick mask", &scratch);
        let mut mask = Mask::new(scratch.resolution, false);
        mask.base = qm.image.clone();
        layer.mask = Some(mask);
        let id = layer.id;
        scratch.layers.push(Arc::new(layer));
        let b = &self.brush;
        let mut material = b.material.clone();
        material.mask_value = if b.mode == StrokeMode::Erase { 0.0 } else { 1.0 };
        let stroke = Stroke::begin(&scratch, id, PaintTarget::MaskBase, StrokeMode::Paint, b.settings, b.options, &material)?
            .with_alpha(b.alpha.clone())
            .with_stencil(if b.stencil_bypass { None } else { b.stencil.clone() })
            .with_uv_view(view);
        self.quick_scratch = Some(scratch);
        self.stroke = Some(ActiveStroke { stroke, texture_set: self.selection.texture_set, label: "Quick Mask Stroke", camera: None, quick: true, key: None });
        Ok(())
    }

    /// Keep the quick mask at its texture set's resolution (resampled), and drop it with its set.
    /// QA 2026-11: after a resolution change (or an undo across one) an applied quick mask of the
    /// old size was silently ignored by strokes, so protected areas were painted, and editing it
    /// failed every stroke with a resolution mismatch.
    fn fit_quick_mask(&mut self) {
        let Some(set) = self.quick_mask.as_ref().map(|q| q.texture_set) else { return };
        match self.doc.texture_set(set).map(|ts| ts.resolution) {
            None => {
                self.quick_mask = None;
                self.revision += 1;
            }
            Some(res) => {
                if let Some(q) = self.quick_mask.as_mut()
                    && (q.image.width() != res || q.image.height() != res)
                {
                    q.image = q.image.resampled(res, res);
                    q.revision += 1;
                    self.revision += 1;
                }
            }
        }
    }

    /// The quick mask, when one exists for the selected texture set.
    pub fn quick_mask(&self) -> Option<&QuickMask> {
        self.quick_mask.as_ref().filter(|q| q.texture_set == self.selection.texture_set)
    }

    /// Whether strokes paint the quick mask instead of the layer.
    pub fn quick_mask_editing(&self) -> bool {
        self.quick_mask().is_some_and(|q| q.editing)
    }

    /// The quick mask restricting strokes right now (one exists, and it is not being edited).
    pub fn applied_quick_mask(&self) -> Option<&subcraft_image::TiledImage> {
        self.quick_mask().filter(|q| !q.editing).map(|q| &q.image)
    }

    /// Painter's U: start editing a quick mask (a new one selects nothing), or leave the edit and
    /// apply it. Leaving with nothing painted removes the quick mask (no restriction).
    pub fn toggle_quick_mask(&mut self) {
        if self.stroke.is_some() {
            return;
        }
        let ts = self.selected_texture_set();
        let (id, res) = (ts.id, ts.resolution);
        match self.quick_mask.as_mut().filter(|q| q.texture_set == id) {
            Some(q) if q.editing => {
                q.editing = false;
                q.revision += 1;
                // Erased texels keep their tile: a mask painted and erased again, or only erased,
                // has tiles and selects nothing. Counted as painted, it was applied and every
                // stroke after it painted nothing, with no message.
                let painted = (0..q.image.tile_count()).any(|i| q.image.tile(i).is_some_and(|t| t.data.iter().any(|v| v.to_f32() > 0.0)));
                let empty = !painted && q.image.fill()[0] <= 0.0;
                if empty {
                    self.quick_mask = None;
                }
            }
            Some(q) => {
                q.editing = true;
                q.revision += 1;
            }
            None => {
                let revision = self.quick_mask.as_ref().map_or(0, |q| q.revision + 1);
                self.quick_mask = Some(QuickMask { texture_set: id, image: subcraft_image::TiledImage::new(res, res, 1, [0.0; 4]), editing: true, revision });
            }
        }
        self.revision += 1;
    }

    /// Painter's Y: remove the quick mask (strokes paint everywhere again).
    pub fn clear_quick_mask(&mut self) {
        if self.stroke.is_none() && self.quick_mask.take().is_some() {
            self.revision += 1;
        }
    }

    /// Painter's I: swap the quick mask's painted and protected areas. Returns false without one.
    pub fn invert_quick_mask(&mut self) -> bool {
        if self.stroke.is_some() {
            return false;
        }
        let id = self.selection.texture_set;
        let Some(q) = self.quick_mask.as_mut().filter(|q| q.texture_set == id) else { return false };
        let img = &q.image;
        let f = img.fill();
        let mut inverted = subcraft_image::TiledImage::new(img.width(), img.height(), img.comps(), [1.0 - f[0], 0.0, 0.0, 0.0]);
        let mut buf = vec![0.0f32; subcraft_image::TILE_TEXELS * img.comps()];
        for t in 0..img.tile_count() {
            if img.is_allocated(t) {
                img.read_tile(t, &mut buf);
                buf.iter_mut().for_each(|v| *v = 1.0 - v.clamp(0.0, 1.0));
                inverted.write_tile(t, &buf);
            }
        }
        q.image = inverted;
        q.revision += 1;
        self.revision += 1;
        true
    }

    /// Pen pressure (0..1) for the following stroke points; a mouse never calls this (1.0).
    pub fn set_stroke_pressure(&mut self, pressure: f32) {
        if let Some(active) = self.stroke.as_mut() {
            active.stroke.set_pressure(pressure);
        }
    }

    /// Continue the stroke to a screen position (`line`: straight segment, no lazy mouse).
    pub fn stroke_to(&mut self, camera: &Camera, screen: Vec2, line: bool) -> Result<Vec<usize>, PaintError> {
        let Some(mut active) = self.stroke.take() else { return Ok(Vec::new()) };
        active.camera = Some(*camera);
        let result = self.with_stroke_context(active.texture_set, camera, |stroke, ts, ctx| {
            if line { stroke.line_to(ts, ctx, screen) } else { stroke.add_point(ts, ctx, screen) }
        }, active);
        if result.as_ref().is_ok_and(|t| !t.is_empty()) {
            self.revision += 1;
        }
        if let Some(offset) = self.stroke.as_ref().and_then(|a| a.stroke.clone_offset()) {
            self.clone_offset = Some(offset);
        }
        result
    }

    /// Continue the stroke through every position of a frame's input events at once (a pen
    /// reports hundreds per second): one stamping pass, one tile rewrite, stamps spaced along the
    /// polyline whatever the event rate. A single position is exactly [`Session::stroke_to`].
    pub fn stroke_to_many(&mut self, camera: &Camera, screens: &[Vec2]) -> Result<Vec<usize>, PaintError> {
        let Some(mut active) = self.stroke.take() else { return Ok(Vec::new()) };
        if screens.is_empty() {
            self.stroke = Some(active);
            return Ok(Vec::new());
        }
        active.camera = Some(*camera);
        let result = self.with_stroke_context(active.texture_set, camera, |stroke, ts, ctx| stroke.add_points(ts, ctx, screens), active);
        if result.as_ref().is_ok_and(|t| !t.is_empty()) {
            self.revision += 1;
        }
        if let Some(offset) = self.stroke.as_ref().and_then(|a| a.stroke.clone_offset()) {
            self.clone_offset = Some(offset);
        }
        result
    }

    /// Clone tool: use the surface point under `screen` as the source (a new offset is fixed by
    /// the next stroke's first dab). Returns false when the cursor is not over the model.
    pub fn set_clone_source(&mut self, camera: &Camera, screen: Vec2) -> bool {
        let Some(pick) = self.pick(camera, screen) else { return false };
        self.clone_source = Some(pick.position);
        self.clone_offset = None;
        self.clone_source_uv = self.doc.texture_set_for_slot(pick.material_slot).map(|ts| (ts.id, pick.uv.rem_euclid(Vec2::ONE)));
        self.clone_offset_uv = None;
        true
    }

    /// Clone tool in the 2D view: use texture position `uv` (0..1, origin top-left) of the
    /// selected texture set as the source. It is also the 3D source (the surface point at that
    /// texel), so either view can paint from it. Returns false outside the set's UV islands.
    pub fn set_clone_source_uv(&mut self, uv: Vec2) -> bool {
        let Some(point) = self.pen_uv_position(uv) else { return false };
        self.clone_source = Some(point);
        self.clone_offset = None;
        self.clone_source_uv = Some((self.selection.texture_set, uv));
        self.clone_offset_uv = None;
        true
    }

    /// The clone source as a texture position of the selected texture set, if one is set there.
    pub fn clone_source_uv(&self) -> Option<Vec2> {
        self.clone_source_uv.filter(|(ts, _)| *ts == self.selection.texture_set).map(|(_, uv)| uv)
    }

    /// UV offset from a 2D clone stroke to its source, once a stroke fixed it.
    pub fn clone_offset_uv(&self) -> Option<Vec2> {
        self.clone_offset_uv
    }

    /// The clone source point (object space), if one is set.
    pub fn clone_source(&self) -> Option<Vec3> {
        self.clone_source
    }

    /// Screen offset from the stroke position to the clone source, once a stroke fixed it.
    pub fn clone_offset(&self) -> Option<Vec2> {
        self.clone_offset
    }

    /// Continue the stroke at a 2D-view texture position (UV 0..1, origin top-left). The stroke
    /// paints UV discs on the texels of the selected layer, without a 3D visibility test.
    /// `texels_per_px` scales the lazy-mouse distance from view pixels to texels.
    pub fn stroke_to_uv(&mut self, uv: Vec2, line: bool, texels_per_px: f32) -> Result<Vec<usize>, PaintError> {
        let Some(active) = self.stroke.take() else { return Ok(Vec::new()) };
        let result = self.with_stroke_context(active.texture_set, &uv_camera(), |stroke, ts, ctx| {
            stroke.add_uv_point(ts, ctx, uv, line, texels_per_px)
        }, active);
        if result.as_ref().is_ok_and(|t| !t.is_empty()) {
            self.revision += 1;
        }
        if let Some(offset) = self.stroke.as_ref().and_then(|a| a.stroke.uv_clone_offset()) {
            self.clone_offset_uv = Some(offset);
        }
        result
    }

    /// Polygon fill from the 2D view: the triangle / element / UV island at `uv` (one history entry).
    pub fn polygon_fill_uv(&mut self, uv: Vec2, mode: PolygonFillMode) -> Result<Vec<usize>, PaintError> {
        self.polygon_fill_with(&uv_camera(), |stroke, ts, ctx| stroke.polygon_fill_uv(ts, ctx, uv, mode))
    }

    /// Brush radius in UV units for the selected texture set (matches [`Session::stroke_to_uv`]
    /// for a stroke begun without a 2D view).
    pub fn brush_radius_uv(&self, uv_per_unit: f32) -> f32 {
        self.brush_radius_uv_in(uv_per_unit, None)
    }

    /// Brush radius in UV units in the 2D view `view` (matches a stroke begun with
    /// [`Session::begin_uv_stroke`] in that view: Viewport size space keeps the brush's size on
    /// screen there).
    pub fn brush_radius_uv_in(&self, uv_per_unit: f32, view: Option<subcraft_paint::UvView>) -> f32 {
        subcraft_paint::stroke::uv_brush_radius(&self.brush.settings, self.scene.mesh.bounds.diagonal(), uv_per_unit, view)
    }

    /// Polygon fill on the selected layer at a screen position (one history entry).
    pub fn polygon_fill(&mut self, camera: &Camera, screen: Vec2, mode: PolygonFillMode) -> Result<Vec<usize>, PaintError> {
        self.polygon_fill_with(camera, |stroke, ts, ctx| stroke.polygon_fill(ts, ctx, screen, mode))
    }

    /// Polygon fill of everything a screen box `[a, b]` touches, grown to `mode` (one history
    /// entry). `visible_only` leaves back faces and occluded triangles alone.
    pub fn polygon_fill_box(&mut self, camera: &Camera, a: Vec2, b: Vec2, mode: PolygonFillMode, visible_only: bool) -> Result<Vec<usize>, PaintError> {
        self.polygon_fill_with(camera, |stroke, ts, ctx| stroke.polygon_fill_box(ts, ctx, a, b, mode, visible_only))
    }

    /// Polygon fill of everything a UV box `[a, b]` touches in the 2D view (one history entry).
    pub fn polygon_fill_uv_box(&mut self, a: Vec2, b: Vec2, mode: PolygonFillMode) -> Result<Vec<usize>, PaintError> {
        self.polygon_fill_with(&uv_camera(), |stroke, ts, ctx| stroke.polygon_fill_uv_box(ts, ctx, a, b, mode))
    }

    /// Every polygon fill: one history entry, nothing left behind when it fills nothing or fails.
    /// On a fill layer or a folder the fill paints the layer's mask, as in Painter (so a smart
    /// material or fill is restricted to the clicked triangle, polygon, mesh, UV island or object
    /// in one click); a layer without a mask gets a black one in the same undo step.
    fn polygon_fill_with(
        &mut self,
        camera: &Camera,
        f: impl FnOnce(&mut Stroke, &mut TextureSet, &PaintContext) -> Result<Vec<usize>, PaintError>,
    ) -> Result<Vec<usize>, PaintError> {
        if self.stroke.is_some() {
            return Ok(Vec::new());
        }
        let routed = self.route_polygon_fill_to_mask();
        if let Err(e) = self.begin_stroke() {
            self.undo_polygon_fill_routing(routed);
            return Err(e);
        }
        let active = self.stroke.take().expect("just begun");
        let result = self.with_stroke_context(active.texture_set, camera, f, active);
        if let Some(a) = self.stroke.as_mut() {
            a.label = "Polygon Fill";
        }
        match &result {
            Ok(tiles) if !tiles.is_empty() => self.end_stroke(),
            _ => {
                self.cancel_stroke();
                self.undo_polygon_fill_routing(routed);
            }
        }
        result
    }

    /// Where a polygon fill on the selected layer goes when its content takes no paint: a fill
    /// layer or folder fills its mask (added black, uncommitted, if missing). Returns the target
    /// to restore if the fill is abandoned, `None` when nothing was rerouted.
    fn route_polygon_fill_to_mask(&mut self) -> Option<PaintTarget> {
        if self.brush.target != PaintTarget::Content || self.quick_mask_editing() {
            return None;
        }
        let layer = self.selected_layer()?;
        if !matches!(layer.content, subcraft_doc::LayerContent::Fill(_) | subcraft_doc::LayerContent::Group(_)) {
            return None;
        }
        let (id, missing) = (layer.id, layer.mask.is_none());
        if missing {
            let ts = self.doc.texture_set_mut(self.selection.texture_set)?;
            let res = ts.resolution;
            ts.find_mut(id)?.mask = Some(Mask::new(res, false));
        }
        let previous = self.brush.target;
        self.brush.target = PaintTarget::MaskBase;
        Some(previous)
    }

    fn undo_polygon_fill_routing(&mut self, routed: Option<PaintTarget>) {
        if let Some(previous) = routed {
            // Drops a mask added for the fill (the document goes back to the last history entry).
            self.doc = self.history.current().clone();
            self.revision += 1;
            self.brush.target = previous;
            self.normalize_target();
        }
    }

    /// Where a polygon fill would land on the selected layer right now: its content (paint layer),
    /// its mask (fill layer or folder, or a mask selected), or `None` when it can't take one.
    pub fn polygon_fill_destination(&self) -> Option<LayerTarget> {
        let layer = self.selected_layer()?;
        if self.brush.target.is_mask() {
            return Some(LayerTarget::Mask);
        }
        match &layer.content {
            subcraft_doc::LayerContent::Paint(p) if p.path.is_none() && p.text.is_none() => Some(LayerTarget::Content),
            subcraft_doc::LayerContent::Fill(_) | subcraft_doc::LayerContent::Group(_) => Some(LayerTarget::Mask),
            _ => None,
        }
    }

    fn with_stroke_context(
        &mut self,
        ts_id: TextureSetId,
        camera: &Camera,
        f: impl FnOnce(&mut Stroke, &mut TextureSet, &PaintContext) -> Result<Vec<usize>, PaintError>,
        mut active: ActiveStroke,
    ) -> Result<Vec<usize>, PaintError> {
        let source = if active.quick { self.quick_scratch.clone() } else { self.doc.texture_set(ts_id).cloned() };
        let Some(mut ts) = source else {
            return Err(PaintError::LayerNotFound);
        };
        let geo = self.scene.geometry.get(ts.material_slot, ts.resolution);
        let view = self.viewport_scene();
        let ctx = PaintContext { mesh: &view.mesh, bvh: &view.bvh, geometry: &geo, camera, depth: &view.depth };
        let result = f(&mut active.stroke, &mut ts, &ctx);
        if active.quick {
            self.quick_scratch = Some(ts);
        } else if let Some(slot) = self.doc.texture_set_mut(ts_id) {
            *slot = ts;
        }
        self.stroke = Some(active);
        result
    }

    /// Finish the stroke: the post-stroke document becomes one history entry (none if nothing
    /// was painted).
    pub fn end_stroke(&mut self) {
        if let Some(active) = self.stroke.take() {
            // Catch-up end: let the brush reach the last cursor position first; then apply the
            // stamps the live preview thinned out (one batched pass).
            if let Some(camera) = active.camera {
                let catch_up = self.brush.options.catch_up;
                let run = |stroke: &mut Stroke, ts: &mut TextureSet, ctx: &PaintContext| -> Result<Vec<usize>, PaintError> {
                    let mut tiles = if catch_up { stroke.finish(ts, ctx)? } else { Vec::new() };
                    tiles.extend(stroke.flush_deferred(ts, ctx)?);
                    Ok(tiles)
                };
                if self.with_stroke_context(active.texture_set, &camera, run, active).is_ok_and(|t| !t.is_empty()) {
                    self.revision += 1;
                }
            } else {
                self.stroke = Some(active);
            }
        }
        let Some(active) = self.stroke.take() else { return };
        if active.quick {
            // The painted quick mask replaces the old one; it is view state, not a history step.
            let painted = self.quick_scratch.take().and_then(|ts| ts.layers.first().and_then(|l| l.mask.as_ref().map(|m| m.base.clone())));
            if let (Some(image), Some(q)) = (painted, self.quick_mask.as_mut()) {
                q.image = image;
                q.revision += 1;
                self.revision += 1;
            }
            return;
        }
        self.sync_instances();
        if self.doc != *self.history.current() {
            log::info!("action: {}", active.label);
            self.history.push(active.label, self.doc.clone());
            self.revision += 1;
        }
        // Kept so the next stroke can continue it (A); dropped when it cannot be.
        self.last_stroke = active.key.is_some().then(|| LastStroke { cursor: self.history.cursor(), entries: self.history.entries().len(), active });
    }

    /// Abandon the stroke and restore the pre-stroke document.
    pub fn cancel_stroke(&mut self) {
        if let Some(active) = self.stroke.take() {
            if active.quick {
                self.quick_scratch = None;
                return;
            }
            self.doc = self.history.current().clone();
            self.revision += 1;
        }
    }

    // ----- compositing ---------------------------------------------------------------------------

    /// Bring every texture set's composite exactly up to date (blocks until done); returns the
    /// tiles re-evaluated since the last update. Export, bake, saving and tests use this.
    pub fn update_composites(&mut self) -> Vec<CompositeUpdate> {
        if self.worker.is_none() {
            return self.update_composites_inline(None);
        }
        self.submit_composites();
        self.worker.as_ref().expect("checked above").finish();
        self.collect_composites()
    }

    /// Interactive frames: never blocks on compositing. The document goes to the background
    /// worker (started on the first call) and whatever it finished since the last call is
    /// returned; [`Session::composites_pending`] says whether more is coming. Where no worker
    /// thread is running, up to about `budget` worth of tiles is evaluated here instead.
    /// Whether layer computation is paused (Shift+Esc).
    pub fn engine_paused(&self) -> bool {
        self.engine_paused
    }

    /// Pause or resume the incremental layer computation. While paused,
    /// [`Session::update_composites_within`] computes and publishes nothing and the pending work
    /// waits; the full [`Session::update_composites`] (export, save) still computes.
    pub fn set_engine_paused(&mut self, paused: bool) {
        self.engine_paused = paused;
    }

    pub fn update_composites_within(&mut self, budget: Option<std::time::Duration>) -> Vec<CompositeUpdate> {
        if self.engine_paused {
            return Vec::new();
        }
        if self.worker.is_none() && !self.inline_only {
            // An exact synchronous composite may already exist (for example after export).
            // Adopting unchanged caches publishes no tiles, so retain their displayed values.
            let shown = self.composites.iter().filter_map(|(id, c)| c.composite().map(|c| (*id, c.clone()))).collect();
            match CompositeWorker::spawn(&mut self.composites) {
                Ok(w) => {
                    self.shown = shown;
                    self.worker = Some(w);
                }
                Err(e) => {
                    log::info!("compositing on the UI thread: {e}");
                    self.inline_only = true;
                }
            }
        }
        if self.worker.is_none() {
            return self.update_composites_inline(budget);
        }
        self.submit_composites();
        self.collect_composites()
    }

    /// Whether composites are not yet up to date with the document (call again next frame).
    pub fn composites_pending(&self) -> bool {
        match &self.worker {
            Some(w) => self.composite_inputs_changed() || !w.idle() || w.has_published(),
            None => self.doc.texture_sets.iter().any(|ts| self.composites.get(&ts.id).is_some_and(|c| c.has_pending())),
        }
    }

    fn update_composites_inline(&mut self, budget: Option<std::time::Duration>) -> Vec<CompositeUpdate> {
        for id in std::mem::take(&mut self.invalidated) {
            if let Some(c) = self.composites.get_mut(&id) {
                c.invalidate();
            }
        }
        let mut out = Vec::new();
        for ts in &self.doc.texture_sets {
            let geo = self.scene.geometry.get(ts.material_slot, ts.resolution);
            let maps = self.mesh_maps.get(&ts.id).unwrap_or(&self.no_maps);
            let ctx = EvalContext { geometry: &geo, resources: &self.resources, mesh_maps: maps };
            let cache = self.composites.entry(ts.id).or_default();
            let tiles = cache.update_within(ts, &ctx, budget);
            if !tiles.is_empty() {
                out.push(CompositeUpdate { texture_set: ts.id, material_slot: ts.material_slot, tiles, changed: cache.last_changed().clone() });
            }
        }
        if !out.is_empty() { self.composite_generation += 1; }
        out
    }

    /// The document, geometry or mesh maps differ from what the worker last received.
    fn composite_inputs_changed(&self) -> bool {
        let Some(last) = &self.submitted else { return true };
        !self.invalidated.is_empty()
            || last.sets.len() != self.doc.texture_sets.len()
            || last.sets.iter().zip(&self.doc.texture_sets).any(|(s, ts)| {
                !same_composite_inputs(&s.texture_set, ts) || !Arc::ptr_eq(&s.geometry, &self.scene.geometry.get(ts.material_slot, ts.resolution))
            })
    }

    /// Hand the current document to the worker if it changed (cheap: layers, tiles and
    /// resources are shared).
    fn submit_composites(&mut self) {
        let Some(worker) = &self.worker else { return };
        if !self.composite_inputs_changed() {
            return;
        }
        let sets = self
            .doc
            .texture_sets
            .iter()
            .map(|ts| SetSnapshot {
                texture_set: ts.clone(),
                geometry: self.scene.geometry.get(ts.material_slot, ts.resolution),
                mesh_maps: self.mesh_maps.get(&ts.id).cloned().unwrap_or_default(),
                preview_geometry: subcraft_compositor::preview_resolution(ts.resolution).map(|r| self.scene.geometry.get(ts.material_slot, r)),
            })
            .collect();
        let snapshot = Snapshot { sets, resources: self.resources.clone(), invalidate: std::mem::take(&mut self.invalidated) };
        worker.submit(snapshot.clone());
        self.submitted = Some(snapshot);
    }

    /// Install what the worker published since the last call.
    fn collect_composites(&mut self) -> Vec<CompositeUpdate> {
        let Some(worker) = &self.worker else { return Vec::new() };
        let mut published = worker.take();
        let mut out = Vec::new();
        for ts in &self.doc.texture_sets {
            let Some(p) = published.remove(&ts.id) else { continue };
            self.shown.insert(ts.id, p.composite);
            out.push(CompositeUpdate { texture_set: ts.id, material_slot: ts.material_slot, tiles: p.tiles, changed: p.changed });
        }
        let doc = &self.doc;
        self.shown.retain(|id, _| doc.texture_set(*id).is_some());
        if !out.is_empty() { self.composite_generation += 1; }
        out
    }

    /// Base name for exported files: the project file name, else the mesh name.
    pub fn export_name(&self) -> String {
        self.project_path
            .as_deref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .map(str::to_string)
            .unwrap_or_else(|| self.scene.mesh.name.clone())
    }

    /// `settings` with the choices saved in the project applied: unchecked maps, normal-map
    /// format and UDIM folders.
    pub fn effective_export_settings(&self, settings: &subcraft_export::ExportSettings) -> subcraft_export::ExportSettings {
        let mut s = settings.clone();
        s.selection = self.export_selection();
        s.udim_folders = self.settings.udim_folders;
        // `$project` and `$sceneMaterial` in file name patterns.
        s.project_name = Some(self.export_name());
        s.scene_materials = self
            .doc
            .texture_sets
            .iter()
            .filter_map(|ts| self.scene.mesh.materials.get(ts.material_slot as usize).map(|m| (ts.name.clone(), m.name.clone())))
            .collect();
        if let Some(format) = self.settings.normal_format {
            s.normal_directx = Some(format == subcraft_doc::NormalFormat::DirectX);
        }
        s
    }

    /// Files an export with `settings` would write (checked maps only).
    pub fn export_plan(&mut self, settings: &subcraft_export::ExportSettings) -> Vec<(String, String)> {
        self.update_composites();
        let settings = self.effective_export_settings(settings);
        let name = self.export_name();
        let sets = self.export_sets();
        subcraft_export::plan(&name, &sets, &settings)
    }

    /// Every map the template offers per texture set, with its checkbox state.
    pub fn export_choices(&mut self, settings: &subcraft_export::ExportSettings) -> Vec<subcraft_export::PlannedFile> {
        self.update_composites();
        let settings = self.effective_export_settings(settings);
        let name = self.export_name();
        let sets = self.export_sets();
        subcraft_export::plan_all(&name, &sets, &settings)
    }

    /// Checked-map selection saved in the project.
    pub fn export_selection(&self) -> subcraft_export::MapSelection {
        subcraft_export::MapSelection::from_stored(&self.settings.export_maps_off, self.settings.export_solo_previous.as_deref())
    }

    /// Change the checked maps (`all` = every offered map, see [`Session::export_choices`]).
    /// Saved with the project; not undoable.
    pub fn edit_export_selection(&mut self, settings: &subcraft_export::ExportSettings, edit: impl FnOnce(&mut subcraft_export::MapSelection, &[subcraft_export::MapKey])) {
        let all: Vec<subcraft_export::MapKey> = self.export_choices(settings).into_iter().map(|f| (f.set, f.suffix)).collect();
        let mut selection = self.export_selection();
        edit(&mut selection, &all);
        self.settings.export_maps_off = selection.stored_off();
        self.settings.export_solo_previous = selection.stored_solo_previous();
    }

    /// Number of distinct UDIM tiles in the project (0 or 1 for ordinary projects).
    pub fn udim_tile_count(&self) -> usize {
        self.doc.texture_sets.iter().filter_map(|t| t.udim_number()).collect::<std::collections::BTreeSet<_>>().len()
    }

    fn export_sets(&self) -> Vec<subcraft_export::ExportTextureSet<'_>> {
        self.doc
            .texture_sets
            .iter()
            .filter_map(|ts| {
                let c = self.composite(ts.id)?;
                Some(subcraft_export::ExportTextureSet {
                    name: &ts.name,
                    material_slot: ts.material_slot,
                    udim: ts.udim_number(),
                    channels: &c.channels,
                    mesh_maps: self.mesh_maps.get(&ts.id).map(|m| m.as_map()),
                    mixing: ts.mixing,
                })
            })
            .collect()
    }

    /// Export every texture set's composite through `settings` into `dir`.
    pub fn export_textures(&mut self, dir: &Path, settings: &subcraft_export::ExportSettings) -> Result<Vec<PathBuf>, String> {
        self.update_composites();
        let settings = self.effective_export_settings(settings);
        let name = self.export_name();
        let sets = self.export_sets();
        subcraft_export::export(&self.scene.mesh, &name, &sets, &settings, dir).map_err(|e| e.to_string())
    }

    /// Export what the 2D view shows (`view`) for the selected texture set as one image, using
    /// the normal export settings (file format, padding). Returns the written file.
    pub fn export_view_2d(&mut self, dir: &Path, view: subcraft_export::View2d, settings: &subcraft_export::ExportSettings) -> Result<Vec<PathBuf>, String> {
        self.update_composites();
        let mut settings = self.effective_export_settings(settings);
        settings.template = subcraft_export::ExportTemplate::view_2d();
        settings.view_2d = Some(view);
        settings.selection = subcraft_export::MapSelection::default();
        let (name, selected) = (self.export_name(), self.selected_texture_set().name.clone());
        let sets: Vec<_> = self.export_sets().into_iter().filter(|s| s.name == selected).collect();
        subcraft_export::export(&self.scene.mesh, &name, &sets, &settings, dir).map_err(|e| e.to_string())
    }

    /// Export raw baked mesh maps (issue #93): every map in `kinds` of every texture set that has
    /// it baked (only the selected set when `selected_only`), one file each named
    /// `<mesh>_<set>_baked_<map>`, using the export settings' padding and file format override.
    /// Maps a set has not baked are skipped.
    pub fn export_baked_maps(&mut self, dir: &Path, kinds: &[subcraft_core::MeshMapKind], selected_only: bool, settings: &subcraft_export::ExportSettings) -> Result<Vec<PathBuf>, String> {
        // Export sets are listed from the composites, so they must exist (the maps themselves are raw).
        self.update_composites();
        let mut settings = self.effective_export_settings(settings);
        settings.selection = subcraft_export::MapSelection::default();
        let (name, selected) = (self.export_name(), self.selected_texture_set().name.clone());
        let sets: Vec<_> = self.export_sets().into_iter().filter(|s| !selected_only || s.name == selected).collect();
        let mut written = Vec::new();
        for &kind in kinds {
            settings.template = subcraft_export::ExportTemplate::baked_map(kind);
            settings.view_2d = Some(subcraft_export::View2d::MeshMap(kind));
            written.extend(subcraft_export::export(&self.scene.mesh, &name, &sets, &settings, dir).map_err(|e| e.to_string())?);
        }
        Ok(written)
    }

    /// Baked mesh maps available for export: those at least one texture set has baked.
    pub fn baked_map_kinds(&self) -> Vec<subcraft_core::MeshMapKind> {
        subcraft_core::MeshMapKind::ALL.into_iter().filter(|k| self.document().texture_sets.iter().any(|ts| self.mesh_maps(ts.id).is_some_and(|m| m.get(*k).is_some()))).collect()
    }

    /// Export textures plus a scene file (OBJ+MTL, glTF, GLB or USDA) into `dir`.
    pub fn export_scene(&mut self, dir: &Path, settings: &subcraft_export::ExportSettings, format: subcraft_export::scene::SceneFormat) -> Result<Vec<PathBuf>, String> {
        self.update_composites();
        let settings = self.effective_export_settings(settings);
        let name = self.export_name();
        let sets = self.export_sets();
        subcraft_export::scene::export_scene(&self.scene.mesh, &name, &sets, &settings, format, dir).map_err(|e| e.to_string())
    }

    /// The texture set holding `layer`, and a copy of it whose stack is that layer alone (visible).
    fn isolated(&self, layer: LayerId) -> Option<(&TextureSet, TextureSet, Arc<Layer>)> {
        let ts = self.doc.texture_sets.iter().find(|ts| ts.find(layer).is_some())?;
        let mut l = ts.find(layer)?.clone();
        l.visible = true;
        let l = Arc::new(l);
        let mut alone = ts.clone();
        alone.layers = vec![l.clone()];
        Some((ts, alone, l))
    }

    fn composite_of(&self, ts: &TextureSet, stack: &TextureSet) -> Composite {
        let geo = self.scene.geometry.get(ts.material_slot, ts.resolution);
        let maps = self.mesh_maps.get(&ts.id).unwrap_or(&self.no_maps);
        let ctx = EvalContext { geometry: &geo, resources: &self.resources, mesh_maps: maps };
        subcraft_compositor::composite_full(stack, &ctx)
    }

    /// Painter's Export flattened: each layer (a folder with its content) composited on its own,
    /// one file per channel it blends into, named by `pattern`
    /// (`subcraft_export::layer_export::FLATTENED_PATTERN`).
    pub fn export_flattened_layers(&self, layers: &[LayerId], dir: &Path, pattern: &str) -> Result<Vec<PathBuf>, String> {
        let name = self.export_name();
        let mut out = Vec::new();
        for &id in layers {
            let Some((ts, alone, layer)) = self.isolated(id) else { continue };
            let c = self.composite_of(ts, &alone);
            let set = subcraft_export::ExportTextureSet { name: &ts.name, material_slot: ts.material_slot, udim: ts.udim_number(), channels: &c.channels, mesh_maps: None, mixing: ts.mixing };
            // Every layer has a blend entry for every channel of its set: the filter was no filter
            // (QA 2026-11: a fill setting Base Color only also wrote Roughness, Metallic, Height...
            // at their defaults). A channel is written when its blending is on and the layer's
            // content gives it (folders and content effects: assumed to).
            let writes = |kind: ChannelKind| {
                layer.blend.get(&kind).is_some_and(|b| b.is_active())
                    && (!layer.content_effects.is_empty()
                        || match &layer.content {
                            LayerContent::Fill(f) => f.channels.get(&kind).is_some_and(|c| c.enabled),
                            LayerContent::Paint(p) => p.channels.contains_key(&kind),
                            LayerContent::Group(_) => true,
                        })
            };
            let channels: Vec<_> = ts.channels.iter().filter(|d| writes(d.kind)).map(|d| (d.kind, d.format)).collect();
            out.extend(subcraft_export::layer_export::write_flattened(&self.scene.mesh, &name, &set, &layer.name, &channels, pattern, dir).map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    /// Painter's Export mask: each layer's mask (its effects evaluated) as one gray file.
    pub fn export_layer_masks(&self, layers: &[LayerId], dir: &Path, pattern: &str, format: subcraft_image::FileFormat) -> Result<Vec<PathBuf>, String> {
        let name = self.export_name();
        let mut out = Vec::new();
        for &id in layers {
            let Some((ts, mut alone, layer)) = self.isolated(id) else { continue };
            let Some(mask) = layer.mask.clone() else { continue };
            // A white fill through the mask into a channel that starts black reads the mask back.
            alone.channels = vec![ChannelDef::default_for(ChannelKind::Metallic)];
            let mut probe = Layer::new_fill("mask", &alone);
            if let LayerContent::Fill(f) = &mut probe.content {
                f.channels.insert(ChannelKind::Metallic, subcraft_doc::FillChannel::new(subcraft_doc::FillSource::uniform(subcraft_core::ChannelValue::scalar(1.0))));
            }
            probe.mask = Some(mask);
            alone.layers = vec![Arc::new(probe)];
            let c = self.composite_of(ts, &alone);
            let Some(img) = c.channels.get(&ChannelKind::Metallic) else { continue };
            let set = subcraft_export::ExportTextureSet { name: &ts.name, material_slot: ts.material_slot, udim: ts.udim_number(), channels: &c.channels, mesh_maps: None, mixing: ts.mixing };
            out.push(subcraft_export::layer_export::write_mask(&self.scene.mesh, &name, &set, &layer.name, img, format, pattern, dir).map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    /// File > Export mesh: the mesh alone (optionally displaced by the Height channel) into `dir`.
    pub fn export_mesh(&mut self, dir: &Path, options: &subcraft_export::mesh_export::MeshExport) -> Result<Vec<PathBuf>, String> {
        if options.displacement.is_some() {
            self.update_composites();
        }
        let name = self.export_name();
        let sets = self.export_sets();
        subcraft_export::mesh_export::export_mesh(&self.scene.mesh, &name, &sets, options, dir).map_err(|e| e.to_string())
    }

    /// Export every map as one in-memory ZIP archive.
    pub fn export_archive(&mut self, settings: &subcraft_export::ExportSettings) -> Result<subcraft_export::TextureArchive,String> {
        self.update_composites();let settings=self.effective_export_settings(settings);let name=self.export_name();let sets=self.export_sets();
        subcraft_export::archive(&self.scene.mesh,&name,&sets,&settings).map_err(|e|e.to_string())
    }

    // ----- assets --------------------------------------------------------------------------------

    /// A material clicked on the Shelf: on a selected fill layer it swaps that layer's material
    /// (channels and name; its mask, parts, opacity and blend modes stay), otherwise it adds a new
    /// fill layer above the selection. One undo step either way. Returns true when it swapped.
    pub fn apply_material(&mut self, preset: &subcraft_assets::MaterialPreset) -> Result<bool, CommandError> {
        let Some(old) = self.selected_layer().filter(|l| matches!(l.content, subcraft_doc::LayerContent::Fill(_))).cloned() else {
            self.add_material(preset)?;
            return Ok(false);
        };
        let ids = self.import_preset_images(&preset.images)?;
        let fresh = preset.build_with(self.selected_texture_set(), &ids);
        let mut replacement = old.clone();
        replacement.content = fresh.content;
        replacement.name = fresh.name;
        self.execute(Command::ReplaceLayer { layer: old.id, replacement: Box::new(replacement), label: format!("Swap Material: {}", preset.name) })?;
        Ok(true)
    }

    /// Add a material preset as a fill layer above the selection.
    pub fn add_material(&mut self, preset: &subcraft_assets::MaterialPreset) -> Result<(), CommandError> {
        let ids = self.import_preset_images(&preset.images)?;
        let layer = preset.build_with(self.selected_texture_set(), &ids);
        self.execute(Command::InsertLayer { layer: Box::new(layer), label: format!("Material: {}", preset.name) })
    }

    /// Add a smart material (folder of masked layers) above the selection.
    pub fn add_smart_material(&mut self, smart: &subcraft_assets::SmartMaterial) -> Result<(), CommandError> {
        for r in smart.resources() {
            self.resources.insert_shared(r.clone());
        }
        let ids = self.import_preset_images(smart.images())?;
        let layer = smart.build_with(self.selected_texture_set(), &ids);
        let label = format!("Smart Material: {}", smart.name);
        // Imported texture materials may bring AO / emissive / opacity maps the set lacks.
        let missing: Vec<_> = crate::smart_import::channels_to_add(&layer).into_iter().filter(|&k| !self.selected_texture_set().has_channel(k)).collect();
        if missing.is_empty() {
            self.execute(Command::InsertLayer { layer: Box::new(layer), label })
        } else {
            self.execute(Command::InsertLayerWithChannels { layer: Box::new(layer), channels: missing, label })
        }
    }

    /// Import a built-in preset's embedded images (reusing identical ones already in the project),
    /// so the layer's image fills resolve and saving embeds them.
    pub(crate) fn import_preset_images(&mut self, images: &[subcraft_assets::PresetImage]) -> Result<subcraft_assets::ImageIds, CommandError> {
        let mut ids = subcraft_assets::ImageIds::new();
        for img in images {
            let id = self.import_image_bytes(img.name, img.bytes.to_vec()).map_err(CommandError::Invalid)?;
            ids.insert(img.name, id);
        }
        Ok(ids)
    }

    /// Import a Painter smart material file (`.spsm`) as a folder above the selection, rebuilt for
    /// the selected texture set like a Painter project's layer stack (fills, masks, generators and
    /// levels; procedural Painter graphs are approximated or kept as hidden placeholders). One
    /// history entry. Returns the folder's name and how many layers it holds.
    pub fn import_painter_smart_material(&mut self, path: &Path) -> Result<(String, usize), String> {
        let fallback = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Smart Material").to_string();
        let mut spp = subcraft_mesh::spp::SppFile::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let info = spp.smart_material(&fallback).map_err(|e| format!("{}: {e}", path.display()))?
            .ok_or_else(|| format!("{} is not a smart material", path.display()))?;
        let ts = self.selected_texture_set().clone();
        let geometry = self.scene.geometry.get(ts.material_slot, ts.resolution);
        let maps = self.mesh_maps.get(&ts.id).cloned().unwrap_or_default();
        let mut resources = std::mem::take(&mut self.resources);
        let layers = {
            let mut rebuilder = crate::spp_stack::Rebuilder::new(&mut spp, &ts, &mut resources);
            rebuilder.mesh_maps = Some(maps);
            rebuilder.geometry = Some(geometry);
            rebuilder.rebuild(std::slice::from_ref(&info))
        };
        self.resources = resources;
        let mut layer = layers.into_iter().next().ok_or_else(|| format!("{} holds no layers", path.display()))?;
        if layer.name.trim().is_empty() { layer.name = fallback; }
        fn count(l: &Layer) -> usize { 1 + l.children().iter().map(|c| count(c)).sum::<usize>() }
        let count = count(&layer) - 1;
        let name = layer.name.clone();
        let missing: Vec<_> = crate::smart_import::channels_to_add(&layer).into_iter().filter(|&k| !self.selected_texture_set().has_channel(k)).collect();
        let label = format!("Smart Material: {name}");
        let result = if missing.is_empty() {
            self.execute(Command::InsertLayer { layer: Box::new(layer), label })
        } else {
            self.execute(Command::InsertLayerWithChannels { layer: Box::new(layer), channels: missing, label })
        };
        result.map_err(|e| e.to_string())?;
        Ok((name, count))
    }

    /// Apply a smart mask to the selected layer.
    pub fn apply_smart_mask(&mut self, mask: &subcraft_assets::SmartMask) -> Result<(), CommandError> {
        self.apply_smart_mask_with(mask, false)
    }

    /// `apply_smart_mask`; with `replace` the mask's previous effects are removed first.
    pub fn apply_smart_mask_with(&mut self, mask: &subcraft_assets::SmartMask, replace: bool) -> Result<(), CommandError> {
        let layer = self.selection.layer.ok_or(CommandError::LayerNotFound)?;
        for r in &mask.resources {
            self.resources.insert_shared(r.clone());
        }
        let (effects, label) = (mask.effects.clone(), format!("Smart Mask: {}", mask.name));
        self.execute(if replace { Command::ReplaceMaskEffects { layer, effects, label } } else { Command::ApplyMaskEffects { layer, effects, label } })
    }

    /// Add a colour profile LUT to the project from its file bytes (an EXR / HDR / PNG... image
    /// whose size holds a whole cube, see `display::lut_cube_size`). The original bytes are kept,
    /// so saving stores the file as it was. Returns the new resource's id.
    pub fn add_color_lut(&mut self, name: &str, bytes: Vec<u8>) -> Result<subcraft_core::ResourceId, String> {
        let bitmap = subcraft_image::io::decode_bitmap(&bytes, Some(name)).map_err(|e| format!("{name}: {e}"))?;
        if subcraft_doc::display::lut_cube_size(bitmap.width, bitmap.height).is_none() {
            return Err(format!("{name} is {} x {}: not a 3D LUT (width x height must be a cube, such as 2048 x 128)", bitmap.width, bitmap.height));
        }
        Ok(self.resources.insert(subcraft_doc::Resource {
            id: subcraft_core::ResourceId::new(),
            name: name.to_string(),
            kind: subcraft_doc::ResourceKind::ColorLut,
            bitmap: Arc::new(bitmap),
            source_path: None,
            encoded: Some(Arc::new(bytes)),
        }))
    }

    /// Use a brush alpha for the next strokes.
    /// The current brush tip as an Alpha resource of the project (the same resource again for
    /// the same tip), so a pen path can stamp it and the project keeps it. `None` for the round tip.
    pub fn brush_tip_resource(&mut self) -> Option<subcraft_core::ResourceId> {
        let bitmap = self.brush.alpha.clone()?;
        if let Some(r) = self.resources.iter().find(|r| Arc::ptr_eq(&r.bitmap, &bitmap)) {
            return Some(r.id);
        }
        // A reopened project holds a loaded copy of the same tip: compared by pointer only, each
        // session added (and saved) another copy of it.
        if let Some(r) = self.resources.iter().find(|r| r.kind == subcraft_doc::ResourceKind::Alpha && *r.bitmap == *bitmap) {
            return Some(r.id);
        }
        Some(self.resources.insert(subcraft_doc::Resource {
            id: subcraft_core::ResourceId::new(),
            name: self.brush.alpha_name.clone(),
            kind: subcraft_doc::ResourceKind::Alpha,
            bitmap,
            source_path: None,
            encoded: None,
        }))
    }

    pub fn set_brush_alpha(&mut self, alpha: &subcraft_assets::BrushAlpha) {
        self.clear_brush_stamp();
        self.set_brush_tip(alpha);
    }

    /// Set the tip image only (presets restore their own stamp state separately).
    pub(crate) fn set_brush_tip(&mut self, alpha: &subcraft_assets::BrushAlpha) {
        self.brush.alpha = alpha.bitmap.clone();
        self.brush.alpha_name = alpha.name.clone();
    }

    /// Channels present in the selected texture set.
    pub fn channels(&self) -> Vec<ChannelKind> {
        self.selected_texture_set().channel_kinds().collect()
    }
}

/// Two texture sets composite identically (names aside): same tiling, channels and layers.
/// Placeholder camera for 2D-view strokes: UV stamps never project through it.
fn uv_camera() -> Camera {
    Camera { view: glam::Mat4::IDENTITY, proj: glam::Mat4::IDENTITY, viewport: [1, 1] }
}

fn same_composite_inputs(a: &TextureSet, b: &TextureSet) -> bool {
    a.id == b.id
        && a.material_slot == b.material_slot
        && a.resolution == b.resolution
        && a.albedo_filter == b.albedo_filter
        && a.transparent == b.transparent
        && a.channels == b.channels
        && a.layers.len() == b.layers.len()
        && a.layers.iter().zip(&b.layers).all(|(x, y)| Arc::ptr_eq(x, y))
}

/// Give every layer of a tree its blend entry for a channel the texture set just gained
/// (folders pass it through; without an entry a layer ignores the channel).
fn add_blend(layers: &mut [Arc<Layer>], channel: ChannelKind) {
    for layer in layers {
        let l = Arc::make_mut(layer);
        let mode = if l.is_folder() { subcraft_doc::BlendMode::Passthrough } else { subcraft_doc::BlendMode::Normal };
        l.blend.entry(channel).or_insert(subcraft_doc::ChannelBlend::new(mode));
        if let Some(children) = l.children_mut() {
            add_blend(children, channel);
        }
    }
}

/// Layers pasted from a set of another resolution: their text and pen path rasters are drawn
/// again at `to` (resampled, a path or text from 512 pasted at 4096 came out blurred, and was
/// exported that way until its next edit).
fn redraw_pasted(layers: Vec<Layer>, from: u32, to: u32, resources: &ResourceStore) -> Vec<Layer> {
    if from == to {
        return layers;
    }
    let mut layers: Vec<Arc<Layer>> = layers.into_iter().map(Arc::new).collect();
    crate::text::redraw_text_layers(&mut layers, to, resources);
    crate::pen::redraw_path_layers(&mut layers, to, resources);
    layers.into_iter().map(Arc::unwrap_or_clone).collect()
}
