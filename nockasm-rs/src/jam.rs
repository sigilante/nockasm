//! `jam` and `cue`: Urbit noun serialization.
//!
//! The canonical bit-level encoding, LSB-first: atom = `0` + mat,
//! cell = `10` + head + tail, backref = `11` + mat of the original tag
//! position. A `.jam` file is the jammed atom's bytes, little-endian.
//!
//! Backreference deduplication is by *structural* equality (two equal but
//! distinct subtrees share one encoding), exactly like the reference
//! implementations — [`Noun`]'s cached structural hashes make the memo
//! table O(1) per node. Both directions run iteratively, so arbitrarily
//! deep nouns cannot overflow the stack.

use std::collections::HashMap;

use crate::error::CueError;
use crate::noun::{Atom, Noun, NounRef};

// ----------------------------------------------------------------------
// Bit-stream writer
// ----------------------------------------------------------------------

struct BitWriter {
    buf: Vec<u8>,
    len: u64,
}

impl BitWriter {
    fn new() -> BitWriter {
        BitWriter {
            buf: Vec::new(),
            len: 0,
        }
    }

    /// Append `width` bits (LSB-first). `width <= 64`; higher bits of
    /// `bits` must be clear.
    fn emit(&mut self, width: u32, bits: u64) {
        debug_assert!(width == 64 || bits >> width == 0);
        let mut w = width;
        let mut b = bits;
        while w > 0 {
            let byte_i = (self.len / 8) as usize;
            let bit_i = (self.len % 8) as u32;
            if byte_i == self.buf.len() {
                self.buf.push(0);
            }
            let take = (8 - bit_i).min(w);
            self.buf[byte_i] |= ((b & ((1u64 << take) - 1)) as u8) << bit_i;
            b >>= take;
            w -= take;
            self.len += u64::from(take);
        }
    }

    /// Append all `bit_len` significant bits of `a`.
    fn emit_atom_bits(&mut self, a: &Atom) {
        let bits = a.bit_len();
        let bytes = a.le_bytes();
        let full = (bits / 8) as usize;
        for &byte in &bytes[..full] {
            self.emit(8, u64::from(byte));
        }
        let rem = (bits % 8) as u32;
        if rem > 0 {
            self.emit(rem, u64::from(bytes[full]) & ((1u64 << rem) - 1));
        }
    }

    /// The length-prefixed atom encoding: `bit_len` in unary-prefixed
    /// binary (leading 1 implicit), then the value's bits.
    fn mat(&mut self, a: &Atom) {
        if a.is_zero() {
            self.emit(1, 1);
            return;
        }
        let b = a.bit_len();
        let c = 64 - b.leading_zeros(); // bit length of b; 1..=64
        self.emit(c, 0);
        self.emit(1, 1);
        self.emit(c - 1, b & ((1u64 << (c - 1)) - 1));
        self.emit_atom_bits(a);
    }
}

/// Serialize a noun to jamfile bytes (the jammed atom, little-endian, no
/// trailing zero bytes).
///
/// ```
/// use nockasm::{jam, noun};
/// assert_eq!(jam(&noun![0]), vec![0x02]);
/// assert_eq!(jam(&noun![4 0 1]), vec![0x61, 0x26, 0x03]);
/// ```
pub fn jam(n: &Noun) -> Vec<u8> {
    let mut w = BitWriter::new();
    let mut memo: HashMap<Noun, u64> = HashMap::new();
    let mut stack: Vec<&Noun> = vec![n];
    while let Some(cur) = stack.pop() {
        if let Some(&ref_pos) = memo.get(cur) {
            let ref_bits = u64::from(64 - ref_pos.leading_zeros());
            match cur.view() {
                // A small atom re-encodes when that is no wider than the
                // backreference would be.
                NounRef::Atom(a) if a.bit_len() <= ref_bits => {
                    w.emit(1, 0);
                    w.mat(a);
                }
                _ => {
                    w.emit(2, 0b11);
                    w.mat(&Atom::from(ref_pos));
                }
            }
            continue;
        }
        memo.insert(cur.clone(), w.len);
        match cur.view() {
            NounRef::Atom(a) => {
                w.emit(1, 0);
                w.mat(a);
            }
            NounRef::Cell(h, t) => {
                w.emit(2, 0b01);
                stack.push(t);
                stack.push(h);
            }
        }
    }
    let mut buf = w.buf;
    while buf.last() == Some(&0) {
        buf.pop();
    }
    buf
}

// ----------------------------------------------------------------------
// Bit-stream reader
// ----------------------------------------------------------------------

struct BitReader<'a> {
    data: &'a [u8],
}

impl BitReader<'_> {
    fn total_bits(&self) -> u64 {
        self.data.len() as u64 * 8
    }

    fn bit(&self, i: u64) -> Result<bool, CueError> {
        if i >= self.total_bits() {
            return Err(CueError::Truncated);
        }
        Ok((self.data[(i / 8) as usize] >> (i % 8)) & 1 == 1)
    }

    /// Read `len` bits at `pos` as an atom.
    fn read_atom(&self, pos: u64, len: u64) -> Result<Atom, CueError> {
        if pos + len > self.total_bits() {
            return Err(CueError::Truncated);
        }
        if len == 0 {
            return Ok(Atom::ZERO);
        }
        let nbytes = (len.div_ceil(8)) as usize;
        let start = (pos / 8) as usize;
        let off = (pos % 8) as u32;
        let mut out = vec![0u8; nbytes];
        for (k, slot) in out.iter_mut().enumerate() {
            let lo = self.data.get(start + k).copied().unwrap_or(0) >> off;
            let hi = if off == 0 {
                0
            } else {
                self.data.get(start + k + 1).copied().unwrap_or(0) << (8 - off)
            };
            *slot = lo | hi;
        }
        let rem = (len % 8) as u32;
        if rem != 0 {
            out[nbytes - 1] &= ((1u16 << rem) - 1) as u8;
        }
        Ok(Atom::from_le_bytes(&out))
    }

    /// The mat decoding at `pos`: `(value, next_pos)`.
    fn rub(&self, pos: u64) -> Result<(Atom, u64), CueError> {
        let mut z: u64 = 0;
        while !self.bit(pos + z)? {
            z += 1;
        }
        if z == 0 {
            return Ok((Atom::ZERO, pos + 1));
        }
        if z - 1 > 63 {
            return Err(CueError::TooWide);
        }
        let j = pos + z + 1;
        let lbits = self
            .read_atom(j, z - 1)?
            .as_u64()
            .expect("z - 1 <= 63 bits fits u64");
        let len = lbits | (1u64 << (z - 1));
        let j = j + (z - 1);
        let val = self.read_atom(j, len)?;
        Ok((val, j + len))
    }
}

enum Frame {
    /// Waiting for the head; `start` is the cell's tag position.
    Head { start: u64 },
    /// Waiting for the tail.
    Tail { start: u64, head: Noun },
}

/// Deserialize jamfile bytes back to a noun. Trailing zero bytes (e.g.
/// word-alignment padding) are ignored; a truncated or malformed stream
/// is an error rather than a hang.
///
/// Inverts [`jam`]: `cue(&jam(n)) == Ok(n)` for every noun `n`.
pub fn cue(data: &[u8]) -> Result<Noun, CueError> {
    let r = BitReader { data };
    let mut memo: HashMap<u64, Noun> = HashMap::new();
    let mut frames: Vec<Frame> = Vec::new();
    let mut pos: u64 = 0;
    loop {
        // Read one value starting at `pos` (descending through cell tags).
        let mut value: Noun = loop {
            let start = pos;
            if !r.bit(pos)? {
                let (v, j) = r.rub(pos + 1)?;
                pos = j;
                let n = Noun::from(v);
                memo.insert(start, n.clone());
                break n;
            }
            if !r.bit(pos + 1)? {
                frames.push(Frame::Head { start });
                pos += 2;
                continue;
            }
            let (ref_atom, j) = r.rub(pos + 2)?;
            pos = j;
            let ref_pos = ref_atom.as_u64().ok_or(CueError::BadBackref)?;
            break memo.get(&ref_pos).ok_or(CueError::BadBackref)?.clone();
        };
        // Deliver it upward through completed frames.
        loop {
            match frames.pop() {
                None => return Ok(value),
                Some(Frame::Head { start }) => {
                    frames.push(Frame::Tail { start, head: value });
                    break; // go read the tail
                }
                Some(Frame::Tail { start, head }) => {
                    let cell = Noun::cell(head, value);
                    memo.insert(start, cell.clone());
                    value = cell;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::noun;

    fn jam_int(n: &Noun) -> u128 {
        // Collapse the byte output to an integer for vector checks.
        let bytes = jam(n);
        assert!(bytes.len() <= 16);
        let mut buf = [0u8; 16];
        buf[..bytes.len()].copy_from_slice(&bytes);
        u128::from_le_bytes(buf)
    }

    #[test]
    fn jam_vectors() {
        // From tests/test_lift.py.
        assert_eq!(jam_int(&noun![0]), 2);
        assert_eq!(jam_int(&noun![1]), 12);
        assert_eq!(jam_int(&Noun::cell(1u64, 1u64)), 817);
    }

    #[test]
    fn cue_inverts_jam() {
        let big = Atom::from((1u128 << 100) + 12345);
        let cases: Vec<Noun> = vec![
            noun![0],
            noun![42],
            noun![1 2],
            noun![1 2 3 4 5],
            Noun::cell(Noun::cell(noun![1 2 3], noun![1 2 3]), noun![1 2 3]),
            Noun::from(big),
        ];
        for n in cases {
            assert_eq!(cue(&jam(&n)), Ok(n.clone()), "case {n}");
        }
    }

    #[test]
    fn cue_tolerates_padding_and_rejects_garbage() {
        let mut padded = jam(&noun![4 0 1]);
        padded.extend_from_slice(&[0, 0, 0, 0, 0]);
        assert_eq!(cue(&padded), Ok(noun![4 0 1]));
        assert_eq!(cue(&[]), Err(CueError::Truncated));
        assert_eq!(cue(&[0]), Err(CueError::Truncated));
        // A backref pointing at nothing: 11 then mat(5) — position 5 was
        // never a value start.
        assert!(cue(&[0b1111]).is_err());
    }

    #[test]
    fn deep_noun_round_trip() {
        let mut n = Noun::from(0u64);
        for i in 0..100_000u64 {
            n = Noun::cell(i % 3, n);
        }
        assert_eq!(cue(&jam(&n)), Ok(n.clone()));
    }
}
