//! Painter HBO decoder (registry v11+), ported from Universal-Painter-Files `hbo_decode` / `_readers`.

use std::io::{Cursor, Read};

pub const MAGIC: u32 = 0x1B7C2FDD;

const MAX_RECURSION: u32 = 128;
const MAX_ARRAY_ITEMS: u32 = 1_000_000;

#[derive(Debug, Clone)]
pub struct UNode {
    pub type_name: String,
    pub fields: Vec<(String, Value)>,
}

#[derive(Debug, Clone)]
pub enum Value {
    Object(Option<UNode>),
    Array(Vec<Value>),
    String(Vec<u8>),
    /// HBO primitive type code (kept for parity with UPF decoder).
    Primitive(#[allow(dead_code)] u32, Vec<u8>),
    Null,
}

#[derive(Debug, Clone)]
struct TypeDef {
    name: String,
    members: Vec<(String, u32)>,
}

fn primitive_sizes() -> std::collections::HashMap<u32, usize> {
    [
        (1, 4),
        (2, 8),
        (3, 12),
        (4, 16),
        (5, 4),
        (6, 8),
        (7, 12),
        (8, 16),
        (9, 4),
        (10, 1),
        (11, 4),
        (12, 8),
        (13, 36),
        (14, 64),
        (15, 8),
        (16, 32),
        (21, 8),
        (22, 16),
    ]
    .into_iter()
    .collect()
}

pub fn read_header(raw: &[u8]) -> Option<(u32, u32, u32)> {
    if raw.len() < 12 {
        return None;
    }
    let magic = u32::from_le_bytes(raw[0..4].try_into().unwrap());
    let vc = u32::from_le_bytes(raw[4..8].try_into().unwrap());
    let dv = u32::from_le_bytes(raw[8..12].try_into().unwrap());
    if magic != MAGIC {
        return None;
    }
    Some((magic, vc, dv))
}

pub fn decode(raw: &[u8]) -> Result<(UNode, &'static str, u32), String> {
    let (_, vc, dv) = read_header(raw).ok_or("not an HBO stream (bad magic or short)")?;
    match vc {
        0 => decode_inline(raw).map(|n| (n, "inline", dv)),
        1 => decode_registry(raw).map(|n| (n, "registry", dv)),
        _ => Err(format!("unknown HBO version_check {vc}")),
    }
}

fn decode_registry(raw: &[u8]) -> Result<UNode, String> {
    if raw.len() < 12 {
        return Err("HBO too short".into());
    }
    let mut src = Cursor::new(&raw[12..]);
    let tag = read_u32(&mut src)?;
    if tag != 0x12 && tag != 0x13 {
        return Err("registry stream does not start with an object/root tag".into());
    }
    let type_count = read_u32(&mut src)?;
    let mut registry: Vec<TypeDef> = Vec::new();
    if let Some(table) = parse_v11_registry_table(&mut src, type_count)? {
        registry = table;
    }
    let (name, fields) = parse_v11_object(&mut src, &mut registry, 0)?;
    Ok(UNode {
        type_name: name,
        fields,
    })
}

fn read_u32(src: &mut Cursor<&[u8]>) -> Result<u32, String> {
    let mut buf = [0u8; 4];
    src.read_exact(&mut buf).map_err(|_| "unexpected EOF reading u32".to_string())?;
    Ok(u32::from_le_bytes(buf))
}

fn read_u8(src: &mut Cursor<&[u8]>) -> Result<u8, String> {
    let mut buf = [0u8; 1];
    src.read_exact(&mut buf).map_err(|_| "unexpected EOF reading u8".to_string())?;
    Ok(buf[0])
}

fn read_string(src: &mut Cursor<&[u8]>) -> Result<String, String> {
    let length = read_u32(src)?;
    if length == 0 {
        return Ok(String::new());
    }
    let mut buf = vec![0u8; length as usize];
    src.read_exact(&mut buf).map_err(|_| "unexpected EOF reading string".to_string())?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn guard_count(remaining: usize, count: u32) -> Result<u32, String> {
    if count > MAX_ARRAY_ITEMS {
        return Err(format!("array item count {count} exceeds limit {MAX_ARRAY_ITEMS}"));
    }
    if count as usize > remaining {
        return Err(format!("array item count {count} exceeds remaining bytes {remaining}"));
    }
    Ok(count)
}

fn remaining_bytes(src: &Cursor<&[u8]>) -> usize {
    let pos = src.position() as usize;
    src.get_ref().len() - pos
}

fn parse_v11_type_def(src: &mut Cursor<&[u8]>, registry: &mut Vec<TypeDef>) -> Result<TypeDef, String> {
    let idx = read_u32(src)?;
    if idx != 0xFFFF_FFFF {
        let i = idx as usize;
        if i >= registry.len() {
            return Err(format!("type index out of range: {idx}"));
        }
        return Ok(registry[i].clone());
    }
    let name = read_string(src)?;
    let member_count = read_u32(src)?;
    let mut members = Vec::new();
    for _ in 0..member_count {
        let m_name = read_string(src)?;
        let m_type = read_u32(src)?;
        members.push((m_name, m_type));
    }
    let type_def = TypeDef { name, members };
    registry.push(type_def.clone());
    Ok(type_def)
}

fn parse_v11_object(
    src: &mut Cursor<&[u8]>,
    registry: &mut Vec<TypeDef>,
    depth: u32,
) -> Result<(String, Vec<(String, Value)>), String> {
    if depth > MAX_RECURSION {
        return Err("max recursion depth exceeded".into());
    }
    let flag = read_u32(src)?;
    if flag == 0 {
        return Ok((String::new(), Vec::new()));
    }
    let type_def = parse_v11_type_def(src, registry)?;
    parse_v11_object_with_def(src, registry, &type_def, depth)
}

fn parse_v11_object_with_def(
    src: &mut Cursor<&[u8]>,
    registry: &mut Vec<TypeDef>,
    type_def: &TypeDef,
    depth: u32,
) -> Result<(String, Vec<(String, Value)>), String> {
    let mut fields = Vec::new();
    for (m_name, m_type) in &type_def.members {
        let value = parse_v11_value(src, *m_type, registry, depth + 1)?;
        fields.push((m_name.clone(), value));
    }
    Ok((type_def.name.clone(), fields))
}

fn parse_v11_value(
    src: &mut Cursor<&[u8]>,
    type_code: u32,
    registry: &mut Vec<TypeDef>,
    depth: u32,
) -> Result<Value, String> {
    if depth > MAX_RECURSION {
        return Err("max recursion depth exceeded".into());
    }
    let sizes = primitive_sizes();
    match type_code {
        0x12 | 0x14 => {
            let (name, fields) = parse_v11_object(src, registry, depth)?;
            if name.is_empty() && fields.is_empty() {
                Ok(Value::Object(None))
            } else {
                Ok(Value::Object(Some(UNode { type_name: name, fields })))
            }
        }
        0x13 => {
            let count = guard_count(remaining_bytes(src), read_u32(src)?)?;
            let mut values = Vec::new();
            for _ in 0..count {
                let start = src.position();
                let (name, fields) = parse_v11_object(src, registry, depth + 1)?;
                if src.position() == start {
                    return Err("no progress while parsing v11 array (0x13)".into());
                }
                if name.is_empty() && fields.is_empty() {
                    values.push(Value::Object(None));
                } else {
                    values.push(Value::Object(Some(UNode { type_name: name, fields })));
                }
            }
            Ok(Value::Array(values))
        }
        0x11 => {
            let count = guard_count(remaining_bytes(src), read_u32(src)?)?;
            let mut values = Vec::new();
            if count > 0 {
                let elem_def = parse_v11_type_def(src, registry)?;
                for _ in 0..count {
                    let start = src.position();
                    let (name, fields) = parse_v11_object_with_def(src, registry, &elem_def, depth + 1)?;
                    if src.position() == start {
                        return Err("no progress while parsing v11 array (0x11)".into());
                    }
                    values.push(Value::Object(Some(UNode { type_name: name, fields })));
                }
            }
            Ok(Value::Array(values))
        }
        0x10 => {
            let s = read_string(src)?;
            Ok(Value::String(s.into_bytes()))
        }
        0x0A => Ok(Value::Primitive(0x0A, vec![read_u8(src)?])),
        0x09 => {
            let mut raw = [0u8; 4];
            src.read_exact(&mut raw).map_err(|_| "EOF reading type 9".to_string())?;
            Ok(Value::Primitive(0x09, raw.to_vec()))
        }
        0x0F => {
            let mut raw = [0u8; 8];
            src.read_exact(&mut raw).map_err(|_| "EOF reading type 15".to_string())?;
            Ok(Value::Primitive(0x0F, raw.to_vec()))
        }
        0x15 => {
            let mut raw = [0u8; 8];
            src.read_exact(&mut raw).map_err(|_| "EOF reading type 21".to_string())?;
            Ok(Value::Primitive(0x15, raw.to_vec()))
        }
        _ => {
            let size = sizes.get(&type_code).copied().unwrap_or(0);
            if size > 0 {
                let mut raw = vec![0u8; size];
                src.read_exact(&mut raw).map_err(|_| "EOF reading primitive".to_string())?;
                Ok(Value::Primitive(type_code, raw))
            } else {
                Err(format!("unhandled v11 type code: {type_code}"))
            }
        }
    }
}

fn parse_v11_registry_table(src: &mut Cursor<&[u8]>, type_count: u32) -> Result<Option<Vec<TypeDef>>, String> {
    if type_count <= 0 {
        return Ok(None);
    }
    let start = src.position();
    let first = read_u32(src)?;
    if first != 0xFFFF_FFFF {
        src.set_position(start);
        return Ok(None);
    }
    src.set_position(start);
    let mut registry = Vec::new();
    for _ in 0..type_count {
        let idx = read_u32(src)?;
        if idx != 0xFFFF_FFFF {
            src.set_position(start);
            return Ok(None);
        }
        let name = read_string(src)?;
        let member_count = read_u32(src)?;
        let mut members = Vec::new();
        for _ in 0..member_count {
            let m_name = read_string(src)?;
            let m_type = read_u32(src)?;
            members.push((m_name, m_type));
        }
        registry.push(TypeDef { name, members });
    }
    Ok(Some(registry))
}

fn decode_inline(raw: &[u8]) -> Result<UNode, String> {
    // Minimal inline parser for tests; Painter 11 uses registry.
    let sizes = inline_sizes();
    let d = raw;
    let mut p = 12usize;

    fn u8(d: &[u8], p: &mut usize) -> Result<u8, String> {
        if *p >= d.len() {
            return Err("inline EOF".into());
        }
        let v = d[*p];
        *p += 1;
        Ok(v)
    }
    fn u16(d: &[u8], p: &mut usize) -> Result<u16, String> {
        if *p + 2 > d.len() {
            return Err("inline EOF".into());
        }
        let v = u16::from_le_bytes(d[*p..*p + 2].try_into().unwrap());
        *p += 2;
        Ok(v)
    }
    fn u32(d: &[u8], p: &mut usize) -> Result<u32, String> {
        if *p + 4 > d.len() {
            return Err("inline EOF".into());
        }
        let v = u32::from_le_bytes(d[*p..*p + 4].try_into().unwrap());
        *p += 4;
        Ok(v)
    }
    fn name(d: &[u8], p: &mut usize, ln: usize) -> Result<String, String> {
        if *p + ln > d.len() {
            return Err("inline EOF".into());
        }
        let s = String::from_utf8_lossy(&d[*p..*p + ln]).into_owned();
        *p += ln;
        Ok(s)
    }

    fn prim(d: &[u8], p: &mut usize, tag: u8, sizes: &std::collections::HashMap<u8, usize>) -> Result<Value, String> {
        let sz = sizes.get(&tag).copied().unwrap_or(0);
        if sz == 0 {
            return Err(format!("unknown inline primitive {tag}"));
        }
        if *p + sz > d.len() {
            return Err("inline EOF".into());
        }
        let v = d[*p..*p + sz].to_vec();
        *p += sz;
        Ok(Value::Primitive(tag as u32, v))
    }

    fn val(d: &[u8], p: &mut usize, sizes: &std::collections::HashMap<u8, usize>) -> Result<Value, String> {
        let tag = u8(d, p)?;
        if tag == 0x10 {
            let ln = u32(d, p)? as usize;
            if *p + ln > d.len() {
                return Err("inline EOF".into());
            }
            let v = d[*p..*p + ln].to_vec();
            *p += ln;
            return Ok(Value::String(v));
        }
        if tag == 0x12 || tag == 0x14 {
            if d[*p] == 0xFF {
                *p += 1;
                return Ok(Value::Object(None));
            }
            return Ok(Value::Object(Some(obj(d, p, sizes)?)));
        }
        if tag == 0x13 || tag == 0x11 {
            u32(d, p)?;
            let c = u32(d, p)? as usize;
            let mut els = Vec::new();
            for _ in 0..c {
                let e = u8(d, p)?;
                if e == 0x12 || e == 0x14 {
                    if d[*p] == 0xFF {
                        *p += 1;
                        els.push(Value::Object(None));
                    } else {
                        els.push(Value::Object(Some(obj(d, p, sizes)?)));
                    }
                } else if e == 0x10 {
                    let ln = u32(d, p)? as usize;
                    if *p + ln > d.len() {
                        return Err("inline EOF".into());
                    }
                    els.push(Value::String(d[*p..*p + ln].to_vec()));
                    *p += ln;
                } else {
                    els.push(prim(d, p, e, sizes)?);
                }
            }
            return Ok(Value::Array(els));
        }
        if tag == 0x00 {
            return Ok(Value::Null);
        }
        prim(d, p, tag, sizes)
    }

    fn obj(d: &[u8], p: &mut usize, sizes: &std::collections::HashMap<u8, usize>) -> Result<UNode, String> {
        u8(d, p)?;
        u32(d, p)?;
        let name_len = u32(d, p)? as usize;
        let nm = name(d, p, name_len)?;
        let fc = u16(d, p)? as usize;
        let mut fields = Vec::new();
        for _ in 0..fc {
            let field_len = u32(d, p)? as usize;
            let fn_ = name(d, p, field_len)?;
            fields.push((fn_, val(d, p, sizes)?));
        }
        Ok(UNode { type_name: nm, fields })
    }

    if u8(d, &mut p)? != 0x12 {
        return Err("inline stream does not start with root object".into());
    }
    obj(d, &mut p, &sizes)
}

fn inline_sizes() -> std::collections::HashMap<u8, usize> {
    // Subset from UPF primitive_sizes.json "inline" — extend when needed.
    [
        (1u8, 1),
        (2, 2),
        (3, 4),
        (4, 4),
        (5, 8),
        (6, 8),
        (7, 4),
        (8, 1),
        (9, 4),
        (10, 8),
        (11, 4),
        (12, 8),
        (13, 36),
        (14, 64),
        (15, 8),
    ]
    .into_iter()
    .collect()
}

pub fn field<'a>(node: &'a UNode, name: &str) -> Option<&'a Value> {
    node.fields.iter().find(|(n, _)| n == name).map(|(_, v)| v)
}

pub fn object_val(val: &Value) -> Option<&UNode> {
    match val {
        Value::Object(Some(n)) => Some(n),
        _ => None,
    }
}

pub fn array_items(val: &Value) -> &[Value] {
    match val {
        Value::Array(a) => a.as_slice(),
        _ => &[],
    }
}

pub fn string_val(val: &Value) -> Option<String> {
    match val {
        Value::String(b) => Some(String::from_utf8_lossy(b).into_owned()),
        _ => None,
    }
}

pub fn prim_bytes(val: &Value) -> Option<&[u8]> {
    match val {
        Value::Primitive(_, b) => Some(b.as_slice()),
        _ => None,
    }
}

pub fn prim_int(val: &Value) -> Option<i64> {
    let b = prim_bytes(val)?;
    match b.len() {
        1 => Some(b[0] as i8 as i64),
        2 => Some(i16::from_le_bytes(b[..2].try_into().unwrap()) as i64),
        4 => Some(i32::from_le_bytes(b[..4].try_into().unwrap()) as i64),
        8 => Some(i64::from_le_bytes(b[..8].try_into().unwrap())),
        _ => Some(i64::from_le_bytes(
            b.get(..8).unwrap_or(b).try_into().unwrap_or([0; 8]),
        )),
    }
}

pub fn prim_bool(val: &Value) -> Option<bool> {
    prim_int(val).map(|i| i != 0)
}
