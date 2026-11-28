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

use subcraft_core::{ChannelKind, EffectId, LayerId, MeshMapKind, ResourceId, TextureSetId};
use subcraft_doc::{ContentEffect, ContentEffectKind, FillChannel, FillSource, GraphEngine, GraphMaterial, GraphParam, Layer, LayerContent, MaskEffect, MaskEffectKind, Projection, Resource, ResourceKind};
use subcraft_image::{Bitmap, StorageToken, TiledImage};
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
    /// The graph has a grayscale output a mask generator can use (Substance generators).
    pub mask: bool,
    /// Image inputs that read a channel of the layer stack below (Substance filters), by
    /// identifier. A graph with any is a filter: it goes into a layer's content.
    pub filter_inputs: Vec<(String, ChannelKind)>,
}

impl GraphDesc {
    /// A Substance filter: material channels made from the channels of the stack below.
    pub fn is_filter(&self) -> bool {
        !self.filter_inputs.is_empty()
    }
}

/// What a graph renders into: a graph layer's fill channels, or a Substance generator's mask
/// image (an image mask effect with a graph).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GraphTarget {
    Layer(LayerId),
    Mask { layer: LayerId, effect: EffectId },
    /// A Substance filter: a fill content effect whose channels the graph renders from the
    /// layer stack below it.
    Content { layer: LayerId, effect: EffectId },
}

impl GraphTarget {
    pub fn layer(self) -> LayerId {
        match self {
            GraphTarget::Layer(l) | GraphTarget::Mask { layer: l, .. } | GraphTarget::Content { layer: l, .. } => l,
        }
    }
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
    /// Render the graph's mask output as a one-channel bitmap (Substance generators).
    fn render_mask(&mut self, graph: &str, values: &BTreeMap<String, Vec<f64>>, size_log2: u32, maps: &MeshMaps) -> Result<Bitmap, String> {
        let _ = (graph, values, size_log2, maps);
        Err("this kind of material graph cannot be a generator".into())
    }
    /// Render a filter graph with the stack below (`below`, linear channels as the compositor
    /// keeps them) for its channel inputs.
    fn render_filter(&mut self, graph: &str, values: &BTreeMap<String, Vec<f64>>, size_log2: u32, maps: &MeshMaps, below: &BTreeMap<ChannelKind, TiledImage>) -> Result<Vec<RenderedMap>, String> {
        let _ = (graph, values, size_log2, maps, below);
        Err("this kind of material graph cannot be a filter".into())
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

/// True for a Substance archive whose graphs only offer a mask output (a generator, not a
/// material): it belongs in a layer's mask. Unreadable files are not generators.
pub fn is_generator_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    if engine_for_file(name) != Some(GraphEngine::Sbsar) || std::fs::metadata(path).map_or(true, |m| m.len() > MAX_PACKAGE_BYTES) {
        return false;
    }
    std::fs::read(path).ok().and_then(|b| subcraft_sbsar::inspect(&b).ok()).is_some_and(|p| {
        p.graphs().iter().all(|g| g.channels.is_empty()) && p.graphs().iter().any(|g| g.mask_output.is_some())
    })
}

/// True for a Substance archive with a filter graph (image inputs named after the channels it
/// outputs): it belongs on top of a layer's content. Unreadable files are not filters.
pub fn is_filter_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    if engine_for_file(name) != Some(GraphEngine::Sbsar) || std::fs::metadata(path).map_or(true, |m| m.len() > MAX_PACKAGE_BYTES) {
        return false;
    }
    std::fs::read(path).ok().and_then(|b| SbsarBackend::open(&b).ok()).is_some_and(|b| b.graphs.iter().any(|g| g.is_filter()) && !b.graphs.iter().any(|g| !g.channels.is_empty() && !g.is_filter()))
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
            .filter(|g| !g.channels.is_empty() || g.mask_output.is_some())
            .map(|g| {
                let filter_inputs: Vec<(String, ChannelKind)> = g.image_inputs.iter().filter_map(|i| Some((i.identifier.clone(), sbsar_filter_channel(i, &g.channels)?))).collect();
                let others = mesh_image_inputs(&g.image_inputs, &filter_inputs);
                let notes = sbsar_input_notes(&others);
                GraphDesc {
                    graph: g.path.clone(),
                    label: g.label.clone(),
                    params: g.params.clone(),
                    channels: g.channels.clone(),
                    notes,
                    mesh_inputs: {
                        let mut kinds: Vec<MeshMapKind> = others.iter().filter_map(sbsar_mesh_map).collect();
                        kinds.sort();
                        kinds.dedup();
                        kinds
                    },
                    mask: g.mask_output.is_some(),
                    filter_inputs,
                }
            })
            .collect();
        Ok(SbsarBackend { package, graphs, renderers: HashMap::new() })
    }
}

/// The baked mesh map an archive's image input reads: by identifier, then by label.
pub fn sbsar_mesh_map(input: &subcraft_sbsar::ImageInputInfo) -> Option<MeshMapKind> {
    crate::materialcraft::mesh_map_for_name(&input.identifier).or_else(|| crate::materialcraft::mesh_map_for_name(&input.label))
}

/// The channel of the stack below that an archive's image input reads, when the graph also
/// outputs that channel (a Substance filter: "basecolor" in, "basecolor" out). Names that are
/// also baked mesh maps ("normal", "height", "ambient_occlusion") read the baked map unless
/// they say "input" ("input_normal", "normal_input").
pub fn sbsar_filter_channel(input: &subcraft_sbsar::ImageInputInfo, outputs: &[ChannelKind]) -> Option<ChannelKind> {
    let key: String = input.identifier.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    let explicit = key.strip_prefix("input").or_else(|| key.strip_suffix("input")).filter(|k| !k.is_empty());
    let kind = match explicit {
        Some(k) => subcraft_doc::graph_material::channel_for_usage(k)?.0,
        None if crate::materialcraft::mesh_map_for_name(&key).is_some() => return None,
        None => subcraft_doc::graph_material::channel_for_usage(&key)?.0,
    };
    outputs.contains(&kind).then_some(kind)
}

/// The image inputs that are not filter inputs (they read baked mesh maps).
fn mesh_image_inputs(inputs: &[subcraft_sbsar::ImageInputInfo], filter: &[(String, ChannelKind)]) -> Vec<subcraft_sbsar::ImageInputInfo> {
    inputs.iter().filter(|i| !filter.iter().any(|(id, _)| *id == i.identifier)).cloned().collect()
}

/// The stack's channels for a filter's inputs. Color channels go in sRGB-encoded (as the
/// archive's base color input expects, like an image file), data channels raw, normals as
/// OpenGL-style normal maps.
pub fn sbsar_filter_images(filter: &[(String, ChannelKind)], below: &BTreeMap<ChannelKind, TiledImage>) -> BTreeMap<String, subcraft_sbsar::InputImage> {
    filter
        .iter()
        .filter_map(|(id, kind)| {
            let image = below.get(kind)?;
            let comps = image.comps();
            if !matches!(comps, 1 | 3 | 4) {
                return None;
            }
            let mut data = image.to_dense();
            if kind.is_color() {
                for px in data.chunks_mut(comps) {
                    for v in px.iter_mut().take(comps.min(3)) {
                        *v = subcraft_core::color::linear_to_srgb(*v);
                    }
                }
            }
            Some((id.clone(), subcraft_sbsar::InputImage { width: image.width(), height: image.height(), channels: comps as u32, data, normal: *kind == ChannelKind::Normal }))
        })
        .collect()
}

fn sbsar_input_notes(inputs: &[subcraft_sbsar::ImageInputInfo]) -> Vec<String> {
    let unmatched: Vec<&str> = inputs.iter().filter(|i| sbsar_mesh_map(i).is_none()).map(|i| i.label.as_str()).collect();
    let mut notes = Vec::new();
    if inputs.len() > unmatched.len() {
        notes.push("baked mesh maps feed the graph's inputs; bake the texture set for them to show".to_string());
    }
    if !unmatched.is_empty() {
        notes.push(format!("image inputs with no mesh map (they keep the archive's default): {}", unmatched.join(", ")));
    }
    notes
}

/// Baked maps for an archive's image inputs, by identifier, as raw values (mesh maps are data,
/// never color-transformed). A map that is not baked yet gives its neutral value (no
/// occlusion, flat curvature, flat normal), as SubCraft's own generators read it.
pub fn sbsar_input_images(inputs: &[subcraft_sbsar::ImageInputInfo], maps: &MeshMaps) -> BTreeMap<String, subcraft_sbsar::InputImage> {
    inputs
        .iter()
        .filter_map(|input| {
            let kind = sbsar_mesh_map(input)?;
            let normal = matches!(kind, MeshMapKind::Normal | MeshMapKind::BentNormal);
            let image = match maps.get(kind) {
                Some(image) if matches!(image.comps(), 1 | 3 | 4) => subcraft_sbsar::InputImage { width: image.width(), height: image.height(), channels: image.comps() as u32, data: image.to_dense(), normal },
                _ => subcraft_sbsar::InputImage { width: 1, height: 1, channels: 3, data: kind.neutral_value().to_vec(), normal },
            };
            Some((input.identifier.clone(), image))
        })
        .collect()
}

impl GraphBackend for SbsarBackend {
    fn graphs(&self) -> &[GraphDesc] {
        &self.graphs
    }

    fn render(&mut self, graph: &str, values: &BTreeMap<String, Vec<f64>>, size_log2: u32) -> Result<Vec<RenderedMap>, String> {
        self.render_with_maps(graph, values, size_log2, &MeshMaps::new())
    }

    fn render_with_maps(&mut self, graph: &str, values: &BTreeMap<String, Vec<f64>>, size_log2: u32, maps: &MeshMaps) -> Result<Vec<RenderedMap>, String> {
        self.render_filter(graph, values, size_log2, maps, &BTreeMap::new())
    }

    fn render_filter(&mut self, graph: &str, values: &BTreeMap<String, Vec<f64>>, size_log2: u32, maps: &MeshMaps, below: &BTreeMap<ChannelKind, TiledImage>) -> Result<Vec<RenderedMap>, String> {
        let info = self.package.graph(graph);
        let path = info.path.clone();
        let filter = self.graphs.iter().find(|g| g.graph == path).map(|g| g.filter_inputs.clone()).unwrap_or_default();
        let mut images = sbsar_input_images(&mesh_image_inputs(&info.image_inputs, &filter), maps);
        images.extend(sbsar_filter_images(&filter, below));
        let package = self.package.clone();
        let r = self.renderers.entry(path.clone()).or_insert_with(|| subcraft_sbsar::GraphRenderer::new(package, &path));
        let out = r.render_with_images(values, size_log2, &images).map_err(|e| e.to_string())?;
        for w in &out.warnings {
            log::info!("sbsar {path}: {w}");
        }
        Ok(out.maps.into_iter().map(|m| RenderedMap { channel: m.channel, bitmap: m.bitmap }).collect())
    }

    fn render_mask(&mut self, graph: &str, values: &BTreeMap<String, Vec<f64>>, size_log2: u32, maps: &MeshMaps) -> Result<Bitmap, String> {
        let info = self.package.graph(graph);
        let path = info.path.clone();
        let images = sbsar_input_images(&info.image_inputs, maps);
        let package = self.package.clone();
        let r = self.renderers.entry(path.clone()).or_insert_with(|| subcraft_sbsar::GraphRenderer::new(package, &path));
        let (bitmap, warnings) = r.render_mask(values, size_log2, &images).map_err(|e| e.to_string())?;
        for w in &warnings {
            log::info!("sbsar {path}: {w}");
        }
        Ok(bitmap)
    }
}

type SharedBackend = Arc<Mutex<Box<dyn GraphBackend>>>;

/// What a running render will do with its images.
#[derive(Clone, Debug)]
enum Target {
    /// Add a new layer to this texture set.
    New { texture_set: TextureSetId, name: String },
    /// Add a Substance generator to this layer's mask (the mask is made if there is none).
    NewMask { layer: LayerId, name: String },
    /// Add a Substance filter to the top of this layer's content effects.
    NewContent { layer: LayerId, name: String },
    /// Swap the images of this layer or generator.
    Update(GraphTarget),
}

impl Target {
    fn updates(&self) -> Option<GraphTarget> {
        match self {
            Target::Update(t) => Some(*t),
            _ => None,
        }
    }
}

/// What a render made.
enum Output {
    Maps(Vec<RenderedMap>),
    Mask(Bitmap),
}

struct Running {
    target: Target,
    material: GraphMaterial,
    /// Started by a new bake, not by the user (names the undo step).
    refresh: bool,
    /// The stack a filter read (kept for the next render of the same filter).
    below: Option<Below>,
    rx: Receiver<Result<Output, String>>,
}

/// The layer stack below a Substance filter: linear channels as the compositor keeps them.
type Below = Arc<BTreeMap<ChannelKind, TiledImage>>;

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
    /// Newest request per target that waits for that target's running render.
    queued: HashMap<GraphTarget, GraphMaterial>,
    /// Baked maps per texture set at the last [`Self::refresh_baked`], and the session's
    /// maps generation then.
    baked: HashMap<TextureSetId, BTreeMap<MeshMapKind, StorageToken>>,
    baked_generation: Option<u64>,
    /// The stack below the filter rendered last, so dragging its sliders does not composite the
    /// texture set again for every render. One entry: a composite is large.
    below: Option<(GraphTarget, Below)>,
    /// Substance filters known so far (see [`Self::refresh_new_filters`]) and the stacks they
    /// were collected from.
    filters_seen: Option<std::collections::HashSet<GraphTarget>>,
    filters_stacks: Vec<usize>,
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
        self.shown_for(GraphTarget::Layer(layer.id), layer.graph.as_deref())
    }

    /// [`Self::shown_material`] for any target, whose own material is `current`.
    pub fn shown_for(&self, target: GraphTarget, current: Option<&GraphMaterial>) -> Option<GraphMaterial> {
        if let Some(m) = self.queued.get(&target) {
            return Some(m.clone());
        }
        if let Some(r) = self.running.iter().rev().find(|r| r.target.updates() == Some(target)) {
            return Some(r.material.clone());
        }
        current.cloned()
    }

    /// True while a render for `layer` runs or waits.
    pub fn is_rendering(&self, layer: LayerId) -> bool {
        self.is_rendering_target(GraphTarget::Layer(layer))
    }

    /// True while a render for `target` runs or waits.
    pub fn is_rendering_target(&self, target: GraphTarget) -> bool {
        self.queued.contains_key(&target) || self.running.iter().any(|r| r.target.updates() == Some(target))
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
        let graph = backend.graphs().iter().find(|g| !g.channels.is_empty() && !g.is_filter()).cloned().ok_or_else(|| {
            if backend.graphs().iter().any(|g| g.is_filter()) {
                format!("{name}: this is a filter (it reads the channels below it); add it to a layer's content instead")
            } else if backend.graphs().iter().any(|g| g.mask) { format!("{name}: this is a generator (it outputs a mask, not material channels); add it to a layer's mask instead") } else { format!("{name}: no graph has an output SubCraft can use") }
        })?;
        let (id, shared) = self.add_package(session, engine, backend, bytes, name);
        let material = GraphMaterial::new(engine, id, graph.graph.clone(), subcraft_sbsar::DEFAULT_SIZE_LOG2);
        let texture_set = session.selection().texture_set;
        let maps = session.mesh_maps(texture_set).cloned().unwrap_or_default();
        self.spawn(shared, Target::New { texture_set, name: graph.label.clone() }, material, maps, None, false);
        let mut msg = format!("Rendering {} ({})…", graph.label, engine.label());
        for n in &graph.notes {
            msg.push_str(&format!(" Note: {n}."));
        }
        Ok(msg)
    }

    /// The package resource for `bytes` (the same file again shares one resource, saved once)
    /// and its opened backend.
    fn add_package(&mut self, session: &mut Session, engine: GraphEngine, backend: Box<dyn GraphBackend>, bytes: Vec<u8>, name: &str) -> (ResourceId, SharedBackend) {
        let _ = engine;
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
        (id, shared)
    }

    /// Add a Substance generator (an `.sbsar` with a mask output) to `layer`'s mask. The effect
    /// appears when its first render is done; baked mesh maps feed the graph's inputs.
    pub fn import_generator_file(&mut self, session: &mut Session, layer: LayerId, path: &Path) -> Result<String, String> {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("generator").to_string();
        let engine = engine_for_file(&name).ok_or_else(|| format!("{name}: not a material graph SubCraft can open"))?;
        let len = std::fs::metadata(path).map_err(|e| format!("{name}: {e}"))?.len();
        if len > MAX_PACKAGE_BYTES {
            return Err(format!("{name}: {} MB is larger than the {} MB limit", len >> 20, MAX_PACKAGE_BYTES >> 20));
        }
        let bytes = std::fs::read(path).map_err(|e| format!("{name}: {e}"))?;
        self.import_generator_bytes(session, layer, engine, bytes, &name)
    }

    /// [`Self::import_generator_file`] for bytes already read.
    pub fn import_generator_bytes(&mut self, session: &mut Session, layer: LayerId, engine: GraphEngine, bytes: Vec<u8>, name: &str) -> Result<String, String> {
        if find_layer(session, layer).is_none() {
            return Err("select a layer first".into());
        }
        let backend = open_backend(engine, &bytes, name).map_err(|e| format!("{name}: {e}"))?;
        let graph = backend.graphs().iter().find(|g| g.mask).cloned().ok_or_else(|| format!("{name}: no graph has a mask output (an output named or tagged \"mask\")"))?;
        let (id, shared) = self.add_package(session, engine, backend, bytes, name);
        let material = GraphMaterial::new(engine, id, graph.graph.clone(), subcraft_sbsar::DEFAULT_SIZE_LOG2);
        let maps = find_layer(session, layer).and_then(|(ts, _)| session.mesh_maps(ts).cloned()).unwrap_or_default();
        self.spawn(shared, Target::NewMask { layer, name: graph.label.clone() }, material, maps, None, false);
        let mut msg = format!("Rendering generator {}…", graph.label);
        for n in &graph.notes {
            msg.push_str(&format!(" Note: {n}."));
        }
        Ok(msg)
    }

    /// Add a Substance filter (an `.sbsar` whose image inputs read the channels it outputs) on top
    /// of `layer`'s content. It renders from the stack below as it is now; the effect appears when
    /// the render is done.
    pub fn import_filter_file(&mut self, session: &mut Session, layer: LayerId, path: &Path) -> Result<String, String> {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("filter").to_string();
        let engine = engine_for_file(&name).ok_or_else(|| format!("{name}: not a material graph SubCraft can open"))?;
        let len = std::fs::metadata(path).map_err(|e| format!("{name}: {e}"))?.len();
        if len > MAX_PACKAGE_BYTES {
            return Err(format!("{name}: {} MB is larger than the {} MB limit", len >> 20, MAX_PACKAGE_BYTES >> 20));
        }
        let bytes = std::fs::read(path).map_err(|e| format!("{name}: {e}"))?;
        self.import_filter_bytes(session, layer, engine, bytes, &name)
    }

    /// [`Self::import_filter_file`] for bytes already read.
    pub fn import_filter_bytes(&mut self, session: &mut Session, layer: LayerId, engine: GraphEngine, bytes: Vec<u8>, name: &str) -> Result<String, String> {
        if find_layer(session, layer).is_none() {
            return Err("select a layer first".into());
        }
        let backend = open_backend(engine, &bytes, name).map_err(|e| format!("{name}: {e}"))?;
        let graph = backend.graphs().iter().find(|g| g.is_filter()).cloned().ok_or_else(|| {
            format!("{name}: not a filter (a filter has image inputs named after the channels it outputs, like \"basecolor\" in and out)")
        })?;
        let (id, shared) = self.add_package(session, engine, backend, bytes, name);
        let material = GraphMaterial::new(engine, id, graph.graph.clone(), subcraft_sbsar::DEFAULT_SIZE_LOG2);
        let maps = find_layer(session, layer).and_then(|(ts, _)| session.mesh_maps(ts).cloned()).unwrap_or_default();
        let below = session.composite_below(layer, None).map(|c| Arc::new(c.channels)).ok_or("select a layer first")?;
        self.spawn(shared, Target::NewContent { layer, name: graph.label.clone() }, material, maps, Some(below), false);
        let mut msg = format!("Rendering filter {}…", graph.label);
        for n in &graph.notes {
            msg.push_str(&format!(" Note: {n}."));
        }
        Ok(msg)
    }

    /// Render a Substance filter again from the stack below it as it is now (after the layers
    /// below changed), with `material` (its current values).
    pub fn update_filter_input(&mut self, session: &Session, layer: LayerId, effect: EffectId, material: GraphMaterial) -> Result<(), String> {
        let target = GraphTarget::Content { layer, effect };
        if self.below.as_ref().is_some_and(|(t, _)| *t == target) {
            self.below = None;
        }
        self.request_as(session, target, material, false)
    }

    /// Render `layer` again with `material` (new values or size). The layer changes when the
    /// render is done.
    pub fn request(&mut self, session: &Session, layer: LayerId, material: GraphMaterial) -> Result<(), String> {
        self.request_as(session, GraphTarget::Layer(layer), material, false)
    }

    /// [`Self::request`] for any target (a Substance generator's mask image too).
    pub fn request_target(&mut self, session: &Session, target: GraphTarget, material: GraphMaterial) -> Result<(), String> {
        self.request_as(session, target, material, false)
    }

    fn request_as(&mut self, session: &Session, target: GraphTarget, material: GraphMaterial, refresh: bool) -> Result<(), String> {
        if self.running.iter().any(|r| r.target.updates() == Some(target)) {
            self.queued.insert(target, material);
            return Ok(());
        }
        let backend = self.backend(session, material.engine, material.package)?;
        let layer = target.layer();
        let maps = find_layer(session, layer).and_then(|(ts, _)| session.mesh_maps(ts).cloned()).unwrap_or_default();
        let below = match target {
            GraphTarget::Content { layer, effect } => Some(match &self.below {
                // A refresh after a bake reads the stack again (the maps below may have changed too).
                Some((t, b)) if *t == target && !refresh => b.clone(),
                _ => session.composite_below(layer, Some(effect)).map(|c| Arc::new(c.channels)).ok_or("the layer was deleted")?,
            }),
            _ => None,
        };
        self.spawn(backend, Target::Update(target), material, maps, below, refresh);
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
        let mut stale: Vec<(GraphTarget, GraphMaterial, Vec<MeshMapKind>)> = Vec::new();
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
                            stale.push((GraphTarget::Layer(layer.id), self.shown_material(layer).unwrap_or_else(|| material.clone()), changed.clone()));
                        }
                        for e in layer.mask.iter().flat_map(|m| &m.effects) {
                            if let Some(material) = e.graph.as_deref() {
                                let target = GraphTarget::Mask { layer: layer.id, effect: e.id };
                                stale.push((target, self.shown_for(target, Some(material)).unwrap_or_else(|| material.clone()), changed.clone()));
                            }
                        }
                        for e in &layer.content_effects {
                            if let Some(material) = e.graph.as_deref() {
                                let target = GraphTarget::Content { layer: layer.id, effect: e.id };
                                stale.push((target, self.shown_for(target, Some(material)).unwrap_or_else(|| material.clone()), changed.clone()));
                            }
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

    /// Render the Substance filters that appeared since the last call by any other way than
    /// this object adding them (a smart material, a pasted or duplicated layer): they carry
    /// images rendered from another stack. The first call only takes note. Returns how many
    /// started rendering.
    pub fn refresh_new_filters(&mut self, session: &Session) -> usize {
        let stacks: Vec<usize> = session.document().texture_sets.iter().flat_map(|ts| ts.layers.iter().map(|l| Arc::as_ptr(l) as usize)).collect();
        if self.filters_seen.is_some() && stacks == self.filters_stacks {
            return 0;
        }
        self.filters_stacks = stacks;
        let mut now: Vec<(GraphTarget, GraphMaterial)> = Vec::new();
        for ts in &session.document().texture_sets {
            ts.walk(|layer, _| {
                for e in &layer.content_effects {
                    if let Some(g) = e.graph.as_deref() {
                        now.push((GraphTarget::Content { layer: layer.id, effect: e.id }, g.clone()));
                    }
                }
            });
        }
        let Some(seen) = self.filters_seen.as_mut() else {
            self.filters_seen = Some(now.into_iter().map(|(t, _)| t).collect());
            return 0;
        };
        let fresh: Vec<(GraphTarget, GraphMaterial)> = now.into_iter().filter(|(t, _)| seen.insert(*t)).collect();
        let mut started = 0;
        for (target, material) in fresh {
            if !self.is_rendering_target(target) && self.request_as(session, target, material, true).is_ok() {
                started += 1;
            }
        }
        started
    }

    fn spawn(&mut self, backend: SharedBackend, target: Target, material: GraphMaterial, maps: MeshMaps, below: Option<Below>, refresh: bool) {
        let (tx, rx) = channel();
        let m = material.clone();
        let mask = matches!(target, Target::NewMask { .. } | Target::Update(GraphTarget::Mask { .. }));
        let filter = below.clone();
        let spawned = std::thread::Builder::new().name("graph-render".into()).spawn(move || {
            let result = match backend.lock() {
                Ok(mut b) => {
                    let graph = m.graph.clone();
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        if mask {
                            b.render_mask(&graph, &m.values, m.size_log2, &maps).map(Output::Mask)
                        } else if let Some(below) = &filter {
                            b.render_filter(&graph, &m.values, m.size_log2, &maps, below).map(Output::Maps)
                        } else {
                            b.render_with_maps(&graph, &m.values, m.size_log2, &maps).map(Output::Maps)
                        }
                    }))
                    .unwrap_or_else(|_| Err("the renderer crashed on this material".into()))
                }
                Err(_) => Err("the renderer crashed earlier on this material".into()),
            };
            let _ = tx.send(result);
        });
        if let Err(e) = spawned {
            let (tx, rx) = channel();
            let _ = tx.send(Err(format!("could not start the render: {e}")));
            self.running.push(Running { target, material, refresh, below, rx });
            return;
        }
        self.running.push(Running { target, material, refresh, below, rx });
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
            let applied = result.and_then(|maps| apply(session, &done.target, &done.material, done.refresh, maps));
            // Keep the stack the filter read for its next render (slider drags).
            if applied.is_ok()
                && let Some(below) = done.below.clone()
            {
                let target = match &done.target {
                    Target::Update(t) => Some(*t),
                    Target::NewContent { layer, .. } => find_layer(session, *layer).and_then(|(_, l)| l.content_effects.last().map(|e| GraphTarget::Content { layer: *layer, effect: e.id })),
                    _ => None,
                };
                self.below = target.map(|t| (t, below));
                // A filter this object added is not "new" to [`Self::refresh_new_filters`].
                if let (Some(t), Some(seen)) = (target, self.filters_seen.as_mut()) {
                    seen.insert(t);
                }
            }
            // A graph copied to partner UDIM tiles carries this tile's render. When the graph
            // reads baked maps, each partner renders its own from its tile's maps (as a refresh,
            // so this does not repeat).
            let reads_maps = |g: &mut Self| g.graph(session, &done.material).is_ok_and(|d| !d.mesh_inputs.is_empty());
            // A filter reads its own tile's stack, so its partners always render their own.
            let filter = matches!(done.target, Target::Update(GraphTarget::Content { .. }));
            if applied.is_ok() && !done.refresh && (filter || reads_maps(self)) {
                for (target, material) in udim_partner_targets(session, &done.target) {
                    if let Err(e) = self.request_as(session, target, material, true) {
                        events.push(GraphEvent::Failed(e));
                    }
                }
            }
            events.push(match applied {
                Ok(text) => GraphEvent::Applied(text),
                Err(e) => GraphEvent::Failed(e),
            });
            if let Some(target) = done.target.updates()
                && let Some(next) = self.queued.remove(&target)
                && let Err(e) = self.request_target(session, target, next)
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

/// The graph layers or generators on partner UDIM tiles that mirror the one `target` just
/// rendered, with their graphs (a new generator is the last effect of its mask).
fn udim_partner_targets(session: &Session, target: &Target) -> Vec<(GraphTarget, GraphMaterial)> {
    if let Target::Update(GraphTarget::Layer(layer)) = target {
        let Some(source_ts) = session.texture_set_of(*layer).and_then(|id| session.document().texture_set(id)) else { return Vec::new() };
        let doc = session.document();
        return doc
            .udim_paint_partners(source_ts.id)
            .into_iter()
            .filter_map(|id| {
                let ts = doc.texture_set(id)?;
                let other = ts.find(ts.corresponding_layer(source_ts, *layer)?)?;
                Some((GraphTarget::Layer(other.id), (**other.graph.as_ref()?).clone()))
            })
            .collect();
    }
    if let Target::Update(GraphTarget::Content { layer, effect }) = target {
        return session
            .udim_partner_content_effects(*layer, *effect)
            .into_iter()
            .filter_map(|(layer, effect)| {
                let (_, l) = find_layer(session, layer)?;
                let graph = l.content_effects.iter().find(|e| e.id == effect)?.graph.clone()?;
                Some((GraphTarget::Content { layer, effect }, (*graph).clone()))
            })
            .collect();
    }
    let (layer, effect) = match target {
        Target::Update(GraphTarget::Mask { layer, effect }) => (*layer, *effect),
        Target::NewMask { layer, .. } => match find_layer(session, *layer).and_then(|(_, l)| l.mask.as_ref().and_then(|m| m.effects.last().map(|e| e.id))) {
            Some(effect) => (*layer, effect),
            None => return Vec::new(),
        },
        _ => return Vec::new(),
    };
    session
        .udim_partner_mask_effects(layer, effect)
        .into_iter()
        .filter_map(|(layer, effect)| {
            let (_, l) = find_layer(session, layer)?;
            let graph = l.mask.as_ref()?.effects.iter().find(|e| e.id == effect)?.graph.clone()?;
            Some((GraphTarget::Mask { layer, effect }, (*graph).clone()))
        })
        .collect()
}

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

/// The rendered mask as a texture resource of the project.
fn add_mask_image(session: &mut Session, material: &GraphMaterial, bitmap: Bitmap) -> ResourceId {
    let name = session.resources().get(material.package).map(|r| r.name.clone()).unwrap_or_default();
    session.add_resource(Resource { id: ResourceId::new(), name: format!("{name} mask"), kind: ResourceKind::Texture, bitmap: Arc::new(bitmap), source_path: None, encoded: None })
}

/// A Substance generator's image mask effect: the rendered image in UV space.
fn generator_kind(resource: ResourceId) -> MaskEffectKind {
    MaskEffectKind::Image { resource, projection: Projection::default(), invert: false, balance: 0.5, contrast: 0.0 }
}

fn apply_mask(session: &mut Session, target: &Target, material: &GraphMaterial, refresh: bool, bitmap: Bitmap) -> Result<String, String> {
    match target {
        Target::NewMask { layer, name } => {
            find_layer(session, *layer).ok_or("the layer was deleted while rendering")?;
            let resource = add_mask_image(session, material, bitmap);
            let effect = MaskEffect { name: name.clone(), graph: Some(Arc::new(material.clone())), ..MaskEffect::new(generator_kind(resource)) };
            session.execute(Command::ApplyMaskStack { layer: *layer, effects: vec![effect], replace: false, label: format!("Add Generator: {name}") }).map_err(|e| e.to_string())?;
            Ok(format!("Added generator {name}"))
        }
        Target::Update(GraphTarget::Mask { layer, effect }) => {
            let (_, l) = find_layer(session, *layer).ok_or("the layer was deleted while rendering")?;
            let mut e = (**l.mask.as_ref().and_then(|m| m.effects.iter().find(|e| e.id == *effect)).ok_or("the generator was deleted while rendering")?).clone();
            let resource = add_mask_image(session, material, bitmap);
            match &mut e.kind {
                MaskEffectKind::Image { resource: r, .. } => *r = resource,
                _ => return Err("the effect is no longer a generator".into()),
            }
            e.graph = Some(Arc::new(material.clone()));
            let name = e.name.clone();
            session.execute(Command::SetMaskEffect { layer: *layer, effect: e }).map_err(|e| e.to_string())?;
            Ok(if refresh { format!("Updated {name} for the new bake") } else { format!("Updated {name}") })
        }
        _ => Err("a mask render for a layer".into()),
    }
}

/// A Substance filter's fill: the rendered channels the texture set has, each replacing the stack
/// below (Normal blend, normals too: the filter read them).
fn filter_fill(ts: &subcraft_doc::TextureSet, ids: &BTreeMap<ChannelKind, ResourceId>) -> (ContentEffectKind, BTreeMap<ChannelKind, subcraft_doc::ChannelBlend>) {
    let channels = ids.iter().filter(|(k, _)| ts.has_channel(**k)).map(|(k, &resource)| (*k, FillChannel::new(FillSource::Bitmap { resource }))).collect();
    let blend = ts.channels.iter().map(|c| (c.kind, subcraft_doc::ChannelBlend::new(subcraft_doc::BlendMode::Normal))).collect();
    (ContentEffectKind::Fill(subcraft_doc::FillContent { projection: Projection::default(), channels }), blend)
}

fn apply_filter(session: &mut Session, target: &Target, material: &GraphMaterial, refresh: bool, maps: Vec<RenderedMap>) -> Result<String, String> {
    match target {
        Target::NewContent { layer, name } => {
            let (ts_id, mut l) = find_layer(session, *layer).ok_or("the layer was deleted while rendering")?;
            let ts = session.document().texture_set(ts_id).ok_or("the texture set is gone")?.clone();
            let ids = add_maps(session, material, maps);
            let (kind, blend) = filter_fill(&ts, &ids);
            let effect = ContentEffect { name: name.clone(), blend, graph: Some(Arc::new(material.clone())), ..ContentEffect::new(kind, &ts) };
            l.content_effects.push(Arc::new(effect));
            session.execute(Command::ReplaceLayer { layer: *layer, replacement: Box::new(l), label: format!("Add Filter: {name}") }).map_err(|e| e.to_string())?;
            Ok(format!("Added filter {name}"))
        }
        Target::Update(GraphTarget::Content { layer, effect }) => {
            let (ts_id, l) = find_layer(session, *layer).ok_or("the layer was deleted while rendering")?;
            let ts = session.document().texture_set(ts_id).ok_or("the texture set is gone")?.clone();
            let mut e = (**l.content_effects.iter().find(|e| e.id == *effect).ok_or("the filter was deleted while rendering")?).clone();
            let ContentEffectKind::Fill(fill) = &mut e.kind else { return Err("the effect is no longer a filter".into()) };
            let ids = add_maps(session, material, maps);
            let (ContentEffectKind::Fill(new), _) = filter_fill(&ts, &ids) else { unreachable!("filter_fill makes a fill") };
            // Keep the user's strength / adjustments per channel.
            for (k, mut c) in new.channels {
                if let Some(old) = fill.channels.get(&k) {
                    c.strength = old.strength;
                    c.adjust = old.adjust;
                    c.enabled = old.enabled;
                }
                fill.channels.insert(k, c);
            }
            e.graph = Some(Arc::new(material.clone()));
            let name = e.name.clone();
            session.execute(Command::SetContentEffect { layer: *layer, effect: e }).map_err(|e| e.to_string())?;
            Ok(if refresh { format!("Updated {name} from the layers below") } else { format!("Updated {name}") })
        }
        _ => Err("a filter render for a layer".into()),
    }
}

fn apply(session: &mut Session, target: &Target, material: &GraphMaterial, refresh: bool, output: Output) -> Result<String, String> {
    let maps = match output {
        Output::Mask(bitmap) => return apply_mask(session, target, material, refresh, bitmap),
        Output::Maps(maps) => maps,
    };
    if maps.is_empty() {
        return Err("the render produced no image".into());
    }
    match target {
        Target::NewContent { .. } | Target::Update(GraphTarget::Content { .. }) => apply_filter(session, target, material, refresh, maps),
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
        Target::NewMask { .. } | Target::Update(GraphTarget::Mask { .. }) => Err("material channels for a generator".into()),
        Target::Update(GraphTarget::Layer(layer)) => {
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

#[cfg(test)]
mod sbsar_input_tests {
    use super::*;
    use subcraft_sbsar::ImageInputInfo;

    fn input(identifier: &str, label: &str) -> ImageInputInfo {
        ImageInputInfo { identifier: identifier.into(), label: label.into() }
    }

    #[test]
    fn archive_inputs_find_their_mesh_maps_by_identifier_or_label() {
        for (id, label, kind) in [
            ("ambient_occlusion", "Ambient Occlusion", Some(MeshMapKind::AmbientOcclusion)),
            ("curvature_map", "", Some(MeshMapKind::Curvature)),
            ("world_space_normals", "", Some(MeshMapKind::WorldSpaceNormal)),
            ("position_input", "", Some(MeshMapKind::Position)),
            ("input_3", "Thickness Map", Some(MeshMapKind::Thickness)),
            ("normal", "Normal", Some(MeshMapKind::Normal)),
            ("input", "Input", None),
            ("basecolor_input", "Base Color", None),
        ] {
            assert_eq!(sbsar_mesh_map(&input(id, label)), kind, "{id} / {label}");
        }
    }

    #[test]
    fn baked_maps_feed_archive_inputs_and_unbaked_ones_are_neutral() {
        let mut maps = MeshMaps::new();
        maps.insert(MeshMapKind::Curvature, subcraft_image::TiledImage::new(4, 4, 1, [0.75, 0.0, 0.0, 0.0]));
        let inputs = [input("curvature", "Curvature"), input("ao", "AO"), input("normal", "Normal"), input("mask", "Mask")];
        let images = sbsar_input_images(&inputs, &maps);
        assert_eq!((images["curvature"].width, images["curvature"].channels, &images["curvature"].data), (4, 1, &vec![0.75; 16]));
        assert!(!images["curvature"].normal);
        assert_eq!(images["ao"].data, MeshMapKind::AmbientOcclusion.neutral_value().to_vec(), "unbaked AO: no occlusion");
        assert!(images["normal"].normal, "normal inputs are marked so DirectX graphs get green flipped");
        assert!(!images.contains_key("mask"), "inputs that are not mesh maps keep the archive's default");
        let notes = sbsar_input_notes(&inputs);
        assert!(notes.iter().any(|n| n.contains("Mask")) && notes.iter().any(|n| n.contains("bake")), "{notes:?}");
        assert!(sbsar_input_notes(&[]).is_empty());
    }
}
