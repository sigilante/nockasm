//! `noted`: the annotated instantiation of the IR vocabulary, as the
//! Hoon reference spells it.
//!
//! [`Nasm`] is the IR the rest of this crate consumes, and it is a
//! closed, typed tree: every child is another `Nasm`, and an opcode's
//! axis argument is a bare [`Atom`]. A compiler emitting nockasm
//! usually wants to carry something alongside each node — a source
//! position for diagnostics, a provenance tag — and the tempting move
//! is to fork the vocabulary: a second enum with the same variants
//! plus an annotation, drifting from the first every time a variant is
//! added.
//!
//! The Hoon reference (`desk/sur/nockasm.hoon`) avoids the fork by
//! parameterizing the vocabulary over its own recursion, so that an
//! annotated instantiation is a first-class citizen rather than a copy:
//!
//! ```text
//! ++  nasm-of  |$  [self]  $%  [%atom p=@]  [%axis p=@t]  [%cell p=(list self)]
//!                              [%op p=@t q=(list self)]  [%let p=@t q=self r=self]
//!                              [%match p=self q=(list [p=self q=self]) r=self]
//!                              [%nock p=*]  ==
//! +$  nasm   $~([%atom 0] (nasm-of nasm))                        ::  the plain knot
//! +$  noted  $~([*note [%atom 0]] [=note node=(nasm-of noted)])  ::  an annotated one
//! ```
//!
//! This module is that pattern in Rust, and it keeps the reference
//! vocabulary's one property the typed [`Nasm`] gives up: the opcode is
//! a name and its arguments are a list of nodes. [`NasmOf<N>`] is the
//! vocabulary generic over the child type, case for case with
//! `+nasm-of`; [`Noted<A>`] ties the knot through a wrapper that
//! carries a note of type `A` on every node. An emitter that
//! instantiates the Hoon builder with `[pos=(unit hair) node=(nasm-of
//! nasm)]` holds `Noted<Option<Pos>>` here — and because the axis of a
//! `%slot`, `%call`, or `%edit` is a node like any other, it carries a
//! position too, which is what a positioned emitter's conformance
//! vectors compare.
//!
//! # The strip contract
//!
//! [`Noted::strip`] is the projection onto the bare IR: drop every
//! note, check the vocabulary, recurse. It is where the reference
//! implementations' lower-time refusals live — an unknown opcode, a
//! wrong arity, a non-atom axis argument, a unary raw cell — since
//! [`Nasm`] cannot represent those and the reference vocabulary can;
//! they come back as a [`StripError`] carrying the reference crash tag.
//! [`Noted::dress`] is a section of it — every node gets the same note,
//! axis atoms become `%atom` nodes — so `Noted::dress(&n, a).strip() ==
//! Ok(n)` for every `n`. The meaning of an annotated value is the
//! meaning of its projection, `lower(schema, &noted.strip()?)`; the
//! conformance law a positioned emitter is held to is `(expand ours)
//! == (reference expansion of (strip ours))`. [`lower`] and [`render`]
//! here are exactly the bare pipeline composed with the projection, so
//! a compiler holding annotated IR never has to spell it out.
//!
//! ```
//! use nockasm::noted::{self, NasmOf, Noted};
//! use nockasm::{expand, parse, Atom};
//!
//! let src = ":subject .x  #let .d = (%inc .x) in (%slot 2)";
//! let program = parse(src).unwrap();
//! // A compiler builds `Noted` directly, positions and all: here the
//! // let at line 1, its value and the axis atom of the slot elsewhere.
//! let at = |line: u32, col: u32, node| Noted::new(Some((line, col)), node);
//! let ours = at(
//!     1,
//!     14,
//!     NasmOf::Let(
//!         "d".into(),
//!         Box::new(at(1, 23, NasmOf::Op("inc".into(), vec![at(1, 28, NasmOf::Axis("x".into()))]))),
//!         Box::new(at(1, 35, NasmOf::Op("slot".into(), vec![at(1, 41, NasmOf::Atom(Atom::from(2u64)))]))),
//!     ),
//! );
//! assert_eq!(ours.strip().unwrap(), program.body);
//! assert_eq!(
//!     noted::lower(program.schema.as_ref(), &ours).unwrap(),
//!     expand(src).unwrap(),
//! );
//! ```
//!
//! # Drift is a compile error
//!
//! `Nasm` stays a separate concrete type — it is the cross-implementation
//! contract and what every other stage consumes — so the two
//! vocabularies are kept in step by hand, and this module makes a slip
//! fail to compile rather than fail at runtime. The bridge from the
//! typed side is one private function, `unroll` (`&Nasm -> NasmOf<_>`),
//! an exhaustive match over `Nasm` and `Op` with no wildcard arm, so a
//! variant added to `Nasm` without its untyped reading breaks it; the
//! bridge back is [`roll`] (`NasmOf<Nasm> -> Result<Nasm, StripError>`),
//! exhaustive over `NasmOf` and over the opcode table, and the unit
//! tests hold the table to every `Op` variant. Everything else in the
//! module ([`strip`](Noted::strip), [`dress`](Noted::dress),
//! [`map_note`](Noted::map_note), the teardown) is built from those two
//! and from the vocabulary's own [`map`](NasmOf::map), so it inherits a
//! new variant automatically.
//!
//! # Depth
//!
//! `Noted` is a boxed tree like `Nasm` and keeps the same guarantee:
//! teardown, `strip`, `dress`, and `map_note` all run on explicit
//! stacks, so annotated IR of any depth converts and drops without
//! touching the call stack. As with `Nasm`, the *derived* `Clone`,
//! `PartialEq`, and `Debug` recurse; deep-cloning or deep-comparing
//! annotated IR is the caller's lookout.

use std::fmt;

use crate::ast::{MatchArm, Name, Nasm, Op, Schema};
use crate::error::LowerError;
use crate::noun::{Atom, Noun};

// ----------------------------------------------------------------------
// The vocabulary, generic over its recursion
// ----------------------------------------------------------------------

/// `+nasm-of`: one layer of the IR vocabulary with every child position
/// replaced by `N`, case for case with the Hoon builder.
///
/// `NasmOf<Noted<A>>` is the node of an annotated tree; `NasmOf<()>` is
/// a node's shape with its children forgotten. The opcode is a name and
/// its arguments a list: the vocabulary check ([`roll`]) is where the
/// name and the arity are held to the [`Op`] table, and an axis
/// argument is an `Atom` *node*, so it can carry a note.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NasmOf<N> {
    /// `[%atom p=@]` — an atom literal. See [`Nasm::Atom`].
    Atom(Atom),
    /// `[%axis p=@t]` — `.name`, a reference into the subject schema.
    /// See [`Nasm::Axis`]; the name is checked nowhere here (an
    /// emitter's names never pass through the text grammar).
    Axis(String),
    /// `[%cell p=(list self)]` — a raw structural cell; two or more
    /// elements once stripped. See [`Nasm::Cell`].
    Cell(Vec<N>),
    /// `[%op p=@t q=(list self)]` — `(%opcode args...)`. See
    /// [`Nasm::Op`]; the name and the arity are checked by [`roll`].
    Op(String, Vec<N>),
    /// `[%let p=@t q=self r=self]` — `#let name = value in body`. See
    /// [`Nasm::Let`].
    Let(String, Box<N>, Box<N>),
    /// `[%match p=self q=(list [p q]) r=self]` — `#match scrutinee {
    /// pattern => body ... _ => default }`. See [`Nasm::Match`].
    Match(Box<N>, Vec<(N, N)>, Box<N>),
    /// `[%nock p=*]` — an opaque embedded formula. See [`Nasm::Nock`].
    Nock(Noun),
}

impl<N> NasmOf<N> {
    /// The case name: `atom`, `axis`, `cell`, `op`, `let`, `match`, or
    /// `nock`.
    pub fn kind(&self) -> &'static str {
        match self {
            NasmOf::Atom(_) => "atom",
            NasmOf::Axis(_) => "axis",
            NasmOf::Cell(_) => "cell",
            NasmOf::Op(..) => "op",
            NasmOf::Let(..) => "let",
            NasmOf::Match(..) => "match",
            NasmOf::Nock(_) => "nock",
        }
    }

    /// Whether this layer has no children.
    pub fn is_leaf(&self) -> bool {
        match self {
            NasmOf::Atom(_) | NasmOf::Axis(_) | NasmOf::Nock(_) => true,
            NasmOf::Cell(items) => items.is_empty(),
            NasmOf::Op(_, args) => args.is_empty(),
            NasmOf::Let(..) | NasmOf::Match(..) => false,
        }
    }

    /// Rewrite every child, keeping the layer. Children are visited in
    /// their source order: a cell's elements, an op's arguments, a
    /// let's value then body, a match's scrutinee, then each arm's
    /// pattern and body, then the default. Every traversal in this
    /// module enumerates children through `map`, so no two of them can
    /// disagree about the order.
    pub fn map<M>(self, mut f: impl FnMut(N) -> M) -> NasmOf<M> {
        match self {
            NasmOf::Atom(a) => NasmOf::Atom(a),
            NasmOf::Axis(name) => NasmOf::Axis(name),
            NasmOf::Cell(items) => NasmOf::Cell(items.into_iter().map(f).collect()),
            NasmOf::Op(name, args) => NasmOf::Op(name, args.into_iter().map(f).collect()),
            NasmOf::Let(name, value, body) => {
                let value = Box::new(f(*value));
                let body = Box::new(f(*body));
                NasmOf::Let(name, value, body)
            }
            NasmOf::Match(scrutinee, arms, default) => {
                let scrutinee = Box::new(f(*scrutinee));
                let arms = arms
                    .into_iter()
                    .map(|(pattern, body)| {
                        let pattern = f(pattern);
                        let body = f(body);
                        (pattern, body)
                    })
                    .collect();
                let default = Box::new(f(*default));
                NasmOf::Match(scrutinee, arms, default)
            }
            NasmOf::Nock(noun) => NasmOf::Nock(noun),
        }
    }

    /// The same layer with the children borrowed.
    pub fn as_ref(&self) -> NasmOf<&N> {
        match self {
            NasmOf::Atom(a) => NasmOf::Atom(a.clone()),
            NasmOf::Axis(name) => NasmOf::Axis(name.clone()),
            NasmOf::Cell(items) => NasmOf::Cell(items.iter().collect()),
            NasmOf::Op(name, args) => NasmOf::Op(name.clone(), args.iter().collect()),
            NasmOf::Let(name, value, body) => {
                NasmOf::Let(name.clone(), Box::new(value), Box::new(body))
            }
            NasmOf::Match(scrutinee, arms, default) => NasmOf::Match(
                Box::new(scrutinee),
                arms.iter().map(|(p, b)| (p, b)).collect(),
                Box::new(default),
            ),
            NasmOf::Nock(noun) => NasmOf::Nock(noun.clone()),
        }
    }

    /// The children, in `map` order.
    pub fn children(&self) -> Vec<&N> {
        let mut kids = Vec::new();
        self.as_ref().map(|child| kids.push(child));
        kids
    }
}

// ----------------------------------------------------------------------
// The refusals
// ----------------------------------------------------------------------

/// The opcode table, as `Op::name` spells it.
const OPCODES: &[&str] = &[
    "slot", "self", "battery", "payload", "sample", "context", "crash", "const", "arm", "eval",
    "isa", "inc", "eq", "if", "comp", "push", "call", "edit", "hint", "hintd", "scry",
];

/// Why a [`Noted`] tree has no [`Nasm`] projection: a node the typed
/// vocabulary cannot hold, which the reference implementations refuse
/// at lower time under the tag [`tag`](StripError::tag) answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StripError {
    /// `(%op name ...)` where `name` is not an opcode (`%nock` included:
    /// it is a case of the vocabulary, not an opcode).
    UnknownOpcode(String),
    /// An opcode applied to the wrong number of arguments.
    OpArity {
        /// The opcode name, without `%`.
        op: String,
        /// How many arguments it was given.
        got: usize,
    },
    /// The axis argument of `%slot`, `%call`, or `%edit` is not an
    /// atom node.
    AxisArg {
        /// The opcode name, without `%`.
        op: String,
    },
    /// A raw cell of fewer than two elements.
    EmptyRawCell,
}

impl StripError {
    /// The reference implementations' crash tag: `unknown-opcode`,
    /// `op-arity`, `axis-arg-must-be-atom`, or `empty-raw-cell`.
    pub fn tag(&self) -> &'static str {
        match self {
            StripError::UnknownOpcode(_) => "unknown-opcode",
            StripError::OpArity { .. } => "op-arity",
            StripError::AxisArg { .. } => "axis-arg-must-be-atom",
            StripError::EmptyRawCell => "empty-raw-cell",
        }
    }
}

impl fmt::Display for StripError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StripError::UnknownOpcode(op) => write!(f, "%unknown-opcode: %{op}"),
            StripError::OpArity { op, got } => {
                write!(f, "%op-arity: %{op} applied to {got} arguments")
            }
            StripError::AxisArg { op } => {
                write!(
                    f,
                    "%axis-arg-must-be-atom: the axis of %{op} is not an atom"
                )
            }
            StripError::EmptyRawCell => write!(f, "%empty-raw-cell: a raw cell needs two elements"),
        }
    }
}

impl std::error::Error for StripError {}

/// Why annotated IR did not lower: the projection refused it, or the
/// bare lowering did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExpandError {
    /// No projection: see [`StripError`].
    Strip(StripError),
    /// The bare lowering's refusal: see [`LowerError`].
    Lower(LowerError),
}

impl fmt::Display for ExpandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExpandError::Strip(e) => write!(f, "{e}"),
            ExpandError::Lower(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ExpandError {}

impl From<StripError> for ExpandError {
    fn from(e: StripError) -> ExpandError {
        ExpandError::Strip(e)
    }
}

impl From<LowerError> for ExpandError {
    fn from(e: LowerError) -> ExpandError {
        ExpandError::Lower(e)
    }
}

// ----------------------------------------------------------------------
// The two bridges
// ----------------------------------------------------------------------

/// A child of one unrolled layer: a node of the typed tree, or an
/// opcode's axis atom, which the untyped vocabulary reads as a node.
#[derive(Clone)]
enum Src<'a> {
    Node(&'a Nasm),
    Axis(Atom),
}

/// One child of the typed tree as a layer: a node unrolls, an axis atom
/// is a leaf.
fn open_src<'a>(s: &Src<'a>) -> NasmOf<Src<'a>> {
    match s {
        &Src::Node(n) => unroll(n),
        Src::Axis(a) => NasmOf::Atom(a.clone()),
    }
}

/// Read one plain node as a layer of the vocabulary, children borrowed;
/// an opcode's axis argument becomes an atom child.
///
/// Exhaustive over [`Nasm`] and [`Op`] with no wildcard arm — this is
/// where a variant added to `Nasm` without its untyped reading fails to
/// compile (see the module docs). Cheap: children are borrowed, atoms
/// and nouns are refcounted, a name is one string clone.
fn unroll(n: &Nasm) -> NasmOf<Src<'_>> {
    let node = Src::Node;
    match n {
        Nasm::Atom(a) => NasmOf::Atom(a.clone()),
        Nasm::Axis(name) => NasmOf::Axis(name.as_str().to_string()),
        Nasm::Cell {
            first,
            second,
            rest,
        } => NasmOf::Cell(
            [&**first, &**second]
                .into_iter()
                .chain(rest.iter())
                .map(node)
                .collect(),
        ),
        Nasm::Op(op) => {
            let axis = |a: &Atom| Src::Axis(a.clone());
            let args = match op {
                Op::Slot(ax) => vec![axis(ax)],
                Op::Self_ | Op::Battery | Op::Payload | Op::Sample | Op::Context | Op::Crash => {
                    vec![]
                }
                Op::Const(x) | Op::Arm(x) | Op::Isa(x) | Op::Inc(x) => vec![node(x)],
                Op::Eval(a, b)
                | Op::Eq(a, b)
                | Op::Comp(a, b)
                | Op::Push(a, b)
                | Op::Hint(a, b)
                | Op::Scry(a, b) => vec![node(a), node(b)],
                Op::If(a, b, c) | Op::Hintd(a, b, c) => vec![node(a), node(b), node(c)],
                Op::Call(ax, f) => vec![axis(ax), node(f)],
                Op::Edit(ax, v, f) => vec![axis(ax), node(v), node(f)],
            };
            NasmOf::Op(op.name().to_string(), args)
        }
        Nasm::Let { name, value, body } => NasmOf::Let(
            name.as_str().to_string(),
            Box::new(node(value)),
            Box::new(node(body)),
        ),
        Nasm::Match {
            scrutinee,
            arms,
            default,
        } => NasmOf::Match(
            Box::new(node(scrutinee)),
            arms.iter()
                .map(|arm| (node(&arm.pattern), node(&arm.body)))
                .collect(),
            Box::new(node(default)),
        ),
        Nasm::Nock(noun) => NasmOf::Nock(noun.clone()),
    }
}

/// Close one layer of the vocabulary over plain children: the
/// vocabulary check. The opcode name and arity are held to the [`Op`]
/// table, an axis argument must be an atom node, a raw cell needs two
/// elements; a name is taken as it is ([`Name::raw`]), the schema
/// lookup being the bare lowering's business.
pub fn roll(layer: NasmOf<Nasm>) -> Result<Nasm, StripError> {
    Ok(match layer {
        NasmOf::Atom(a) => Nasm::Atom(a),
        NasmOf::Axis(name) => Nasm::Axis(Name::raw(name)),
        NasmOf::Cell(items) => Nasm::raw_cell(items).ok_or(StripError::EmptyRawCell)?,
        NasmOf::Op(name, args) => Nasm::Op(op_of(name, args)?),
        NasmOf::Let(name, value, body) => Nasm::Let {
            name: Name::raw(name),
            value,
            body,
        },
        NasmOf::Match(scrutinee, arms, default) => Nasm::Match {
            scrutinee,
            arms: arms
                .into_iter()
                .map(|(pattern, body)| MatchArm { pattern, body })
                .collect(),
            default,
        },
        NasmOf::Nock(noun) => Nasm::Nock(noun),
    })
}

/// `(%name args...)` against the opcode table.
fn op_of(name: String, args: Vec<Nasm>) -> Result<Op, StripError> {
    let got = args.len();
    let axis = |x: Nasm| match &x {
        Nasm::Atom(a) => Ok(a.clone()),
        _ => Err(StripError::AxisArg { op: name.clone() }),
    };
    let mut it = args.into_iter();
    let mut arg = || Box::new(it.next().expect("the arity was checked"));
    Ok(match (name.as_str(), got) {
        ("slot", 1) => Op::Slot(axis(*arg())?),
        ("self", 0) => Op::Self_,
        ("battery", 0) => Op::Battery,
        ("payload", 0) => Op::Payload,
        ("sample", 0) => Op::Sample,
        ("context", 0) => Op::Context,
        ("crash", 0) => Op::Crash,
        ("const", 1) => Op::Const(arg()),
        ("arm", 1) => Op::Arm(arg()),
        ("eval", 2) => Op::Eval(arg(), arg()),
        ("isa", 1) => Op::Isa(arg()),
        ("inc", 1) => Op::Inc(arg()),
        ("eq", 2) => Op::Eq(arg(), arg()),
        ("if", 3) => Op::If(arg(), arg(), arg()),
        ("comp", 2) => Op::Comp(arg(), arg()),
        ("push", 2) => Op::Push(arg(), arg()),
        ("call", 2) => {
            let ax = axis(*arg())?;
            Op::Call(ax, arg())
        }
        ("edit", 3) => {
            let ax = axis(*arg())?;
            Op::Edit(ax, arg(), arg())
        }
        ("hint", 2) => Op::Hint(arg(), arg()),
        ("hintd", 3) => Op::Hintd(arg(), arg(), arg()),
        ("scry", 2) => Op::Scry(arg(), arg()),
        (known, _) if OPCODES.contains(&known) => {
            return Err(StripError::OpArity { op: name, got });
        }
        (_, _) => return Err(StripError::UnknownOpcode(name)),
    })
}

// ----------------------------------------------------------------------
// The post-order driver
// ----------------------------------------------------------------------

/// One pending step of a [`fold`].
enum Task<S> {
    /// Open this node and schedule its children.
    Visit(S),
    /// Every child of `layer` has a result on the value stack; refill
    /// the layer with them and close it.
    Build {
        source: S,
        layer: NasmOf<()>,
        count: usize,
    },
}

/// Post-order fold over any tree that reads as layers of the
/// vocabulary, on an explicit stack: `open` views one node as a layer
/// over its children, `close` builds a node's result from the node and
/// the results of its children, or refuses. Children are enumerated
/// through [`NasmOf::map`] itself, so the order they are visited in and
/// the order their results are handed back in cannot disagree.
fn fold<S, R, E>(
    root: S,
    open: impl Fn(&S) -> NasmOf<S>,
    mut close: impl FnMut(S, NasmOf<R>) -> Result<R, E>,
) -> Result<R, E> {
    let mut tasks: Vec<Task<S>> = vec![Task::Visit(root)];
    let mut done: Vec<R> = Vec::new();
    while let Some(task) = tasks.pop() {
        match task {
            Task::Visit(source) => {
                let mut kids: Vec<S> = Vec::new();
                let layer = open(&source).map(|child| kids.push(child));
                tasks.push(Task::Build {
                    source,
                    layer,
                    count: kids.len(),
                });
                // Leftmost child on top, so it completes first and its
                // result sits deepest in `done`.
                tasks.extend(kids.into_iter().rev().map(Task::Visit));
            }
            Task::Build {
                source,
                layer,
                count,
            } => {
                let mut results = done
                    .drain(done.len() - count..)
                    .collect::<Vec<R>>()
                    .into_iter();
                let filled = layer.map(|()| results.next().expect("one result per child"));
                debug_assert!(results.next().is_none(), "no result left over");
                done.push(close(source, filled)?);
            }
        }
    }
    debug_assert_eq!(done.len(), 1, "every fold yields one result");
    Ok(done.pop().expect("the root result"))
}

// ----------------------------------------------------------------------
// The annotated instantiation
// ----------------------------------------------------------------------

/// `+$ noted`: the annotated instantiation of the vocabulary. Every
/// node carries a note of type `A` beside its [`NasmOf`] layer, whose
/// children are again `Noted<A>`.
///
/// `Noted<()>` is the reference vocabulary in all but name;
/// `Noted<Option<Pos>>` is a positioned IR. The fields are public — a
/// compiler builds these directly — but, like [`Nasm`], the type
/// implements `Drop` (so that deep trees tear down iteratively), which
/// means it cannot be destructured by value; borrow the fields, or
/// `strip` / `map_note`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Noted<A> {
    /// The annotation.
    pub note: A,
    /// The node, with annotated children.
    pub node: NasmOf<Noted<A>>,
}

impl<A> Noted<A> {
    /// A node.
    pub fn new(note: A, node: NasmOf<Noted<A>>) -> Noted<A> {
        Noted { note, node }
    }

    /// One layer with the children borrowed.
    fn open(&self) -> NasmOf<&Noted<A>> {
        self.node.as_ref()
    }

    /// Project onto the bare IR: drop every note, check the vocabulary,
    /// recurse.
    ///
    /// This is the strip contract of the module docs —
    /// `Noted::dress(&n, a).strip() == Ok(n)` — and the meaning of an
    /// annotated value is the meaning of its projection. Runs on an
    /// explicit stack; every node's data is cloned once (atoms and
    /// nouns are refcounted, names are string clones). Refuses, with
    /// the reference crash tag, a node the typed vocabulary cannot hold.
    pub fn strip(&self) -> Result<Nasm, StripError> {
        fold(self, |n| n.open(), |_, layer| roll(layer))
    }

    /// Rewrite every note, keeping the tree: `strip` of the result is
    /// `strip` of the input. Notes are visited in post-order (children
    /// before parents, left to right), on an explicit stack.
    ///
    /// By reference rather than by value: `Noted` implements `Drop`, so
    /// moving a note out of it is not expressible in safe Rust, and
    /// this crate forbids unsafe. A `FnMut(&A) -> B` covers the
    /// by-value shape anyway (clone inside the closure when `B = A`).
    pub fn map_note<B>(&self, mut f: impl FnMut(&A) -> B) -> Noted<B> {
        let done: Result<Noted<B>, std::convert::Infallible> = fold(
            self,
            |n| n.open(),
            |n, layer| Ok(Noted::new(f(&n.note), layer)),
        );
        match done {
            Ok(x) => x,
        }
    }

    /// The notes, in post-order (the order `map_note` visits).
    pub fn notes(&self) -> Vec<&A> {
        let mut out = Vec::new();
        let done: Result<(), std::convert::Infallible> = fold(
            self,
            |n| n.open(),
            |n, _| {
                out.push(&n.note);
                Ok(())
            },
        );
        match done {
            Ok(()) => out,
        }
    }
}

impl<A: Clone> Noted<A> {
    /// Annotate plain IR with the same note on every node: a section
    /// of [`strip`](Noted::strip), so `Noted::dress(&n, a).strip() ==
    /// Ok(n)` for every `n`. An opcode's axis atom becomes an atom node
    /// with the note. Runs on an explicit stack; total.
    pub fn dress(n: &Nasm, note: A) -> Noted<A> {
        let done: Result<Noted<A>, std::convert::Infallible> =
            fold(Src::Node(n), open_src, |_, layer| {
                Ok(Noted::new(note.clone(), layer))
            });
        match done {
            Ok(x) => x,
        }
    }

    /// [`crate::lift`], dressed: a formula as annotated IR with the same
    /// note on every node, so that `lower(None, &Noted::lift(&f, a))`
    /// is `f` (the lift's soundness law, through the projection).
    pub fn lift(formula: &Noun, note: A) -> Noted<A> {
        Noted::dress(&crate::lift(formula), note)
    }
}

/// Move the children of a non-leaf node onto `stack`, leaving a leaf in
/// its place. The children's own nodes are untouched here — each is
/// hollowed in turn when it is popped — so no node is ever dropped
/// with more than one live layer beneath it.
fn hollow<A>(node: &mut NasmOf<Noted<A>>, stack: &mut Vec<Noted<A>>) {
    if node.is_leaf() {
        return;
    }
    std::mem::replace(node, NasmOf::Atom(Atom::ZERO)).map(|child| stack.push(child));
}

impl<A> Drop for Noted<A> {
    /// Iterative teardown, in the style of `Nasm`'s: a positioned
    /// emitter can produce IR as deep as its input, so dropping must
    /// not recurse. Each popped child is hollowed into `stack` before
    /// it drops, leaving only its note for the normal glue to free —
    /// and its own `Drop`, finding a leaf, returns at once.
    fn drop(&mut self) {
        if self.node.is_leaf() {
            return;
        }
        let mut stack: Vec<Noted<A>> = Vec::new();
        hollow(&mut self.node, &mut stack);
        while let Some(mut child) = stack.pop() {
            hollow(&mut child.node, &mut stack);
        }
    }
}

// ----------------------------------------------------------------------
// The pipeline, composed with the projection
// ----------------------------------------------------------------------

/// Lower annotated IR to its canonical Nock noun:
/// [`crate::lower`]`(schema, &expr.strip()?)`.
pub fn lower<A>(schema: Option<&Schema>, expr: &Noted<A>) -> Result<Noun, ExpandError> {
    Ok(crate::lower(schema, &expr.strip()?)?)
}

/// Render annotated IR to canonical `.nasm` source:
/// [`crate::render`]`(schema, &expr.strip()?)`. Notes do not render —
/// the canonical text is byte-identical across implementations and
/// carries no annotations by design.
pub fn render<A>(schema: Option<&Schema>, expr: &Noted<A>) -> Result<String, StripError> {
    Ok(crate::render(schema, &expr.strip()?))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::{expand, noun, parse, Program};

    const TEST_STACK: usize = 2 * 1024 * 1024;

    /// Every vocabulary case and every opcode, plus the mixed forms,
    /// as source (the parser is the most convenient constructor).
    const SAMPLES: &[&str] = &[
        "42",
        "0x1.0000",
        "'fast'",
        "123.456.789.012.345.678.901.234.567.890",
        ":subject {.a .b}  .b",
        "[4 0 1]",
        "[8 [1 0] 4 0 6]",
        "[1 2 3 4]",
        "(%slot 7)",
        "(%self)",
        "(%battery)",
        "(%payload)",
        "(%sample)",
        "(%context)",
        "(%crash)",
        "(%const [1 2])",
        "(%arm (%if (%slot 1) 0 1))",
        "(%eval (%slot 1) (%const 42))",
        "(%isa (%slot 1))",
        "(%inc (%slot 1))",
        "(%eq (%slot 2) (%slot 3))",
        "(%if (%slot 1) 0 1)",
        "(%comp (%slot 1) (%inc (%slot 1)))",
        "(%push (%const 42) (%slot 1))",
        "(%call 2 (%slot 1))",
        "(%edit 6 (%inc (%slot 1)) (%slot 1))",
        "(%hint 'fast' (%slot 1))",
        "(%hintd 'fast' 0 (%slot 1))",
        "(%scry (%const 138) (%slot 1))",
        "(%nock [11 'fast' 0 1])",
        "(%nock 42)",
        ":subject .x  #let .a = (%inc .x) in #let .b = (%inc .a) in (%eq .a .b)",
        ":subject {.tag .data}  #match .tag { 1 => (%inc .data)  2 => 20  _ => 0 }",
        ":subject {.before .target .after}\n#let .next = (%inc .target) in\n  [.before .next .after]\n",
    ];

    fn samples() -> Vec<(&'static str, Program)> {
        SAMPLES
            .iter()
            .map(|src| (*src, parse(src).unwrap_or_else(|e| panic!("{src}: {e}"))))
            .collect()
    }

    /// The kinds and opcode names present in an annotated tree, by an
    /// iterative walk.
    fn inventory<A>(n: &Noted<A>) -> (BTreeSet<&'static str>, BTreeSet<String>) {
        let mut kinds = BTreeSet::new();
        let mut ops = BTreeSet::new();
        let mut stack = vec![n];
        while let Some(n) = stack.pop() {
            kinds.insert(n.node.kind());
            if let NasmOf::Op(name, _) = &n.node {
                ops.insert(name.clone());
            }
            stack.extend(n.node.children());
        }
        (kinds, ops)
    }

    /// The nodes of a typed tree, counting an opcode's axis atom as one
    /// (the untyped reading does).
    fn typed_nodes(n: &Nasm) -> usize {
        let mut count = 0;
        let mut stack = vec![Src::Node(n)];
        while let Some(s) = stack.pop() {
            count += 1;
            if let Src::Node(n) = s {
                stack.extend(unroll(n).children().into_iter().cloned());
            }
        }
        count
    }

    fn all_ops() -> BTreeSet<String> {
        OPCODES.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn samples_cover_the_vocabulary() {
        let mut kinds = BTreeSet::new();
        let mut ops = BTreeSet::new();
        for (_, program) in samples() {
            let (k, o) = inventory(&Noted::dress(&program.body, ()));
            kinds.extend(k);
            ops.extend(o);
        }
        let want_kinds: BTreeSet<&str> = ["atom", "axis", "cell", "op", "let", "match", "nock"]
            .into_iter()
            .collect();
        assert_eq!(kinds, want_kinds, "every case is sampled");
        assert_eq!(ops, all_ops(), "every opcode is sampled");
    }

    #[test]
    fn the_opcode_table_is_the_op_enum() {
        // every `Op` variant's name, read off the samples through
        // `unroll`, is in the table, and the table has nothing else
        let mut seen = BTreeSet::new();
        for (_, program) in samples() {
            let mut stack = vec![&program.body];
            while let Some(n) = stack.pop() {
                if let Nasm::Op(op) = n {
                    seen.insert(op.name().to_string());
                }
                for child in unroll(n).children() {
                    if let Src::Node(c) = child {
                        stack.push(c);
                    }
                }
            }
        }
        assert_eq!(seen, all_ops());
    }

    #[test]
    fn strip_undoes_dress() {
        for (src, program) in samples() {
            let unit = Noted::dress(&program.body, ());
            assert_eq!(unit.strip(), Ok(program.body.clone()), "{src}");
            let tagged = Noted::dress(&program.body, src);
            assert_eq!(tagged.strip(), Ok(program.body.clone()), "{src}");
            let renamed = tagged.map_note(|s| s.len());
            assert_eq!(renamed.strip(), Ok(program.body.clone()), "{src}: map_note");
        }
    }

    #[test]
    fn lower_and_render_agree_with_the_bare_pipeline() {
        for (src, program) in samples() {
            let schema = program.schema.as_ref();
            let noted = Noted::dress(&program.body, 7u8);
            let want = expand(src).unwrap_or_else(|e| panic!("{src}: {e}"));
            assert_eq!(program.lower().unwrap(), want, "{src}");
            assert_eq!(lower(schema, &noted).unwrap(), want, "{src}: lower");
            assert_eq!(
                render(schema, &noted).unwrap(),
                program.render(),
                "{src}: render"
            );
        }
    }

    #[test]
    fn dress_puts_the_note_on_every_node() {
        for (src, program) in samples() {
            let noted = Noted::dress(&program.body, src);
            let notes = noted.notes();
            assert!(notes.iter().all(|n| **n == src), "{src}");
            assert_eq!(
                notes.len(),
                typed_nodes(&program.body),
                "{src}: one note per node"
            );
        }
    }

    // A hand-built annotated tree, one distinct note per node, covering
    // every vocabulary case and every opcode with asymmetric arguments
    // kept distinct — so a field mislabelled in `roll`/`unroll` (a
    // swapped `%if` branch, a `#let` value for its body) shows up
    // against the parser and the expander, not just against itself.
    // The axis atoms of `%slot`, `%call`, and `%edit` are nodes with
    // notes of their own, which the typed tree has no place for.
    type Node = Noted<u32>;
    fn atom(note: u32, v: u64) -> Node {
        Noted::new(note, NasmOf::Atom(Atom::from(v)))
    }
    fn cord(note: u32, s: &str) -> Node {
        Noted::new(note, NasmOf::Atom(Atom::from_cord(s)))
    }
    fn axis(note: u32, name: &str) -> Node {
        Noted::new(note, NasmOf::Axis(name.into()))
    }
    fn op(note: u32, name: &str, args: Vec<Node>) -> Node {
        Noted::new(note, NasmOf::Op(name.into(), args))
    }
    fn cell(note: u32, items: Vec<Node>) -> Node {
        Noted::new(note, NasmOf::Cell(items))
    }
    const HAND_BUILT_SRC: &str = "\
:subject {.a .b}
#let .v = (%eval (%const [1 2]) (%if .a (%inc .b) (%crash))) in
#match (%edit 6 (%call 2 .v) (%hintd 'memo' (%comp .a .b) (%push (%scry .a .b) (%hint 'fast' [.a .b 7])))) {
  1 => (%eq .a .b)
  'two' => (%arm (%isa (%self)))
  3 => [(%battery) (%payload) (%sample) (%context) (%slot 5)]
  _ => (%nock [0 1])
}
";
    const HAND_BUILT_NOTES: u32 = 49;
    fn hand_built() -> Noted<u32> {
        let value = op(
            1,
            "eval",
            vec![
                op(2, "const", vec![cell(3, vec![atom(4, 1), atom(5, 2)])]),
                op(
                    6,
                    "if",
                    vec![
                        axis(7, "a"),
                        op(8, "inc", vec![axis(9, "b")]),
                        op(10, "crash", vec![]),
                    ],
                ),
            ],
        );
        let scrutinee = op(
            11,
            "edit",
            vec![
                atom(46, 6),
                op(12, "call", vec![atom(47, 2), axis(13, "v")]),
                op(
                    14,
                    "hintd",
                    vec![
                        cord(15, "memo"),
                        op(16, "comp", vec![axis(17, "a"), axis(18, "b")]),
                        op(
                            19,
                            "push",
                            vec![
                                op(20, "scry", vec![axis(21, "a"), axis(22, "b")]),
                                op(
                                    23,
                                    "hint",
                                    vec![
                                        cord(24, "fast"),
                                        cell(25, vec![axis(26, "a"), axis(27, "b"), atom(28, 7)]),
                                    ],
                                ),
                            ],
                        ),
                    ],
                ),
            ],
        );
        let arms = vec![
            (
                atom(29, 1),
                op(30, "eq", vec![axis(31, "a"), axis(32, "b")]),
            ),
            (
                cord(33, "two"),
                op(34, "arm", vec![op(35, "isa", vec![op(36, "self", vec![])])]),
            ),
            (
                atom(37, 3),
                cell(
                    38,
                    vec![
                        op(39, "battery", vec![]),
                        op(40, "payload", vec![]),
                        op(41, "sample", vec![]),
                        op(42, "context", vec![]),
                        op(43, "slot", vec![atom(48, 5)]),
                    ],
                ),
            ),
        ];
        let default = Noted::new(44, NasmOf::Nock(noun![0 1]));
        let body = Noted::new(
            45,
            NasmOf::Match(Box::new(scrutinee), arms, Box::new(default)),
        );
        Noted::new(0, NasmOf::Let("v".into(), Box::new(value), Box::new(body)))
    }

    #[test]
    fn hand_built_tree_covers_the_vocabulary() {
        let (kinds, ops) = inventory(&hand_built());
        assert_eq!(kinds.len(), 7, "every case: {kinds:?}");
        assert_eq!(ops, all_ops());
    }

    #[test]
    fn hand_built_tree_matches_parser_expander_and_renderer() {
        let program = parse(HAND_BUILT_SRC).expect("parses");
        let ours = hand_built();
        assert_eq!(
            ours.strip(),
            Ok(program.body.clone()),
            "strip agrees with parse"
        );
        let schema = program.schema.as_ref();
        let want = expand(HAND_BUILT_SRC).expect("expands");
        assert_eq!(lower(schema, &ours).expect("lowers"), want);
        assert_eq!(render(schema, &ours).unwrap(), program.render());
        assert_eq!(
            expand(&render(schema, &ours).unwrap()).expect("re-expands"),
            want,
            "round trip through the renderer"
        );
        // and dressing the parse gives the same tree up to the notes
        assert_eq!(
            Noted::dress(&program.body, 0).strip(),
            ours.map_note(|_| 0).strip()
        );
        assert_eq!(Noted::dress(&program.body, 0), ours.map_note(|_| 0));
    }

    #[test]
    fn map_note_visits_every_note_once_and_keeps_the_tree() {
        let ours = hand_built();
        let mut seen: Vec<u32> = Vec::new();
        let shifted = ours.map_note(|n| {
            seen.push(*n);
            n + 100
        });
        seen.sort_unstable();
        assert_eq!(
            seen,
            (0..HAND_BUILT_NOTES).collect::<Vec<u32>>(),
            "each note once"
        );
        let mut notes: Vec<u32> = shifted.notes().into_iter().copied().collect();
        notes.sort_unstable();
        assert_eq!(notes, (100..100 + HAND_BUILT_NOTES).collect::<Vec<u32>>());
        assert_eq!(shifted.strip(), ours.strip());
    }

    #[test]
    fn axis_atoms_keep_their_notes() {
        // the three axis arguments are nodes 46, 47, 48 in the hand-built
        // tree; the typed projection holds their atoms and forgets the
        // notes, and dressing the projection gives them the note back
        let ours = hand_built();
        let notes: BTreeSet<u32> = ours.notes().into_iter().copied().collect();
        assert!(notes.contains(&46) && notes.contains(&47) && notes.contains(&48));
        let program = parse(HAND_BUILT_SRC).expect("parses");
        let count = Noted::dress(&program.body, ()).notes().len();
        assert_eq!(count as u32, HAND_BUILT_NOTES);
    }

    #[test]
    fn strip_refuses_what_the_typed_vocabulary_cannot_hold() {
        let n = |node| Noted::new((), node);
        let one = || n(NasmOf::Atom(Atom::from(1u64)));
        let cases: Vec<(Noted<()>, &str)> = vec![
            (n(NasmOf::Op("frob".into(), vec![])), "unknown-opcode"),
            (n(NasmOf::Op("nock".into(), vec![one()])), "unknown-opcode"),
            (n(NasmOf::Op("inc".into(), vec![])), "op-arity"),
            (n(NasmOf::Op("crash".into(), vec![one()])), "op-arity"),
            (n(NasmOf::Op("if".into(), vec![one(), one()])), "op-arity"),
            (
                n(NasmOf::Op(
                    "slot".into(),
                    vec![n(NasmOf::Op("inc".into(), vec![one()]))],
                )),
                "axis-arg-must-be-atom",
            ),
            (
                n(NasmOf::Op(
                    "call".into(),
                    vec![n(NasmOf::Axis("a".into())), one()],
                )),
                "axis-arg-must-be-atom",
            ),
            (
                n(NasmOf::Op(
                    "edit".into(),
                    vec![n(NasmOf::Cell(vec![one(), one()])), one(), one()],
                )),
                "axis-arg-must-be-atom",
            ),
            (n(NasmOf::Cell(vec![])), "empty-raw-cell"),
            (n(NasmOf::Cell(vec![one()])), "empty-raw-cell"),
            // deep inside, under a well-formed node
            (
                n(NasmOf::Let(
                    "x".into(),
                    Box::new(one()),
                    Box::new(n(NasmOf::Op("inc".into(), vec![n(NasmOf::Cell(vec![]))]))),
                )),
                "empty-raw-cell",
            ),
        ];
        for (tree, tag) in cases {
            let err = tree.strip().expect_err(tag);
            assert_eq!(err.tag(), tag, "{err}");
            assert!(matches!(lower(None, &tree), Err(ExpandError::Strip(e)) if e == err));
        }
        // a well-formed op with a bad name is a lower-time refusal of
        // the bare pipeline, not of the projection
        let unbound = n(NasmOf::Axis("nowhere".into()));
        assert!(unbound.strip().is_ok());
        assert!(matches!(lower(None, &unbound), Err(ExpandError::Lower(_))));
    }

    #[test]
    fn the_untyped_reading_of_an_axis_is_an_atom_node() {
        let program = parse("(%edit 6 (%inc (%slot 1)) (%slot 1))").unwrap();
        let noted = Noted::dress(&program.body, ());
        let NasmOf::Op(name, args) = &noted.node else {
            panic!("an op")
        };
        assert_eq!(name, "edit");
        assert_eq!(args.len(), 3);
        assert_eq!(args[0].node, NasmOf::Atom(Atom::from(6u64)));
    }

    /// A left-deep chain of `depth` `%inc`s, built iteratively.
    fn deep(depth: usize) -> Noted<u8> {
        let mut n = Noted::new(0, NasmOf::Op("slot".into(), vec![atom8(1)]));
        for _ in 0..depth {
            n = Noted::new(0, NasmOf::Op("inc".into(), vec![n]));
        }
        n
    }
    fn atom8(v: u64) -> Noted<u8> {
        Noted::new(0, NasmOf::Atom(Atom::from(v)))
    }

    #[test]
    fn deep_trees_convert_and_drop_without_recursion() {
        std::thread::Builder::new()
            .stack_size(TEST_STACK)
            .spawn(|| {
                let depth = 200_000;
                let ours = deep(depth);
                let relabelled = ours.map_note(|n| n + 1);
                assert_eq!(relabelled.notes().len(), depth + 2);
                let bare = ours.strip().expect("strips");
                let again = Noted::dress(&bare, 0u8);
                assert_eq!(again.notes().len(), depth + 2);
                drop(again);
                drop(relabelled);
                drop(ours);
                drop(bare);
            })
            .expect("spawn")
            .join()
            .expect("no overflow");
    }
}
