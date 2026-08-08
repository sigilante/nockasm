"""Differential test for the pure Rust crate (nockasm-rs/): the fourth
independent executor of the nockasm laws, after CPython, Hoon-on-vere,
and Hoon-on-nockvm (nasmc).

Two layers:

1. Oracle parity (always runs): every corpus source compiled by the Rust
   CLI must match the Python oracle byte-for-byte, in every mode — jam,
   --text, --render, and --lift — plus the lift fallback zoo and
   big-atom rendering, and every BAD source must fail with a nonzero
   exit.

2. NockApp parity (runs when the nasmc binary is available, or NASMC_BIN
   is set): the Rust and Hoon/NockApp binaries are compared *directly*,
   byte-for-byte, over the full corpus in jam mode and spot-checked in
   the text modes.

    python tests/test_rust.py

Set NOCKASM_RS_BIN to the Rust binary (default:
nockasm-rs/target/release/nockasm). Build it with
`cargo build --release` in nockasm-rs/.
"""

import os
import subprocess
import tempfile

# _testkit first: importing it puts the repo root on sys.path (see there).
from _testkit import ROOT, Tally
from nockasm import (cell, cord_to_nat, expand, expand_to_noun, jam, lift,
                     parse, render)
from test_hoon import BAD, GOOD, benchmark_cases

RUST = os.environ.get(
    'NOCKASM_RS_BIN',
    os.path.join(ROOT, 'nockasm-rs', 'target', 'release', 'nockasm'))
NASMC = os.environ.get(
    'NASMC_BIN', os.path.join(ROOT, 'nasmc', 'target', 'release', 'nasmc'))

_t = Tally('rust')
check = _t.expect
section = _t.section


def run(binary, args, timeout=120):
    return subprocess.run([binary] + args, capture_output=True,
                          timeout=timeout)


def jam_bytes(noun):
    j = jam(noun)
    return j.to_bytes((j.bit_length() + 7) // 8, 'little')


# The lift fallback zoo (mirrors tests/test_lift.py) plus atoms big
# enough to exercise bignum parsing, dotted rendering, and the cord form.
LIFT_CASES = [
    ('atom-in-formula-pos', (2, (5, 6))),
    ('opcode-above-12', (13, 3)),
    ('malformed-scry', (12, 3)),
    ('slot-of-cell', (0, (2, 3))),
    ('bare-atom', 42),
    ('const-deep-data', (1, cell(9, 9, 9, 9))),
    ('cons-formula', ((4, (0, 2)), (4, (0, 3)))),
    ('hint-static', (11, (0x74736166, (0, 1)))),
    ('hint-dynamic', (11, ((0x74736166, (1, 0)), (0, 1)))),
    ('malformed-if', (6, ((1, 0), 5))),
    ('call-cell-axis', (9, ((2, 3), (0, 1)))),
    ('big-atom-dotted', (1, 2 ** 200 + 12345)),
    ('big-atom-cord', (1, cord_to_nat('a cord big enough to be a cord'))),
    ('huge-atom-line', (1, 10 ** 120 + 7)),
]


def oracle_parity(td):
    corpus = GOOD + benchmark_cases()

    section('oracle parity: jam mode, full corpus')
    for name, src in corpus:
        src_path = os.path.join(td, 'case.nasm')
        out_path = os.path.join(td, 'case.jam')
        with open(src_path, 'w') as f:
            f.write(src)
        proc = run(RUST, [src_path, '-o', out_path])
        if proc.returncode != 0:
            check(f"jam:{name}", False, f"exit {proc.returncode}",
                  proc.stderr.decode(errors='replace'))
            continue
        with open(out_path, 'rb') as f:
            got = f.read()
        check(f"jam:{name}", got == jam_bytes(expand_to_noun(src)))
        os.remove(out_path)

    section('oracle parity: text/render modes, full corpus')
    for name, src in corpus:
        src_path = os.path.join(td, 'case.nasm')
        with open(src_path, 'w') as f:
            f.write(src)

        out_path = os.path.join(td, 'case.txt')
        proc = run(RUST, [src_path, '--text', '-o', out_path])
        with open(out_path) as f:
            got = f.read()
        check(f"text:{name}",
              proc.returncode == 0 and got == expand(src) + '\n',
              repr(got[:80]))

        out_path = os.path.join(td, 'case.rendered')
        proc = run(RUST, [src_path, '--render', '-o', out_path])
        with open(out_path) as f:
            got = f.read()
        check(f"render:{name}",
              proc.returncode == 0 and got == render(*parse(src)),
              repr(got[:80]))

    section('oracle parity: lift mode, corpus formulas + fallback zoo')
    lift_corpus = ([(name, expand_to_noun(src)) for name, src in corpus]
                   + LIFT_CASES)
    for name, noun in lift_corpus:
        jam_path = os.path.join(td, 'case.jam')
        with open(jam_path, 'wb') as f:
            f.write(jam_bytes(noun))
        out_path = os.path.join(td, 'case.lifted')
        proc = run(RUST, [jam_path, '--lift', '-o', out_path])
        with open(out_path) as f:
            got = f.read()
        check(f"lift:{name}",
              proc.returncode == 0 and got == render(None, lift(noun)),
              repr(got[:80]))

    section('oracle parity: the negative corpus fails')
    for name, src in BAD:
        src_path = os.path.join(td, 'bad.nasm')
        with open(src_path, 'w') as f:
            f.write(src)
        proc = run(RUST, [src_path, '-o', os.path.join(td, 'bad.jam')])
        check(f"bad:{name}", proc.returncode != 0,
              f"exit {proc.returncode} (expected nonzero)")


def nasmc_parity(td):
    """Direct Rust <-> Hoon/NockApp comparison: same inputs, same flags,
    byte-identical outputs. Each nasmc run boots the kernel, so the text
    modes are spot-checked while jam mode covers the full corpus."""
    corpus = GOOD + benchmark_cases()

    section('nasmc parity: jam mode, full corpus')
    for name, src in corpus:
        src_path = os.path.join(td, 'case.nasm')
        rust_out = os.path.join(td, 'rust.jam')
        nock_out = os.path.join(td, 'nasmc.jam')
        with open(src_path, 'w') as f:
            f.write(src)
        rust_proc = run(RUST, [src_path, '-o', rust_out])
        nasmc_proc = run(NASMC, [src_path, '-o', nock_out])
        if rust_proc.returncode != 0 or nasmc_proc.returncode != 0:
            check(f"nasmc-jam:{name}", False,
                  f"exits rust={rust_proc.returncode} "
                  f"nasmc={nasmc_proc.returncode}")
            continue
        with open(rust_out, 'rb') as f:
            got_rust = f.read()
        with open(nock_out, 'rb') as f:
            got_nasmc = f.read()
        check(f"nasmc-jam:{name}", got_rust == got_nasmc)

    section('nasmc parity: text/render/lift spot checks')
    spots = [corpus[0], corpus[10], corpus[-1]]
    for name, src in spots:
        src_path = os.path.join(td, 'case.nasm')
        with open(src_path, 'w') as f:
            f.write(src)
        for mode, ext in [('--text', 'txt'), ('--render', 'rendered')]:
            rust_out = os.path.join(td, f'rust.{ext}')
            nock_out = os.path.join(td, f'nasmc.{ext}')
            rust_proc = run(RUST, [src_path, mode, '-o', rust_out])
            nasmc_proc = run(NASMC, [src_path, mode, '-o', nock_out])
            with open(rust_out) as f:
                got_rust = f.read()
            with open(nock_out) as f:
                got_nasmc = f.read()
            check(f"nasmc-{mode[2:]}:{name}",
                  rust_proc.returncode == 0 and nasmc_proc.returncode == 0
                  and got_rust == got_nasmc,
                  repr(got_rust[:60]), repr(got_nasmc[:60]))

        jam_path = os.path.join(td, 'case.jam')
        with open(jam_path, 'wb') as f:
            f.write(jam_bytes(expand_to_noun(src)))
        rust_out = os.path.join(td, 'rust.lifted')
        nock_out = os.path.join(td, 'nasmc.lifted')
        rust_proc = run(RUST, [jam_path, '--lift', '-o', rust_out])
        nasmc_proc = run(NASMC, [jam_path, '--lift', '-o', nock_out])
        with open(rust_out) as f:
            got_rust = f.read()
        with open(nock_out) as f:
            got_nasmc = f.read()
        check(f"nasmc-lift:{name}",
              rust_proc.returncode == 0 and nasmc_proc.returncode == 0
              and got_rust == got_nasmc,
              repr(got_rust[:60]), repr(got_nasmc[:60]))


def main():
    with tempfile.TemporaryDirectory() as td:
        oracle_parity(td)
        if os.path.exists(NASMC):
            nasmc_parity(td)
        else:
            print(f"\n(nasmc binary not found at {NASMC}; skipping the "
                  f"direct NockApp comparison — set NASMC_BIN or build "
                  f"nasmc/ to enable it)")
    _t.done()


if __name__ == '__main__':
    main()
