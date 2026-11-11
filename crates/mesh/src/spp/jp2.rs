//! A small JPEG 2000 decoder for the baked-map tiles inside Substance 3D Painter projects.
//!
//! Painter stores each 512x512 tile of a baked map as a JP2 file written by OpenJPEG: lossless
//! (reversible 5/3 wavelet, optional reversible colour transform), 16 bits per component, one tile,
//! default 64x64 code blocks and no precincts. This decoder handles exactly that family (all
//! progression orders, several layers, SOP/EPH markers, one or more tile-parts). Anything else
//! (irreversible 9/7, code-block style flags, user precincts, several tiles, subsampling) is
//! reported as [`Jp2Error::Unsupported`] instead of being guessed at.

#[derive(Debug, thiserror::Error)]
pub enum Jp2Error {
    #[error("unsupported JPEG 2000 feature: {0}")]
    Unsupported(String),
    #[error("corrupt JPEG 2000 data: {0}")]
    Corrupt(&'static str),
}

type Result<T> = std::result::Result<T, Jp2Error>;

/// Decoded samples, interleaved (`components` values per pixel), unsigned, `precision` bits each.
#[derive(Debug, Clone)]
pub struct Decoded {
    pub width: usize,
    pub height: usize,
    pub components: usize,
    pub precision: u32,
    /// How many of the finest wavelet levels were left out: `width` and `height` are the image's
    /// size divided by `2^reduced` (rounded up).
    pub reduced: usize,
    pub samples: Vec<u16>,
}

fn corrupt<T>(what: &'static str) -> Result<T> {
    Err(Jp2Error::Corrupt(what))
}

fn unsupported<T>(what: impl Into<String>) -> Result<T> {
    Err(Jp2Error::Unsupported(what.into()))
}

/// Largest image side accepted from a codestream.
const MAX_SIDE: usize = 16384;

/// A marker segment's body (after its marker and length), checked against the codestream's end.
fn segment(cs: &[u8], p: usize) -> Result<&[u8]> {
    let l = be16(cs, p + 2)?;
    if l < 2 {
        return corrupt("marker segment length");
    }
    cs.get(p + 4..p + 2 + l).ok_or(Jp2Error::Corrupt("marker segment past the end"))
}

fn be16(b: &[u8], o: usize) -> Result<usize> {
    b.get(o..o + 2).map(|s| u16::from_be_bytes([s[0], s[1]]) as usize).ok_or(Jp2Error::Corrupt("short marker"))
}

fn be32(b: &[u8], o: usize) -> Result<usize> {
    b.get(o..o + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]) as usize).ok_or(Jp2Error::Corrupt("short marker"))
}

#[derive(Default, Clone)]
struct Params {
    width: usize,
    height: usize,
    tile_w: usize,
    tile_h: usize,
    components: usize,
    precision: u32,
    progression: u8,
    layers: usize,
    mct: bool,
    levels: usize,
    xcb: usize,
    ycb: usize,
    sop: bool,
    eph: bool,
    guard: u32,
    exponents: Vec<u32>,
}

/// Decode a JP2 file or a raw codestream.
pub fn decode(data: &[u8]) -> Result<Decoded> {
    decode_reduced(data, 0)
}

/// Decode at `1 / 2^reduce` of the size (clamped to the number of wavelet levels): the finest
/// levels' code blocks are never entropy decoded, so this is much faster than decoding in full.
pub fn decode_reduced(data: &[u8], reduce: usize) -> Result<Decoded> {
    let start = find_codestream(data)?;
    decode_codestream(&data[start..], reduce)
}

fn find_codestream(data: &[u8]) -> Result<usize> {
    if data.starts_with(&[0xff, 0x4f]) {
        return Ok(0);
    }
    let mut p = 0;
    while p + 8 <= data.len() {
        let mut len = be32(data, p)?;
        let kind = &data[p + 4..p + 8];
        let mut header = 8;
        if len == 1 {
            len = data.get(p + 8..p + 16).map(|s| u64::from_be_bytes(s.try_into().unwrap()) as usize).ok_or(Jp2Error::Corrupt("box"))?;
            header = 16;
        } else if len == 0 {
            len = data.len() - p;
        }
        if kind == b"jp2c" {
            return Ok(p + header);
        }
        if len < header {
            return corrupt("box length");
        }
        p += len;
    }
    corrupt("no codestream")
}

fn decode_codestream(cs: &[u8], reduce: usize) -> Result<Decoded> {
    if !cs.starts_with(&[0xff, 0x4f]) {
        return corrupt("missing SOC");
    }
    let mut prm = Params::default();
    let mut p = 2;
    let mut tile_data: Vec<u8> = Vec::new();
    let mut seen_cod = false;
    let mut seen_qcd = false;
    loop {
        let marker = be16(cs, p)?;
        match marker {
            0xff51 => {
                let b = segment(cs, p)?;
                if be16(b, 0)? != 0 {
                    return unsupported("capabilities (Rsiz)");
                }
                let (xs, ys, xo, yo) = (be32(b, 2)?, be32(b, 6)?, be32(b, 10)?, be32(b, 14)?);
                let (xt, yt, xto, yto) = (be32(b, 18)?, be32(b, 22)?, be32(b, 26)?, be32(b, 30)?);
                if xo != 0 || yo != 0 || xto != 0 || yto != 0 {
                    return unsupported("image or tile offsets");
                }
                prm.width = xs;
                prm.height = ys;
                prm.tile_w = xt;
                prm.tile_h = yt;
                prm.components = be16(b, 34)?;
                if xt < xs || yt < ys {
                    return unsupported("several tiles");
                }
                // Sizes come from the file: bound them before anything is allocated from them (a
                // huge size aborted the process). Painter's tiles are far below this.
                if prm.components == 0 || prm.components > 4 || prm.width == 0 || prm.height == 0 || prm.width > MAX_SIDE || prm.height > MAX_SIDE {
                    return corrupt("image size");
                }
                if b.len() < 36 + 3 * prm.components {
                    return corrupt("image header");
                }
                for c in 0..prm.components {
                    let ssiz = b[36 + c * 3] as u32;
                    if ssiz & 0x80 != 0 {
                        return unsupported("signed components");
                    }
                    if b[37 + c * 3] != 1 || b[38 + c * 3] != 1 {
                        return unsupported("subsampled components");
                    }
                    let prec = (ssiz & 0x7f) + 1;
                    if c == 0 {
                        prm.precision = prec;
                    } else if prec != prm.precision {
                        return unsupported("components with different precision");
                    }
                }
                if prm.precision > 16 {
                    return unsupported("more than 16 bits per component");
                }
            }
            0xff52 => {
                let b = segment(cs, p)?;
                if b.len() < 10 {
                    return corrupt("coding style");
                }
                let scod = b[0];
                if scod & 1 != 0 {
                    return unsupported("user-defined precincts");
                }
                prm.sop = scod & 2 != 0;
                prm.eph = scod & 4 != 0;
                prm.progression = b[1];
                prm.layers = be16(b, 2)?;
                prm.mct = b[4] != 0;
                prm.levels = b[5] as usize;
                prm.xcb = (b[6] & 15) as usize + 2;
                prm.ycb = (b[7] & 15) as usize + 2;
                if b[8] != 0 {
                    return unsupported(format!("code-block style {:#x}", b[8]));
                }
                if b[9] != 1 {
                    return unsupported("irreversible 9/7 wavelet (lossy)");
                }
                if prm.levels > 32 || prm.xcb > 10 || prm.ycb > 10 || prm.xcb + prm.ycb > 12 || prm.layers == 0 {
                    return corrupt("coding style");
                }
                seen_cod = true;
            }
            0xff5c => {
                let b = segment(cs, p)?;
                if b.is_empty() {
                    return corrupt("quantisation");
                }
                if b[0] & 31 != 0 {
                    return unsupported("quantisation (lossy)");
                }
                prm.guard = (b[0] >> 5) as u32;
                prm.exponents = b[1..].iter().map(|v| (*v >> 3) as u32).collect();
                seen_qcd = true;
            }
            0xff53 | 0xff5d => return unsupported("per-component coding or quantisation overrides"),
            0xff90 => {
                // Start of tile-part.
                if !seen_cod || !seen_qcd {
                    return corrupt("missing COD or QCD");
                }
                let psot = be32(cs, p + 6)?;
                let tile_index = be16(cs, p + 4)?;
                if tile_index != 0 {
                    return unsupported("several tiles");
                }
                let end = if psot == 0 { cs.len() } else { (p + psot).min(cs.len()) };
                let mut q = p + 12;
                loop {
                    let m = be16(cs, q)?;
                    if m == 0xff93 {
                        q += 2;
                        break;
                    }
                    q += 2 + be16(cs, q + 2)?;
                }
                let mut stop = end;
                if psot == 0 && cs[..stop].ends_with(&[0xff, 0xd9]) {
                    stop -= 2;
                }
                tile_data.extend_from_slice(&cs[q.min(stop)..stop]);
                p = end;
                if be16(cs, p).map(|m| m == 0xff90).unwrap_or(false) {
                    continue;
                }
                break;
            }
            0xffd9 => break,
            _ => {}
        }
        p += 2 + be16(cs, p + 2)?;
    }
    if tile_data.is_empty() {
        return corrupt("no tile data");
    }
    // No SIZ segment (its marker was damaged): nothing sized the image, and precision 0 would
    // underflow below.
    if prm.precision == 0 || prm.width == 0 || prm.height == 0 || prm.components == 0 {
        return corrupt("missing image header");
    }
    decode_tile(&prm, &tile_data, reduce)
}

// ----- tier 2 ---------------------------------------------------------------------------------------

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u32,
    bits: u32,
}

impl<'a> BitReader<'a> {
    fn bit(&mut self) -> Result<u32> {
        if self.bits == 0 {
            let prev_ff = self.buf == 0xff;
            let byte = *self.data.get(self.pos).ok_or(Jp2Error::Corrupt("packet header past end"))?;
            self.pos += 1;
            self.buf = byte as u32;
            self.bits = if prev_ff { 7 } else { 8 };
        }
        self.bits -= 1;
        Ok((self.buf >> self.bits) & 1)
    }

    fn bits(&mut self, n: u32) -> Result<u32> {
        let mut v = 0;
        for _ in 0..n {
            v = (v << 1) | self.bit()?;
        }
        Ok(v)
    }

    /// Finish the header: align to a byte, and skip the stuffed byte after a trailing 0xFF.
    fn align(&mut self) {
        self.bits = 0;
        if self.buf == 0xff {
            self.pos += 1;
        }
        self.buf = 0;
    }
}

struct TagTree {
    // Per level, (width, height, offset into nodes); level 0 = leaves.
    levels: Vec<(usize, usize, usize)>,
    value: Vec<u32>,
    low: Vec<u32>,
}

impl TagTree {
    fn new(w: usize, h: usize) -> Self {
        let mut levels = Vec::new();
        let (mut cw, mut ch, mut off) = (w, h, 0);
        loop {
            levels.push((cw, ch, off));
            off += cw * ch;
            if cw <= 1 && ch <= 1 {
                break;
            }
            cw = cw.div_ceil(2);
            ch = ch.div_ceil(2);
        }
        Self { levels, value: vec![u32::MAX; off], low: vec![0; off] }
    }

    /// True when the leaf's value is known to be below `threshold`.
    fn decode(&mut self, br: &mut BitReader, x: usize, y: usize, threshold: u32) -> Result<bool> {
        let mut low = 0;
        let mut node = 0;
        for level in (0..self.levels.len()).rev() {
            let (w, _, off) = self.levels[level];
            node = off + (y >> level) * w + (x >> level);
            if low > self.low[node] {
                self.low[node] = low;
            } else {
                low = self.low[node];
            }
            while low < threshold && low < self.value[node] {
                if br.bit()? == 1 {
                    self.value[node] = low;
                } else {
                    low += 1;
                }
            }
            self.low[node] = low;
        }
        Ok(self.value[node] < threshold)
    }
}

struct Block {
    x0: usize,
    y0: usize,
    w: usize,
    h: usize,
    included: bool,
    zero_planes: u32,
    lblock: u32,
    passes: u32,
    data: Vec<u8>,
}

struct Band {
    /// 0 = LL, 1 = HL, 2 = LH, 3 = HH.
    orient: u8,
    /// Position of the band in the Mallat-layout coefficient plane.
    px: usize,
    py: usize,
    w: usize,
    h: usize,
    mb: u32,
    gw: usize,
    blocks: Vec<Block>,
    incl: TagTree,
    zbp: TagTree,
}

fn build_bands(prm: &Params, res: usize) -> Vec<Band> {
    let l = prm.levels;
    // Resolution `res` has the size of the image reduced `l - res` times; its high bands come from
    // the next-finer split.
    let dim = |n: usize, k: usize| n.div_ceil(1 << k);
    let (rw, rh) = (dim(prm.width, l - res), dim(prm.height, l - res));
    let (cw, ch) = (prm.xcb.min(15), prm.ycb.min(15));
    let mut bands = Vec::new();
    let mk = |orient: u8, px: usize, py: usize, w: usize, h: usize, band_index: usize| {
        let eps = prm.exponents.get(band_index).copied().unwrap_or(0);
        let (gw, gh) = (w.div_ceil(1 << cw), h.div_ceil(1 << ch));
        let mut blocks = Vec::new();
        for by in 0..gh {
            for bx in 0..gw {
                let (x0, y0) = (bx << cw, by << ch);
                blocks.push(Block { x0, y0, w: (1 << cw).min(w - x0), h: (1 << ch).min(h - y0), included: false, zero_planes: 0, lblock: 3, passes: 0, data: Vec::new() });
            }
        }
        Band { orient, px, py, w, h, mb: (prm.guard + eps).saturating_sub(1), gw, blocks, incl: TagTree::new(gw.max(1), gh.max(1)), zbp: TagTree::new(gw.max(1), gh.max(1)) }
    };
    if res == 0 {
        bands.push(mk(0, 0, 0, rw, rh, 0));
    } else {
        let (lw, lh) = (rw.div_ceil(2), rh.div_ceil(2));
        let base = 1 + (res - 1) * 3;
        bands.push(mk(1, lw, 0, rw - lw, lh, base));
        bands.push(mk(2, 0, lh, lw, rh - lh, base + 1));
        bands.push(mk(3, lw, lh, rw - lw, rh - lh, base + 2));
    }
    bands
}

fn read_packet(prm: &Params, data: &[u8], pos: &mut usize, bands: &mut [Band], layer: usize) -> Result<()> {
    if prm.sop && data.get(*pos..*pos + 2) == Some(&[0xff, 0x91]) {
        *pos += 6;
    }
    let mut br = BitReader { data, pos: *pos, buf: 0, bits: 0 };
    let mut included: Vec<(usize, usize, usize)> = Vec::new();
    if br.bit()? == 1 {
        for (bi, band) in bands.iter_mut().enumerate() {
            if band.w == 0 || band.h == 0 {
                continue;
            }
            for k in 0..band.blocks.len() {
                let (bx, by) = (k % band.gw, k / band.gw);
                let first = !band.blocks[k].included;
                let inc = if first { band.incl.decode(&mut br, bx, by, layer as u32 + 1)? } else { br.bit()? == 1 };
                if !inc {
                    continue;
                }
                if first {
                    let mut t = 1;
                    while !band.zbp.decode(&mut br, bx, by, t)? {
                        t += 1;
                        if t > 64 {
                            return corrupt("zero bit-plane count");
                        }
                    }
                    band.blocks[k].zero_planes = t - 1;
                    band.blocks[k].included = true;
                }
                let passes = if br.bit()? == 0 {
                    1
                } else if br.bit()? == 0 {
                    2
                } else {
                    let v = br.bits(2)?;
                    if v < 3 {
                        3 + v
                    } else {
                        let v = br.bits(5)?;
                        if v < 31 { 6 + v } else { 37 + br.bits(7)? }
                    }
                };
                while br.bit()? == 1 {
                    band.blocks[k].lblock += 1;
                }
                let bits = band.blocks[k].lblock + (31 - passes.leading_zeros());
                if bits > 32 {
                    return corrupt("code-block length");
                }
                let len = br.bits(bits)? as usize;
                band.blocks[k].passes += passes;
                included.push((bi, k, len));
            }
        }
    }
    br.align();
    *pos = br.pos;
    if prm.eph && data.get(*pos..*pos + 2) == Some(&[0xff, 0x92]) {
        *pos += 2;
    }
    for (bi, k, len) in included {
        let chunk = data.get(*pos..*pos + len).ok_or(Jp2Error::Corrupt("code-block data past end"))?;
        bands[bi].blocks[k].data.extend_from_slice(chunk);
        *pos += len;
    }
    Ok(())
}

// ----- tier 1 ---------------------------------------------------------------------------------------

const QE: [(u32, u8, u8, u8); 47] = [
    (0x5601, 1, 1, 1), (0x3401, 2, 6, 0), (0x1801, 3, 9, 0), (0x0ac1, 4, 12, 0), (0x0521, 5, 29, 0), (0x0221, 38, 33, 0),
    (0x5601, 7, 6, 1), (0x5401, 8, 14, 0), (0x4801, 9, 14, 0), (0x3801, 10, 14, 0), (0x3001, 11, 17, 0), (0x2401, 12, 18, 0),
    (0x1c01, 13, 20, 0), (0x1601, 29, 21, 0), (0x5601, 15, 14, 1), (0x5401, 16, 14, 0), (0x5101, 17, 15, 0), (0x4801, 18, 16, 0),
    (0x3801, 19, 17, 0), (0x3401, 20, 18, 0), (0x3001, 21, 19, 0), (0x2801, 22, 19, 0), (0x2401, 23, 20, 0), (0x2201, 24, 21, 0),
    (0x1c01, 25, 22, 0), (0x1801, 26, 23, 0), (0x1601, 27, 24, 0), (0x1401, 28, 25, 0), (0x1201, 29, 26, 0), (0x1101, 30, 27, 0),
    (0x0ac1, 31, 28, 0), (0x09c1, 32, 29, 0), (0x08a1, 33, 30, 0), (0x0521, 34, 31, 0), (0x0441, 35, 32, 0), (0x02a1, 36, 33, 0),
    (0x0221, 37, 34, 0), (0x0141, 38, 35, 0), (0x0111, 39, 36, 0), (0x0085, 40, 37, 0), (0x0049, 41, 38, 0), (0x0025, 42, 39, 0),
    (0x0015, 43, 40, 0), (0x0009, 44, 41, 0), (0x0005, 45, 42, 0), (0x0001, 45, 43, 0), (0x5601, 46, 46, 0),
];

const CTX_RUN: usize = 17;
const CTX_UNI: usize = 18;

struct Mq<'a> {
    data: &'a [u8],
    bp: usize,
    c: u32,
    a: u32,
    ct: u32,
    state: [u8; 19],
    mps: [u8; 19],
}

impl<'a> Mq<'a> {
    fn new(data: &'a [u8]) -> Self {
        let mut state = [0u8; 19];
        state[0] = 4;
        state[CTX_RUN] = 3;
        state[CTX_UNI] = 46;
        let mut mq = Mq { data, bp: 0, c: 0, a: 0x8000, ct: 0, state, mps: [0; 19] };
        mq.c = (mq.byte(0) as u32) << 16;
        mq.bytein();
        mq.c <<= 7;
        mq.ct -= 7;
        mq
    }

    fn byte(&self, i: usize) -> u8 {
        self.data.get(i).copied().unwrap_or(0xff)
    }

    fn bytein(&mut self) {
        if self.byte(self.bp) == 0xff {
            if self.byte(self.bp + 1) > 0x8f {
                self.c += 0xff00;
                self.ct = 8;
            } else {
                self.bp += 1;
                self.c += (self.byte(self.bp) as u32) << 9;
                self.ct = 7;
            }
        } else {
            self.bp += 1;
            self.c += (self.byte(self.bp) as u32) << 8;
            self.ct = 8;
        }
    }

    fn renorm(&mut self) {
        loop {
            if self.ct == 0 {
                self.bytein();
            }
            self.a <<= 1;
            self.c <<= 1;
            self.ct -= 1;
            if self.a & 0x8000 != 0 {
                break;
            }
        }
    }

    fn decode(&mut self, cx: usize) -> u32 {
        let (qe, nmps, nlps, sw) = QE[self.state[cx] as usize];
        self.a -= qe;
        let d;
        if (self.c >> 16) < qe {
            // LPS exchange.
            if self.a < qe {
                d = self.mps[cx] as u32;
                self.state[cx] = nmps;
            } else {
                d = 1 - self.mps[cx] as u32;
                if sw == 1 {
                    self.mps[cx] ^= 1;
                }
                self.state[cx] = nlps;
            }
            self.a = qe;
            self.renorm();
        } else {
            self.c -= qe << 16;
            if self.a & 0x8000 == 0 {
                // MPS exchange.
                if self.a < qe {
                    d = 1 - self.mps[cx] as u32;
                    if sw == 1 {
                        self.mps[cx] ^= 1;
                    }
                    self.state[cx] = nlps;
                } else {
                    d = self.mps[cx] as u32;
                    self.state[cx] = nmps;
                }
                self.renorm();
            } else {
                d = self.mps[cx] as u32;
            }
        }
        d
    }
}

// Per-sample state: bits 0..=7 hold which of the eight neighbours are significant (N, S, W, E, NW,
// NE, SW, SE), bits 8..=11 the signs of N, S, W, E, and the rest the sample's own state.
const F_SIG: u16 = 1 << 12;
const F_VISITED: u16 = 1 << 13;
const F_REFINED: u16 = 1 << 14;
const F_NEG: u16 = 1 << 15;
const NEIGHBOURS: u16 = 0xff;

struct Tables {
    /// Zero-coding context by orientation (LL, HL, LH, HH) and neighbour bits.
    zc: [[u8; 256]; 4],
    /// Sign-coding (context, xor bit) by significance and sign bits of N, S, W, E.
    sc: [(u8, u8); 256],
}

fn zero_coding_context(orient: usize, nb: usize) -> u8 {
    let bit = |k: usize| (nb >> k) & 1;
    let (v, h, d) = (bit(0) + bit(1), bit(2) + bit(3), bit(4) + bit(5) + bit(6) + bit(7));
    let (h, v) = if orient == 1 { (v, h) } else { (h, v) };
    if orient == 3 {
        let hv = h + v;
        match d {
            0 => hv.min(2) as u8,
            1 => 3 + hv.min(2) as u8,
            2 => if hv >= 1 { 7 } else { 6 },
            _ => 8,
        }
    } else if h == 2 {
        8
    } else if h == 1 {
        if v >= 1 { 7 } else if d >= 1 { 6 } else { 5 }
    } else if v == 2 {
        4
    } else if v == 1 {
        3
    } else if d >= 2 {
        2
    } else {
        d as u8
    }
}

fn tables() -> &'static Tables {
    static T: std::sync::OnceLock<Tables> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut zc = [[0u8; 256]; 4];
        for (orient, row) in zc.iter_mut().enumerate() {
            for (nb, out) in row.iter_mut().enumerate() {
                *out = zero_coding_context(orient, nb);
            }
        }
        let mut sc = [(0u8, 0u8); 256];
        for (idx, out) in sc.iter_mut().enumerate() {
            let contrib = |k: usize| -> i32 {
                if (idx >> k) & 1 == 0 { 0 } else if (idx >> (4 + k)) & 1 == 1 { -1 } else { 1 }
            };
            let v = (contrib(0) + contrib(1)).clamp(-1, 1);
            let h = (contrib(2) + contrib(3)).clamp(-1, 1);
            *out = match (h, v) {
                (1, 1) => (13, 0),
                (1, 0) => (12, 0),
                (1, -1) => (11, 0),
                (0, 1) => (10, 0),
                (0, 0) => (9, 0),
                (0, -1) => (10, 1),
                (-1, 1) => (11, 1),
                (-1, 0) => (12, 1),
                _ => (13, 1),
            };
        }
        Tables { zc, sc }
    })
}

/// Mark sample `i` significant and tell its eight neighbours.
fn set_significant(flags: &mut [u16], i: usize, fw: usize, neg: bool) {
    flags[i] |= F_SIG | if neg { F_NEG } else { 0 };
    let n = neg as u16;
    flags[i - fw] |= 2 | n << 9;
    flags[i + fw] |= 1 | n << 8;
    flags[i - 1] |= 8 | n << 11;
    flags[i + 1] |= 4 | n << 10;
    flags[i - fw - 1] |= 128;
    flags[i - fw + 1] |= 64;
    flags[i + fw - 1] |= 32;
    flags[i + fw + 1] |= 16;
}

fn decode_sign(mq: &mut Mq, t: &Tables, f: u16) -> bool {
    let idx = ((f & 0xf) | ((f >> 8) & 0xf) << 4) as usize;
    let (cx, xor) = t.sc[idx];
    (mq.decode(cx as usize) ^ xor as u32) == 1
}

/// Decode one code block into signed integers (row-major, `w * h`).
fn decode_block(blk: &Block, orient: u8, mb: u32) -> Result<Vec<i32>> {
    let (w, h) = (blk.w, blk.h);
    let mut out = vec![0i32; w * h];
    if blk.passes == 0 || blk.data.is_empty() || blk.zero_planes >= mb {
        return Ok(out);
    }
    let planes = mb - blk.zero_planes;
    if planes > 30 {
        return corrupt("too many bit-planes");
    }
    let t = tables();
    let zc = &t.zc[orient as usize];
    // One-sample border so neighbour updates need no bounds checks.
    let fw = w + 2;
    let mut flags = vec![0u16; fw * (h + 2)];
    let mut mag = vec![0u32; w * h];
    let idx = |x: usize, y: usize| (y + 1) * fw + x + 1;
    let mut mq = Mq::new(&blk.data);

    let mut total = blk.passes;
    let mut bp = planes as i32 - 1;
    let mut pass_type = 2; // 0 = significance propagation, 1 = refinement, 2 = cleanup
    while total > 0 && bp >= 0 {
        let bit = 1u32 << bp;
        match pass_type {
            0 => {
                for y0 in (0..h).step_by(4) {
                    for x in 0..w {
                        for y in y0..(y0 + 4).min(h) {
                            let i = idx(x, y);
                            let f = flags[i];
                            if f & F_SIG == 0 && f & NEIGHBOURS != 0 {
                                if mq.decode(zc[(f & NEIGHBOURS) as usize] as usize) == 1 {
                                    let neg = decode_sign(&mut mq, t, f);
                                    set_significant(&mut flags, i, fw, neg);
                                    mag[y * w + x] |= bit;
                                }
                                flags[i] |= F_VISITED;
                            }
                        }
                    }
                }
            }
            1 => {
                for y0 in (0..h).step_by(4) {
                    for x in 0..w {
                        for y in y0..(y0 + 4).min(h) {
                            let i = idx(x, y);
                            let f = flags[i];
                            if f & F_SIG != 0 && f & F_VISITED == 0 {
                                let ctx = if f & F_REFINED != 0 { 16 } else if f & NEIGHBOURS != 0 { 15 } else { 14 };
                                if mq.decode(ctx) == 1 {
                                    mag[y * w + x] |= bit;
                                }
                                flags[i] |= F_REFINED;
                            }
                        }
                    }
                }
            }
            _ => {
                for y0 in (0..h).step_by(4) {
                    for x in 0..w {
                        let mut y = y0;
                        if y0 + 4 <= h && (0..4).all(|k| flags[idx(x, y0 + k)] & (NEIGHBOURS | F_SIG | F_VISITED) == 0) {
                            if mq.decode(CTX_RUN) == 0 {
                                continue;
                            }
                            let first = ((mq.decode(CTX_UNI) << 1) | mq.decode(CTX_UNI)) as usize;
                            y = y0 + first;
                            let i = idx(x, y);
                            let neg = decode_sign(&mut mq, t, flags[i]);
                            set_significant(&mut flags, i, fw, neg);
                            mag[y * w + x] |= bit;
                            y += 1;
                        }
                        while y < (y0 + 4).min(h) {
                            let i = idx(x, y);
                            let f = flags[i];
                            if f & (F_SIG | F_VISITED) == 0 && mq.decode(zc[(f & NEIGHBOURS) as usize] as usize) == 1 {
                                let neg = decode_sign(&mut mq, t, f);
                                set_significant(&mut flags, i, fw, neg);
                                mag[y * w + x] |= bit;
                            }
                            y += 1;
                        }
                    }
                }
                for f in flags.iter_mut() {
                    *f &= !F_VISITED;
                }
            }
        }
        total -= 1;
        if pass_type == 2 {
            pass_type = 0;
            bp -= 1;
        } else {
            pass_type += 1;
        }
    }
    for y in 0..h {
        for x in 0..w {
            let m = mag[y * w + x] as i32;
            out[y * w + x] = if flags[idx(x, y)] & F_NEG != 0 { -m } else { m };
        }
    }
    Ok(out)
}

// ----- tile -----------------------------------------------------------------------------------------

fn inverse_53_1d(x: &mut [i32]) {
    let n = x.len();
    if n < 2 {
        return;
    }
    let at = |x: &[i32], i: isize| -> i32 {
        let n = x.len() as isize;
        let j = if i < 0 { -i } else if i >= n { 2 * (n - 1) - i } else { i };
        x[j as usize]
    };
    for i in (0..n).step_by(2) {
        let d = (at(x, i as isize - 1) + at(x, i as isize + 1) + 2) >> 2;
        x[i] -= d;
    }
    for i in (1..n).step_by(2) {
        let d = (at(x, i as isize - 1) + at(x, i as isize + 1)) >> 1;
        x[i] += d;
    }
}

fn inverse_dwt(plane: &mut [i32], stride: usize, w: usize, h: usize) {
    let mut row = vec![0i32; w.max(h)];
    for y in 0..h {
        let line = &mut plane[y * stride..y * stride + w];
        let lw = w.div_ceil(2);
        for k in 0..lw {
            row[2 * k] = line[k];
        }
        for k in 0..w - lw {
            row[2 * k + 1] = line[lw + k];
        }
        inverse_53_1d(&mut row[..w]);
        line.copy_from_slice(&row[..w]);
    }
    let lh = h.div_ceil(2);
    for x in 0..w {
        for k in 0..lh {
            row[2 * k] = plane[k * stride + x];
        }
        for k in 0..h - lh {
            row[2 * k + 1] = plane[(lh + k) * stride + x];
        }
        inverse_53_1d(&mut row[..h]);
        for y in 0..h {
            plane[y * stride + x] = row[y];
        }
    }
}

fn decode_tile(prm: &Params, data: &[u8], reduce: usize) -> Result<Decoded> {
    let (full_w, full_h) = (prm.width, prm.height);
    let reduce = reduce.min(prm.levels);
    let top = prm.levels - reduce; // finest resolution index that is decoded
    let (w, h) = (full_w, full_h);
    let ncomp = prm.components;
    let nres = prm.levels + 1;
    let needed = 1 + 3 * prm.levels;
    if prm.exponents.len() < needed {
        return corrupt("quantisation exponents");
    }
    // bands[comp][res]
    let mut bands: Vec<Vec<Vec<Band>>> = (0..ncomp).map(|_| (0..nres).map(|r| build_bands(prm, r)).collect()).collect();

    // Packet sequence (one precinct per resolution).
    let mut order: Vec<(usize, usize, usize)> = Vec::new(); // (layer, res, comp)
    match prm.progression {
        0 => (0..prm.layers).for_each(|l| (0..nres).for_each(|r| (0..ncomp).for_each(|c| order.push((l, r, c))))),
        1 => (0..nres).for_each(|r| (0..prm.layers).for_each(|l| (0..ncomp).for_each(|c| order.push((l, r, c))))),
        2 => (0..nres).for_each(|r| (0..ncomp).for_each(|c| (0..prm.layers).for_each(|l| order.push((l, r, c))))),
        3 => (0..ncomp).for_each(|c| (0..nres).for_each(|r| (0..prm.layers).for_each(|l| order.push((l, r, c))))),
        4 => (0..ncomp).for_each(|c| (0..nres).for_each(|r| (0..prm.layers).for_each(|l| order.push((l, r, c))))),
        _ => return corrupt("progression order"),
    }
    let mut pos = 0;
    for (l, r, c) in order {
        if pos >= data.len() {
            break;
        }
        read_packet(prm, data, &mut pos, &mut bands[c][r], l)?;
    }

    let mut planes: Vec<Vec<i32>> = Vec::with_capacity(ncomp);
    for comp_bands in &bands {
        let mut plane = vec![0i32; w * h];
        for band in comp_bands[..=top].iter().flatten() {
            for blk in &band.blocks {
                let vals = decode_block(blk, band.orient, band.mb)?;
                for y in 0..blk.h {
                    let dst = (band.py + blk.y0 + y) * w + band.px + blk.x0;
                    plane[dst..dst + blk.w].copy_from_slice(&vals[y * blk.w..(y + 1) * blk.w]);
                }
            }
        }
        for res in 1..=top {
            let k = prm.levels - res;
            inverse_dwt(&mut plane, w, w.div_ceil(1 << k), h.div_ceil(1 << k));
        }
        planes.push(plane);
    }
    // The decoded image is the top-left corner of each plane.
    let (w, h) = (full_w.div_ceil(1 << reduce), full_h.div_ceil(1 << reduce));
    let stride = full_w;

    if prm.mct && ncomp >= 3 {
        for y in 0..h {
            for i in y * stride..y * stride + w {
                let (y0, y1, y2) = (planes[0][i], planes[1][i], planes[2][i]);
                let g = y0 - ((y1 + y2) >> 2);
                planes[0][i] = y2 + g;
                planes[1][i] = g;
                planes[2][i] = y1 + g;
            }
        }
    }
    let shift = 1i32 << (prm.precision - 1);
    let max = (1i32 << prm.precision) - 1;
    let mut samples = vec![0u16; w * h * ncomp];
    for (c, plane) in planes.iter().enumerate() {
        for y in 0..h {
            for x in 0..w {
                samples[(y * w + x) * ncomp + c] = (plane[y * stride + x] + shift).clamp(0, max) as u16;
            }
        }
    }
    Ok(Decoded { width: w, height: h, components: ncomp, precision: prm.precision, reduced: reduce, samples })
}
