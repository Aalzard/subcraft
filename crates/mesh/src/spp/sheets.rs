//! Stencil sheets inside compiled Substance graphs.
//!
//! A stencil graph (the `.sbsar` of a stencil pack) keeps its alpha sheets as raw 8-bit pictures
//! at the start of the compiled assembly (`.sbsasm`): after a 52-byte header the sheets follow each
//! other without padding, top row first; the graph description starts at the position the footer
//! names. The assembly does not state each sheet's size, so the layout is recovered: the sheet
//! count comes from the graph's `.xml` (the largest value of its picker input) and the sizes from
//! the length of the data plus how well each candidate size lines up row to row (a picture repeats
//! along its row stride, not along a wrong one). Layouts that do not add up exactly are refused,
//! not guessed.

/// One sheet: 8-bit grayscale, top row first.
#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

const DATA_START: usize = 52;
const WIDTHS: [u32; 7] = [1024, 2048, 3072, 4096, 5120, 6144, 8192];

/// The number of picker values of a stencil graph: the largest `max` of its slider named like a
/// sheet picker.
pub fn sheet_count(xml: &str) -> Option<usize> {
    let mut best = None;
    for part in xml.split("<input ").skip(1) {
        let head = part.split('>').next().unwrap_or("");
        let Some(id) = head.split("identifier=\"").nth(1).and_then(|s| s.split('"').next()) else { continue };
        let id = id.to_ascii_lowercase();
        if !(id.contains("stencil") || id.contains("alpha") || id.contains("switch") || id.contains("choose")) {
            continue;
        }
        let Some(slider) = part.split("<guislider").nth(1) else { continue };
        let max = slider.split("max=\"").nth(1).and_then(|m| m.split('"').next()).and_then(|m| m.parse::<f32>().ok());
        if let Some(m) = max {
            best = best.max(Some(m as usize));
        }
    }
    best.filter(|n| (1..=64).contains(n))
}

/// Raw sheet data of an assembly: its bytes from the data start up to the footer's data end.
fn sheet_bytes(asm: &[u8]) -> Option<&[u8]> {
    if asm.len() < DATA_START + 16 || &asm[..4] != b"SBAM" {
        return None;
    }
    let at = asm.len() - 12;
    let end = (u32::from_le_bytes(asm[at..at + 4].try_into().unwrap()) as usize).checked_add(48)?;
    (end > DATA_START && end <= asm.len()).then(|| &asm[DATA_START..end])
}

/// Mean absolute difference between texels one row stride apart, over a window at `from`.
fn stride_error(data: &[u8], from: usize, stride: usize) -> f64 {
    const WINDOW: usize = 1 << 17;
    if from + stride + WINDOW > data.len() {
        return f64::MAX;
    }
    let a = &data[from..from + WINDOW];
    let b = &data[from + stride..from + stride + WINDOW];
    a.iter().zip(b).map(|(x, y)| (*x as i32 - *y as i32).unsigned_abs() as f64).sum::<f64>() / WINDOW as f64
}

/// The row stride of the picture that starts at `at`: the smallest width whose rows line up (a
/// width twice the true one lines up too, two rows apart, a smaller one does not: it is a long way
/// along the row). `None` while the windows tried hold nothing but blank texels.
fn stride_of(data: &[u8], at: usize) -> Option<u32> {
    for offset in [1usize << 18, 1 << 21, 6 << 20, 12 << 20, 24 << 20] {
        let errors: Vec<(f64, u32)> = WIDTHS.iter().map(|w| (stride_error(data, at + offset, *w as usize), *w)).filter(|e| e.0 < f64::MAX).collect();
        let floor = errors.iter().map(|e| e.0).fold(f64::MAX, f64::min);
        if floor < f64::MAX && floor > 0.05 {
            return errors.iter().find(|e| e.0 <= floor * 1.3).map(|e| e.1);
        }
    }
    None
}

fn candidates() -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for w in WIDTHS {
        for h in [w, w / 2, w * 2 / 3, w * 3 / 4] {
            if h >= 512 && !out.contains(&(w, h)) {
                out.push((w, h));
            }
        }
    }
    out
}

type Memo = std::collections::HashMap<(usize, usize), Option<(f64, Vec<(u32, u32)>)>>;

/// The best way to lay `left` sheets over `data[at..]` exactly: lowest total stride error.
fn solve(data: &[u8], at: usize, left: usize, cands: &[(u32, u32)], memo: &mut Memo) -> Option<(f64, Vec<(u32, u32)>)> {
    if left == 0 {
        return (at == data.len()).then(|| (0.0, Vec::new()));
    }
    if let Some(found) = memo.get(&(at, left)) {
        return found.clone();
    }
    // Each remaining sheet has at least 512x512 texels and at most 8192x8192.
    let rest = data.len() - at;
    let mut best: Option<(f64, Vec<(u32, u32)>)> = None;
    if rest >= left * 512 * 512 && rest <= left * 8192 * 8192 {
        let stride = stride_of(data, at);
        for &(w, h) in cands {
            let size = w as usize * h as usize;
            if at + size > data.len() || stride.is_some_and(|s| s != w) {
                continue;
            }
            // Packs hold square sheets unless the data says otherwise: a stride cannot tell a boundary
            // from a row inside the picture, so odd shapes are charged.
            let e = if w == h { 0.0 } else { 1000.0 };
            if let Some((score, mut dims)) = solve(data, at + size, left - 1, cands, memo) {
                if best.as_ref().is_none_or(|(s, _)| score + e < *s) {
                    dims.insert(0, (w, h));
                    best = Some((score + e, dims));
                }
            }
        }
    }
    memo.insert((at, left), best.clone());
    best
}

/// The sheets of an assembly with `count` sheets (see [`sheet_count`]); `None` when the layout
/// cannot be recovered exactly.
pub fn read_sheets(asm: &[u8], count: usize) -> Option<Vec<Sheet>> {
    let data = sheet_bytes(asm)?;
    let (_, dims) = solve(data, 0, count, &candidates(), &mut Memo::new())?;
    let mut at = 0;
    let mut out = Vec::new();
    for (w, h) in dims {
        let size = w as usize * h as usize;
        out.push(Sheet { width: w, height: h, data: data[at..at + size].to_vec() });
        at += size;
    }
    Some(out)
}

/// `sheet` reduced by whole factors until neither side exceeds `max`.
pub fn reduced(sheet: &Sheet, max: u32) -> Sheet {
    let mut s = sheet.clone();
    while s.width.max(s.height) > max && s.width % 2 == 0 && s.height % 2 == 0 {
        let (w, h) = (s.width as usize / 2, s.height as usize / 2);
        let stride = s.width as usize;
        let mut data = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                let i = 2 * y * stride + 2 * x;
                let sum = s.data[i] as u32 + s.data[i + 1] as u32 + s.data[i + stride] as u32 + s.data[i + stride + 1] as u32;
                data[y * w + x] = ((sum + 2) / 4) as u8;
            }
        }
        s = Sheet { width: w as u32, height: h as u32, data };
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An assembly of the given sheet sizes: header, sheets, a description, the footer.
    fn assembly(sheets: &[(u32, u32)]) -> Vec<u8> {
        let mut asm = b"SBAM".to_vec();
        asm.resize(DATA_START, 0);
        for (k, (w, h)) in sheets.iter().enumerate() {
            // Vertical bars of a width per sheet, shifting a little every 4 rows.
            for y in 0..*h {
                for x in 0..*w {
                    let v: u8 = if (x / (40 + 7 * k as u32)) % 2 == 0 { 200 } else { 20 };
                    asm.push(v.wrapping_add(((y / 4) * 3) as u8));
                }
            }
        }
        let end = asm.len();
        asm.extend_from_slice(&[7u8; 64]);
        let total = asm.len() + 16;
        asm.extend_from_slice(&[0, 0, 0, 0]);
        asm.extend_from_slice(&((end - 48) as u32).to_le_bytes());
        asm.extend_from_slice(&0u32.to_le_bytes());
        asm.extend_from_slice(&(total as u32).to_le_bytes());
        asm
    }

    #[test]
    fn picker_range_is_read_from_the_description() {
        let xml = r#"<input uid="1" identifier="$randomseed" type="4" default="0"/><input uid="2" identifier="switch_stencil" type="4" default="1" alteroutputs=""><inputgui widget="slider"><guislider  min="1" max="10" step="1"/></inputgui></input><input uid="3" identifier="Blur" type="0"><inputgui><guislider  min="0" max="20"/></inputgui></input>"#;
        assert_eq!(sheet_count(xml), Some(10));
    }

    #[test]
    fn mixed_sheet_sizes_are_recovered_from_the_row_stride() {
        let sizes = [(1024, 1024), (2048, 2048), (1024, 1024)];
        let asm = assembly(&sizes);
        let sheets = read_sheets(&asm, 3).expect("layout");
        let found: Vec<(u32, u32)> = sheets.iter().map(|s| (s.width, s.height)).collect();
        assert_eq!(found, sizes.to_vec());
        assert_eq!(sheets[1].data[0], 200);
        assert_eq!(sheets[1].data[47], 20);
    }

    #[test]
    fn a_layout_that_does_not_add_up_is_refused() {
        let asm = assembly(&[(1024, 1024), (1024, 1024)]);
        assert!(read_sheets(&asm, 5).is_none());
        assert!(read_sheets(b"nonsense", 2).is_none());
    }

    #[test]
    fn sheets_reduce_by_whole_factors() {
        let s = Sheet { width: 4, height: 4, data: (0..16).map(|v| v as u8 * 10).collect() };
        let r = reduced(&s, 2);
        assert_eq!((r.width, r.height), (2, 2));
        assert_eq!(r.data[0], ((0 + 10 + 40 + 50 + 2) / 4) as u8);
    }
}
