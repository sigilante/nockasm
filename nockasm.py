"""
nockasm: a thin macro expander from Nock Assembly to canonical Nock 4K.

Target: emits whitespace-separated, right-associated bracketed Nock
parseable by pinochle.parse_noun(). Use the assembler's output as the
:formula in a pinochle Jupyter kernel cell.

Syntax
======

  ; comments to end-of-line
  ; declare a subject axis schema (optional; if omitted, only raw cells
  ; and %opcode forms work — no .name references):

    :subject SCHEMA
    SCHEMA  := LEAF | "{" LEAF+ "}"
    LEAF    := ".name" | SCHEMA

  ; flat schema lists are right-leaning by Hoon convention:
  ;   {.a .b}        -> .a=2, .b=3
  ;   {.a .b .c}     -> .a=2, .b=6, .c=7
  ;   {{.a .b} .c}   -> .a=4, .b=5, .c=3

  ; named opcodes (the only macro that's just lexing):
    (%slot N)       -> [0 N]
    (%self)         -> [0 1]               ; whole subject
    (%battery)      -> [0 2]               ; standard core battery axis
    (%payload)      -> [0 3]               ; standard core payload axis
    (%sample)       -> [0 6]               ; standard gate sample axis
    (%context)      -> [0 7]               ; standard gate context axis
    (%crash)        -> [0 0]               ; Nock crash idiom
    (%const X)      -> [1 X]
    (%arm X)        -> [1 X]               ; intent-marker for callable formula
    (%eval S F)     -> [2 S F]
    (%isa F)        -> [3 F]
    (%inc F)        -> [4 F]
    (%eq F G)       -> [5 F G]
    (%if C T E)     -> [6 C T E]
    (%comp F G)     -> [7 F G]
    (%push F G)     -> [8 F G]
    (%call N F)     -> [9 N F]
    (%edit N V F)   -> [10 [N V] F]
    (%hint T F)     -> [11 T F]            ; static hint
    (%hintd T C F)  -> [11 [T C] F]        ; dynamic hint

  ; structural macros:
    #let .name = EXPR in EXPR
        ; pushes EXPR onto the subject via opcode 8, binds .name to
        ; axis 2 in the body, shifts existing names rightward via +peg

    #match EXPR { PAT => EXPR ... _ => EXPR }
        ; evaluates EXPR once via opcode 8, then nested opcode-6
        ; dispatches against each literal PAT; default _ is required

  ; axis references:
    .name           -> [0 axis]  ; axis from current schema

  ; literal cells (no macro expansion inside, but sub-EXPRs are still
  ; expanded — use this to write raw Nock and to compose):
    [a b c ...]

  ; atom literals:
    42        ; decimal
    1.000     ; decimal with thousands separator (Hoon-style)
    0x2a      ; hex
    0x1.0000  ; hex with separator
    'cord'    ; little-endian byte-packed natural

API
===

  expand(src, *, pretty=False) -> str

  pretty=False (default): canonical flat form, e.g. "[8 [4 0 1] 5 [0 2] 0 3]"
  pretty=True: explicit binary cells, e.g. "[8 [[4 [0 1]] [5 [[0 2] [0 3]]]]]"

  expand_to_noun(src) -> Noun
      Returns the Python nested-tuple noun directly, for in-process use
      (e.g., feeding pinochle without an intermediate string round-trip).

Discipline
==========

The canonical Nock 4K specification is the truth. This assembler is a
text-to-canonical-tree convenience. Where this module's behavior would
contradict Nock 4K, Nock 4K wins; file a bug.
"""

from __future__ import annotations
import re
import sys
from typing import Union, Tuple, Dict, List, Optional, Any


# ----------------------------------------------------------------------
# Noun representation
# ----------------------------------------------------------------------

# A Noun is either an int (atom) or a (head, tail) tuple (cell).
Noun = Union[int, Tuple[Any, Any]]


def cell(*elems) -> Noun:
    """Build a right-associated cell from >=2 elements."""
    if len(elems) < 2:
        raise ValueError("cell needs at least 2 elements")
    if len(elems) == 2:
        return (elems[0], elems[1])
    return (elems[0], cell(*elems[1:]))


def peg(a: int, b: int) -> int:
    """Hoon's +peg: re-root axis b inside subtree at axis a.

    peg(a, 1)       = a
    peg(a, 2n)      = 2 * peg(a, n)
    peg(a, 2n+1)    = 2 * peg(a, n) + 1
    """
    if b < 1:
        raise ValueError("axis must be >= 1")
    if b == 1:
        return a
    if b % 2 == 0:
        return 2 * peg(a, b // 2)
    return 2 * peg(a, b // 2) + 1


def cord_to_nat(s: str) -> int:
    """Pack an ASCII string as a little-endian natural."""
    n = 0
    for i, ch in enumerate(s):
        n |= ord(ch) << (8 * i)
    return n


# ----------------------------------------------------------------------
# Tokenizer
# ----------------------------------------------------------------------

class Token:
    __slots__ = ('kind', 'value', 'line', 'col')

    def __init__(self, kind, value, line, col):
        self.kind, self.value, self.line, self.col = kind, value, line, col

    def __repr__(self):
        return f"Token({self.kind}, {self.value!r}, L{self.line}:C{self.col})"


# Order matters: longer/more specific patterns first.
_TOKEN_RE = re.compile(r'''
      (?P<COMMENT>   ;[^\n]* )
    | (?P<WS>        [ \t\n\r]+ )
    | (?P<ARROW>     => )
    | (?P<LPAREN>    \( )
    | (?P<RPAREN>    \) )
    | (?P<LCURLY>    \{ )
    | (?P<RCURLY>    \} )
    | (?P<LBRACK>    \[ )
    | (?P<RBRACK>    \] )
    | (?P<EQUALS>    = )
    | (?P<UNDER>     _ )
    | (?P<CORD>      '[^']*' )
    | (?P<HEX>       0x[0-9a-fA-F][0-9a-fA-F._]* )
    | (?P<DEC>       [0-9][0-9_.]* )
    | (?P<AXIS>      \.[A-Za-z_][A-Za-z0-9_-]* )
    | (?P<OPCODE>    %[A-Za-z_][A-Za-z0-9_-]* )
    | (?P<MACRO>     \#[A-Za-z_][A-Za-z0-9_-]* )
    | (?P<DIRECTIVE> :[A-Za-z_][A-Za-z0-9_-]* )
    | (?P<IDENT>     [A-Za-z][A-Za-z0-9_-]* )
''', re.VERBOSE)


def tokenize(src: str) -> List[Token]:
    tokens = []
    i = 0
    line = 1
    col = 1
    while i < len(src):
        m = _TOKEN_RE.match(src, i)
        if m is None:
            raise SyntaxError(
                f"unexpected character {src[i]!r} at line {line} col {col}"
            )
        kind = m.lastgroup
        text = m.group(0)
        if kind not in ('COMMENT', 'WS'):
            tokens.append(Token(kind, text, line, col))
        for ch in text:
            if ch == '\n':
                line += 1
                col = 1
            else:
                col += 1
        i = m.end()
    return tokens


# ----------------------------------------------------------------------
# AST
# ----------------------------------------------------------------------

class Node:
    pass


class IntAtom(Node):
    def __init__(self, n): self.n = n
    def __repr__(self): return f"IntAtom({self.n})"


class CordAtom(Node):
    def __init__(self, s): self.s = s
    def __repr__(self): return f"CordAtom({self.s!r})"


class AxisRef(Node):
    def __init__(self, name): self.name = name
    def __repr__(self): return f"AxisRef({self.name!r})"


class RawCell(Node):
    def __init__(self, elems): self.elems = elems
    def __repr__(self): return f"RawCell({self.elems})"


class OpApp(Node):
    def __init__(self, op, args):
        self.op, self.args = op, args
    def __repr__(self):
        return f"OpApp({self.op}, {self.args})"


class LetForm(Node):
    def __init__(self, name, value, body):
        self.name, self.value, self.body = name, value, body
    def __repr__(self):
        return f"LetForm({self.name!r}, {self.value}, {self.body})"


class MatchForm(Node):
    def __init__(self, scrutinee, cases, default):
        self.scrutinee, self.cases, self.default = scrutinee, cases, default
    def __repr__(self):
        return f"MatchForm({self.scrutinee}, {self.cases}, {self.default})"


# Schema is represented as either a string (".name") or a 2-tuple
# (head_schema, tail_schema).


# ----------------------------------------------------------------------
# Parser
# ----------------------------------------------------------------------

class Parser:
    def __init__(self, tokens: List[Token]):
        self.tokens = tokens
        self.i = 0

    def _peek(self, k: int = 0) -> Optional[Token]:
        j = self.i + k
        if j >= len(self.tokens):
            return None
        return self.tokens[j]

    def _advance(self) -> Token:
        t = self.tokens[self.i]
        self.i += 1
        return t

    def _expect(self, kind: str) -> Token:
        t = self._peek()
        if t is None:
            raise SyntaxError(f"expected {kind}, got EOF")
        if t.kind != kind:
            raise SyntaxError(
                f"expected {kind}, got {t.kind} {t.value!r} at "
                f"L{t.line}:C{t.col}"
            )
        return self._advance()

    def parse_program(self):
        """Returns (schema_or_None, expr)."""
        schema = None
        t = self._peek()
        if t is not None and t.kind == 'DIRECTIVE' and t.value == ':subject':
            self._advance()
            schema = self._parse_schema()
        expr = self._parse_expr()
        trailing = self._peek()
        if trailing is not None:
            raise SyntaxError(
                f"trailing tokens after expression: {trailing!r}"
            )
        return schema, expr

    def _parse_schema(self):
        t = self._peek()
        if t is None:
            raise SyntaxError("expected schema, got EOF")
        if t.kind == 'AXIS':
            self._advance()
            return t.value
        if t.kind == 'LCURLY':
            self._advance()
            leaves = []
            while True:
                t2 = self._peek()
                if t2 is None:
                    raise SyntaxError("unterminated schema")
                if t2.kind == 'RCURLY':
                    self._advance()
                    break
                leaves.append(self._parse_schema())
            if not leaves:
                raise SyntaxError("empty schema {}")
            if len(leaves) == 1:
                return leaves[0]
            # Right-leaning cons of >=2 leaves
            acc = leaves[-1]
            for leaf in reversed(leaves[:-1]):
                acc = (leaf, acc)
            return acc
        raise SyntaxError(
            f"expected schema, got {t.kind} {t.value!r} at "
            f"L{t.line}:C{t.col}"
        )

    def _parse_expr(self) -> Node:
        t = self._peek()
        if t is None:
            raise SyntaxError("unexpected EOF in expression")
        if t.kind == 'DEC':
            self._advance()
            return IntAtom(int(t.value.replace('_', '').replace('.', '')))
        if t.kind == 'HEX':
            self._advance()
            return IntAtom(int(
                t.value[2:].replace('.', '').replace('_', ''), 16
            ))
        if t.kind == 'CORD':
            self._advance()
            return CordAtom(t.value[1:-1])
        if t.kind == 'AXIS':
            self._advance()
            return AxisRef(t.value)
        if t.kind == 'LBRACK':
            return self._parse_raw_cell()
        if t.kind == 'LPAREN':
            return self._parse_op_app()
        if t.kind == 'MACRO':
            return self._parse_macro()
        raise SyntaxError(
            f"unexpected {t.kind} {t.value!r} at L{t.line}:C{t.col}"
        )

    def _parse_raw_cell(self) -> RawCell:
        self._expect('LBRACK')
        elems = []
        while True:
            t = self._peek()
            if t is None:
                raise SyntaxError("unterminated raw cell [")
            if t.kind == 'RBRACK':
                self._advance()
                break
            elems.append(self._parse_expr())
        if len(elems) < 2:
            raise SyntaxError("raw cell needs >=2 elements")
        return RawCell(elems)

    def _parse_op_app(self) -> OpApp:
        self._expect('LPAREN')
        t = self._peek()
        if t is None or t.kind != 'OPCODE':
            raise SyntaxError(
                f"expected %opcode after '(', got {t!r}"
            )
        op = self._advance().value
        args = []
        while True:
            t = self._peek()
            if t is None:
                raise SyntaxError("unterminated (")
            if t.kind == 'RPAREN':
                self._advance()
                break
            args.append(self._parse_expr())
        return OpApp(op, args)

    def _parse_macro(self) -> Node:
        m = self._advance()
        if m.value == '#let':
            name = self._expect('AXIS').value
            self._expect('EQUALS')
            value = self._parse_expr()
            kw = self._expect('IDENT')
            if kw.value != 'in':
                raise SyntaxError(
                    f"expected 'in' after #let value, got {kw.value!r}"
                )
            body = self._parse_expr()
            return LetForm(name, value, body)
        if m.value == '#match':
            scrutinee = self._parse_expr()
            self._expect('LCURLY')
            cases = []
            default = None
            while True:
                t = self._peek()
                if t is None:
                    raise SyntaxError("unterminated #match {")
                if t.kind == 'RCURLY':
                    self._advance()
                    break
                if t.kind == 'UNDER':
                    self._advance()
                    self._expect('ARROW')
                    if default is not None:
                        raise SyntaxError("duplicate _ in #match")
                    default = self._parse_expr()
                else:
                    pat = self._parse_expr()
                    self._expect('ARROW')
                    body = self._parse_expr()
                    cases.append((pat, body))
            if default is None:
                raise SyntaxError("#match requires a `_ => ...` default")
            return MatchForm(scrutinee, cases, default)
        raise SyntaxError(f"unknown macro {m.value}")


# ----------------------------------------------------------------------
# Expander
# ----------------------------------------------------------------------

class Expander:
    # Per-opcode argument kinds:
    #   'f' = formula position (bare atoms lifted to [1 atom])
    #   'n' = noun-literal position (no lift; arbitrary noun)
    #   'a' = axis position (must be atom; no lift)
    OPS = {
        # name -> (kinds, builder)
        # 0-arg axis aliases for the standard Hoon core layout.
        '%self':    ('',    lambda a: cell(0, 1)),
        '%battery': ('',    lambda a: cell(0, 2)),
        '%payload': ('',    lambda a: cell(0, 3)),
        '%sample':  ('',    lambda a: cell(0, 6)),
        '%context': ('',    lambda a: cell(0, 7)),
        '%slot':    ('a',   lambda a: cell(0, a[0])),
        '%crash':   ('',    lambda a: cell(0, 0)),
        '%const':   ('n',   lambda a: cell(1, a[0])),
        '%arm':     ('n',   lambda a: cell(1, a[0])),
        '%eval':  ('ff',  lambda a: cell(2, a[0], a[1])),
        '%isa':   ('f',   lambda a: cell(3, a[0])),
        '%inc':   ('f',   lambda a: cell(4, a[0])),
        '%eq':    ('ff',  lambda a: cell(5, a[0], a[1])),
        '%if':    ('fff', lambda a: cell(6, a[0], a[1], a[2])),
        '%comp':  ('ff',  lambda a: cell(7, a[0], a[1])),
        '%push':  ('ff',  lambda a: cell(8, a[0], a[1])),
        '%call':  ('af',  lambda a: cell(9, a[0], a[1])),
        '%edit':  ('aff', lambda a: cell(10, (a[0], a[1]), a[2])),
        '%hint':  ('nf',  lambda a: cell(11, a[0], a[1])),
        '%hintd': ('nff', lambda a: cell(11, (a[0], a[1]), a[2])),
    }

    def expand_program(self, schema, expr) -> Noun:
        if schema is not None:
            axes = self._resolve_schema(schema, 1)
        else:
            axes = {}
        return self._expand(expr, axes)

    def _resolve_schema(self, schema, base_axis: int) -> Dict[str, int]:
        if isinstance(schema, str):
            return {schema: base_axis}
        head, tail = schema
        d = {}
        d.update(self._resolve_schema(head, 2 * base_axis))
        for k, v in self._resolve_schema(tail, 2 * base_axis + 1).items():
            if k in d:
                raise SyntaxError(f"duplicate name in schema: {k}")
            d[k] = v
        return d

    def _shift_right(self, axes: Dict[str, int]) -> Dict[str, int]:
        """When subject becomes [new old], old axis n -> peg(3, n)."""
        return {name: peg(3, ax) for name, ax in axes.items()}

    @staticmethod
    def _lift(n: Noun) -> Noun:
        """If n is a bare atom, wrap it as the const-formula [1 atom]."""
        if isinstance(n, int):
            return cell(1, n)
        return n

    def _formula(self, e: Node, axes: Dict[str, int]) -> Noun:
        """Expand e and ensure the result is a valid Nock formula
        (lifts bare atoms via [1 atom])."""
        return self._lift(self._expand(e, axes))

    def _expand(self, e: Node, axes: Dict[str, int]) -> Noun:
        if isinstance(e, IntAtom):
            return e.n
        if isinstance(e, CordAtom):
            return cord_to_nat(e.s)
        if isinstance(e, AxisRef):
            if e.name not in axes:
                raise NameError(
                    f"unbound axis {e.name}; declared: "
                    f"{sorted(axes.keys()) or '(no :subject)'}"
                )
            return cell(0, axes[e.name])
        if isinstance(e, RawCell):
            return cell(*[self._expand(x, axes) for x in e.elems])
        if isinstance(e, OpApp):
            spec = self.OPS.get(e.op)
            if spec is None:
                raise NameError(f"unknown opcode {e.op}")
            kinds, build = spec
            if len(e.args) != len(kinds):
                raise TypeError(
                    f"{e.op} takes {len(kinds)} args, got {len(e.args)}"
                )
            compiled = []
            for arg, kind in zip(e.args, kinds):
                v = self._expand(arg, axes)
                if kind == 'f':
                    v = self._lift(v)
                elif kind == 'a':
                    if not isinstance(v, int):
                        raise TypeError(
                            f"{e.op}: axis argument must be an atom, "
                            f"got cell {v!r}"
                        )
                # 'n' accepts any noun unchanged
                compiled.append(v)
            return build(compiled)
        if isinstance(e, LetForm):
            # value compiled against OLD subject; lifted as formula
            v = self._formula(e.value, axes)
            new_axes = self._shift_right(axes)
            if e.name in new_axes:
                raise SyntaxError(f"#let shadows existing name {e.name}")
            new_axes[e.name] = 2
            b = self._formula(e.body, new_axes)
            return cell(8, v, b)
        if isinstance(e, MatchForm):
            # Lift scrutinee via opcode 8 so it evaluates once; dispatch
            # in augmented subject with scrutinee at axis 2.
            s = self._formula(e.scrutinee, axes)
            new_axes = self._shift_right(axes)
            s_ref = cell(0, 2)
            default = self._formula(e.default, new_axes)
            result: Noun = default
            for pat, body in reversed(e.cases):
                # Pattern is a literal noun (no lift — it's the value
                # being compared, wrapped in [1 ...] below).
                pat_val = self._expand(pat, new_axes)
                body_val = self._formula(body, new_axes)
                # if pat == scrutinee then body else result
                result = cell(6,
                              cell(5, cell(1, pat_val), s_ref),
                              body_val,
                              result)
            return cell(8, s, result)
        raise TypeError(f"unknown AST node {e!r}")


# ----------------------------------------------------------------------
# Printer
# ----------------------------------------------------------------------

def print_noun(n: Noun, pretty: bool = False) -> str:
    if isinstance(n, int):
        return str(n)
    if pretty:
        return f"[{print_noun(n[0], pretty)} {print_noun(n[1], pretty)}]"
    # Canonical flat right-spine form
    elems = []
    while isinstance(n, tuple):
        elems.append(n[0])
        n = n[1]
    elems.append(n)
    return '[' + ' '.join(print_noun(e, pretty) for e in elems) + ']'


# ----------------------------------------------------------------------
# Public API
# ----------------------------------------------------------------------

def expand_to_noun(src: str) -> Noun:
    toks = tokenize(src)
    schema, expr = Parser(toks).parse_program()
    return Expander().expand_program(schema, expr)


def expand(src: str, *, pretty: bool = False) -> str:
    return print_noun(expand_to_noun(src), pretty=pretty)


# ----------------------------------------------------------------------
# CLI
# ----------------------------------------------------------------------

def _cli(argv):
    pretty = '--pretty' in argv
    paths = [a for a in argv[1:] if not a.startswith('--')]
    if paths:
        for p in paths:
            with open(p) as f:
                src = f.read()
            print(expand(src, pretty=pretty))
    else:
        src = sys.stdin.read()
        print(expand(src, pretty=pretty))


if __name__ == '__main__':
    _cli(sys.argv)
