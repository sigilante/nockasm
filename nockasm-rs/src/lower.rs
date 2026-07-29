//! `lower`: the macro expander, IR to canonical Nock noun.
//!
//! A direct port of the reference expansion equations:
//!
//! - Formula positions lift bare atoms to `[1 atom]`; noun positions
//!   never lift; raw-cell elements expand structurally (never lifted).
//! - `#let` compiles its value against the old subject, then binds the
//!   name to axis 2 in the body with every prior axis pegged under 3.
//! - `#match` lifts the scrutinee to axis 2 via opcode 8, then dispatches
//!   with nested opcode-6 equality tests against the literal patterns,
//!   ending in the required default.
//!
//! Axes are [`Atom`]s, not machine words, so deeply nested binders cannot
//! overflow — parity with the bignum reference implementations.

use std::collections::BTreeMap;

use crate::ast::{Name, Nasm, Op, Program, Schema};
use crate::error::{Error, LowerError};
use crate::noun::{peg, Atom, Noun};
use crate::parse::parse;

type Axes = BTreeMap<Name, Atom>;

/// Expand `.nasm` source to a canonical Nock formula: `parse` then
/// [`lower`].
///
/// ```
/// use nockasm::{expand, noun};
/// assert_eq!(expand("(%inc (%self))").unwrap(), noun![4 0 1]);
/// assert_eq!(
///     expand(":subject {.a .b} .b").unwrap(),
///     noun![0 3]
/// );
/// ```
pub fn expand(src: &str) -> Result<Noun, Error> {
    let program = parse(src)?;
    Ok(program.lower()?)
}

/// Lower an IR value to its canonical Nock noun.
pub fn lower(schema: Option<&Schema>, expr: &Nasm) -> Result<Noun, LowerError> {
    let axes = match schema {
        None => Axes::new(),
        Some(s) => {
            let mut axes = Axes::new();
            resolve_schema(s, Atom::from(1u64), &mut axes)?;
            axes
        }
    };
    expand_node(expr, &axes)
}

impl Program {
    /// Lower this program to its canonical Nock noun.
    pub fn lower(&self) -> Result<Noun, LowerError> {
        lower(self.schema.as_ref(), &self.body)
    }
}

fn resolve_schema(s: &Schema, base: Atom, axes: &mut Axes) -> Result<(), LowerError> {
    match s {
        Schema::Leaf(name) => {
            if axes.insert(name.clone(), base).is_some() {
                return Err(LowerError::DuplicateSchemaName(name.clone()));
            }
            Ok(())
        }
        Schema::Pair(head, tail) => {
            resolve_schema(head, base.double_plus(false), axes)?;
            resolve_schema(tail, base.double_plus(true), axes)
        }
    }
}

/// When the subject becomes `[new old]`, every old axis n moves to
/// `peg(3, n)`.
fn shift_axes(axes: &Axes) -> Axes {
    let three = Atom::from(3u64);
    axes.iter()
        .map(|(name, ax)| {
            let shifted = peg(&three, ax).expect("axes are nonzero");
            (name.clone(), shifted)
        })
        .collect()
}

/// A bare atom in formula position becomes the constant `[1 atom]`.
fn quot(n: Noun) -> Noun {
    if n.is_atom() {
        Noun::cell(1u64, n)
    } else {
        n
    }
}

/// Expand in formula position (lifting).
fn formula(e: &Nasm, axes: &Axes) -> Result<Noun, LowerError> {
    Ok(quot(expand_node(e, axes)?))
}

/// The recursion point. A slim dispatcher (fat locals live in the
/// per-construct helpers) so that IR at the parser's depth bound lowers
/// comfortably within a 2 MiB thread stack even in debug builds.
fn expand_node(e: &Nasm, axes: &Axes) -> Result<Noun, LowerError> {
    match e {
        Nasm::Atom(_) | Nasm::Axis(_) => expand_leaf(e, axes),
        Nasm::Cell { .. } => expand_cell(e, axes),
        Nasm::Op(op) => expand_op(op, axes),
        Nasm::Let { .. } => expand_let(e, axes),
        Nasm::Match { .. } => expand_match(e, axes),
    }
}

#[inline(never)]
fn expand_leaf(e: &Nasm, axes: &Axes) -> Result<Noun, LowerError> {
    match e {
        Nasm::Atom(a) => Ok(Noun::from(a.clone())),
        Nasm::Axis(name) => match axes.get(name) {
            Some(ax) => Ok(Noun::cell(0u64, ax.clone())),
            None => Err(LowerError::UnboundAxis {
                name: name.clone(),
                declared: axes.keys().cloned().collect(),
            }),
        },
        _ => unreachable!("caller matched a leaf"),
    }
}

#[inline(never)]
fn expand_cell(e: &Nasm, axes: &Axes) -> Result<Noun, LowerError> {
    let Nasm::Cell {
        first,
        second,
        rest,
    } = e
    else {
        unreachable!("caller matched Cell");
    };
    let mut elems = Vec::with_capacity(2 + rest.len());
    elems.push(expand_node(first, axes)?);
    elems.push(expand_node(second, axes)?);
    for r in rest {
        elems.push(expand_node(r, axes)?);
    }
    Ok(Noun::autocons(elems).expect("cell has >= 2 elements"))
}

#[inline(never)]
fn expand_let(e: &Nasm, axes: &Axes) -> Result<Noun, LowerError> {
    let Nasm::Let { name, value, body } = e else {
        unreachable!("caller matched Let");
    };
    // Value compiled against the old subject; the body sees the binding
    // at axis 2 and everything else pegged under 3.
    let v = formula(value, axes)?;
    let mut new = shift_axes(axes);
    if new.contains_key(name) {
        return Err(LowerError::LetShadows(name.clone()));
    }
    new.insert(name.clone(), Atom::from(2u64));
    let b = formula(body, &new)?;
    Ok(Noun::cell(8u64, Noun::cell(v, b)))
}

#[inline(never)]
fn expand_match(e: &Nasm, axes: &Axes) -> Result<Noun, LowerError> {
    let Nasm::Match {
        scrutinee,
        arms,
        default,
    } = e
    else {
        unreachable!("caller matched Match");
    };
    let s = formula(scrutinee, axes)?;
    let new = shift_axes(axes);
    let mut result = formula(default, &new)?;
    for arm in arms.iter().rev() {
        // The pattern is a literal noun: expanded unlifted, then wrapped
        // [1 pat] for the equality test against the scrutinee at axis 2.
        let pat = expand_node(&arm.pattern, &new)?;
        let body = formula(&arm.body, &new)?;
        result = crate::noun![6 [5 [1 (pat)] 0 2] (body) (result)];
    }
    Ok(crate::noun![8(s)(result)])
}

fn expand_op(op: &Op, axes: &Axes) -> Result<Noun, LowerError> {
    use crate::noun;
    let f = |e: &Nasm| formula(e, axes);
    let n = |e: &Nasm| expand_node(e, axes);
    Ok(match op {
        Op::Self_ => noun![0 1],
        Op::Battery => noun![0 2],
        Op::Payload => noun![0 3],
        Op::Sample => noun![0 6],
        Op::Context => noun![0 7],
        Op::Crash => noun![0 0],
        Op::Slot(ax) => noun![0(ax.clone())],
        Op::Const(x) => noun![1(n(x)?)],
        Op::Arm(x) => noun![1(n(x)?)],
        Op::Eval(s, g) => noun![2(f(s)?)(f(g)?)],
        Op::Isa(x) => noun![3(f(x)?)],
        Op::Inc(x) => noun![4(f(x)?)],
        Op::Eq(a, b) => noun![5(f(a)?)(f(b)?)],
        Op::If(c, t, e) => noun![6(f(c)?)(f(t)?)(f(e)?)],
        Op::Comp(a, b) => noun![7(f(a)?)(f(b)?)],
        Op::Push(a, b) => noun![8(f(a)?)(f(b)?)],
        Op::Call(ax, g) => noun![9(ax.clone())(f(g)?)],
        Op::Edit(ax, v, g) => noun![10[(ax.clone())(f(v)?)](f(g)?)],
        Op::Hint(t, g) => noun![11(n(t)?)(f(g)?)],
        Op::Hintd(t, c, g) => noun![11[(n(t)?)(f(c)?)](f(g)?)],
    })
}
