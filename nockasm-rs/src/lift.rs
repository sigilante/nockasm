//! `lift`: read a noun as a formula — the deterministic, zero-heuristic
//! reading of the compiler-target spec §5 (sigilante/jock
//! `docs/spec/nockasm-target.md`).
//!
//! Nock is homoiconic: nothing in a noun marks it as code, so the caller
//! asserts the root is a formula and the lift propagates that assumption
//! through Nock's positional grammar. Cell heads 0–12 with well-shaped
//! tails lift to named ops; a cell head means cons-formula (both halves
//! lift); anywhere a formula-position subtree cannot be macro-ized (an
//! atom in formula position, an opcode head above 12, a malformed tail)
//! the node falls back to an opaque `%nock` embed, making the lift total
//! as a formula reader. Data positions (opcode-1 payloads, dynamic hint
//! tags) stay structural raw cells: they are nouns, not formulas, so
//! `%nock` would be a false claim. No intent is ever claimed: constants
//! are `%const` (never `%arm`), axes are `%slot` (never the core
//! aliases), and no macro skeleton is recognized.
//!
//! Soundness law: `lower(None, &lift(f)) == f` for every noun `f`.
//!
//! The traversal runs on an explicit stack, one post-order pass: a task
//! stack of pending reads and assembly frames, and a value stack of
//! completed nodes. Nouns straight off the wire (`cue` of a hostile
//! jamfile) can nest arbitrarily deep in any direction — formula tails,
//! cons-formula heads, opcode-1 payloads — and none of it touches the
//! call stack. O(nodes) time, O(depth + spine width) auxiliary space.

use crate::ast::{Nasm, Op};
use crate::error::CueError;
use crate::jam::cue;
use crate::noun::{Atom, Noun, NounRef};
use crate::render::render;

/// One pending step of the traversal.
enum Task<'a> {
    /// Read this noun as a formula.
    Lift(&'a Noun),
    /// Read this noun as pure structure (the raw-cell fallback).
    Ast(&'a Noun),
    /// Take this frame's completed children off the value stack and
    /// push the assembled node.
    Assemble(Frame),
}

/// How to assemble a node from its completed children.
///
/// Children are always scheduled to execute left-to-right, so the
/// leftmost child's value sits deepest in the value stack: fixed-arity
/// frames pop right-to-left, and [`Frame::Raw`] drains its range in
/// order.
enum Frame {
    /// Cons-formula: two lifted halves.
    Cons,
    /// Structural raw cell of this many elements.
    Raw(usize),
    /// `[1 X]` — the payload is one structural child.
    Const,
    Eval,
    Isa,
    Inc,
    Eq,
    If,
    Comp,
    Push,
    /// `[9 ax f]` — the axis is already known; one lifted child.
    Call(Atom),
    /// `[10 [ax v] f]` — the axis is already known; two lifted children.
    Edit(Atom),
    /// `[11 tag f]` — the atom tag is already known; one lifted child.
    Hint(Atom),
    /// `[11 [tag clue] f]` — structural tag, lifted clue and body.
    Hintd,
    /// `[12 ref path]` — two lifted formula children.
    Scry,
}

/// Read a noun as a formula (see the module docs and soundness law).
pub fn lift(n: &Noun) -> Nasm {
    let mut tasks: Vec<Task<'_>> = vec![Task::Lift(n)];
    let mut values: Vec<Nasm> = Vec::new();
    while let Some(task) = tasks.pop() {
        match task {
            Task::Lift(n) => lift_step(n, &mut tasks, &mut values),
            Task::Ast(n) => ast_step(n, &mut tasks, &mut values),
            Task::Assemble(frame) => {
                let node = assemble(frame, &mut values);
                values.push(node);
            }
        }
    }
    debug_assert_eq!(values.len(), 1, "every task tree yields one value");
    values.pop().expect("the root value")
}

/// One formula-position read: push a finished leaf, or schedule an
/// assembly frame under its children (leftmost child on top, so it
/// executes first). The shape guards mirror the reference lift exactly;
/// every ill-shaped case falls back to an opaque `%nock` embed of the
/// whole node.
fn lift_step<'a>(n: &'a Noun, tasks: &mut Vec<Task<'a>>, values: &mut Vec<Nasm>) {
    let (h, t) = match n.view() {
        NounRef::Atom(_) => {
            // An atom is never a formula.
            values.push(Nasm::Nock(n.clone()));
            return;
        }
        NounRef::Cell(h, t) => (h, t),
    };
    let NounRef::Atom(opcode) = h.view() else {
        // Cons-formula: both halves are formula positions.
        tasks.push(Task::Assemble(Frame::Cons));
        tasks.push(Task::Lift(t));
        tasks.push(Task::Lift(h));
        return;
    };
    match opcode.as_u64() {
        Some(0) => match t.view() {
            NounRef::Atom(ax) => values.push(Nasm::Op(Op::Slot(ax.clone()))),
            NounRef::Cell(..) => values.push(Nasm::Nock(n.clone())),
        },
        Some(1) => {
            tasks.push(Task::Assemble(Frame::Const));
            tasks.push(Task::Ast(t));
        }
        Some(2) => match t.as_cell() {
            Some((s, f)) if s.is_cell() && f.is_cell() => {
                tasks.push(Task::Assemble(Frame::Eval));
                tasks.push(Task::Lift(f));
                tasks.push(Task::Lift(s));
            }
            _ => values.push(Nasm::Nock(n.clone())),
        },
        Some(3) if t.is_cell() => {
            tasks.push(Task::Assemble(Frame::Isa));
            tasks.push(Task::Lift(t));
        }
        Some(4) if t.is_cell() => {
            tasks.push(Task::Assemble(Frame::Inc));
            tasks.push(Task::Lift(t));
        }
        Some(5) => match t.as_cell() {
            Some((x, y)) if x.is_cell() && y.is_cell() => {
                tasks.push(Task::Assemble(Frame::Eq));
                tasks.push(Task::Lift(y));
                tasks.push(Task::Lift(x));
            }
            _ => values.push(Nasm::Nock(n.clone())),
        },
        Some(6) => match t.as_cell() {
            Some((c, branches)) if c.is_cell() => match branches.as_cell() {
                Some((th, el)) if th.is_cell() && el.is_cell() => {
                    tasks.push(Task::Assemble(Frame::If));
                    tasks.push(Task::Lift(el));
                    tasks.push(Task::Lift(th));
                    tasks.push(Task::Lift(c));
                }
                _ => values.push(Nasm::Nock(n.clone())),
            },
            _ => values.push(Nasm::Nock(n.clone())),
        },
        Some(7) => match t.as_cell() {
            Some((x, y)) if x.is_cell() && y.is_cell() => {
                tasks.push(Task::Assemble(Frame::Comp));
                tasks.push(Task::Lift(y));
                tasks.push(Task::Lift(x));
            }
            _ => values.push(Nasm::Nock(n.clone())),
        },
        Some(8) => match t.as_cell() {
            Some((x, y)) if x.is_cell() && y.is_cell() => {
                tasks.push(Task::Assemble(Frame::Push));
                tasks.push(Task::Lift(y));
                tasks.push(Task::Lift(x));
            }
            _ => values.push(Nasm::Nock(n.clone())),
        },
        Some(9) => match t.as_cell() {
            Some((ax, f)) if f.is_cell() => match ax.view() {
                NounRef::Atom(ax) => {
                    tasks.push(Task::Assemble(Frame::Call(ax.clone())));
                    tasks.push(Task::Lift(f));
                }
                NounRef::Cell(..) => values.push(Nasm::Nock(n.clone())),
            },
            _ => values.push(Nasm::Nock(n.clone())),
        },
        Some(10) => match t.as_cell() {
            Some((spec, f)) if f.is_cell() => match spec.as_cell() {
                Some((ax, v)) if v.is_cell() => match ax.view() {
                    NounRef::Atom(ax) => {
                        tasks.push(Task::Assemble(Frame::Edit(ax.clone())));
                        tasks.push(Task::Lift(f));
                        tasks.push(Task::Lift(v));
                    }
                    NounRef::Cell(..) => values.push(Nasm::Nock(n.clone())),
                },
                _ => values.push(Nasm::Nock(n.clone())),
            },
            _ => values.push(Nasm::Nock(n.clone())),
        },
        Some(11) => match t.as_cell() {
            Some((tag, f)) if tag.is_atom() && f.is_cell() => {
                let NounRef::Atom(tag) = tag.view() else {
                    unreachable!("guard checked is_atom");
                };
                tasks.push(Task::Assemble(Frame::Hint(tag.clone())));
                tasks.push(Task::Lift(f));
            }
            Some((spec, f)) if f.is_cell() => match spec.as_cell() {
                Some((tag, clue)) if clue.is_cell() => {
                    tasks.push(Task::Assemble(Frame::Hintd));
                    tasks.push(Task::Lift(f));
                    tasks.push(Task::Lift(clue));
                    tasks.push(Task::Ast(tag));
                }
                _ => values.push(Nasm::Nock(n.clone())),
            },
            _ => values.push(Nasm::Nock(n.clone())),
        },
        Some(12) => match t.as_cell() {
            Some((r, p)) if r.is_cell() && p.is_cell() => {
                tasks.push(Task::Assemble(Frame::Scry));
                tasks.push(Task::Lift(p));
                tasks.push(Task::Lift(r));
            }
            _ => values.push(Nasm::Nock(n.clone())),
        },
        _ => values.push(Nasm::Nock(n.clone())),
    }
}

/// One structural read (the `noun_ast` of the reference): atoms finish
/// immediately; a cell right-spine-flattens into a [`Frame::Raw`] over
/// every spine element, scheduled leftmost-first.
fn ast_step<'a>(n: &'a Noun, tasks: &mut Vec<Task<'a>>, values: &mut Vec<Nasm>) {
    match n.view() {
        NounRef::Atom(a) => values.push(Nasm::Atom(a.clone())),
        NounRef::Cell(..) => {
            // Walk the spine pushing elements in encounter order, then
            // patch the frame's count and reverse the just-pushed range
            // so the leftmost element pops first — no temporary buffer.
            let base = tasks.len();
            tasks.push(Task::Assemble(Frame::Raw(0)));
            let mut count = 0usize;
            let mut cur = n;
            while let NounRef::Cell(h, t) = cur.view() {
                tasks.push(Task::Ast(h));
                count += 1;
                cur = t;
            }
            tasks.push(Task::Ast(cur));
            count += 1;
            tasks[base] = Task::Assemble(Frame::Raw(count));
            tasks[base + 1..].reverse();
        }
    }
}

/// Build one node from the top of the value stack (children left-to-
/// right, leftmost deepest).
fn assemble(frame: Frame, values: &mut Vec<Nasm>) -> Nasm {
    fn pop(values: &mut Vec<Nasm>) -> Box<Nasm> {
        Box::new(values.pop().expect("assemble: child value present"))
    }
    match frame {
        Frame::Cons => {
            let second = pop(values);
            let first = pop(values);
            Nasm::Cell {
                first,
                second,
                rest: Vec::new(),
            }
        }
        Frame::Raw(count) => {
            let elems: Vec<Nasm> = values.drain(values.len() - count..).collect();
            Nasm::raw_cell(elems).expect("spine of a cell has >= 2 elements")
        }
        Frame::Const => Nasm::Op(Op::Const(pop(values))),
        Frame::Isa => Nasm::Op(Op::Isa(pop(values))),
        Frame::Inc => Nasm::Op(Op::Inc(pop(values))),
        Frame::Eval => {
            let f = pop(values);
            let s = pop(values);
            Nasm::Op(Op::Eval(s, f))
        }
        Frame::Eq => {
            let y = pop(values);
            let x = pop(values);
            Nasm::Op(Op::Eq(x, y))
        }
        Frame::Comp => {
            let y = pop(values);
            let x = pop(values);
            Nasm::Op(Op::Comp(x, y))
        }
        Frame::Push => {
            let y = pop(values);
            let x = pop(values);
            Nasm::Op(Op::Push(x, y))
        }
        Frame::If => {
            let el = pop(values);
            let th = pop(values);
            let c = pop(values);
            Nasm::Op(Op::If(c, th, el))
        }
        Frame::Call(ax) => Nasm::Op(Op::Call(ax, pop(values))),
        Frame::Edit(ax) => {
            let f = pop(values);
            let v = pop(values);
            Nasm::Op(Op::Edit(ax, v, f))
        }
        Frame::Hint(tag) => Nasm::Op(Op::Hint(Box::new(Nasm::Atom(tag)), pop(values))),
        Frame::Hintd => {
            let f = pop(values);
            let clue = pop(values);
            let tag = pop(values);
            Nasm::Op(Op::Hintd(tag, clue, f))
        }
        Frame::Scry => {
            let path = pop(values);
            let reference = pop(values);
            Nasm::Op(Op::Scry(reference, path))
        }
    }
}

/// Jamfile bytes to canonical `.nasm` source for the jammed formula:
/// [`cue`], then [`lift`], then [`render`](crate::render).
pub fn nasm_from_jam(data: &[u8]) -> Result<String, CueError> {
    let noun = cue(data)?;
    Ok(render(None, &lift(&noun)))
}
