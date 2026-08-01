//! The depth guarantees, all checked on a 2 MiB thread stack in a debug
//! build:
//!
//! - Source admitted by the parser runs through the *entire* pipeline —
//!   lower, render, round trip, lift, jam/cue — without approaching
//!   stack exhaustion; nesting beyond the bound is a clean `TooDeep`
//!   error. The parser is the one remaining recursive stage, structured
//!   as a slim dispatcher so its frames stay small; if that test
//!   overflows, the structure has regressed (or `MAX_DEPTH` outgrew it).
//! - `lift`, `lower`, `render`, and the IR's teardown run on explicit
//!   stacks: nouns and IR arbitrarily deeper than the parser would ever
//!   admit — hostile jamfiles, machine-emitted binder chains — must
//!   flow through them untroubled.

use nockasm::{
    cue, expand, jam, lift, lower, nasm_from_jam, noun, parse, Error, Nasm, Noun, Op,
    ParseErrorKind,
};

const TEST_STACK: usize = 2 * 1024 * 1024;

fn nested(depth: usize) -> String {
    format!("{}42{}", "[1 ".repeat(depth), "]".repeat(depth))
}

#[test]
fn admitted_depth_survives_the_full_pipeline() {
    let src = nested(nockasm::parse::MAX_DEPTH - 1);
    let handle = std::thread::Builder::new()
        .stack_size(TEST_STACK)
        .spawn(move || {
            let program = parse(&src).expect("depth just under the bound parses");
            let noun = program.lower().expect("lowers");
            let text = program.render();
            assert_eq!(expand(&text).expect("re-expands"), noun, "round-trip law");
            let lifted = lift(&noun);
            assert_eq!(
                lower(None, &lifted).expect("lowers"),
                noun,
                "lift soundness"
            );
            assert_eq!(cue(&jam(&noun)).expect("cues"), noun, "serialization");
        })
        .expect("thread spawns");
    handle
        .join()
        .expect("no stack overflow at the admitted depth");
}

/// `lift`, `lower`, and the `Drop` of the IR between them run on
/// explicit stacks: nouns cued from hostile jamfiles can nest
/// arbitrarily deep in any direction, and none of it may touch the call
/// stack. Each case nests half a million levels one way — through
/// formula tails, cons-formula heads, and opcode-1 payload spines — is
/// shape-checked with an iterative walk, closes the soundness law
/// `lower(None, &lift(f)) == f` (noun equality is iterative too), and
/// is dropped, all inside a 2 MiB thread.
#[test]
fn lift_and_lower_survive_arbitrarily_deep_nouns() {
    const DEPTH: usize = 500_000;
    let handle = std::thread::Builder::new()
        .stack_size(TEST_STACK)
        .spawn(|| {
            // [4 [4 ... [0 1]]] — depth through formula tails.
            let mut n = noun![0 1];
            for _ in 0..DEPTH {
                n = Noun::cell(4u64, n);
            }
            let ast = lift(&n);
            let mut cur = &ast;
            let mut seen = 0usize;
            while let Nasm::Op(Op::Inc(inner)) = cur {
                seen += 1;
                cur = inner.as_ref();
            }
            assert_eq!(seen, DEPTH, "every %inc layer lifted");
            assert!(
                matches!(cur, Nasm::Op(Op::Slot(a)) if a.as_u64() == Some(1)),
                "innermost node is (%slot 1)"
            );
            assert_eq!(lower(None, &ast).expect("lowers"), n, "soundness at depth");
            drop(ast);

            // [[[[...] f] f] f] — depth through cons-formula heads.
            let mut n = noun![0 1];
            for _ in 0..DEPTH {
                n = Noun::cell(n, noun![0 1]);
            }
            let ast = lift(&n);
            let mut cur = &ast;
            let mut seen = 0usize;
            while let Nasm::Cell {
                first,
                second,
                rest,
            } = cur
            {
                assert!(rest.is_empty(), "cons-formula cells are pairs");
                assert!(matches!(second.as_ref(), Nasm::Op(Op::Slot(_))));
                seen += 1;
                cur = first.as_ref();
            }
            assert_eq!(seen, DEPTH, "every cons layer lifted");
            assert_eq!(lower(None, &ast).expect("lowers"), n, "soundness at depth");
            drop(ast);

            // [1 [[[...] 0] 0]] — depth through the structural fallback:
            // an opcode-1 payload whose spine heads nest leftward.
            let mut payload = Noun::cell(0u64, 0u64);
            for _ in 0..DEPTH {
                payload = Noun::cell(payload, 0u64);
            }
            let n = Noun::cell(1u64, payload);
            let ast = lift(&n);
            let Nasm::Op(Op::Const(inner)) = &ast else {
                panic!("opcode 1 lifts to %const");
            };
            let mut cur = inner.as_ref();
            let mut seen = 0usize;
            while let Nasm::Cell { first, .. } = cur {
                seen += 1;
                cur = first.as_ref();
            }
            assert_eq!(seen, DEPTH + 1, "every payload layer read structurally");
            assert_eq!(lower(None, &ast).expect("lowers"), n, "soundness at depth");
            drop(ast);
        })
        .expect("thread spawns");
    handle.join().expect("no stack overflow lifting deep nouns");
}

/// `render` (via `nasm_from_jam`: cue, lift, render) on formulas far
/// deeper than the parser would admit — the hostile `--lift` path, end
/// to end. Depth is kept to thousands, not the half million above,
/// because tall output is quadratic in size from indentation alone;
/// what matters is that this is ~10x past where the recursive renderer
/// overflowed this same 2 MiB thread. The text cannot be re-expanded
/// (the parser's depth bound is narrower, by documented contract), so
/// the assertions are structural.
#[test]
fn render_survives_formulas_beyond_parser_depth() {
    const DEPTH: usize = 4_000;
    let handle = std::thread::Builder::new()
        .stack_size(TEST_STACK)
        .spawn(|| {
            // [4 [4 ... [0 1]]]: tall (%inc nesting until the tail fits wide.
            let mut n = noun![0 1];
            for _ in 0..DEPTH {
                n = Noun::cell(4u64, n);
            }
            let text = nasm_from_jam(&jam(&n)).expect("cues and renders");
            let lines: Vec<&str> = text.lines().collect();
            assert!(lines.len() > DEPTH - 20, "one line per tall %inc layer");
            assert_eq!(lines[0], "(%inc");
            assert!(
                lines.last().expect("nonempty").ends_with(")"),
                "every opener closed"
            );
            // Past column 76 nothing fits wide, so even the innermost
            // (%slot 1) renders tall.
            assert!(text.contains("(%slot"), "the innermost formula appears");

            // [[[[...] f] f] f]: cons-formula heads exercise the
            // tall-cell merge, every level splicing `[ ` into line one.
            let mut n = noun![8 [1 0] 4 0 6];
            for _ in 0..2_000 {
                n = Noun::cell(n, noun![0 1]);
            }
            let text = nasm_from_jam(&jam(&n)).expect("cues and renders");
            let first = text.lines().next().expect("nonempty");
            assert!(first.starts_with("[ [ [ "), "merged cell openers");
        })
        .expect("thread spawns");
    handle
        .join()
        .expect("no stack overflow rendering deep formulas");
}

#[test]
fn beyond_the_bound_is_a_clean_error() {
    let src = nested(nockasm::parse::MAX_DEPTH + 1);
    let handle = std::thread::Builder::new()
        .stack_size(TEST_STACK)
        .spawn(move || match expand(&src) {
            Err(Error::Parse(e)) => {
                assert!(
                    matches!(e.kind, ParseErrorKind::TooDeep),
                    "got {:?}",
                    e.kind
                )
            }
            other => panic!("expected TooDeep, got {other:?}"),
        })
        .expect("thread spawns");
    handle.join().expect("no stack overflow past the bound");
}
