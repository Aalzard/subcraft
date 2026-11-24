//! Graph layers: fill layers rendered by a material graph (`.sbsar` today, MaterialCraft next),
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

use subcraft_core::{ChannelKind, LayerId, ResourceId, TextureSetId};
use subcraft_doc::{FillChannel, FillSource, GraphEngine, GraphMaterial, GraphParam, Layer, LayerContent, Resource, ResourceKind};
use subcraft_image::Bitmap;

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
        self.spawn(shared, Target::New { texture_set, name: graph.label.clone() }, material);
        let mut msg = format!("Rendering {} ({})…", graph.label, engine.label());
        for n in &graph.notes {
            msg.push_str(&format!(" Note: {n}."));
        }
        Ok(msg)
    }

    /// Render `layer` again with `material` (new values or size). The layer changes when the
    /// render is done.
    pub fn request(&mut self, session: &Session, layer: LayerId, material: GraphMaterial) -> Result<(), String> {
        if self.running.iter().any(|r| matches!(r.target, Target::Update { layer: l } if l == layer)) {
            self.queued.insert(layer, material);
            return Ok(());
        }
        let backend = self.backend(session, material.engine, material.package)?;
        self.spawn(backend, Target::Update { layer }, material);
        Ok(())
    }

    fn spawn(&mut self, backend: SharedBackend, target: Target, material: GraphMaterial) {
        let (tx, rx) = channel();
        let m = material.clone();
        let spawned = std::thread::Builder::new().name("graph-render".into()).spawn(move || {
            let result = match backend.lock() {
                Ok(mut b) => {
                    let graph = m.graph.clone();
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| b.render(&graph, &m.values, m.size_log2))).unwrap_or_else(|_| Err("the renderer crashed on this material".into()))
                }
                Err(_) => Err("the renderer crashed earlier on this material".into()),
            };
            let _ = tx.send(result);
        });
        if let Err(e) = spawned {
            let (tx, rx) = channel();
            let _ = tx.send(Err(format!("could not start the render: {e}")));
            self.running.push(Running { target, material, rx });
            return;
        }
        self.running.push(Running { target, material, rx });
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
            events.push(match result.and_then(|maps| apply(session, &done.target, &done.material, maps)) {
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

fn apply(session: &mut Session, target: &Target, material: &GraphMaterial, maps: Vec<RenderedMap>) -> Result<String, String> {
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
            let (_, mut l) = find_layer(session, *layer).ok_or("the layer was deleted while rendering")?;
            if !matches!(l.content, LayerContent::Fill(_)) {
                return Err("the layer is no longer a fill layer".into());
            }
            fill_with(session, &mut l, material, maps);
            let label = format!("Material Parameters: {}", l.name);
            let name = l.name.clone();
            session.execute(Command::ReplaceLayer { layer: *layer, replacement: Box::new(l), label }).map_err(|e| e.to_string())?;
            Ok(format!("Updated {name}"))
        }
    }
}
