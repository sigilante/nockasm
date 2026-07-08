"""Shared harness for the standalone test scripts.

Every test_*.py here is run directly (``python test_x.py``) by CI, not
under pytest: each keeps a pass/fail tally and exits nonzero if anything
failed. This module factors out that tally and its reporting footer, plus
the ``urbit eval`` subprocess plumbing the Hoon suites share, so the
individual files carry only their cases.
"""

import os
import subprocess
import sys


HERE = os.path.dirname(os.path.abspath(__file__))

# The latest release vere by default; override for a local build. The
# Hoon suites are differential against the platform, so the binary is
# deliberately unpinned.
VERE = os.environ.get(
    'URBIT_BIN',
    '/Users/neal/urbit/vere/zig-out/aarch64-macos-none/urbit',
)


def desk_file(*parts):
    """Absolute path to a file under the repo's desk/ tree."""
    return os.path.join(HERE, 'desk', *parts)


def run_eval(src, timeout=600):
    """Feed ``src`` to ``urbit eval`` and return combined stdout+stderr."""
    proc = subprocess.run(
        [VERE, 'eval'], input=src, capture_output=True, text=True,
        timeout=timeout,
    )
    return proc.stdout + proc.stderr


class Tally:
    """A pass/fail counter with the reporting the scripts share.

    Standard assertions::

        check(name, got, want)   equality
        expect(name, cond, ...)  boolean; trailing args printed on failure
        raises(name, fn, exc)    fn() must raise exc

    Custom runners (which format their own lines) count with the
    low-level ``pass_``/``fail_``. ``section`` prints a header; ``done``
    prints the summary and exits nonzero if anything failed.
    """

    def __init__(self, label=''):
        self.label = label
        self.passed = 0
        self.failed = 0

    # -- low-level recording (for runners with custom output) ----------

    def pass_(self, name, note=''):
        self.passed += 1
        print(f"  ok   {name}" + (f": {note}" if note else ""))

    def fail_(self, name, *lines):
        self.failed += 1
        print(f"  FAIL {name}")
        for ln in lines:
            print(f"       {ln}")

    # -- standard assertions -------------------------------------------

    def check(self, name, got, want):
        if got == want:
            self.pass_(name)
        else:
            self.fail_(name, f"got:  {got!r}", f"want: {want!r}")

    def expect(self, name, cond, *detail):
        if cond:
            self.pass_(name)
        else:
            self.fail_(name, *detail)

    def raises(self, name, fn, exc):
        try:
            fn()
        except exc:
            self.pass_(name)
        except BaseException as e:  # noqa: BLE001
            self.fail_(name,
                       f"expected {exc.__name__}, got {type(e).__name__}: {e}")
        else:
            self.fail_(name, f"expected {exc.__name__}, got no exception")

    # -- output --------------------------------------------------------

    def section(self, title):
        print(f"\n== {title} ==")

    def done(self):
        print()
        tag = f"{self.label}: " if self.label else ""
        print(f"==> {tag}{self.passed} passed, {self.failed} failed")
        sys.exit(0 if self.failed == 0 else 1)
