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

/// Default texture-set resolution for new sessions.
pub const DEFAULT_RESOLUTION: u32 = 1024;

/// Immutable geometry of the session and everything derived from it.
pub struct Scene {
    symmetry_sections: std::sync::OnceLock<[Vec<[Vec3; 2]>; 3]>,
    pub mesh: Arc<Mesh>,
    pub bvh: Bvh,
    pub geometry: GeometryCache,
    pub depth: DepthCache,
}

impl Scene {
    pub fn new(mesh: Mesh) -> Self {
        let mesh = Arc::new(mesh);
        Self { symmetry_sections: Default::default(), bvh: Bvh::build(&mesh), geometry: GeometryCache::new(mesh.clone()), depth: DepthCache::new(), mesh }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub texture_set: TextureSetId,
    pub layer: Option<LayerId>,
}

/// The selected layer's editable domain. Transient tool state, never project data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerTarget { Content, Mask }

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
}

pub struct Session {
    pub(crate) part_view: crate::part_workflow::PartView,
    scene: Scene,
    resources: ResourceStore,
    mesh_maps: HashMap<TextureSetId, MeshMaps>,
    no_maps: MeshMaps,
    history: History,
    /// Live document: equals the current history entry except while a stroke is in progress.
    doc: Document,
    selection: Selection,
    mask_clipboard: Option<Mask>,
    /// Selected effect of the selected layer's current domain (see [`Session::selected_effect`]).
    effect: Option<EffectId>,
    pub brush: BrushState,
    stroke: Option<ActiveStroke>,
    /// Clone tool: object-space source point, and the screen offset the first stroke fixed.
    clone_source: Option<Vec3>,
    clone_offset: Option<Vec2>,
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
    revision: u64,
    layer_counter: u32,
    project_path: Option<PathBuf>,
    /// Document as of the last save / load / new (for the unsaved-changes indicator).
    saved_doc: Document,
    /// Saved with the project but outside the undo history (export choices, normal format).
    settings: subcraft_doc::ProjectSettings,
    saved_settings: subcraft_doc::ProjectSettings,
    /// Mesh maps changed (baked) since the last save.
    maps_dirty: bool,
    /// Bumped whenever baked mesh maps change (autosave change tracking).
    maps_generation: u64,
    /// Opened from a recovery copy: unsaved until the user saves.
    recovered: bool,
    /// Live Substance `.sbsar` graph parameters for imported SBSAR fill layers.
    pub sbsar_layers: HashMap<LayerId, crate::sbsar::SbsarLayerParams>,
    /// Live Substance `.sbsar` graph parameters for imported SBSAR content effects.
    pub sbsar_effects: HashMap<(LayerId, EffectId), crate::sbsar::SbsarLayerParams>,
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
            no_maps: MeshMaps::new(),
            history: History::new(doc.clone(), label),
            saved_doc: doc.clone(),
            settings: Default::default(),
            saved_settings: Default::default(),
            doc,
            selection,
            mask_clipboard: None,
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
            },
            stroke: None,
            clone_source: None,
            clone_offset: None,
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
            sbsar_layers: HashMap::new(),
            sbsar_effects: HashMap::new(),
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
        let maps: subcraft_project::MeshMapSet = self.mesh_maps.iter().map(|(id, m)| (*id, m.as_map().clone())).collect();
        subcraft_project::save_project_with(&path, &self.scene.mesh, &self.doc, &self.resources, &maps, &self.settings).map_err(|e| e.to_string())?;
        self.project_path = Some(path.clone());
        self.saved_doc = self.doc.clone();
        self.saved_settings = self.settings.clone();
        self.maps_dirty = false;
        self.recovered = false;
        Ok(path)
    }

    /// The last committed state (no half-finished stroke) for a background save.
    pub fn snapshot(&self) -> ProjectSnapshot {
        ProjectSnapshot {
            mesh: self.scene.mesh.clone(),
            document: self.history.current().clone(),
            resources: self.resources.clone(),
            mesh_maps: self.mesh_maps.iter().map(|(id, m)| (*id, m.as_map().clone())).collect(),
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
        self.maps_dirty || self.recovered || self.doc != self.saved_doc || self.settings != self.saved_settings
    }

    /// Project-wide settings (export choices, normal format), saved with the project.
    pub fn settings(&self) -> &subcraft_doc::ProjectSettings {
        &self.settings
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

    /// Install baked maps (merged over existing ones) and refresh that set's composite.
    pub fn set_mesh_maps(&mut self, ts: TextureSetId, maps: std::collections::BTreeMap<subcraft_core::MeshMapKind, subcraft_image::TiledImage>) {
        let entry = self.mesh_maps.entry(ts).or_default();
        let mut merged = entry.as_map().clone();
        merged.extend(maps);
        *entry = MeshMaps::from_map(merged);
        self.invalidated.insert(ts);
        self.maps_dirty = true;
        self.maps_generation += 1;
        self.revision += 1;
    }

    /// Start baking on a background thread; poll the job each frame.
    pub fn start_bake(&self, request: crate::bake_job::BakeRequest) -> crate::bake_job::BakeJob {
        crate::bake_job::BakeJob::start(self.scene.mesh.clone(), Arc::new(self.scene.bvh.clone()), request)
    }

    /// Reuse the high-poly mesh already loaded for the bake preview.
    pub fn start_bake_prepared(&self, request: crate::bake_job::BakeRequest, high: Option<crate::bake_job::BakePreviewMesh>) -> crate::bake_job::BakeJob {
        crate::bake_job::BakeJob::start_prepared(self.scene.mesh.clone(), Arc::new(self.scene.bvh.clone()), request, high)
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

    /// The built-in two-material sample crate.
    pub fn sample() -> Self {
        Self::new(primitives::sample_crate(), DEFAULT_RESOLUTION)
    }

    /// New session from a mesh file (OBJ / glTF / FBX).
    pub fn open_mesh(path: &Path, resolution: u32) -> Result<(Self, Vec<String>), String> {
        let report = import_mesh(path).map_err(|e| e.to_string())?;
        if report.mesh.triangle_count() == 0 {
            return Err("mesh has no triangles".into());
        }
        let mut warnings = report.warnings;
        let tiles = report.mesh.udim_tiles(None).len();
        if tiles > 1 {
            // Folding the tiles onto one would make their islands paint over each other.
            let settings = crate::new_project::NewProjectSettings { resolution, ..Default::default() };
            let session = Self::from_mesh_with(report.mesh, &settings)?;
            warnings.push(format!("The model uses {tiles} UDIM tiles; each tile became its own texture set (use File > New project to choose differently)."));
            return Ok((session, warnings));
        }
        Ok((Self::new(report.mesh, resolution), warnings))
    }

    // ----- queries -------------------------------------------------------------------------------

    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    pub fn symmetry_guide_segments(&self) -> Vec<[Vec3; 2]> {
        if !self.brush.options.symmetry.contains(&true) { return Vec::new(); }
        let sections = self.scene.symmetry_sections.get_or_init(|| crate::guides::symmetry_sections(&self.scene.mesh));
        sections.iter().zip(self.brush.options.symmetry).filter(|(_, enabled)| *enabled).flat_map(|(lines, _)| lines.iter().copied()).collect()
    }
    pub fn document(&self) -> &Document {
        &self.doc
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

    /// Publication generation, so consumers can detect results installed by another caller
    /// (for example export or the eyedropper) even if it consumed the tile notifications.
    pub fn composite_generation(&self) -> u64 {
        self.composite_generation
    }

    fn texture_set_of(&self, layer: LayerId) -> Option<TextureSetId> {
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

    /// World radius of the current brush.
    pub fn brush_radius(&self) -> f32 {
        self.brush.settings.radius(self.scene.mesh.bounds.diagonal())
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

    fn commit(&mut self, label: &str, merge_key: Option<String>) {
        // Recent actions end up in the log (and in crash reports). Slider drags merge into one
        // history entry; log only the first step.
        if merge_key.is_none() || self.history.current_merge_key() != merge_key.as_deref() {
            log::info!("action: {label}");
        }
        self.history.push_merge(label, self.doc.clone(), merge_key);
        self.revision += 1;
    }

    /// Apply `f` to the texture set containing `layer`, then record history.
    fn edit_layer_set<R>(
        &mut self,
        layer: LayerId,
        label: &str,
        merge_key: Option<String>,
        f: impl FnOnce(&mut TextureSet) -> Result<R, CommandError>,
    ) -> Result<R, CommandError> {
        let ts_id = self.texture_set_of(layer).ok_or(CommandError::LayerNotFound)?;
        let ts = self.doc.texture_set_mut(ts_id).ok_or(CommandError::TextureSetNotFound)?;
        let r = f(ts)?;
        self.commit(label, merge_key);
        Ok(r)
    }

    fn layer_mut(ts: &mut TextureSet, id: LayerId) -> Result<&mut Layer, CommandError> {
        ts.find_mut(id).ok_or(CommandError::LayerNotFound)
    }

    fn sync_after_history_move(&mut self) {
        self.doc = self.history.current().clone();
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
                if [settings.size, settings.flow, settings.stroke_opacity, settings.hardness].iter().any(|v| !v.is_finite()) {
                    return Err(CommandError::Invalid("brush values must be finite".into()));
                }
                settings.size = settings.size.clamp(0.1, 100.0);
                settings.flow = settings.flow.clamp(0.0, 1.0);
                settings.stroke_opacity = settings.stroke_opacity.clamp(0.0, 1.0);
                settings.hardness = settings.hardness.clamp(0.0, 1.0);
                self.brush.settings = settings;
            }
            Command::SetStrokeOptions(mut options) => {
                if !options.lazy_distance_px.is_finite() {
                    return Err(CommandError::Invalid("lazy mouse distance must be finite".into()));
                }
                options.lazy_distance_px = options.lazy_distance_px.clamp(0.0, 100.0);
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
                let id = self.edit_layer_set(layer, "Add Effect", None, |ts| {
                    let effect = ContentEffect::new(kind, ts);
                    let l = Self::layer_mut(ts, layer)?;
                    if l.is_folder() && !effect.kind.is_adjustment() {
                        return Err(CommandError::Invalid("Folders support adjustment effects only".into()));
                    }
                    let id = effect.id;
                    l.content_effects.push(Arc::new(effect));
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
                    if l.is_folder() && !effect.kind.is_adjustment() { return Err(CommandError::Invalid("Folders support adjustment effects only".into())); }
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
            Command::AddLayer(kind) => {
                self.layer_counter += 1;
                let ts_id = self.selection.texture_set;
                let reference = self.selection.layer;
                let ts = self.doc.texture_set_mut(ts_id).ok_or(CommandError::TextureSetNotFound)?;
                let tc = ts.clone();
                let (layer, label) = match kind {
                    NewLayer::Paint => (Layer::new_paint(format!("Layer {}", self.layer_counter), &tc), "New Paint Layer"),
                    NewLayer::Fill => (Layer::new_fill(format!("Fill {}", self.layer_counter), &tc), "New Fill Layer"),
                    NewLayer::Folder => (Layer::new_folder(format!("Folder {}", self.layer_counter), &tc), "New Folder"),
                };
                let id = ts.insert_above(reference, layer);
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
                let id = ts.insert_above(reference, layer);
                self.selection.layer = Some(id);
                self.commit(&label, None);
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
                for ts in self.doc.texture_sets.iter_mut().filter(|ts| texture_set.is_none_or(|id| ts.id == id)) {
                    changed |= ts.resolution != resolution;
                    ts.set_resolution(resolution);
                }
                if changed {
                    self.commit(&format!("Resolution {resolution}"), None);
                }
            }
            Command::InsertLayerWithChannels { layer, channels, label } => {
                let ts = self.doc.texture_set_mut(self.selection.texture_set).ok_or(CommandError::TextureSetNotFound)?;
                for kind in channels {
                    if !ts.has_channel(kind) {
                        ts.channels.push(ChannelDef::default_for(kind));
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
            Command::DeleteLayer(id) => {
                let loc = self.edit_layer_set(id, "Delete Layer", None, |ts| {
                    let loc = ts.location(id);
                    ts.remove(id).map_err(|_| CommandError::LayerNotFound)?;
                    Ok(loc)
                })?;
                if self.selection.layer == Some(id) {
                    self.selection.layer = self.neighbour_after_removal(loc);
                }
            }
            Command::DuplicateLayer(id) => {
                let copy = self.edit_layer_set(id, "Duplicate Layer", None, |ts| ts.duplicate(id).map_err(|_| CommandError::LayerNotFound))?;
                self.selection.layer = Some(copy);
            }
            Command::GroupLayer(id) => {
                self.edit_layer_set(id, "Group Layer", None, |ts| ts.group(id, "Folder").map_err(|_| CommandError::LayerNotFound))?;
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
                    b.opacity = opacity.clamp(0.0, 1.0);
                    Ok(())
                })?;
            }
            Command::SetFillChannel { layer, channel, fill } => {
                let key = format!("fill:{layer}:{channel:?}");
                self.edit_layer_set(layer, "Fill", Some(key), |ts| {
                    let LayerContent::Fill(f) = &mut Self::layer_mut(ts, layer)?.content else { return Err(CommandError::NotAFillLayer) };
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
                    mask.effects = mask.effects.iter().map(|effect| {
                        let mut effect = (**effect).clone();
                        effect.id = subcraft_core::EffectId::new();
                        if let subcraft_doc::MaskEffectKind::Paint { image } = &mut effect.kind {
                            *image = image.resampled(ts.resolution, ts.resolution);
                        }
                        Arc::new(effect)
                    }).collect();
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
                self.edit_layer_set(layer, "Layer Parts", None, |ts| {
                    Self::layer_mut(ts, layer)?.parts = parts;
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
                let id = self.edit_layer_set(layer, "Add Mask Effect", None, |ts| {
                    let mask = Self::layer_mut(ts, layer)?.mask.as_mut().ok_or(CommandError::NoMask)?;
                    let effect = MaskEffect::new(kind);
                    let id = effect.id;
                    mask.effects.push(Arc::new(effect));
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

    fn check_content_effect(kind: &ContentEffectKind) -> Result<(), CommandError> {
        if let ContentEffectKind::Filter { channel, filter } = kind
            && (!filter.valid() || !filter.supports_channel(*channel)) {
            return Err(CommandError::Invalid("Filter parameters or target channel are invalid".into()));
        }
        if matches!(kind, ContentEffectKind::Levels { channel: subcraft_core::ChannelKind::Normal, .. }) {
            return Err(CommandError::Invalid("Levels cannot adjust the Normal channel".into()));
        }
        Ok(())
    }

    fn check_mask_effect(kind: &subcraft_doc::MaskEffectKind) -> Result<(), CommandError> {
        if let subcraft_doc::MaskEffectKind::Filter(filter) = kind
            && (!filter.valid() || !filter.supports_mask()) {
            return Err(CommandError::Invalid("Filter parameters or mask target are invalid".into()));
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
            Action::NewFolder => self.execute(Command::AddLayer(NewLayer::Folder))?,
            Action::DeleteLayer => {
                if let Some(l) = sel {
                    self.execute(Command::DeleteLayer(l))?;
                }
            }
            Action::DuplicateLayer => {
                if let Some(l) = sel {
                    self.execute(Command::DuplicateLayer(l))?;
                }
            }
            Action::BrushTool => self.brush.mode = StrokeMode::Paint,
            Action::EraserTool => self.brush.mode = StrokeMode::Erase,
            Action::BrushSmaller => self.brush.settings.size = (self.brush.settings.size / 1.15).max(0.1),
            Action::BrushLarger => self.brush.settings.size = (self.brush.settings.size * 1.15).min(100.0),
            Action::ToggleSymmetryX => self.brush.options.symmetry[0] = !self.brush.options.symmetry[0],
            Action::PenTool | Action::ResetCamera | Action::Save | Action::SaveAs | Action::OpenProject | Action::Export | Action::Bake => return Ok(false),
        }
        Ok(true)
    }

    // ----- painting ------------------------------------------------------------------------------

    /// Start a stroke on the selected layer with the current brush.
    pub fn begin_stroke(&mut self) -> Result<(), PaintError> {
        if self.stroke.is_some() {
            return Ok(());
        }
        let layer = self.selection.layer.ok_or(PaintError::LayerNotFound)?;
        let ts = self.selected_texture_set();
        let b = &self.brush;
        let mut stroke = Stroke::begin(ts, layer, b.target, b.mode, b.settings, b.options, &b.material)?
            .with_resources(self.resources())
            .with_alpha(b.alpha.clone())
            .with_stencil(if b.stencil_bypass { None } else { b.stencil.clone() });
        if b.mode == StrokeMode::Clone {
            let point = self.clone_source.ok_or(PaintError::NoCloneSource)?;
            stroke = stroke.with_clone(point, self.clone_offset);
        }
        let b = &self.brush;
        let label = match (b.mode, b.target) {
            (StrokeMode::Smudge, _) => "Smudge Stroke",
            (StrokeMode::Clone, _) => "Clone Stroke",
            (StrokeMode::Paint, PaintTarget::Content) => "Paint Stroke",
            (StrokeMode::Erase, PaintTarget::Content) => "Erase Stroke",
            (StrokeMode::Paint, PaintTarget::ContentEffect(_)) => "Paint Effect Stroke",
            (StrokeMode::Erase, PaintTarget::ContentEffect(_)) => "Erase Effect Stroke",
            (_, PaintTarget::MaskBase | PaintTarget::MaskEffect(_)) => "Mask Stroke",
        };
        self.stroke = Some(ActiveStroke { stroke, texture_set: ts.id, label });
        Ok(())
    }

    /// Continue the stroke to a screen position (`line`: straight segment, no lazy mouse).
    pub fn stroke_to(&mut self, camera: &Camera, screen: Vec2, line: bool) -> Result<Vec<usize>, PaintError> {
        let Some(active) = self.stroke.take() else { return Ok(Vec::new()) };
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

    /// Clone tool: use the surface point under `screen` as the source (a new offset is fixed by
    /// the next stroke's first dab). Returns false when the cursor is not over the model.
    pub fn set_clone_source(&mut self, camera: &Camera, screen: Vec2) -> bool {
        let Some(pick) = self.pick(camera, screen) else { return false };
        self.clone_source = Some(pick.position);
        self.clone_offset = None;
        true
    }

    /// The clone source point (object space), if one is set.
    pub fn clone_source(&self) -> Option<Vec3> {
        self.clone_source
    }

    /// Screen offset from the stroke position to the clone source, once a stroke fixed it.
    pub fn clone_offset(&self) -> Option<Vec2> {
        self.clone_offset
    }

    /// Polygon fill on the selected layer at a screen position (one history entry).
    pub fn polygon_fill(&mut self, camera: &Camera, screen: Vec2, mode: PolygonFillMode) -> Result<Vec<usize>, PaintError> {
        if self.stroke.is_some() {
            return Ok(Vec::new());
        }
        self.begin_stroke()?;
        let active = self.stroke.take().expect("just begun");
        let tiles = self.with_stroke_context(active.texture_set, camera, |stroke, ts, ctx| stroke.polygon_fill(ts, ctx, screen, mode), active)?;
        if let Some(a) = self.stroke.as_mut() {
            a.label = "Polygon Fill";
        }
        self.end_stroke();
        Ok(tiles)
    }

    fn with_stroke_context(
        &mut self,
        ts_id: TextureSetId,
        camera: &Camera,
        f: impl FnOnce(&mut Stroke, &mut TextureSet, &PaintContext) -> Result<Vec<usize>, PaintError>,
        mut active: ActiveStroke,
    ) -> Result<Vec<usize>, PaintError> {
        let Some(mut ts) = self.doc.texture_set(ts_id).cloned() else {
            return Err(PaintError::LayerNotFound);
        };
        let geo = self.scene.geometry.get(ts.material_slot, ts.resolution);
        let view = self.viewport_scene();
        let ctx = PaintContext { mesh: &view.mesh, bvh: &view.bvh, geometry: &geo, camera, depth: &view.depth };
        let result = f(&mut active.stroke, &mut ts, &ctx);
        if let Some(slot) = self.doc.texture_set_mut(ts_id) {
            *slot = ts;
        }
        self.stroke = Some(active);
        result
    }

    /// Finish the stroke: the post-stroke document becomes one history entry (none if nothing
    /// was painted).
    pub fn end_stroke(&mut self) {
        let Some(active) = self.stroke.take() else { return };
        if self.doc != *self.history.current() {
            log::info!("action: {}", active.label);
            self.history.push(active.label, self.doc.clone());
            self.revision += 1;
        }
    }

    /// Abandon the stroke and restore the pre-stroke document.
    pub fn cancel_stroke(&mut self) {
        if self.stroke.take().is_some() {
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
    pub fn update_composites_within(&mut self, budget: Option<std::time::Duration>) -> Vec<CompositeUpdate> {
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

    /// Export every map as one in-memory ZIP archive.
    pub fn export_archive(&mut self, settings: &subcraft_export::ExportSettings) -> Result<subcraft_export::TextureArchive,String> {
        self.update_composites();let settings=self.effective_export_settings(settings);let name=self.export_name();let sets=self.export_sets();
        subcraft_export::archive(&self.scene.mesh,&name,&sets,&settings).map_err(|e|e.to_string())
    }

    // ----- assets --------------------------------------------------------------------------------

    /// Add a material preset as a fill layer above the selection.
    pub fn add_material(&mut self, preset: &subcraft_assets::MaterialPreset) -> Result<(), CommandError> {
        let layer = preset.build(self.selected_texture_set());
        self.execute(Command::InsertLayer { layer: Box::new(layer), label: format!("Material: {}", preset.name) })
    }

    /// Add a smart material (folder of masked layers) above the selection.
    pub fn add_smart_material(&mut self, smart: &subcraft_assets::SmartMaterial) -> Result<(), CommandError> {
        for r in smart.resources() {
            self.resources.insert_shared(r.clone());
        }
        let layer = smart.build(self.selected_texture_set());
        self.execute(Command::InsertLayer { layer: Box::new(layer), label: format!("Smart Material: {}", smart.name) })
    }

    /// Apply a smart mask to the selected layer.
    pub fn apply_smart_mask(&mut self, mask: &subcraft_assets::SmartMask) -> Result<(), CommandError> {
        let layer = self.selection.layer.ok_or(CommandError::LayerNotFound)?;
        for r in &mask.resources {
            self.resources.insert_shared(r.clone());
        }
        self.execute(Command::ApplyMaskEffects { layer, effects: mask.effects.clone(), label: format!("Smart Mask: {}", mask.name) })
    }

    /// Use a brush alpha for the next strokes.
    pub fn set_brush_alpha(&mut self, alpha: &subcraft_assets::BrushAlpha) {
        self.brush.alpha = alpha.bitmap.clone();
        self.brush.alpha_name = alpha.name.clone();
    }

    /// Channels present in the selected texture set.
    pub fn channels(&self) -> Vec<ChannelKind> {
        self.selected_texture_set().channel_kinds().collect()
    }
}

/// Two texture sets composite identically (names aside): same tiling, channels and layers.
fn same_composite_inputs(a: &TextureSet, b: &TextureSet) -> bool {
    a.id == b.id
        && a.material_slot == b.material_slot
        && a.resolution == b.resolution
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
