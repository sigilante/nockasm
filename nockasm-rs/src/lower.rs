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
//!
//! The traversal runs on an explicit stack, one post-order pass over the
//! IR: a task stack of pending expansions and assembly frames, and a
//! value stack of completed nouns. IR of any depth — machine-emitted
//! binder chains, or the lift of a hostile jamfile — lowers without
//! touching the call stack. Two structural facts keep it simple: only
//! [`Nasm::Atom`] nodes can expand to atoms, so the formula-position
//! lift is a per-task flag consulted there alone; and binder
//! environments are immutable once built, so tasks share them by
//! refcount. One divergence from the reference, deliberately harmless:
//! environment errors (`#let` shadowing) surface when the binder is
//! *scheduled*, not after its value expands, so on a source with several
//! errors a different one may be reported first — the set of accepted
//! and rejected sources is identical.

use std::collections::BTreeMap;

use crate::ast::{Name, Nasm, Op, Program, Schema};
use crate::error::{Error, LowerError};
use crate::noun;
use crate::noun::{peg, Atom, Noun, P};
use crate::parse::parse;

type Axes = BTreeMap<Name, Atom>;

/// A shared binder environment: name to subject axis.
type Env = P<Axes>;

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

/// One pending step of the traversal.
enum Task<'a> {
    /// Expand `node` against `env`. `formula` marks a formula position,
    /// where a bare atom lifts to `[1 atom]`; only [`Nasm::Atom`] can
    /// expand to an atom, so the flag is consulted there alone.
    Expand {
        node: &'a Nasm,
        env: Env,
        formula: bool,
    },
    /// Take this frame's completed children off the value stack and
    /// push the assembled noun.
    Assemble(Frame),
}

/// How to assemble a noun from its completed children.
///
/// Children always execute left-to-right (in the reference evaluation
/// order), so the leftmost child's value sits deepest in the value
/// stack: fixed-arity frames pop right-to-left, and the counted frames
/// drain their range in order.
enum Frame {
    /// A raw cell of this many elements, right-associated.
    Cell(usize),
    /// `[1 x]` — `%const` and `%arm` share the equation.
    Quote,
    Eval,
    Isa,
    Inc,
    Eq,
    If,
    Comp,
    Push,
    /// `[9 ax f]` — the axis is an atom literal, already known.
    Call(Atom),
    /// `[10 [ax v] f]` — the axis is an atom literal, already known.
    Edit(Atom),
    /// `[11 t f]` — static hint.
    Hint,
    /// `[11 [t c] f]` — dynamic hint.
    Hintd,
    /// `[8 v b]`.
    Let,
    /// `[8 s dispatch]` over this many arms; children are the
    /// scrutinee, the default, then each arm's pattern and body in the
    /// reference (last-arm-first) evaluation order.
    Match {
        arms: usize,
    },
}

/// Lower an IR value to its canonical Nock noun.
pub fn lower(schema: Option<&Schema>, expr: &Nasm) -> Result<Noun, LowerError> {
    let mut axes = Axes::new();
    if let Some(s) = schema {
        resolve_schema(s, &mut axes)?;
    }
    let mut tasks: Vec<Task<'_>> = vec![Task::Expand {
        node: expr,
        env: P::new(axes),
        formula: false,
    }];
    let mut values: Vec<Noun> = Vec::new();
    while let Some(task) = tasks.pop() {
        match task {
            Task::Expand { node, env, formula } => {
                expand_step(node, env, formula, &mut tasks, &mut values)?
            }
            Task::Assemble(frame) => {
                let v = assemble(frame, &mut values);
                values.push(v);
            }
        }
    }
    debug_assert_eq!(values.len(), 1, "every task tree yields one value");
    Ok(values.pop().expect("the root value"))
}

impl Program {
    /// Lower this program to its canonical Nock noun.
    pub fn lower(&self) -> Result<Noun, LowerError> {
        lower(self.schema.as_ref(), &self.body)
    }
}

/// Resolve a `:subject` schema to its name-to-axis map, rooted at 1.
fn resolve_schema(s: &Schema, axes: &mut Axes) -> Result<(), LowerError> {
    let mut stack: Vec<(&Schema, Atom)> = vec![(s, Atom::from(1u64))];
    while let Some((s, base)) = stack.pop() {
        match s {
            Schema::Leaf(name) => {
                if axes.insert(name.clone(), base).is_some() {
                    return Err(LowerError::DuplicateSchemaName(name.clone()));
                }
            }
            Schema::Pair(head, tail) => {
                stack.push((tail, base.double_plus(true)));
                stack.push((head, base.double_plus(false)));
            }
        }
    }
    Ok(())
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

/// One expansion step: push a finished leaf value, or schedule an
/// assembly frame under its children (leftmost child on top of the task
/// stack, so it executes first).
fn expand_step<'a>(
    node: &'a Nasm,
    env: Env,
    formula: bool,
    tasks: &mut Vec<Task<'a>>,
    values: &mut Vec<Noun>,
) -> Result<(), LowerError> {
    match node {
        Nasm::Atom(a) => {
            let v = Noun::from(a.clone());
            values.push(if formula { Noun::cell(1u64, v) } else { v });
        }
        Nasm::Axis(name) => match env.get(name) {
            Some(ax) => values.push(Noun::cell(0u64, ax.clone())),
            None => {
                return Err(LowerError::UnboundAxis {
                    name: name.clone(),
                    declared: env.keys().cloned().collect(),
                })
            }
        },
        Nasm::Cell {
            first,
            second,
            rest,
        } => {
            tasks.push(Task::Assemble(Frame::Cell(2 + rest.len())));
            for r in rest.iter().rev() {
                tasks.push(Task::Expand {
                    node: r,
                    env: env.clone(),
                    formula: false,
                });
            }
            tasks.push(Task::Expand {
                node: second,
                env: env.clone(),
                formula: false,
            });
            tasks.push(Task::Expand {
                node: first,
                env,
                formula: false,
            });
        }
        Nasm::Op(op) => schedule_op(op, env, tasks, values),
        Nasm::Let { name, value, body } => {
            // The value is compiled against the old subject; the body
            // sees the binding at axis 2 and everything else pegged
            // under 3.
            let mut new = shift_axes(&env);
            if new.contains_key(name) {
                return Err(LowerError::LetShadows(name.clone()));
            }
            new.insert(name.clone(), Atom::from(2u64));
            tasks.push(Task::Assemble(Frame::Let));
            tasks.push(Task::Expand {
                node: body,
                env: P::new(new),
                formula: true,
            });
            tasks.push(Task::Expand {
                node: value,
                env,
                formula: true,
            });
        }
        Nasm::Match {
            scrutinee,
            arms,
            default,
        } => {
            // Patterns are literal nouns: expanded unlifted, wrapped
            // [1 pat] at assembly for the equality test against the
            // scrutinee at axis 2.
            let new = P::new(shift_axes(&env));
            tasks.push(Task::Assemble(Frame::Match { arms: arms.len() }));
            for arm in arms.iter() {
                tasks.push(Task::Expand {
                    node: &arm.body,
                    env: new.clone(),
                    formula: true,
                });
                tasks.push(Task::Expand {
                    node: &arm.pattern,
                    env: new.clone(),
                    formula: false,
                });
            }
            tasks.push(Task::Expand {
                node: default,
                env: new,
                formula: true,
            });
            tasks.push(Task::Expand {
                node: scrutinee,
                env,
                formula: true,
            });
        }
    }
    Ok(())
}

/// Schedule one opcode application: zero-child ops finish immediately;
/// the rest put an assembly frame under their children, with the
/// reference argument kinds (`formula` for 'f' positions, plain
/// expansion for 'n' positions, and axis atoms carried in the frame).
fn schedule_op<'a>(op: &'a Op, env: Env, tasks: &mut Vec<Task<'a>>, values: &mut Vec<Noun>) {
    // Two local shapes: `f` schedules a formula position, `n` a noun
    // position. Push order is reversed below so children execute
    // left-to-right.
    match op {
        Op::Self_ => values.push(noun![0 1]),
        Op::Battery => values.push(noun![0 2]),
        Op::Payload => values.push(noun![0 3]),
        Op::Sample => values.push(noun![0 6]),
        Op::Context => values.push(noun![0 7]),
        Op::Crash => values.push(noun![0 0]),
        Op::Slot(ax) => values.push(noun![0(ax.clone())]),
        Op::Const(x) | Op::Arm(x) => {
            tasks.push(Task::Assemble(Frame::Quote));
            tasks.push(Task::Expand {
                node: x,
                env,
                formula: false,
            });
        }
        Op::Isa(x) => {
            tasks.push(Task::Assemble(Frame::Isa));
            tasks.push(Task::Expand {
                node: x,
                env,
                formula: true,
            });
        }
        Op::Inc(x) => {
            tasks.push(Task::Assemble(Frame::Inc));
            tasks.push(Task::Expand {
                node: x,
                env,
                formula: true,
            });
        }
        Op::Eval(a, b) => {
            tasks.push(Task::Assemble(Frame::Eval));
            tasks.push(Task::Expand {
                node: b,
                env: env.clone(),
                formula: true,
            });
            tasks.push(Task::Expand {
                node: a,
                env,
                formula: true,
            });
        }
        Op::Eq(a, b) => {
            tasks.push(Task::Assemble(Frame::Eq));
            tasks.push(Task::Expand {
                node: b,
                env: env.clone(),
                formula: true,
            });
            tasks.push(Task::Expand {
                node: a,
                env,
                formula: true,
            });
        }
        Op::Comp(a, b) => {
            tasks.push(Task::Assemble(Frame::Comp));
            tasks.push(Task::Expand {
                node: b,
                env: env.clone(),
                formula: true,
            });
            tasks.push(Task::Expand {
                node: a,
                env,
                formula: true,
            });
        }
        Op::Push(a, b) => {
            tasks.push(Task::Assemble(Frame::Push));
            tasks.push(Task::Expand {
                node: b,
                env: env.clone(),
                formula: true,
            });
            tasks.push(Task::Expand {
                node: a,
                env,
                formula: true,
            });
        }
        Op::If(c, t, e) => {
            tasks.push(Task::Assemble(Frame::If));
            tasks.push(Task::Expand {
                node: e,
                env: env.clone(),
                formula: true,
            });
            tasks.push(Task::Expand {
                node: t,
                env: env.clone(),
                formula: true,
            });
            tasks.push(Task::Expand {
                node: c,
                env,
                formula: true,
            });
        }
        Op::Call(ax, f) => {
            tasks.push(Task::Assemble(Frame::Call(ax.clone())));
            tasks.push(Task::Expand {
                node: f,
                env,
                formula: true,
            });
        }
        Op::Edit(ax, v, f) => {
            tasks.push(Task::Assemble(Frame::Edit(ax.clone())));
            tasks.push(Task::Expand {
                node: f,
                env: env.clone(),
                formula: true,
            });
            tasks.push(Task::Expand {
                node: v,
                env,
                formula: true,
            });
        }
        Op::Hint(t, f) => {
            tasks.push(Task::Assemble(Frame::Hint));
            tasks.push(Task::Expand {
                node: f,
                env: env.clone(),
                formula: true,
            });
            tasks.push(Task::Expand {
                node: t,
                env,
                formula: false,
            });
        }
        Op::Hintd(t, c, f) => {
            tasks.push(Task::Assemble(Frame::Hintd));
            tasks.push(Task::Expand {
                node: f,
                env: env.clone(),
                formula: true,
            });
            tasks.push(Task::Expand {
                node: c,
                env: env.clone(),
                formula: true,
            });
            tasks.push(Task::Expand {
                node: t,
                env,
                formula: false,
            });
        }
    }
}

/// Build one noun from the top of the value stack (children
/// left-to-right, leftmost deepest).
fn assemble(frame: Frame, values: &mut Vec<Noun>) -> Noun {
    fn pop(values: &mut Vec<Noun>) -> Noun {
        values.pop().expect("assemble: child value present")
    }
    match frame {
        Frame::Cell(count) => {
            let elems: Vec<Noun> = values.drain(values.len() - count..).collect();
            Noun::autocons(elems).expect("cell has >= 2 elements")
        }
        Frame::Quote => noun![1(pop(values))],
        Frame::Isa => noun![3(pop(values))],
        Frame::Inc => noun![4(pop(values))],
        Frame::Eval => {
            let b = pop(values);
            let a = pop(values);
            noun![2(a)(b)]
        }
        Frame::Eq => {
            let b = pop(values);
            let a = pop(values);
            noun![5(a)(b)]
        }
        Frame::Comp => {
            let b = pop(values);
            let a = pop(values);
            noun![7(a)(b)]
        }
        Frame::Push => {
            let b = pop(values);
            let a = pop(values);
            noun![8(a)(b)]
        }
        Frame::If => {
            let e = pop(values);
            let t = pop(values);
            let c = pop(values);
            noun![6(c)(t)(e)]
        }
        Frame::Call(ax) => noun![9(ax)(pop(values))],
        Frame::Edit(ax) => {
            let f = pop(values);
            let v = pop(values);
            noun![10[(ax)(v)](f)]
        }
        Frame::Hint => {
            let f = pop(values);
            let t = pop(values);
            noun![11(t)(f)]
        }
        Frame::Hintd => {
            let f = pop(values);
            let c = pop(values);
            let t = pop(values);
            noun![11[(t)(c)](f)]
        }
        Frame::Let => {
            let b = pop(values);
            let v = pop(values);
            noun![8(v)(b)]
        }
        Frame::Match { arms } => {
            // Children in evaluation order: scrutinee, default, then
            // (pattern, body) per arm from the last arm to the first —
            // exactly the reference fold, so wrapping in drain order
            // puts the last arm innermost and the first arm outermost.
            let count = 2 + 2 * arms;
            let drained: Vec<Noun> = values.drain(values.len() - count..).collect();
            let mut it = drained.into_iter();
            let s = it.next().expect("scrutinee");
            let mut result = it.next().expect("default");
            while let (Some(p), Some(b)) = (it.next(), it.next()) {
                result = noun![6 [5 [1 (p)] 0 2] (b) (result)];
            }
            noun![8(s)(result)]
        }
    }
}
