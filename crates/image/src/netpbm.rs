//! Netpbm files written by the exporter, like Painter's File type menu offers: PBM (1-bit),
//! PGM (gray 8 / 16-bit), PPM (RGB 8 / 16-bit) and PFM (32-bit float, gray or RGB).
//!
//! All are binary ("raw") variants. None stores alpha: it is dropped. PBM and PGM store one
//! gray value; colour data is turned into gray with Rec. 709 luma weights. PBM writes a texel
//! black when its gray is below one half. PFM rows go bottom to top, little-endian.

/// Netpbm flavour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `P4`: one bit per texel, 1 = black.
    Pbm,
    /// `P5`: gray, 8 or 16 bits (big-endian).
    Pgm,
    /// `P6`: RGB, 8 or 16 bits (big-endian).
    Ppm,
    /// `PF` (RGB) or `Pf` (gray): 32-bit float.
    Pfm,
}

/// Gray value of texel `px` (`comps` interleaved values).
fn gray(px: &[f32]) -> f32 {
    match px.len() {
        1 | 2 => px[0],
        _ => 0.2126 * px[0] + 0.7152 * px[1] + 0.0722 * px[2],
    }
}

/// RGB of texel `px`: gray is repeated, alpha dropped.
fn rgb(px: &[f32]) -> [f32; 3] {
    match px.len() {
        1 | 2 => [px[0]; 3],
        _ => [px[0], px[1], px[2]],
    }
}

/// Encode `data` (row-major, `comps` interleaved, 1..=4) as a Netpbm file. `bits16` picks 16-bit
/// samples for PGM / PPM; `quantize8` turns 0..1 values into bytes (rounded or dithered).
pub fn encode(width: u32, height: u32, comps: usize, data: &[f32], kind: Kind, bits16: bool, quantize8: impl Fn(&[f32]) -> Vec<u8>) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    debug_assert_eq!(data.len(), w * h * comps);
    let texels = || data.chunks_exact(comps);
    let q16 = |v: f32| (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16;
    let mut out = Vec::new();
    match kind {
        Kind::Pbm => {
            out.extend_from_slice(format!("P4\n{w} {h}\n").as_bytes());
            let grays: Vec<f32> = texels().map(gray).collect();
            for row in grays.chunks_exact(w.max(1)).take(h) {
                for byte in row.chunks(8) {
                    let mut b = 0u8;
                    for (i, &g) in byte.iter().enumerate() {
                        if g < 0.5 {
                            b |= 0x80u8 >> i;
                        }
                    }
                    out.push(b);
                }
            }
        }
        Kind::Pgm | Kind::Ppm => {
            let values: Vec<f32> = if kind == Kind::Pgm { texels().map(gray).collect() } else { texels().flat_map(rgb).collect() };
            let magic = if kind == Kind::Pgm { "P5" } else { "P6" };
            out.extend_from_slice(format!("{magic}\n{w} {h}\n{}\n", if bits16 { 65535 } else { 255 }).as_bytes());
            if bits16 {
                out.extend(values.iter().flat_map(|&v| q16(v).to_be_bytes()));
            } else {
                out.extend(quantize8(&values));
            }
        }
        Kind::Pfm => {
            let one = comps <= 2;
            // A negative scale means little-endian samples.
            out.extend_from_slice(format!("{}\n{w} {h}\n-1.0\n", if one { "Pf" } else { "PF" }).as_bytes());
            let per = if one { 1 } else { 3 };
            let values: Vec<f32> = if one { texels().map(|p| p[0]).collect() } else { texels().flat_map(rgb).collect() };
            for row in values.chunks_exact((w * per).max(1)).take(h).rev() {
                out.extend(row.iter().flat_map(|v| v.to_le_bytes()));
            }
        }
    }
    out
}

/// A decoded Netpbm file: width, height, values per texel and the values (0..1 for integer
/// files, raw for PFM), top row first.
#[derive(Debug, PartialEq)]
pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pub comps: usize,
    pub data: Vec<f32>,
}

/// Decode a binary PBM, PGM, PPM or PFM file (the variants [`encode`] writes). `None` when the
/// bytes are not one of them or are cut short.
pub fn decode(bytes: &[u8]) -> Option<Decoded> {
    let mut pos = 0;
    // Header fields are separated by whitespace; `#` starts a comment up to the end of the line.
    let mut field = || -> Option<String> {
        loop {
            while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            if pos < bytes.len() && bytes[pos] == b'#' {
                while pos < bytes.len() && bytes[pos] != b'\n' {
                    pos += 1;
                }
                continue;
            }
            break;
        }
        let start = pos;
        while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        (pos > start).then(|| String::from_utf8_lossy(&bytes[start..pos]).into_owned())
    };
    let magic = field()?;
    let width: usize = field()?.parse().ok()?;
    let height: usize = field()?.parse().ok()?;
    let max = if magic == "P4" { None } else { Some(field()?) };
    // Exactly one whitespace byte separates the header from the samples.
    let body = bytes.get(pos + 1..)?;
    let n = width * height;
    let (comps, data) = match magic.as_str() {
        "P4" => {
            let stride = width.div_ceil(8);
            let rows = body.get(..stride * height)?;
            let data = (0..n).map(|i| {
                let (x, y) = (i % width, i / width);
                if rows[y * stride + x / 8] & (0x80u8 >> (x % 8)) != 0 { 0.0 } else { 1.0 }
            });
            (1, data.collect::<Vec<f32>>())
        }
        "P5" | "P6" => {
            let comps = if magic == "P5" { 1 } else { 3 };
            let maxval: f32 = max?.parse().ok()?;
            let data: Vec<f32> = if maxval > 255.0 {
                body.get(..n * comps * 2)?.chunks_exact(2).map(|b| u16::from_be_bytes([b[0], b[1]]) as f32 / maxval).collect()
            } else {
                body.get(..n * comps)?.iter().map(|&b| b as f32 / maxval).collect()
            };
            (comps, data)
        }
        "PF" | "Pf" => {
            let comps = if magic == "PF" { 3 } else { 1 };
            let little = max?.parse::<f32>().ok()? < 0.0;
            let values: Vec<f32> = body
                .get(..n * comps * 4)?
                .chunks_exact(4)
                .map(|b| {
                    let b = [b[0], b[1], b[2], b[3]];
                    if little { f32::from_le_bytes(b) } else { f32::from_be_bytes(b) }
                })
                .collect();
            let row = (width * comps).max(1);
            (comps, values.chunks_exact(row).rev().flatten().copied().collect())
        }
        _ => return None,
    };
    Some(Decoded { width: width as u32, height: height as u32, comps, data })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round(data: &[f32]) -> Vec<u8> {
        data.iter().map(|&v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8).collect()
    }

    #[test]
    fn every_kind_round_trips() {
        // 3 x 2 RGB, values on the 8-bit grid.
        let rgb: Vec<f32> = (0..18).map(|i| (i * 13) as f32 / 255.0).collect();
        let ppm = decode(&encode(3, 2, 3, &rgb, Kind::Ppm, false, round)).unwrap();
        assert_eq!((ppm.width, ppm.height, ppm.comps), (3, 2, 3));
        assert!(ppm.data.iter().zip(&rgb).all(|(a, b)| (a - b).abs() < 1e-6), "{:?}", ppm.data);
        // 16-bit keeps values between the 8-bit steps.
        let fine = [0.123_45_f32, 0.5, 0.999];
        let pgm = decode(&encode(3, 1, 1, &fine, Kind::Pgm, true, round)).unwrap();
        assert!(pgm.data.iter().zip(fine).all(|(a, b)| (a - b).abs() < 1.0 / 65535.0), "{:?}", pgm.data);
        // PGM of colour is its luma; alpha is dropped.
        let rgba = [1.0, 0.0, 0.0, 0.25];
        let g = decode(&encode(1, 1, 4, &rgba, Kind::Pgm, true, round)).unwrap();
        assert_eq!(g.comps, 1);
        assert!((g.data[0] - 0.2126).abs() < 1e-4);
        // PFM keeps HDR and negative values exactly, with the rows the right way up.
        let hdr = [0.0, 2.5, -1.0, 100.0];
        let f = decode(&encode(2, 2, 1, &hdr, Kind::Pfm, false, round)).unwrap();
        assert_eq!((f.comps, f.data.as_slice()), (1, &hdr[..]));
        let frgb: Vec<f32> = (0..12).map(|i| i as f32 * 1.5).collect();
        let f = decode(&encode(2, 2, 3, &frgb, Kind::Pfm, false, round)).unwrap();
        assert_eq!((f.comps, f.data), (3, frgb));
    }

    #[test]
    fn pbm_thresholds_at_one_half_and_pads_rows() {
        // 10 wide: two bytes per row, the second padded.
        let gray: Vec<f32> = (0..20).map(|i| if i % 3 == 0 { 0.9 } else { 0.1 }).collect();
        let bytes = encode(10, 2, 1, &gray, Kind::Pbm, false, round);
        assert!(bytes.starts_with(b"P4\n10 2\n"));
        assert_eq!(bytes.len(), "P4\n10 2\n".len() + 2 * 2);
        let d = decode(&bytes).unwrap();
        let want: Vec<f32> = gray.iter().map(|&g| if g < 0.5 { 0.0 } else { 1.0 }).collect();
        assert_eq!(d.data, want);
        // The first texel is white (bit 0), the second black (bit 1).
        assert_eq!(bytes["P4\n10 2\n".len()] & 0xC0, 0x40);
    }

    #[test]
    fn headers_follow_the_netpbm_spec() {
        let one = [0.5_f32];
        let head = |kind, comps: usize, bits16| {
            let data = vec![0.5_f32; comps];
            let b = encode(1, 1, comps, &data, kind, bits16, round);
            let end = b.iter().rposition(|c| *c == b'\n').unwrap();
            String::from_utf8_lossy(&b[..=end.min(b.len() - 1)]).into_owned()
        };
        assert!(head(Kind::Pgm, 1, false).starts_with("P5\n1 1\n255\n"));
        assert!(head(Kind::Ppm, 3, true).starts_with("P6\n1 1\n65535\n"));
        assert!(encode(1, 1, 1, &one, Kind::Pfm, false, round).starts_with(b"Pf\n1 1\n-1.0\n"));
        assert!(encode(1, 1, 3, &[0.5; 3], Kind::Pfm, false, round).starts_with(b"PF\n1 1\n-1.0\n"));
        assert_eq!(decode(b"P7\n1 1\n"), None, "PAM is not read");
        assert_eq!(decode(b"P5\n2 2\n255\n\x00"), None, "cut short");
    }
}
