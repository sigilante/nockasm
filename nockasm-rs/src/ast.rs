//! The `$nasm` target IR, as clean Rust types.
//!
//! This is the compiler-target contract of `doc/compiler-target.md`
//! (IR version [`NASM_VERSION`](crate::NASM_VERSION)), with one Rust
//! refinement: ill-formed nodes are unrepresentable. Where the Python and
//! Hoon IRs carry `%op` as a free-form name plus argument list and reject
//! unknown opcodes, wrong arities, and non-atom axis arguments at lower
//! time, [`Op`] carries exactly the well-formed applications, so those
//! rejections happen at parse (or IR-construction) time instead. The
//! composed `expand = lower ∘ parse` accepts and rejects exactly the same
//! sources as the reference implementations.

use std::fmt;

use crate::error::InvalidName;
use crate::noun::Atom;

/// A schema or binder name: `[A-Za-z_][A-Za-z0-9_-]*`, stored without the
/// leading dot.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name(String);

impl Name {
    /// Validate and build a name. The accepted grammar is exactly the
    /// reference tokenizer's: first `[A-Za-z_]`, then `[A-Za-z0-9_-]*`.
    pub fn new(s: impl Into<String>) -> Result<Name, InvalidName> {
        let s = s.into();
        let ok = match s.as_bytes() {
            [] => false,
            [first, rest @ ..] => {
                (first.is_ascii_alphabetic() || *first == b'_')
                    && rest
                        .iter()
                        .all(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
            }
        };
        if ok {
            Ok(Name(s))
        } else {
            Err(InvalidName(s))
        }
    }

    /// The name, without a leading dot.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A `:subject` axis schema (`$sema`): a named leaf or a pair of schemas.
///
/// Flat `{.a .b .c}` groups parse right-leaning by Hoon convention:
/// `Pair(a, Pair(b, c))`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Schema {
    /// `.name` — this whole subtree of the subject.
    Leaf(Name),
    /// `{head tail}` — a cell of two sub-schemas.
    Pair(Box<Schema>, Box<Schema>),
}

/// A parsed program: an optional `:subject` schema and one expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    /// The `:subject` schema, if the source declared one.
    pub schema: Option<Schema>,
    /// The program body.
    pub body: Nasm,
}

/// One `#match` arm: a literal pattern and its body.
///
/// The pattern is expanded in noun position (never lifted) and compared
/// against the scrutinee's runtime value; the body is a formula position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchArm {
    /// The literal pattern.
    pub pattern: Nasm,
    /// The arm body.
    pub body: Nasm,
}

/// A nockasm expression: the `$nasm` IR node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Nasm {
    /// An atom literal (decimal, hex, or cord — the value is already
    /// packed; rendering is a function of the value, not the spelling).
    Atom(Atom),
    /// `.name` — a reference into the subject schema.
    Axis(Name),
    /// `[a b c ...]` — a raw structural cell of two-or-more expressions,
    /// right-associated. Elements expand structurally: sub-expressions
    /// expand, but atoms are never lifted. The two mandatory elements are
    /// explicit so a unary cell is unrepresentable.
    Cell {
        /// First element.
        first: Box<Nasm>,
        /// Second element.
        second: Box<Nasm>,
        /// Any further elements.
        rest: Vec<Nasm>,
    },
    /// `(%opcode ...)` — a named opcode application.
    Op(Op),
    /// `#let name = value in body`.
    Let {
        /// The bound name (axis 2 in the body).
        name: Name,
        /// The pushed value — a formula position, compiled against the
        /// old subject.
        value: Box<Nasm>,
        /// The body — a formula position; prior names shift under axis 3.
        body: Box<Nasm>,
    },
    /// `#match scrutinee { pat => body ... _ => default }`.
    Match {
        /// The scrutinee — a formula position, evaluated once.
        scrutinee: Box<Nasm>,
        /// The literal-pattern arms, in source order.
        arms: Vec<MatchArm>,
        /// The required `_ =>` default — a formula position.
        default: Box<Nasm>,
    },
}

impl Nasm {
    /// Build a raw cell from two-or-more elements; `None` otherwise.
    pub fn raw_cell(elems: Vec<Nasm>) -> Option<Nasm> {
        let mut it = elems.into_iter();
        let first = Box::new(it.next()?);
        let second = Box::new(it.next()?);
        Some(Nasm::Cell {
            first,
            second,
            rest: it.collect(),
        })
    }

    fn is_leaf(&self) -> bool {
        matches!(self, Nasm::Atom(_) | Nasm::Axis(_))
    }
}

/// Move every non-leaf child of `node` onto `stack`, leaving a leaf in
/// its place. Leaf children stay put — they drop trivially with the
/// hollowed-out node — so harvesting an already-hollowed node pushes
/// nothing (and allocates nothing: an untouched `Vec::new` is free).
fn harvest(node: &mut Nasm, stack: &mut Vec<Nasm>) {
    fn grab(slot: &mut Nasm, stack: &mut Vec<Nasm>) {
        if !slot.is_leaf() {
            stack.push(std::mem::replace(slot, Nasm::Atom(Atom::ZERO)));
        }
    }
    match node {
        Nasm::Atom(_) | Nasm::Axis(_) => {}
        Nasm::Cell {
            first,
            second,
            rest,
        } => {
            grab(first, stack);
            grab(second, stack);
            for e in rest.iter_mut() {
                grab(e, stack);
            }
        }
        Nasm::Op(op) => match op {
            Op::Slot(_)
            | Op::Self_
            | Op::Battery
            | Op::Payload
            | Op::Sample
            | Op::Context
            | Op::Crash => {}
            Op::Const(x) | Op::Arm(x) | Op::Isa(x) | Op::Inc(x) => grab(x, stack),
            Op::Eval(a, b) | Op::Eq(a, b) | Op::Comp(a, b) | Op::Push(a, b) | Op::Hint(a, b) => {
                grab(a, stack);
                grab(b, stack);
            }
            Op::If(a, b, c) | Op::Hintd(a, b, c) => {
                grab(a, stack);
                grab(b, stack);
                grab(c, stack);
            }
            Op::Call(_, f) => grab(f, stack),
            Op::Edit(_, v, f) => {
                grab(v, stack);
                grab(f, stack);
            }
        },
        Nasm::Let { value, body, .. } => {
            grab(value, stack);
            grab(body, stack);
        }
        Nasm::Match {
            scrutinee,
            arms,
            default,
        } => {
            grab(scrutinee, stack);
            for arm in arms.iter_mut() {
                grab(&mut arm.pattern, stack);
                grab(&mut arm.body, stack);
            }
            grab(default, stack);
        }
    }
}

impl Drop for Nasm {
    /// Iterative teardown: [`lift`](crate::lift) can produce IR as deep
    /// as the noun it reads (hostile jamfiles included), so dropping
    /// must not recurse. Each popped node is hollowed into `stack`
    /// before it drops, leaving only its own boxes for the normal glue
    /// to free.
    fn drop(&mut self) {
        if self.is_leaf() {
            return;
        }
        let mut stack: Vec<Nasm> = Vec::new();
        harvest(self, &mut stack);
        while let Some(mut node) = stack.pop() {
            harvest(&mut node, &mut stack);
        }
    }
}

/// A well-formed named-opcode application.
///
/// Argument kinds follow the reference `OPS` table: *formula* positions
/// lift bare atoms to `[1 atom]` at lower time, *noun* positions expand
/// without lifting, and *axis* positions are atoms (in the reference
/// implementations an axis argument may be any expression that expands to
/// an atom — but only atom literals do, so `Atom` is the same domain).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// `(%slot N)` → `[0 N]` — axis N of the subject.
    Slot(Atom),
    /// `(%self)` → `[0 1]` — the whole subject.
    Self_,
    /// `(%battery)` → `[0 2]` — standard core battery axis.
    Battery,
    /// `(%payload)` → `[0 3]` — standard core payload axis.
    Payload,
    /// `(%sample)` → `[0 6]` — standard gate sample axis.
    Sample,
    /// `(%context)` → `[0 7]` — standard gate context axis.
    Context,
    /// `(%crash)` → `[0 0]` — the Nock crash idiom.
    Crash,
    /// `(%const X)` → `[1 X]` — X in noun position.
    Const(Box<Nasm>),
    /// `(%arm X)` → `[1 X]` — `%const` with intent: X is a formula that
    /// will later be invoked via `%call`.
    Arm(Box<Nasm>),
    /// `(%eval S F)` → `[2 S F]`.
    Eval(Box<Nasm>, Box<Nasm>),
    /// `(%isa F)` → `[3 F]`.
    Isa(Box<Nasm>),
    /// `(%inc F)` → `[4 F]`.
    Inc(Box<Nasm>),
    /// `(%eq F G)` → `[5 F G]`.
    Eq(Box<Nasm>, Box<Nasm>),
    /// `(%if C T E)` → `[6 C T E]`.
    If(Box<Nasm>, Box<Nasm>, Box<Nasm>),
    /// `(%comp F G)` → `[7 F G]`.
    Comp(Box<Nasm>, Box<Nasm>),
    /// `(%push F G)` → `[8 F G]`.
    Push(Box<Nasm>, Box<Nasm>),
    /// `(%call N F)` → `[9 N F]` — N in axis position.
    Call(Atom, Box<Nasm>),
    /// `(%edit N V F)` → `[10 [N V] F]` — N in axis position.
    Edit(Atom, Box<Nasm>, Box<Nasm>),
    /// `(%hint T F)` → `[11 T F]` — static hint; T in noun position.
    Hint(Box<Nasm>, Box<Nasm>),
    /// `(%hintd T C F)` → `[11 [T C] F]` — dynamic hint; T in noun
    /// position, C a formula (per the 4K spec the clue is evaluated).
    Hintd(Box<Nasm>, Box<Nasm>, Box<Nasm>),
}

impl Op {
    /// The opcode's source name, without the `%`.
    pub fn name(&self) -> &'static str {
        match self {
            Op::Slot(_) => "slot",
            Op::Self_ => "self",
            Op::Battery => "battery",
            Op::Payload => "payload",
            Op::Sample => "sample",
            Op::Context => "context",
            Op::Crash => "crash",
            Op::Const(_) => "const",
            Op::Arm(_) => "arm",
            Op::Eval(..) => "eval",
            Op::Isa(_) => "isa",
            Op::Inc(_) => "inc",
            Op::Eq(..) => "eq",
            Op::If(..) => "if",
            Op::Comp(..) => "comp",
            Op::Push(..) => "push",
            Op::Call(..) => "call",
            Op::Edit(..) => "edit",
            Op::Hint(..) => "hint",
            Op::Hintd(..) => "hintd",
        }
    }
}

/// A borrowed opcode argument: an expression or an axis atom. Axis atoms
/// render exactly as atom literals do.
#[derive(Clone, Copy)]
pub(crate) enum ArgRef<'a> {
    Expr(&'a Nasm),
    Axis(&'a Atom),
}

impl Op {
    /// The arguments in source order, for rendering.
    pub(crate) fn args(&self) -> Vec<ArgRef<'_>> {
        use ArgRef::{Axis, Expr};
        match self {
            Op::Self_ | Op::Battery | Op::Payload | Op::Sample | Op::Context | Op::Crash => {
                vec![]
            }
            Op::Slot(n) => vec![Axis(n)],
            Op::Const(x) | Op::Arm(x) | Op::Isa(x) | Op::Inc(x) => vec![Expr(x)],
            Op::Eval(a, b) | Op::Eq(a, b) | Op::Comp(a, b) | Op::Push(a, b) | Op::Hint(a, b) => {
                vec![Expr(a), Expr(b)]
            }
            Op::If(a, b, c) | Op::Hintd(a, b, c) => vec![Expr(a), Expr(b), Expr(c)],
            Op::Call(n, f) => vec![Axis(n), Expr(f)],
            Op::Edit(n, v, f) => vec![Axis(n), Expr(v), Expr(f)],
        }
    }
}
