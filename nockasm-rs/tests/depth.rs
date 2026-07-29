//! The depth-bound guarantee: source admitted by the parser runs through
//! the *entire* pipeline — lower, render, round trip, lift, jam/cue —
//! without approaching stack exhaustion, even on a 2 MiB thread stack in
//! a debug build. Nesting beyond the bound is a clean `TooDeep` error.
//!
//! This is a regression test for frame-size creep in the recursive
//! stages: `parse`, `lower`, `render`, and `wide` are deliberately
//! structured as slim dispatchers with fat locals pushed into
//! per-construct helpers. If this test overflows, that structure has
//! regressed (or `MAX_DEPTH` has been raised past what the frames
//! afford).

use nockasm::{cue, expand, jam, lift, lower, noun, parse, Error, Nasm, Noun, Op, ParseErrorKind};

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

/// `lift` (and the `Drop` of the IR it produces) runs on an explicit
/// stack: nouns cued from hostile jamfiles can nest arbitrarily deep in
/// any direction, and none of it may touch the call stack. Each case
/// nests half a million levels one way — through formula tails,
/// cons-formula heads, and opcode-1 payload spines — is shape-checked
/// with an iterative walk, and is dropped, all inside a 2 MiB thread.
/// (`lower`/`render` on IR this deep still recurse, by documented
/// contract; only `lift` and teardown are exercised here.)
#[test]
fn lift_survives_arbitrarily_deep_nouns() {
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
            drop(ast);

            // [1 [[[...] 0] 0]] — depth through the structural fallback:
            // an opcode-1 payload whose spine heads nest leftward.
            let mut payload = Noun::cell(0u64, 0u64);
            for _ in 0..DEPTH {
                payload = Noun::cell(payload, 0u64);
            }
            let ast = lift(&Noun::cell(1u64, payload));
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
            drop(ast);
        })
        .expect("thread spawns");
    handle.join().expect("no stack overflow lifting deep nouns");
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
