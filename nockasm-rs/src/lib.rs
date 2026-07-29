//! Pure Rust implementation of **Nockasm**: a thin macro expander from
//! legible Nock assembly to canonical [Nock 4K](https://nock.is)
//! formulas.
//!
//! This crate is a fourth independent executor of the nockasm laws,
//! alongside the Python reference (`nockasm.py`), the Hoon library
//! (`desk/lib/nockasm.hoon`), and the Hoon-on-nockvm NockApp (`nasmc`).
//! The differential suite (`tests/test_rust.py` in the repository) holds
//! it byte-identical to all of them over the shared corpus. Beneath every
//! implementation, the Nock 4K specification is the truth.
//!
//! # The pipeline
//!
//! ```text
//! .nasm source ─ parse ─▶ Program (schema + Nasm IR)
//!                              │
//!                              ├─ lower  ─▶ Noun     (canonical Nock)
//!                              └─ render ─▶ String   (canonical .nasm)
//!
//! jamfile bytes ─ cue ─▶ Noun ─ lift ─▶ Nasm ─ render ─▶ String
//! Noun ─ jam ─▶ jamfile bytes
//! ```
//!
//! [`expand`] is `lower ∘ parse`; [`nasm_from_jam`] is
//! `render ∘ lift ∘ cue`.
//!
//! # The laws
//!
//! Everything above is governed by three laws, enforced by this crate's
//! test suite and cross-implementation by the repository's differential
//! suites (IR contract version [`NASM_VERSION`]):
//!
//! - **Round trip**: `expand(&render(s, a)) == lower(s, a)` for every
//!   well-formed IR value, and rendering is idempotent through `parse`.
//! - **Lift soundness**: `lower(None, &lift(f)) == f` for every noun
//!   `f` — the lift is deterministic and zero-heuristic, so
//!   misclassifying data as code is impossible by construction.
//! - **Serialization**: `cue(&jam(n)) == Ok(n)` for every noun `n`.
//!
//! # Types
//!
//! The IR ([`Nasm`], [`Op`], [`Schema`]) refines the reference contract:
//! ill-formed nodes — unknown opcodes, wrong arities, unary raw cells, a
//! `#match` without a default — are unrepresentable, so what the
//! reference implementations reject at lower time is rejected at parse
//! (or construction) time here, and [`lower`] can only fail on name
//! resolution ([`LowerError`]). Compilers targeting nockasm (the
//! intended embedder) construct [`Program`] values directly and get the
//! same guarantees without going through text.
//!
//! # Example
//!
//! ```
//! use nockasm::{expand, jam, noun, parse, render};
//!
//! let src = "
//! :subject {.before .target .after}
//! #let .next = (%inc .target) in
//!   [.before .next .after]
//! ";
//! let formula = expand(src).unwrap();
//! assert_eq!(formula, noun![8 [4 0 6] [0 6] [0 2] 0 15]);
//! assert_eq!(formula.to_string(), "[8 [4 0 6] [0 6] [0 2] 0 15]");
//!
//! // The canonical renderer is deterministic and byte-identical across
//! // implementations:
//! let program = parse(src).unwrap();
//! assert_eq!(
//!     program.render(),
//!     ":subject {.before .target .after}\n#let .next = (%inc .target) in\n[.before .next .after]\n",
//! );
//!
//! // Jamfile bytes, ready for any cue:
//! let bytes = jam(&formula);
//! assert_eq!(nockasm::cue(&bytes).unwrap(), formula);
//! ```
//!
//! # Limits
//!
//! Parsing is depth-bounded ([`parse::MAX_DEPTH`]); everything else —
//! the noun layer (equality, hashing, drop, printing, `jam`, `cue`),
//! [`lift`], [`lower`], [`render`], and the IR's own teardown — runs on
//! explicit stacks, so no input, however deep, can overflow the call
//! stack through this crate's pipeline: hostile jamfiles lift, lower,
//! and render cleanly, and machine-emitted IR has no depth bound at
//! all. The IR's *derived* `Clone`, `PartialEq`, and `Debug` still
//! recurse; deep-cloning or deep-comparing IR is the caller's lookout.
//! One asymmetry to know about: `render` happily emits IR deeper than
//! the parser re-admits, so the textual round trip of the render law is
//! quantified over parser-admissible depth — beyond it, equality lives
//! at the noun level via `lower`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod ast;
mod error;
mod jam;
mod lex;
mod lift;
mod lower;
pub mod noun;
pub mod parse;
mod render;

pub use ast::{MatchArm, Name, Nasm, Op, Program, Schema};
pub use error::{CueError, Error, InvalidName, LowerError, ParseError, ParseErrorKind, Pos};
pub use jam::{cue, jam};
pub use lift::{lift, nasm_from_jam};
pub use lower::{expand, lower};
pub use noun::{peg, Atom, Noun, NounRef};
pub use parse::parse;
pub use render::render;

/// Version of the target-IR contract: the `$nasm` node set, the lowering
/// equations, and the canonical rendering rules. Append-only; shared with
/// the Python (`NASM_VERSION`) and Hoon (`nasm-version`) implementations.
pub const NASM_VERSION: u32 = 1;
