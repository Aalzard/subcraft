//! The editing session: scene (mesh + acceleration structures), document + history, selection,
//! brush state, the active stroke and the composite caches.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use glam::{Vec2, Vec3};
use subcraft_compositor::{Composite, CompositeCache, EvalContext};
use subcraft_core::{ChannelKind, LayerId, TextureSetId};
use subcraft_doc::{ChannelDef, Document, History, Layer, LayerContent, LayerLocation, Mask, MaskEffect, ResourceStore, TextureSet};
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
    pub mesh: Arc<Mesh>,
    pub bvh: Bvh,
    pub geometry: GeometryCache,
    pub depth: DepthCache,
}

impl Scene {
    pub fn new(mesh: Mesh) -> Self {
        let mesh = Arc::new(mesh);
        Self { bvh: Bvh::build(&mesh), geometry: GeometryCache::new(mesh.clone()), depth: DepthCache::new(), mesh }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub texture_set: TextureSetId,
    pub layer: Option<LayerId>,
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
    pub tiles: Vec<usize>,
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
    pub brush: BrushState,
    stroke: Option<ActiveStroke>,
    composites: HashMap<TextureSetId, CompositeCache>,
    /// Bumped on every document change (UI caches key off it).
    revision: u64,
    layer_counter: u32,
    project_path: Option<PathBuf>,
    /// Document as of the last save / load / new (for the unsaved-changes indicator).
    saved_doc: Document,
    /// Mesh maps changed (baked) since the last save.
    maps_dirty: bool,
    /// Bumped whenever baked mesh maps change (autosave change tracking).
    maps_generation: u64,
    /// Opened from a recovery copy: unsaved until the user saves.
    recovered: bool,
}

/// Everything a project file holds, captured cheaply (shared tiles and resources) so it can be
/// written on a background thread while editing continues.
pub struct ProjectSnapshot {
    pub mesh: Arc<Mesh>,
    pub document: Document,
    pub resources: ResourceStore,
    pub mesh_maps: subcraft_project::MeshMapSet,
}

impl ProjectSnapshot {
    pub fn save(&self, path: &Path) -> Result<(), String> {
        subcraft_project::save_project(path, &self.mesh, &self.document, &self.resources, &self.mesh_maps).map_err(|e| e.to_string())
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
            doc,
            selection,
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
            composites: HashMap::new(),
            revision: 0,
            layer_counter,
            project_path: None,
            maps_dirty: false,
            maps_generation: 0,
            recovered: false,
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
        subcraft_project::save_project(&path, &self.scene.mesh, &self.doc, &self.resources, &maps).map_err(|e| e.to_string())?;
        self.project_path = Some(path.clone());
        self.saved_doc = self.doc.clone();
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

    #[cfg(not(target_arch = "wasm32"))]
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
        self.maps_dirty || self.recovered || self.doc != self.saved_doc
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
        if let Some(c) = self.composites.get_mut(&ts) {
            c.invalidate();
        }
        self.maps_dirty = true;
        self.maps_generation += 1;
        self.revision += 1;
    }

    /// Start baking on a background thread; poll the job each frame.
    pub fn start_bake(&self, request: crate::bake_job::BakeRequest) -> crate::bake_job::BakeJob {
        crate::bake_job::BakeJob::start(self.scene.mesh.clone(), Arc::new(self.scene.bvh.clone()), request)
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
        Ok((Self::new(report.mesh, resolution), report.warnings))
    }

    // ----- queries -------------------------------------------------------------------------------

    pub fn scene(&self) -> &Scene {
        &self.scene
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

    pub fn composite(&self, ts: TextureSetId) -> Option<&Composite> {
        self.composites.get(&ts).and_then(|c| c.composite())
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
                self.revision += 1;
            }
            Command::SelectLayer(layer) => {
                if let Some(l) = layer {
                    self.selection.texture_set = self.texture_set_of(l).ok_or(CommandError::LayerNotFound)?;
                }
                self.selection.layer = layer;
                self.revision += 1;
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
                self.edit_layer_set(layer, "Add Mask Effect", None, |ts| {
                    let mask = Self::layer_mut(ts, layer)?.mask.as_mut().ok_or(CommandError::NoMask)?;
                    mask.effects.push(Arc::new(MaskEffect::new(kind)));
                    Ok(())
                })?;
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
                let key = format!("effect:{layer}:{}", effect.id);
                self.edit_layer_set(layer, "Mask Effect", Some(key), |ts| {
                    let mask = Self::layer_mut(ts, layer)?.mask.as_mut().ok_or(CommandError::NoMask)?;
                    let slot = mask.effects.iter_mut().find(|e| e.id == effect.id).ok_or(CommandError::Invalid("no such effect".into()))?;
                    *slot = Arc::new(effect);
                    Ok(())
                })?;
            }
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
        let stroke = Stroke::begin(ts, layer, b.target, b.mode, b.settings, b.options, &b.material)?
            .with_alpha(b.alpha.clone())
            .with_stencil(if b.stencil_bypass { None } else { b.stencil.clone() });
        let label = match (b.mode, b.target) {
            (StrokeMode::Paint, PaintTarget::Content) => "Paint Stroke",
            (StrokeMode::Erase, PaintTarget::Content) => "Erase Stroke",
            (_, PaintTarget::MaskBase) => "Mask Stroke",
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
        result
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

    /// Bring every texture set's composite up to date; returns the re-evaluated tiles.
    pub fn update_composites(&mut self) -> Vec<CompositeUpdate> {
        let mut out = Vec::new();
        for ts in &self.doc.texture_sets {
            let geo = self.scene.geometry.get(ts.material_slot, ts.resolution);
            let maps = self.mesh_maps.get(&ts.id).unwrap_or(&self.no_maps);
            let ctx = EvalContext { geometry: &geo, resources: &self.resources, mesh_maps: maps };
            let tiles = self.composites.entry(ts.id).or_default().update(ts, &ctx);
            if !tiles.is_empty() {
                out.push(CompositeUpdate { texture_set: ts.id, material_slot: ts.material_slot, tiles });
            }
        }
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

    /// Files an export with `settings` would write.
    pub fn export_plan(&mut self, settings: &subcraft_export::ExportSettings) -> Vec<(String, String)> {
        self.update_composites();
        let name = self.export_name();
        let sets = self.export_sets();
        subcraft_export::plan(&name, &sets, settings)
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
                    channels: &c.channels,
                    mesh_maps: self.mesh_maps.get(&ts.id).map(|m| m.as_map()),
                })
            })
            .collect()
    }

    /// Export every texture set's composite through `settings` into `dir`.
    pub fn export_textures(&mut self, dir: &Path, settings: &subcraft_export::ExportSettings) -> Result<Vec<PathBuf>, String> {
        self.update_composites();
        let name = self.export_name();
        let sets = self.export_sets();
        subcraft_export::export(&self.scene.mesh, &name, &sets, settings, dir).map_err(|e| e.to_string())
    }

    /// Export every map in one download without sending individual writes to the web sink.
    pub fn export_archive(&mut self, settings: &subcraft_export::ExportSettings) -> Result<subcraft_export::TextureArchive,String> {
        self.update_composites();let name=self.export_name();let sets=self.export_sets();
        subcraft_export::archive(&self.scene.mesh,&name,&sets,settings).map_err(|e|e.to_string())
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
