"""M0 target-IR properties: the round-trip law and render idempotence.

    expand_to_noun(render(s, a)) == lower(s, a) == expand_to_noun(src)
    render(parse(render(s, a))) == render(s, a)

Corpus: every expansion case from the differential suite plus the
benchmark transcriptions (imported from test_hoon).
"""

import sys

from nockasm import parse, lower, render, expand_to_noun, NASM_VERSION
from test_hoon import GOOD, benchmark_cases


PASS = 0
FAIL = 0


def check(name, ok, detail=''):
    global PASS, FAIL
    if ok:
        PASS += 1
        print(f"  ok   {name}")
    else:
        FAIL += 1
        print(f"  FAIL {name}  {detail}")


print(f"nasm ir version {NASM_VERSION}")
corpus = GOOD + benchmark_cases()

for name, src in corpus:
    schema, ast = parse(src)
    text = render(schema, ast)

    want = expand_to_noun(src)
    low = lower(schema, ast)
    rt = expand_to_noun(text)
    if not (want == low == rt):
        check(f"{name}: round-trip", False,
              f"lower/render/expand disagree\n{text}")
        continue

    schema2, ast2 = parse(text)
    text2 = render(schema2, ast2)
    if text2 != text:
        check(f"{name}: idempotence", False,
              f"re-render differs:\n--- first\n{text}--- second\n{text2}")
        continue

    over = [ln for ln in text.splitlines() if len(ln) > 76]
    check(name, not over, f"line over 76 cols: {over[:1]}")

print()
print(f"==> render: {PASS} passed, {FAIL} failed")
sys.exit(0 if FAIL == 0 else 1)
