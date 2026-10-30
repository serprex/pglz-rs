//! Compression. Ports `pglz_compress` from `src/common/pg_lzcompress.c`.

use crate::strategy::Strategy;

const MAX_HISTORY_LISTS: usize = 8192;
const HISTORY_SIZE: usize = 4096;
const MAX_MATCH: usize = 273;

/// Hash chains of recent input positions used to find back-references.
/// `prev` is a ring keyed by position; recycled slots are always older than
/// the max back-reference offset, so chain walks stop before reading them.
/// Positions are stored biased by `HISTORY_SIZE` so 0 means out of window.
struct History {
    head: Vec<u32>,
    prev: Box<[u32; HISTORY_SIZE]>,
    mask: usize,
}

impl History {
    fn new(slen: usize) -> Self {
        let hashsz = match slen {
            n if n < 128 => 512,
            n if n < 256 => 1024,
            n if n < 512 => 2048,
            n if n < 1024 => 4096,
            _ => MAX_HISTORY_LISTS,
        };
        History {
            head: vec![0; hashsz],
            prev: Box::new([0; HISTORY_SIZE]),
            mask: hashsz - 1,
        }
    }

    #[inline]
    fn hash(&self, source: &[u8], i: usize) -> usize {
        let h = match source[i..] {
            [a, b, c, d, ..] => {
                ((a as usize) << 6) ^ ((b as usize) << 4) ^ ((c as usize) << 2) ^ d as usize
            }
            [a, ..] => a as usize,
            [] => unreachable!(),
        };
        h & self.mask
    }

    #[inline]
    fn add(&mut self, source: &[u8], pos: usize) {
        let h = self.hash(source, pos);
        self.prev[pos % HISTORY_SIZE] = self.head[h];
        self.head[h] = (pos + HISTORY_SIZE) as u32;
    }

    fn find_match(
        &self,
        source: &[u8],
        input_idx: usize,
        mut good_match: usize,
        good_drop: usize,
    ) -> Option<(usize, usize)> {
        let mut best_len = 0usize;
        let mut best_off = 0usize;
        let input = &source[input_idx..];
        let input = &input[..input.len().min(MAX_MATCH)];

        let mut biased = self.head[self.hash(source, input_idx)] as usize;
        loop {
            let off = input_idx + HISTORY_SIZE - biased;
            if off >= 0x0fff {
                break;
            }
            let hp = input_idx - off;

            // Skip candidates that can't beat a long best without a full scan
            if best_len < 16 || input[..best_len] == source[hp..hp + best_len] {
                let len = match_len(input, &source[hp..]);
                if len > best_len {
                    best_len = len;
                    best_off = off;
                }
            }

            if best_len >= good_match {
                break;
            }
            good_match -= (good_match * good_drop) / 100;
            biased = self.prev[hp % HISTORY_SIZE] as usize;
        }

        (best_len > 2).then_some((best_len, best_off))
    }
}

/// Length of common prefix of `a` and `b`, capped at `a.len()`
#[inline]
fn match_len(a: &[u8], b: &[u8]) -> usize {
    let (a8, _) = a.as_chunks::<8>();
    for (i, (x, y)) in a8.iter().zip(b.as_chunks::<8>().0).enumerate() {
        let diff = u64::from_le_bytes(*x) ^ u64::from_le_bytes(*y);
        if diff != 0 {
            return i * 8 + (diff.trailing_zeros() / 8) as usize;
        }
    }
    let done = a8.len() * 8;
    done + a[done..]
        .iter()
        .zip(&b[done..])
        .take_while(|(x, y)| x == y)
        .count()
}

/// Output cursor maintaining the rolling 8-bit control byte described in the
/// pglz format spec.
struct Output<'a> {
    dest: &'a mut [u8],
    bp: usize,
    ctrlp: usize,
    ctrl: u8,
}

impl<'a> Output<'a> {
    fn new(dest: &'a mut [u8]) -> Self {
        Output { dest, bp: 0, ctrlp: 0, ctrl: 0 }
    }

    #[inline]
    fn step_ctrl(&mut self, is_tag: bool) {
        if self.ctrl == 0 {
            self.ctrlp = self.bp;
            self.dest[self.bp] = 0;
            self.bp += 1;
            self.ctrl = 1;
        }
        if is_tag {
            self.dest[self.ctrlp] |= self.ctrl;
        }
        self.ctrl <<= 1;
    }

    fn emit_literal(&mut self, byte: u8) {
        self.step_ctrl(false);
        self.dest[self.bp] = byte;
        self.bp += 1;
    }

    fn emit_tag(&mut self, len: usize, off: usize) {
        self.step_ctrl(true);
        let hi = ((off & 0xf00) >> 4) as u8;
        let lo = off as u8;
        if len > 17 {
            self.dest[self.bp..self.bp + 3].copy_from_slice(&[hi | 0x0f, lo, (len - 18) as u8]);
            self.bp += 3;
        } else {
            self.dest[self.bp..self.bp + 2].copy_from_slice(&[hi | (len - 3) as u8, lo]);
            self.bp += 2;
        }
    }
}

/// Buffer size required by `compress_into`: input length + 4 slop bytes.
/// Mirrors `PGLZ_MAX_OUTPUT(dlen)`.
pub const fn max_compressed_buffer_size(input_len: usize) -> usize {
    input_len + 4
}

/// Compress `source` using `strategy`. Returns `None` if compression fails
/// (strategy disallows it, ratio target not met, etc).
pub fn compress(source: &[u8], strategy: &Strategy) -> Option<Vec<u8>> {
    let mut dest = vec![0u8; max_compressed_buffer_size(source.len())];
    let n = compress_into(source, &mut dest, strategy)?;
    dest.truncate(n);
    Some(dest)
}

/// Compress `source` into `dest`, returning bytes written.
/// `dest` must hold at least `max_compressed_buffer_size(source.len())` bytes.
pub fn compress_into(source: &[u8], dest: &mut [u8], strategy: &Strategy) -> Option<usize> {
    let slen = source.len();
    let slen_i = slen as i64;

    if strategy.match_size_good <= 0
        || slen_i < strategy.min_input_size as i64
        || slen_i > strategy.max_input_size as i64
    {
        return None;
    }

    let good_match = (strategy.match_size_good as usize).clamp(17, MAX_MATCH);
    let good_drop = strategy.match_size_drop.clamp(0, 100) as usize;
    let need_rate = strategy.min_comp_rate.clamp(0, 99) as usize;
    let first_success_by = strategy.first_success_by.max(0) as usize;

    // Match the C reference's overflow guard for large slen.
    let result_max = if slen > i32::MAX as usize / 100 {
        (slen / 100) * (100 - need_rate)
    } else {
        (slen * (100 - need_rate)) / 100
    };

    let mut history = History::new(slen);
    let mut out = Output::new(dest);
    let mut dp = 0usize;
    let mut found_match = false;

    while dp < slen {
        if out.bp >= result_max {
            return None;
        }
        if !found_match && out.bp >= first_success_by {
            return None;
        }

        if let Some((len, off)) = history.find_match(source, dp, good_match, good_drop) {
            out.emit_tag(len, off);
            for pos in dp..dp + len {
                history.add(source, pos);
            }
            dp += len;
            found_match = true;
        } else {
            out.emit_literal(source[dp]);
            history.add(source, dp);
            dp += 1;
        }
    }

    (out.bp < result_max).then_some(out.bp)
}
