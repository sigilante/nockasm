//! Property tests over generated nouns (dependency-free: a fixed-seed
//! splitmix64 generator, so failures reproduce exactly).
//!
//!     cue(jam(n)) == n                                (serialization)
//!     lower(None, lift(f)) == f                       (lift soundness)
//!     expand(render(None, lift(f))) == f              (through text)
//!     render(parse(render(...))) == render(...)       (idempotence)

use nockasm::{cue, expand, jam, lift, lower, parse, render, Atom, Noun};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn gen_atom(rng: &mut Rng) -> Atom {
    match rng.below(10) {
        // Mostly opcode-sized atoms, so generated cells often look like
        // (possibly malformed) formulas and exercise lift's grammar.
        0..=5 => Atom::from(rng.below(13)),
        6 | 7 => Atom::from(rng.below(1 << 20)),
        8 => Atom::from(rng.next()),
        _ => {
            let len = (rng.below(24) + 9) as usize;
            let mut bytes: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
            *bytes.last_mut().expect("nonempty") |= 1; // keep it big
            Atom::from_le_bytes(&bytes)
        }
    }
}

fn gen_noun(rng: &mut Rng, depth: u32) -> Noun {
    if depth == 0 || rng.below(3) == 0 {
        Noun::from(gen_atom(rng))
    } else {
        let head = gen_noun(rng, depth - 1);
        let tail = gen_noun(rng, depth - 1);
        Noun::cell(head, tail)
    }
}

#[test]
fn serialization_round_trips() {
    let mut rng = Rng(0x0c4a5);
    for i in 0..400 {
        let n = gen_noun(&mut rng, 8);
        assert_eq!(cue(&jam(&n)).as_ref(), Ok(&n), "case {i}: {n}");
    }
}

#[test]
fn lift_is_sound_on_arbitrary_nouns() {
    let mut rng = Rng(0xdecafbad);
    for i in 0..400 {
        let f = gen_noun(&mut rng, 7);
        let ast = lift(&f);
        let low = lower(None, &ast).unwrap_or_else(|e| panic!("case {i}: {e}"));
        assert_eq!(low, f, "case {i}: lower(lift) unsound for {f}");

        let text = render(None, &ast);
        let rt = expand(&text).unwrap_or_else(|e| panic!("case {i}: {e}\n{text}"));
        assert_eq!(rt, f, "case {i}: unsound through text for {f}\n{text}");

        let again = parse(&text)
            .unwrap_or_else(|e| panic!("case {i}: re-parse: {e}"))
            .render();
        assert_eq!(text, again, "case {i}: render not idempotent");
        for line in text.lines() {
            // The 76-column rule has exactly one escape valve, same as
            // the reference renderers: an atom wider than the line still
            // emits wide (atoms have no tall form), possibly behind the
            // tall-cell `[ ` merge.
            let mut content = line.trim_start();
            while let Some(rest) = content.strip_prefix("[ ") {
                content = rest;
            }
            let starts_atom = content.starts_with(|c: char| c.is_ascii_digit() || c == '\'');
            assert!(
                line.len() <= 76 || starts_atom,
                "case {i}: non-atom line over 76 cols: {line:?}"
            );
        }
    }
}

#[test]
fn shared_subtrees_share_encodings() {
    // Structural (not pointer) deduplication: two equal but separately
    // built subtrees must jam identically to two clones.
    let mut rng = Rng(7);
    for _ in 0..50 {
        let sub_a = gen_noun(&mut rng, 5);
        let sub_b = {
            // rebuild an equal noun with fresh allocations
            let bytes = jam(&sub_a);
            cue(&bytes).expect("round-trips")
        };
        let by_clone = Noun::cell(sub_a.clone(), sub_a.clone());
        let by_equal = Noun::cell(sub_a, sub_b);
        assert_eq!(jam(&by_clone), jam(&by_equal));
    }
}
