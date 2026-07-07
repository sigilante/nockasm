# Nock Assembly

![](./img/hero.jpg)

Nock Assembly is a thin macro over [Nock ISA](https://nock.is) designed to make the language more legible for pedagogical purposes.

## Design

| | |
|---|---|
| Named opcodes        | `(%inc .x)` instead of `[4 0 2]`. Pure lexical. |
| Axis schemas         | `:subject {.a .b .c}` resolves `.a` `.b` `.c` to axes 2, 6, 7. Right-leaning by Hoon convention. |
| `#let .name = E in B`| Opcode-8 push. Tracks subject shift via `+peg(3, n)` so old names still resolve in body. |
| `#match E { ... }`   | Scrutinee lifted once via opcode 8. Nested opcode-6 dispatch on literal patterns. Required `_ =>` default. |
| `; comments`         | And whitespace. |

## Install / use

```bash
pip install nockasm
```

```python
from nockasm import expand
print(expand("(%inc (%self))"))
# [4 0 1]

print(expand("""
:subject {.tag .data}
#match .tag {
  1 => (%inc .data)
  2 => .data
  _ => 0
}
"""))
# [8 [0 2] 6 [5 [1 1] 0 2] [4 0 7] 6 [5 [1 2] 0 2] [0 7] 1 0]
```

End-to-end with [`pinochle`](https://github.com/sigilante/pinochle):

```python
from pinochle import nock, parse_noun
from nockasm import expand

src = """
:subject {.before .target .after}
#let .next = (%inc .target) in
  [.before .next .after]
"""

formula = parse_noun(expand(src))
result  = nock(parse_noun("[10 41 99]"), formula)
# result == [10 42 99]
```

At the CLI:

```bash
python -m nockasm program.nasm           # canonical flat
python -m nockasm --pretty program.nasm  # explicit binary cells
echo "(%inc (%self))" | python -m nockasm
python -m nockasm --from-jam formula.jam # jammed formula -> .nasm source
```

`--from-jam` cues a jamfile and lifts the noun back to canonical
Nockasm: named ops wherever Nock's positional grammar proves the
shape, structural raw cells everywhere else, no guessed intent.
Sound by construction: the emitted source re-expands to the exact
jammed noun.

## Integration with the Nock kernel

Pinochle ships `nock-kernel` for Jupyter (`Nock 4K` kernel). It accepts
canonical Nock in `:formula` cells. Workflow today:

1. Write `.nasm` in a regular Python cell (or text editor).
2. Run `expand(src)` in a Python notebook to get canonical Nock.
3. Paste the result into a `:formula` cell in a Nock notebook.

A `:asm` cell magic for the Nock kernel that does this in one step is the
obvious next step. Roughly:

```python
# in pinochle/packages/nock_kernel/kernel.py
if cell.startswith(':asm'):
    from nockasm import expand
    formula_src = expand(cell[len(':asm'):])
    # then dispatch as if user had typed ':formula <formula_src>'
```

## Hoon library

`desk/lib/nockasm.hoon` is a port of the expander to Hoon: `.nasm` source
in as a cord, Nock formula out as a noun. The parser is written in the
`++rule` combinator idiom; errors crash with tagged traces
(`%unbound-axis`, `%unknown-opcode`, `%let-shadows`, …).

```
> =nasm -build-file %/lib/nockasm/hoon
> (expand:nasm '(%inc (%self))')
[4 0 1]
> .*([10 41 99] (expand:nasm ':subject {.a .b} .b'))
[42 99]
```

The noun is directly usable with `.*` — no text round-trip. The Python
suite is the conformance oracle: `test_hoon.py` expands every unit-test
source and all five benchmarks through both implementations via
`urbit eval` and compares nouns bit-for-bit (no ship required; set
`URBIT_BIN` to your vere binary). `desk/tests/lib/nockasm.hoon` carries
a representative subset for the on-ship test framework:

```
> -test %/tests/lib/nockasm ~
```

Both implementations also expose the parsed AST as a versioned compiler
target IR — `parse` / `lower` / `render`, with `render` byte-identical
across the two and governed by the round-trip law
`expand(render(x)) == lower(x)` — plus `jam` / `cue` / `lift` /
`nasm-from-jam` for reading jammed formulas back to source under the
soundness law `lower(lift(f)) == f`. See `doc/compiler-target.md`.

## Use in a NockApp

The library compiles unmodified under `hoonc` and is registered in the
[typhoon](https://github.com/sigilante/typhoon) registry, so any
[Nockup](https://github.com/nockchain/nockchain/tree/master/crates/nockup)
project can depend on it by name:

```toml
[dependencies]
"sigilante/nockasm" = "latest"
```

Then `/+  nockasm` in your kernel and call `expand:nockasm` (cord in,
formula out), `render:nockasm`, or `nasm-from-jam:nockasm` at runtime —
or precompute formulas at build time via `/#`.

`desk/mar/nasm.hoon` is a clay mark for `.nasm` files (noun form: the
source cord, parallel to `%hoon`), so the same files live in Urbit
desks, hoonc builds (`/*  f  %nasm  /path` loads octs), and text
tooling alike. See `doc/nasm-mark.md`.

## Structural macros

### `#let .name = VALUE in BODY`

Pushes `VALUE` onto the subject via opcode 8 and binds `.name` to axis 2 in
`BODY`. Any axes that were already in scope are shifted rightward via
`+peg(3, axis)`, so the old names still resolve in the body.

```
:subject {.before .target .after}
#let .next = (%inc .target) in
  [.before .next .after]
; -> [8 [4 0 6] [0 6] [0 2] 0 15]
; against [10 41 99] -> [10 42 99]
```

`VALUE` and `BODY` are both formula positions (bare atoms lift). Shadowing
an existing schema name is a compile error.

### `#match EXPR { PAT => BODY ... _ => DEFAULT }`

Pattern match on the value of `EXPR`. The scrutinee is evaluated once via
opcode 8 — i.e. lifted onto the subject — then each `PAT` is compared
against the lifted value via opcode 5 (eq), with opcode 6 (if) dispatching
to the matching `BODY`. The `_ =>` default is required.

```
:subject {.tag .data}
#match .tag {
  1 => (%inc .data)
  2 => .data
  _ => 0
}
; -> [8 [0 2] 6 [5 [1 1] 0 2] [4 0 7] 6 [5 [1 2] 0 2] [0 7] 1 0]
; against [1 41] -> 42
; against [2 41] -> 41
; against [9 41] -> 0
```

`EXPR` and each `BODY` are formula positions. `PAT`s are *noun literals* —
they're compared against the scrutinee's runtime value, not against a
formula. Bare atoms in `PAT` position are not lifted: writing `1 => ...`
matches the atom `1`, not the formula `[1 1]`.

In the body of each arm (and the default), the scrutinee is at axis 2, and
the original schema axes are shifted rightward via `+peg(3, axis)` — same
shift rule as `#let`. That's why `.data` resolves to `[0 7]` (not `[0 3]`)
in the example above.

## What lifts and what doesn't

Bare atoms get lifted to `[1 atom]` in formula positions. Not in noun-literal
positions (`%const` arg, hint tag) or axis positions (`%slot` arg, `%call`
arity arg, etc.). The per-opcode kinds:

| Opcode    | Kinds | Notes |
|-----------|-------|-------|
| `%slot N` | a     | axis literal |
| `%const X`| n     | any noun, no lift |
| `%arm X`  | n     | synonym for `%const`; intent: callable formula |
| `%crash`  | —     | `[0 0]` — Nock crash idiom |
| `%self`   | —     | `[0 1]` — whole subject |
| `%battery`| —     | `[0 2]` — standard core battery |
| `%payload`| —     | `[0 3]` — standard core payload |
| `%sample` | —     | `[0 6]` — standard gate sample |
| `%context`| —     | `[0 7]` — standard gate context |
| `%eval`   | ff    | both formulas |
| `%isa`    | f     | |
| `%inc`    | f     | |
| `%eq`     | ff    | |
| `%if`     | fff   | branches lift |
| `%comp`   | ff    | |
| `%push`   | ff    | |
| `%call N F`| af   | |
| `%edit N V F`| aff | |
| `%hint T F`  | nf  | tag is a noun literal |
| `%hintd T C F` | nff | clue is a formula — per 4K spec it's evaluated |

The intent-marking opcodes (`%arm`, `%crash`, and the axis aliases) all lower
to the same cells as their `%const` / `%slot` equivalents — they exist purely
to surface meaning at the source level. `%arm X` is `%const X` for cases
where `X` is a formula that will later be invoked via `%call`; `%self`
through `%context` name the standard Hoon core/gate axes.

`#let` value and body are formulas. `#match` scrutinee and arm bodies are
formulas. Match *patterns* are noun literals (compared against the
scrutinee's evaluated value).

Raw cells `[...]` are taken structurally: their elements are *not* lifted.
That gives you an escape hatch into raw Nock when you need it, and the
cons-formula distribution pattern works as expected:

```
:subject {.a .b}
[(%inc .a) (%inc .b)]
; -> [[4 0 2] [4 0 3]]
; against [3 5] -> [4 6] via Nock distribution
```

## Tests

```bash
python test_nockasm.py     # unit tests, 55 cases
python test_e2e.py         # end-to-end: expand -> pinochle -> verify, 19 cases
python test_benchmarks.py  # urbit/benchmark equivalents, 5 cases (loaded from disk)
python test_hoon.py        # hoon lib vs python oracle, 48 + 11 cases (urbit eval)
python test_render.py      # target-IR round-trip law + render idempotence
python test_lift.py        # jam/cue vectors + lift soundness, 69 cases
python test_desk.py        # on-ship test arms via urbit eval shim
python test_mark.py        # %nasm clay mark grow/grab round-trips
```

`test_benchmarks.py` reads `benchmarks/tests.json` and `benchmarks/<name>.nasm`
from disk and runs each through pinochle. The five benchmarks present
(`dec`, `add`, `factorial`, `fibonacci`, `ackermann`) are faithful
transcriptions of `urbit/benchmark/desk/bar/<name>.nock` — each `.nasm`
expands to a noun bit-identical to the corresponding `.nock` formula.

## License

MIT.
