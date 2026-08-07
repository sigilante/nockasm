//! Unit corpus: a port of `tests/test_nockasm.py` (the conformance
//! oracle's own unit suite), expected values included. Cross-
//! implementation byte parity is enforced separately by
//! `tests/test_rust.py` at the repository root.

use nockasm::{expand, peg, Atom, Error, LowerError, ParseErrorKind};

fn flat(src: &str) -> String {
    expand(src)
        .unwrap_or_else(|e| panic!("{src:?}: {e}"))
        .to_string()
}

#[test]
fn peg_arithmetic() {
    let a = |n: u64| Atom::from(n);
    assert_eq!(peg(&a(3), &a(1)), Some(a(3)));
    assert_eq!(peg(&a(3), &a(2)), Some(a(6)));
    assert_eq!(peg(&a(3), &a(3)), Some(a(7)));
    assert_eq!(peg(&a(3), &a(6)), Some(a(14)));
    assert_eq!(peg(&a(3), &a(7)), Some(a(15)));
    assert_eq!(peg(&a(2), &a(5)), Some(a(9)));
}

#[test]
fn cord_packing() {
    assert_eq!(Atom::from_cord("fast"), Atom::from(0x7473_6166u64));
    assert_eq!(Atom::from_cord(""), Atom::ZERO);
    assert_eq!(Atom::from_cord("a"), Atom::from(97u64));
}

#[test]
fn atom_literals() {
    assert_eq!(flat("42"), "42");
    assert_eq!(flat("1.000"), "1000");
    assert_eq!(flat("0x2a"), "42");
    assert_eq!(flat("0x1.0000"), "65536");
    assert_eq!(flat("'fast'"), 0x7473_6166u64.to_string());
}

#[test]
fn named_opcodes() {
    assert_eq!(flat("(%slot 1)"), "[0 1]");
    assert_eq!(flat("(%const 42)"), "[1 42]");
    assert_eq!(flat("(%inc (%slot 1))"), "[4 0 1]");
    assert_eq!(flat("(%eq (%slot 2) (%slot 3))"), "[5 [0 2] 0 3]");
    assert_eq!(flat("(%if (%slot 1) 0 1)"), "[6 [0 1] [1 0] 1 1]");
    assert_eq!(flat("(%eval (%slot 1) (%const 42))"), "[2 [0 1] 1 42]");
    assert_eq!(flat("(%isa (%slot 1))"), "[3 0 1]");
    assert_eq!(
        flat("(%comp (%slot 1) (%inc (%slot 1)))"),
        "[7 [0 1] 4 0 1]"
    );
    assert_eq!(flat("(%push (%const 42) (%slot 1))"), "[8 [1 42] 0 1]");
    assert_eq!(flat("(%call 2 (%slot 1))"), "[9 2 0 1]");
    assert_eq!(
        flat("(%edit 6 (%inc (%slot 1)) (%slot 1))"),
        "[10 [6 4 0 1] 0 1]"
    );
    assert_eq!(flat("(%hint 'fast' (%slot 1))"), "[11 1953718630 0 1]");
    // The clue is a formula position: bare 0 lifts to [1 0].
    assert_eq!(
        flat("(%hintd 'fast' 0 (%slot 1))"),
        "[11 [1953718630 1 0] 0 1]"
    );
    assert_eq!(flat("(%scry (%const 138) (%slot 1))"), "[12 [1 138] 0 1]");
    assert_eq!(flat("(%scry 138 0)"), "[12 [1 138] 1 0]");
    assert_eq!(
        flat("[(%self) (%battery) (%payload) (%sample) (%context) (%crash)]"),
        "[[0 1] [0 2] [0 3] [0 6] [0 7] 0 0]"
    );
}

#[test]
fn raw_cells() {
    assert_eq!(flat("[4 0 1]"), "[4 0 1]");
    assert_eq!(flat("[8 [1 0] 4 0 6]"), "[8 [1 0] 4 0 6]");
    assert_eq!(flat("[4 (%slot 1)]"), "[4 0 1]");
}

#[test]
fn axis_schemas() {
    assert_eq!(flat(":subject .x  .x"), "[0 1]");
    assert_eq!(flat(":subject {.x .y}  .x"), "[0 2]");
    assert_eq!(flat(":subject {.x .y}  .y"), "[0 3]");
    assert_eq!(flat(":subject {.a .b .c}  .a"), "[0 2]");
    assert_eq!(flat(":subject {.a .b .c}  .b"), "[0 6]");
    assert_eq!(flat(":subject {.a .b .c}  .c"), "[0 7]");
    assert_eq!(flat(":subject {.a .b .c .d}  .d"), "[0 15]");
    assert_eq!(flat(":subject {{.a .b} .c}  .a"), "[0 4]");
    assert_eq!(flat(":subject {{.a .b} .c}  .b"), "[0 5]");
    assert_eq!(flat(":subject {{.a .b} .c}  .c"), "[0 3]");
    assert_eq!(flat(":subject {.x .y} (%eq .x .y)"), "[5 [0 2] 0 3]");
}

#[test]
fn let_form() {
    assert_eq!(
        flat(":subject .x  #let .d = (%inc .x) in (%eq .d .x)"),
        "[8 [4 0 1] 5 [0 2] 0 3]"
    );
    // body axes: d=2, x=peg(3,2)=6, y=peg(3,3)=7
    assert_eq!(
        flat(":subject {.x .y}  #let .d = (%inc .x) in (%eq .d .y)"),
        "[8 [4 0 2] 5 [0 2] 0 7]"
    );
    assert_eq!(
        flat(":subject .x  #let .a = (%inc .x) in #let .b = (%inc .a) in (%eq .a .b)"),
        "[8 [4 0 1] 8 [4 0 2] 5 [0 6] 0 2]"
    );
}

#[test]
fn match_form() {
    assert_eq!(
        flat(":subject {.tag .data}  #match .tag { 1 => (%inc .data)  _ => 0 }"),
        "[8 [0 2] 6 [5 [1 1] 0 2] [4 0 7] 1 0]"
    );
    assert_eq!(
        flat(":subject .tag  #match .tag { 1 => 10  2 => 20  _ => 0 }"),
        "[8 [0 1] 6 [5 [1 1] 0 2] [1 10] 6 [5 [1 2] 0 2] [1 20] 1 0]"
    );
}

#[test]
fn nock_embeds() {
    // Identity: (%nock F) expands to exactly F — no recursion, no
    // validation, no rewriting. Hint-laden and intentionally-partial
    // payloads must survive untouched.
    assert_eq!(flat("(%nock 42)"), "42");
    assert_eq!(flat("(%nock [4 0 1])"), "[4 0 1]");
    assert_eq!(flat("(%nock [11 'fast' 0 1])"), "[11 1953718630 0 1]");
    assert_eq!(flat("(%nock [6 1 2])"), "[6 1 2]");
    assert_eq!(flat("(%nock [8 [1 0] 4 0 6])"), "[8 [1 0] 4 0 6]");
    // In an argument position the enclosing op's kind applies to the
    // result, as for any expression: a cell passes through, an atom
    // lifts, and an atom payload is accepted in axis position.
    assert_eq!(
        flat("(%comp (%nock [0 1]) (%inc (%self)))"),
        "[7 [0 1] 4 0 1]"
    );
    assert_eq!(flat("(%inc (%nock 55))"), "[4 1 55]");
    assert_eq!(flat("(%slot (%nock 3))"), "[0 3]");
}

#[test]
fn anonymous_schema_positions() {
    // '_' is structure with no name bound; generated schemas depend
    // on it, and holes repeat freely.
    assert_eq!(flat(":subject {.a _}  .a"), "[0 2]");
    assert_eq!(flat(":subject {_ .b}  .b"), "[0 3]");
    assert_eq!(flat(":subject {_ .b _}  .b"), "[0 6]");
    assert_eq!(flat(":subject {{.a _} .c}  (%eq .a .c)"), "[5 [0 4] 0 3]");
}

#[test]
fn schema_constructed_as_data() {
    // The typed IR is the contract: a schema built directly as data
    // (no text) must behave exactly like its text-parsed equivalent —
    // emitters project layout trees into Schema values mechanically.
    use nockasm::{lower, Name, Nasm, Op, Schema};
    let schema = Schema::Pair(
        Box::new(Schema::Leaf(Name::new("a").unwrap())),
        Box::new(Schema::Hole),
    );
    let ast = Nasm::Let {
        name: Name::new("d").unwrap(),
        value: Box::new(Nasm::Op(Op::Inc(Box::new(Nasm::Axis(
            Name::new("a").unwrap(),
        ))))),
        body: Box::new(Nasm::Axis(Name::new("d").unwrap())),
    };
    let direct = lower(Some(&schema), &ast).expect("lowers");
    let via_text = expand(":subject {.a _} #let .d = (%inc .a) in .d").expect("expands");
    assert_eq!(direct, via_text);
}

#[test]
fn comments() {
    assert_eq!(
        flat("\n  ; this is a comment\n  (%inc (%slot 1))  ; and so is this\n"),
        "[4 0 1]"
    );
}

#[test]
fn pretty_mode() {
    assert_eq!(
        expand("(%inc (%slot 1))").unwrap().pretty().to_string(),
        "[4 [0 1]]"
    );
    assert_eq!(
        expand("(%eq (%slot 2) (%slot 3))")
            .unwrap()
            .pretty()
            .to_string(),
        "[5 [[0 2] [0 3]]]"
    );
}

#[test]
fn worked_example() {
    let src = "
:subject {.before .target .after}
#let .next = (%inc .target) in
  [.before .next .after]
";
    assert_eq!(flat(src), "[8 [4 0 6] [0 6] [0 2] 0 15]");
}

/// The shared negative corpus (`test_hoon.BAD`): all four
/// implementations must reject exactly these sources. Here the expected
/// error kind is pinned as well.
#[test]
fn errors() {
    use ParseErrorKind as K;
    let parse_err = |src: &str, want: fn(&K) -> bool| match expand(src) {
        Err(Error::Parse(e)) => assert!(want(&e.kind), "{src:?}: got {:?}", e.kind),
        other => panic!("{src:?}: expected parse error, got {other:?}"),
    };
    let lower_err = |src: &str, want: fn(&LowerError) -> bool| match expand(src) {
        Err(Error::Lower(e)) => assert!(want(&e), "{src:?}: got {e:?}"),
        other => panic!("{src:?}: expected lower error, got {other:?}"),
    };

    lower_err("(%inc .x)", |e| matches!(e, LowerError::UnboundAxis { .. }));
    parse_err(
        "(%nope 1)",
        |k| matches!(k, K::UnknownOpcode(n) if n == "nope"),
    );
    parse_err("(%inc 1 2)", |k| {
        matches!(
            k,
            K::OpArity {
                op: "inc",
                want: 1,
                got: 2
            }
        )
    });
    parse_err("(%scry 1)", |k| {
        matches!(
            k,
            K::OpArity {
                op: "scry",
                want: 2,
                got: 1
            }
        )
    });
    parse_err(":subject .x #match .x { 1 => 0 }", |k| {
        matches!(k, K::MatchNeedsDefault)
    });
    parse_err(":subject .x #match .x { _ => 0 _ => 1 }", |k| {
        matches!(k, K::MatchDuplicateDefault)
    });
    parse_err("42 42", |k| matches!(k, K::TrailingTokens { .. }));
    lower_err(
        ":subject .x #let .x = 1 in .x",
        |e| matches!(e, LowerError::LetShadows(n) if n.as_str() == "x"),
    );
    lower_err(
        ":subject {.a .a} .a",
        |e| matches!(e, LowerError::DuplicateSchemaName(n) if n.as_str() == "a"),
    );
    parse_err("[42]", |k| matches!(k, K::RawCellTooFew));
    parse_err("(%slot [1 2])", |k| {
        matches!(k, K::AxisArgNotAtom { op: "slot" })
    });
    parse_err("   ; nothing here\n", |k| {
        matches!(k, K::UnexpectedEof { .. })
    });
    // (%nock ...) payloads are noun literals, exactly one, >= 2 cell
    // elements — never expressions.
    parse_err("(%nock)", |k| matches!(k, K::UnexpectedToken { .. }));
    parse_err("(%nock 1 2)", |k| matches!(k, K::UnexpectedToken { .. }));
    parse_err("(%nock (%inc (%self)))", |k| {
        matches!(k, K::UnexpectedToken { .. })
    });
    parse_err("(%nock .a)", |k| matches!(k, K::UnexpectedToken { .. }));
    parse_err("(%nock [5])", |k| matches!(k, K::NockPayloadTooFew));
    parse_err("(%slot (%nock [1 2]))", |k| {
        matches!(k, K::AxisArgNotAtom { op: "slot" })
    });
}

#[test]
fn depth_limit_is_generous_but_finite() {
    let deep_ok = format!("{}42{}", "[1 ".repeat(300), "]".repeat(300));
    assert!(expand(&deep_ok).is_ok());
    let too_deep = format!("{}42{}", "[1 ".repeat(2000), "]".repeat(2000));
    match expand(&too_deep) {
        Err(Error::Parse(e)) => assert!(matches!(e.kind, ParseErrorKind::TooDeep)),
        other => panic!("expected TooDeep, got {other:?}"),
    }
}
