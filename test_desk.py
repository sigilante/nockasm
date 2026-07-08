"""Run the on-ship test file (desk/tests/lib/nockasm.hoon) via
`urbit eval`, no ship required.

The desk tests import /+ *test and /+ nockasm, which eval cannot
resolve; this runner shims them: the library core is bound to a
`nockasm` face, expect-eq / expect-fail are provided (mirroring
lib/test.hoon), and every `++ test-*` arm is discovered from the file
and evaluated. Output %tests-all-ok means every arm returned an empty
tang.

    python test_desk.py

Set URBIT_BIN to point at a vere binary if the default is wrong.
"""

import re
import sys

from _testkit import desk_file, run_eval

LIB = desk_file('lib', 'nockasm.hoon')
TESTS = desk_file('tests', 'lib', 'nockasm.hoon')

SHIM = """
=/  expect-eq
  |=  [expected=vase actual=vase]
  ^-  tang
  ?:  =(q.expected q.actual)  ~
  [%leaf "expect-eq failed"]~
=/  expect-fail
  |=  a=(trap)
  ^-  tang
  =/  b  (mule a)
  ?-  -.b
    %|  ~
    %&  ['expected failure - succeeded' ~]
  ==
=>
"""


def build_eval_input() -> str:
    with open(LIB) as f:
        lib = f.read()
    with open(TESTS) as f:
        tests = f.read()
    arms = re.findall(r'^\+\+  (test-[a-z0-9-]+)', tests, re.M)
    if not arms:
        raise SystemExit('no test arms found in desk tests file')
    body = '\n'.join(f"      ['{a}' {a}]" for a in arms)
    tests_body = '\n'.join(
        ln for ln in tests.splitlines() if not ln.startswith('/+'))
    tail = f"""
=/  results=(list [name=@t res=tang])
  :~
{body}
  ==
=/  fails
  %+  skim  results
  |=([name=@t res=tang] !=(~ res))
?:  =(~ fails)  %tests-all-ok
[%failures (turn fails |=([name=@t res=tang] name))]
"""
    return ('=/  nockasm\n' + lib + SHIM + tests_body + '\n' + tail), arms


def main():
    src, arms = build_eval_input()
    out = run_eval(src)
    if 'tests-all-ok' in out:
        print(f'ok: {len(arms)} desk test arms pass under the eval shim')
        return 0
    print('FAIL: desk tests did not pass')
    print(out[-3000:])
    return 1


if __name__ == '__main__':
    sys.exit(main())
