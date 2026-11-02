//! A small read-only 7z reader: just what Substance archives (`.sbsar`) use. Folders with a single
//! LZMA or Copy coder, a plain or LZMA-compressed header, files with names. Anything else is an
//! error. Checksums are not verified (the data comes from a project file we already trust as input,
//! and a wrong decode shows up as a size or format error further on).

#[derive(Debug, thiserror::Error)]
pub enum SevenZipError {
    #[error("not a 7z archive")]
    NotSevenZip,
    #[error("unsupported 7z feature: {0}")]
    Unsupported(String),
    #[error("corrupt 7z archive: {0}")]
    Corrupt(&'static str),
}

type Result<T> = std::result::Result<T, SevenZipError>;

fn corrupt<T>(what: &'static str) -> Result<T> {
    Err(SevenZipError::Corrupt(what))
}

/// Largest single decompressed stream accepted (guards against hostile headers).
const MAX_STREAM: u64 = 1 << 30;

// ----- LZMA -------------------------------------------------------------------------------------------

struct RangeDecoder<'a> {
    data: &'a [u8],
    pos: usize,
    range: u32,
    code: u32,
}

impl<'a> RangeDecoder<'a> {
    fn new(data: &'a [u8]) -> Result<Self> {
        if data.len() < 5 || data[0] != 0 {
            return corrupt("LZMA stream start");
        }
        let code = u32::from_be_bytes([data[1], data[2], data[3], data[4]]);
        Ok(Self { data, pos: 5, range: 0xffff_ffff, code })
    }

    fn next_byte(&mut self) -> Result<u32> {
        let b = *self.data.get(self.pos).ok_or(SevenZipError::Corrupt("LZMA stream ends early"))?;
        self.pos += 1;
        Ok(b as u32)
    }

    fn normalize(&mut self) -> Result<()> {
        if self.range < 1 << 24 {
            self.range <<= 8;
            self.code = (self.code << 8) | self.next_byte()?;
        }
        Ok(())
    }

    fn bit(&mut self, prob: &mut u16) -> Result<u32> {
        let bound = (self.range >> 11) * *prob as u32;
        let bit;
        if self.code < bound {
            self.range = bound;
            *prob += (2048 - *prob) >> 5;
            bit = 0;
        } else {
            self.range -= bound;
            self.code -= bound;
            *prob -= *prob >> 5;
            bit = 1;
        }
        self.normalize()?;
        Ok(bit)
    }

    fn direct(&mut self, n: u32) -> Result<u32> {
        let mut res = 0u32;
        for _ in 0..n {
            self.range >>= 1;
            self.code = self.code.wrapping_sub(self.range);
            let t = 0u32.wrapping_sub(self.code >> 31);
            self.code = self.code.wrapping_add(self.range & t);
            self.normalize()?;
            res = (res << 1).wrapping_add(t.wrapping_add(1));
        }
        Ok(res)
    }

    fn tree(&mut self, probs: &mut [u16], bits: u32) -> Result<u32> {
        let mut m = 1usize;
        for _ in 0..bits {
            m = (m << 1) + self.bit(&mut probs[m])? as usize;
        }
        Ok(m as u32 - (1 << bits))
    }

    fn reverse_tree(&mut self, probs: &mut [u16], bits: u32) -> Result<u32> {
        let (mut m, mut sym) = (1usize, 0u32);
        for i in 0..bits {
            let b = self.bit(&mut probs[m])?;
            m = (m << 1) + b as usize;
            sym |= b << i;
        }
        Ok(sym)
    }
}

struct LenDecoder {
    choice: u16,
    choice2: u16,
    low: [[u16; 8]; 16],
    mid: [[u16; 8]; 16],
    high: [u16; 256],
}

impl LenDecoder {
    fn new() -> Self {
        Self { choice: 1024, choice2: 1024, low: [[1024; 8]; 16], mid: [[1024; 8]; 16], high: [1024; 256] }
    }

    fn decode(&mut self, rc: &mut RangeDecoder, pos_state: usize) -> Result<u32> {
        if rc.bit(&mut self.choice)? == 0 {
            return rc.tree(&mut self.low[pos_state], 3);
        }
        if rc.bit(&mut self.choice2)? == 0 {
            return Ok(8 + rc.tree(&mut self.mid[pos_state], 3)?);
        }
        Ok(16 + rc.tree(&mut self.high, 8)?)
    }
}

/// Decompress a raw LZMA stream (no header) with the 5 property bytes of a 7z coder.
pub fn lzma_decompress(props: &[u8], data: &[u8], size: usize) -> Result<Vec<u8>> {
    if props.len() < 5 || props[0] >= 9 * 5 * 5 {
        return corrupt("LZMA properties");
    }
    let mut d = props[0] as u32;
    let lc = d % 9;
    d /= 9;
    let lp = d % 5;
    let pb = d / 5;
    let mut out: Vec<u8> = Vec::with_capacity(size);
    let mut rc = RangeDecoder::new(data)?;
    let mut literal = vec![1024u16; 0x300 << (lc + lp)];
    let mut is_match = [1024u16; 12 << 4];
    let mut is_rep = [1024u16; 12];
    let mut is_rep_g0 = [1024u16; 12];
    let mut is_rep_g1 = [1024u16; 12];
    let mut is_rep_g2 = [1024u16; 12];
    let mut is_rep0_long = [1024u16; 12 << 4];
    let mut pos_slot = [[1024u16; 64]; 4];
    let mut pos_decoders = [1024u16; 1 + 128 - 14];
    let mut align = [1024u16; 16];
    let mut len_dec = LenDecoder::new();
    let mut rep_len_dec = LenDecoder::new();
    let (mut state, mut rep0, mut rep1, mut rep2, mut rep3) = (0usize, 0u32, 0u32, 0u32, 0u32);

    while out.len() < size {
        let pos_state = out.len() & ((1 << pb) - 1);
        if rc.bit(&mut is_match[(state << 4) + pos_state])? == 0 {
            let prev = out.last().copied().unwrap_or(0) as usize;
            let base = 0x300 * (((out.len() & ((1 << lp) - 1)) << lc) + (prev >> (8 - lc)));
            let probs = &mut literal[base..base + 0x300];
            let mut symbol = 1usize;
            if state >= 7 {
                let mut match_byte = *out.get(out.len().wrapping_sub(rep0 as usize + 1)).ok_or(SevenZipError::Corrupt("LZMA distance"))? as usize;
                loop {
                    let match_bit = (match_byte >> 7) & 1;
                    match_byte <<= 1;
                    let bit = rc.bit(&mut probs[((1 + match_bit) << 8) + symbol])? as usize;
                    symbol = (symbol << 1) | bit;
                    if match_bit != bit || symbol >= 0x100 {
                        break;
                    }
                }
            }
            while symbol < 0x100 {
                symbol = (symbol << 1) | rc.bit(&mut probs[symbol])? as usize;
            }
            out.push(symbol as u8);
            state = if state < 4 { 0 } else if state < 10 { state - 3 } else { state - 6 };
            continue;
        }
        let len;
        if rc.bit(&mut is_rep[state])? != 0 {
            if out.is_empty() {
                return corrupt("LZMA repeat at start");
            }
            if rc.bit(&mut is_rep_g0[state])? == 0 {
                if rc.bit(&mut is_rep0_long[(state << 4) + pos_state])? == 0 {
                    state = if state < 7 { 9 } else { 11 };
                    let b = *out.get(out.len().wrapping_sub(rep0 as usize + 1)).ok_or(SevenZipError::Corrupt("LZMA distance"))?;
                    out.push(b);
                    continue;
                }
            } else {
                let dist;
                if rc.bit(&mut is_rep_g1[state])? == 0 {
                    dist = rep1;
                } else {
                    if rc.bit(&mut is_rep_g2[state])? == 0 {
                        dist = rep2;
                    } else {
                        dist = rep3;
                        rep3 = rep2;
                    }
                    rep2 = rep1;
                }
                rep1 = rep0;
                rep0 = dist;
            }
            len = rep_len_dec.decode(&mut rc, pos_state)?;
            state = if state < 7 { 8 } else { 11 };
        } else {
            rep3 = rep2;
            rep2 = rep1;
            rep1 = rep0;
            len = len_dec.decode(&mut rc, pos_state)?;
            state = if state < 7 { 7 } else { 10 };
            let len_state = len.min(3) as usize;
            let slot = rc.tree(&mut pos_slot[len_state], 6)?;
            rep0 = if slot < 4 {
                slot
            } else {
                let direct = (slot >> 1) - 1;
                let mut dist = (2 | (slot & 1)) << direct;
                if slot < 14 {
                    let base = (dist - slot) as usize;
                    dist += rc.reverse_tree(&mut pos_decoders[base..], direct)?;
                } else {
                    dist += rc.direct(direct - 4)? << 4;
                    dist += rc.reverse_tree(&mut align, 4)?;
                }
                dist
            };
            if rep0 == 0xffff_ffff {
                break; // end marker
            }
        }
        let len = len as usize + 2;
        let dist = rep0 as usize + 1;
        if dist > out.len() || out.len() + len > size {
            return corrupt("LZMA match");
        }
        let start = out.len() - dist;
        for k in 0..len {
            let b = out[start + k];
            out.push(b);
        }
    }
    if out.len() != size {
        return corrupt("LZMA size");
    }
    Ok(out)
}

// ----- the container -------------------------------------------------------------------------------------

struct Reader<'a> {
    d: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8> {
        let b = *self.d.get(self.p).ok_or(SevenZipError::Corrupt("header ends early"))?;
        self.p += 1;
        Ok(b)
    }

    /// 7z variable-length number.
    fn number(&mut self) -> Result<u64> {
        let first = self.byte()?;
        let mut mask = 0x80u8;
        let mut value = 0u64;
        for i in 0..8 {
            if first & mask == 0 {
                let high = (first & (mask.wrapping_sub(1))) as u64;
                return Ok(value | (high << (8 * i)));
            }
            value |= (self.byte()? as u64) << (8 * i);
            mask >>= 1;
        }
        Ok(value)
    }

    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let s = self.d.get(self.p..self.p.checked_add(n).ok_or(SevenZipError::Corrupt("length"))?).ok_or(SevenZipError::Corrupt("header ends early"))?;
        self.p += n;
        Ok(s)
    }

    fn bits(&mut self, n: usize) -> Result<Vec<bool>> {
        let bytes = self.take(n.div_ceil(8))?;
        Ok((0..n).map(|i| bytes[i / 8] & (0x80 >> (i % 8)) != 0).collect())
    }

    /// An "all defined" byte, then a bit vector when not all are.
    fn defined(&mut self, n: usize) -> Result<Vec<bool>> {
        if self.byte()? != 0 { Ok(vec![true; n]) } else { self.bits(n) }
    }
}

struct Coder {
    method: Vec<u8>,
    props: Vec<u8>,
}

struct Folder {
    coder: Coder,
    unpack_size: u64,
    crc_defined: bool,
}

#[derive(Default)]
struct Streams {
    pack_pos: u64,
    pack_sizes: Vec<u64>,
    folders: Vec<Folder>,
    /// Per folder: sizes of the files packed into it.
    substreams: Vec<Vec<u64>>,
}

fn read_streams_info(r: &mut Reader) -> Result<Streams> {
    let mut s = Streams::default();
    loop {
        match r.byte()? {
            0x00 => break,
            0x06 => {
                s.pack_pos = r.number()?;
                let n = r.number()? as usize;
                if n > 1 << 20 {
                    return corrupt("pack stream count");
                }
                loop {
                    match r.byte()? {
                        0x00 => break,
                        0x09 => s.pack_sizes = (0..n).map(|_| r.number()).collect::<Result<_>>()?,
                        0x0a => {
                            let def = r.defined(n)?;
                            r.take(4 * def.iter().filter(|d| **d).count())?;
                        }
                        _ => return corrupt("pack info"),
                    }
                }
            }
            0x07 => {
                if r.byte()? != 0x0b {
                    return corrupt("folder list");
                }
                let n = r.number()? as usize;
                if n > 1 << 20 || r.byte()? != 0 {
                    return Err(SevenZipError::Unsupported("external folders".into()));
                }
                let mut coders = Vec::new();
                for _ in 0..n {
                    let num_coders = r.number()?;
                    if num_coders != 1 {
                        return Err(SevenZipError::Unsupported(format!("{num_coders} coders in a folder")));
                    }
                    let flag = r.byte()?;
                    if flag & 0x10 != 0 {
                        return Err(SevenZipError::Unsupported("complex coder".into()));
                    }
                    let method = r.take((flag & 0x0f) as usize)?.to_vec();
                    let props = if flag & 0x20 != 0 {
                        let n = r.number()? as usize;
                        r.take(n)?.to_vec()
                    } else {
                        Vec::new()
                    };
                    coders.push(Coder { method, props });
                }
                if r.byte()? != 0x0c {
                    return corrupt("unpack sizes");
                }
                let mut folders: Vec<Folder> = Vec::new();
                for coder in coders {
                    folders.push(Folder { coder, unpack_size: r.number()?, crc_defined: false });
                }
                loop {
                    match r.byte()? {
                        0x00 => break,
                        0x0a => {
                            let def = r.defined(n)?;
                            for (f, d) in folders.iter_mut().zip(&def) {
                                f.crc_defined = *d;
                            }
                            r.take(4 * def.iter().filter(|d| **d).count())?;
                        }
                        _ => return corrupt("unpack info"),
                    }
                }
                s.folders = folders;
            }
            0x08 => {
                let mut counts: Vec<u64> = vec![1; s.folders.len()];
                let mut sizes_read = false;
                loop {
                    match r.byte()? {
                        0x00 => break,
                        0x0d => counts = (0..s.folders.len()).map(|_| r.number()).collect::<Result<_>>()?,
                        0x09 => {
                            s.substreams.clear();
                            for (f, &c) in s.folders.iter().zip(&counts) {
                                if c == 0 {
                                    s.substreams.push(Vec::new());
                                    continue;
                                }
                                if c > 1 << 20 {
                                    return corrupt("substream count");
                                }
                                let mut v = Vec::new();
                                let mut sum = 0u64;
                                for _ in 0..c - 1 {
                                    let n = r.number()?;
                                    sum = sum.checked_add(n).ok_or(SevenZipError::Corrupt("substream sizes"))?;
                                    v.push(n);
                                }
                                v.push(f.unpack_size.checked_sub(sum).ok_or(SevenZipError::Corrupt("substream sizes"))?);
                                s.substreams.push(v);
                            }
                            sizes_read = true;
                        }
                        0x0a => {
                            let unknown: usize = s.folders.iter().zip(&counts).map(|(f, &c)| if c == 1 && f.crc_defined { 0 } else { c as usize }).sum();
                            let def = r.defined(unknown)?;
                            r.take(4 * def.iter().filter(|d| **d).count())?;
                        }
                        _ => return corrupt("substreams info"),
                    }
                }
                if !sizes_read {
                    s.substreams = s.folders.iter().zip(&counts).map(|(f, &c)| if c == 0 { Vec::new() } else { vec![f.unpack_size] }).collect();
                }
            }
            _ => return corrupt("streams info"),
        }
    }
    if s.substreams.is_empty() {
        s.substreams = s.folders.iter().map(|f| vec![f.unpack_size]).collect();
    }
    Ok(s)
}

fn decode_folder(archive: &[u8], s: &Streams, index: usize) -> Result<Vec<u8>> {
    let mut offset = 32u64 + s.pack_pos;
    for size in s.pack_sizes.iter().take(index) {
        offset += size;
    }
    let size = *s.pack_sizes.get(index).ok_or(SevenZipError::Corrupt("pack stream"))?;
    let packed = archive.get(offset as usize..(offset + size) as usize).ok_or(SevenZipError::Corrupt("pack stream range"))?;
    let folder = &s.folders[index];
    if folder.unpack_size > MAX_STREAM {
        return Err(SevenZipError::Unsupported("huge stream".into()));
    }
    match folder.coder.method.as_slice() {
        [] | [0x00] => Ok(packed.to_vec()),
        [0x03, 0x01, 0x01] => lzma_decompress(&folder.coder.props, packed, folder.unpack_size as usize),
        m => Err(SevenZipError::Unsupported(format!("coder {m:02x?}"))),
    }
}

/// One file of the archive.
pub struct Entry {
    pub name: String,
    pub data: Vec<u8>,
}

/// Read every file of a 7z archive.
pub fn read_all(archive: &[u8]) -> Result<Vec<Entry>> {
    if archive.len() < 32 || archive[..6] != [b'7', b'z', 0xbc, 0xaf, 0x27, 0x1c] {
        return Err(SevenZipError::NotSevenZip);
    }
    let le64 = |o: usize| u64::from_le_bytes(archive[o..o + 8].try_into().unwrap());
    let (next_off, next_size) = (le64(12), le64(20));
    if next_size == 0 {
        return Ok(Vec::new());
    }
    let start = 32u64.checked_add(next_off).ok_or(SevenZipError::Corrupt("header offset"))? as usize;
    let end = start.checked_add(next_size as usize).ok_or(SevenZipError::Corrupt("header size"))?;
    let mut header = archive.get(start..end).ok_or(SevenZipError::Corrupt("header range"))?.to_vec();
    // An encoded header is a streams-info block describing the real (compressed) header.
    for _ in 0..4 {
        if header.first() != Some(&0x17) {
            break;
        }
        let mut r = Reader { d: &header, p: 1 };
        let s = read_streams_info(&mut r)?;
        if s.folders.is_empty() {
            return corrupt("encoded header");
        }
        header = decode_folder(archive, &s, 0)?;
    }
    let mut r = Reader { d: &header, p: 0 };
    if r.byte()? != 0x01 {
        return corrupt("header");
    }
    let mut streams = Streams::default();
    let mut names: Vec<String> = Vec::new();
    let mut empty_stream: Vec<bool> = Vec::new();
    loop {
        match r.byte()? {
            0x00 => break,
            0x02 | 0x03 => return Err(SevenZipError::Unsupported("archive properties".into())),
            0x04 => streams = read_streams_info(&mut r)?,
            0x05 => {
                let n = r.number()? as usize;
                if n > 1 << 20 {
                    return corrupt("file count");
                }
                loop {
                    let kind = r.byte()?;
                    if kind == 0 {
                        break;
                    }
                    let size = r.number()? as usize;
                    let body = r.take(size)?.to_vec();
                    match kind {
                        0x11 => {
                            if body.first() != Some(&0) {
                                return Err(SevenZipError::Unsupported("external names".into()));
                            }
                            let units: Vec<u16> = body[1..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                            names = units.split(|u| *u == 0).take(n).map(|s| String::from_utf16_lossy(s)).collect();
                        }
                        0x0e => empty_stream = Reader { d: &body, p: 0 }.bits(n)?,
                        _ => {}
                    }
                }
                if names.len() != n {
                    names.resize(n, String::new());
                }
                if empty_stream.len() != n {
                    empty_stream = vec![false; n];
                }
            }
            _ => return corrupt("header"),
        }
    }
    let mut out = Vec::new();
    let mut folder_index = 0;
    let mut sub_index = 0;
    let mut folder_data: Option<(usize, Vec<u8>)> = None;
    let mut offset = 0usize;
    for (name, empty) in names.into_iter().zip(empty_stream) {
        if empty {
            continue; // directory or empty file
        }
        while folder_index < streams.substreams.len() && sub_index >= streams.substreams[folder_index].len() {
            folder_index += 1;
            sub_index = 0;
            offset = 0;
        }
        if folder_index >= streams.substreams.len() {
            return corrupt("more files than streams");
        }
        if folder_data.as_ref().map(|(i, _)| *i) != Some(folder_index) {
            folder_data = Some((folder_index, decode_folder(archive, &streams, folder_index)?));
        }
        let size = streams.substreams[folder_index][sub_index] as usize;
        let (_, data) = folder_data.as_ref().unwrap();
        let slice = data.get(offset..offset + size).ok_or(SevenZipError::Corrupt("file range"))?;
        out.push(Entry { name: name.replace('\\', "/"), data: slice.to_vec() });
        offset += size;
        sub_index += 1;
    }
    Ok(out)
}
