//! `render`: IR to canonical `.nasm` text.
//!
//! This is the normative "canonical rendering v1" of
//! `doc/compiler-target.md`, ported to be **byte-identical** to the
//! Python and Hoon renderers: every layout decision is a pure function of
//! the IR value, the indent, and the *reserve* (how many closing
//! delimiters an enclosing form will append to the final line), under a
//! 76-column limit. Nothing remembers source spelling — atoms render from
//! their value (cord form iff at least two bytes, all printable ASCII, no
//! quote; else dotted decimal).

use crate::ast::{ArgRef, Nasm, Op, Program, Schema};
use crate::noun::Atom;

const WIDTH: usize = 76;

/// Render an IR value to canonical `.nasm` source (newline-terminated).
///
/// The round-trip law, checked by the differential suites:
/// `expand(&render(schema, expr)) == lower(schema, expr)` for every
/// well-formed IR value, and rendering is idempotent through `parse`.
pub fn render(schema: Option<&Schema>, expr: &Nasm) -> String {
    let mut lines: Vec<String> = Vec::new();
    if let Some(s) = schema {
        lines.push(format!(":subject {}", schema_text(s)));
    }
    lines.extend(rend(ArgRef::Expr(expr), 0, 0));
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

/// Schemas always render wide; right spines flatten
/// (`{.a .b .c}`, never `{.a {.b .c}}`).
fn schema_text(s: &Schema) -> String {
    match s {
        Schema::Leaf(name) => format!(".{name}"),
        Schema::Pair(..) => {
            let mut elems: Vec<&Schema> = Vec::new();
            let mut cur = s;
            while let Schema::Pair(head, tail) = cur {
                elems.push(head);
                cur = tail;
            }
            elems.push(cur);
            let parts: Vec<String> = elems.into_iter().map(schema_text).collect();
            format!("{{{}}}", parts.join(" "))
        }
    }
}

fn op_head(op: &Op) -> String {
    format!("(%{}", op.name())
}

/// Single-line form, or `None` (`#let` / `#match` have no wide form).
///
/// A slim dispatcher — fat locals live in the per-construct helpers — so
/// deep IR renders comfortably within a 2 MiB thread stack even in debug
/// builds.
fn wide(e: ArgRef<'_>) -> Option<String> {
    match e {
        ArgRef::Axis(a) => Some(atom_text(a)),
        ArgRef::Expr(e) => match e {
            Nasm::Atom(a) => Some(atom_text(a)),
            Nasm::Axis(name) => Some(format!(".{name}")),
            Nasm::Cell { .. } => wide_cell(e),
            Nasm::Op(op) => wide_op(op),
            Nasm::Let { .. } | Nasm::Match { .. } => None,
        },
    }
}

#[inline(never)]
fn wide_cell(e: &Nasm) -> Option<String> {
    let parts: Option<Vec<String>> = cell_elems(e).map(|el| wide(ArgRef::Expr(el))).collect();
    Some(format!("[{}]", parts?.join(" ")))
}

#[inline(never)]
fn wide_op(op: &Op) -> Option<String> {
    let args = op.args();
    if args.is_empty() {
        return Some(format!("(%{})", op.name()));
    }
    let parts: Option<Vec<String>> = args.into_iter().map(wide).collect();
    Some(format!("{} {})", op_head(op), parts?.join(" ")))
}

/// The elements of a raw cell, in order.
fn cell_elems(e: &Nasm) -> impl Iterator<Item = &Nasm> {
    let Nasm::Cell {
        first,
        second,
        rest,
    } = e
    else {
        unreachable!("caller matched Cell");
    };
    std::iter::once(first.as_ref())
        .chain(std::iter::once(second.as_ref()))
        .chain(rest.iter())
}

fn pad(ind: usize) -> String {
    " ".repeat(ind)
}

/// Append a suffix to the final line.
fn amend_last(mut lines: Vec<String>, suffix: &str) -> Vec<String> {
    match lines.last_mut() {
        Some(last) => last.push_str(suffix),
        None => lines.push(suffix.to_string()),
    }
    lines
}

/// Render `e` at indent `ind` as a list of lines (indent included).
///
/// `res` is the reserve: how many characters an enclosing form will
/// append to this expression's final line (closing delimiters), so that
/// width decisions account for them and no emitted line exceeds 76.
fn rend(e: ArgRef<'_>, ind: usize, res: usize) -> Vec<String> {
    if let Some(lines) = rend_fitting_wide(e, ind, res) {
        return lines;
    }
    let ArgRef::Expr(expr) = e else {
        unreachable!("axis atoms always emit from rend_fitting_wide");
    };
    match expr {
        Nasm::Atom(_) | Nasm::Axis(_) => {
            unreachable!("atoms and axes always emit from rend_fitting_wide")
        }
        Nasm::Cell { .. } => rend_cell(expr, ind, res),
        Nasm::Op(op) => rend_op(op, ind, res),
        Nasm::Let { .. } => rend_let(expr, ind, res),
        Nasm::Match { .. } => rend_match(expr, ind, res),
    }
}

/// The wide-or-not decision: `Some` when `e` emits as a single line —
/// because its wide form fits within the reserve, or because it is an
/// atom or axis (which always emit their wide form, fitting or not).
#[inline(never)]
fn rend_fitting_wide(e: ArgRef<'_>, ind: usize, res: usize) -> Option<Vec<String>> {
    let w = wide(e);
    if let Some(w) = &w {
        if ind + w.len() + res <= WIDTH {
            return Some(vec![format!("{}{w}", pad(ind))]);
        }
    }
    match e {
        ArgRef::Axis(_) | ArgRef::Expr(Nasm::Atom(_) | Nasm::Axis(_)) => {
            Some(vec![format!("{}{}", pad(ind), w.expect("atoms are wide"))])
        }
        _ => None,
    }
}

/// `[ ` merged with the first element's first line (two columns, so
/// continuations align); remaining elements at indent + 2; `]` appended
/// to the final line.
#[inline(never)]
fn rend_cell(expr: &Nasm, ind: usize, res: usize) -> Vec<String> {
    let elems: Vec<&Nasm> = cell_elems(expr).collect();
    let first = rend(ArgRef::Expr(elems[0]), ind + 2, 0);
    let mut out = vec![format!("{}[ {}", pad(ind), &first[0][ind + 2..])];
    out.extend(first.into_iter().skip(1));
    for el in &elems[1..elems.len() - 1] {
        out.extend(rend(ArgRef::Expr(el), ind + 2, 0));
    }
    out.extend(rend(ArgRef::Expr(elems[elems.len() - 1]), ind + 2, res + 1));
    amend_last(out, "]")
}

#[inline(never)]
fn rend_op(op: &Op, ind: usize, res: usize) -> Vec<String> {
    let args = op.args();
    if args.is_empty() {
        return vec![format!("{}(%{})", pad(ind), op.name())];
    }
    let mut out = vec![format!("{}{}", pad(ind), op_head(op))];
    let last = args.len() - 1;
    for a in &args[..last] {
        out.extend(rend(*a, ind + 2, 0));
    }
    out.extend(rend(args[last], ind + 2, res + 1));
    amend_last(out, ")")
}

#[inline(never)]
fn rend_let(expr: &Nasm, ind: usize, res: usize) -> Vec<String> {
    let Nasm::Let { name, value, body } = expr else {
        unreachable!("caller matched Let");
    };
    let padding = pad(ind);
    let head = format!("{padding}#let .{name} =");
    let mut out: Vec<String> = Vec::new();
    let one = wide(ArgRef::Expr(value))
        .map(|vw| format!("{head} {vw} in"))
        .filter(|line| line.len() <= WIDTH);
    match one {
        Some(line) => out.push(line),
        None => {
            out.push(head);
            out.extend(rend(ArgRef::Expr(value), ind + 2, 0));
            out.push(format!("{padding}in"));
        }
    }
    out.extend(rend(ArgRef::Expr(body), ind, res));
    out
}

/// Note: the reserve is deliberately unused — the reference renderers do
/// not account for it in the `#match` head or closing `}` lines.
#[inline(never)]
fn rend_match(expr: &Nasm, ind: usize, _res: usize) -> Vec<String> {
    let Nasm::Match {
        scrutinee,
        arms,
        default,
    } = expr
    else {
        unreachable!("caller matched Match");
    };
    let padding = pad(ind);
    let one = wide(ArgRef::Expr(scrutinee))
        .map(|sw| format!("{padding}#match {sw} {{"))
        .filter(|line| line.len() <= WIDTH);
    let mut out: Vec<String> = Vec::new();
    match one {
        Some(line) => out.push(line),
        None => {
            out.push(format!("{padding}#match"));
            out.extend(rend(ArgRef::Expr(scrutinee), ind + 2, 0));
            out.push(format!("{padding}{{"));
        }
    }
    for arm in arms {
        out.extend(rend_case(Some(&arm.pattern), &arm.body, ind + 2));
    }
    out.extend(rend_case(None, default, ind + 2));
    out.push(format!("{padding}}}"));
    out
}

/// One `#match` arm at indent `ind`; a `None` pattern is the `_` default.
fn rend_case(pattern: Option<&Nasm>, body: &Nasm, ind: usize) -> Vec<String> {
    let padding = pad(ind);
    let pw = match pattern {
        None => Some("_".to_string()),
        Some(p) => wide(ArgRef::Expr(p)),
    };
    let bw = wide(ArgRef::Expr(body));
    if let (Some(pw), Some(bw)) = (&pw, &bw) {
        let line = format!("{padding}{pw} => {bw}");
        if line.len() <= WIDTH {
            return vec![line];
        }
    }
    if let Some(pw) = &pw {
        let head = format!("{padding}{pw} =>");
        if head.len() <= WIDTH {
            let mut out = vec![head];
            out.extend(rend(ArgRef::Expr(body), ind + 2, 0));
            return out;
        }
    }
    let pl = match pattern {
        None => vec![format!("{padding}_")],
        Some(p) => rend(ArgRef::Expr(p), ind, 3),
    };
    let mut out = amend_last(pl, " =>");
    out.extend(rend(ArgRef::Expr(body), ind + 2, 0));
    out
}
