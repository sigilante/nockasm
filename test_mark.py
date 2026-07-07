"""Compile and exercise the %nasm clay mark (desk/mar/nasm.hoon) via
`urbit eval` -- checks the grow/grab arms without a ship. (The mark
was also verified on a live fake ship: clay accepts .nasm files
through it, and dojo round-trips them into the expander.)

    python test_mark.py
"""

import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
MARK = os.path.join(HERE, 'desk', 'mar', 'nasm.hoon')
VERE = os.environ.get(
    'URBIT_BIN',
    '/Users/neal/urbit/vere/zig-out/aarch64-macos-none/urbit',
)

TAIL = """
=/  src  '(%inc (%self))'
=/  bay  ~(. door src)
=/  m  mime:grow:bay
?.  =(p.m /text/x-nockasm)  [%fail %mime-path p.m]
?.  =(src (mime:grab:bay m))  [%fail %mime-roundtrip ~]
?.  =(src (noun:grab:bay `*`src))  [%fail %noun-clam ~]
?.  =(src (of-wain:format txt:grow:bay))  [%fail %txt-roundtrip ~]
%nasm-mark-ok
"""


def main():
    with open(MARK) as f:
        mark = f.read()
    src = '=/  door\n' + mark + TAIL
    proc = subprocess.run(
        [VERE, 'eval'], input=src, capture_output=True, text=True,
        timeout=600,
    )
    out = proc.stdout + proc.stderr
    if 'nasm-mark-ok' in out:
        print('ok: %nasm mark compiles; mime/txt/noun arms round-trip')
        return 0
    print('FAIL: mark check did not pass')
    print(out[-2000:])
    return 1


if __name__ == '__main__':
    sys.exit(main())
