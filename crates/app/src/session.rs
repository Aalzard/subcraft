//! The editing session: scene (mesh + acceleration structures), document + history, selection,
//! brush state, the active stroke and the composite caches.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use glam::{Vec2, Vec3};
use subcraft_compositor::{Composite, CompositeCache, EvalContext};
use subcraft_core::{ChannelKind, LayerId, TextureSetId};
use subcraft_doc::{Document, History, Layer, LayerContent, LayerLocation, Mask, MaskEffect, ResourceStore, TextureSet};
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
    pub settings: BrushSettings,
    pub options: StrokeOptions,
    pub material: BrushMaterial,
    pub mode: StrokeMode,
    pub target: PaintTarget,
}

/// A surface point under the cursor.
#[derive(Clone, Copy, Debug)]
pub struct Pick {
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
}

impl Session {
    /// One texture set per material slot, each with an empty paint layer selected.
    pub fn new(mesh: Mesh, resolution: u32) -> Self {
        let names: Vec<String> = mesh.materials.iter().map(|m| m.name.clone()).collect();
        let mut doc = Document::from_material_slots(&names, resolution);
        let mut first_layer = None;
        for ts in doc.texture_sets.iter_mut() {
            let tc = ts.clone();
            let id = ts.insert_above(None, Layer::new_paint("Layer 1", &tc));
            first_layer.get_or_insert(id);
        }
        let selection = Selection { texture_set: doc.texture_sets[0].id, layer: first_layer };
        let material = BrushMaterial::new(&doc.texture_sets[0]);
        Self {
            scene: Scene::new(mesh),
            resources: ResourceStore::default(),
            mesh_maps: HashMap::new(),
            no_maps: MeshMaps::new(),
            history: History::new(doc.clone(), "New Project"),
            doc,
            selection,
            brush: BrushState {
                settings: BrushSettings::default(),
                options: StrokeOptions::default(),
                material,
                mode: StrokeMode::Paint,
                target: PaintTarget::Content,
            },
            stroke: None,
            composites: HashMap::new(),
            revision: 0,
            layer_counter: 1,
        }
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
        let hit = self.scene.bvh.intersect(o, d, 0.0, f32::INFINITY)?;
        let sp = self.scene.mesh.interpolate(hit.tri as usize, hit.u, hit.v);
        Some(Pick {
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

    // ----- commands ------------------------------------------------------------------------------

    fn commit(&mut self, label: &str, merge_key: Option<String>) {
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
            Action::ResetCamera => return Ok(false),
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
        let stroke = Stroke::begin(ts, layer, b.target, b.mode, b.settings, b.options, &b.material)?;
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
        let ctx = PaintContext { mesh: &self.scene.mesh, bvh: &self.scene.bvh, geometry: &geo, camera, depth: &self.scene.depth };
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

    /// Channels present in the selected texture set.
    pub fn channels(&self) -> Vec<ChannelKind> {
        self.selected_texture_set().channel_kinds().collect()
    }
}
