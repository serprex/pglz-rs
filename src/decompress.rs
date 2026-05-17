//! Decompression. Ports `pglz_decompress` from `src/common/pg_lzcompress.c`.

/// Decompress all of `source` into a fresh `Vec`, growing the output buffer
/// as bytes are produced. Returns `None` if the stream is corrupted
/// (truncated tag, invalid back-reference offset).
pub fn decompress(source: &[u8]) -> Option<Vec<u8>> {
    let mut dest: Vec<u8> = Vec::new();
    let mut sp = 0;

    while sp < source.len() {
        let mut ctrl = source[sp];
        sp += 1;

        for _ in 0..8 {
            if sp >= source.len() {
                break;
            }

            if ctrl & 1 != 0 {
                let (mut off, mut len) = read_tag(source, &mut sp)?;
                if off == 0 || off > dest.len() {
                    return None;
                }
                let mut dp = dest.len();
                dest.resize(dp + len, 0);
                while off < len {
                    let s = dp - off;
                    dest.copy_within(s..s + off, dp);
                    len -= off;
                    dp += off;
                    off += off;
                }
                let s = dp - off;
                dest.copy_within(s..s + len, dp);
            } else {
                dest.push(source[sp]);
                sp += 1;
            }
            ctrl >>= 1;
        }
    }
    Some(dest)
}

/// Decompress `source` into a caller-provided `dest`. Stops when either runs
/// out. Returns bytes written.
///
/// `check_complete = true` requires both buffers to be fully consumed —
/// useful when `dest.len()` is the exact expected uncompressed size.
/// `check_complete = false` allows partial decompression for slice/prefix
/// reads.
pub fn decompress_into(source: &[u8], dest: &mut [u8], check_complete: bool) -> Option<usize> {
    let mut sp = 0;
    let mut dp = 0;

    while sp < source.len() && dp < dest.len() {
        let mut ctrl = source[sp];
        sp += 1;

        for _ in 0..8 {
            if sp >= source.len() || dp >= dest.len() {
                break;
            }

            if ctrl & 1 != 0 {
                let (mut off, mut len) = read_tag(source, &mut sp)?;
                if off == 0 || off > dp {
                    return None;
                }
                len = len.min(dest.len() - dp);
                while off < len {
                    let s = dp - off;
                    dest.copy_within(s..s + off, dp);
                    len -= off;
                    dp += off;
                    off += off;
                }
                let s = dp - off;
                dest.copy_within(s..s + len, dp);
                dp += len;
            } else {
                dest[dp] = source[sp];
                dp += 1;
                sp += 1;
            }
            ctrl >>= 1;
        }
    }

    if check_complete && (dp != dest.len() || sp != source.len()) {
        return None;
    }
    Some(dp)
}

#[inline]
fn read_tag(source: &[u8], sp: &mut usize) -> Option<(usize, usize)> {
    if source.len() - *sp < 2 {
        return None;
    }
    let mut len = (source[*sp] & 0x0f) as usize + 3;
    let off = (((source[*sp] & 0xf0) as usize) << 4) | source[*sp + 1] as usize;
    *sp += 2;
    if len == 18 {
        len += *source.get(*sp)? as usize;
        *sp += 1;
    }
    Some((off, len))
}

/// Maximum compressed prefix size needed to decode `rawsize` raw bytes.
/// Mirrors `pglz_maximum_compressed_size`.
pub fn maximum_compressed_size(rawsize: i32, total_compressed_size: i32) -> i32 {
    let cs = ((rawsize as i64) * 9 + 7) / 8 + 2;
    cs.min(total_compressed_size as i64) as i32
}
