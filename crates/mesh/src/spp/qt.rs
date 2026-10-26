//! Qt `QDataStream` (big endian) property maps, the small headers in front of Painter's image
//! blobs: a count, then `QString` key + `QVariant` value pairs.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Variant {
    Bool(bool),
    Int(i32),
    UInt(u32),
    Int64(i64),
    UInt64(u64),
    Double(f64),
    Str(String),
    Bytes(Vec<u8>),
}

impl Variant {
    pub fn as_u32(&self) -> Option<u32> {
        match self {
            Variant::UInt(v) => Some(*v),
            Variant::Int(v) => u32::try_from(*v).ok(),
            Variant::UInt64(v) => u32::try_from(*v).ok(),
            Variant::Int64(v) => u32::try_from(*v).ok(),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Variant::Str(s) => Some(s),
            _ => None,
        }
    }
}

pub struct Reader<'a> {
    data: &'a [u8],
    pub pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos }
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.data.get(self.pos..self.pos.checked_add(n)?)?;
        self.pos += n;
        Some(s)
    }
    pub fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    pub fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub fn u64(&mut self) -> Option<u64> {
        self.take(8).map(|b| u64::from_be_bytes(b.try_into().unwrap()))
    }
    /// `QString`: byte length (0xFFFFFFFF = null) then UTF-16BE.
    pub fn string(&mut self) -> Option<String> {
        let n = self.u32()?;
        if n == u32::MAX {
            return Some(String::new());
        }
        let b = self.take(n as usize)?;
        let units: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        String::from_utf16(&units).ok()
    }
    pub fn variant(&mut self) -> Option<Variant> {
        let kind = self.u32()?;
        let _null = self.u8()?;
        Some(match kind {
            1 => Variant::Bool(self.u8()? != 0),
            2 => Variant::Int(self.u32()? as i32),
            3 => Variant::UInt(self.u32()?),
            4 => Variant::Int64(self.u64()? as i64),
            5 => Variant::UInt64(self.u64()?),
            6 => Variant::Double(f64::from_bits(self.u64()?)),
            10 => Variant::Str(self.string()?),
            12 => {
                let n = self.u32()?;
                if n == u32::MAX {
                    Variant::Bytes(Vec::new())
                } else {
                    Variant::Bytes(self.take(n as usize)?.to_vec())
                }
            }
            _ => return None,
        })
    }
}

/// Read a property map at `pos`; returns it and the position after it.
pub fn read_properties(data: &[u8], pos: usize) -> Option<(BTreeMap<String, Variant>, usize)> {
    let mut r = Reader::new(data, pos);
    let n = r.u32()?;
    if n > 1024 {
        return None;
    }
    let mut map = BTreeMap::new();
    for _ in 0..n {
        let key = r.string()?;
        let value = r.variant()?;
        map.insert(key, value);
    }
    Some((map, r.pos))
}
