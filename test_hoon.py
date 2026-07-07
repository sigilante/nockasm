"""Differential test: desk/lib/nockasm.hoon against nockasm.py (the oracle).

Builds a single Hoon expression that wraps the library core, expands every
test source with it, and compares each result against the noun produced by
the Python expander. Runs via `urbit eval` (no ship needed).

    python test_hoon.py

Set URBIT_BIN to point at a vere binary if the default is wrong.
"""

import os
import subprocess
import sys

from nockasm import expand_to_noun, parse, render

HERE = os.path.dirname(os.path.abspath(__file__))
LIB = os.path.join(HERE, 'desk', 'lib', 'nockasm.hoon')
VERE = os.environ.get(
    'URBIT_BIN',
    '/Users/neal/urbit/vere/zig-out/aarch64-macos-none/urbit',
)


# ----------------------------------------------------------------------
# Hoon literal rendering
# ----------------------------------------------------------------------

def hoon_cord(s: str) -> str:
    """Render a Python string as a Hoon cord literal ('...')."""
    out = []
    for ch in s:
        o = ord(ch)
        if ch == '\\':
            out.append('\\\\')
        elif ch == "'":
            out.append("\\'")
        elif 32 <= o < 127:
            out.append(ch)
        else:
            out.append('\\%02x' % o)   # \0a etc. (hex escape)
    return "'" + ''.join(out) + "'"


def hoon_atom(n: int) -> str:
    """Render an int as a Hoon @ud literal (dots every 3 digits)."""
    s = str(n)
    groups = []
    while len(s) > 3:
        groups.insert(0, s[-3:])
        s = s[:-3]
    groups.insert(0, s)
    return '.'.join(groups)


def hoon_noun(n) -> str:
    """Render a nockasm noun (int | (h, t)) as a Hoon noun literal."""
    if isinstance(n, int):
        return hoon_atom(n)
    elems = []
    while isinstance(n, tuple):
        elems.append(n[0])
        n = n[1]
    elems.append(n)
    return '[' + ' '.join(hoon_noun(e) for e in elems) + ']'


# ----------------------------------------------------------------------
# Test corpus: every source the Python suite exercises, plus benchmarks
# ----------------------------------------------------------------------

GOOD = [
    # atom literals
    ('dec', '42'),
    ('dec-sep', '1.000'),
    ('hex', '0x2a'),
    ('hex-sep', '0x1.0000'),
    ('cord', "'fast'"),
    # named opcodes
    ('slot', '(%slot 1)'),
    ('const', '(%const 42)'),
    ('inc', '(%inc (%slot 1))'),
    ('eq', '(%eq (%slot 2) (%slot 3))'),
    ('if', '(%if (%slot 1) 0 1)'),
    ('eval', '(%eval (%slot 1) (%const 42))'),
    ('isa', '(%isa (%slot 1))'),
    ('comp', '(%comp (%slot 1) (%inc (%slot 1)))'),
    ('push', '(%push (%const 42) (%slot 1))'),
    ('call', '(%call 2 (%slot 1))'),
    ('edit', '(%edit 6 (%inc (%slot 1)) (%slot 1))'),
    ('hint', "(%hint 'fast' (%slot 1))"),
    ('hintd', "(%hintd 'fast' 0 (%slot 1))"),
    ('aliases', '[(%self) (%battery) (%payload) (%sample) (%context) (%crash)]'),
    ('arm', '(%arm (%if (%slot 1) 0 1))'),
    # raw cells
    ('raw2', '[4 0 1]'),
    ('raw-nested', '[8 [1 0] 4 0 6]'),
    ('raw-mixed', '[4 (%slot 1)]'),
    # schemas
    ('sch-single', ':subject .x  .x'),
    ('sch-pair-h', ':subject {.x .y}  .x'),
    ('sch-pair-t', ':subject {.x .y}  .y'),
    ('sch-3a', ':subject {.a .b .c}  .a'),
    ('sch-3b', ':subject {.a .b .c}  .b'),
    ('sch-3c', ':subject {.a .b .c}  .c'),
    ('sch-4d', ':subject {.a .b .c .d}  .d'),
    ('sch-nest-a', ':subject {{.a .b} .c}  .a'),
    ('sch-nest-b', ':subject {{.a .b} .c}  .b'),
    ('sch-nest-c', ':subject {{.a .b} .c}  .c'),
    ('sch-op', ':subject {.x .y} (%eq .x .y)'),
    # #let
    ('let-single', ':subject .x  #let .d = (%inc .x) in (%eq .d .x)'),
    ('let-pair', ':subject {.x .y}  #let .d = (%inc .x) in (%eq .d .y)'),
    ('let-nested',
     ':subject .x  #let .a = (%inc .x) in #let .b = (%inc .a) in (%eq .a .b)'),
    ('let-literal', ':subject {.a .b}  #let .v = 10 in .v'),
    # #match
    ('match-basic',
     ':subject {.tag .data}  #match .tag { 1 => (%inc .data)  _ => 0 }'),
    ('match-multi',
     ':subject .tag  #match .tag { 1 => 10  2 => 20  _ => 0 }'),
    # comments and whitespace
    ('comments', '\n  ; a comment\n  (%inc (%slot 1))  ; trailing\n'),
    # worked example
    ('worked', """
:subject {.before .target .after}
#let .next = (%inc .target) in
  [.before .next .after]
"""),
    # distribution
    ('distribution', ':subject {.a .b}  [(%inc .a) (%inc .b)]'),
]

BAD = [
    ('unbound-axis', '(%inc .x)'),
    ('unknown-opcode', '(%nope 1)'),
    ('wrong-arity', '(%inc 1 2)'),
    ('match-no-default', ':subject .x #match .x { 1 => 0 }'),
    ('match-dup-default', ':subject .x #match .x { _ => 0 _ => 1 }'),
    ('trailing-tokens', '42 42'),
    ('let-shadow', ':subject .x #let .x = 1 in .x'),
    ('dup-schema', ':subject {.a .a} .a'),
    ('raw-cell-one', '[42]'),
    ('slot-cell-axis', '(%slot [1 2])'),
    ('empty', '   ; nothing here\n'),
]


def benchmark_cases():
    bdir = os.path.join(HERE, 'benchmarks')
    cases = []
    for name in sorted(os.listdir(bdir)):
        if name.endswith('.nasm'):
            with open(os.path.join(bdir, name)) as f:
                cases.append((name[:-5], f.read()))
    return cases


# ----------------------------------------------------------------------
# Harness assembly
# ----------------------------------------------------------------------

def build_eval_input() -> str:
    good = GOOD + benchmark_cases()
    case_lines = []
    for name, src in good:
        want = hoon_noun(expand_to_noun(src))
        wren = hoon_cord(render(*parse(src)))
        case_lines.append(
            f'      [{hoon_cord(name)} {hoon_cord(src)} {want} {wren}]')
    bad_lines = [
        f'      [{hoon_cord(name)} {hoon_cord(src)}]' for name, src in BAD
    ]
    with open(LIB) as f:
        lib = f.read()
    harness = f"""
=/  cases=(list [name=@t src=@t want=* wren=@t])
  :~
{chr(10).join(case_lines)}
  ==
=/  bads=(list [name=@t src=@t])
  :~
{chr(10).join(bad_lines)}
  ==
=/  fails=(list [@t @t])
  ;:  weld
    %+  murn  cases
    |=  [name=@t src=@t want=* wren=@t]
    ^-  (unit [@t @t])
    =/  res  (mule |.((expand src)))
    ?:  ?=(%| -.res)  `[name 'crashed']
    ?.  =(p.res want)  `[name 'mismatch']
    =/  ren  (mule |.((render (parse src))))
    ?:  ?=(%| -.ren)  `[name 'render-crashed']
    ?.  =(p.ren wren)  `[name 'render-mismatch']
    =/  rtr  (mule |.((expand p.ren)))
    ?:  ?=(%| -.rtr)  `[name 'roundtrip-crashed']
    ?.  =(p.rtr want)  `[name 'roundtrip-mismatch']
    ~
  ::
    %+  murn  bads
    |=  [name=@t src=@t]
    ^-  (unit [@t @t])
    =/  res  (mule |.((expand src)))
    ?:  ?=(%| -.res)  ~
    `[name 'did-not-crash']
  ==
?~  fails
  [%nockasm-all-ok (lent cases) (lent bads)]
[%failures fails]
"""
    return '=>\n' + lib + '\n' + harness


def main():
    src = build_eval_input()
    proc = subprocess.run(
        [VERE, 'eval'], input=src, capture_output=True, text=True,
        timeout=600,
    )
    out = proc.stdout + proc.stderr
    if 'nockasm-all-ok' in out:
        n_good = len(GOOD) + len(benchmark_cases())
        print(f'ok: {n_good} expansion cases match the python oracle '
              f'(expand + render parity + round-trip), '
              f'{len(BAD)} error cases crash as expected')
        return 0
    print('FAIL: hoon output did not match the oracle')
    print(out[-4000:])
    return 1


if __name__ == '__main__':
    sys.exit(main())
