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
    /// Integer pairs (type 15), kept exact (uids exceed f32 precision).
    Ints(Vec<i64>),
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
    pub fn ints(&self, name: &str) -> Option<&[i64]> {
        match self.get(name)? {
            Val::Ints(v) => Some(v),
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

/// Classes with huge repeated payloads (deformation grids) that are only counted.
const BULK: [&str; 1] = ["DataGridVertex"];

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
                Val::Ints(vec![a as i64, b as i64])
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
    /// The mask (or channel) of the stack as it is at an anchor point (`ActionKind::Anchor` uid),
    /// through Levels when they are not the identity.
    Reference { channels: u64, anchor: i64, levels: Option<LevelsInfo> },
}

impl SourceInfo {
    pub fn channels(&self) -> u64 {
        match self {
            SourceInfo::Uniform { channels, .. } | SourceInfo::Bitmap { channels, .. } | SourceInfo::Procedural { channels, .. } | SourceInfo::Reference { channels, .. } => *channels,
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

/// One polygon-fill selection (`DataDecalHitPunctual` / `DataDecalHitRect`): a click or a rectangle
/// in the viewport, kept as screen positions (0..1, origin top-left) and the inverse of the
/// view-projection matrix at the time, so the selected triangles are found again by ray casting.
#[derive(Debug, Clone, PartialEq)]
pub struct DecalHit {
    /// Value written (1 fills, 0 clears).
    pub value: f32,
    /// What a hit selects: 0 triangle, 1 polygon, 2 mesh element, 3 UV island (Painter's polygon fill modes).
    pub granularity: i64,
    /// One point for a click, two corners for a rectangle.
    pub points: Vec<[f32; 2]>,
    /// Inverse view-projection, column major (OpenGL clip space).
    pub proj_view_inv: [f32; 16],
    /// Paint-through: a click also selects what is hidden behind the first surface.
    pub through: bool,
    /// Made in the 2D view: `points` are UV coordinates (origin bottom-left).
    pub uv_space: bool,
    /// Symmetry copies: the hit is also applied through `duplicate` (object or UV space).
    pub duplicate: Option<[f32; 16]>,
}

/// One brush stroke as recorded: screen positions (0..1, origin top-left) and the camera.
#[derive(Debug, Clone, PartialEq)]
pub struct StrokeInfo {
    /// Painted in Painter's 2D view: `points` are UV coordinates (origin bottom-left) and the
    /// matrices are zero.
    pub uv_space: bool,
    pub points: Vec<[f32; 2]>,
    pub pressure: Vec<f32>,
    pub view: [f32; 16],
    pub projection: [f32; 16],
    pub seed: u32,
    /// Symmetry copy of the stroke: object-space (3D) or UV-space (2D view) transform.
    pub duplicate: Option<[f32; 16]>,
}

/// The brush of a paint or eraser action.
#[derive(Debug, Clone, PartialEq)]
pub struct BrushInfo {
    /// Brush size; `size_space` says relative to what (see [`StrokeInfo`]).
    pub size: f32,
    pub size_space: i64,
    pub flow: f32,
    pub spacing: f32,
    pub pressure_size: bool,
    /// Value painted into a mask (the first uniform color source), 1 when absent.
    pub value: f32,
    /// Hardness of the procedural `shape` tip when it is one.
    pub hardness: Option<f32>,
    /// Name of the tip graph (`shape`, `brush_paint_*`...), empty for none.
    pub tip: String,
    /// Painter's alignment id (0 tangent wrap, 1 tangent planar, 2 camera, 3 UV, as SubCraft's).
    pub alignment: i64,
    pub culling: bool,
    /// Screen-space stencil: the image resource URL and the action's 3x3 `projMatrix` (column
    /// major) taking clip-space screen positions (-1..1, y up) to stencil UV (v up).
    pub stencil: Option<(String, [f32; 9])>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActionKind {
    Fill { sources: Vec<SourceInfo>, projection: i64 },
    /// A procedural filter such as blur or sharpen.
    Filter { graph: GraphRef },
    Levels(LevelsInfo),
    Paint { strokes: Vec<StrokeInfo>, brush: Option<BrushInfo>, opacity: f32 },
    Eraser { strokes: Vec<StrokeInfo>, brush: Option<BrushInfo>, opacity: f32 },
    Decal { hits: Vec<DecalHit> },
    /// A named point in the stack that `SourceInfo::Reference` reads (by `uid`).
    Anchor { uid: i64 },
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
    /// Which action group encloses the action (0 = none; the same number for the whole group).
    pub group: usize,
    /// The action's own blending (mask effects keep theirs here); empty = the default.
    pub own_blends: Vec<(u64, i32, f32)>,
    /// A fill's UV transformation: tiling scale, rotation (degrees as stored), offset.
    pub uv: Option<UvTransform>,
}

/// `Data2DTransformation` of a fill.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UvTransform {
    pub scale: [f32; 2],
    pub rotation: f32,
    pub offset: [f32; 2],
    /// The fill repeats its image (`uvSamplingWrap` 3); 0 places it once.
    pub tiling: bool,
}

impl LevelsInfo {
    pub fn is_identity(&self) -> bool {
        self.gamma == [1.0; 3] && self.in_min == [0.0; 3] && self.in_max == [1.0; 3] && self.out_min == [0.0; 3] && self.out_max == [1.0; 3]
    }
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
    /// Geometry mask (`geometryMaskType` 1): whether meshes are enabled by default, and the mesh
    /// object names (`enabledMeshList`) that are the exception. `None`: the layer covers every mesh.
    pub mesh_filter: Option<(bool, Vec<String>)>,
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
type Collected<'a> = Vec<(&'a Obj, Vec<(u64, i32, f32)>, usize)>;

fn collect_actions<'a>(stack: Option<&'a Obj>, out: &mut Collected<'a>, blends: &mut Vec<(u64, i32, f32)>, group: (&[(u64, i32, f32)], usize), groups: &mut usize) {
    let Some(stack) = stack else { return };
    for a in stack.list("items") {
        if a.class == "DataActionGroup" {
            let own = blends_of(a);
            if blends.is_empty() {
                *blends = own.clone();
            }
            *groups += 1;
            let id = *groups;
            collect_actions(a.obj("subStack"), out, blends, (&own, id), groups);
        } else {
            out.push((a, group.0.to_vec(), group.1));
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
        "DataSourceReference" => Some(SourceInfo::Reference { channels, anchor: *s.obj("reference")?.ints("source")?.first()?, levels: s.obj("levels").map(levels_info).filter(|l| !l.is_identity()) }),
        _ => None,
    }
}

fn vec3(o: &Obj, name: &str, default: f32) -> [f32; 3] {
    match o.floats(name) {
        Some(v) if v.len() >= 3 => [v[0], v[1], v[2]],
        _ => [default; 3],
    }
}

fn mat16(o: &Obj, name: &str) -> Option<[f32; 16]> {
    o.floats(name)?.try_into().ok()
}

/// The symmetry transform of a stroke or hit with `duplicateCount` 1 (more copies are not seen).
fn duplicate_of(o: &Obj) -> Option<[f32; 16]> {
    (o.int("duplicateCount").unwrap_or(0) > 0).then(|| mat16(o, "duplicateMatrix")).flatten()
}

fn decal_hit_of(h: &Obj) -> Option<DecalHit> {
    let points = match h.class.as_str() {
        "DataDecalHitPunctual" => vec![h.floats("position")?.try_into().ok()?],
        "DataDecalHitRect" => vec![h.floats("position0")?.try_into().ok()?, h.floats("position1")?.try_into().ok()?],
        _ => return None,
    };
    Some(DecalHit { value: h.floats("color").and_then(|c| c.first().copied()).unwrap_or(1.0), granularity: h.int("granularity").unwrap_or(2), points, proj_view_inv: mat16(h, "projViewInvMatrix")?, through: h.int("paintThrough").unwrap_or(0) != 0 || h.bool("paintThrough").unwrap_or(false), uv_space: h.int("viewType") == Some(0), duplicate: duplicate_of(h) })
}

fn strokes_of(a: &Obj) -> Vec<StrokeInfo> {
    a.list("strokes")
        .iter()
        .filter_map(|s| {
            Some(StrokeInfo {
                uv_space: s.int("viewType") == Some(0),
                points: s.list("points").iter().filter_map(|k| k.floats("position")?.try_into().ok()).collect(),
                pressure: match s.get("pressure") {
                    Some(Val::Float(v)) => v.clone(),
                    Some(Val::List(v)) => v.iter().filter_map(|p| p.float("value")).collect(),
                    _ => Vec::new(),
                },
                view: mat16(s, "viewMatrix")?,
                projection: mat16(s, "projectionMatrix")?,
                seed: s.int("randomSeed").unwrap_or(0) as u32,
                duplicate: duplicate_of(s),
            })
        })
        .collect()
}

fn brush_of(a: &Obj) -> Option<BrushInfo> {
    let b = a.obj("brush")?;
    let emitter = b.obj("stampsEmitter");
    let value = a.list("sourceColor").iter().find(|s| s.class == "DataSourceUniform").and_then(|s| s.floats("color")?.first().copied()).unwrap_or(1.0);
    let tip = a.list("sourceTransparent").iter().find_map(|s| graph_of(s.obj("procedural")?));
    Some(BrushInfo {
        size: b.float("brushSize").unwrap_or(0.1),
        size_space: b.int("brushSizeSpace").unwrap_or(1),
        flow: b.float("flow").unwrap_or(1.0),
        spacing: emitter.and_then(|e| e.float("spacing")).unwrap_or(0.2),
        pressure_size: emitter.and_then(|e| e.bool("pressureSizeEnabled")).unwrap_or(false),
        value,
        hardness: tip.as_ref().and_then(|g| g.value("hardness")),
        tip: tip.as_ref().map(|g| g.name().to_string()).unwrap_or_default(),
        alignment: b.int("alignment").unwrap_or(0),
        culling: b.int("cullingEnabled").map(|v| v != 0).or(b.bool("cullingEnabled")).unwrap_or(true),
        stencil: a.obj("sourceStencil").filter(|s| s.class == "DataSourceBitmap").and_then(|s| {
            let url = s.obj("bitmap")?.str("urlToBitmapRes")?.to_string();
            let m: [f32; 9] = a.floats("projMatrix")?.try_into().ok()?;
            Some((url, m))
        }),
    })
}

fn levels_info(l: &Obj) -> LevelsInfo {
    LevelsInfo {
        clamp: l.int("clamp").unwrap_or(0) != 0 || l.bool("clamp").unwrap_or(false),
        gamma: vec3(l, "gamma", 1.0),
        in_min: vec3(l, "inputMinimum", 0.0),
        in_max: vec3(l, "inputMaximum", 1.0),
        out_min: vec3(l, "outputMinimum", 0.0),
        out_max: vec3(l, "outputMaximum", 1.0),
    }
}

fn uv_of(a: &Obj) -> Option<UvTransform> {
    let t = a.obj("uvTransformation")?;
    let pair = |n: &str, d: f32| t.floats(n).filter(|v| v.len() >= 2).map_or([d, d], |v| [v[0], v[1]]);
    Some(UvTransform { scale: pair("scaleFactors", 1.0), rotation: t.float("rotation").unwrap_or(0.0), offset: pair("translation", 0.0), tiling: a.int("uvSamplingWrap").unwrap_or(3) != 0 })
}

fn action_of(a: &Obj, blends: Vec<(u64, i32, f32)>, group: usize) -> ActionInfo {
    let kind = match a.class.as_str() {
        "DataActionFill" => ActionKind::Fill { sources: a.list("sources").iter().filter_map(source_of).collect(), projection: a.int("projection").unwrap_or(0) },
        "DataActionFilterProcedural" => match a.obj("procedural").and_then(graph_of) {
            Some(graph) => ActionKind::Filter { graph },
            None => ActionKind::Other(a.class.clone()),
        },
        "DataActionFilterLevels" => match a.obj("levels") {
            Some(l) => ActionKind::Levels(levels_info(l)),
            None => ActionKind::Other(a.class.clone()),
        },
        "DataActionPaint" => ActionKind::Paint { strokes: strokes_of(a), brush: brush_of(a), opacity: a.float("opacity").unwrap_or(1.0) },
        "DataActionEraser" => ActionKind::Eraser { strokes: strokes_of(a), brush: brush_of(a), opacity: a.float("opacity").unwrap_or(1.0) },
        "DataActionDecal" => ActionKind::Decal { hits: a.list("hits").iter().filter_map(decal_hit_of).collect() },
        "DataActionAnchor" => ActionKind::Anchor { uid: a.int("uid").unwrap_or(0) },
        other => ActionKind::Other(other.to_string()),
    };
    ActionInfo { kind, enabled: a.bool("enabled").unwrap_or(true), label: a.str("label").unwrap_or("").to_string(), channels: a.int("channelTypes").unwrap_or(-1) as u64, blends, group, own_blends: blends_of(a), uv: uv_of(a) }
}

fn layer_of(l: &Obj) -> LayerInfo {
    let group = l.class == "DataLayerGroup";
    let mut actions = Vec::new();
    let mut group_blends = Vec::new();
    collect_actions(l.obj("actions"), &mut actions, &mut group_blends, (&[], 0), &mut 0);
    let mut mask_actions = Vec::new();
    collect_actions(l.obj("maskActions"), &mut mask_actions, &mut Vec::new(), (&[], 0), &mut 0);
    let mut kind = if group { LayerKind::Group } else { LayerKind::Empty };
    let mut sources = Vec::new();
    let mut strokes = 0;
    for (a, _, _) in &actions {
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
        content: actions.into_iter().map(|(a, b, g)| action_of(a, b, g)).collect(),
        mask: mask_actions.into_iter().map(|(a, b, g)| action_of(a, b, g)).collect(),
        mask_enabled: l.bool("maskEnabled").unwrap_or(true),
        mask_initial_white: l.int("maskInitial").unwrap_or(1) != 0,
        mesh_filter: (l.int("geometryMaskType").unwrap_or(0) != 0)
            .then(|| (l.bool("enabledMeshDefault").or_else(|| l.int("enabledMeshDefault").map(|v| v != 0)).unwrap_or(true), l.list("enabledMeshList").iter().filter_map(|m| m.str("value").map(str::to_string)).collect())),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(class: &str, fields: Vec<(&str, Val)>) -> Obj {
        Obj { class: class.into(), id: 1, fields: fields.into_iter().map(|(n, v)| (n.to_string(), v)).collect() }
    }
    fn f(v: &[f32]) -> Val {
        Val::Float(v.to_vec())
    }
    fn mat() -> Val {
        f(&(0..16).map(|i| i as f32).collect::<Vec<_>>())
    }

    #[test]
    fn polygon_fill_hits_strokes_and_references_are_read() {
        let click = obj("DataDecalHitPunctual", vec![("color", f(&[0.0, 0.0, 0.0])), ("granularity", Val::Int(3)), ("paintThrough", Val::Int(1)), ("position", f(&[0.25, 0.75])), ("projViewInvMatrix", mat())]);
        let rect = obj("DataDecalHitRect", vec![("color", f(&[1.0, 1.0, 1.0])), ("granularity", Val::Int(2)), ("position0", f(&[0.1, 0.2])), ("position1", f(&[0.3, 0.4])), ("projViewInvMatrix", mat())]);
        let decal = action_of(&obj("DataActionDecal", vec![("hits", Val::List(vec![click, rect]))]), vec![(u64::MAX, 4, 1.0)], 3);
        let ActionKind::Decal { hits } = &decal.kind else { panic!("{decal:?}") };
        assert_eq!(hits.len(), 2);
        assert_eq!((hits[0].value, hits[0].granularity, hits[0].through, hits[0].points.clone()), (0.0, 3, true, vec![[0.25, 0.75]]));
        assert_eq!(hits[1].points, vec![[0.1, 0.2], [0.3, 0.4]]);
        assert_eq!(hits[1].proj_view_inv[5], 5.0);
        assert_eq!((decal.group, decal.blends[0].1), (3, 4));

        let key = |x: f32| obj("DataStrokeKey", vec![("position", f(&[x, 0.5])), ("time", f(&[0.0]))]);
        let stroke = obj("DataStroke", vec![("points", Val::List(vec![key(0.1), key(0.2)])), ("pressure", Val::List(Vec::new())), ("viewMatrix", mat()), ("projectionMatrix", mat()), ("randomSeed", Val::Int(7))]);
        let emitter = obj("DataEmitterStroke", vec![("spacing", f(&[0.2])), ("pressureSizeEnabled", Val::Bool(true))]);
        let brush = obj("DataBrush", vec![("brushSize", f(&[0.04])), ("brushSizeSpace", Val::Int(1)), ("flow", f(&[0.5])), ("alignment", Val::Int(3)), ("cullingEnabled", Val::Bool(true)), ("stampsEmitter", Val::Obj(Some(Box::new(emitter))))]);
        let color = obj("DataSourceUniform", vec![("color", f(&[0.0, 0.0, 0.0]))]);
        let tip = obj("DataProcedural", vec![("urlToSbsRes", Val::Str("resource://tool/shape/shape?version=1.sbsar".into())), ("tweaks", Val::List(vec![obj("DataTweakFloat", vec![("identifier", Val::Str("hardness".into())), ("value", f(&[0.3]))])]))]);
        let tip_source = obj("DataSourceProcedural", vec![("procedural", Val::Obj(Some(Box::new(tip))))]);
        let paint = obj("DataActionPaint", vec![("brush", Val::Obj(Some(Box::new(brush)))), ("opacity", f(&[0.25])), ("sourceColor", Val::List(vec![color])), ("sourceTransparent", Val::List(vec![tip_source])), ("strokes", Val::List(vec![stroke]))]);
        let ActionKind::Paint { strokes, brush: Some(b), opacity } = action_of(&paint, Vec::new(), 0).kind else { panic!() };
        assert_eq!(opacity, 0.25);
        assert_eq!((strokes.len(), strokes[0].points.clone(), strokes[0].seed), (1, vec![[0.1, 0.5], [0.2, 0.5]], 7));
        assert_eq!((b.size, b.flow, b.spacing, b.value, b.hardness, b.tip.as_str(), b.alignment, b.pressure_size), (0.04, 0.5, 0.2, 0.0, Some(0.3), "shape", 3, true));
        assert_eq!(b.stencil, None);

        let reference = obj("DataSourceReference", vec![("channelTypes", Val::Int(0)), ("reference", Val::Obj(Some(Box::new(obj("DataReference", vec![("source", Val::Ints(vec![16_777_217, 0]))])))))]);
        assert_eq!(source_of(&reference), Some(SourceInfo::Reference { channels: 0, anchor: 16_777_217, levels: None }), "uids above 2^24 stay exact");
        let anchor = action_of(&obj("DataActionAnchor", vec![("uid", Val::Int(483613))]), Vec::new(), 0);
        assert_eq!(anchor.kind, ActionKind::Anchor { uid: 483613 });
    }

    #[test]
    fn geometry_mask_is_read() {
        let mesh = |n: &str| obj("DataByteArrayBox", vec![("value", Val::Str(n.into()))]);
        let only = obj("DataLayerGroup", vec![("label", Val::Str("tape".into())), ("geometryMaskType", Val::Int(1)), ("enabledMeshDefault", Val::Bool(false)), ("enabledMeshList", Val::List(vec![mesh("2.002_low")]))]);
        assert_eq!(layer_of(&only).mesh_filter, Some((false, vec!["2.002_low".to_string()])));
        let all = obj("DataLayerColor", vec![("geometryMaskType", Val::Int(0)), ("enabledMeshDefault", Val::Int(255))]);
        assert_eq!(layer_of(&all).mesh_filter, None);
    }
}
