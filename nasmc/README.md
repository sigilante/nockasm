# `nasmc`: the Nockasm compiler as a NockApp

`.nasm` source in, Nock formula out — with the expansion performed by
the Hoon library compiled to Nock, running on nockvm. Modeled on
[hoonc](https://github.com/nockchain/nockchain/tree/master/crates/hoonc):
a kernel (`hoon/apps/nasmc/nasmc.hoon`, wrapping `lib/nockasm`) plus a
thin Rust host that pokes `[%compile mode tex out]` and lets the file
and exit drivers do the rest.

## Usage

```bash
nasmc program.nasm               # -> program.jam  (raw formula jam)
nasmc program.nasm -o out.jam
nasmc --text program.nasm        # canonical flat noun to stdout
nasmc --render program.nasm      # canonical .nasm formatting to stdout
nasmc --lift formula.jam         # -> formula.nasm (deterministic lift)
```

Output jams are raw formulas (never vases): `dec.nasm` compiles to a
~70-byte jamfile that any `cue` reads back directly, including
`python -m nockasm --from-jam`.

## Building

Requires the nightly toolchain pinned in `rust-toolchain.toml` (the
nockvm dependency uses unstable features).

```bash
cargo build --release            # -> target/release/nasmc
```

The kernel is embedded as `bootstrap/nasmc.jam`. After changing the
kernel hoon (or the library — `hoon/lib/nockasm.hoon` is a symlink to
`../../desk/lib/nockasm.hoon`), regenerate it with hoonc and rebuild:

```bash
hoonc --new hoon/apps/nasmc/nasmc.hoon hoon
mv out.jam bootstrap/nasmc.jam
cargo build --release
```

## Testing

`../test_nasmc.py` is the differential suite: every corpus source
(unit cases + the urbit/benchmark transcriptions) compiled by nasmc
must produce a jam byte-identical to `jam(expand_to_noun(src))` from
the Python oracle — making nasmc the third independent executor of
the same laws, after CPython and Hoon-on-vere.
