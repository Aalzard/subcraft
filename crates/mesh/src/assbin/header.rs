use std::io::Cursor;

use byteorder::{LittleEndian, ReadBytesExt};

use crate::import::ImportError;

pub const MAGIC_LEN: usize = 44;
pub const HEADER_LEN: usize = 512;
pub const PAYLOAD_PRELUDE_LEN: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssbinHeader {
    pub format_major: u32,
    pub format_minor: u32,
    pub revision: u32,
    pub compile_flags: u32,
    pub shortened: bool,
    pub compressed: bool,
}

impl AssbinHeader {
    pub fn is_painter_v17(self) -> bool {
        self.format_major == 1 && self.format_minor >= 7 && !self.shortened && !self.compressed
    }
}

pub fn assbin_header(bytes: &[u8]) -> Result<AssbinHeader, ImportError> {
    if bytes.len() < HEADER_LEN {
        return Err(ImportError::Assbin(format!("assbin too small ({} bytes)", bytes.len())));
    }
    if !bytes.starts_with(b"ASSIMP.binary-dump.") {
        return Err(ImportError::Assbin("missing ASSIMP.binary-dump magic".into()));
    }
    let mut c = Cursor::new(&bytes[MAGIC_LEN..]);
    let format_major = c.read_u32::<LittleEndian>().map_err(io_err)?;
    let format_minor = c.read_u32::<LittleEndian>().map_err(io_err)?;
    let revision = c.read_u32::<LittleEndian>().map_err(io_err)?;
    let compile_flags = c.read_u32::<LittleEndian>().map_err(io_err)?;
    let shortened = c.read_u16::<LittleEndian>().map_err(io_err)? != 0;
    let compressed = c.read_u16::<LittleEndian>().map_err(io_err)? != 0;
    Ok(AssbinHeader {
        format_major,
        format_minor,
        revision,
        compile_flags,
        shortened,
        compressed,
    })
}

pub fn payload_offset(header: &AssbinHeader) -> usize {
    if header.is_painter_v17() {
        HEADER_LEN + PAYLOAD_PRELUDE_LEN
    } else {
        HEADER_LEN
    }
}

fn io_err(e: std::io::Error) -> ImportError {
    ImportError::Assbin(e.to_string())
}
