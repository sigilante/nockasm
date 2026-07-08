"""Kernel dispatch tests: directive recognition through NockasmKernel._dispatch.

The kernel keys directives (#subject/#show/#help) off the first line, but a
cell is naturally written with a leading ``;`` comment.  These tests pin the
behaviour that leading blank/comment lines are skipped before dispatch, so a
comment-first cell still sets the subject rather than shipping ``#subject`` to
the assembler.
"""

from nockasm_kernel.kernel import NockasmKernel
from pinochle import parse as parse_noun
from _testkit import Tally

_t = Tally('kernel')
section = _t.section


def fresh():
    k = NockasmKernel.__new__(NockasmKernel)
    k.subject = parse_noun("0")
    k.last_result = None
    k.last_expansion = None
    return k


def check(name, code, want_last_line):
    try:
        out = fresh()._dispatch(code)
    except Exception as e:  # noqa: BLE001
        _t.fail_(name, f"raised {type(e).__name__}: {e}")
        return
    got = out.split("\n")[-1]
    if got == want_last_line:
        _t.pass_(name, got)
    else:
        _t.fail_(name, f"got {got!r}, want {want_last_line!r}")


section("comment-first cells still dispatch #subject")

# Bare #subject line already worked; keep it as a control.
check("subject-first",
      "#subject [0 0]\n(%eq (%slot 2) (%slot 3))",
      "0")

# The regression: a leading ; comment must not defeat directive detection.
check("comment then subject",
      "; equality of slots 2 and 3 — subject [0 0]\n"
      "#subject [0 0]\n(%eq (%slot 2) (%slot 3))",
      "0")

check("comment then subject + schema",
      "; two-element subject\n#subject [5 5]\n"
      ":subject {.x .y}\n(%eq .x .y)",
      "0")

check("blank line then comment then subject",
      "\n; leading blank then comment\n#subject [10 41 99]\n"
      ":subject {.before .target .after}\n(%inc .target)",
      "42")


section("comments do not break plain assembly cells")

check("comment then bare formula (default subject 0)",
      "; increment the default subject\n(%inc (%slot 1))",
      "1")


section("#show / #help survive leading comments")

check("comment then #show",
      "; peek at state\n#show",
      "subject:    0")

_t.done()
