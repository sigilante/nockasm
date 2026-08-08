//! `render`: IR to canonical `.nasm` text.
//!
//! This is the normative "canonical rendering v1" of the compiler-target
//! spec (sigilante/jock `docs/spec/nockasm-target.md`), **byte-identical**
//! to the Python and Hoon renderers: every layout decision is a pure
//! function of the IR value,
//! the indent, and the *reserve* (how many closing delimiters an
//! enclosing form will append to the final line), under a 76-column
//! limit. Nothing remembers source spelling — atoms render from their
//! value (cord form iff at least two bytes, all printable ASCII, no
//! quote; else dotted decimal).
//!
//! The implementation is two explicit-stack passes — the output bytes
//! are the specification; only the algorithm differs from the
//! references:
//!
//! 1. **Sizing**: build a shadow of the render tree, annotating every
//!    node with the *length* of its single-line form (`None` where a
//!    `#let`/`#match` forces tall layout) and caching the rendered text
//!    of atom and axis leaves. Every layout decision consumes only
//!    lengths, and a composite's length is O(1) from its children's —
//!    composite wide *strings* are deliberately never stored (in a
//!    nested all-wide tree that would be quadratic memory).
//! 2. **Emission**: walk the shadow with layout frames, appending into
//!    one line buffer; a node that fits wide materializes its text
//!    exactly once, so each node's text enters the output once.
//!
//! Total O(input + output) time and no recursion — the references
//! recompute wide forms at every level, which is O(n · depth), and IR
//! of any depth renders here without touching the call stack. (Deeply
//! *tall* output is still quadratic in size from indentation alone;
//! that is the format, not the renderer.)

use crate::ast::{ArgRef, Nasm, Program, Schema};
use crate::noun::{Atom, NounRef};

const WIDTH: usize = 76;

/// Render an IR value to canonical `.nasm` source (newline-terminated).
///
/// The round-trip law, checked by the differential suites:
/// `expand(&render(schema, expr)) == lower(schema, expr)` for every
/// well-formed IR value the parser can re-admit, and rendering is
/// idempotent through `parse`.
pub fn render(schema: Option<&Schema>, expr: &Nasm) -> String {
    let mut lines: Vec<String> = Vec::new();
    if let Some(s) = schema {
        lines.push(format!(":subject {}", schema_text(s)));
    }
    let shadow = size(ArgRef::Expr(expr));
    rend_lines(&shadow, &mut lines);
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

impl Program {
    /// Render this program to canonical `.nasm` source.
    pub fn render(&self) -> String {
        render(self.schema.as_ref(), &self.body)
    }
}

// ----------------------------------------------------------------------
// Atom text
// ----------------------------------------------------------------------

/// Decimal with dots every three digits, matching the reader.
fn dotted(n: &Atom) -> String {
    let digits = n.to_decimal_string();
    let bytes = digits.as_bytes();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    let lead = if bytes.len() % 3 == 0 {
        3
    } else {
        bytes.len() % 3
    };
    for (i, b) in bytes.iter().enumerate() {
        if i != 0 && (i + 3 - lead) % 3 == 0 {
            out.push('.');
        }
        out.push(*b as char);
    }
    out
}

/// Cord form iff the value is >= 2 bytes, every byte printable ASCII
/// (0x20–0x7E) excluding the quote (0x27); else dotted decimal.
fn atom_text(n: &Atom) -> String {
    if n.bit_len() >= 9 {
        let bytes = n.le_bytes();
        if bytes
            .iter()
            .all(|&b| (0x20..=0x7e).contains(&b) && b != 0x27)
        {
            let mut out = String::with_capacity(bytes.len() + 2);
            out.push('\'');
            for &b in bytes.iter() {
                out.push(b as char);
            }
            out.push('\'');
            return out;
        }
    }
    dotted(n)
}

// ----------------------------------------------------------------------
// Schemas
// ----------------------------------------------------------------------

/// Schemas always render wide; right spines flatten at every pair
/// (`{.a .b .c}`, never `{.a {.b .c}}`).
fn schema_text(s: &Schema) -> String {
    enum Tok<'x> {
        Sch(&'x Schema),
        Str(&'static str),
    }
    let mut out = String::new();
    let mut stack: Vec<Tok<'_>> = vec![Tok::Sch(s)];
    while let Some(t) = stack.pop() {
        match t {
            Tok::Str(x) => out.push_str(x),
            Tok::Sch(s) => match s {
                Schema::Leaf(name) => {
                    out.push('.');
                    out.push_str(name.as_str());
                }
                Schema::Hole => out.push('_'),
                Schema::Pair(..) => {
                    let mut elems: Vec<&Schema> = Vec::new();
                    let mut cur = s;
                    while let Schema::Pair(head, tail) = cur {
                        elems.push(head);
                        cur = tail;
                    }
                    elems.push(cur);
                    out.push('{');
                    stack.push(Tok::Str("}"));
                    for (i, e) in elems.into_iter().enumerate().rev() {
                        stack.push(Tok::Sch(e));
                        if i > 0 {
                            stack.push(Tok::Str(" "));
                        }
                    }
                }
            },
        }
    }
    out
}

// ----------------------------------------------------------------------
// Pass 1: sizing
// ----------------------------------------------------------------------

/// A sizing shadow of the render tree: one node per rendered argument,
/// children in render order, annotated with the single-line length.
///
/// Child layout by node kind — the emitter indexes by it:
/// - atoms and axes: none (leaves, text cached);
/// - raw cells: the elements, in order;
/// - ops: the arguments, in order (axis atoms are leaves);
/// - `#let`: `[value, body]`;
/// - `#match`: `[scrutinee, pat_1, body_1, ..., pat_k, body_k, default]`.
struct Shadow<'a> {
    arg: ArgRef<'a>,
    /// Length of the wide (single-line) form; `None` when a
    /// `#let`/`#match` makes the subtree tall-only.
    wide: Option<usize>,
    /// Rendered text for atom and axis leaves — needed verbatim at
    /// emission, and the only way to know a bignum's rendered length.
    leaf: Option<String>,
    children: Vec<Shadow<'a>>,
}

enum SizeTask<'a> {
    Visit(ArgRef<'a>),
    Build { arg: ArgRef<'a>, count: usize },
}

/// Build the shadow tree, post-order on an explicit stack.
fn size(root: ArgRef<'_>) -> Shadow<'_> {
    let mut tasks: Vec<SizeTask<'_>> = vec![SizeTask::Visit(root)];
    let mut done: Vec<Shadow<'_>> = Vec::new();
    while let Some(task) = tasks.pop() {
        match task {
            SizeTask::Visit(arg) => visit(arg, &mut tasks, &mut done),
            SizeTask::Build { arg, count } => {
                let children: Vec<Shadow<'_>> = done.drain(done.len() - count..).collect();
                done.push(build_shadow(arg, children));
            }
        }
    }
    debug_assert_eq!(done.len(), 1, "every visit yields one shadow");
    done.pop().expect("the root shadow")
}

/// One sizing step: leaves finish immediately with their text; a
/// composite schedules a build under its children in layout order
/// (leftmost on top of the task stack, so it visits first).
fn visit<'a>(arg: ArgRef<'a>, tasks: &mut Vec<SizeTask<'a>>, done: &mut Vec<Shadow<'a>>) {
    let leaf_text = match arg {
        ArgRef::Axis(a) => Some(atom_text(a)),
        ArgRef::Noun(n) => match n.view() {
            NounRef::Atom(a) => Some(atom_text(a)),
            NounRef::Cell(..) => None,
        },
        ArgRef::Expr(Nasm::Atom(a)) => Some(atom_text(a)),
        ArgRef::Expr(Nasm::Axis(name)) => Some(format!(".{name}")),
        ArgRef::Expr(_) => None,
    };
    if let Some(text) = leaf_text {
        done.push(Shadow {
            arg,
            wide: Some(text.len()),
            leaf: Some(text),
            children: Vec::new(),
        });
        return;
    }
    let expr = match arg {
        ArgRef::Expr(expr) => expr,
        ArgRef::Noun(n) => {
            // A `%nock` payload cell reads as pure structure: atoms and
            // right-spine-flattened raw cells, exactly the reference
            // noun-ast.
            let mut count = 0usize;
            let mut cur = n;
            let mut elems: Vec<ArgRef<'a>> = Vec::new();
            while let NounRef::Cell(h, t) = cur.view() {
                elems.push(ArgRef::Noun(h));
                count += 1;
                cur = t;
            }
            elems.push(ArgRef::Noun(cur));
            count += 1;
            tasks.push(SizeTask::Build { arg, count });
            for e in elems.into_iter().rev() {
                tasks.push(SizeTask::Visit(e));
            }
            return;
        }
        ArgRef::Axis(_) => unreachable!("axis atoms are leaves"),
    };
    match expr {
        Nasm::Atom(_) | Nasm::Axis(_) => unreachable!("leaves handled above"),
        Nasm::Nock(payload) => {
            tasks.push(SizeTask::Build { arg, count: 1 });
            tasks.push(SizeTask::Visit(ArgRef::Noun(payload)));
        }
        Nasm::Cell {
            first,
            second,
            rest,
        } => {
            tasks.push(SizeTask::Build {
                arg,
                count: 2 + rest.len(),
            });
            for r in rest.iter().rev() {
                tasks.push(SizeTask::Visit(ArgRef::Expr(r)));
            }
            tasks.push(SizeTask::Visit(ArgRef::Expr(second)));
            tasks.push(SizeTask::Visit(ArgRef::Expr(first)));
        }
        Nasm::Op(op) => {
            let args = op.args();
            tasks.push(SizeTask::Build {
                arg,
                count: args.len(),
            });
            for a in args.into_iter().rev() {
                tasks.push(SizeTask::Visit(a));
            }
        }
        Nasm::Let { value, body, .. } => {
            tasks.push(SizeTask::Build { arg, count: 2 });
            tasks.push(SizeTask::Visit(ArgRef::Expr(body)));
            tasks.push(SizeTask::Visit(ArgRef::Expr(value)));
        }
        Nasm::Match {
            scrutinee,
            arms,
            default,
        } => {
            tasks.push(SizeTask::Build {
                arg,
                count: 2 + 2 * arms.len(),
            });
            tasks.push(SizeTask::Visit(ArgRef::Expr(default)));
            for arm in arms.iter().rev() {
                tasks.push(SizeTask::Visit(ArgRef::Expr(&arm.body)));
                tasks.push(SizeTask::Visit(ArgRef::Expr(&arm.pattern)));
            }
            tasks.push(SizeTask::Visit(ArgRef::Expr(scrutinee)));
        }
    }
}

/// Assemble a composite shadow, computing its wide length from the
/// children's — mirroring the wide forms `[a b …]` and `(%name a …)`.
fn build_shadow<'a>(arg: ArgRef<'a>, children: Vec<Shadow<'a>>) -> Shadow<'a> {
    // Sum of child lengths plus the separating spaces, if all are wide.
    let joined = |kids: &[Shadow<'_>]| -> Option<usize> {
        let mut total = 0usize;
        for k in kids {
            total += k.wide?;
        }
        Some(total + kids.len() - 1)
    };
    let wide = match arg {
        // `[e1 e2 …]` — payload cells share the raw-cell wide form.
        ArgRef::Noun(_) => joined(&children).map(|j| j + 2),
        ArgRef::Axis(_) => unreachable!("axis atoms are leaves"),
        ArgRef::Expr(expr) => match expr {
            Nasm::Cell { .. } => joined(&children).map(|j| j + 2),
            Nasm::Op(op) => {
                if children.is_empty() {
                    Some(3 + op.name().len())
                } else {
                    joined(&children).map(|j| j + 4 + op.name().len())
                }
            }
            // `(%nock ` + payload + `)`; payload structural readings
            // always have a wide form.
            Nasm::Nock(_) => children[0].wide.map(|w| w + 8),
            Nasm::Let { .. } | Nasm::Match { .. } => None,
            Nasm::Atom(_) | Nasm::Axis(_) => unreachable!("leaves never build"),
        },
    };
    Shadow {
        arg,
        wide,
        leaf: None,
        children,
    }
}

/// Materialize a wide form (single line, no indent) — called exactly
/// once per emitted-wide node, walking the subtree with a token stack.
fn wide_text(sh: &Shadow<'_>) -> String {
    enum Tok<'s, 'a> {
        Sh(&'s Shadow<'a>),
        Str(&'static str),
    }
    let capacity = sh.wide.expect("wide_text needs a wide form");
    let mut out = String::with_capacity(capacity);
    let mut stack: Vec<Tok<'_, '_>> = vec![Tok::Sh(sh)];
    while let Some(t) = stack.pop() {
        match t {
            Tok::Str(s) => out.push_str(s),
            Tok::Sh(sh) => {
                if let Some(text) = &sh.leaf {
                    out.push_str(text);
                    continue;
                }
                let is_cell_form = match sh.arg {
                    ArgRef::Noun(_) => true,
                    ArgRef::Expr(Nasm::Cell { .. }) => true,
                    ArgRef::Expr(_) => false,
                    ArgRef::Axis(_) => unreachable!("axis atoms are leaves"),
                };
                if is_cell_form {
                    out.push('[');
                    stack.push(Tok::Str("]"));
                    for (i, k) in sh.children.iter().enumerate().rev() {
                        stack.push(Tok::Sh(k));
                        if i > 0 {
                            stack.push(Tok::Str(" "));
                        }
                    }
                    continue;
                }
                let ArgRef::Expr(expr) = sh.arg else {
                    unreachable!("cell forms handled above");
                };
                match expr {
                    Nasm::Op(op) => {
                        out.push_str("(%");
                        out.push_str(op.name());
                        stack.push(Tok::Str(")"));
                        for k in sh.children.iter().rev() {
                            stack.push(Tok::Sh(k));
                            stack.push(Tok::Str(" "));
                        }
                    }
                    Nasm::Nock(_) => {
                        out.push_str("(%nock ");
                        stack.push(Tok::Str(")"));
                        stack.push(Tok::Sh(&sh.children[0]));
                    }
                    _ => unreachable!("let/match have no wide form"),
                }
            }
        }
    }
    debug_assert_eq!(out.len(), capacity, "sizing and emission agree");
    out
}

// ----------------------------------------------------------------------
// Pass 2: emission
// ----------------------------------------------------------------------

fn pad(ind: usize) -> String {
    " ".repeat(ind)
}

enum EmitTask<'s, 'a> {
    /// Render this shadow at an indent, with `res` characters of
    /// enclosing closing-delimiters reserved on its final line.
    Rend {
        sh: &'s Shadow<'a>,
        ind: usize,
        res: usize,
    },
    /// One `#match` arm; a `None` pattern is the `_` default.
    Case {
        pattern: Option<&'s Shadow<'a>>,
        body: &'s Shadow<'a>,
        ind: usize,
    },
    /// Push a pre-formatted line.
    Line(String),
    /// Append a suffix to the current final line (closing delimiters,
    /// the arm arrow).
    Append(&'static str),
    /// The tall-cell merge: rewrite two indent spaces of an
    /// already-emitted line into `[ ` (an equal-length splice).
    MergeOpen { line: usize, ind: usize },
}

fn rend_lines(root: &Shadow<'_>, out: &mut Vec<String>) {
    let mut tasks: Vec<EmitTask<'_, '_>> = vec![EmitTask::Rend {
        sh: root,
        ind: 0,
        res: 0,
    }];
    while let Some(task) = tasks.pop() {
        match task {
            EmitTask::Line(s) => out.push(s),
            EmitTask::Append(suffix) => out.last_mut().expect("a line to amend").push_str(suffix),
            EmitTask::MergeOpen { line, ind } => {
                out[line].replace_range(ind..ind + 2, "[ ");
            }
            EmitTask::Rend { sh, ind, res } => rend_step(sh, ind, res, &mut tasks, out),
            EmitTask::Case { pattern, body, ind } => case_step(pattern, body, ind, &mut tasks, out),
        }
    }
}

/// One rend step: emit the wide form when it fits (atoms and axes emit
/// theirs fitting or not), else emit this node's head lines now and
/// schedule its children with the layout frame actions between them.
fn rend_step<'s, 'a>(
    sh: &'s Shadow<'a>,
    ind: usize,
    res: usize,
    tasks: &mut Vec<EmitTask<'s, 'a>>,
    out: &mut Vec<String>,
) {
    if let Some(w) = sh.wide {
        if ind + w + res <= WIDTH {
            out.push(format!("{}{}", pad(ind), wide_text(sh)));
            return;
        }
    }
    if let Some(text) = &sh.leaf {
        // Atoms and axes always have a wide form; emit it even when it
        // does not fit.
        out.push(format!("{}{text}", pad(ind)));
        return;
    }
    let is_cell_form = match sh.arg {
        ArgRef::Noun(_) => true,
        ArgRef::Expr(Nasm::Cell { .. }) => true,
        ArgRef::Expr(_) => false,
        ArgRef::Axis(_) => unreachable!("axis atoms are leaves"),
    };
    if is_cell_form {
        // `[ ` merged with the first element's first line (two
        // columns, so continuations align); remaining elements at
        // indent + 2; `]` appended to the final line. Raw cells and
        // `%nock` payload cells share this layout.
        let kids = &sh.children;
        let last = kids.len() - 1;
        tasks.push(EmitTask::Append("]"));
        tasks.push(EmitTask::Rend {
            sh: &kids[last],
            ind: ind + 2,
            res: res + 1,
        });
        for middle in kids[1..last].iter().rev() {
            tasks.push(EmitTask::Rend {
                sh: middle,
                ind: ind + 2,
                res: 0,
            });
        }
        tasks.push(EmitTask::MergeOpen {
            line: out.len(),
            ind,
        });
        tasks.push(EmitTask::Rend {
            sh: &kids[0],
            ind: ind + 2,
            res: 0,
        });
        return;
    }
    let ArgRef::Expr(expr) = sh.arg else {
        unreachable!("cell forms handled above");
    };
    match expr {
        Nasm::Atom(_) | Nasm::Axis(_) => unreachable!("leaves handled above"),
        Nasm::Cell { .. } => unreachable!("cell forms handled above"),
        Nasm::Nock(_) => {
            // Tall form mirrors a one-argument op; the payload renders
            // as its structural reading.
            out.push(format!("{}(%nock", pad(ind)));
            tasks.push(EmitTask::Append(")"));
            tasks.push(EmitTask::Rend {
                sh: &sh.children[0],
                ind: ind + 2,
                res: res + 1,
            });
        }
        Nasm::Op(op) => {
            if sh.children.is_empty() {
                out.push(format!("{}(%{})", pad(ind), op.name()));
                return;
            }
            out.push(format!("{}(%{}", pad(ind), op.name()));
            let last = sh.children.len() - 1;
            tasks.push(EmitTask::Append(")"));
            tasks.push(EmitTask::Rend {
                sh: &sh.children[last],
                ind: ind + 2,
                res: res + 1,
            });
            for a in sh.children[..last].iter().rev() {
                tasks.push(EmitTask::Rend {
                    sh: a,
                    ind: ind + 2,
                    res: 0,
                });
            }
        }
        Nasm::Let { name, .. } => {
            let value = &sh.children[0];
            let body = &sh.children[1];
            // "#let ." + name + " =" is name.len() + 8 columns; the
            // one-liner adds " " + value + " in".
            let head_len = ind + name.as_str().len() + 8;
            let one_line = value.wide.is_some_and(|vw| head_len + vw + 4 <= WIDTH);
            tasks.push(EmitTask::Rend { sh: body, ind, res });
            if one_line {
                out.push(format!(
                    "{}#let .{name} = {} in",
                    pad(ind),
                    wide_text(value)
                ));
            } else {
                tasks.push(EmitTask::Line(format!("{}in", pad(ind))));
                tasks.push(EmitTask::Rend {
                    sh: value,
                    ind: ind + 2,
                    res: 0,
                });
                out.push(format!("{}#let .{name} =", pad(ind)));
            }
        }
        Nasm::Match { .. } => {
            // Note: the reserve is deliberately unused — the reference
            // renderers do not account for it in the `#match` head or
            // closing `}` lines.
            let kids = &sh.children;
            let arms = (kids.len() - 2) / 2;
            let scrutinee = &kids[0];
            let default = &kids[kids.len() - 1];
            tasks.push(EmitTask::Line(format!("{}}}", pad(ind))));
            tasks.push(EmitTask::Case {
                pattern: None,
                body: default,
                ind: ind + 2,
            });
            for i in (0..arms).rev() {
                tasks.push(EmitTask::Case {
                    pattern: Some(&kids[1 + 2 * i]),
                    body: &kids[2 + 2 * i],
                    ind: ind + 2,
                });
            }
            // "#match " + scrutinee + " {" is scrutinee + 9 columns.
            let head_fits = scrutinee.wide.is_some_and(|sw| ind + sw + 9 <= WIDTH);
            if head_fits {
                out.push(format!("{}#match {} {{", pad(ind), wide_text(scrutinee)));
            } else {
                tasks.push(EmitTask::Line(format!("{}{{", pad(ind))));
                tasks.push(EmitTask::Rend {
                    sh: scrutinee,
                    ind: ind + 2,
                    res: 0,
                });
                out.push(format!("{}#match", pad(ind)));
            }
        }
    }
}

/// One `#match` arm at indent `ind`; a `None` pattern is the `_`
/// default. Three layouts, decided on lengths alone: `P => B` on one
/// line when both fit; `P =>` with the body below; else the pattern
/// tall with ` =>` appended and the body below.
fn case_step<'s, 'a>(
    pattern: Option<&'s Shadow<'a>>,
    body: &'s Shadow<'a>,
    ind: usize,
    tasks: &mut Vec<EmitTask<'s, 'a>>,
    out: &mut Vec<String>,
) {
    let pattern_text = |pattern: Option<&Shadow<'_>>| match pattern {
        None => "_".to_string(),
        Some(p) => wide_text(p),
    };
    let pw_len = match pattern {
        None => Some(1),
        Some(p) => p.wide,
    };
    if let (Some(pl), Some(bl)) = (pw_len, body.wide) {
        // pad + P + " => " + B
        if ind + pl + 4 + bl <= WIDTH {
            out.push(format!(
                "{}{} => {}",
                pad(ind),
                pattern_text(pattern),
                wide_text(body)
            ));
            return;
        }
    }
    if let Some(pl) = pw_len {
        // pad + P + " =>"
        if ind + pl + 3 <= WIDTH {
            out.push(format!("{}{} =>", pad(ind), pattern_text(pattern)));
            tasks.push(EmitTask::Rend {
                sh: body,
                ind: ind + 2,
                res: 0,
            });
            return;
        }
    }
    tasks.push(EmitTask::Rend {
        sh: body,
        ind: ind + 2,
        res: 0,
    });
    tasks.push(EmitTask::Append(" =>"));
    match pattern {
        None => out.push(format!("{}_", pad(ind))),
        Some(p) => tasks.push(EmitTask::Rend { sh: p, ind, res: 3 }),
    }
}
