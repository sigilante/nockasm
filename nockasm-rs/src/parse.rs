//! The parser: recursive descent over the token stream, producing the
//! typed IR of [`crate::ast`].
//!
//! Acceptance matches the reference implementations through the composed
//! `expand`: everything the Python/Hoon parsers accept parses here, except
//! that ill-formed opcode applications (unknown names, wrong arities,
//! non-atom axis arguments) are rejected *now* rather than at lower time —
//! the typed [`Op`] cannot represent them. Sources that only fail at lower
//! time in the reference fail at parse time here; either way `expand`
//! rejects exactly the same set.

use crate::ast::{MatchArm, Nasm, Op, Program, Schema};
use crate::error::{ParseError, ParseErrorKind, Pos};
use crate::lex::{tokenize, Tok, Token};
use crate::noun::{Noun, NounRef};

/// Maximum expression/schema nesting depth.
///
/// The parser is the crate's one remaining recursive stage (`lift`,
/// `lower`, and `render` run on explicit stacks), so this bounds only
/// its own frames: comfortably within a 2 MiB thread stack even in
/// debug builds, with headroom to spare (see `tests/depth.rs`), and
/// beyond any practical source — the reference Python dies from
/// recursion depth around 330 for the full pipeline. Hostile nesting is
/// a clean [`TooDeep`](crate::ParseErrorKind::TooDeep) error, never a
/// stack overflow. Deeper programs belong in the IR API, which has no
/// depth bound.
pub const MAX_DEPTH: usize = 400;

/// Parse `.nasm` source into a [`Program`].
pub fn parse(src: &str) -> Result<Program, ParseError> {
    let tokens = tokenize(src)?;
    let mut p = Parser {
        tokens,
        i: 0,
        depth: 0,
    };
    let program = p.parse_program()?;
    if let Some(t) = p.peek() {
        return Err(ParseError {
            kind: ParseErrorKind::TrailingTokens {
                found: t.tok.describe(),
            },
            pos: Some(t.pos),
        });
    }
    Ok(program)
}

struct Parser {
    tokens: Vec<Token>,
    i: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.i)
    }

    fn advance(&mut self) -> &Token {
        let t = &self.tokens[self.i];
        self.i += 1;
        t
    }

    fn eof(&self, wanted: &'static str) -> ParseError {
        ParseError {
            kind: ParseErrorKind::UnexpectedEof { wanted },
            pos: None,
        }
    }

    fn unexpected(&self, wanted: &'static str, t: &Token) -> ParseError {
        ParseError {
            kind: ParseErrorKind::UnexpectedToken {
                wanted,
                found: t.tok.describe(),
            },
            pos: Some(t.pos),
        }
    }

    fn parse_program(&mut self) -> Result<Program, ParseError> {
        let mut schema = None;
        if let Some(t) = self.peek() {
            if matches!(&t.tok, Tok::Directive(d) if d == "subject") {
                self.advance();
                schema = Some(self.parse_schema()?);
            }
        }
        let body = self.parse_expr()?;
        Ok(Program { schema, body })
    }

    fn parse_schema(&mut self) -> Result<Schema, ParseError> {
        self.depth += 1;
        let r = self.parse_schema_inner();
        self.depth -= 1;
        r
    }

    fn parse_schema_inner(&mut self) -> Result<Schema, ParseError> {
        if self.depth > MAX_DEPTH {
            return Err(ParseError {
                kind: ParseErrorKind::TooDeep,
                pos: None,
            });
        }
        let Some(t) = self.peek() else {
            return Err(self.eof("a schema"));
        };
        match &t.tok {
            Tok::AxisName(n) => {
                let n = n.clone();
                self.advance();
                Ok(Schema::Leaf(n))
            }
            Tok::Under => {
                self.advance();
                Ok(Schema::Hole)
            }
            Tok::LCurly => {
                let open_pos = t.pos;
                self.advance();
                let mut leaves = Vec::new();
                loop {
                    match self.peek() {
                        None => return Err(self.eof("'}' or a schema")),
                        Some(t) if t.tok == Tok::RCurly => {
                            self.advance();
                            break;
                        }
                        Some(_) => leaves.push(self.parse_schema()?),
                    }
                }
                if leaves.is_empty() {
                    return Err(ParseError {
                        kind: ParseErrorKind::EmptySchema,
                        pos: Some(open_pos),
                    });
                }
                // Right-leaning cons, per Hoon convention.
                let mut acc = leaves.pop().expect("nonempty");
                for leaf in leaves.into_iter().rev() {
                    acc = Schema::Pair(Box::new(leaf), Box::new(acc));
                }
                Ok(acc)
            }
            _ => Err(self.unexpected("a schema", t)),
        }
    }

    /// The recursion point. Kept to two slim frames per nesting level
    /// (this dispatcher plus one construct helper) so that even debug
    /// builds at [`MAX_DEPTH`] stay comfortably inside a 2 MiB thread
    /// stack: all fat locals live in the per-construct helpers.
    fn parse_expr(&mut self) -> Result<Nasm, ParseError> {
        if self.depth >= MAX_DEPTH {
            return Err(ParseError {
                kind: ParseErrorKind::TooDeep,
                pos: None,
            });
        }
        self.depth += 1;
        enum Next {
            Leaf,
            Cell,
            Op,
            Macro,
            Other,
        }
        let next = match self.peek().map(|t| &t.tok) {
            Some(Tok::Num(_) | Tok::Cord(_) | Tok::AxisName(_)) => Next::Leaf,
            Some(Tok::LBrack) => Next::Cell,
            Some(Tok::LParen) => Next::Op,
            Some(Tok::MacroName(_)) => Next::Macro,
            _ => Next::Other,
        };
        let r = match next {
            Next::Leaf => self.parse_leaf(),
            Next::Cell => self.parse_raw_cell(),
            Next::Op => self.parse_op_app(),
            Next::Macro => self.parse_macro(),
            Next::Other => self.expr_expected(),
        };
        self.depth -= 1;
        r
    }

    #[inline(never)]
    fn parse_leaf(&mut self) -> Result<Nasm, ParseError> {
        let t = self.advance();
        match &t.tok {
            Tok::Num(a) | Tok::Cord(a) => Ok(Nasm::Atom(a.clone())),
            Tok::AxisName(n) => Ok(Nasm::Axis(n.clone())),
            _ => unreachable!("caller matched a leaf token"),
        }
    }

    #[inline(never)]
    fn expr_expected(&mut self) -> Result<Nasm, ParseError> {
        match self.peek() {
            None => Err(self.eof("an expression")),
            Some(t) => Err(self.unexpected("an expression", t)),
        }
    }

    fn parse_raw_cell(&mut self) -> Result<Nasm, ParseError> {
        let open_pos = self.advance().pos; // the '['
        let mut elems = Vec::new();
        loop {
            match self.peek() {
                None => return Err(self.eof("']' or an expression")),
                Some(t) if t.tok == Tok::RBrack => {
                    self.advance();
                    break;
                }
                Some(_) => elems.push(self.parse_expr()?),
            }
        }
        Nasm::raw_cell(elems).ok_or(ParseError {
            kind: ParseErrorKind::RawCellTooFew,
            pos: Some(open_pos),
        })
    }

    fn parse_op_app(&mut self) -> Result<Nasm, ParseError> {
        self.advance(); // the '('
        let (name, name_pos) = match self.peek() {
            None => return Err(self.eof("a %opcode after '('")),
            Some(t) => match &t.tok {
                Tok::OpName(n) => {
                    let pair = (n.clone(), t.pos);
                    self.advance();
                    pair
                }
                _ => return Err(self.unexpected("a %opcode after '('", t)),
            },
        };
        if name == "nock" {
            let payload = self.parse_noun_literal()?;
            match self.peek() {
                None => return Err(self.eof("')'")),
                Some(t) if t.tok == Tok::RParen => {
                    self.advance();
                }
                Some(t) => return Err(self.unexpected("')'", t)),
            }
            return Ok(Nasm::Nock(payload));
        }
        let mut args = Vec::new();
        loop {
            match self.peek() {
                None => return Err(self.eof("')' or an expression")),
                Some(t) if t.tok == Tok::RParen => {
                    self.advance();
                    break;
                }
                Some(_) => args.push(self.parse_expr()?),
            }
        }
        Ok(Nasm::Op(build_op(&name, args, name_pos)?))
    }

    /// The `(%nock ...)` payload: atom literals and `[...]` cells of
    /// noun literals only — never an expression. The payload is data
    /// to the assembler; nothing in it expands.
    fn parse_noun_literal(&mut self) -> Result<Noun, ParseError> {
        if self.depth >= MAX_DEPTH {
            return Err(ParseError {
                kind: ParseErrorKind::TooDeep,
                pos: None,
            });
        }
        self.depth += 1;
        let r = self.parse_noun_literal_inner();
        self.depth -= 1;
        r
    }

    #[inline(never)]
    fn parse_noun_literal_inner(&mut self) -> Result<Noun, ParseError> {
        let Some(t) = self.peek() else {
            return Err(self.eof("a noun literal in (%nock ...)"));
        };
        match &t.tok {
            Tok::Num(a) | Tok::Cord(a) => {
                let a = a.clone();
                self.advance();
                Ok(Noun::from(a))
            }
            Tok::LBrack => {
                let open_pos = t.pos;
                self.advance();
                let mut elems = Vec::new();
                loop {
                    match self.peek() {
                        None => return Err(self.eof("']' or a noun literal")),
                        Some(t) if t.tok == Tok::RBrack => {
                            self.advance();
                            break;
                        }
                        Some(_) => elems.push(self.parse_noun_literal()?),
                    }
                }
                Noun::autocons(elems).ok_or(ParseError {
                    kind: ParseErrorKind::NockPayloadTooFew,
                    pos: Some(open_pos),
                })
            }
            _ => Err(self.unexpected("a noun literal in (%nock ...)", t)),
        }
    }

    fn parse_macro(&mut self) -> Result<Nasm, ParseError> {
        let t = self.advance();
        let macro_pos = t.pos;
        let Tok::MacroName(name) = &t.tok else {
            unreachable!("caller matched MacroName");
        };
        match name.as_str() {
            "let" => {
                let bind = match self.peek() {
                    None => return Err(self.eof("an axis name after #let")),
                    Some(t) => match &t.tok {
                        Tok::AxisName(n) => {
                            let n = n.clone();
                            self.advance();
                            n
                        }
                        _ => return Err(self.unexpected("an axis name after #let", t)),
                    },
                };
                match self.peek() {
                    None => return Err(self.eof("'='")),
                    Some(t) if t.tok == Tok::Equals => {
                        self.advance();
                    }
                    Some(t) => return Err(self.unexpected("'='", t)),
                }
                let value = self.parse_expr()?;
                match self.peek() {
                    None => return Err(self.eof("'in' after the #let value")),
                    Some(t) if matches!(&t.tok, Tok::Ident(s) if s == "in") => {
                        self.advance();
                    }
                    Some(t) => return Err(self.unexpected("'in' after the #let value", t)),
                }
                let body = self.parse_expr()?;
                Ok(Nasm::Let {
                    name: bind,
                    value: Box::new(value),
                    body: Box::new(body),
                })
            }
            "match" => {
                let scrutinee = self.parse_expr()?;
                match self.peek() {
                    None => return Err(self.eof("'{'")),
                    Some(t) if t.tok == Tok::LCurly => {
                        self.advance();
                    }
                    Some(t) => return Err(self.unexpected("'{'", t)),
                }
                let mut arms = Vec::new();
                let mut default: Option<Nasm> = None;
                loop {
                    match self.peek() {
                        None => return Err(self.eof("'}' or a #match arm")),
                        Some(t) if t.tok == Tok::RCurly => {
                            self.advance();
                            break;
                        }
                        Some(t) if t.tok == Tok::Under => {
                            let under_pos = t.pos;
                            self.advance();
                            match self.peek() {
                                None => return Err(self.eof("'=>'")),
                                Some(t) if t.tok == Tok::Arrow => {
                                    self.advance();
                                }
                                Some(t) => return Err(self.unexpected("'=>'", t)),
                            }
                            if default.is_some() {
                                return Err(ParseError {
                                    kind: ParseErrorKind::MatchDuplicateDefault,
                                    pos: Some(under_pos),
                                });
                            }
                            default = Some(self.parse_expr()?);
                        }
                        Some(_) => {
                            let pattern = self.parse_expr()?;
                            match self.peek() {
                                None => return Err(self.eof("'=>'")),
                                Some(t) if t.tok == Tok::Arrow => {
                                    self.advance();
                                }
                                Some(t) => return Err(self.unexpected("'=>'", t)),
                            }
                            let body = self.parse_expr()?;
                            arms.push(MatchArm { pattern, body });
                        }
                    }
                }
                let Some(default) = default else {
                    return Err(ParseError {
                        kind: ParseErrorKind::MatchNeedsDefault,
                        pos: Some(macro_pos),
                    });
                };
                Ok(Nasm::Match {
                    scrutinee: Box::new(scrutinee),
                    arms,
                    default: Box::new(default),
                })
            }
            _ => Err(ParseError {
                kind: ParseErrorKind::UnknownMacro(name.clone()),
                pos: Some(macro_pos),
            }),
        }
    }
}

/// Validate an opcode application and build the typed [`Op`]. Checks in
/// the reference order: name, then arity, then axis-argument kinds.
fn build_op(name: &str, mut args: Vec<Nasm>, pos: Pos) -> Result<Op, ParseError> {
    let (op, want): (&'static str, usize) = match name {
        "self" => ("self", 0),
        "battery" => ("battery", 0),
        "payload" => ("payload", 0),
        "sample" => ("sample", 0),
        "context" => ("context", 0),
        "crash" => ("crash", 0),
        "slot" => ("slot", 1),
        "const" => ("const", 1),
        "arm" => ("arm", 1),
        "isa" => ("isa", 1),
        "inc" => ("inc", 1),
        "eval" => ("eval", 2),
        "eq" => ("eq", 2),
        "comp" => ("comp", 2),
        "push" => ("push", 2),
        "call" => ("call", 2),
        "hint" => ("hint", 2),
        "if" => ("if", 3),
        "edit" => ("edit", 3),
        "hintd" => ("hintd", 3),
        _ => {
            return Err(ParseError {
                kind: ParseErrorKind::UnknownOpcode(name.to_string()),
                pos: Some(pos),
            })
        }
    };
    if args.len() != want {
        return Err(ParseError {
            kind: ParseErrorKind::OpArity {
                op,
                want,
                got: args.len(),
            },
            pos: Some(pos),
        });
    }
    // Borrow-and-clone rather than destructure: Nasm implements Drop
    // (iterative teardown), which forbids moving fields out of it.
    // Cloning the atom is cheap (inline u64 or a refcount bump).
    // A `(%nock ...)` embed of an atom is also accepted: it is the one
    // other expression that expands to an atom, and the reference
    // implementations accept any such expression in axis position.
    let axis = |e: Nasm| -> Result<crate::noun::Atom, ParseError> {
        match &e {
            Nasm::Atom(a) => Ok(a.clone()),
            Nasm::Nock(n) => match n.view() {
                NounRef::Atom(a) => Ok(a.clone()),
                NounRef::Cell(..) => Err(ParseError {
                    kind: ParseErrorKind::AxisArgNotAtom { op },
                    pos: Some(pos),
                }),
            },
            _ => Err(ParseError {
                kind: ParseErrorKind::AxisArgNotAtom { op },
                pos: Some(pos),
            }),
        }
    };
    let mut it = args.drain(..);
    let mut next = || Box::new(it.next().expect("arity checked"));
    Ok(match op {
        "self" => Op::Self_,
        "battery" => Op::Battery,
        "payload" => Op::Payload,
        "sample" => Op::Sample,
        "context" => Op::Context,
        "crash" => Op::Crash,
        "slot" => Op::Slot(axis(*next())?),
        "const" => Op::Const(next()),
        "arm" => Op::Arm(next()),
        "isa" => Op::Isa(next()),
        "inc" => Op::Inc(next()),
        "eval" => Op::Eval(next(), next()),
        "eq" => Op::Eq(next(), next()),
        "comp" => Op::Comp(next(), next()),
        "push" => Op::Push(next(), next()),
        "call" => Op::Call(axis(*next())?, next()),
        "hint" => Op::Hint(next(), next()),
        "if" => Op::If(next(), next(), next()),
        "edit" => Op::Edit(axis(*next())?, next(), next()),
        "hintd" => Op::Hintd(next(), next(), next()),
        _ => unreachable!("op table above is exhaustive"),
    })
}
