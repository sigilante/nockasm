# Nockasm as a compiler target

*Spec, v0.3 — 2026-07-07. M0 and M0.5 are implemented; later phases
are design.*

Nockasm today is a one-way street: a human writes `.nasm`, `expand`
produces canonical Nock. This document specs the other direction — a
compiler (concretely: Jock, built in Hoon) *emitting* Nockasm as its
code-generation target, so that legible assembly becomes the build
artifact and raw Nock becomes a derived output.

## 1. Motivation

A compiler that emits raw Nock nouns produces artifacts nobody can
review. A compiler that emits Nockasm produces artifacts that are:

- **auditable** — named opcodes, named axes, provenance comments;
- **diffable** — codegen regressions show up as readable text diffs in
  `.nasm` files checked into a test corpus;
- **teachable** — the pedagogical goal of Nockasm, extended from
  hand-written examples to real compiler output.

Non-goals: Nockasm does not become an optimizer, a type checker, or a
second compiler. It stays a thin, semantics-free macro layer. Every
construct has a fixed lowering equation; the compiler upstream owns
all cleverness.

## 2. Where it plugs in

Jock's pipeline (stage names to be confirmed against `jock-lang`
source):

```
source ─parse→ jock AST ─jype→ typed AST ─mint→ Nock noun
```

The proposal replaces the tail:

```
typed AST ─emit→ $nasm IR ─render→ .nasm text     (artifact)
                     │
                     └─lower→ Nock noun            (execution)
```

`emit` lives in Jock and is Jock's business. `$nasm`, `render`, and
`lower` live in this library and are the contract. Because
`lib/nockasm.hoon` depends only on the Hoon stdlib, Jock (a NockApp
built with hoonc) can vendor it directly into its `hoon/lib` tree.

Two integration levels:

- **L1 — IR target.** Jock's codegen constructs `$nasm` values instead
  of nouns and lowers through this library. Names survive into the
  artifact by construction. This is the core proposal.
- **L2 — macro extensions.** Nockasm grows gate/core/loop macros so
  emitted assembly is compact rather than drowning in
  `%push`/`%call` scaffolding. **Conditional** — see §7.

## 3. The IR contract (M0 — implemented)

The target IR is the existing `$nasm` type, promoted from internal AST
to public interface:

```hoon
+$  nasm
  $%  [%atom p=@]                          ::  literal (never lifted here)
      [%axis p=@t]                         ::  named leg reference
      [%cell p=(list nasm)]                ::  raw cell, structural
      [%op p=@t q=(list nasm)]             ::  (%opcode ...) application
      [%let p=@t q=nasm r=nasm]            ::  #let p = q in r
      [%match p=nasm q=(list mcas) r=nasm] ::  #match with default r
  ==
+$  mcas  [p=nasm q=nasm]                  ::  one #match case
```

Public arms (`lib/nockasm.hoon`), with Python equivalents in
`nockasm.py` (`parse`, `lower`, `render`, `NASM_VERSION`):

```hoon
++  nasm-version  ::  1; version of the node set, lowering equations,
                  ::  and rendering rules. append-only.
++  parse   ::  @t -> [sch=(unit sema) ast=nasm]
++  lower   ::  [sch=(unit sema) ast=nasm] -> *   IR to Nock
++  render  ::  [sch=(unit sema) ast=nasm] -> @t  IR to canonical text
```

**The round-trip law.** For every well-formed `[sch ast]`:

```
(expand (render sch ast))  ===  (lower sch ast)      (bit-identical)
```

Checked by `tests/test_render.py` (round-trip + idempotence through parse +
76-column invariant, over the full corpus including the benchmark
transcriptions) and by `tests/test_hoon.py` (Hoon `render` output is
**byte-identical** to the Python renderer's, and the Hoon round-trip
matches, for every corpus case).

**Emitter obligations.** The target stays strict; the compiler adapts:

1. *No shadowing.* `#let` of a bound name is a compile error by
   design. Emitters α-rename (`x`, `x-1`, …); compiler temporaries use
   a reserved convention (`.t0`, `.t1`, …).
2. *Names* match `[a-z][a-z0-9-]*` (the grammar allows more; hoon-term
   discipline keeps artifacts portable).
3. *Every `#match` carries a default.* Non-exhaustive source matches
   lower with `_ => (%crash)`.
4. *Axis arguments are atoms.* Symbolic axes go through `.name`; an
   emitter never writes a literal axis for anything that has a
   source-level name — the library's peg-shift machinery owns that
   bookkeeping.

## 4. Canonical rendering (v1, normative)

`render` is deterministic so `.nasm` artifacts diff cleanly across
compiler versions, and both implementations must agree byte-for-byte.
The rules, as implemented:

- **Atoms**: cord form `'...'` iff the value is ≥ 2 bytes and every
  little-endian byte is printable ASCII (0x20–0x7E) excluding the
  quote (0x27); otherwise decimal with dots every three digits.
  Rendering is a function of the *value* — the IR does not remember
  source spelling.
- **Wide forms** (single line): atom and axis literals; cells
  `[e1 e2 …]`; ops `(%name e1 …)`. `#let`/`#match` have no wide form,
  so anything containing them renders tall.
- **Width**: an expression renders wide iff it has a wide form and
  `indent + length + reserve ≤ 76`, where *reserve* is the number of
  closing-delimiter characters an enclosing form will append to its
  final line. No emitted line exceeds 76 columns.
- **Tall cell**: `[ ` merged with the first element's first line
  (two columns, so continuations align); remaining elements at
  indent+2; `]` appended to the final line.
- **Tall op**: `(%name` on its own line; arguments at indent+2; `)`
  appended to the final line.
- **Tall `#let`**: `#let .n = V in` on one line when `V`'s wide form
  fits; else `#let .n =`, `V` at indent+2, `in` at the same indent;
  body at the same indent as `#let`.
- **Tall `#match`**: `#match S {` when `S`'s wide form fits; else
  `#match`, `S` at indent+2, `{` at the indent. Each arm at indent+2:
  `P => B` on one line when both fit; else `P =>` with `B` at
  indent+4; else `P` tall with ` =>` appended and `B` at indent+4.
  The default renders as an arm with pattern `_`, last. `}` on its own
  line at the match's indent.
- **Schemas** always render wide; right spines flatten
  (`{.a .b .c}`, never `{.a {.b .c}}`).
- **Program**: `:subject SCH` line when a schema is present, then the
  expression at indent 0; every line newline-terminated.
- **Comments** are not represented in the IR and never emitted
  (provenance annotations are §8 and ride a side channel).

## 5. Jamfiles and the deterministic lift (M0.5 — implemented)

Nock artifacts in the wild are jamfiles — hoonc kernels, pills,
`.jam` build outputs. `nasm-from-jam` (Hoon) / `nasm_from_jam`
(Python, plus `python -m nockasm --from-jam formula.jam`) reads one
and emits canonical `.nasm`: cue, then *lift*, then render.

`lift : * -> nasm` is the **deterministic, zero-heuristic** reading of
a noun as a formula. Nock is homoiconic — nothing in a noun marks it
as code — so the contract is: the caller asserts the root is a
formula, and the lift propagates that assumption through Nock's
positional grammar (the expander's per-opcode kinds table read in
reverse). Concretely:

- Cell heads 0–11 with well-shaped tails lift to named ops; formula
  positions recurse; a cell head means cons-formula (both halves
  lift).
- **Anywhere the shape is not a valid formula** — an atom in a formula
  position (`[2 5 6]`), an opcode head above 11, a cell axis — the
  node falls back to a structural raw cell, right-spine flattened.
- **No intent is ever claimed**: constants are `%const` (never
  `%arm`), axes are `%slot` (never the core aliases), and no macro
  skeleton is recognized. Opcode-1 payloads render as pure data even
  when they are batteries — that judgment belongs to tooling with
  out-of-band knowledge (debug info), not to a deterministic reader.

Soundness law, enforced over the corpus and the fallback zoo by
`tests/test_lift.py`, and cross-implementation (byte-identical lifted
renders, via `tests/test_hoon.py`):

```
(lower ~ (lift f))  ===  f        for every noun f
```

Misclassifying data as code is impossible by construction — the only
cost of the no-heuristics stance is that lifted output for constants
reads as data, which is the honest default.

`jam`/`cue` ship in both implementations (canonical encoding,
backreferences included; a `.jam` file is the jammed atom's bytes,
little-endian).

## 6. Mapping Jock environments to names

Jock's lexical environment at any program point is a subject shape.
The mapping is mechanical:

- The compilation unit's initial subject becomes the `:subject`
  schema; nested shapes are already expressible (`{{.a .b} .c}`).
- Every Jock `let` becomes `#let` — the axis bookkeeping (`peg 3`
  shifting of names in scope) is what the library already implements.
- Jock scalar-tag matches become `#match`. Structural patterns with
  binders are an open extension (§9).

## 7. Macro extensions (L2 — conditional)

Hand-written Nockasm (see `benchmarks/*.nasm`) exhibits the idioms
every compiled functional program needs — gate, call, core, loop — as
raw `%push`/`%arm`/`%call` scaffolding. L2 would mechanize them:

| Macro | Lowering |
|---|---|
| `#gate .n = D in B` | `[8 [1 D] [1 B] 0 1]` — `.n` at axis 6, prior names pegged under 7 |
| `#call G A` | `[8 G 9 2 10 [6 A] 0 2]` — `A` compiled with names pegged under 3 |
| `#core { %a => B1 … }` | `[[1 battery] 0 1]` — battery at 2, prior names pegged under 3 |
| `#invoke %arm CORE` | `(%call axis CORE)` — battery axis computed by the library |
| `#loop .acc = I in B` | `[8 I 8 [1 B] 9 2 0 1]` — `.acc` at 6, prior names under 7 |
| `#recur E` | `[9 2 10 [6 E] 0 1]` — works in non-tail position (it is just a kick) |
| `#set .x = E in B` | `[7 [10 [ax E] 0 1] B]` — rebind an existing name; axes unchanged |

(All skeletons verified executable under pinochle; multi-argument
gates and multi-accumulator loops take a schema in the binder
position.)

**Why conditional.** As a *target*, the macros add zero expressive
power — a machine emits scaffolding as easily as sugar. What they buy
is artifact legibility/compression and a single point of truth for the
calling conventions. What they cost is a doubled maintenance surface:
every macro is two synced implementations, forever, held together by
the differential suite. So:

> L2 ships only as a **single frozen batch, gated on evidence**:
> compile real Jock programs at L1 and read the artifacts. Skeleton
> soup → the batch is priced correctly; legible enough → skip it.

**The resolution firewall.** Arm names never resolve implicitly across
core boundaries. `#invoke` always takes an explicit core expression;
crossing into a nested gate/core, the emitter binds the enclosing core
to a leg first:

```
#core {
  %method =>
    #let .this = (%self) in
    #gate .x = 0 in
      (#call (#invoke %sibling .this) .x)
}
```

Explicit `.this`, no capture magic. The moment names resolve through
the subject tree, this layer is reimplementing Hoon — that is the line
not to cross. Arm names (`%name`, battery positions) and leg names
(`.name`, subject axes) remain distinct namespaces.

**Acceptance (if gated in):** rewrite `benchmarks/dec.nasm` and
`benchmarks/fibonacci.nasm` in the macros; they must expand
bit-identical to the `urbit/benchmark` formulas. The inner skeletons
already match exactly; the outermost call conventions may force a
`#call` sibling for "kick the gate that is already the subject" —
add the sibling rather than weaken the test. For scale: `dec` in L2 is

```
#gate .m = 0 in
#loop .b = 0 in
  (%if (%eq .m (%inc .b))
       .b
       (#recur (%inc .b)))
```

against ~40 lines hand-drilled today.

## 8. Provenance channel

Two tiers, both optional, neither in the IR:

1. **Comment provenance** (zero-cost): `render` grows an annotation
   side-table emitting structured comments before forms:

   ```
   ;@ src/fib.jock:12:3
   (%call 2 …)
   ```

   Comments are lexical; the round-trip law is unaffected.

2. **`%spot` hints** (debug builds): wrap annotated forms in dynamic
   hints, `[11 [%spot [1 p q]] f]`, mirroring Hoon's `~_`, so stack
   traces from a crashed formula point at Jock source lines. A
   `lower`/`render` flag; release builds omit it; the differential
   suite runs with it off.

## 9. Open questions

1. **Structural match patterns.** `#match` compares whole nouns by
   equality. Jock destructuring (`[%tag a b]` binding `a`, `b`) wants
   cell patterns with binders — depth tests + tag equality +
   `#let`-style binding. A real extension; needs its own mini-spec.
2. **Jets and hints.** Where Jock wants `%fast` registration, the
   emitter writes `(%hintd …)` directly (works today); revisit a
   `#jet` convenience only if artifacts get noisy.
3. **Optimization surface.** If Jock grows peephole passes, they could
   run on `$nasm` (legible before/after diffs) rather than on nouns.
   Nothing blocks either; the IR makes the option real.
4. **Heuristic lifting.** The *deterministic* lift shipped in M0.5
   (§5) — jamfiles were the concrete consumer. What remains cut is the
   heuristic tier specced in v0.1: skeleton recognition (re-sugaring
   gates/cores/loops), core-alias axes, and `%arm` promotion of
   opcode-1 payloads on use-evidence. All of it is sound
   (`%const`/`%arm` lower identically, so misclassification costs
   legibility, never bits) but it makes claims about intent, and
   intent belongs to debug info. Revisit only if reading lifted
   artifacts without provenance becomes a real workflow.

## 10. Phasing

| Phase | Deliverable | Repo | Status |
|-------|-------------|------|--------|
| M0 | `parse`/`lower`/`render` public, `nasm-version`, round-trip + parity tests | nockasm | **done** |
| M0.5 | `jam`/`cue`, the deterministic `lift`, `nasm-from-jam` + CLI, soundness + parity tests | nockasm | **done** |
| M1 | Jock backend: `emit : typed-ast -> nasm` behind a flag; differential CI vs legacy codegen (bit-identical nouns); `.nasm` artifacts in review | jock-lang | design |
| M2 | The L2 decision, from M1's artifacts; if gated in: the frozen macro batch + benchmark-rewrite acceptance | nockasm | gated |
| M3 | Provenance comments, `%spot` debug mode | both | design |
| — | `%nasm` clay mark + hoonc usage patterns (`doc/nasm-mark.md`) | nockasm | **done** (first-class hoonc patch proposed) |

M0 is useful with no Jock buy-in. Everything Jock-side rides behind a
flag until the differential CI is green.
