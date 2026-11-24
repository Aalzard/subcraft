//! Open MaterialCraft materials in SubCraft.
//!
//! MaterialCraft (<https://github.com/storytold/materialcraft>, MIT OR Apache-2.0) stores every
//! material as an SBS node graph: a loose `.sbs` package, a `.mcraft` ZIP project, or a portable
//! `.mcmat` archive. Rebuilding those graphs out of SubCraft's own procedural pieces would lose most
//! nodes, so this crate runs MaterialCraft's own CPU engine instead. The result is the same pixels
//! MaterialCraft shows, and the graph's exposed parameters stay editable.
//!
//! Nothing from MaterialCraft leaks out of this crate: callers see [`Material`], [`Param`],
//! [`ParamValue`] and [`Output`] only, so a MaterialCraft update touches this file alone.

use std::collections::BTreeMap;

use materialcraft_core::{BaseParameter, Graph, GraphIndex, NodeId, Package, SizeMode, Value};
use materialcraft_engine::{ExportSize, Session, file_cmds};
use materialcraft_eval::{Cooked, Options};

/// File extensions this crate opens.
pub const EXTENSIONS: [&str; 3] = ["mcmat", "mcraft", "sbs"];

/// The pinned MaterialCraft revision, for About boxes and bug reports.
pub const MATERIALCRAFT_REV: &str = "356c066e838c1f8ae31dce8a0ab370d889c8f868";

/// Smallest and largest cook sizes in pixels (MaterialCraft's own limits: 2^4 to 2^13).
pub const MIN_SIZE: u32 = 1 << materialcraft_core::MIN_SIZE_LOG2;
pub const MAX_SIZE: u32 = 1 << materialcraft_core::MAX_SIZE_LOG2;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("MaterialCraft: {0}")]
    Engine(String),
    #[error("MaterialCraft file has no material graph with connected outputs")]
    NoMaterial,
    #[error("MaterialCraft graph {0} does not exist")]
    NoGraph(usize),
    #[error("unknown parameter `{0}`")]
    UnknownParam(String),
    #[error("parameter `{id}` takes a {expected}")]
    WrongType { id: String, expected: &'static str },
    #[error("MaterialCraft graph `{graph}`, node {node}: {message}")]
    Cook { graph: String, node: u64, message: String },
}

pub type Result<T> = std::result::Result<T, Error>;

fn engine(error: impl std::fmt::Display) -> Error {
    Error::Engine(error.to_string())
}

/// True when the bytes or the file name look like something [`Material::open`] reads.
pub fn is_materialcraft(bytes: &[u8], file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    if EXTENSIONS.iter().any(|ext| lower.ends_with(&format!(".{ext}"))) {
        return true;
    }
    materialcraft_formats::material::is_material(bytes) || materialcraft_formats::project::is_archive(bytes)
}

/// A parameter value. Colors are linear RGBA, as MaterialCraft stores them.
#[derive(Clone, Debug, PartialEq)]
pub enum ParamValue {
    Float(f32),
    Float2([f32; 2]),
    Float3([f32; 3]),
    Float4([f32; 4]),
    Int(i32),
    Int2([i32; 2]),
    Int3([i32; 3]),
    Int4([i32; 4]),
    Bool(bool),
    Color([f32; 4]),
    /// Index into [`Param::options`].
    Enum(u32),
    Text(String),
}

impl ParamValue {
    fn from_value(value: &Value) -> Option<Self> {
        Some(match value {
            Value::Float(v) => Self::Float(*v),
            Value::Float2(v) => Self::Float2(*v),
            Value::Float3(v) => Self::Float3(*v),
            Value::Float4(v) => Self::Float4(*v),
            Value::Int(v) => Self::Int(*v),
            Value::Int2(v) => Self::Int2(*v),
            Value::Int3(v) => Self::Int3(*v),
            Value::Int4(v) => Self::Int4(*v),
            Value::Bool(v) => Self::Bool(*v),
            Value::Color(v) => Self::Color(*v),
            Value::Enum(v) => Self::Enum(*v),
            Value::Text(v) => Self::Text(v.clone()),
            // Curves, gradients and function graphs are not sliders; they keep the graph's value.
            Value::Curve(_) | Value::Gradient(_) | Value::Function(_) | Value::PatternGraph(_) => return None,
        })
    }

    fn to_value(&self) -> Value {
        match self {
            Self::Float(v) => Value::Float(*v),
            Self::Float2(v) => Value::Float2(*v),
            Self::Float3(v) => Value::Float3(*v),
            Self::Float4(v) => Value::Float4(*v),
            Self::Int(v) => Value::Int(*v),
            Self::Int2(v) => Value::Int2(*v),
            Self::Int3(v) => Value::Int3(*v),
            Self::Int4(v) => Value::Int4(*v),
            Self::Bool(v) => Value::Bool(*v),
            Self::Color(v) => Value::Color(*v),
            Self::Enum(v) => Value::Enum(*v),
            Self::Text(v) => Value::Text(v.clone()),
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Float(_) => "float",
            Self::Float2(_) => "float2",
            Self::Float3(_) => "float3",
            Self::Float4(_) => "float4",
            Self::Int(_) => "int",
            Self::Int2(_) => "int2",
            Self::Int3(_) => "int3",
            Self::Int4(_) => "int4",
            Self::Bool(_) => "bool",
            Self::Color(_) => "color",
            Self::Enum(_) => "enum",
            Self::Text(_) => "text",
        }
    }

    /// Clamp numbers into `min..=max` when the parameter asks for it.
    fn clamped(self, param: &Param) -> Self {
        if !param.clamp {
            return self;
        }
        let (lo, hi) = (param.min.min(param.max), param.min.max(param.max));
        let f = |v: f32| v.clamp(lo, hi);
        let i = |v: i32| v.clamp(lo.round() as i32, hi.round() as i32);
        match self {
            Self::Float(v) => Self::Float(f(v)),
            Self::Float2(v) => Self::Float2(v.map(f)),
            Self::Float3(v) => Self::Float3(v.map(f)),
            Self::Float4(v) => Self::Float4(v.map(f)),
            Self::Int(v) => Self::Int(i(v)),
            Self::Int2(v) => Self::Int2(v.map(i)),
            Self::Int3(v) => Self::Int3(v.map(i)),
            Self::Int4(v) => Self::Int4(v.map(i)),
            Self::Enum(v) if !param.options.is_empty() => Self::Enum(v.min(param.options.len() as u32 - 1)),
            other => other,
        }
    }
}

/// One exposed graph parameter, ready to become a slider, checkbox, color or dropdown.
#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub id: String,
    pub label: String,
    pub group: String,
    pub description: String,
    pub default: ParamValue,
    pub min: f32,
    pub max: f32,
    pub step: Option<f32>,
    pub clamp: bool,
    /// Labels for an [`ParamValue::Enum`] parameter.
    pub options: Vec<String>,
}

/// One cooked graph output.
#[derive(Clone, Debug)]
pub struct Output {
    /// The output node's identifier inside the graph (`basecolor`, `rough`...).
    pub identifier: String,
    /// The SBS usage (`baseColor`, `roughness`, `metallic`, `normal`, `height`,
    /// `ambientOcclusion`, `emissive`, `opacity`...), or `any` when the author set none.
    pub usage: String,
    pub width: u32,
    pub height: u32,
    /// 1 (gray) or 4 (RGBA).
    pub channels: usize,
    /// Row-major texels, top row first, `width * height * channels` floats. Color usages are
    /// linear (MaterialCraft cooks in linear and applies sRGB only when writing files); data
    /// usages are raw values.
    pub texels: Vec<f32>,
    /// For a `normal` output: true when green points down (DirectX). Raw images are OpenGL.
    pub normal_y_down: bool,
}

/// A material graph inside an opened file.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphInfo {
    pub index: usize,
    pub name: String,
    /// Output usages, in node order.
    pub usages: Vec<String>,
}

/// An opened MaterialCraft file.
pub struct Material {
    session: Session,
    /// The graph to cook when the caller does not pick one.
    pub entry: usize,
}

impl std::fmt::Debug for Material {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Material").field("name", &self.session.pkg.name).field("entry", &self.entry).finish()
    }
}

impl Material {
    /// Open file bytes. `file_name` picks the format by extension; the bytes are sniffed too.
    /// Library nodes (`sbs://...`) come from MaterialCraft's bundled library. A loose `.sbs` that
    /// links bitmaps or packages next to it needs [`Material::open_file`] instead.
    pub fn open(bytes: &[u8], file_name: &str) -> Result<Self> {
        let mut session = empty_session();
        file_cmds::open_bytes(&mut session, bytes, Some(file_name.to_string())).map_err(engine)?;
        Self::from_session(session)
    }

    /// Open a file from disk, resolving the bitmaps and packages it links beside it.
    pub fn open_file(path: &std::path::Path) -> Result<Self> {
        let mut session = empty_session();
        let path = path.to_str().ok_or_else(|| Error::Engine("file path is not UTF-8".into()))?;
        file_cmds::open_file(&mut session, path).map_err(engine)?;
        Self::from_session(session)
    }

    fn from_session(session: Session) -> Result<Self> {
        let usable = |index: usize| session.pkg.graphs.get(index).is_some_and(is_material_graph);
        let entry = if usable(session.graph) {
            session.graph
        } else {
            (0..session.pkg.graphs.len()).find(|&index| usable(index)).ok_or(Error::NoMaterial)?
        };
        Ok(Self { session, entry })
    }

    /// The package name MaterialCraft gave the file.
    pub fn package_name(&self) -> &str {
        &self.session.pkg.name
    }

    /// Warnings MaterialCraft raised while reading (missing linked files and similar).
    pub fn warnings(&self) -> &[String] {
        &self.session.load_warnings
    }

    /// Every material graph in the file (a `.sbs` package can hold several).
    pub fn graphs(&self) -> Vec<GraphInfo> {
        self.session
            .pkg
            .graphs
            .iter()
            .enumerate()
            .filter(|(_, graph)| is_material_graph(graph))
            .map(|(index, graph)| GraphInfo {
                index,
                name: graph.display_name().to_string(),
                usages: graph
                    .nodes
                    .iter()
                    .filter(|node| node.def == "output")
                    .map(|node| materialcraft_eval::output_usage(node).to_string())
                    .collect(),
            })
            .collect()
    }

    /// Find a material graph by its display name or SBS identifier. An empty name means
    /// [`Material::entry`].
    pub fn graph_index(&self, name: &str) -> Option<usize> {
        if name.is_empty() {
            return Some(self.entry);
        }
        self.session
            .pkg
            .graphs
            .iter()
            .position(|graph| is_material_graph(graph) && (graph.display_name() == name || graph.name == name))
    }

    fn graph(&self, index: usize) -> Result<&Graph> {
        self.session.pkg.graphs.get(index).filter(|graph| is_material_graph(graph)).ok_or(Error::NoGraph(index))
    }

    /// The graph's display name.
    pub fn name(&self, graph: usize) -> Result<String> {
        Ok(self.graph(graph)?.display_name().to_string())
    }

    /// The graph's exposed parameters, in the author's order. Parameters that are not plain
    /// values (curves, gradients, functions) are left out and keep the graph's own value.
    pub fn params(&self, graph: usize) -> Result<Vec<Param>> {
        let graph = self.graph(graph)?;
        Ok(graph
            .exposed
            .iter()
            .filter_map(|param| {
                Some(Param {
                    id: param.id.clone(),
                    label: if param.label.is_empty() { param.id.clone() } else { param.label.clone() },
                    group: param.group.clone(),
                    description: param.description.clone(),
                    default: ParamValue::from_value(graph.input_value(&param.id).unwrap_or(&param.default))?,
                    min: param.min,
                    max: param.max,
                    step: param.step,
                    clamp: param.clamp,
                    options: param.options.clone(),
                })
            })
            .collect())
    }

    /// Check `values` against the graph's parameters and convert them, clamped where asked.
    fn overrides(&self, graph: usize, values: &BTreeMap<String, ParamValue>) -> Result<BTreeMap<String, Value>> {
        let params = self.params(graph)?;
        values
            .iter()
            .map(|(id, value)| {
                let param = params.iter().find(|param| param.id == *id).ok_or_else(|| Error::UnknownParam(id.clone()))?;
                if std::mem::discriminant(&param.default) != std::mem::discriminant(value) {
                    return Err(Error::WrongType { id: id.clone(), expected: param.default.type_name() });
                }
                Ok((id.clone(), value.clone().clamped(param).to_value()))
            })
            .collect()
    }

    /// Cook `graph` with `values` over its defaults, with the larger side at `size` pixels
    /// (rounded down to a power of two, 16 to 8192). Returns every output that cooked.
    pub fn cook(&self, graph: usize, values: &BTreeMap<String, ParamValue>, size: u32) -> Result<Vec<Output>> {
        let source = self.graph(graph)?;
        let overrides = self.overrides(graph, values)?;

        // Same steps as MaterialCraft's own export (`MaterialOptions::cook`): resize the graph,
        // and let fixed-size output bridges follow it.
        let mut package: Package = self.session.pkg.clone();
        let resized = package.graphs.get_mut(graph).ok_or(Error::NoGraph(graph))?;
        resized.size = ExportSize::LargerSide(size).log2_size(resized.size);
        for output in resized.nodes.iter_mut().filter(|node| node.def == "output") {
            output.base.size_mode = SizeMode::RelativeToParent;
            output.base.size = [0, 0];
            output.base.functions.remove(&BaseParameter::OutputSize);
        }

        let roots: Vec<NodeId> = source.nodes.iter().filter(|node| node.def == "output").map(|node| node.id).collect();
        let decoder = self.session.resource_decoder();
        let mut options = Options::new(&decoder);
        options.environment = self.session.environment;
        options.only = Some(roots);
        options.overrides = overrides;
        let cooked = materialcraft_eval::cook(&package, graph, &options, &self.session.cache);

        let resized = package.graphs.get(graph).ok_or(Error::NoGraph(graph))?;
        if let Some((id, message)) = cooked.nodes.iter().find_map(|(id, result)| Some((id, result.error.as_ref()?))) {
            return Err(Error::Cook { graph: resized.display_name().to_string(), node: id.0, message: message.to_string() });
        }

        let index = GraphIndex::new(resized);
        Ok(materialcraft_eval::outputs(resized, &cooked)
            .into_iter()
            .filter_map(|(id, usage, identifier, image)| {
                let image = image?;
                Some(Output {
                    identifier,
                    usage: usage.to_string(),
                    width: image.width(),
                    height: image.height(),
                    channels: image.channels(),
                    texels: image.data().to_vec(),
                    normal_y_down: usage == "normal" && normal_y_down(&index, &cooked, id),
                })
            })
            .collect())
    }
}

fn empty_session() -> Session {
    Session::new(Package::new("SubCraft import"))
}

/// A graph worth offering: a material graph (not a function) with at least one wired output.
fn is_material_graph(graph: &Graph) -> bool {
    graph.function.is_none()
        && graph.nodes.iter().any(|node| node.def == "output" && graph.connections.iter().any(|wire| wire.to == node.id))
}

/// A normal generator reports its convention; anything else is OpenGL. Mirrors MaterialCraft's
/// export, which follows dot (reroute) nodes back to the first `normal` node.
fn normal_y_down(index: &GraphIndex<'_>, cooked: &Cooked, output: NodeId) -> bool {
    let mut source = output;
    for _ in 0..cooked.nodes.len() {
        let Some(node) = index.input_of(source, "input").and_then(|wire| index.node(wire.from)) else { break };
        if node.def == "normal" && !node.disabled {
            return cooked.nodes.get(&node.id).and_then(|result| result.parameters.get("opengl")).and_then(Value::as_bool) == Some(false);
        }
        if node.def != "dot" {
            break;
        }
        source = node.id;
    }
    false
}

#[cfg(test)]
mod tests;
