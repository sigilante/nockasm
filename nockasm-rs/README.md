# `nockasm` (pure Rust)

A pure Rust implementation of Nockasm: legible Nock assembly in,
canonical [Nock 4K](https://nock.is) plus the Nock 12 `%scry`
extension out — the fourth
independent executor of the nockasm laws, after the Python reference
(`../nockasm.py`), the Hoon library (`../desk/lib/nockasm.hoon`), and
the Hoon-on-nockvm NockApp (`../nasmc`). The differential suite
(`../tests/test_rust.py`) holds all four byte-identical over the shared
corpus.

Built to be embedded: **zero dependencies**, `#![forbid(unsafe_code)]`,
stable toolchain, and a typed IR a compiler backend can target directly
without going through text.

## Library

```rust
use nockasm::{expand, jam, noun, parse};

let formula = expand(":subject {.a .b}  (%eq .a .b)").unwrap();
assert_eq!(formula, noun![5 [0 2] 0 3]);
assert_eq!(formula.to_string(), "[5 [0 2] 0 3]");

// The IR round trip: parse / lower / render
let program = parse("#let .x = 1 in (%inc .x)").unwrap();
assert_eq!(program.render(), "#let .x = 1 in\n(%inc .x)\n");
assert_eq!(program.lower().unwrap(), noun![8 [1 1] 4 0 2]);

// Jamfiles: jam / cue / lift
let bytes = jam(&formula);
assert_eq!(nockasm::nasm_from_jam(&bytes).unwrap(), "(%eq (%slot 2) (%slot 3))\n");
```

The pipeline and its laws (IR contract version `NASM_VERSION = 3`; see
the compiler-target spec, maintained in sigilante/jock as
`docs/spec/nockasm-target.md`):

```
.nasm source ─ parse ─▶ Program (schema + Nasm IR)
                             │
                             ├─ lower  ─▶ Noun     (canonical Nock)
                             └─ render ─▶ String   (canonical .nasm)

jamfile bytes ─ cue ─▶ Noun ─ lift ─▶ Nasm ─ render ─▶ String
```

- **Round trip**: `expand(render(s, a)) == lower(s, a)`, and rendering
  is idempotent through `parse`. `render` is byte-identical to the
  Python and Hoon renderers.
- **Lift soundness**: `lower(None, lift(f)) == f` for every noun `f`.
- **Serialization**: `cue(&jam(n)) == Ok(n)` for every noun `n`; jam
  deduplicates by structural equality, exactly like the reference
  encoders.

## Types

The IR (`Nasm`, `Op`, `Schema`, `Program`) refines the reference
contract: ill-formed nodes — unknown opcodes, wrong arities, non-atom
axis arguments, unary raw cells, a `#match` without a default — are
unrepresentable. What the Python and Hoon implementations reject at
lower time is rejected at parse (or IR-construction) time here; the
composed `expand` accepts and rejects exactly the same sources. `lower`
can only fail on name resolution: unbound axes, schema duplicates,
`#let` shadowing.

The v3 vocabulary is covered in full: `Nasm::Nock` is the opaque
formula embed (`(%nock F)` — identity expansion, noun-literal-only
payload, never validated or rewritten; also the lift's fallback for
any formula-position subtree it cannot macro-ize, which keeps the lift
total), and `Schema::Hole` is the anonymous subject position (`_` —
structure with no name bound, so machine-generated schemas mirror
subject shape without inventing names), and `Op::Scry` represents
`(%scry R P)` as `[12 R P]` with both arguments in formula position.

The `noted` module is the annotated instantiation of that vocabulary —
the Hoon `+nasm-of` / `noted` pattern from `../desk/sur/nockasm.hoon`,
kept in the reference's own shape: an opcode is a name applied to a
list of nodes, so an axis argument is a node that can carry a note
(a positioned emitter's conformance vectors compare those positions).
`NasmOf<N>` is the vocabulary generic over its child type, and
`Noted<A>` ties the knot with a note of type `A` on every node (a
source position, a provenance tag), so a compiler emitting positioned
IR gets every vocabulary case by construction instead of maintaining a
fork. `Noted::strip` projects onto plain `Nasm` — and is where the
reference implementations' lower-time refusals (`unknown-opcode`,
`op-arity`, `axis-arg-must-be-atom`, `empty-raw-cell`) live, since the
typed IR cannot hold them — `Noted::dress` is its section, and
`noted::lower` / `noted::render` are the bare pipeline composed with
the projection: the meaning of annotated IR is the meaning of its
projection. The bridge from the typed vocabulary is one private
exhaustive match (`unroll`) and the bridge back one opcode table
(`noted::roll`) the tests hold to the `Op` enum, so a variant added to
one without the other is a compile or test failure rather than silent
drift. `Noted` tears down, strips, dresses, and annotates on explicit
stacks, like `Nasm`.

Nouns are cheap to clone (reference-counted cells with cached
structural hashes, which is what makes `jam`'s backreference table
O(1) per node), and atoms are arbitrary-precision with an inline fast
path for values that fit a `u64`. The noun layer — equality, hashing,
drop, printing, `jam`, `cue` — plus `lift`, `lower`, `render`, and the
IR's teardown all run on explicit stacks, and parsing is depth-bounded,
so no input, however deep, can overflow the call stack: hostile
jamfiles lift, lower, and render cleanly, and machine-emitted IR has no
depth bound at all. The renderer sizes wide forms in a bottom-up pass
and materializes each node's text exactly once, so rendering is
O(input + output) where the reference implementations are O(n · depth).
With the `sync` feature the internal `Rc` becomes `Arc` and everything
is `Send + Sync`.

## CLI

Flag-compatible with `nasmc`, so the differential suite drives both
with identical invocations:

```bash
cargo build --release            # -> target/release/nockasm

nockasm program.nasm             # -> program.jam  (raw formula jam)
nockasm --text program.nasm     # canonical flat noun to stdout
nockasm --render program.nasm   # canonical .nasm formatting to stdout
nockasm --lift formula.jam      # -> formula.nasm (deterministic lift)
```

## Testing

```bash
cargo test                       # unit corpus + laws + property tests
python ../tests/test_rust.py     # differential vs the python oracle
```

`cargo test` covers the unit corpus (a port of `tests/test_nockasm.py`),
the law suites over the shared corpus and the benchmark transcriptions
(which are also *executed* against a built-in mini Nock evaluator), the
depth-bound guarantee, and dependency-free property tests over
generated nouns (fixed-seed, so failures reproduce).

`../tests/test_rust.py` is the cross-implementation gate: every corpus
source through the CLI in every mode — jam, `--text`, `--render`,
`--lift` — must match the Python oracle byte-for-byte, and every
negative-corpus source must fail. When the `nasmc` binary is present
(or `NASMC_BIN` is set) it also compares the Rust and Hoon/NockApp
binaries against each other directly, byte-identical output for
identical invocations.
