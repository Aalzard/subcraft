//! Graph layers: fill layers rendered by a material graph (`.sbsar` through open-sbsar, `.mcmat`/`.mcraft`/`.sbs` through MaterialCraft),
//! with the graph's parameters as controls (see `subcraft_doc::graph_material`).
//!
//! [`GraphLayers`] is owned by the caller (the UI keeps it next to the session) and polled once
//! a frame. Rendering runs on worker threads, one at a time per layer: a change made while a
//! render is running waits and only the newest waiting request is rendered. A finished render
//! inserts its images as resources and swaps them into the layer with one undo step.
//!
//! Engines plug in through [`GraphBackend`]; [`open_backend`] picks one by [`GraphEngine`].

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};

use subcraft_core::{ChannelKind, LayerId, MeshMapKind, ResourceId, TextureSetId};
use subcraft_doc::{FillChannel, FillSource, GraphEngine, GraphMaterial, GraphParam, Layer, LayerContent, Resource, ResourceKind};
use subcraft_image::{Bitmap, StorageToken};
use subcraft_materials::MeshMaps;

use crate::{Command, Session};

/// Largest package file read.
pub const MAX_PACKAGE_BYTES: u64 = 512 << 20;

/// One graph of a package, as the controls need it.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphDesc {
    /// The graph's identity inside the package (what [`GraphMaterial::graph`] stores).
    pub graph: String,
    pub label: String,
    pub params: Vec<GraphParam>,
    /// Channels it fills.
    pub channels: Vec<ChannelKind>,
    /// Things the user should know (unconnected image inputs, ...).
    pub notes: Vec<String>,
    /// Baked mesh maps the graph reads; a new bake of one re-renders its layers.
    pub mesh_inputs: Vec<MeshMapKind>,
}

/// A rendered channel image. Keeps encoded values: Base Color and Emissive sRGB-encoded (like
/// an imported PNG), data channels raw, normals OpenGL-style.
#[derive(Clone, Debug)]
pub struct RenderedMap {
    pub channel: ChannelKind,
    pub bitmap: Bitmap,
}

/// An opened package of one engine.
pub trait GraphBackend: Send {
    fn graphs(&self) -> &[GraphDesc];
    /// Render `graph` (empty: the first) with `values` at `2^size_log2` pixels.
    fn render(&mut self, graph: &str, values: &BTreeMap<String, Vec<f64>>, size_log2: u32) -> Result<Vec<RenderedMap>, String>;
    /// [`Self::render`] with the texture set's baked mesh maps for the graph's image inputs
    /// (curvature, ambient occlusion, ...). Engines without image inputs ignore them.
    fn render_with_maps(&mut self, graph: &str, values: &BTreeMap<String, Vec<f64>>, size_log2: u32, maps: &MeshMaps) -> Result<Vec<RenderedMap>, String> {
        let _ = maps;
        self.render(graph, values, size_log2)
    }
}

/// The engine that opens a file, by extension.
pub fn engine_for_file(name: &str) -> Option<GraphEngine> {
    let ext = Path::new(name).extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "sbsar" => Some(GraphEngine::Sbsar),
        "mcmat" | "mcraft" | "sbs" => Some(GraphEngine::MaterialCraft),
        _ => None,
    }
}

/// File extensions graph layers are made from (for file dialogs and drops).
pub const GRAPH_EXTENSIONS: &[&str] = &["sbsar", "mcmat", "mcraft", "sbs"];

/// Open `bytes` (a file named `name`) with `engine`.
pub fn open_backend(engine: GraphEngine, bytes: &[u8], name: &str) -> Result<Box<dyn GraphBackend>, String> {
    match engine {
        GraphEngine::Sbsar => Ok(Box::new(SbsarBackend::open(bytes)?)),
        GraphEngine::MaterialCraft => Ok(Box::new(crate::materialcraft::MaterialCraftBackend::open(bytes, name)?)),
    }
}

struct SbsarBackend {
    package: subcraft_sbsar::Package,
    graphs: Vec<GraphDesc>,
    renderers: HashMap<String, subcraft_sbsar::GraphRenderer>,
}

impl SbsarBackend {
    fn open(bytes: &[u8]) -> Result<Self, String> {
        let package = subcraft_sbsar::inspect(bytes).map_err(|e| e.to_string())?;
        let graphs = package
            .graphs()
            .iter()
            .filter(|g| !g.channels.is_empty())
            .map(|g| GraphDesc {
                graph: g.path.clone(),
                label: g.label.clone(),
                params: g.params.clone(),
                channels: g.channels.clone(),
                notes: if g.image_inputs.is_empty() { Vec::new() } else { vec![format!("image inputs not connected yet: {}", g.image_inputs.join(", "))] },
                mesh_inputs: Vec::new(),
            })
            .collect();
        Ok(SbsarBackend { package, graphs, renderers: HashMap::new() })
    }
}

impl GraphBackend for SbsarBackend {
    fn graphs(&self) -> &[GraphDesc] {
        &self.graphs
    }

    fn render(&mut self, graph: &str, values: &BTreeMap<String, Vec<f64>>, size_log2: u32) -> Result<Vec<RenderedMap>, String> {
        let path = self.package.graph(graph).path.clone();
        let package = self.package.clone();
        let r = self.renderers.entry(path.clone()).or_insert_with(|| subcraft_sbsar::GraphRenderer::new(package, &path));
        let out = r.render(values, size_log2).map_err(|e| e.to_string())?;
        for w in &out.warnings {
            log::info!("sbsar {path}: {w}");
        }
        Ok(out.maps.into_iter().map(|m| RenderedMap { channel: m.channel, bitmap: m.bitmap }).collect())
    }
}

type SharedBackend = Arc<Mutex<Box<dyn GraphBackend>>>;

/// What a running render will do with its images.
#[derive(Clone, Debug)]
enum Target {
    /// Add a new layer to this texture set.
    New { texture_set: TextureSetId, name: String },
    /// Swap the images of this layer.
    Update { layer: LayerId },
}

struct Running {
    target: Target,
    material: GraphMaterial,
    /// Started by a new bake, not by the user (names the undo step).
    refresh: bool,
    rx: Receiver<Result<Vec<RenderedMap>, String>>,
}

/// What [`GraphLayers::poll`] reports.
#[derive(Clone, Debug, PartialEq)]
pub enum GraphEvent {
    /// A layer was added or updated (status text).
    Applied(String),
    Failed(String),
}

/// Background renders of graph layers and the packages they use.
#[derive(Default)]
pub struct GraphLayers {
    backends: HashMap<ResourceId, Result<SharedBackend, String>>,
    descs: HashMap<ResourceId, Arc<Vec<GraphDesc>>>,
    running: Vec<Running>,
    /// Newest request per layer that waits for that layer's running render.
    queued: HashMap<LayerId, GraphMaterial>,
    /// Baked maps per texture set at the last [`Self::refresh_baked`], and the session's
    /// maps generation then.
    baked: HashMap<TextureSetId, BTreeMap<MeshMapKind, StorageToken>>,
    baked_generation: Option<u64>,
}

impl GraphLayers {
    pub fn new() -> Self {
        Self::default()
    }

    /// The opened package behind `package` (a [`ResourceKind::GraphPackage`] resource).
    fn backend(&mut self, session: &Session, engine: GraphEngine, package: ResourceId) -> Result<SharedBackend, String> {
        if let Some(b) = self.backends.get(&package) {
            return b.clone();
        }
        let opened = (|| {
            let r = session.resources().get(package).ok_or("the material's package is missing from the project")?;
            let bytes = r.encoded.as_deref().ok_or("the material's package has no file data")?;
            let backend = open_backend(engine, bytes, &r.name)?;
            self.descs.insert(package, Arc::new(backend.graphs().to_vec()));
            Ok::<_, String>(Arc::new(Mutex::new(backend)))
        })();
        self.backends.insert(package, opened.clone());
        opened
    }

    /// The graphs of the package a layer uses (for its controls).
    pub fn graphs(&mut self, session: &Session, material: &GraphMaterial) -> Result<Arc<Vec<GraphDesc>>, String> {
        if let Some(d) = self.descs.get(&material.package) {
            return Ok(d.clone());
        }
        self.backend(session, material.engine, material.package)?;
        self.descs.get(&material.package).cloned().ok_or_else(|| "the package has no graph".to_string())
    }

    /// The graph `material` names, from [`Self::graphs`].
    pub fn graph(&mut self, session: &Session, material: &GraphMaterial) -> Result<GraphDesc, String> {
        let gs = self.graphs(session, material)?;
        gs.iter().find(|g| g.graph == material.graph).or_else(|| gs.first()).cloned().ok_or_else(|| "the package has no usable graph".to_string())
    }

    /// The values a layer's controls should show: a waiting or running request's, else the
    /// layer's own.
    pub fn shown_material(&self, layer: &Layer) -> Option<GraphMaterial> {
        if let Some(m) = self.queued.get(&layer.id) {
            return Some(m.clone());
        }
        if let Some(r) = self.running.iter().rev().find(|r| matches!(r.target, Target::Update { layer: l } if l == layer.id)) {
            return Some(r.material.clone());
        }
        layer.graph.as_deref().cloned()
    }

    /// True while a render for `layer` runs or waits.
    pub fn is_rendering(&self, layer: LayerId) -> bool {
        self.queued.contains_key(&layer) || self.running.iter().any(|r| matches!(r.target, Target::Update { layer: l } if l == layer))
    }

    /// True while any render runs (call [`Self::poll`] every frame while it does).
    pub fn busy(&self) -> bool {
        !self.running.is_empty() || !self.queued.is_empty()
    }

    /// Import a package file as a new layer on the selected texture set. The layer appears when
    /// its first render is done.
    pub fn import_file(&mut self, session: &mut Session, path: &Path) -> Result<String, String> {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("material").to_string();
        let engine = engine_for_file(&name).ok_or_else(|| format!("{name}: not a material graph SubCraft can open"))?;
        let len = std::fs::metadata(path).map_err(|e| format!("{name}: {e}"))?.len();
        if len > MAX_PACKAGE_BYTES {
            return Err(format!("{name}: {} MB is larger than the {} MB limit", len >> 20, MAX_PACKAGE_BYTES >> 20));
        }
        let bytes = std::fs::read(path).map_err(|e| format!("{name}: {e}"))?;
        self.import_bytes(session, engine, bytes, &name)
    }

    /// [`Self::import_file`] for bytes already read.
    pub fn import_bytes(&mut self, session: &mut Session, engine: GraphEngine, bytes: Vec<u8>, name: &str) -> Result<String, String> {
        let backend = open_backend(engine, &bytes, name).map_err(|e| format!("{name}: {e}"))?;
        let graph = backend.graphs().first().cloned().ok_or_else(|| format!("{name}: no graph has an output SubCraft can use"))?;
        // The same file again shares one package resource (saved once).
        let existing = session.resources().iter().find(|r| r.kind == ResourceKind::GraphPackage && r.encoded.as_deref().is_some_and(|e| *e == bytes)).map(|r| r.id);
        let id = existing.unwrap_or_else(|| session.add_resource(Resource {
            id: ResourceId::new(),
            name: name.to_string(),
            kind: ResourceKind::GraphPackage,
            bitmap: Arc::new(Bitmap::new_f32(1, 1, 1, vec![0.0])),
            source_path: None,
            encoded: Some(Arc::new(bytes)),
        }));
        self.descs.insert(id, Arc::new(backend.graphs().to_vec()));
        let shared: SharedBackend = Arc::new(Mutex::new(backend));
        self.backends.insert(id, Ok(shared.clone()));
        let material = GraphMaterial::new(engine, id, graph.graph.clone(), subcraft_sbsar::DEFAULT_SIZE_LOG2);
        let texture_set = session.selection().texture_set;
        let maps = session.mesh_maps(texture_set).cloned().unwrap_or_default();
        self.spawn(shared, Target::New { texture_set, name: graph.label.clone() }, material, maps, false);
        let mut msg = format!("Rendering {} ({})…", graph.label, engine.label());
        for n in &graph.notes {
            msg.push_str(&format!(" Note: {n}."));
        }
        Ok(msg)
    }

    /// Render `layer` again with `material` (new values or size). The layer changes when the
    /// render is done.
    pub fn request(&mut self, session: &Session, layer: LayerId, material: GraphMaterial) -> Result<(), String> {
        self.request_as(session, layer, material, false)
    }

    fn request_as(&mut self, session: &Session, layer: LayerId, material: GraphMaterial, refresh: bool) -> Result<(), String> {
        if self.running.iter().any(|r| matches!(r.target, Target::Update { layer: l } if l == layer)) {
            self.queued.insert(layer, material);
            return Ok(());
        }
        let backend = self.backend(session, material.engine, material.package)?;
        let maps = find_layer(session, layer).and_then(|(ts, _)| session.mesh_maps(ts).cloned()).unwrap_or_default();
        self.spawn(backend, Target::Update { layer }, material, maps, refresh);
        Ok(())
    }

    /// Re-render the graph layers whose baked inputs changed since the last call (a new bake,
    /// an imported map), with each layer's current values. Call it when no bake is running, so
    /// a bake that installs its maps one by one renders each layer once. The first call only
    /// takes note of the maps. Returns how many layers started rendering.
    pub fn refresh_baked(&mut self, session: &Session) -> Result<usize, String> {
        let generation = session.maps_generation();
        if self.baked_generation == Some(generation) {
            return Ok(0);
        }
        self.baked_generation = Some(generation);
        let mut stale: Vec<(LayerId, GraphMaterial, Vec<MeshMapKind>)> = Vec::new();
        let mut now_baked = HashMap::new();
        for ts in &session.document().texture_sets {
            let maps = session.mesh_maps(ts.id);
            let now: BTreeMap<MeshMapKind, StorageToken> = maps.map(|m| m.as_map().iter().map(|(k, image)| (*k, image.storage_token())).collect()).unwrap_or_default();
            // A texture set seen for the first time has nothing to compare with.
            if let Some(before) = self.baked.get(&ts.id) {
                let image = |k: &MeshMapKind| maps.and_then(|m| m.get(*k));
                let mut changed: Vec<MeshMapKind> = before.iter().filter(|(k, token)| !image(k).is_some_and(|i| token.matches(i))).map(|(k, _)| *k).collect();
                changed.extend(now.keys().filter(|k| !before.contains_key(k)));
                if !changed.is_empty() {
                    ts.walk(|layer, _| {
                        if let Some(material) = layer.graph.as_deref() {
                            stale.push((layer.id, self.shown_material(layer).unwrap_or_else(|| material.clone()), changed.clone()));
                        }
                    });
                }
            }
            now_baked.insert(ts.id, now);
        }
        self.baked = now_baked;
        let mut started = 0;
        let mut errors = Vec::new();
        for (layer, material, changed) in stale {
            let reads = match self.graph(session, &material) {
                Ok(desc) => desc.mesh_inputs.iter().any(|k| changed.contains(k)),
                Err(e) => {
                    errors.push(e);
                    continue;
                }
            };
            if reads {
                match self.request_as(session, layer, material, true) {
                    Ok(()) => started += 1,
                    Err(e) => errors.push(e),
                }
            }
        }
        match errors.into_iter().next() {
            Some(e) if started == 0 => Err(e),
            _ => Ok(started),
        }
    }

    fn spawn(&mut self, backend: SharedBackend, target: Target, material: GraphMaterial, maps: MeshMaps, refresh: bool) {
        let (tx, rx) = channel();
        let m = material.clone();
        let spawned = std::thread::Builder::new().name("graph-render".into()).spawn(move || {
            let result = match backend.lock() {
                Ok(mut b) => {
                    let graph = m.graph.clone();
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| b.render_with_maps(&graph, &m.values, m.size_log2, &maps))).unwrap_or_else(|_| Err("the renderer crashed on this material".into()))
                }
                Err(_) => Err("the renderer crashed earlier on this material".into()),
            };
            let _ = tx.send(result);
        });
        if let Err(e) = spawned {
            let (tx, rx) = channel();
            let _ = tx.send(Err(format!("could not start the render: {e}")));
            self.running.push(Running { target, material, refresh, rx });
            return;
        }
        self.running.push(Running { target, material, refresh, rx });
    }

    /// Apply finished renders and start waiting ones (call once a frame).
    pub fn poll(&mut self, session: &mut Session) -> Vec<GraphEvent> {
        let mut events = Vec::new();
        let mut i = 0;
        while i < self.running.len() {
            let result = match self.running[i].rx.try_recv() {
                Ok(r) => r,
                Err(TryRecvError::Empty) => {
                    i += 1;
                    continue;
                }
                Err(TryRecvError::Disconnected) => Err("the render stopped without a result".into()),
            };
            let done = self.running.remove(i);
            events.push(match result.and_then(|maps| apply(session, &done.target, &done.material, done.refresh, maps)) {
                Ok(text) => GraphEvent::Applied(text),
                Err(e) => GraphEvent::Failed(e),
            });
            if let Target::Update { layer } = done.target
                && let Some(next) = self.queued.remove(&layer)
                && let Err(e) = self.request(session, layer, next)
            {
                events.push(GraphEvent::Failed(e));
            }
        }
        events
    }

    /// Wait for every render and apply it (tests, scripts).
    pub fn finish(&mut self, session: &mut Session) -> Vec<GraphEvent> {
        let mut events = Vec::new();
        while self.busy() {
            events.extend(self.poll(session));
            if self.busy() {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        events
    }
}

/// Channels a graph may bring that the texture set might not have yet; they are added with the
/// layer (like an imported texture material's).
const ADDED_WITH_LAYER: [ChannelKind; 3] = [ChannelKind::AmbientOcclusion, ChannelKind::Emissive, ChannelKind::Opacity];

fn find_layer(session: &Session, id: LayerId) -> Option<(TextureSetId, Layer)> {
    session.document().texture_sets.iter().find_map(|ts| ts.find(id).map(|l| (ts.id, l.clone())))
}

/// Put `maps` into `layer`'s fill channels; channels without a map are switched off.
fn fill_with(session: &mut Session, layer: &mut Layer, material: &GraphMaterial, maps: Vec<RenderedMap>) {
    let ids = add_maps(session, material, maps);
    fill_with_ids(layer, material, &ids);
}

/// The rendered maps as texture resources of the project, by channel.
fn add_maps(session: &mut Session, material: &GraphMaterial, maps: Vec<RenderedMap>) -> BTreeMap<ChannelKind, ResourceId> {
    let name = session.resources().get(material.package).map(|r| r.name.clone()).unwrap_or_default();
    let mut ids: BTreeMap<ChannelKind, ResourceId> = BTreeMap::new();
    for m in maps {
        let id = session.add_resource(Resource {
            id: ResourceId::new(),
            name: format!("{name} {}", m.channel.display_name()),
            kind: ResourceKind::Texture,
            bitmap: Arc::new(m.bitmap),
            source_path: None,
            encoded: None,
        });
        ids.insert(m.channel, id);
    }
    ids
}

/// Point `layer`'s fill channels at the rendered `ids`; channels without a map are switched off.
fn fill_with_ids(layer: &mut Layer, material: &GraphMaterial, ids: &BTreeMap<ChannelKind, ResourceId>) {
    if let LayerContent::Fill(f) = &mut layer.content {
        for (kind, c) in f.channels.iter_mut() {
            match ids.get(kind) {
                Some(&resource) => {
                    // Keep the user's strength / adjustments on an update.
                    let keep = matches!(c.source, FillSource::Bitmap { .. }).then_some((c.strength, c.adjust));
                    *c = FillChannel::new(FillSource::Bitmap { resource });
                    if let Some((strength, adjust)) = keep {
                        c.strength = strength;
                        c.adjust = adjust;
                    }
                }
                None => c.enabled = false,
            }
        }
    }
    layer.graph = Some(Arc::new(material.clone()));
}

fn apply(session: &mut Session, target: &Target, material: &GraphMaterial, refresh: bool, maps: Vec<RenderedMap>) -> Result<String, String> {
    if maps.is_empty() {
        return Err("the render produced no image".into());
    }
    match target {
        Target::New { texture_set, name } => {
            let ts = session.document().texture_set(*texture_set).ok_or("the texture set is gone")?.clone();
            let mut wide = ts.clone();
            for m in &maps {
                if !wide.has_channel(m.channel) && ADDED_WITH_LAYER.contains(&m.channel) {
                    wide.channels.push(subcraft_doc::ChannelDef::default_for(m.channel));
                }
            }
            let missing: Vec<ChannelKind> = wide.channels.iter().map(|c| c.kind).filter(|k| !ts.has_channel(*k)).collect();
            let mut layer = Layer::new_fill(name.clone(), &wide);
            fill_with(session, &mut layer, material, maps);
            let label = format!("Graph Material: {name}");
            if session.selection().texture_set != *texture_set {
                return Err(format!("{name}: the texture set changed while rendering; import it again"));
            }
            let cmd = if missing.is_empty() { Command::InsertLayer { layer: Box::new(layer), label } } else { Command::InsertLayerWithChannels { layer: Box::new(layer), channels: missing, label } };
            session.execute(cmd).map_err(|e| e.to_string())?;
            Ok(format!("Added {name}"))
        }
        Target::Update { layer } => {
            let (ts_id, mut l) = find_layer(session, *layer).ok_or("the layer was deleted while rendering")?;
            if !matches!(l.content, LayerContent::Fill(_)) {
                return Err("the layer is no longer a fill layer".into());
            }
            let ids = add_maps(session, material, maps);
            fill_with_ids(&mut l, material, &ids);
            let label = if refresh { format!("Refresh After Bake: {}", l.name) } else { format!("Material Parameters: {}", l.name) };
            let name = l.name.clone();
            // Paint across UDIM tiles (issue #581): the graph layer was added on every tile of
            // the group, so the new parameters drive every tile's copy (the maps are UV-free).
            // A refresh after a bake re-renders each tile's copy with that tile's own maps.
            let doc = session.document();
            let partners: Vec<Command> = doc
                .texture_set(ts_id)
                .filter(|_| !refresh)
                .map(|source| {
                    doc.udim_paint_partners(ts_id)
                        .into_iter()
                        .filter_map(|id| {
                            let ts = doc.texture_set(id)?;
                            let mut other = ts.find(ts.corresponding_layer(source, *layer)?)?.clone();
                            if !matches!(other.content, LayerContent::Fill(_)) || other.graph.as_ref().is_none_or(|g| g.package != material.package) {
                                return None;
                            }
                            fill_with_ids(&mut other, material, &ids);
                            Some(Command::ReplaceLayer { layer: other.id, replacement: Box::new(other), label: label.clone() })
                        })
                        .collect()
                })
                .unwrap_or_default();
            session.execute_with_partner_commands(Command::ReplaceLayer { layer: *layer, replacement: Box::new(l), label }, partners).map_err(|e| e.to_string())?;
            Ok(if refresh { format!("Updated {name} for the new bake") } else { format!("Updated {name}") })
        }
    }
}
