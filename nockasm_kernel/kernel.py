"""
nockasm_kernel: Jupyter kernel for Nock Assembly.

Each cell is Nockasm source — macros are expanded to canonical Nock 4K,
then the formula is evaluated against the current subject using pinochle.

Kernel commands (prefix #):
    #subject NOUN       Set evaluation subject
    #show               Show subject / last expansion / last result
    #help               Command reference
"""

from ipykernel.kernelbase import Kernel
from nockasm import expand_to_noun, print_noun
import traceback

try:
    from pinochle import nock as _nock_eval, parse as _parse_noun, pretty as _pretty_noun
    _PINOCHLE = True
except ImportError:
    _PINOCHLE = False


def _to_pinochle(noun):
    """Convert nockasm's int/tuple noun to a pinochle Noun (via string)."""
    return _parse_noun(print_noun(noun))


class NockasmKernel(Kernel):
    implementation = 'Nockasm'
    implementation_version = '1.0'
    language = 'nockasm'
    language_version = '1.0'
    language_info = {
        'name': 'nockasm',
        'mimetype': 'text/plain',
        'file_extension': '.nasm',
    }
    banner = (
        "Nockasm Kernel — Nock Assembly macro expander"
        + (" + Nock 4K evaluator" if _PINOCHLE else
           " (install pinochle to enable evaluation)")
        + "\nType #help for commands.\n"
    )
    help_links = [
        {'text': 'Nock Specification',
         'url': 'https://nock.is/content/specification/index.html'},
        {'text': 'Urbit Documentation', 'url': 'https://docs.urbit.org'},
    ]

    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        self.subject = _parse_noun("0") if _PINOCHLE else 0
        self.last_result = None
        self.last_expansion = None

    # ------------------------------------------------------------------
    # Jupyter protocol
    # ------------------------------------------------------------------

    def do_execute(self, code, silent, store_history=True,
                   user_expressions=None, allow_stdin=False):
        if not code.strip():
            return {'status': 'ok', 'execution_count': self.execution_count,
                    'payload': [], 'user_expressions': {}}
        try:
            output = self._dispatch(code.strip())
            if not silent:
                self.send_response(self.iopub_socket, 'stream',
                                   {'name': 'stdout', 'text': output + '\n'})
            return {'status': 'ok', 'execution_count': self.execution_count,
                    'payload': [], 'user_expressions': {}}
        except Exception as exc:
            if not silent:
                self.send_response(self.iopub_socket, 'stream', {
                    'name': 'stderr',
                    'text': f"{type(exc).__name__}: {exc}\n{traceback.format_exc()}",
                })
            return {
                'status': 'error',
                'execution_count': self.execution_count,
                'ename': type(exc).__name__,
                'evalue': str(exc),
                'traceback': traceback.format_exc().split('\n'),
            }

    # ------------------------------------------------------------------
    # Dispatch
    # ------------------------------------------------------------------

    def _dispatch(self, code):
        if code.startswith('#subject'):
            rest = code[8:].strip()
            if '\n' in rest:
                # "#subject NOUN\n<nockasm source>" — set subject then assemble
                noun_str, nasm_src = rest.split('\n', 1)
                self._cmd_subject(noun_str.strip())
                return self._assemble_eval(nasm_src.strip())
            return self._cmd_subject(rest)
        if code.startswith('#show'):
            return self._cmd_show()
        if code.startswith('#help'):
            return self._cmd_help()
        return self._assemble_eval(code)

    # ------------------------------------------------------------------
    # Kernel commands
    # ------------------------------------------------------------------

    def _cmd_subject(self, noun_str):
        if not noun_str:
            raise ValueError("#subject requires a noun argument")
        if not _PINOCHLE:
            raise RuntimeError(
                "pinochle is not installed; cannot parse a subject noun"
            )
        self.subject = _parse_noun(noun_str)
        return f"subject: {_pretty_noun(self.subject, False)}"

    def _cmd_show(self):
        lines = []
        if _PINOCHLE:
            lines.append(f"subject:    {_pretty_noun(self.subject, False)}")
        else:
            lines.append(f"subject:    {self.subject!r}")
        if self.last_expansion is not None:
            lines.append(f"expansion:  {self.last_expansion}")
        if self.last_result is not None and _PINOCHLE:
            lines.append(f"result:     {_pretty_noun(self.last_result, False)}")
        return '\n'.join(lines) if lines else "(no state)"

    def _cmd_help(self):
        eval_note = (
            "pinochle installed — assembly + evaluation active."
            if _PINOCHLE else
            "pinochle not installed — only macro expansion is available.\n"
            "  pip install pinochle  to enable evaluation."
        )
        return f"""\
Nockasm Kernel  ({eval_note})

Kernel commands (# prefix):
  #subject NOUN      Set the evaluation subject noun
                       e.g.  #subject [42 43 44]
  #subject NOUN      If followed by a newline + nockasm source,
    <nockasm src>    sets the subject then assembles+evaluates in one cell
  #show              Show subject, last expansion, last result
  #help              Show this message

Everything else is Nockasm source, assembled then evaluated:

  :subject SCHEMA    Bind axis names within this cell
                       SCHEMA := .name | {{ SCHEMA... }}
  (%slot N)   → [0 N]        (%self)     → [0 1]
  (%battery)  → [0 2]        (%payload)  → [0 3]
  (%sample)   → [0 6]        (%context)  → [0 7]
  (%crash)    → [0 0]
  (%const X)  → [1 X]        (%arm X)    → [1 X]
  (%eval S F) → [2 S F]      (%isa F)    → [3 F]
  (%inc F)    → [4 F]        (%eq F G)   → [5 F G]
  (%if C T E) → [6 C T E]   (%comp F G) → [7 F G]
  (%push F G) → [8 F G]      (%call N F) → [9 N F]
  (%edit N V F) → [10 [N V] F]
  (%hint T F)   → [11 T F]   (%hintd T C F) → [11 [T C] F]

  #let .name = EXPR in EXPR
  #match EXPR {{ PAT => EXPR  ...  _ => EXPR }}

Atom literals:  42   1.000   0x2a   0x1.0000   'cord'

Example session:
  #subject [42 43 44]
  (%slot 2)               ; → 42
  (%inc (%slot 2))        ; → 43

  :subject {{ .a .b .c }}
  (%eq .a .b)             ; → 1 (false — different values)
"""

    # ------------------------------------------------------------------
    # Assembly + evaluation
    # ------------------------------------------------------------------

    def _assemble_eval(self, code):
        noun = expand_to_noun(code)
        flat = print_noun(noun)
        self.last_expansion = flat
        if not _PINOCHLE:
            return flat
        formula = _to_pinochle(noun)
        result = _nock_eval(self.subject, formula)
        self.last_result = result
        return f"; {flat}\n{_pretty_noun(result, False)}"


if __name__ == '__main__':
    from ipykernel.kernelapp import IPKernelApp
    IPKernelApp.launch_instance(kernel_class=NockasmKernel)
