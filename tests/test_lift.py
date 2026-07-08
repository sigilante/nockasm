"""Lift and jamfile properties.

    lower(None, lift(f)) == f                     (soundness, every noun)
    expand_to_noun(render(None, lift(f))) == f    (through text)
    cue(jam(n)) == n                              (serialization)

Corpus: every formula the differential suite expands, plus handmade
nouns that exercise the fallback paths.
"""

# _testkit first: importing it puts the repo root on sys.path (see there).
from _testkit import Tally
from nockasm import (cell, cue, expand_to_noun, jam, lift, lower,
                     nasm_from_jam, render)
from test_hoon import GOOD, benchmark_cases

_t = Tally('lift')
check = _t.check
section = _t.section


# ----------------------------------------------------------------------
section("jam/cue vectors")
# ----------------------------------------------------------------------

check("jam 0", jam(0), 2)
check("jam 1", jam(1), 12)
check("jam [1 1] (backref-eligible atom)", jam((1, 1)), 817)
check("cue inverts jam 0", cue(jam(0)), 0)
for name, n in [
    ("atom", 42),
    ("pair", (1, 2)),
    ("deep", cell(1, 2, 3, 4, 5)),
    ("shared subtree", ((cell(1, 2, 3), cell(1, 2, 3)),
                        cell(1, 2, 3))),
    ("big atom", 2 ** 200 + 12345),
]:
    check(f"cue(jam) round-trip: {name}", cue(jam(n)), n)


# ----------------------------------------------------------------------
section("lift soundness over the corpus")
# ----------------------------------------------------------------------

for name, src in GOOD + benchmark_cases():
    f = expand_to_noun(src)
    ast = lift(f)
    low = lower(None, ast)
    if low != f:
        check(f"{name}: lower(lift)", low, f)
        continue
    rt = expand_to_noun(render(None, ast))
    check(name, rt, f)


# ----------------------------------------------------------------------
section("fallback paths (data that is not a formula)")
# ----------------------------------------------------------------------

CASES = [
    ("atom in formula position stays raw", (2, (5, 6))),
    ("opcode head above 11", (12, 3)),
    ("slot of a cell", (0, (2, 3))),
    ("bare atom", 42),
    ("const of deep data", (1, cell(9, 9, 9, 9))),
    ("cons-formula", ((4, (0, 2)), (4, (0, 3)))),
    ("hint static", (11, (0x74736166, (0, 1)))),
    ("hint dynamic", (11, ((0x74736166, (1, 0)), (0, 1)))),
    ("malformed if", (6, ((1, 0), 5))),
    ("call with cell axis", (9, ((2, 3), (0, 1)))),
]
for name, f in CASES:
    ast = lift(f)
    low = lower(None, ast)
    if low != f:
        check(f"{name}: lower(lift)", low, f)
        continue
    check(name, expand_to_noun(render(None, ast)), f)


# ----------------------------------------------------------------------
section("nasm_from_jam")
# ----------------------------------------------------------------------

f = expand_to_noun("(%inc (%slot 1))")
data = jam(f).to_bytes((jam(f).bit_length() + 7) // 8, 'little')
check("jamfile to nasm", nasm_from_jam(data), "(%inc (%slot 1))\n")
check("jamfile round-trips", expand_to_noun(nasm_from_jam(data)), f)

_t.done()
