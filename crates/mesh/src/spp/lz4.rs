//! LZ4 block decompression (the raw block format, no frame), used by Painter's texture cache.

#[derive(Debug, thiserror::Error)]
#[error("corrupt LZ4 block")]
pub struct Lz4Error;

/// Decompress `src` into exactly `size` bytes.
pub fn decompress(src: &[u8], size: usize) -> Result<Vec<u8>, Lz4Error> {
    let mut out: Vec<u8> = Vec::with_capacity(size);
    let mut i = 0;
    while i < src.len() {
        let token = src[i];
        i += 1;
        let mut literals = (token >> 4) as usize;
        if literals == 15 {
            loop {
                let b = *src.get(i).ok_or(Lz4Error)?;
                i += 1;
                literals += b as usize;
                if b != 255 {
                    break;
                }
            }
        }
        out.extend_from_slice(src.get(i..i + literals).ok_or(Lz4Error)?);
        i += literals;
        if i >= src.len() {
            break;
        }
        let offset = *src.get(i).ok_or(Lz4Error)? as usize | (*src.get(i + 1).ok_or(Lz4Error)? as usize) << 8;
        i += 2;
        let mut len = (token & 15) as usize;
        if len == 15 {
            loop {
                let b = *src.get(i).ok_or(Lz4Error)?;
                i += 1;
                len += b as usize;
                if b != 255 {
                    break;
                }
            }
        }
        len += 4;
        if offset == 0 || offset > out.len() || out.len() + len > size {
            return Err(Lz4Error);
        }
        let start = out.len() - offset;
        for k in 0..len {
            let b = out[start + k];
            out.push(b);
        }
    }
    if out.len() != size {
        return Err(Lz4Error);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_and_overlapping_match() {
        // "abc" then a match of length 7 at offset 3 -> "abcabcabca" (10 bytes).
        let block = [0x33, b'a', b'b', b'c', 0x03, 0x00];
        assert_eq!(decompress(&block, 10).unwrap(), b"abcabcabca");
    }

    #[test]
    fn long_literal_run() {
        let mut block = vec![0xf0, 5];
        block.extend((0..20).map(|i| i as u8));
        assert_eq!(decompress(&block, 20).unwrap(), (0..20).map(|i| i as u8).collect::<Vec<_>>());
    }

    #[test]
    fn rejects_wrong_size_and_bad_offset() {
        let block = [0x33, b'a', b'b', b'c', 0x03, 0x00];
        assert!(decompress(&block, 11).is_err());
        let bad = [0x10, b'a', 0x05, 0x00];
        assert!(decompress(&bad, 5).is_err());
    }
}
