//! Minimal read-only HDF5 reader, just enough for Substance 3D Painter `.spp` projects.
//!
//! Supported: superblock version 0, version-1 object headers, old-style groups (symbol tables,
//! local heaps, version-1 B-trees) and byte datasets stored as compact, contiguous or chunked
//! (unfiltered) data. Anything else is reported as an error instead of being guessed at.

use std::io::{Read, Seek, SeekFrom};

const SIGNATURE: [u8; 8] = [0x89, b'H', b'D', b'F', 0x0d, 0x0a, 0x1a, 0x0a];
const UNDEFINED: u64 = u64::MAX;

#[derive(Debug, thiserror::Error)]
pub enum Hdf5Error {
    #[error("not an HDF5 file")]
    NotHdf5,
    #[error("unsupported HDF5 feature: {0}")]
    Unsupported(String),
    #[error("corrupt HDF5 file: {0}")]
    Corrupt(String),
    #[error("no dataset named '{0}'")]
    NotFound(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

type Result<T> = std::result::Result<T, Hdf5Error>;

fn corrupt<T>(what: &str) -> Result<T> {
    Err(Hdf5Error::Corrupt(what.into()))
}

fn u16_at(b: &[u8], o: usize) -> Result<u16> {
    b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or_else(|| Hdf5Error::Corrupt("short read".into()))
}

fn u32_at(b: &[u8], o: usize) -> Result<u32> {
    b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).ok_or_else(|| Hdf5Error::Corrupt("short read".into()))
}

fn u64_at(b: &[u8], o: usize) -> Result<u64> {
    b.get(o..o + 8)
        .map(|s| u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
        .ok_or_else(|| Hdf5Error::Corrupt("short read".into()))
}

/// One dataset's location, found from its object header.
#[derive(Clone, Debug)]
pub struct DatasetInfo {
    /// Total size in bytes.
    pub len: u64,
    layout: Layout,
}

#[derive(Clone, Debug)]
enum Layout {
    Compact(Vec<u8>),
    Contiguous { address: u64 },
    Chunked { btree: u64, dims: Vec<u32> },
}

pub struct Hdf5File<R: Read + Seek> {
    r: R,
    root_btree: u64,
    root_heap: u64,
}

impl<R: Read + Seek> Hdf5File<R> {
    pub fn open(mut r: R) -> Result<Self> {
        let sb = read_at(&mut r, 0, 96)?;
        if sb[..8] != SIGNATURE {
            return Err(Hdf5Error::NotHdf5);
        }
        if sb[8] != 0 {
            return Err(Hdf5Error::Unsupported(format!("superblock version {}", sb[8])));
        }
        if sb[13] != 8 || sb[14] != 8 {
            return Err(Hdf5Error::Unsupported("offset/length sizes other than 8 bytes".into()));
        }
        // Root group symbol table entry follows the four 8-byte addresses at offset 24.
        let entry = 56;
        let cache_type = u32_at(&sb, entry + 16)?;
        if cache_type != 1 {
            return Err(Hdf5Error::Unsupported("root group without a symbol table cache".into()));
        }
        Ok(Self { root_btree: u64_at(&sb, entry + 24)?, root_heap: u64_at(&sb, entry + 32)?, r })
    }

    /// Find a dataset by its slash-separated path (for example `paint/scene.assbin`).
    pub fn dataset(&mut self, path: &str) -> Result<DatasetInfo> {
        let (mut btree, mut heap) = (self.root_btree, self.root_heap);
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        for (i, part) in parts.iter().enumerate() {
            let addr = self.lookup(btree, heap, part)?.ok_or_else(|| Hdf5Error::NotFound(path.into()))?;
            if i + 1 == parts.len() {
                return self.read_dataset_header(addr);
            }
            let msgs = self.object_messages(addr)?;
            let st = msgs.iter().find(|m| m.0 == 0x11).ok_or_else(|| Hdf5Error::NotFound(path.into()))?;
            btree = u64_at(&st.1, 0)?;
            heap = u64_at(&st.1, 8)?;
        }
        Err(Hdf5Error::NotFound(path.into()))
    }

    /// Names of the direct children of the group at `path` (empty path = the root group).
    pub fn list(&mut self, path: &str) -> Result<Vec<String>> {
        let (mut btree, mut heap) = (self.root_btree, self.root_heap);
        for part in path.split('/').filter(|p| !p.is_empty()) {
            let addr = self.lookup(btree, heap, part)?.ok_or_else(|| Hdf5Error::NotFound(path.into()))?;
            let msgs = self.object_messages(addr)?;
            let st = msgs.iter().find(|m| m.0 == 0x11).ok_or_else(|| Hdf5Error::NotFound(path.into()))?;
            btree = u64_at(&st.1, 0)?;
            heap = u64_at(&st.1, 8)?;
        }
        let mut names = Vec::new();
        self.walk_group(btree, heap, &mut |name, _| {
            names.push(name.to_string());
            false
        })?;
        Ok(names)
    }

    /// Read the whole dataset.
    pub fn read(&mut self, info: &DatasetInfo) -> Result<Vec<u8>> {
        match &info.layout {
            Layout::Compact(data) => Ok(data.clone()),
            Layout::Contiguous { address } => {
                if *address == UNDEFINED {
                    return Ok(vec![0; info.len as usize]);
                }
                read_at(&mut self.r, *address, info.len as usize)
            }
            Layout::Chunked { btree, dims } => {
                let mut out = vec![0u8; info.len as usize];
                if *btree != UNDEFINED {
                    let rank = dims.len();
                    self.read_chunks(*btree, rank, &mut out)?;
                }
                Ok(out)
            }
        }
    }

    fn read_chunks(&mut self, node: u64, rank: usize, out: &mut [u8]) -> Result<()> {
        let head = read_at(&mut self.r, node, 24)?;
        if &head[..4] != b"TREE" || head[4] != 1 {
            return corrupt("chunk B-tree node");
        }
        let level = head[5];
        let used = u16_at(&head, 6)? as usize;
        let key_size = 8 + 8 * rank;
        let body = read_at(&mut self.r, node + 24, used * (key_size + 8) + key_size)?;
        for i in 0..used {
            let key = i * (key_size + 8);
            let size = u32_at(&body, key)? as usize;
            let mask = u32_at(&body, key + 4)?;
            let child = u64_at(&body, key + key_size)?;
            if level > 0 {
                self.read_chunks(child, rank, out)?;
                continue;
            }
            if mask != 0 {
                return Err(Hdf5Error::Unsupported("filtered chunks".into()));
            }
            // Offsets are per dimension; datasets here are one-dimensional bytes.
            let offset = u64_at(&body, key + 8)? as usize;
            if offset >= out.len() {
                continue;
            }
            let take = size.min(out.len() - offset);
            let data = read_at(&mut self.r, child, take)?;
            out[offset..offset + take].copy_from_slice(&data);
        }
        Ok(())
    }

    fn lookup(&mut self, btree: u64, heap: u64, name: &str) -> Result<Option<u64>> {
        let mut found = None;
        self.walk_group(btree, heap, &mut |n, addr| {
            if n == name {
                found = Some(addr);
                true
            } else {
                false
            }
        })?;
        Ok(found)
    }

    /// Visit every (name, object header address) of a group; the callback returns true to stop.
    fn walk_group(&mut self, btree: u64, heap: u64, f: &mut dyn FnMut(&str, u64) -> bool) -> Result<()> {
        let h = read_at(&mut self.r, heap, 32)?;
        if &h[..4] != b"HEAP" {
            return corrupt("local heap");
        }
        let heap_data = u64_at(&h, 24)?;
        let heap_size = u64_at(&h, 8)? as usize;
        let heap_bytes = read_at(&mut self.r, heap_data, heap_size)?;
        self.walk_btree(btree, &heap_bytes, f).map(|_| ())
    }

    fn walk_btree(&mut self, node: u64, heap: &[u8], f: &mut dyn FnMut(&str, u64) -> bool) -> Result<bool> {
        let head = read_at(&mut self.r, node, 24)?;
        if &head[..4] != b"TREE" || head[4] != 0 {
            return corrupt("group B-tree node");
        }
        let level = head[5];
        let used = u16_at(&head, 6)? as usize;
        let body = read_at(&mut self.r, node + 24, used * 16 + 8)?;
        for i in 0..used {
            let child = u64_at(&body, 8 + i * 16)?;
            if level > 0 {
                if self.walk_btree(child, heap, f)? {
                    return Ok(true);
                }
                continue;
            }
            let snod = read_at(&mut self.r, child, 8)?;
            if &snod[..4] != b"SNOD" {
                return corrupt("symbol table node");
            }
            let count = u16_at(&snod, 6)? as usize;
            let entries = read_at(&mut self.r, child + 8, count * 40)?;
            for e in 0..count {
                let name_off = u64_at(&entries, e * 40)? as usize;
                let addr = u64_at(&entries, e * 40 + 8)?;
                let end = heap.get(name_off..).and_then(|s| s.iter().position(|&c| c == 0)).ok_or_else(|| Hdf5Error::Corrupt("link name".into()))?;
                let name = String::from_utf8_lossy(&heap[name_off..name_off + end]);
                if f(&name, addr) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// All header messages of a version-1 object header as (type, data).
    fn object_messages(&mut self, addr: u64) -> Result<Vec<(u16, Vec<u8>)>> {
        let head = read_at(&mut self.r, addr, 16)?;
        if head[0] != 1 {
            return Err(Hdf5Error::Unsupported(format!("object header version {}", head[0])));
        }
        let mut remaining = u16_at(&head, 2)? as usize;
        let mut blocks = vec![(addr + 16, u32_at(&head, 8)? as u64)];
        let mut out = Vec::new();
        while let Some((start, len)) = blocks.pop() {
            let data = read_at(&mut self.r, start, len as usize)?;
            let mut p = 0;
            while p + 8 <= data.len() && remaining > 0 {
                let kind = u16_at(&data, p)?;
                let size = u16_at(&data, p + 2)? as usize;
                let body = data.get(p + 8..p + 8 + size).ok_or_else(|| Hdf5Error::Corrupt("header message".into()))?.to_vec();
                p += 8 + size;
                remaining -= 1;
                if kind == 0x10 {
                    blocks.push((u64_at(&body, 0)?, u64_at(&body, 8)?));
                } else {
                    out.push((kind, body));
                }
            }
        }
        Ok(out)
    }

    fn read_dataset_header(&mut self, addr: u64) -> Result<DatasetInfo> {
        let msgs = self.object_messages(addr)?;
        let find = |t: u16| msgs.iter().find(|m| m.0 == t).map(|m| m.1.as_slice());
        if find(0x0b).is_some() {
            return Err(Hdf5Error::Unsupported("filtered (compressed) dataset".into()));
        }
        let space = find(0x01).ok_or_else(|| Hdf5Error::Corrupt("dataset without dataspace".into()))?;
        let dims = dataspace_dims(space)?;
        let ty = find(0x03).ok_or_else(|| Hdf5Error::Corrupt("dataset without datatype".into()))?;
        let element = u32_at(ty, 4)? as u64;
        let len = dims.iter().product::<u64>() * element;
        let lay = find(0x08).ok_or_else(|| Hdf5Error::Corrupt("dataset without layout".into()))?;
        if lay[0] != 3 {
            return Err(Hdf5Error::Unsupported(format!("data layout version {}", lay[0])));
        }
        let layout = match lay[1] {
            0 => {
                let size = u16_at(lay, 2)? as usize;
                Layout::Compact(lay.get(4..4 + size).ok_or_else(|| Hdf5Error::Corrupt("compact data".into()))?.to_vec())
            }
            1 => Layout::Contiguous { address: u64_at(lay, 2)? },
            2 => {
                let rank = lay[2] as usize;
                let btree = u64_at(lay, 3)?;
                let mut d = Vec::new();
                for i in 0..rank {
                    d.push(u32_at(lay, 11 + 4 * i)?);
                }
                Layout::Chunked { btree, dims: d }
            }
            c => return Err(Hdf5Error::Unsupported(format!("layout class {c}"))),
        };
        Ok(DatasetInfo { len, layout })
    }
}

fn dataspace_dims(m: &[u8]) -> Result<Vec<u64>> {
    let (rank, start) = match m[0] {
        1 => (m[1] as usize, 8),
        2 => (m[1] as usize, 4),
        v => return Err(Hdf5Error::Unsupported(format!("dataspace version {v}"))),
    };
    (0..rank).map(|i| u64_at(m, start + 8 * i)).collect()
}

fn read_at<R: Read + Seek>(r: &mut R, offset: u64, len: usize) -> Result<Vec<u8>> {
    r.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}
