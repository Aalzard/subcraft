//! `paint/document.bin`: Painter's serialized project model (texture sets, channels, layer stack).
//!
//! The stream is little endian: magic `0x1B7C2FDD`, a version, three counts, then one root object.
//! An object is `id: u32`, a class word and the class's fields in schema order. A class word is
//! `0xFFFFFFFF` followed by the class definition (name, property count, `(name, type)` pairs) the
//! first time the class occurs, and its index in definition order afterwards. Property types (found
//! by decoding all sample files to their exact end): 1 f32, 2/3/4 float vectors, 5 i32, 6 two i32,
//! 9 i32 (enums), 10 bool byte, 11 u32, 12 u64, 13 3x3 matrix, 14 4x4 matrix, 15 two u32, 16 string
//! (u32 length + UTF-8), 17 list of embedded structs (count, then one class word, then the
//! structs without ids), 18 and 20 object (id 0 = null), 19 list of objects, 21 u64 (flags).

use std::collections::HashSet;

#[derive(Debug, thiserror::Error)]
pub enum DocumentError {
    #[error("not a Painter document (bad magic)")]
    BadMagic,
    #[error("unsupported document version {0}")]
    Version(u32),
    #[error("document ended early or has an unknown layout near byte {0}")]
    Corrupt(usize),
}

#[derive(Debug, Clone)]
pub enum Val {
    Float(Vec<f32>),
    Int(i64),
    Bool(bool),
    Str(String),
    Obj(Option<Box<Obj>>),
    List(Vec<Obj>),
    /// A list of bulk objects (stroke samples, grid vertices) that is only counted.
    Count(usize),
}

#[derive(Debug, Clone)]
pub struct Obj {
    pub class: String,
    pub id: u32,
    pub fields: Vec<(String, Val)>,
}

impl Obj {
    pub fn get(&self, name: &str) -> Option<&Val> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }
    pub fn int(&self, name: &str) -> Option<i64> {
        match self.get(name)? {
            Val::Int(v) => Some(*v),
            _ => None,
        }
    }
    pub fn float(&self, name: &str) -> Option<f32> {
        match self.get(name)? {
            Val::Float(v) => v.first().copied(),
            _ => None,
        }
    }
    pub fn floats(&self, name: &str) -> Option<&[f32]> {
        match self.get(name)? {
            Val::Float(v) => Some(v),
            _ => None,
        }
    }
    pub fn bool(&self, name: &str) -> Option<bool> {
        match self.get(name)? {
            Val::Bool(v) => Some(*v),
            _ => None,
        }
    }
    pub fn str(&self, name: &str) -> Option<&str> {
        match self.get(name)? {
            Val::Str(v) => Some(v),
            _ => None,
        }
    }
    pub fn obj(&self, name: &str) -> Option<&Obj> {
        match self.get(name)? {
            Val::Obj(Some(o)) => Some(o),
            _ => None,
        }
    }
    pub fn list(&self, name: &str) -> &[Obj] {
        match self.get(name) {
            Some(Val::List(v)) => v,
            _ => &[],
        }
    }
    pub fn count(&self, name: &str) -> usize {
        match self.get(name) {
            Some(Val::Count(n)) => *n,
            Some(Val::List(v)) => v.len(),
            _ => 0,
        }
    }
}

struct Class {
    name: String,
    props: Vec<(String, u32)>,
    bulk: bool,
}

struct Parser<'a> {
    d: &'a [u8],
    pos: usize,
    classes: Vec<Class>,
    depth: usize,
}

/// Classes with huge repeated payloads (brush strokes, deformation grids) that are only counted.
const BULK: [&str; 3] = ["DataStroke", "DataStrokeKey", "DataGridVertex"];

type Res<T> = Result<T, DocumentError>;

impl<'a> Parser<'a> {
    fn bad<T>(&self) -> Res<T> {
        Err(DocumentError::Corrupt(self.pos))
    }
    fn take(&mut self, n: usize) -> Res<&'a [u8]> {
        match self.d.get(self.pos..self.pos.wrapping_add(n)) {
            Some(s) if self.pos.checked_add(n).is_some() => {
                self.pos += n;
                Ok(s)
            }
            _ => self.bad(),
        }
    }
    fn u32(&mut self) -> Res<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn u64(&mut self) -> Res<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn floats(&mut self, n: usize) -> Res<Vec<f32>> {
        Ok(self.take(n * 4)?.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
    }
    fn string(&mut self) -> Res<String> {
        let n = self.u32()? as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }

    fn class_word(&mut self) -> Res<usize> {
        let word = self.u32()?;
        if word != u32::MAX {
            return if (word as usize) < self.classes.len() { Ok(word as usize) } else { self.bad() };
        }
        let name = self.string()?;
        let n = self.u32()? as usize;
        if n > 512 {
            return self.bad();
        }
        let mut props = Vec::with_capacity(n);
        for _ in 0..n {
            let pn = self.string()?;
            let pt = self.u32()?;
            props.push((pn, pt));
        }
        let bulk = BULK.contains(&name.as_str());
        self.classes.push(Class { name, props, bulk });
        Ok(self.classes.len() - 1)
    }

    fn object(&mut self) -> Res<Obj> {
        let id = self.u32()?;
        let class = self.class_word()?;
        self.fields(class, id)
    }

    fn reference(&mut self) -> Res<Option<Box<Obj>>> {
        if self.d.get(self.pos..self.pos + 4) == Some(&[0, 0, 0, 0]) {
            self.pos += 4;
            return Ok(None);
        }
        Ok(Some(Box::new(self.object()?)))
    }

    fn fields(&mut self, class: usize, id: u32) -> Res<Obj> {
        self.depth += 1;
        if self.depth > 200 {
            return self.bad();
        }
        let props = self.classes[class].props.clone();
        let bulk = self.classes[class].bulk;
        let name = self.classes[class].name.clone();
        let mut fields = Vec::with_capacity(if bulk { 0 } else { props.len() });
        for (pn, pt) in props {
            let v = self.value(pt)?;
            if !bulk {
                fields.push((pn, v));
            }
        }
        self.depth -= 1;
        Ok(Obj { class: name, id, fields })
    }

    fn value(&mut self, t: u32) -> Res<Val> {
        Ok(match t {
            1 => Val::Float(self.floats(1)?),
            2 => Val::Float(self.floats(2)?),
            3 => Val::Float(self.floats(3)?),
            4 => Val::Float(self.floats(4)?),
            13 => Val::Float(self.floats(9)?),
            14 => Val::Float(self.floats(16)?),
            5 | 9 => Val::Int(self.u32()? as i32 as i64),
            6 => {
                let (a, b) = (self.u32()? as i32, self.u32()? as i32);
                Val::Float(vec![a as f32, b as f32])
            }
            10 => Val::Bool(self.take(1)?[0] != 0),
            11 => Val::Int(self.u32()? as i64),
            15 => {
                let (a, b) = (self.u32()?, self.u32()?);
                Val::Float(vec![a as f32, b as f32])
            }
            12 | 21 => Val::Int(self.u64()? as i64),
            16 => Val::Str(self.string()?),
            18 | 20 => Val::Obj(self.reference()?),
            19 => {
                let n = self.u32()? as usize;
                if n > self.d.len() {
                    return self.bad();
                }
                let mut v = Vec::with_capacity(n.min(1024));
                for _ in 0..n {
                    if let Some(o) = self.reference()? {
                        v.push(*o);
                    }
                }
                Val::List(v)
            }
            17 => {
                let n = self.u32()? as usize;
                if n == 0 {
                    return Ok(Val::List(Vec::new()));
                }
                if n > self.d.len() {
                    return self.bad();
                }
                let class = self.class_word()?;
                if self.classes[class].bulk {
                    for _ in 0..n {
                        self.fields(class, 0)?;
                    }
                    return Ok(Val::Count(n));
                }
                let mut v = Vec::with_capacity(n.min(1024));
                for _ in 0..n {
                    v.push(self.fields(class, 0)?);
                }
                Val::List(v)
            }
            _ => return self.bad(),
        })
    }
}

/// Parse the document and return its root object.
pub fn parse(data: &[u8]) -> Result<Obj, DocumentError> {
    if data.len() < 20 || data[..4] != 0x1b7c_2fddu32.to_le_bytes() {
        return Err(DocumentError::BadMagic);
    }
    let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    if version != 1 {
        return Err(DocumentError::Version(version));
    }
    let mut p = Parser { d: data, pos: 20, classes: Vec::new(), depth: 0 };
    let root = p.object()?;
    if p.pos != data.len() {
        return Err(DocumentError::Corrupt(p.pos));
    }
    Ok(root)
}

// ----- the project as the app needs it --------------------------------------------------------------

/// Painter's channel type flags. Verified against projects: the flag of each channel in a texture
/// set equals the key of its entry in `texture-cache`.
pub mod channel_bits {
    pub const BASE_COLOR: u64 = 1;
    pub const HEIGHT: u64 = 2;
    pub const OPACITY: u64 = 8;
    pub const EMISSIVE: u64 = 16;
    pub const ROUGHNESS: u64 = 128;
    pub const METALLIC: u64 = 1 << 13;
    pub const NORMAL: u64 = 1 << 22;
    pub const AMBIENT_OCCLUSION: u64 = 1 << 23;
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChannelInfo {
    pub bits: u64,
    pub format: i32,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerKind {
    Fill,
    Paint,
    Filter,
    Group,
    Empty,
}

/// A procedural graph (`.sbsar`) with the values Painter set on it.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphRef {
    /// Resource URL including `?version=<sha1>.sbsar`.
    pub url: String,
    /// Exposed inputs that were set: `(identifier, values)`; ints, floats and vectors all as floats.
    pub tweaks: Vec<(String, Vec<f32>)>,
}

impl GraphRef {
    /// The graph's name: the second to last path segment of its URL (`starter_assets/mg_dirt/mg_dirt` is `mg_dirt`).
    pub fn name(&self) -> &str {
        let path = self.url.split('?').next().unwrap_or(&self.url);
        let mut parts = path.rsplit('/');
        let last = parts.next().unwrap_or("");
        parts.next().unwrap_or(last)
    }

    /// The first value of a tweak.
    pub fn value(&self, identifier: &str) -> Option<f32> {
        self.tweaks.iter().find(|(n, _)| n.eq_ignore_ascii_case(identifier)).and_then(|(_, v)| v.first().copied())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SourceInfo {
    Uniform { channels: u64, color: [f32; 3] },
    Bitmap { channels: u64, url: String },
    Procedural { channels: u64, graph: GraphRef },
}

impl SourceInfo {
    pub fn channels(&self) -> u64 {
        match self {
            SourceInfo::Uniform { channels, .. } | SourceInfo::Bitmap { channels, .. } | SourceInfo::Procedural { channels, .. } => *channels,
        }
    }
}

/// Painter's per-channel Levels.
#[derive(Debug, Clone, PartialEq)]
pub struct LevelsInfo {
    pub clamp: bool,
    pub gamma: [f32; 3],
    pub in_min: [f32; 3],
    pub in_max: [f32; 3],
    pub out_min: [f32; 3],
    pub out_max: [f32; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActionKind {
    Fill { sources: Vec<SourceInfo>, projection: i64 },
    /// A procedural filter such as blur or sharpen.
    Filter { graph: GraphRef },
    Levels(LevelsInfo),
    Paint { strokes: usize },
    Eraser { strokes: usize },
    Decal { hits: usize },
    Anchor,
    Other(String),
}

/// One step of a layer's content or mask stack.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionInfo {
    pub kind: ActionKind,
    pub enabled: bool,
    pub label: String,
    /// Channels the action applies to (flags; all bits set = every channel).
    pub channels: u64,
    /// Blending of the enclosing action group: `(channel flags, mode id, opacity)`.
    pub blends: Vec<(u64, i32, f32)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayerInfo {
    pub name: String,
    pub kind: LayerKind,
    pub visible: bool,
    /// `(channel flags, blending mode id, opacity)` as stored (mode ids are Painter's own enum).
    pub blends: Vec<(u64, i32, f32)>,
    pub has_mask: bool,
    pub sources: Vec<SourceInfo>,
    pub strokes: usize,
    /// Content actions, bottom to top.
    pub content: Vec<ActionInfo>,
    /// Mask actions, bottom to top; `mask_initial_white` is the mask's starting value.
    pub mask: Vec<ActionInfo>,
    pub mask_enabled: bool,
    pub mask_initial_white: bool,
    pub children: Vec<LayerInfo>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextureSetInfo {
    pub name: String,
    pub uid: u64,
    pub width_log2: i32,
    pub height_log2: i32,
    pub channels: Vec<ChannelInfo>,
    /// Bottom-to-top.
    pub layers: Vec<LayerInfo>,
    /// `(usage id, resource url)` of the baked mesh maps.
    pub mesh_maps: Vec<(i32, String)>,
    /// The document's `normalMapSyscoord` (0 in every project seen; believed to mean DirectX normals).
    pub normal_syscoord: i64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProjectInfo {
    pub texture_sets: Vec<TextureSetInfo>,
}

fn blends_of(o: &Obj) -> Vec<(u64, i32, f32)> {
    o.list("perChannelBlending")
        .iter()
        .map(|b| (b.int("channelTypes").unwrap_or(-1) as u64, b.int("blendingMode").unwrap_or(0) as i32, b.float("opacity").unwrap_or(1.0)))
        .collect()
}

/// Flatten an action list (descending into action groups); each action gets its group's blending.
fn collect_actions<'a>(stack: Option<&'a Obj>, out: &mut Vec<(&'a Obj, Vec<(u64, i32, f32)>)>, blends: &mut Vec<(u64, i32, f32)>, group: &[(u64, i32, f32)]) {
    let Some(stack) = stack else { return };
    for a in stack.list("items") {
        if a.class == "DataActionGroup" {
            let own = blends_of(a);
            if blends.is_empty() {
                *blends = own.clone();
            }
            collect_actions(a.obj("subStack"), out, blends, &own);
        } else {
            out.push((a, group.to_vec()));
        }
    }
}

fn graph_of(p: &Obj) -> Option<GraphRef> {
    let url = p.str("urlToSbsRes")?.to_string();
    let mut tweaks = Vec::new();
    for t in p.list("tweaks") {
        let Some(id) = t.str("identifier") else { continue };
        let values = if let Some(f) = t.floats("value") {
            f.to_vec()
        } else if let Some(i) = t.int("value") {
            vec![i as f32]
        } else {
            continue;
        };
        tweaks.push((id.to_string(), values));
    }
    Some(GraphRef { url, tweaks })
}

fn source_of(s: &Obj) -> Option<SourceInfo> {
    let channels = s.int("channelTypes").unwrap_or(-1) as u64;
    match s.class.as_str() {
        "DataSourceUniform" => {
            let c = s.floats("color")?;
            Some(SourceInfo::Uniform { channels, color: [*c.first()?, *c.get(1)?, *c.get(2)?] })
        }
        "DataSourceBitmap" => Some(SourceInfo::Bitmap { channels, url: s.obj("bitmap")?.str("urlToBitmapRes")?.to_string() }),
        "DataSourceProcedural" => Some(SourceInfo::Procedural { channels, graph: graph_of(s.obj("procedural")?)? }),
        _ => None,
    }
}

fn vec3(o: &Obj, name: &str, default: f32) -> [f32; 3] {
    match o.floats(name) {
        Some(v) if v.len() >= 3 => [v[0], v[1], v[2]],
        _ => [default; 3],
    }
}

fn action_of(a: &Obj, blends: Vec<(u64, i32, f32)>) -> ActionInfo {
    let kind = match a.class.as_str() {
        "DataActionFill" => ActionKind::Fill { sources: a.list("sources").iter().filter_map(source_of).collect(), projection: a.int("projection").unwrap_or(0) },
        "DataActionFilterProcedural" => match a.obj("procedural").and_then(graph_of) {
            Some(graph) => ActionKind::Filter { graph },
            None => ActionKind::Other(a.class.clone()),
        },
        "DataActionFilterLevels" => match a.obj("levels") {
            Some(l) => ActionKind::Levels(LevelsInfo {
                clamp: l.int("clamp").unwrap_or(0) != 0 || l.bool("clamp").unwrap_or(false),
                gamma: vec3(l, "gamma", 1.0),
                in_min: vec3(l, "inputMinimum", 0.0),
                in_max: vec3(l, "inputMaximum", 1.0),
                out_min: vec3(l, "outputMinimum", 0.0),
                out_max: vec3(l, "outputMaximum", 1.0),
            }),
            None => ActionKind::Other(a.class.clone()),
        },
        "DataActionPaint" => ActionKind::Paint { strokes: a.count("strokes") + a.count("strokes3D") },
        "DataActionEraser" => ActionKind::Eraser { strokes: a.count("strokes") + a.count("strokes3D") },
        "DataActionDecal" => ActionKind::Decal { hits: a.count("hits") },
        "DataActionAnchor" => ActionKind::Anchor,
        other => ActionKind::Other(other.to_string()),
    };
    ActionInfo { kind, enabled: a.bool("enabled").unwrap_or(true), label: a.str("label").unwrap_or("").to_string(), channels: a.int("channelTypes").unwrap_or(-1) as u64, blends }
}

fn layer_of(l: &Obj) -> LayerInfo {
    let group = l.class == "DataLayerGroup";
    let mut actions = Vec::new();
    let mut group_blends = Vec::new();
    collect_actions(l.obj("actions"), &mut actions, &mut group_blends, &[]);
    let mut mask_actions = Vec::new();
    collect_actions(l.obj("maskActions"), &mut mask_actions, &mut Vec::new(), &[]);
    let mut kind = if group { LayerKind::Group } else { LayerKind::Empty };
    let mut sources = Vec::new();
    let mut strokes = 0;
    for (a, _) in &actions {
        match a.class.as_str() {
            "DataActionFill" => {
                if kind == LayerKind::Empty {
                    kind = LayerKind::Fill;
                }
                sources.extend(a.list("sources").iter().filter_map(source_of));
            }
            "DataActionPaint" => {
                if kind == LayerKind::Empty || kind == LayerKind::Filter {
                    kind = LayerKind::Paint;
                }
                strokes += a.count("strokes") + a.count("strokes3D");
            }
            "DataActionFilterProcedural" => {
                if kind == LayerKind::Empty {
                    kind = LayerKind::Filter;
                }
                if let Some(graph) = a.obj("procedural").and_then(graph_of) {
                    sources.push(SourceInfo::Procedural { channels: a.int("channelTypes").unwrap_or(-1) as u64, graph });
                }
            }
            _ => {}
        }
    }
    let mut blends = blends_of(l);
    if blends.is_empty() {
        blends = group_blends;
    }
    let children = if group { l.obj("subStack").map(|s| s.list("items").iter().map(layer_of).collect()).unwrap_or_default() } else { Vec::new() };
    LayerInfo {
        name: l.str("label").unwrap_or("").to_string(),
        kind,
        visible: l.bool("enabled").unwrap_or(true),
        blends,
        has_mask: l.obj("maskActions").is_some(),
        sources,
        strokes,
        content: actions.into_iter().map(|(a, b)| action_of(a, b)).collect(),
        mask: mask_actions.into_iter().map(|(a, b)| action_of(a, b)).collect(),
        mask_enabled: l.bool("maskEnabled").unwrap_or(true),
        mask_initial_white: l.int("maskInitial").unwrap_or(1) != 0,
        children,
    }
}

/// Extract the texture sets from a parsed document.
pub fn project_info(root: &Obj) -> ProjectInfo {
    let normal_syscoord = root.int("normalMapSyscoord").unwrap_or(0);
    let mut texture_sets = Vec::new();
    for m in root.list("materials") {
        let Some(stack) = m.list("stacks").first() else { continue };
        let res = m.floats("resolutionLog2").map(|v| (v[0] as i32, v.get(1).copied().unwrap_or(v[0]) as i32)).unwrap_or((11, 11));
        texture_sets.push(TextureSetInfo {
            name: m.str("sceneMaterialName").unwrap_or("").to_string(),
            uid: m.int("sceneMaterialUID").unwrap_or(0) as u64,
            width_log2: res.0,
            height_log2: res.1,
            channels: stack
                .list("channels")
                .iter()
                .map(|c| ChannelInfo { bits: c.int("type").unwrap_or(0) as u64, format: c.int("format").unwrap_or(0) as i32, name: c.str("userName").unwrap_or("").to_string() })
                .collect(),
            layers: stack.obj("stack").map(|s| s.list("items").iter().map(layer_of).collect()).unwrap_or_default(),
            mesh_maps: m.list("additionalMaps").iter().filter_map(|a| Some((a.int("usage")? as i32, a.str("urlToBitmapRes")?.to_string()))).collect(),
            normal_syscoord,
        });
    }
    ProjectInfo { texture_sets }
}

/// All layers of a stack, depth first (groups before their children).
pub fn walk_layers<'a>(layers: &'a [LayerInfo], f: &mut dyn FnMut(&'a LayerInfo, usize), depth: usize) {
    for l in layers {
        f(l, depth);
        walk_layers(&l.children, f, depth + 1);
    }
}

/// Distinct procedural (`.sbsar`) resources a stack relies on.
pub fn procedural_resources(layers: &[LayerInfo]) -> HashSet<String> {
    let mut set = HashSet::new();
    walk_layers(layers, &mut |l, _| {
        for s in &l.sources {
            if let SourceInfo::Procedural { graph, .. } = s {
                set.insert(graph.url.split('?').next().unwrap_or(&graph.url).to_string());
            }
        }
    }, 0);
    set
}
