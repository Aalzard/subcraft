//! Substance archives (`.sbsar`): read a graph's parameters and render its outputs as channel
//! maps, through the [open-sbsar](https://crates.io/crates/open-sbsar) engine (a clean-room
//! implementation of the compiled format, MIT/Apache-2.0).
//!
//! This crate only knows about archives, parameters and bitmaps. The layer that a graph drives
//! (its saved values and rendered resources) lives in `subcraft-doc`, the background render job
//! in `subcraft-app`.
//!
//! The engine runs on the CPU here: crates below `render` must not depend on wgpu. A render is
//! not cancellable and can take seconds (or, for a hostile file, minutes), so callers run it on a
//! worker thread and drop results that are no longer wanted.
//!
//! Conventions applied to every render, so the maps mean what SubCraft's channels mean:
//! * `$outputsize` is set from the requested size; every other `$` input is hidden except the
//!   random seed.
//! * Inputs in the "Channels" group that switch outputs on and off (`channel_basecolor`, ...)
//!   are hidden and forced on.
//! * Normal maps come out OpenGL-style (SubCraft's convention): a graph with a `normal_format`
//!   combo is set to its OpenGL entry; any other graph is assumed to give DirectX normals (the
//!   Substance default), and green is flipped.
//! * Maps keep their encoded values (Base Color stays sRGB-encoded, like an imported PNG);
//!   float outputs are stored as 16-bit so they follow the same rule.
//!
//! Parameters are described with the engine-neutral `subcraft_doc::GraphParam`, and outputs
//! are matched to channels by `subcraft_doc::graph_material::channel_for_usage`.

use std::collections::BTreeMap;
use std::sync::Arc;

use subcraft_core::ChannelKind;
use subcraft_doc::graph_material::{GraphParam, GraphParamKind, channel_for_usage, usage_inverts};
use subcraft_image::{Bitmap, BitmapData};

/// Largest archive read.
pub const MAX_PACKAGE_BYTES: usize = 512 << 20;
/// Output sizes accepted, as log2 of the side: 16 to 4096 pixels.
pub const MIN_SIZE_LOG2: u32 = 4;
pub const MAX_SIZE_LOG2: u32 = 12;
/// Size a new Substance layer renders at (512 px): the CPU engine takes seconds per render.
pub const DEFAULT_SIZE_LOG2: u32 = 9;

/// Parameter values by input identifier. Integers and toggles are stored as whole numbers,
/// colours as 3 or 4 numbers in 0..1.
pub type Values = BTreeMap<String, Vec<f64>>;

#[derive(Debug, Clone, PartialEq)]
pub struct SbsarError(pub String);

impl std::fmt::Display for SbsarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SbsarError {}

impl From<open_sbsar::Error> for SbsarError {
    fn from(e: open_sbsar::Error) -> Self {
        SbsarError(e.0)
    }
}

/// What a graph offers.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphInfo {
    /// The graph's path inside the archive: its identity in saved files.
    pub path: String,
    pub label: String,
    pub params: Vec<GraphParam>,
    /// Channels it can fill, in output order.
    pub channels: Vec<ChannelKind>,
    /// Image inputs (mesh maps and such) the graph has, in input order.
    pub image_inputs: Vec<ImageInputInfo>,
    /// The grayscale output a mask generator uses (Painter's custom generators): an output
    /// named or tagged `mask` / `output`, else the only image output of a graph that fills no
    /// channel. `None`: the graph cannot be a generator.
    pub mask_output: Option<String>,
}

/// An image input of a graph.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageInputInfo {
    pub identifier: String,
    /// The label the author gave it (the identifier when there is none).
    pub label: String,
}

/// An image for an image input: raw values (data, never color-transformed), row-major,
/// `width * height * channels` of them. 1, 3 or 4 channels; normals OpenGL-style (green up),
/// as SubCraft bakes them.
#[derive(Clone, Debug, PartialEq)]
pub struct InputImage {
    pub width: u32,
    pub height: u32,
    pub channels: u32,
    pub data: Vec<f32>,
    /// A tangent-space normal map: green is flipped for graphs that take DirectX normals.
    pub normal: bool,
}

impl InputImage {
    /// The engine's form: gray stays gray, anything else becomes RGBA (alpha 1 when missing).
    fn to_engine(&self, flip_green: bool) -> Option<open_sbsar::InputImage> {
        let (w, h, ch) = (self.width as usize, self.height as usize, self.channels as usize);
        if w == 0 || h == 0 || !matches!(ch, 1 | 3 | 4) || self.data.len() != w * h * ch {
            return None;
        }
        let flip = flip_green && self.normal && ch >= 3;
        let data = if ch == 1 {
            self.data.clone()
        } else {
            let mut out = Vec::with_capacity(w * h * 4);
            for px in self.data.chunks_exact(ch) {
                out.extend_from_slice(&[px[0], if flip { 1.0 - px[1] } else { px[1] }, px[2], if ch == 4 { px[3] } else { 1.0 }]);
            }
            out
        };
        Some(open_sbsar::InputImage { width: w, height: h, channels: if ch == 1 { 1 } else { 4 }, data, storage: open_sbsar::PixelFormat::F32 })
    }
}

/// An opened archive. Cheap to share.
#[derive(Clone)]
pub struct Package {
    inner: Arc<open_sbsar::Package>,
    graphs: Arc<Vec<GraphInfo>>,
}

impl std::fmt::Debug for Package {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Package").field("graphs", &self.graphs).finish()
    }
}

/// For each channel, the output identifier that fills it and whether it is a fallback.
fn output_plan(graph: &open_sbsar::Graph) -> Vec<(ChannelKind, String, u8)> {
    let mut best: BTreeMap<ChannelKind, (u8, usize, String)> = BTreeMap::new();
    for (order, o) in graph.outputs().iter().enumerate().filter(|(_, o)| o.is_image) {
        let mut usages: Vec<&str> = o.usages.iter().map(String::as_str).collect();
        usages.push(&o.identifier);
        if let Some((ch, rank)) = usages.iter().find_map(|u| channel_for_usage(u)) {
            let better = best.get(&ch).is_none_or(|(r, _, _)| rank < *r);
            if better {
                best.insert(ch, (rank, order, o.identifier.clone()));
            }
        }
    }
    let mut plan: Vec<(usize, ChannelKind, String, u8)> = best.into_iter().map(|(ch, (rank, order, id))| (order, ch, id, rank)).collect();
    plan.sort_by_key(|p| p.0);
    plan.into_iter().map(|(_, ch, id, rank)| (ch, id, rank)).collect()
}

/// The output a mask generator reads (see [`GraphInfo::mask_output`]).
fn mask_output(graph: &open_sbsar::Graph, has_channels: bool) -> Option<String> {
    let images: Vec<&open_sbsar::OutputInfo> = graph.outputs().iter().filter(|o| o.is_image).collect();
    let tagged = images.iter().find(|o| {
        o.usages.iter().map(String::as_str).chain([o.identifier.as_str()]).any(|u| matches!(u.to_ascii_lowercase().as_str(), "mask" | "output" | "generator"))
    });
    match tagged {
        Some(o) => Some(o.identifier.clone()),
        None if !has_channels => images.first().map(|o| o.identifier.clone()),
        None => None,
    }
}

fn nums(s: &str) -> Vec<f64> {
    s.split(',').filter_map(|x| x.trim().parse().ok()).collect()
}

/// An input that switches an output on and off (Substance's "Channels" group).
fn is_channel_switch(i: &open_sbsar::InputInfo) -> bool {
    matches!(i.kind, open_sbsar::InputKind::Int(1))
        && i.widget.as_deref() == Some("togglebutton")
        && (i.group.as_deref().is_some_and(|g| g.eq_ignore_ascii_case("channels")) || i.identifier.starts_with("channel_"))
}

/// The `normal_format` combo and its OpenGL entry, if the graph has one.
fn normal_format(graph: &open_sbsar::Graph) -> Option<(String, i32)> {
    graph.inputs().iter().find_map(|i| {
        let named = i.identifier.eq_ignore_ascii_case("normal_format") || i.label.as_deref().is_some_and(|l| l.eq_ignore_ascii_case("normal format"));
        if !named {
            return None;
        }
        i.options.iter().find(|(_, t)| t.to_ascii_lowercase().contains("opengl")).map(|(v, _)| (i.identifier.clone(), *v))
    })
}

fn describe(graph: &open_sbsar::Graph) -> GraphInfo {
    use open_sbsar::InputKind;
    let nf = normal_format(graph);
    let mut params = Vec::new();
    let mut image_inputs = Vec::new();
    for i in graph.inputs() {
        if i.identifier == "$randomseed" {
            let d = i.default.as_deref().map(nums).and_then(|v| v.first().copied()).unwrap_or(0.0);
            params.push(GraphParam { id: i.identifier.clone(), label: "Random seed".into(), group: String::new(), kind: GraphParamKind::Seed, default: vec![d] });
            continue;
        }
        if i.identifier.starts_with('$') || is_channel_switch(i) || nf.as_ref().is_some_and(|(id, _)| *id == i.identifier) {
            continue;
        }
        let label = i.label.clone().filter(|l| !l.trim().is_empty()).unwrap_or_else(|| i.identifier.clone());
        let group = i.group.clone().unwrap_or_default();
        let (control, n) = match (&i.kind, i.widget.as_deref()) {
            (InputKind::Image, _) => {
                image_inputs.push(ImageInputInfo { identifier: i.identifier.clone(), label: label.clone() });
                continue;
            }
            (InputKind::String, _) | (InputKind::Other(_), _) => continue,
            (InputKind::Float(n @ (3 | 4)), Some("color")) => (GraphParamKind::Color { alpha: *n == 4, linear: false }, *n),
            (InputKind::Int(1), Some("togglebutton")) => (GraphParamKind::Bool, 1),
            (InputKind::Int(1), Some("combobox")) if !i.options.is_empty() => (GraphParamKind::Enum(i.options.clone()), 1),
            (InputKind::Float(n), _) => (GraphParamKind::Float { components: *n, min: i.min.clone().unwrap_or_else(|| vec![0.0]), max: i.max.clone().unwrap_or_else(|| vec![1.0]), step: 0.0 }, *n),
            (InputKind::Int(n), _) => (GraphParamKind::Int { components: *n, min: i.min.clone().unwrap_or_else(|| vec![0.0]), max: i.max.clone().unwrap_or_else(|| vec![10.0]) }, *n),
        };
        let mut default = i.default.as_deref().map(nums).unwrap_or_default();
        default.resize(n, default.last().copied().unwrap_or(0.0));
        params.push(GraphParam { id: i.identifier.clone(), label, group, kind: control, default });
    }
    let label = graph.label().filter(|l| !l.trim().is_empty()).map(str::to_string).unwrap_or_else(|| {
        let file = graph.path().rsplit('/').next().unwrap_or(graph.path());
        file.trim_end_matches(".sbsasm").to_string()
    });
    let channels: Vec<ChannelKind> = output_plan(graph).into_iter().map(|p| p.0).collect();
    let mask_output = mask_output(graph, !channels.is_empty());
    GraphInfo { path: graph.path().to_string(), label, params, channels, image_inputs, mask_output }
}

impl Package {
    pub fn from_bytes(bytes: &[u8]) -> Result<Package, SbsarError> {
        if bytes.len() > MAX_PACKAGE_BYTES {
            return Err(SbsarError(format!("{} MB is larger than the {} MB limit", bytes.len() >> 20, MAX_PACKAGE_BYTES >> 20)));
        }
        let inner = open_sbsar::Package::from_bytes(bytes)?;
        if inner.graphs().is_empty() {
            return Err(SbsarError("the archive has no graph".into()));
        }
        let graphs = inner.graphs().iter().map(describe).collect();
        Ok(Package { inner: Arc::new(inner), graphs: Arc::new(graphs) })
    }

    pub fn graphs(&self) -> &[GraphInfo] {
        &self.graphs
    }

    /// The graph at `path`, or the first one when `path` is not in the archive.
    pub fn graph(&self, path: &str) -> &GraphInfo {
        self.graphs.iter().find(|g| g.path == path).unwrap_or(&self.graphs[0])
    }

    fn engine_graph(&self, path: &str) -> &open_sbsar::Graph {
        let gs = self.inner.graphs();
        gs.iter().find(|g| g.path() == path).unwrap_or(&gs[0])
    }
}

/// One rendered channel map.
#[derive(Clone, Debug)]
pub struct Map {
    pub channel: ChannelKind,
    /// The output it came from.
    pub output: String,
    pub bitmap: Bitmap,
}

/// The result of one render.
#[derive(Clone, Debug)]
pub struct Rendered {
    pub maps: Vec<Map>,
    /// Inputs the engine ignored or features it does not support.
    pub warnings: Vec<String>,
}

/// Renders one graph again and again, recomputing only what changed between calls.
pub struct GraphRenderer {
    package: Package,
    path: String,
    renderer: open_sbsar::Renderer,
    instance: open_sbsar::Instance,
}

impl GraphRenderer {
    pub fn new(package: Package, path: &str) -> GraphRenderer {
        let graph = package.engine_graph(path);
        let path = graph.path().to_string();
        let instance = open_sbsar::Instance::new(graph);
        GraphRenderer { package, path, renderer: open_sbsar::Renderer::new_cpu(), instance }
    }

    pub fn package(&self) -> &Package {
        &self.package
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// Render every channel the graph fills at `2^size_log2` pixels (non-square outputs keep
    /// the graph's own aspect). Values not in `values` use the graph's defaults; unknown
    /// identifiers are ignored.
    pub fn render(&mut self, values: &Values, size_log2: u32) -> Result<Rendered, SbsarError> {
        self.render_with_images(values, size_log2, &BTreeMap::new())
    }

    /// [`Self::render`] with images for the graph's image inputs, by input identifier. Inputs
    /// with no image (or an unusable one) keep the graph's default; unknown identifiers are
    /// ignored.
    pub fn render_with_images(&mut self, values: &Values, size_log2: u32, images: &BTreeMap<String, InputImage>) -> Result<Rendered, SbsarError> {
        let graph = self.package.engine_graph(&self.path);
        let nf = normal_format(graph);
        let params = self.parameters(values, size_log2, images);
        let plan = output_plan(graph);
        let wanted: Vec<&str> = plan.iter().map(|p| p.1.as_str()).collect();
        if wanted.is_empty() {
            return Err(SbsarError("the graph has no output SubCraft can use (base color, normal, roughness, ...)".into()));
        }
        let outputs = self.instance.render(&mut self.renderer, graph, &params, Some(&wanted))?;
        let mut maps = Vec::new();
        for (channel, id, rank) in plan {
            let Some(out) = outputs.iter().find(|o| o.identifier == id) else { continue };
            let invert = usage_inverts(channel, rank);
            let flip_green = channel == ChannelKind::Normal && nf.is_none();
            maps.push(Map { channel, output: id, bitmap: to_bitmap(out, invert, flip_green) });
        }
        Ok(Rendered { maps, warnings: self.renderer.warnings.clone() })
    }

    /// Render the graph's mask output ([`GraphInfo::mask_output`]) as a one-channel bitmap
    /// (a color output becomes the average of red, green and blue).
    pub fn render_mask(&mut self, values: &Values, size_log2: u32, images: &BTreeMap<String, InputImage>) -> Result<(Bitmap, Vec<String>), SbsarError> {
        let graph = self.package.engine_graph(&self.path);
        let id = self.package.graph(&self.path).mask_output.clone().ok_or_else(|| SbsarError("the graph has no mask output (name one output \"mask\")".into()))?;
        let params = self.parameters(values, size_log2, images);
        let outputs = self.instance.render(&mut self.renderer, graph, &params, Some(&[id.as_str()]))?;
        let out = outputs.iter().find(|o| o.identifier == id).ok_or_else(|| SbsarError(format!("the graph did not render its output {id}")))?;
        Ok((to_gray(out), self.renderer.warnings.clone()))
    }

    /// The engine parameters for `values` at `2^size_log2`, with the image inputs set.
    fn parameters(&self, values: &Values, size_log2: u32, images: &BTreeMap<String, InputImage>) -> open_sbsar::Parameters {
        let graph = self.package.engine_graph(&self.path);
        let info = self.package.graph(&self.path);
        let size = size_log2.clamp(MIN_SIZE_LOG2, MAX_SIZE_LOG2) as i32;
        let mut params = open_sbsar::Parameters::new();
        params.set("$outputsize", open_sbsar::Value::Int(vec![size, size]));
        for p in &info.params {
            let v = values.get(&p.id).map(|v| p.clamp(v)).unwrap_or_else(|| p.default.clone());
            let value = match &p.kind {
                GraphParamKind::Color { .. } | GraphParamKind::Float { .. } => open_sbsar::Value::Float(v.iter().map(|x| *x as f32).collect()),
                _ => open_sbsar::Value::Int(v.iter().map(|x| *x as i32).collect()),
            };
            params.set(&p.id, value);
        }
        for i in graph.inputs().iter().filter(|i| is_channel_switch(i)) {
            params.set(&i.identifier, open_sbsar::Value::Int(vec![1]));
        }
        let nf = normal_format(graph);
        if let Some((id, v)) = &nf {
            params.set(id, open_sbsar::Value::Int(vec![*v]));
        }
        for input in &info.image_inputs {
            // A graph with no normal format switch takes DirectX normals (as it outputs them).
            if let Some(image) = images.get(&input.identifier).and_then(|i| i.to_engine(nf.is_none())) {
                params.set_image(&input.identifier, image);
            }
        }
        params
    }
}

/// An engine output as a one-channel 16-bit bitmap: gray stays, color is averaged (alpha
/// ignored). Mask values are data, never color-transformed.
fn to_gray(out: &open_sbsar::OutputImage) -> Bitmap {
    let ch = out.channels.max(1);
    let vals = out.as_f32();
    let take = ch.min(3);
    let data = vals.chunks_exact(ch).map(|px| ((px[..take].iter().sum::<f32>() / take as f32).clamp(0.0, 1.0) * 65535.0).round() as u16).collect();
    Bitmap { width: out.width as u32, height: out.height as u32, channels: 1, data: BitmapData::U16(data) }
}

/// An engine output as a bitmap that keeps its encoded values. 8-bit outputs stay 8-bit, all
/// others become 16-bit. `invert` gives 1 - v (glossiness to roughness); `flip_green` turns a
/// DirectX normal into an OpenGL one.
fn to_bitmap(out: &open_sbsar::OutputImage, invert: bool, flip_green: bool) -> Bitmap {
    let ch = out.channels;
    let fix = |i: usize, v: f32| {
        let c = i % ch;
        let mut v = v.clamp(0.0, 1.0);
        let alpha = ch == 4 && c == 3;
        if invert && !alpha {
            v = 1.0 - v;
        }
        if flip_green && ch == 4 && c == 1 {
            v = 1.0 - v;
        }
        v
    };
    let vals = out.as_f32();
    let (w, h) = (out.width as u32, out.height as u32);
    if out.format == open_sbsar::PixelFormat::U8 {
        let data = vals.iter().enumerate().map(|(i, &v)| (fix(i, v) * 255.0).round() as u8).collect();
        Bitmap::new_u8(w, h, ch as u8, data)
    } else {
        let data = vals.iter().enumerate().map(|(i, &v)| (fix(i, v) * 65535.0).round() as u16).collect();
        Bitmap { width: w, height: h, channels: ch as u8, data: BitmapData::U16(data) }
    }
}

/// Check that `bytes` is an archive SubCraft can use, and describe it.
pub fn inspect(bytes: &[u8]) -> Result<Package, SbsarError> {
    let p = Package::from_bytes(bytes)?;
    if p.graphs().iter().all(|g| g.channels.is_empty() && g.mask_output.is_none()) {
        return Err(SbsarError("no graph in the archive has an output SubCraft can use (base color, normal, roughness, mask, ...)".into()));
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_images_become_gray_or_rgba_and_normals_follow_the_graph() {
        let gray = InputImage { width: 2, height: 1, channels: 1, data: vec![0.25, 0.75], normal: false };
        let e = gray.to_engine(true).unwrap();
        assert_eq!((e.channels, e.data.clone(), e.storage), (1, vec![0.25, 0.75], open_sbsar::PixelFormat::F32));
        let normal = InputImage { width: 1, height: 1, channels: 3, data: vec![0.5, 0.8, 1.0], normal: true };
        // OpenGL graph: kept; DirectX graph: green flipped. Alpha is added.
        assert_eq!(normal.to_engine(false).unwrap().data, vec![0.5, 0.8, 1.0, 1.0]);
        let dx = normal.to_engine(true).unwrap().data;
        assert!((dx[1] - 0.2).abs() < 1e-6 && dx[3] == 1.0);
        // Data maps are never flipped.
        let pos = InputImage { normal: false, ..normal.clone() };
        assert_eq!(pos.to_engine(true).unwrap().data, vec![0.5, 0.8, 1.0, 1.0]);
        // Broken sizes are skipped, not passed on.
        assert!(InputImage { width: 3, ..gray.clone() }.to_engine(false).is_none());
        assert!(InputImage { channels: 2, data: vec![0.0; 4], ..gray }.to_engine(false).is_none());
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(Package::from_bytes(b"").is_err());
        assert!(Package::from_bytes(b"7z\xbc\xaf\x27\x1c not really").is_err());
        assert!(Package::from_bytes(&[0u8; 4096]).is_err());
    }
}
