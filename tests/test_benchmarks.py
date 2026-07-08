"""Benchmark harness: load .nasm files from disk, run against pinochle,
compare to expected results.

Mirrors the layout of urbit/benchmark/desk/bar — each entry in
benchmarks/tests.json names a benchmark whose formula source lives at
benchmarks/<name>.nasm. Subject and expected result come from the JSON.

Run with: python test_benchmarks.py
"""

import json
import os
import sys

# _testkit first: importing it puts the repo root on sys.path (see there).
from _testkit import ROOT, Tally
from pinochle import nock, parse_noun
from nockasm import expand


BENCH_DIR = os.path.join(ROOT, 'benchmarks')

# pinochle is a direct recursive interpreter; benchmarks like factorial
# and dec need deeper Python stacks than the default 1000 frames.
sys.setrecursionlimit(200_000)


def load_manifest():
    with open(os.path.join(BENCH_DIR, 'tests.json')) as f:
        return json.load(f)


def run_one(entry):
    name = entry['name']
    nasm_path = os.path.join(BENCH_DIR, f'{name}.nasm')
    with open(nasm_path) as f:
        nasm_src = f.read()
    formula_str = expand(nasm_src)
    formula = parse_noun(formula_str)
    subject = parse_noun(entry['subject'])
    want = parse_noun(entry['result'])
    got = nock(subject, formula)
    return got, want, formula_str


def main():
    t = Tally('benchmarks')
    manifest = load_manifest()
    for entry in manifest:
        name = entry['name']
        try:
            got, want, formula_str = run_one(entry)
        except FileNotFoundError as e:
            print(f"  SKIP {name}: {e}")
            continue
        except Exception as e:  # noqa: BLE001
            t.fail_(name, f"{type(e).__name__}: {e}")
            continue
        if got == want:
            t.pass_(name, entry['description'])
        else:
            t.fail_(name, f"got {got!r}, want {want!r}",
                    f"formula: {formula_str}")
    t.done()


if __name__ == '__main__':
    main()
