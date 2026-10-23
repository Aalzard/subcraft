//! Builders for synthetic Substance Painter projects, shared by the mesh and app tests (included
//! with `#[path]`): a small HDF5 writer, Qt-style property blobs, a `document.bin` writer, an
//! Assimp-style mesh dump and two tiny OpenJPEG-written tiles. No real project is involved.
#![allow(dead_code)]

use std::collections::BTreeMap;

pub const GRAY16: &[u8] = include_bytes!("../data/gray16.jp2");
pub const RGB8: &[u8] = include_bytes!("../data/rgb8.jp2");
pub const EMBEDDED_7Z: &[u8] = include_bytes!("../data/embedded.7z");
pub const W: usize = 136;
pub const H: usize = 72;

/// The test pattern the two JP2 files were made from.
pub fn pattern(x: usize, y: usize, c: usize) -> u16 {
    ((x * 257 + y * 131 + c * 977 + (((x / 5) * (y / 3)) % 97) * 400) % 65536) as u16
}

// ----- a minimal HDF5 writer (superblock v0, symbol-table groups, contiguous byte datasets) ----------

#[derive(Default)]
pub struct Node {
    children: BTreeMap<String, Node>,
    data: Option<Vec<u8>>,
}

fn pad8(out: &mut Vec<u8>) {
    while out.len() % 8 != 0 {
        out.push(0);
    }
}

fn object_header(out: &mut Vec<u8>, messages: &[(u16, Vec<u8>)]) -> u64 {
    pad8(out);
    let addr = out.len() as u64;
    let body: usize = messages.iter().map(|(_, d)| 8 + d.len().div_ceil(8) * 8).sum();
    out.extend([1, 0]);
    out.extend((messages.len() as u16).to_le_bytes());
    out.extend(1u32.to_le_bytes());
    out.extend((body as u32).to_le_bytes());
    out.extend([0; 4]);
    for (kind, data) in messages {
        out.extend(kind.to_le_bytes());
        out.extend(((data.len().div_ceil(8) * 8) as u16).to_le_bytes());
        out.extend([0; 4]);
        out.extend(data);
        out.resize(out.len() + data.len().div_ceil(8) * 8 - data.len(), 0);
    }
    addr
}

/// Write a group (and everything under it); returns (object header, b-tree, heap) addresses.
fn write_group(out: &mut Vec<u8>, node: &Node) -> (u64, u64, u64) {
    let mut entries: Vec<(String, u64)> = Vec::new();
    for (name, child) in &node.children {
        let addr = match &child.data {
            Some(bytes) => {
                pad8(out);
                let data_addr = out.len() as u64;
                out.extend(bytes);
                let space = [vec![1, 1, 0, 0, 0, 0, 0, 0], (bytes.len() as u64).to_le_bytes().to_vec()].concat();
                let ty = vec![0x10, 0, 0, 0, 1, 0, 0, 0, 0, 0, 8, 0];
                let layout = [vec![3, 1], data_addr.to_le_bytes().to_vec(), (bytes.len() as u64).to_le_bytes().to_vec()].concat();
                object_header(out, &[(1, space), (3, ty), (8, layout)])
            }
            None => write_group(out, child).0,
        };
        entries.push((name.clone(), addr));
    }
    // Local heap data: an empty name first, then the names.
    let mut heap_data = vec![0u8; 8];
    let mut offsets = Vec::new();
    for (name, _) in &entries {
        offsets.push(heap_data.len() as u64);
        heap_data.extend(name.as_bytes());
        heap_data.push(0);
        while heap_data.len() % 8 != 0 {
            heap_data.push(0);
        }
    }
    pad8(out);
    let heap_data_addr = out.len() as u64;
    out.extend(&heap_data);
    pad8(out);
    let heap = out.len() as u64;
    out.extend(b"HEAP");
    out.extend([0; 4]);
    out.extend((heap_data.len() as u64).to_le_bytes());
    out.extend(u64::MAX.to_le_bytes());
    out.extend(heap_data_addr.to_le_bytes());
    pad8(out);
    let snod = out.len() as u64;
    out.extend(b"SNOD");
    out.extend([1, 0]);
    out.extend((entries.len() as u16).to_le_bytes());
    for ((_, addr), off) in entries.iter().zip(&offsets) {
        out.extend(off.to_le_bytes());
        out.extend(addr.to_le_bytes());
        out.extend([0; 4 + 4 + 16]);
    }
    pad8(out);
    let btree = out.len() as u64;
    out.extend(b"TREE");
    out.extend([0, 0]);
    out.extend(1u16.to_le_bytes());
    out.extend(u64::MAX.to_le_bytes());
    out.extend(u64::MAX.to_le_bytes());
    out.extend(0u64.to_le_bytes());
    out.extend(snod.to_le_bytes());
    out.extend(0u64.to_le_bytes());
    let st = [btree.to_le_bytes(), heap.to_le_bytes()].concat();
    let header = object_header(out, &[(0x11, st)]);
    (header, btree, heap)
}

pub fn hdf5_file(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut root = Node::default();
    for (path, data) in files {
        let mut node = &mut root;
        for part in path.split('/') {
            node = node.children.entry(part.to_string()).or_default();
        }
        node.data = Some(data.clone());
    }
    let mut out = vec![0u8; 96];
    let (header, btree, heap) = write_group(&mut out, &root);
    out[..8].copy_from_slice(&[0x89, b'H', b'D', b'F', 0x0d, 0x0a, 0x1a, 0x0a]);
    out[13] = 8;
    out[14] = 8;
    let eof = out.len() as u64;
    out[40..48].copy_from_slice(&eof.to_le_bytes());
    out[64..72].copy_from_slice(&header.to_le_bytes());
    out[72..76].copy_from_slice(&1u32.to_le_bytes());
    out[80..88].copy_from_slice(&btree.to_le_bytes());
    out[88..96].copy_from_slice(&heap.to_le_bytes());
    out
}

// ----- Qt-style blobs ---------------------------------------------------------------------------------

pub fn qstring(s: &str) -> Vec<u8> {
    let units: Vec<u8> = s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect();
    [(units.len() as u32).to_be_bytes().to_vec(), units].concat()
}

pub enum Q {
    Str(&'static str),
    UInt(u32),
    UInt64(u64),
}

pub fn properties(props: &[(&str, Q)]) -> Vec<u8> {
    let mut out = (props.len() as u32).to_be_bytes().to_vec();
    for (k, v) in props {
        out.extend(qstring(k));
        match v {
            Q::Str(s) => {
                out.extend(10u32.to_be_bytes());
                out.push(0);
                out.extend(qstring(s));
            }
            Q::UInt(n) => {
                out.extend(3u32.to_be_bytes());
                out.push(0);
                out.extend(n.to_be_bytes());
            }
            Q::UInt64(n) => {
                out.extend(5u32.to_be_bytes());
                out.push(0);
                out.extend(n.to_be_bytes());
            }
        }
    }
    out
}

fn le(out: &mut Vec<u8>, v: u32) {
    out.extend(v.to_le_bytes());
}

/// A `resources/<key>` blob: header, then per tile a 24-byte record and its padded JP2 data.
pub fn resource_blob(format: &'static str, width: u32, height: u32, tiles: &[([u32; 4], &[u8])]) -> Vec<u8> {
    let mut out = properties(&[
        ("compression", Q::Str("jp2")),
        ("format", Q::Str(format)),
        ("height", Q::UInt(height)),
        ("tilesCount", Q::UInt(tiles.len() as u32)),
        ("width", Q::UInt(width)),
        ("dataSizeRaw", Q::UInt64(0)),
    ]);
    for (rect, data) in tiles {
        for v in rect {
            le(&mut out, *v);
        }
        le(&mut out, data.len() as u32);
        le(&mut out, 0);
        out.extend(*data);
        out.resize(out.len() + (data.len() + 7) / 8 * 8 - data.len(), 0);
    }
    out
}

/// A `texture-cache` entry holding an RG8 buffer as a single LZ4 literal run.
pub fn cache_blob(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let mut block = Vec::new();
    let n = pixels.len();
    if n < 15 {
        block.push((n as u8) << 4);
    } else {
        block.push(0xf0);
        let mut rest = n - 15;
        while rest >= 255 {
            block.push(255);
            rest -= 255;
        }
        block.push(rest as u8);
    }
    block.extend(pixels);
    let mut out = properties(&[
        ("bufferSize", Q::UInt(pixels.len() as u32)),
        ("compression", Q::Str("lz4")),
        ("height", Q::UInt(height)),
        ("internalFormat", Q::UInt(33323)),
        ("version", Q::Str("v1")),
        ("width", Q::UInt(width)),
    ]);
    out.extend((block.len() as u32).to_be_bytes());
    out.extend(block);
    out
}

pub fn resources_toc(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut out = 1u32.to_be_bytes().to_vec();
    out.extend(qstring("Alg::ResourceImage"));
    out.extend((entries.len() as u32).to_be_bytes());
    for (name, key) in entries {
        out.extend(qstring(&format!("/{name}?version={key}")));
    }
    out
}

// ----- a document.bin writer ----------------------------------------------------------------------------

pub struct DocWriter {
    out: Vec<u8>,
    classes: Vec<&'static str>,
}

impl DocWriter {
    fn u32(&mut self, v: u32) {
        self.out.extend(v.to_le_bytes());
    }
    fn string(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.out.extend(s.as_bytes());
    }
    /// Class word: the definition the first time, the index afterwards.
    fn class(&mut self, name: &'static str, props: &[(&str, u32)]) {
        if let Some(i) = self.classes.iter().position(|c| *c == name) {
            self.u32(i as u32);
        } else {
            self.u32(u32::MAX);
            self.string(name);
            self.u32(props.len() as u32);
            for (n, t) in props {
                self.string(n);
                self.u32(*t);
            }
            self.classes.push(name);
        }
    }
}

pub fn document(normal_syscoord: i32) -> Vec<u8> {
    let mut w = DocWriter { out: Vec::new(), classes: Vec::new() };
    w.out.extend(0x1b7c_2fddu32.to_le_bytes());
    for v in [1, 0, 0, 0] {
        w.u32(v);
    }
    // DataDocument { materials: list of objects, normalMapSyscoord: int }
    w.u32(0x100);
    w.class("DataDocument", &[("materials", 19), ("normalMapSyscoord", 9)]);
    w.u32(1);
    // DataMaterial { additionalMaps: struct list, resolutionLog2: ivec2, sceneMaterialName, sceneMaterialUID, stacks }
    w.u32(0x101);
    w.class("DataMaterial", &[("additionalMaps", 17), ("resolutionLog2", 6), ("sceneMaterialName", 16), ("sceneMaterialUID", 12), ("stacks", 19)]);
    w.u32(2);
    w.class("DataAdditionalMap", &[("urlToBitmapRes", 16), ("usage", 9)]);
    w.string("/Position Cube?version=bbbb");
    w.u32(4);
    w.string("/Ambient Occlusion Map from Mesh Cube?version=aaaa");
    w.u32(1);
    w.u32(8);
    w.u32(8);
    w.string("Cube");
    w.out.extend(4242u64.to_le_bytes());
    w.u32(1);
    // DataMaterialStack { channels: list, stack: object }
    w.u32(0x102);
    w.class("DataMaterialStack", &[("channels", 19), ("stack", 18)]);
    w.u32(2);
    for (bits, format) in [(1u64, 0u32), (128, 1)] {
        w.u32(0x200 + bits as u32);
        w.class("DataChannel", &[("format", 9), ("type", 21), ("userName", 16)]);
        w.u32(format);
        w.out.extend(bits.to_le_bytes());
        w.string("");
    }
    // The layer stack: one fill layer with a uniform color source, one group holding a paint layer.
    w.u32(0x103);
    w.class("DataStackLayers", &[("items", 19)]);
    w.u32(2);
    w.u32(0x300);
    w.class("DataLayerColor", &[("actions", 18), ("enabled", 10), ("label", 16), ("maskActions", 18), ("perChannelBlending", 17)]);
    w.u32(0x301);
    w.class("DataStackActions", &[("items", 19)]);
    w.u32(1);
    w.u32(0x302);
    w.class("DataActionFill", &[("channelTypes", 12), ("sources", 19)]);
    w.out.extend(u64::MAX.to_le_bytes());
    w.u32(1);
    w.u32(0x303);
    w.class("DataSourceUniform", &[("channelTypes", 12), ("color", 3)]);
    w.out.extend(1u64.to_le_bytes());
    for c in [0.25f32, 0.5, 0.75] {
        w.out.extend(c.to_le_bytes());
    }
    w.out.push(1);
    w.string("Rust");
    w.u32(0); // no mask
    w.u32(0); // no per-channel blending
    // Group with a paint layer that has a mask.
    w.u32(0x310);
    w.class("DataLayerGroup", &[("actions", 18), ("enabled", 10), ("label", 16), ("maskActions", 18), ("perChannelBlending", 17), ("subStack", 18)]);
    w.u32(0);
    w.out.push(0);
    w.string("Dirt");
    w.u32(0);
    w.u32(0);
    w.u32(0x311);
    w.class("DataStackLayers", &[("items", 19)]);
    w.u32(1);
    w.u32(0x312);
    w.class("DataLayerColor", &[("actions", 18), ("enabled", 10), ("label", 16), ("maskActions", 18), ("perChannelBlending", 17)]);
    w.u32(0x313);
    w.class("DataStackActions", &[("items", 19)]);
    w.u32(0);
    w.out.push(1);
    w.string("Scratches");
    w.u32(0x314); // a mask
    w.class("DataStackActions", &[("items", 19)]);
    w.u32(0);
    w.u32(0);
    // The document's last field.
    w.u32(normal_syscoord as u32);
    w.out
}

/// An Assimp-style `scene.assbin` with one quad (two triangles, UVs covering the unit square).
pub fn assbin_quad() -> Vec<u8> {
    let mut b = b"ASSIMP.binary-dump.test".to_vec();
    b.resize(524, 0);
    b.extend(4u32.to_le_bytes()); // node data length (counted from the length field itself)
    let mut rec = Vec::new();
    rec.extend(1u32.to_le_bytes()); // the first record starts with the mesh count
    rec.extend(0u32.to_le_bytes()); // length, patched below
    rec.extend(0u32.to_le_bytes());
    rec.extend(4u32.to_le_bytes());
    rec.extend(b"Cube");
    rec.extend(4u32.to_le_bytes()); // triangles
    rec.extend(0u32.to_le_bytes());
    rec.extend((1u32 | 2 | 0x100).to_le_bytes());
    let corners = [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]];
    let floats = |rec: &mut Vec<u8>, data: &[[f32; 3]]| {
        rec.extend((data.len() as u32).to_le_bytes());
        for v in data {
            for c in v {
                rec.extend(c.to_le_bytes());
            }
        }
    };
    floats(&mut rec, &corners);
    floats(&mut rec, &[[0.0, 0.0, 1.0]; 4]);
    rec.extend(3u32.to_le_bytes());
    floats(&mut rec, &corners);
    rec.extend(2u32.to_le_bytes());
    for t in [[0u32, 1, 2], [0, 2, 3]] {
        rec.extend(0u32.to_le_bytes());
        rec.extend(3u32.to_le_bytes());
        for i in t {
            rec.extend(i.to_le_bytes());
        }
    }
    let len = rec.len() as u32;
    rec[4..8].copy_from_slice(&len.to_le_bytes());
    b.extend(rec);
    b
}

/// A complete synthetic project: a one-quad model, the document, two baked maps and one cached channel.
pub fn project_file() -> Vec<u8> {
    let toc = resources_toc(&[("Ambient Occlusion Map from Mesh Cube", "aaaa"), ("Position Cube", "bbbb"), ("Not a map", "cccc")]);
    let ao = resource_blob("LUM16", 136, 72, &[([0, 0, 136, 72], GRAY16)]);
    let position = resource_blob("RGBA16", 136, 72, &[([0, 0, 136, 72], RGB8)]);
    // 4x2 RG8 pixels: value = 10 * index, alpha 255.
    let pixels: Vec<u8> = (0..8).flat_map(|i| [i * 10, 255]).collect();
    hdf5_file(&[
        ("paint/scene.assbin", assbin_quad()),
        ("paint/document.bin", document(0)),
        ("resources.toc", toc),
        ("resources/aaaa", ao),
        ("resources/bbbb", position),
        ("resources/dddd.sbsar", EMBEDDED_7Z.to_vec()),
        ("texture-cache/4242/1/128", cache_blob(4, 2, &pixels)),
        ("texture-cache/header", vec![0, 1, 2]),
    ])
}
