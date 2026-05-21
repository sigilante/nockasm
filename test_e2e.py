"""End-to-end: expand Nock Asm -> feed pinochle -> check result.

This is the real test. If pinochle agrees with us, the assembler is sound.
"""

import sys
from pinochle import nock, parse_noun
from nockasm import expand


PASS = 0
FAIL = 0


def run(name, subject_src, formula_src, want):
    """Parse subject; expand formula assembly; run on pinochle; check."""
    global PASS, FAIL
    formula_str = expand(formula_src)
    subject = parse_noun(subject_src)
    formula = parse_noun(formula_str)
    try:
        got = nock(subject, formula)
    except Exception as e:
        FAIL += 1
        print(f"  FAIL {name}: pinochle raised {type(e).__name__}: {e}")
        print(f"       formula asm:   {formula_src.strip()}")
        print(f"       formula nock:  {formula_str}")
        return
    if got == want:
        PASS += 1
        print(f"  ok   {name}: {got}")
    else:
        FAIL += 1
        print(f"  FAIL {name}: got {got}, want {want}")
        print(f"       formula asm:   {formula_src.strip()}")
        print(f"       formula nock:  {formula_str}")


def section(s): print(f"\n== {s} ==")


# ----------------------------------------------------------------------
section("basic opcodes")
# ----------------------------------------------------------------------

run("inc 41", "41", "(%inc (%slot 1))", 42)
run("slot whole", "42", "(%slot 1)", 42)
run("const 99 ignoring subject", "0", "(%const 99)", 99)
run("eq same", "[42 42]", "(%eq (%slot 2) (%slot 3))", 0)
run("eq diff", "[42 43]", "(%eq (%slot 2) (%slot 3))", 1)


# ----------------------------------------------------------------------
section("schema-driven slot access")
# ----------------------------------------------------------------------

# subject [10 20 30]: with schema {.a .b .c}, .a=2, .b=6, .c=7
run(".a from [10 20 30]", "[10 20 30]", ":subject {.a .b .c}  .a", 10)
run(".b from [10 20 30]", "[10 20 30]", ":subject {.a .b .c}  .b", 20)
run(".c from [10 20 30]", "[10 20 30]", ":subject {.a .b .c}  .c", 30)


# ----------------------------------------------------------------------
section("named ops with schema")
# ----------------------------------------------------------------------

run("inc .x",
    "[5 99]",
    ":subject {.x .ignored}  (%inc .x)",
    6)

run("eq .x .y returns 0 when equal",
    "[7 7]",
    ":subject {.x .y}  (%eq .x .y)",
    0)


# ----------------------------------------------------------------------
section("#let — value lifted, body in shifted axes")
# ----------------------------------------------------------------------

# #let .d = (%inc .x) in .d
# subject = [5 0]   schema .x=2
# value: [4 0 2] -> 6
# body axes after let: .d=2, .x=6
# body: .d -> [0 2]
# pinochle should yield 6
run("let-inc, return bound",
    "[5 0]",
    ":subject {.x .y}  #let .d = (%inc .x) in .d",
    6)

# Sanity: the original .x is still accessible after the let.
run("let-inc, return old .x (shifted)",
    "[5 0]",
    ":subject {.x .y}  #let .d = (%inc .x) in .x",
    5)

# Bind a literal — the bare 10 must lift to [1 10].
run("let literal",
    "[0 0]",
    ":subject {.a .b}  #let .v = 10 in .v",
    10)

# Nested lets, demonstrate axis shifting twice.
run("nested let",
    "0",
    ":subject .x  "
    "#let .a = 10 in "
    "#let .b = 20 in "
    "(%inc .a)",
    11)


# ----------------------------------------------------------------------
section("#match — head-tag dispatch")
# ----------------------------------------------------------------------

# subject [tag data]: schema .tag at 2, .data at 3
# match .tag { 1 => (%inc .data)  2 => .data  _ => 0 }
src = (":subject {.tag .data}  "
       "#match .tag { 1 => (%inc .data)  2 => .data  _ => 0 }")
run("match tag=1", "[1 41]", src, 42)
run("match tag=2", "[2 41]", src, 41)
run("match tag=99 default", "[99 41]", src, 0)


# ----------------------------------------------------------------------
section("worked example: rewrite middle of a 3-cell")
# ----------------------------------------------------------------------

src = """
:subject {.before .target .after}
#let .next = (%inc .target) in
  [.before .next .after]
"""
# In: [10 41 99] -> Out: [10 42 99]
run("inc middle of triple",
    "[10 41 99]",
    src,
    parse_noun("[10 42 99]"))


# ----------------------------------------------------------------------
section("composing raw Nock with macros")
# ----------------------------------------------------------------------

# A cell-headed formula triggers Nock's distribution rule:
#   *[a [b c] d] = [*[a [b c]] *[a d]]
# Build [(%inc .a) (%inc .b)] -> [[4 0 2] [4 0 3]], which against
# subject [3 5] evaluates to [4 6].
run("cons-formula via distribution",
    "[3 5]",
    ":subject {.a .b}  [(%inc .a) (%inc .b)]",
    parse_noun("[4 6]"))


# ----------------------------------------------------------------------
print()
print(f"==> end-to-end: {PASS} passed, {FAIL} failed")
sys.exit(0 if FAIL == 0 else 1)
