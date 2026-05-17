//! Compression. Ports `pglz_compress` from `src/common/pg_lzcompress.c`.

use crate::strategy::Strategy;

const MAX_HISTORY_LISTS: usize = 8192;
const HISTORY_SIZE: usize = 4096;
const MAX_MATCH: usize = 273;

#[derive(Clone, Copy, Default)]
struct HistEntry {
    next: i16,
    prev: i16,
    hindex: u16,
    pos: u32,
}

/// Hash table of recent input positions used to find back-references.
/// Entry 0 is reserved as the invalid sentinel and is freely scribbled on
/// (matches the C reference's "branch-free" linked-list write trick).
struct History {
    start: Vec<i16>,
    entries: Vec<HistEntry>,
    next: i16,
    recycle: bool,
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
            start: vec![0; hashsz],
            entries: vec![HistEntry::default(); HISTORY_SIZE + 1],
            next: 1,
            recycle: false,
            mask: hashsz - 1,
        }
    }

    #[inline]
    fn hash(&self, source: &[u8], i: usize) -> usize {
        let h = if source.len() - i < 4 {
            source[i] as usize
        } else {
            ((source[i] as usize) << 6)
                ^ ((source[i + 1] as usize) << 4)
                ^ ((source[i + 2] as usize) << 2)
                ^ (source[i + 3] as usize)
        };
        h & self.mask
    }

    fn add(&mut self, source: &[u8], pos: usize) {
        let hn = self.next;
        let h = self.hash(source, pos);

        if self.recycle {
            let HistEntry { prev, next, hindex, .. } = self.entries[hn as usize];
            if prev == 0 {
                self.start[hindex as usize] = next;
            } else {
                self.entries[prev as usize].next = next;
            }
            self.entries[next as usize].prev = prev;
        }

        let head = self.start[h];
        let he = &mut self.entries[hn as usize];
        he.next = head;
        he.prev = 0;
        he.hindex = h as u16;
        he.pos = pos as u32;
        self.entries[head as usize].prev = hn;
        self.start[h] = hn;

        self.next += 1;
        if self.next > HISTORY_SIZE as i16 {
            self.next = 1;
            self.recycle = true;
        }
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
        let slen = source.len();

        let mut hentno = self.start[self.hash(source, input_idx)];
        while hentno != 0 {
            let hent = &self.entries[hentno as usize];
            let hp = hent.pos as usize;
            let off = input_idx - hp;
            if off >= 0x0fff {
                break;
            }

            // Fast-fail when we already have a long best: skip this entry
            // unless its first `best_len` bytes match en bloc.
            let skip = best_len >= 16
                && source[input_idx..input_idx + best_len] != source[hp..hp + best_len];
            if !skip {
                let max_compare = (slen - input_idx).min(MAX_MATCH);
                let thislen = source[input_idx..input_idx + max_compare]
                    .iter()
                    .zip(&source[hp..])
                    .position(|(a, b)| a != b)
                    .unwrap_or(max_compare);
                if thislen > best_len {
                    best_len = thislen;
                    best_off = off;
                }
            }

            hentno = hent.next;
            if hentno != 0 {
                if best_len >= good_match {
                    break;
                }
                good_match -= (good_match * good_drop) / 100;
            }
        }

        (best_len > 2).then_some((best_len, best_off))
    }
}

/// Output cursor maintaining the rolling 8-bit control byte described in the
/// pglz format spec.
struct Output<'a> {
    dest: &'a mut [u8],
    bp: usize,
    ctrlp: Option<usize>,
    ctrlb: u8,
    ctrl: u32,
}

impl<'a> Output<'a> {
    fn new(dest: &'a mut [u8]) -> Self {
        Output { dest, bp: 0, ctrlp: None, ctrlb: 0, ctrl: 0 }
    }

    #[inline]
    fn step_ctrl(&mut self) {
        if self.ctrl & 0xff == 0 {
            if let Some(idx) = self.ctrlp {
                self.dest[idx] = self.ctrlb;
            }
            self.ctrlp = Some(self.bp);
            self.bp += 1;
            self.ctrlb = 0;
            self.ctrl = 1;
        }
    }

    fn emit_literal(&mut self, byte: u8) {
        self.step_ctrl();
        self.dest[self.bp] = byte;
        self.bp += 1;
        self.ctrl <<= 1;
    }

    fn emit_tag(&mut self, len: usize, off: usize) {
        self.step_ctrl();
        self.ctrlb |= self.ctrl as u8;
        self.ctrl <<= 1;
        if len > 17 {
            self.dest[self.bp] = (((off & 0xf00) >> 4) | 0x0f) as u8;
            self.dest[self.bp + 1] = (off & 0xff) as u8;
            self.dest[self.bp + 2] = (len - 18) as u8;
            self.bp += 3;
        } else {
            self.dest[self.bp] = (((off & 0xf00) >> 4) | (len - 3)) as u8;
            self.dest[self.bp + 1] = (off & 0xff) as u8;
            self.bp += 2;
        }
    }

    fn finish(self) -> usize {
        if let Some(idx) = self.ctrlp {
            self.dest[idx] = self.ctrlb;
        }
        self.bp
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
            for _ in 0..len {
                history.add(source, dp);
                dp += 1;
            }
            found_match = true;
        } else {
            out.emit_literal(source[dp]);
            history.add(source, dp);
            dp += 1;
        }
    }

    let n = out.finish();
    if n >= result_max {
        return None;
    }
    Some(n)
}
