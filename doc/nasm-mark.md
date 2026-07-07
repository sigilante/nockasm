# The `%nasm` mark

*2026-07-07. The clay mark is implemented (`desk/mar/nasm.hoon`); the
hoonc patterns below work today; the hoonc patch is proposed.*

One file type, three worlds. The design principle that keeps them
compatible: **the canonical noun form of a `.nasm` file is its source
text as a cord** — exactly parallel to `%hoon`. Storage and transport
stay textual everywhere; expansion to a Nock formula is always a
library call (`+expand:nockasm`), never a storage-layer behavior. A
`.nasm` file with a syntax error is still a file.

## Urbit (clay)

`desk/mar/nasm.hoon`, modeled on the base `%hoon` mark:

- noun form `@t` (the source cord)
- `grow`: `%mime` (`/text/x-nockasm`), `%txt`
- `grab`: `%mime`, `%txt`, `%noun`
- `grad %txt` (line-diff revision control, like all source marks)

With the mark committed, `.nasm` files live in clay like any source
file:

```
> .^(@t %cx /=base=/tst/dec/nasm)
'...'
> =nasm -build-file %/lib/nockasm/hoon
> .*(41 (expand:nasm .^(@t %cx /=base=/tst/dec/nasm)))
40
```

No expansion happens at commit time, deliberately: marks validate
shape, not semantics, and `%hoon` sets the precedent (clay does not
compile your hoon on commit either).

## NockApp (hoonc), today — no patch required

hoonc's `/*` rune treats the mark as a file extension and loads any
non-`.hoon` file as raw `%octs` bound to the face. Combined with the
typhoon-registered library, `.nasm` files are already usable:

```hoon
/+  nockasm
/*  dec-src  %nasm  /formulas/dec
::  dec-src is octs: [len=@ud data=@]; expand at runtime:
=/  formula  (expand:nockasm q.dec-src)
```

For build-time expansion, use the `/#` (dat) mechanism — dat files
are kicked during the build, so the formula is precomputed into the
kernel:

```hoon
::  /dat/dec-formula.hoon
/+  nockasm
/*  dec-src  %nasm  /formulas/dec
|.  (expand:nockasm q.dec-src)
```

```hoon
::  consumer
/#  dec-formula  ::  the formula noun, computed at build time
```

## NockApp (hoonc), proposed patch — first-class `%nasm`

The deeper integration makes `/*  f  %nasm  /path` bind `f` directly
to the expanded formula. hoonc's `+make-node` currently forks on
`+is-hoon`; the patch adds a `.nasm` case that parses at build time
and emits a constant-noun hoon AST leaf:

```hoon
?:  (is-nasm pat)
  :_  new-pc
  :*  pat
      file-hash
      ~
      [%hoon [%rock %n (expand:nockasm fil)]]   ::  formula as constant
      %.n
  ==
```

plus `+is-nasm` (extension test, parallel to `+is-hoon`) and the
vendored library. Expansion errors become build errors with `~|`
traces, caching falls out of the existing file-hash machinery, and
the `%jock` mark is the precedent for source-typed build inputs. This
is the Tier 2 item from `doc/compiler-target.md` §10, sized at ~20
lines plus the vendor.

## Interchange summary

| context | `.nasm` at rest | formula |
|---|---|---|
| clay | `%nasm` mark, noun = source cord | `+expand:nockasm` in userspace |
| hoonc today | `%octs` via `/*` | `+expand:nockasm` at runtime, or `/#` precompute |
| hoonc patched | source, parsed at build | `/*` face is the formula |
| anywhere | — | jammed formula bytes (`.jam`), lifted back via `+nasm-from-jam` |

The last row is the escape hatch between all worlds: a raw formula
jam is 67 bytes for `dec`, readable by `cue` everywhere, and
`nasm-from-jam` recovers legible source from it.
