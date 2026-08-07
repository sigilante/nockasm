"""Tests for nockasm. Run with: python test_nockasm.py"""

# _testkit first: importing it puts the repo root on sys.path (see there).
from _testkit import Tally
from nockasm import (AxisRef, IntAtom, LetForm, MatchForm, OpApp,
                     cord_to_nat, expand, expand_to_noun, lift, lower,
                     parse, peg, render)
from test_hoon import BAD

_t = Tally()
check = _t.check
section = _t.section


# ----------------------------------------------------------------------
section("peg arithmetic")
# ----------------------------------------------------------------------

check("peg(3, 1)", peg(3, 1), 3)
check("peg(3, 2)", peg(3, 2), 6)
check("peg(3, 3)", peg(3, 3), 7)
check("peg(3, 6)", peg(3, 6), 14)
check("peg(3, 7)", peg(3, 7), 15)
check("peg(2, 5)", peg(2, 5), 9)  # 5=2*2+1 -> 2*peg(2,2)+1 = 2*4+1


# ----------------------------------------------------------------------
section("cord packing")
# ----------------------------------------------------------------------

# 'fast' little-endian:
# f=0x66, a=0x61, s=0x73, t=0x74
# n = 0x66 | 0x61<<8 | 0x73<<16 | 0x74<<24
#   = 0x74736166 = 1953718630
check("cord_to_nat('fast')", cord_to_nat('fast'), 0x74736166)
check("cord_to_nat('')", cord_to_nat(''), 0)
check("cord_to_nat('a')", cord_to_nat('a'), 97)
# non-ASCII packs UTF-8 bytes, matching hoon and rust ('é' = c3 a9,
# not the codepoint 233; 'nöck' = 6e c3 b6 63 6b, little-endian)
check("cord_to_nat('é') is utf-8 bytes", cord_to_nat('é'), 0xa9c3)
check("cord_to_nat('nöck')", cord_to_nat('nöck'), 0x6b63b6c36e)
check("utf-8 cord expands", expand("'nöck'"), str(0x6b63b6c36e))


# ----------------------------------------------------------------------
section("atom literals")
# ----------------------------------------------------------------------

check("decimal", expand("42"), "42")
check("decimal with .", expand("1.000"), "1000")
check("hex", expand("0x2a"), "42")
check("hex with .", expand("0x1.0000"), "65536")
check("cord", expand("'fast'"), str(0x74736166))


# ----------------------------------------------------------------------
section("named opcodes (no schema)")
# ----------------------------------------------------------------------

check("slot 1",        expand("(%slot 1)"),                "[0 1]")
check("const 42",      expand("(%const 42)"),              "[1 42]")
check("inc slot 1",    expand("(%inc (%slot 1))"),         "[4 0 1]")
check("eq",            expand("(%eq (%slot 2) (%slot 3))"),"[5 [0 2] 0 3]")
check("if",            expand("(%if (%slot 1) 0 1)"),      "[6 [0 1] [1 0] 1 1]")
check("eval",          expand("(%eval (%slot 1) (%const 42))"), "[2 [0 1] 1 42]")
check("isa",           expand("(%isa (%slot 1))"),         "[3 0 1]")
check("comp",          expand("(%comp (%slot 1) (%inc (%slot 1)))"),
                                                            "[7 [0 1] 4 0 1]")
check("push",          expand("(%push (%const 42) (%slot 1))"),
                                                            "[8 [1 42] 0 1]")
check("call",          expand("(%call 2 (%slot 1))"),      "[9 2 0 1]")
check("edit",          expand("(%edit 6 (%inc (%slot 1)) (%slot 1))"),
                                                            "[10 [6 4 0 1] 0 1]")
check("hint static",   expand("(%hint 'fast' (%slot 1))"),
                                                            f"[11 {0x74736166} 0 1]")
check("hint dynamic",  expand("(%hintd 'fast' 0 (%slot 1))"),
                                                            f"[11 [{0x74736166} 1 0] 0 1]")
check("scry",         expand("(%scry (%const 138) (%slot 1))"),
                                                            "[12 [1 138] 0 1]")
check("scry lifts both formula arguments", expand("(%scry 138 0)"),
                                                            "[12 [1 138] 1 0]")
# Clue is a formula position; bare 0 lifts to [1 0]. Use (%slot 1) etc.
# if you want the clue to be a real formula evaluated against subject.


# ----------------------------------------------------------------------
section("raw cells")
# ----------------------------------------------------------------------

check("raw 2",         expand("[4 0 1]"),                  "[4 0 1]")
check("raw nested",    expand("[8 [1 0] 4 0 6]"),          "[8 [1 0] 4 0 6]")
check("mix raw/op",    expand("[4 (%slot 1)]"),            "[4 0 1]")


# ----------------------------------------------------------------------
section("axis schemas")
# ----------------------------------------------------------------------

check("single",        expand(":subject .x  .x"),          "[0 1]")
check("pair head",     expand(":subject {.x .y}  .x"),     "[0 2]")
check("pair tail",     expand(":subject {.x .y}  .y"),     "[0 3]")
check("flat 3 a",      expand(":subject {.a .b .c}  .a"),  "[0 2]")
check("flat 3 b",      expand(":subject {.a .b .c}  .b"),  "[0 6]")
check("flat 3 c",      expand(":subject {.a .b .c}  .c"),  "[0 7]")
check("flat 4 d",      expand(":subject {.a .b .c .d}  .d"), "[0 15]")
check("nested left",   expand(":subject {{.a .b} .c}  .a"), "[0 4]")
check("nested left b", expand(":subject {{.a .b} .c}  .b"), "[0 5]")
check("nested left c", expand(":subject {{.a .b} .c}  .c"), "[0 3]")
check("schema in op",  expand(":subject {.x .y} (%eq .x .y)"),
                                                            "[5 [0 2] 0 3]")


# ----------------------------------------------------------------------
section("#let")
# ----------------------------------------------------------------------

check("let single",
      expand(":subject .x  #let .d = (%inc .x) in (%eq .d .x)"),
      "[8 [4 0 1] 5 [0 2] 0 3]")

check("let pair",
      expand(":subject {.x .y}  #let .d = (%inc .x) in (%eq .d .y)"),
      # value: [4 0 2]
      # body axes: d=2, x=peg(3,2)=6, y=peg(3,3)=7
      # body: [5 [0 2] [0 7]]
      "[8 [4 0 2] 5 [0 2] 0 7]")

check("let nested",
      expand(":subject .x  "
             "#let .a = (%inc .x) in "
             "#let .b = (%inc .a) in "
             "(%eq .a .b)"),
      # outer: x at 1
      # after .a let: a=2, x=3.  value = [4 0 1]
      # after .b let: b=2, a=peg(3,2)=6, x=peg(3,3)=7. value = [4 0 2]
      # body: [5 [0 6] [0 2]]
      "[8 [4 0 1] 8 [4 0 2] 5 [0 6] 0 2]")


# ----------------------------------------------------------------------
section("#match")
# ----------------------------------------------------------------------

check("match basic",
      expand(":subject {.tag .data}  "
             "#match .tag { 1 => (%inc .data)  _ => 0 }"),
      # scrutinee compiled in old axes: [0 2]
      # shifted axes for body: tag=peg(3,2)=6, data=peg(3,3)=7
      # case 1: [6 [5 [1 1] [0 2]] [4 0 7] [1 0]]
      # full: [8 [0 2] [6 [5 [1 1] [0 2]] [4 0 7] [1 0]]]
      "[8 [0 2] 6 [5 [1 1] 0 2] [4 0 7] 1 0]")

check("match multi",
      expand(":subject .tag  "
             "#match .tag { 1 => 10  2 => 20  _ => 0 }"),
      # scrutinee: [0 1]
      # shifted: tag=peg(3,1)=3
      # nested: [6 [5 [1 1] [0 2]] [1 10] [6 [5 [1 2] [0 2]] [1 20] [1 0]]]
      # outer: [8 [0 1] above]
      "[8 [0 1] 6 [5 [1 1] 0 2] [1 10] 6 [5 [1 2] 0 2] [1 20] 1 0]")


# ----------------------------------------------------------------------
section("comments")
# ----------------------------------------------------------------------

check("comment only",
      expand("""
        ; this is a comment
        (%inc (%slot 1))  ; and so is this
      """),
      "[4 0 1]")


# ----------------------------------------------------------------------
section("pretty mode")
# ----------------------------------------------------------------------

check("pretty inc",
      expand("(%inc (%slot 1))", pretty=True),
      "[4 [0 1]]")

check("pretty eq",
      expand("(%eq (%slot 2) (%slot 3))", pretty=True),
      "[5 [[0 2] [0 3]]]")


# ----------------------------------------------------------------------
section("a small worked example")
# ----------------------------------------------------------------------

# Increment a cell's second element, using a schema and #let.
src = """
:subject {.before .target .after}
#let .next = (%inc .target) in
  [.before .next .after]
"""
# before=2, target=6, after=7
# value of .next: [4 0 6]
# body axes: next=2, before=peg(3,2)=6, target=peg(3,6)=14, after=peg(3,7)=15
# body: [[0 6] [0 2] [0 15]]
# full: [8 [4 0 6] [[0 6] [0 2] [0 15]]]
check("worked example",
      expand(src),
      "[8 [4 0 6] [0 6] [0 2] 0 15]")


# ----------------------------------------------------------------------
section("%nock embeds")
# ----------------------------------------------------------------------

# identity: (%nock F) expands to exactly F — no recursion, no
# validation, no rewriting. Hint-laden and intentionally-partial
# payloads must survive untouched.
check("nock atom",      expand("(%nock 42)"),               "42")
check("nock cell",      expand("(%nock [4 0 1])"),          "[4 0 1]")
check("nock hint formula",
      expand("(%nock [11 'fast' 0 1])"),
      f"[11 {0x74736166} 0 1]")
check("nock partial formula (not validated)",
      expand("(%nock [6 1 2])"),
      "[6 1 2]")
check("nock deep",      expand("(%nock [8 [1 0] 4 0 6])"),  "[8 [1 0] 4 0 6]")
# in an argument position the enclosing op's kind applies to the
# result, as for any expression: a cell passes through, an atom lifts
check("nock in formula position",
      expand("(%comp (%nock [0 1]) (%inc (%self)))"),
      "[7 [0 1] 4 0 1]")
check("nock atom lifts in formula position",
      expand("(%inc (%nock 55))"),
      "[4 1 55]")

# text round-trip through render/parse
for _src in ["(%nock 42)", "(%nock [4 0 1])", "(%nock [11 'fast' 0 1])"]:
    _s, _a = parse(_src)
    check(f"nock round-trip {_src}",
          expand_to_noun(render(_s, _a)), lower(_s, _a))

# lifter fallback: non-macro-izable formula subtrees come back as
# %nock, and the result still lowers to the input (totality)
check("lift fallback render", render(None, lift((13, 34))),
      "(%nock [13 34])\n")
check("lift fallback atom", render(None, lift(42)), "(%nock 42)\n")
check("lift fallback lowers", lower(None, lift((2, (5, 6)))), (2, (5, 6)))


# ----------------------------------------------------------------------
section("anonymous schema positions")
# ----------------------------------------------------------------------

# '_' is structure with no name bound; generated schemas depend on it
check("hole tail",   expand(":subject {.a _}  .a"),      "[0 2]")
check("hole head",   expand(":subject {_ .b}  .b"),      "[0 3]")
check("hole multi",  expand(":subject {_ .b _}  .b"),    "[0 6]")
check("hole nested", expand(":subject {{.a _} .c}  (%eq .a .c)"),
      "[5 [0 4] 0 3]")

# a schema constructed directly as data (no text) must behave exactly
# like its text-parsed equivalent — downstream compilers construct
# schema values mechanically from a layout tree, never via text
_sch = ('.a', '_')
_ast = LetForm('.d', OpApp('%inc', [AxisRef('.a')]), AxisRef('.d'))
check("data-constructed schema (let)",
      lower(_sch, _ast),
      expand_to_noun(":subject {.a _} #let .d = (%inc .a) in .d"))
_msch = (('.a', '_'), '.c')
_mast = MatchForm(AxisRef('.c'), [(IntAtom(1), AxisRef('.a'))], IntAtom(0))
check("data-constructed schema (match, hole)",
      lower(_msch, _mast),
      expand_to_noun(
          ":subject {{.a _} .c} #match .c { 1 => .a  _ => 0 }"))
check("hole renders as _", render(('.a', '_'), AxisRef('.a')),
      ":subject {.a _}\n.a\n")


# ----------------------------------------------------------------------
section("errors")
# ----------------------------------------------------------------------

# The negative corpus is shared with the Hoon differential suite
# (test_hoon.BAD) so both expanders are held to rejecting exactly the
# same sources. Expected exception types are Python-specific; a case not
# listed here need only raise *something* — which keeps parity automatic
# when a new BAD case is added to the shared corpus.
EXPECTED = {
    'unbound-axis':     NameError,
    'unknown-opcode':   NameError,
    'wrong-arity':      TypeError,
    'scry-wrong-arity': TypeError,
    'slot-cell-axis':   TypeError,
    'match-no-default': SyntaxError,
    'match-dup-default':SyntaxError,
    'trailing-tokens':  SyntaxError,
    'let-shadow':       SyntaxError,
    'dup-schema':       SyntaxError,
    'raw-cell-one':     SyntaxError,
    'empty':            SyntaxError,
    'nock-no-payload':  SyntaxError,
    'nock-two-payloads': SyntaxError,
    'nock-expr-payload': SyntaxError,
    'nock-axis-payload': SyntaxError,
    'nock-cell-one':    SyntaxError,
}
for _name, _src in BAD:
    _t.raises(_name, lambda s=_src: expand(s), EXPECTED.get(_name, Exception))


# ----------------------------------------------------------------------
_t.done()
