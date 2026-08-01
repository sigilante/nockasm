//! Pipeline benchmarks, dependency-free: `cargo bench --bench pipeline`.
//!
//! Emits TSV to stdout — `workload <tab> op <tab> ns_per_op <tab> iters`
//! — so cross-implementation drivers can diff the same workloads against
//! the Python reference. Iteration counts are calibrated per op (double
//! until the batch runs ≥ 100 ms), with one warmup call. Pass an
//! argument to filter: `cargo bench --bench pipeline -- jam`.
//!
//! Workloads: the benchmark corpus sources, plus deterministic
//! synthetics (mirrored exactly by the Python driver) — a wide raw cell,
//! a `#let` chain that exercises the peg-shift maps, a deep formula, a
//! balanced noun tree, and a shared-subtree spine that exercises jam's
//! structural backreference table. `deep100k`/`tree16` are Rust-only
//! scale points: the reference cannot reach them (recursion limits, and
//! quadratic bigint bit-appends in its jam).

use std::hint::black_box;
use std::time::Instant;

use nockasm::{cue, expand, jam, lift, nasm_from_jam, parse, Noun, Program};

fn time_ns(mut f: impl FnMut()) -> (f64, u64) {
    f(); // warmup
    let mut iters: u64 = 1;
    loop {
        let start = Instant::now();
        for _ in 0..iters {
            f();
        }
        let elapsed = start.elapsed();
        if elapsed.as_millis() >= 100 || iters >= 1 << 30 {
            return (elapsed.as_nanos() as f64 / iters as f64, iters);
        }
        iters *= 4;
    }
}

fn report(workload: &str, op: &str, filter: Option<&str>, f: impl FnMut()) {
    if let Some(pat) = filter {
        if !workload.contains(pat) && !op.contains(pat) {
            return;
        }
    }
    let (ns, iters) = time_ns(f);
    println!("{workload}\t{op}\t{ns:.1}\t{iters}");
}

fn benchmark_source(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate lives inside the repository")
        .join("benchmarks")
        .join(format!("{name}.nasm"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// `[(%inc (%slot 1)) ... (%inc (%slot 5000))]` — a wide raw cell.
fn wide5k_source() -> String {
    let mut src = String::from("[");
    for i in 1..=5000u32 {
        if i > 1 {
            src.push(' ');
        }
        src.push_str(&format!("(%inc (%slot {i}))"));
    }
    src.push(']');
    src
}

/// 200 nested `#let`s over an 8-name schema: every binder re-pegs the
/// whole environment, so the axis maps grow as the chain deepens.
fn lets200_source() -> String {
    let mut src = String::from(":subject {.a .b .c .d .e .f .g .h}\n");
    for k in 0..200u32 {
        src.push_str(&format!("#let .x{k} = (%inc .a) in\n"));
    }
    src.push_str("[.a .x0 .x199]");
    src
}

/// `[4 [4 ... [0 1]]]` — a formula nested through its tails.
fn deep_formula(depth: usize) -> Noun {
    let mut n = Noun::cell(0u64, 1u64);
    for _ in 0..depth {
        n = Noun::cell(4u64, n);
    }
    n
}

/// A perfect binary tree with distinct leaf atoms `0..2^depth`.
fn balanced_tree(depth: u32) -> Noun {
    let mut nodes: Vec<Noun> = (0..1u64 << depth).map(Noun::from).collect();
    while nodes.len() > 1 {
        nodes = nodes
            .chunks(2)
            .map(|pair| Noun::cell(pair[0].clone(), pair[1].clone()))
            .collect();
    }
    nodes.pop().expect("nonempty tree")
}

/// A right spine of `count` references to one shared formula —
/// structural-dedup heaven for jam.
fn shared_spine(formula: &Noun, count: usize) -> Noun {
    let mut acc = formula.clone();
    for _ in 0..count {
        acc = Noun::cell(formula.clone(), acc);
    }
    acc
}

fn main() {
    // Skip flag-like args: cargo passes `--bench` to harness = false
    // binaries.
    let filter_arg = std::env::args().skip(1).find(|a| !a.starts_with('-'));
    let filter = filter_arg.as_deref();

    // -- source workloads: parse, expand, render -----------------------
    let mut sources: Vec<(String, String)> = ["dec", "fibonacci", "ackermann"]
        .iter()
        .map(|n| (n.to_string(), benchmark_source(n)))
        .collect();
    sources.push(("wide5k".into(), wide5k_source()));
    sources.push(("lets200".into(), lets200_source()));

    for (name, src) in &sources {
        report(name, "parse", filter, || {
            black_box(parse(black_box(src)).expect("parses"));
        });
        report(name, "expand", filter, || {
            black_box(expand(black_box(src)).expect("expands"));
        });
        let program: Program = parse(src).expect("parses");
        report(name, "render", filter, || {
            black_box(black_box(&program).render());
        });
    }

    // -- noun workloads: jam, cue, lift, nasm_from_jam -----------------
    let ack = expand(&benchmark_source("ackermann")).expect("expands");
    let nouns: Vec<(String, Noun)> = vec![
        ("ack-formula".into(), ack.clone()),
        ("deep200".into(), deep_formula(200)),
        ("tree10".into(), balanced_tree(10)),
        ("shared500".into(), shared_spine(&ack, 500)),
        // Rust-only scale points; no reference counterpart.
        ("deep100k".into(), deep_formula(100_000)),
        ("tree16".into(), balanced_tree(16)),
    ];

    for (name, noun) in &nouns {
        report(name, "jam", filter, || {
            black_box(jam(black_box(noun)));
        });
        let bytes = jam(noun);
        report(name, "cue", filter, || {
            black_box(cue(black_box(&bytes)).expect("cues"));
        });
        report(name, "lift", filter, || {
            black_box(lift(black_box(noun)));
        });
        if name == "deep100k" {
            // Tall output is quadratic in size from indentation alone
            // (~10 GB of spaces at this depth): a property of the
            // format, not a renderer measurement. Skipped.
            continue;
        }
        report(name, "nasm_from_jam", filter, || {
            black_box(nasm_from_jam(black_box(&bytes)).expect("lifts"));
        });
    }
}
