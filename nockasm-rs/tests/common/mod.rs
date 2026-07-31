//! Shared corpus for the law tests.
//!
//! `GOOD` mirrors `tests/test_hoon.py::GOOD` at the repository root (the
//! canonical corpus definition); `benchmark_cases` reads the same
//! `benchmarks/*.nasm` files from disk. Cross-implementation parity over
//! this corpus is enforced by `tests/test_rust.py` — drift between this
//! copy and the Python list weakens only the Rust-internal law coverage,
//! never parity.

/// (name, source) expansion cases.
pub const GOOD: &[(&str, &str)] = &[
    // atom literals
    ("dec", "42"),
    ("dec-sep", "1.000"),
    ("hex", "0x2a"),
    ("hex-sep", "0x1.0000"),
    ("cord", "'fast'"),
    // named opcodes
    ("slot", "(%slot 1)"),
    ("const", "(%const 42)"),
    ("inc", "(%inc (%slot 1))"),
    ("eq", "(%eq (%slot 2) (%slot 3))"),
    ("if", "(%if (%slot 1) 0 1)"),
    ("eval", "(%eval (%slot 1) (%const 42))"),
    ("isa", "(%isa (%slot 1))"),
    ("comp", "(%comp (%slot 1) (%inc (%slot 1)))"),
    ("push", "(%push (%const 42) (%slot 1))"),
    ("call", "(%call 2 (%slot 1))"),
    ("edit", "(%edit 6 (%inc (%slot 1)) (%slot 1))"),
    ("hint", "(%hint 'fast' (%slot 1))"),
    ("hintd", "(%hintd 'fast' 0 (%slot 1))"),
    (
        "aliases",
        "[(%self) (%battery) (%payload) (%sample) (%context) (%crash)]",
    ),
    ("arm", "(%arm (%if (%slot 1) 0 1))"),
    // raw cells
    ("raw2", "[4 0 1]"),
    ("raw-nested", "[8 [1 0] 4 0 6]"),
    ("raw-mixed", "[4 (%slot 1)]"),
    // %nock opaque embeds: identity expansion, payload untouched
    ("nock-atom", "(%nock 42)"),
    ("nock-cell", "(%nock [4 0 1])"),
    ("nock-hint", "(%nock [11 'fast' 0 1])"),
    ("nock-partial", "(%nock [6 1 2])"),
    ("nock-deep", "(%nock [8 [1 0] 4 0 6])"),
    ("nock-in-formula", "(%comp (%nock [0 1]) (%inc (%self)))"),
    ("nock-atom-lifts", "(%inc (%nock 55))"),
    (
        "nock-tall",
        "(%nock [123.456.789.012 987.654.321.098 111.222.333.444 \
         555.666.777.888 999.888.777.666])",
    ),
    // schemas
    ("sch-single", ":subject .x  .x"),
    ("sch-pair-h", ":subject {.x .y}  .x"),
    ("sch-pair-t", ":subject {.x .y}  .y"),
    ("sch-3a", ":subject {.a .b .c}  .a"),
    ("sch-3b", ":subject {.a .b .c}  .b"),
    ("sch-3c", ":subject {.a .b .c}  .c"),
    ("sch-4d", ":subject {.a .b .c .d}  .d"),
    ("sch-nest-a", ":subject {{.a .b} .c}  .a"),
    ("sch-nest-b", ":subject {{.a .b} .c}  .b"),
    ("sch-nest-c", ":subject {{.a .b} .c}  .c"),
    ("sch-op", ":subject {.x .y} (%eq .x .y)"),
    // anonymous positions: structure with no name bound
    ("sch-hole-tail", ":subject {.a _}  .a"),
    ("sch-hole-head", ":subject {_ .b}  .b"),
    ("sch-hole-multi", ":subject {_ .b _}  .b"),
    ("sch-hole-nested", ":subject {{.a _} .c}  (%eq .a .c)"),
    // #let
    ("let-single", ":subject .x  #let .d = (%inc .x) in (%eq .d .x)"),
    ("let-pair", ":subject {.x .y}  #let .d = (%inc .x) in (%eq .d .y)"),
    (
        "let-nested",
        ":subject .x  #let .a = (%inc .x) in #let .b = (%inc .a) in (%eq .a .b)",
    ),
    ("let-literal", ":subject {.a .b}  #let .v = 10 in .v"),
    // #match
    (
        "match-basic",
        ":subject {.tag .data}  #match .tag { 1 => (%inc .data)  _ => 0 }",
    ),
    (
        "match-multi",
        ":subject .tag  #match .tag { 1 => 10  2 => 20  _ => 0 }",
    ),
    // comments and whitespace
    ("comments", "\n  ; a comment\n  (%inc (%slot 1))  ; trailing\n"),
    // worked example
    (
        "worked",
        "\n:subject {.before .target .after}\n#let .next = (%inc .target) in\n  [.before .next .after]\n",
    ),
    // distribution
    ("distribution", ":subject {.a .b}  [(%inc .a) (%inc .b)]"),
];

/// The `benchmarks/*.nasm` transcriptions, read from the repository.
pub fn benchmark_cases() -> Vec<(String, String)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate lives inside the repository")
        .join("benchmarks");
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.expect("readable dir entry").file_name())
        .filter_map(|n| {
            let n = n.to_string_lossy().into_owned();
            n.strip_suffix(".nasm").map(str::to_string)
        })
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let src = std::fs::read_to_string(dir.join(format!("{name}.nasm")))
                .expect("benchmark source reads");
            (name, src)
        })
        .collect()
}

/// The full corpus: `GOOD` plus the benchmarks.
pub fn corpus() -> Vec<(String, String)> {
    GOOD.iter()
        .map(|(n, s)| (n.to_string(), s.to_string()))
        .chain(benchmark_cases())
        .collect()
}
