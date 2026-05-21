# nockasm

A thin macro expander from Nock Assembly text to canonical Nock 4K. Lowering
is one-pass, deterministic, and bijective-by-construction with the canonical
tree. Output is parseable by `pinochle.parse_noun` and runnable on the
`nock-kernel` Jupyter kernel.

The goal is pedagogical: programmers who already know one language should be
able to read Nock at the formula level without manually decoding axes. This
is the layer between raw `[8 [1 0] [1 4 0 6] 0 1]` and Hoon.

## Design

Five things the macro does. Stop there.

| | |
|---|---|
| Named opcodes        | `(%inc .x)` instead of `[4 0 2]`. Pure lexical. |
| Axis schemas         | `:subject {.a .b .c}` resolves `.a` `.b` `.c` to axes 2, 6, 7. Right-leaning by Hoon convention. |
| `#let .name = E in B`| Opcode-8 push. Tracks subject shift via `+peg(3, n)` so old names still resolve in body. |
| `#match E { ... }`   | Scrutinee lifted once via opcode 8. Nested opcode-6 dispatch on literal patterns. Required `_ =>` default. |
| `; comments`         | And whitespace. |

Anything else (closures, types, comprehensions) is out of scope. Graduate to
Hoon when you need them.

## Canonical spec wins

This module follows Nock 4K. Where its behavior would contradict the spec,
the spec wins. File a bug.

## Install / use

Single file, no dependencies for the expander itself. `pip install pinochle`
to round-trip outputs through the reference interpreter.

```python
from nockasm import expand
print(expand("(%inc (%slot 1))"))
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

End-to-end with pinochle:

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

CLI:

```bash
python -m nockasm program.nasm           # canonical flat
python -m nockasm --pretty program.nasm  # explicit binary cells
echo "(%inc (%slot 1))" | python -m nockasm
```

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

Patch sketch only — not bundled in v0.

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
```

`test_benchmarks.py` reads `benchmarks/tests.json` and `benchmarks/<name>.nasm`
from disk and runs each through pinochle. The five benchmarks present
(`dec`, `add`, `factorial`, `fibonacci`, `ackermann`) are faithful
transcriptions of `urbit/benchmark/desk/bar/<name>.nock` — each `.nasm`
expands to a noun bit-identical to the corresponding `.nock` formula.

## What's deliberately not here

- `#core` with `%fast` jet hints. Cores are non-trivial — the calling
  convention with opcode 9 and the jet-matching rules deserve their own
  pass. For now use the `%hint` opcode directly.
- Closures / lambda lifting. That's Hoon's job.
- Type checking of any kind.
- Multi-form modules. One formula per program. Compose externally.

## License

MIT.
