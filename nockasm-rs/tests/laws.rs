//! The target-IR laws over the shared corpus (ports of
//! `tests/test_render.py` and `tests/test_lift.py`):
//!
//! - round trip: `expand(render(s, a)) == lower(s, a) == expand(src)`
//! - idempotence: `render(parse(render(s, a))) == render(s, a)`
//! - width: no rendered line exceeds 76 columns
//! - lift soundness: `lower(None, lift(f)) == f`, also through text
//! - serialization: `cue(jam(n)) == n`, plus the fixed jam vectors

mod common;

use nockasm::{cue, expand, jam, lift, lower, nasm_from_jam, noun, parse, render, Noun};

#[test]
fn round_trip_and_idempotence() {
    for (name, src) in common::corpus() {
        let program = parse(&src).unwrap_or_else(|e| panic!("{name}: parse: {e}"));
        let text = program.render();

        let want = expand(&src).unwrap_or_else(|e| panic!("{name}: expand: {e}"));
        let low = program
            .lower()
            .unwrap_or_else(|e| panic!("{name}: lower: {e}"));
        let rt = expand(&text).unwrap_or_else(|e| panic!("{name}: re-expand: {e}"));
        assert_eq!(want, low, "{name}: lower disagrees with expand");
        assert_eq!(
            want, rt,
            "{name}: round trip through render disagrees\n{text}"
        );

        let again = parse(&text)
            .unwrap_or_else(|e| panic!("{name}: re-parse: {e}"))
            .render();
        assert_eq!(text, again, "{name}: render not idempotent");

        for line in text.lines() {
            assert!(line.len() <= 76, "{name}: line over 76 cols: {line:?}");
        }
    }
}

#[test]
fn lift_soundness_over_corpus() {
    for (name, src) in common::corpus() {
        let f = expand(&src).unwrap_or_else(|e| panic!("{name}: {e}"));
        let ast = lift(&f);
        let low = lower(None, &ast).unwrap_or_else(|e| panic!("{name}: lower(lift): {e}"));
        assert_eq!(low, f, "{name}: lower(lift) unsound");
        let rt = expand(&render(None, &ast))
            .unwrap_or_else(|e| panic!("{name}: expand(render(lift)): {e}"));
        assert_eq!(rt, f, "{name}: lift unsound through text");
    }
}

/// Handmade nouns exercising every lift fallback path
/// (`tests/test_lift.py::CASES`).
#[test]
fn lift_fallback_zoo() {
    let cases: Vec<(&str, Noun)> = vec![
        ("atom in formula position embeds opaque", noun![2 5 6]),
        ("opcode head above 12", noun![13 3]),
        ("malformed scry", noun![12 3]),
        ("slot of a cell", noun![0 2 3]),
        ("bare atom", Noun::from(42u64)),
        ("const of deep data", noun![1 9 9 9 9]),
        ("cons-formula", Noun::cell(noun![4 0 2], noun![4 0 3])),
        ("hint static", noun![11 1953718630 0 1]),
        ("hint dynamic", noun![11 [1953718630 1 0] 0 1]),
        ("malformed if", noun![6 [1 0] 5]),
        ("call with cell axis", noun![9 [2 3] 0 1]),
    ];
    for (name, f) in cases {
        let ast = lift(&f);
        let low = lower(None, &ast).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(low, f, "{name}: lower(lift) unsound");
        let rt = expand(&render(None, &ast)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(rt, f, "{name}: unsound through text");
    }
}

#[test]
fn jam_vectors_and_round_trips() {
    // Vectors from tests/test_lift.py.
    assert_eq!(jam(&noun![0]), vec![2]);
    assert_eq!(jam(&noun![1]), vec![12]);
    let jammed = jam(&Noun::cell(1u64, 1u64)); // backref-eligible atom
    assert_eq!(jammed, 817u64.to_le_bytes()[..2].to_vec());

    let shared = noun![1 2 3];
    let cases: Vec<(&str, Noun)> = vec![
        ("atom", Noun::from(42u64)),
        ("pair", noun![1 2]),
        ("deep", noun![1 2 3 4 5]),
        (
            "shared subtree",
            Noun::cell(Noun::cell(shared.clone(), shared.clone()), shared),
        ),
        (
            "big atom",
            Noun::from(nockasm::Atom::from((1u128 << 100) + 12345)),
        ),
    ];
    for (name, n) in cases {
        assert_eq!(cue(&jam(&n)).as_ref(), Ok(&n), "{name}");
    }
}

#[test]
fn corpus_jam_round_trips() {
    for (name, src) in common::corpus() {
        let f = expand(&src).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(cue(&jam(&f)).as_ref(), Ok(&f), "{name}: cue(jam) failed");
    }
}

#[test]
fn nasm_from_jam_round_trips() {
    let f = expand("(%inc (%slot 1))").unwrap();
    let bytes = jam(&f);
    let text = nasm_from_jam(&bytes).unwrap();
    assert_eq!(text, "(%inc (%slot 1))\n");
    assert_eq!(expand(&text).unwrap(), f);
}

/// The benchmark `.nasm` transcriptions expand to formulas that actually
/// compute (a tiny Nock 4K evaluator keeps this self-contained). The
/// naive evaluator recurses per Nock call — Ackermann needs real depth —
/// so it runs on a dedicated big-stack thread; only `Send` primitives
/// cross the boundary (nouns are built inside).
#[test]
fn benchmarks_execute() {
    // (name, subject as a right-associated spine, expected atom)
    let expectations: &[(&str, &[u64], u64)] = &[
        ("add", &[1000, 2000], 3000),
        ("dec", &[100], 99),
        ("factorial", &[5], 120),
        ("fibonacci", &[10], 55),
        ("ackermann", &[3, 3], 61),
    ];
    let handle = std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(move || {
            let cases = common::benchmark_cases();
            for (name, subject_spine, want) in expectations {
                let (_, src) = cases
                    .iter()
                    .find(|(n, _)| n == name)
                    .unwrap_or_else(|| panic!("benchmark {name} present"));
                let formula = expand(src).unwrap_or_else(|e| panic!("{name}: {e}"));
                let subject =
                    Noun::autocons(subject_spine.iter().map(|&v| Noun::from(v)).collect())
                        .unwrap_or_else(|| Noun::from(subject_spine[0]));
                let got = nock(subject, &formula).unwrap_or_else(|| panic!("{name}: crashed"));
                assert_eq!(got, Noun::from(*want), "{name}");
            }
        })
        .expect("thread spawns");
    handle.join().expect("benchmarks execute without overflow");
}

/// A minimal Nock 4K evaluator, for executing the benchmark corpus only.
fn nock(subject: Noun, formula: &Noun) -> Option<Noun> {
    let (op, tail) = formula.as_cell()?;
    if op.is_cell() {
        // cons-formula: distribute
        let head = nock(subject.clone(), op)?;
        let rest = nock(subject, tail)?;
        return Some(Noun::cell(head, rest));
    }
    let opcode = op.as_atom()?.as_u64()?;
    match opcode {
        0 => slot(&subject, tail.as_atom()?),
        1 => Some(tail.clone()),
        2 => {
            let (s, f) = tail.as_cell()?;
            let new_subject = nock(subject.clone(), s)?;
            let new_formula = nock(subject, f)?;
            nock(new_subject, &new_formula)
        }
        3 => Some(Noun::from(u64::from(!nock(subject, tail)?.is_cell()))),
        4 => {
            let n = nock(subject, tail)?;
            let a = n.as_atom()?;
            // Benchmark arithmetic stays small; grow through u128 for
            // headroom without a general bignum increment.
            let v = a.as_u64().map(u128::from).unwrap_or_else(|| {
                let bytes = a.to_le_bytes();
                assert!(bytes.len() <= 16, "increment beyond test range");
                let mut buf = [0u8; 16];
                buf[..bytes.len()].copy_from_slice(&bytes);
                u128::from_le_bytes(buf)
            });
            Some(Noun::from(nockasm::Atom::from(v + 1)))
        }
        5 => {
            let (x, y) = tail.as_cell()?;
            let a = nock(subject.clone(), x)?;
            let b = nock(subject, y)?;
            Some(Noun::from(u64::from(a != b)))
        }
        6 => {
            let (c, branches) = tail.as_cell()?;
            let (t, e) = branches.as_cell()?;
            match nock(subject.clone(), c)?.as_atom()?.as_u64()? {
                0 => nock(subject, t),
                1 => nock(subject, e),
                _ => None,
            }
        }
        7 => {
            let (f, g) = tail.as_cell()?;
            let mid = nock(subject, f)?;
            nock(mid, g)
        }
        8 => {
            let (f, g) = tail.as_cell()?;
            let pushed = nock(subject.clone(), f)?;
            nock(Noun::cell(pushed, subject), g)
        }
        9 => {
            let (ax, f) = tail.as_cell()?;
            let core = nock(subject, f)?;
            let arm = slot(&core, ax.as_atom()?)?;
            nock(core, &arm)
        }
        10 => {
            let (spec, f) = tail.as_cell()?;
            let (ax, v) = spec.as_cell()?;
            let value = nock(subject.clone(), v)?;
            let target = nock(subject, f)?;
            edit(&target, ax.as_atom()?, value)
        }
        11 => {
            let (spec, f) = tail.as_cell()?;
            if let Some((_, clue)) = spec.as_cell() {
                nock(subject.clone(), clue)?; // dynamic hint: evaluated
            }
            nock(subject, f)
        }
        _ => None,
    }
}

fn slot(subject: &Noun, axis: &nockasm::Atom) -> Option<Noun> {
    let bits = axis.bit_len();
    if bits == 0 {
        return None; // axis 0 crashes
    }
    let bytes = axis.to_le_bytes();
    let mut cur = subject.clone();
    for i in (0..bits - 1).rev() {
        let bit = (bytes[(i / 8) as usize] >> (i % 8)) & 1;
        let (h, t) = cur.as_cell()?;
        cur = if bit == 0 { h.clone() } else { t.clone() };
    }
    Some(cur)
}

fn edit(target: &Noun, axis: &nockasm::Atom, value: Noun) -> Option<Noun> {
    let bits = axis.bit_len();
    if bits == 0 {
        return None; // axis 0 crashes
    }
    // The bits below the leading 1, MSB-first, are the path.
    edit_at(target, &axis.to_le_bytes(), bits - 1, value)
}

fn edit_at(target: &Noun, path: &[u8], depth: u64, value: Noun) -> Option<Noun> {
    if depth == 0 {
        return Some(value);
    }
    let i = depth - 1;
    let bit = (path[(i / 8) as usize] >> (i % 8)) & 1;
    let (h, t) = target.as_cell()?;
    if bit == 0 {
        Some(Noun::cell(edit_at(h, path, i, value)?, t.clone()))
    } else {
        Some(Noun::cell(h.clone(), edit_at(t, path, i, value)?))
    }
}
