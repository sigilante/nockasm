//! Nouns: atoms (arbitrary-precision naturals) and cells (ordered pairs).
//!
//! The representation is tuned for the expander's working set — axes,
//! opcodes, and cords are almost always small — while staying correct for
//! arbitrary magnitudes:
//!
//! - [`Atom`] inlines values that fit in a `u64` and spills larger values
//!   to a shared little-endian byte buffer (no trailing zero bytes, so
//!   equality and hashing are canonical by construction).
//! - [`Noun`] cells are reference-counted and carry a structural hash
//!   computed at construction, making structural hashing O(1) per cell —
//!   which is what keeps [`jam`](crate::jam)'s backreference table cheap.
//! - Equality, drop, and [`Display`](fmt::Display) are iterative, so
//!   arbitrarily deep nouns (e.g. cued from hostile jamfiles) cannot
//!   overflow the stack through this module.
//!
//! With the `sync` feature the internal pointer is `Arc` instead of `Rc`,
//! making both types `Send + Sync`.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::mem;
use std::ops::Deref;

#[cfg(feature = "sync")]
pub(crate) type P<T> = std::sync::Arc<T>;
#[cfg(not(feature = "sync"))]
pub(crate) type P<T> = std::rc::Rc<T>;

// ----------------------------------------------------------------------
// Atom
// ----------------------------------------------------------------------

/// An arbitrary-precision natural number: the atom half of a noun.
///
/// Values that fit in a `u64` are stored inline; larger values are stored
/// as shared little-endian bytes. The representation is normalized (the
/// byte form is only used for values above `u64::MAX` and never carries
/// trailing zero bytes), so derived structural equality is value equality.
#[derive(Clone, PartialEq, Eq)]
pub struct Atom(Repr);

#[derive(Clone, PartialEq, Eq)]
enum Repr {
    Small(u64),
    /// Little-endian bytes; invariant: no trailing zeros, length > 8.
    Big(P<[u8]>),
}

impl Atom {
    /// The atom `0`.
    pub const ZERO: Atom = Atom(Repr::Small(0));

    /// Build an atom from little-endian bytes (trailing zeros ignored).
    pub fn from_le_bytes(bytes: &[u8]) -> Atom {
        let end = bytes.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
        let sig = &bytes[..end];
        if sig.len() <= 8 {
            let mut buf = [0u8; 8];
            buf[..sig.len()].copy_from_slice(sig);
            Atom(Repr::Small(u64::from_le_bytes(buf)))
        } else {
            Atom(Repr::Big(P::from(sig)))
        }
    }

    /// The significant little-endian bytes of the value (empty for `0`).
    pub fn to_le_bytes(&self) -> Vec<u8> {
        self.le_bytes().to_vec()
    }

    pub(crate) fn le_bytes(&self) -> AtomBytes<'_> {
        match &self.0 {
            Repr::Small(v) => {
                let buf = v.to_le_bytes();
                let len = (8 - v.leading_zeros() / 8) as usize;
                AtomBytes::Inline {
                    buf,
                    len: if *v == 0 { 0 } else { len },
                }
            }
            Repr::Big(b) => AtomBytes::Slice(b),
        }
    }

    /// Number of significant bits (`0` for the atom `0`).
    pub fn bit_len(&self) -> u64 {
        match &self.0 {
            Repr::Small(v) => (64 - v.leading_zeros()) as u64,
            Repr::Big(b) => {
                let last = *b.last().expect("Big atom is nonempty");
                (b.len() as u64 - 1) * 8 + (8 - last.leading_zeros()) as u64
            }
        }
    }

    /// True iff the value is `0`.
    pub fn is_zero(&self) -> bool {
        matches!(self.0, Repr::Small(0))
    }

    /// The value as a `u64`, if it fits.
    pub fn as_u64(&self) -> Option<u64> {
        match &self.0 {
            Repr::Small(v) => Some(*v),
            Repr::Big(_) => None,
        }
    }

    /// Parse a run of ASCII decimal digits (separators already removed).
    pub(crate) fn from_decimal_digits(digits: &str) -> Atom {
        debug_assert!(digits.bytes().all(|b| b.is_ascii_digit()));
        let mut small: u64 = 0;
        let mut iter = digits.bytes();
        for (i, b) in iter.by_ref().enumerate() {
            let d = (b - b'0') as u64;
            match small.checked_mul(10).and_then(|v| v.checked_add(d)) {
                Some(v) => small = v,
                None => {
                    // Spill to bytes and finish there.
                    let mut bytes = small.to_le_bytes().to_vec();
                    mul10_add(&mut bytes, b - b'0');
                    let _ = i;
                    for b2 in iter {
                        mul10_add(&mut bytes, b2 - b'0');
                    }
                    return Atom::from_le_bytes(&bytes);
                }
            }
        }
        Atom(Repr::Small(small))
    }

    /// Parse a run of ASCII hex digits, most significant first
    /// (separators already removed).
    pub(crate) fn from_hex_digits(digits: &str) -> Atom {
        debug_assert!(digits.bytes().all(|b| b.is_ascii_hexdigit()));
        let nib = |b: u8| -> u8 {
            match b {
                b'0'..=b'9' => b - b'0',
                b'a'..=b'f' => b - b'a' + 10,
                _ => b - b'A' + 10,
            }
        };
        let ds = digits.as_bytes();
        let mut bytes = Vec::with_capacity(ds.len().div_ceil(2));
        let mut i = ds.len();
        while i >= 2 {
            bytes.push(nib(ds[i - 1]) | (nib(ds[i - 2]) << 4));
            i -= 2;
        }
        if i == 1 {
            bytes.push(nib(ds[0]));
        }
        Atom::from_le_bytes(&bytes)
    }

    /// Pack a string's UTF-8 bytes as a little-endian natural (a cord).
    pub fn from_cord(s: &str) -> Atom {
        Atom::from_le_bytes(s.as_bytes())
    }

    /// Plain decimal digits, no separators.
    pub fn to_decimal_string(&self) -> String {
        match &self.0 {
            Repr::Small(v) => v.to_string(),
            Repr::Big(b) => {
                // Little-endian u32 limbs; peel base-1e9 groups.
                let mut limbs: Vec<u32> = b
                    .chunks(4)
                    .map(|c| {
                        let mut w = [0u8; 4];
                        w[..c.len()].copy_from_slice(c);
                        u32::from_le_bytes(w)
                    })
                    .collect();
                let mut groups: Vec<u32> = Vec::new();
                while !limbs.is_empty() {
                    let mut rem: u64 = 0;
                    for l in limbs.iter_mut().rev() {
                        let cur = (rem << 32) | u64::from(*l);
                        *l = (cur / 1_000_000_000) as u32;
                        rem = cur % 1_000_000_000;
                    }
                    while limbs.last() == Some(&0) {
                        limbs.pop();
                    }
                    groups.push(rem as u32);
                }
                let mut out = String::new();
                for (i, g) in groups.iter().rev().enumerate() {
                    if i == 0 {
                        out.push_str(&g.to_string());
                    } else {
                        out.push_str(&format!("{g:09}"));
                    }
                }
                out
            }
        }
    }

    /// A deterministic 64-bit structural hash (FNV-1a over the bytes).
    pub(crate) fn hash64(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in self.le_bytes().iter() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }

    /// `self * 2 + bit` — used by schema axis resolution.
    pub(crate) fn double_plus(&self, bit: bool) -> Atom {
        if let Repr::Small(v) = self.0 {
            if v.leading_zeros() > 0 {
                return Atom(Repr::Small((v << 1) | u64::from(bit)));
            }
        }
        let bits = self.bit_len() + 1;
        let mut out = vec![0u8; (bits as usize).div_ceil(8)];
        or_shifted(&mut out, &self.le_bytes(), 1);
        if bit {
            out[0] |= 1;
        }
        Atom::from_le_bytes(&out)
    }
}

/// Hoon's `+peg`: re-root axis `b` inside the subtree at axis `a`.
///
/// Equivalently: keep `a`'s bits, then append `b`'s bits below its
/// leading 1. Returns `None` iff `b` is `0` (axis 0 does not exist).
///
/// ```
/// use nockasm::{peg, Atom};
/// let a = |n: u64| Atom::from(n);
/// assert_eq!(peg(&a(3), &a(2)), Some(a(6)));
/// assert_eq!(peg(&a(3), &a(7)), Some(a(15)));
/// assert_eq!(peg(&a(3), &a(0)), None);
/// ```
pub fn peg(a: &Atom, b: &Atom) -> Option<Atom> {
    if b.is_zero() {
        return None;
    }
    let shift = b.bit_len() - 1;
    if let (Some(av), Some(bv)) = (a.as_u64(), b.as_u64()) {
        // b fits u64, so shift <= 63; the result fits when the bit counts do.
        if a.bit_len() + shift <= 64 {
            let low = if shift == 0 {
                0
            } else {
                bv & ((1u64 << shift) - 1)
            };
            return Some(Atom(Repr::Small((av << shift) | low)));
        }
    }
    let total_bits = a.bit_len() + shift;
    let mut out = vec![0u8; (total_bits as usize).div_ceil(8)];
    // b's bits below its leading 1: copy the low bytes, then clear the
    // leading 1 where it falls inside a copied byte (b is normalized, so
    // there is nothing above it to mask).
    let bb = b.le_bytes();
    let low_bytes = (shift as usize).div_ceil(8);
    out[..low_bytes].copy_from_slice(&bb[..low_bytes]);
    if shift % 8 != 0 {
        out[(shift / 8) as usize] &= (1u8 << (shift % 8)) - 1;
    }
    or_shifted(&mut out, &a.le_bytes(), shift);
    Some(Atom::from_le_bytes(&out))
}

/// `dst |= src << shift_bits` over little-endian byte buffers.
fn or_shifted(dst: &mut [u8], src: &[u8], shift_bits: u64) {
    let byte_shift = (shift_bits / 8) as usize;
    let bit_shift = (shift_bits % 8) as u32;
    for (i, &b) in src.iter().enumerate() {
        let lo_idx = i + byte_shift;
        if lo_idx < dst.len() {
            dst[lo_idx] |= b << bit_shift;
        }
        if bit_shift > 0 {
            let hi_idx = lo_idx + 1;
            if hi_idx < dst.len() {
                dst[hi_idx] |= b >> (8 - bit_shift);
            }
        }
    }
}

/// `dst = dst * 10 + digit` over little-endian bytes.
fn mul10_add(bytes: &mut Vec<u8>, digit: u8) {
    let mut carry: u32 = u32::from(digit);
    for b in bytes.iter_mut() {
        let v = u32::from(*b) * 10 + carry;
        *b = v as u8;
        carry = v >> 8;
    }
    while carry > 0 {
        bytes.push(carry as u8);
        carry >>= 8;
    }
}

/// Borrowed view of an atom's significant little-endian bytes.
pub(crate) enum AtomBytes<'a> {
    Inline { buf: [u8; 8], len: usize },
    Slice(&'a [u8]),
}

impl Deref for AtomBytes<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            AtomBytes::Inline { buf, len } => &buf[..*len],
            AtomBytes::Slice(s) => s,
        }
    }
}

impl From<u64> for Atom {
    fn from(v: u64) -> Atom {
        Atom(Repr::Small(v))
    }
}

impl From<u32> for Atom {
    fn from(v: u32) -> Atom {
        Atom(Repr::Small(v.into()))
    }
}

impl From<u8> for Atom {
    fn from(v: u8) -> Atom {
        Atom(Repr::Small(v.into()))
    }
}

impl From<usize> for Atom {
    fn from(v: usize) -> Atom {
        Atom(Repr::Small(v as u64))
    }
}

impl From<u128> for Atom {
    fn from(v: u128) -> Atom {
        Atom::from_le_bytes(&v.to_le_bytes())
    }
}

impl PartialEq<u64> for Atom {
    fn eq(&self, other: &u64) -> bool {
        self.as_u64() == Some(*other)
    }
}

impl PartialOrd for Atom {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Atom {
    fn cmp(&self, other: &Self) -> Ordering {
        match (&self.0, &other.0) {
            (Repr::Small(a), Repr::Small(b)) => a.cmp(b),
            (Repr::Small(_), Repr::Big(_)) => Ordering::Less,
            (Repr::Big(_), Repr::Small(_)) => Ordering::Greater,
            (Repr::Big(a), Repr::Big(b)) => a
                .len()
                .cmp(&b.len())
                .then_with(|| a.iter().rev().cmp(b.iter().rev())),
        }
    }
}

impl Hash for Atom {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.hash64());
    }
}

impl fmt::Display for Atom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_decimal_string())
    }
}

impl fmt::Debug for Atom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

// ----------------------------------------------------------------------
// Noun
// ----------------------------------------------------------------------

/// A noun: an [`Atom`] or a cell (an ordered pair of nouns).
///
/// Cells are cheap to clone (reference-counted) and carry a structural
/// hash computed at construction. Equality is structural.
#[derive(Clone)]
pub struct Noun(NounRepr);

#[derive(Clone)]
enum NounRepr {
    Atom(Atom),
    Cell(P<CellData>),
}

pub(crate) struct CellData {
    head: Noun,
    tail: Noun,
    hash: u64,
}

/// Borrowed view of a noun for pattern matching.
#[derive(Clone, Copy)]
pub enum NounRef<'a> {
    /// The noun is an atom.
    Atom(&'a Atom),
    /// The noun is a cell `[head tail]`.
    Cell(&'a Noun, &'a Noun),
}

fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

impl Noun {
    /// Build an atom noun.
    pub fn atom(a: impl Into<Atom>) -> Noun {
        Noun(NounRepr::Atom(a.into()))
    }

    /// Build the cell `[head tail]`.
    pub fn cell(head: impl Into<Noun>, tail: impl Into<Noun>) -> Noun {
        let head = head.into();
        let tail = tail.into();
        let hash = splitmix(head.hash64() ^ splitmix(tail.hash64() ^ 0xce11));
        Noun(NounRepr::Cell(P::new(CellData { head, tail, hash })))
    }

    /// Right-associate `elems` into nested cells: `[a b c]` = `[a [b c]]`.
    ///
    /// Returns `None` when fewer than two elements are given (Nock has no
    /// unary cells).
    pub fn autocons(elems: Vec<Noun>) -> Option<Noun> {
        if elems.len() < 2 {
            return None;
        }
        let mut it = elems.into_iter().rev();
        let mut acc = it.next().expect("len >= 2");
        for e in it {
            acc = Noun::cell(e, acc);
        }
        Some(acc)
    }

    /// View for pattern matching.
    pub fn view(&self) -> NounRef<'_> {
        match &self.0 {
            NounRepr::Atom(a) => NounRef::Atom(a),
            NounRepr::Cell(c) => NounRef::Cell(&c.head, &c.tail),
        }
    }

    /// The atom, if this noun is one.
    pub fn as_atom(&self) -> Option<&Atom> {
        match &self.0 {
            NounRepr::Atom(a) => Some(a),
            NounRepr::Cell(_) => None,
        }
    }

    /// The `(head, tail)` pair, if this noun is a cell.
    pub fn as_cell(&self) -> Option<(&Noun, &Noun)> {
        match &self.0 {
            NounRepr::Atom(_) => None,
            NounRepr::Cell(c) => Some((&c.head, &c.tail)),
        }
    }

    /// True iff this noun is an atom.
    pub fn is_atom(&self) -> bool {
        matches!(self.0, NounRepr::Atom(_))
    }

    /// True iff this noun is a cell.
    pub fn is_cell(&self) -> bool {
        matches!(self.0, NounRepr::Cell(_))
    }

    pub(crate) fn hash64(&self) -> u64 {
        match &self.0 {
            NounRepr::Atom(a) => a.hash64(),
            NounRepr::Cell(c) => c.hash,
        }
    }

    /// Display adapter for the explicit-binary-cell form,
    /// e.g. `[8 [[4 [0 1]] [5 [[0 2] [0 3]]]]]`.
    pub fn pretty(&self) -> Pretty<'_> {
        Pretty(self)
    }
}

impl From<Atom> for Noun {
    fn from(a: Atom) -> Noun {
        Noun(NounRepr::Atom(a))
    }
}

impl From<u64> for Noun {
    fn from(v: u64) -> Noun {
        Noun::atom(v)
    }
}

impl From<u32> for Noun {
    fn from(v: u32) -> Noun {
        Noun::atom(v)
    }
}

impl From<u8> for Noun {
    fn from(v: u8) -> Noun {
        Noun::atom(v)
    }
}

impl From<usize> for Noun {
    fn from(v: usize) -> Noun {
        Noun::atom(v)
    }
}

impl From<&Noun> for Noun {
    fn from(n: &Noun) -> Noun {
        n.clone()
    }
}

impl PartialEq for Noun {
    fn eq(&self, other: &Self) -> bool {
        let mut stack: Vec<(&Noun, &Noun)> = vec![(self, other)];
        while let Some((a, b)) = stack.pop() {
            match (&a.0, &b.0) {
                (NounRepr::Atom(x), NounRepr::Atom(y)) => {
                    if x != y {
                        return false;
                    }
                }
                (NounRepr::Cell(x), NounRepr::Cell(y)) => {
                    if P::ptr_eq(x, y) {
                        continue;
                    }
                    if x.hash != y.hash {
                        return false;
                    }
                    stack.push((&x.tail, &y.tail));
                    stack.push((&x.head, &y.head));
                }
                _ => return false,
            }
        }
        true
    }
}

impl Eq for Noun {}

impl Hash for Noun {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.hash64());
    }
}

impl Drop for Noun {
    /// Iterative teardown: dropping a deep noun must not recurse.
    fn drop(&mut self) {
        if matches!(self.0, NounRepr::Atom(_)) {
            return;
        }
        let NounRepr::Cell(rc) = mem::replace(&mut self.0, NounRepr::Atom(Atom::ZERO)) else {
            return;
        };
        let Some(cell) = P::into_inner(rc) else {
            return; // shared: the count just dropped, nothing to tear down
        };
        let mut stack = vec![cell];
        while let Some(cell) = stack.pop() {
            let CellData { head, tail, .. } = cell;
            for mut child in [head, tail] {
                if matches!(child.0, NounRepr::Atom(_)) {
                    continue;
                }
                let NounRepr::Cell(rc) = mem::replace(&mut child.0, NounRepr::Atom(Atom::ZERO))
                else {
                    continue;
                };
                if let Some(inner) = P::into_inner(rc) {
                    stack.push(inner);
                }
            }
        }
    }
}

// ----------------------------------------------------------------------
// Printing
// ----------------------------------------------------------------------

enum Tok<'a> {
    N(&'a Noun),
    S(&'static str),
}

impl fmt::Display for Noun {
    /// Canonical flat form: right spines flatten, atoms are plain
    /// decimal — e.g. `[8 [4 0 1] 5 [0 2] 0 3]`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut stack: Vec<Tok<'_>> = vec![Tok::N(self)];
        while let Some(t) = stack.pop() {
            match t {
                Tok::S(s) => f.write_str(s)?,
                Tok::N(n) => match n.view() {
                    NounRef::Atom(a) => write!(f, "{a}")?,
                    NounRef::Cell(..) => {
                        f.write_str("[")?;
                        stack.push(Tok::S("]"));
                        // collect the right spine
                        let mut elems: Vec<&Noun> = Vec::new();
                        let mut cur = n;
                        while let NounRef::Cell(h, t) = cur.view() {
                            elems.push(h);
                            cur = t;
                        }
                        elems.push(cur);
                        for (i, e) in elems.into_iter().enumerate().rev() {
                            stack.push(Tok::N(e));
                            if i > 0 {
                                stack.push(Tok::S(" "));
                            }
                        }
                    }
                },
            }
        }
        Ok(())
    }
}

impl fmt::Debug for Noun {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// Display adapter returned by [`Noun::pretty`].
pub struct Pretty<'a>(&'a Noun);

impl fmt::Display for Pretty<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut stack: Vec<Tok<'_>> = vec![Tok::N(self.0)];
        while let Some(t) = stack.pop() {
            match t {
                Tok::S(s) => f.write_str(s)?,
                Tok::N(n) => match n.view() {
                    NounRef::Atom(a) => write!(f, "{a}")?,
                    NounRef::Cell(h, t) => {
                        f.write_str("[")?;
                        stack.push(Tok::S("]"));
                        stack.push(Tok::N(t));
                        stack.push(Tok::S(" "));
                        stack.push(Tok::N(h));
                    }
                },
            }
        }
        Ok(())
    }
}

/// Build a noun from bracketed, space-separated elements: right-spine
/// cells auto-associate exactly as in `.nasm` / Hoon noun syntax.
///
/// Elements are integer literals, nested `[...]` cells, or parenthesized
/// expressions of any type convertible into [`Noun`]:
///
/// ```
/// use nockasm::{noun, Noun};
/// assert_eq!(noun![4 0 1], Noun::cell(4u64, Noun::cell(0u64, 1u64)));
/// assert_eq!(noun![8 [1 0] 4 0 6].to_string(), "[8 [1 0] 4 0 6]");
/// let inner = noun![0 2];
/// assert_eq!(noun![5 (inner) 0 3].to_string(), "[5 [0 2] 0 3]");
/// ```
#[macro_export]
macro_rules! noun {
    ([$($inner:tt)+]) => { $crate::noun!($($inner)+) };
    (($e:expr)) => { $crate::Noun::from($e) };
    ($n:literal) => { $crate::Noun::from($n as u64) };
    ($first:tt $($rest:tt)+) => {
        $crate::Noun::cell($crate::noun!($first), $crate::noun!($($rest)+))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: u64) -> Atom {
        Atom::from(v)
    }

    #[test]
    fn atom_normalization() {
        assert_eq!(Atom::from_le_bytes(&[42, 0, 0]), a(42));
        assert_eq!(Atom::from_le_bytes(&[]), Atom::ZERO);
        assert_eq!(Atom::from_le_bytes(&[0; 16]), Atom::ZERO);
        let nine = Atom::from_le_bytes(&[1, 0, 0, 0, 0, 0, 0, 0, 1]);
        assert_eq!(nine.bit_len(), 65);
        assert_eq!(nine.as_u64(), None);
        assert_eq!(nine, Atom::from(1u128 + (1u128 << 64)));
    }

    #[test]
    fn atom_decimal() {
        assert_eq!(Atom::from_decimal_digits("0"), a(0));
        assert_eq!(Atom::from_decimal_digits("42"), a(42));
        assert_eq!(
            Atom::from_decimal_digits("18446744073709551616"), // 2^64
            Atom::from(1u128 << 64)
        );
        let big = Atom::from_decimal_digits("340282366920938463463374607431768211456"); // 2^128
        assert_eq!(big.bit_len(), 129);
        assert_eq!(
            big.to_decimal_string(),
            "340282366920938463463374607431768211456"
        );
        assert_eq!(a(0).to_decimal_string(), "0");
    }

    #[test]
    fn atom_hex() {
        assert_eq!(Atom::from_hex_digits("2a"), a(42));
        assert_eq!(Atom::from_hex_digits("10000"), a(65536));
        assert_eq!(Atom::from_hex_digits("f"), a(15));
        assert_eq!(
            Atom::from_hex_digits("100000000000000000"), // 2^68
            Atom::from(1u128 << 68)
        );
    }

    #[test]
    fn atom_cord() {
        assert_eq!(Atom::from_cord("fast"), a(0x7473_6166));
        assert_eq!(Atom::from_cord(""), a(0));
        assert_eq!(Atom::from_cord("a"), a(97));
    }

    #[test]
    fn atom_ord() {
        let big = Atom::from(1u128 << 100);
        let bigger = Atom::from((1u128 << 100) + 1);
        assert!(a(u64::MAX) < big);
        assert!(big < bigger);
        assert!(a(1) < a(2));
    }

    #[test]
    fn peg_matches_reference() {
        // The recursive definition, checked against the bit-splice impl.
        fn peg_ref(a: u128, b: u128) -> u128 {
            if b == 1 {
                a
            } else if b % 2 == 0 {
                2 * peg_ref(a, b / 2)
            } else {
                2 * peg_ref(a, b / 2) + 1
            }
        }
        for pa in 1u64..=9 {
            for pb in 1u64..=33 {
                assert_eq!(
                    peg(&a(pa), &a(pb)),
                    Some(Atom::from(peg_ref(pa.into(), pb.into()))),
                    "peg({pa}, {pb})"
                );
            }
        }
        assert_eq!(peg(&a(3), &a(0)), None);
        // Big-path: peg over 2^80
        let b = Atom::from(1u128 << 80);
        assert_eq!(peg(&a(3), &b), Some(Atom::from(3u128 << 80)));
        assert_eq!(peg(&b, &a(3)), Some(Atom::from((1u128 << 81) + 1)));
    }

    #[test]
    fn double_plus() {
        assert_eq!(a(1).double_plus(false), a(2));
        assert_eq!(a(1).double_plus(true), a(3));
        assert_eq!(
            Atom::from(u64::MAX).double_plus(true),
            Atom::from((u128::from(u64::MAX) << 1) | 1)
        );
    }

    #[test]
    fn noun_display() {
        assert_eq!(
            noun![8 [4 0 6] [0 6] [0 2] 0 15].to_string(),
            "[8 [4 0 6] [0 6] [0 2] 0 15]"
        );
        assert_eq!(noun![4 0 1].pretty().to_string(), "[4 [0 1]]");
        assert_eq!(Noun::from(42u64).to_string(), "42");
    }

    #[test]
    fn noun_eq_and_hash() {
        let x = noun![8 [1 0] 4 0 6];
        let y = noun![8 [1 0] 4 0 6];
        assert_eq!(x, y);
        assert_eq!(x.hash64(), y.hash64());
        assert_ne!(x, noun![8 [1 0] 4 0 7]);
        assert_ne!(x, Noun::from(5u64));
    }

    #[test]
    fn deep_noun_drop_and_eq() {
        // A million-deep right spine: Drop and Eq must not recurse.
        let mut n = Noun::from(0u64);
        for _ in 0..1_000_000 {
            n = Noun::cell(1u64, n);
        }
        let m = n.clone();
        assert_eq!(n, m);
        drop(n);
        drop(m);
    }

    #[test]
    fn autocons() {
        assert_eq!(Noun::autocons(vec![]), None);
        assert_eq!(Noun::autocons(vec![Noun::from(1u64)]), None);
        assert_eq!(
            Noun::autocons(vec![1u64.into(), 2u64.into(), 3u64.into()]),
            Some(noun![1 2 3])
        );
    }
}
