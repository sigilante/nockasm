"""Differential test for the nasmc binary (nasmc/): the third executor.

Every corpus source is compiled by nasmc (Hoon expander on nockvm) and
the output jamfile must match jam(expand_to_noun(src)) from the Python
oracle byte-for-byte. The text/render/lift modes are spot-checked on a
subset (each nasmc run boots the kernel, so full-corpus x all-modes
would be slow for no extra coverage).

    python test_nasmc.py

Set NASMC_BIN to the binary (default: nasmc/target/release/nasmc).
"""

import os
import subprocess
import tempfile

from nockasm import expand, expand_to_noun, jam, lift, parse, render
from test_hoon import GOOD, benchmark_cases
from _testkit import Tally

HERE = os.path.dirname(os.path.abspath(__file__))
NASMC = os.environ.get(
    'NASMC_BIN', os.path.join(HERE, 'nasmc', 'target', 'release', 'nasmc'))

_t = Tally('nasmc')
check = _t.expect


def run_nasmc(args, timeout=120):
    return subprocess.run([NASMC] + args, capture_output=True, timeout=timeout)


def main():
    corpus = GOOD + benchmark_cases()
    with tempfile.TemporaryDirectory() as td:
        # jam mode, full corpus
        for name, src in corpus:
            src_path = os.path.join(td, 'case.nasm')
            out_path = os.path.join(td, 'case.jam')
            with open(src_path, 'w') as f:
                f.write(src)
            proc = run_nasmc([src_path, '-o', out_path])
            if proc.returncode != 0:
                check(f"jam:{name}", False, f"exit {proc.returncode}")
                continue
            with open(out_path, 'rb') as f:
                got = int.from_bytes(f.read(), 'little')
            want = jam(expand_to_noun(src))
            check(f"jam:{name}", got == want)
            os.remove(out_path)

        # spot checks on the other modes
        spots = [corpus[0], corpus[10], corpus[-1]]
        for name, src in spots:
            src_path = os.path.join(td, 'case.nasm')
            with open(src_path, 'w') as f:
                f.write(src)

            out_path = os.path.join(td, 'case.txt')
            proc = run_nasmc([src_path, '--text', '-o', out_path])
            with open(out_path) as f:
                got = f.read()
            check(f"text:{name}",
                  proc.returncode == 0 and got == expand(src) + '\n',
                  repr(got))

            out_path = os.path.join(td, 'case.rendered')
            proc = run_nasmc([src_path, '--render', '-o', out_path])
            with open(out_path) as f:
                got = f.read()
            check(f"render:{name}",
                  proc.returncode == 0 and got == render(*parse(src)),
                  repr(got[:80]))

            jam_path = os.path.join(td, 'case.jam')
            noun = expand_to_noun(src)
            data = jam(noun)
            with open(jam_path, 'wb') as f:
                f.write(data.to_bytes((data.bit_length() + 7) // 8, 'little'))
            out_path = os.path.join(td, 'case.nasm.lifted')
            proc = run_nasmc([jam_path, '--lift', '-o', out_path])
            with open(out_path) as f:
                got = f.read()
            check(f"lift:{name}",
                  proc.returncode == 0 and got == render(None, lift(noun)),
                  repr(got[:80]))

        # a bad source must fail with a nonzero exit
        src_path = os.path.join(td, 'bad.nasm')
        with open(src_path, 'w') as f:
            f.write('(%inc .unbound)')
        proc = run_nasmc([src_path, '-o', os.path.join(td, 'bad.jam')])
        check("error-exit", proc.returncode != 0,
              f"exit {proc.returncode}")

    _t.done()


if __name__ == '__main__':
    main()
