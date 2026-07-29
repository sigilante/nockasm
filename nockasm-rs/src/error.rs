//! Error types.
//!
//! The reference implementations crash with tagged traces (Hoon) or raise
//! `SyntaxError`/`NameError`/`TypeError` (Python); here every failure is a
//! value. [`ParseError`] covers everything rejected while reading source —
//! including unknown opcodes, wrong arities, and non-atom axis arguments,
//! which the reference implementations reject at lower time but whose
//! ill-formed IR this crate cannot even represent (see [`crate::ast`]).
//! [`LowerError`] covers the residue that only expansion can see: name
//! resolution and shadowing. The composed [`expand`](crate::expand)
//! accepts and rejects exactly the same sources either way.

use std::fmt;

use crate::ast::Name;

/// A source position, 1-based, as reported by the lexer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pos {
    /// 1-based line.
    pub line: u32,
    /// 1-based column (in characters).
    pub col: u32,
}

impl fmt::Display for Pos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "L{}:C{}", self.line, self.col)
    }
}

/// A rejected name (see [`Name::new`](crate::Name::new)).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidName(pub String);

impl fmt::Display for InvalidName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid name {:?}: want [A-Za-z_][A-Za-z0-9_-]*", self.0)
    }
}

impl std::error::Error for InvalidName {}

/// Why a source failed to parse.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseErrorKind {
    /// A character no token starts with.
    UnexpectedChar(char),
    /// A `'cord'` missing its closing quote.
    UnterminatedCord,
    /// The source ended where a token was required.
    UnexpectedEof {
        /// What the parser was looking for.
        wanted: &'static str,
    },
    /// A token that cannot appear here.
    UnexpectedToken {
        /// What the parser was looking for.
        wanted: &'static str,
        /// A rendering of what it found.
        found: String,
    },
    /// Tokens remained after the program's one expression.
    TrailingTokens {
        /// A rendering of the first leftover token.
        found: String,
    },
    /// `{}` — a schema group with no leaves.
    EmptySchema,
    /// `[x]` or `[]` — a raw cell needs at least two elements.
    RawCellTooFew,
    /// `(%name ...)` where `name` is not an opcode.
    UnknownOpcode(String),
    /// An opcode applied to the wrong number of arguments.
    OpArity {
        /// The opcode name, without `%`.
        op: &'static str,
        /// Its arity.
        want: usize,
        /// The number of arguments found.
        got: usize,
    },
    /// An axis argument (`%slot`/`%call`/`%edit` position) that is not an
    /// atom literal.
    AxisArgNotAtom {
        /// The opcode name, without `%`.
        op: &'static str,
    },
    /// `#name` where `name` is not a macro.
    UnknownMacro(String),
    /// `#match` with no `_ =>` default.
    MatchNeedsDefault,
    /// `#match` with more than one `_ =>` default.
    MatchDuplicateDefault,
    /// Expression nesting beyond [`crate::parse::MAX_DEPTH`].
    TooDeep,
}

/// A syntax error, with the position where it was noticed when one is
/// available.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// What went wrong.
    pub kind: ParseErrorKind,
    /// Where, if known.
    pub pos: Option<Pos>,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ParseErrorKind as K;
        match &self.kind {
            K::UnexpectedChar(c) => write!(f, "unexpected character {c:?}")?,
            K::UnterminatedCord => write!(f, "unterminated cord literal")?,
            K::UnexpectedEof { wanted } => write!(f, "expected {wanted}, got end of input")?,
            K::UnexpectedToken { wanted, found } => write!(f, "expected {wanted}, got {found}")?,
            K::TrailingTokens { found } => write!(f, "trailing tokens after expression: {found}")?,
            K::EmptySchema => write!(f, "empty schema {{}}")?,
            K::RawCellTooFew => write!(f, "raw cell needs at least 2 elements")?,
            K::UnknownOpcode(name) => write!(f, "unknown opcode %{name}")?,
            K::OpArity { op, want, got } => write!(f, "%{op} takes {want} args, got {got}")?,
            K::AxisArgNotAtom { op } => write!(f, "%{op}: axis argument must be an atom literal")?,
            K::UnknownMacro(name) => write!(f, "unknown macro #{name}")?,
            K::MatchNeedsDefault => write!(f, "#match requires a `_ => ...` default")?,
            K::MatchDuplicateDefault => write!(f, "duplicate _ in #match")?,
            K::TooDeep => write!(f, "expression nesting too deep")?,
        }
        if let Some(pos) = self.pos {
            write!(f, " at {pos}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ParseError {}

/// Why a well-formed IR value failed to lower.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LowerError {
    /// `.name` with no binding in scope.
    UnboundAxis {
        /// The unresolved name.
        name: Name,
        /// The names that are in scope, sorted.
        declared: Vec<Name>,
    },
    /// The same name twice in one `:subject` schema.
    DuplicateSchemaName(Name),
    /// `#let` of a name already in scope.
    LetShadows(Name),
}

impl fmt::Display for LowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LowerError::UnboundAxis { name, declared } => {
                write!(f, "unbound axis .{name}; declared: ")?;
                if declared.is_empty() {
                    write!(f, "(no :subject)")
                } else {
                    let names: Vec<String> = declared.iter().map(|n| format!(".{n}")).collect();
                    write!(f, "{}", names.join(" "))
                }
            }
            LowerError::DuplicateSchemaName(name) => {
                write!(f, "duplicate name in schema: .{name}")
            }
            LowerError::LetShadows(name) => {
                write!(f, "#let shadows existing name .{name}")
            }
        }
    }
}

impl std::error::Error for LowerError {}

/// Any failure of [`expand`](crate::expand): a parse or lower error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The source failed to parse.
    Parse(ParseError),
    /// The parsed program failed to lower.
    Lower(LowerError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Parse(e) => e.fmt(f),
            Error::Lower(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Parse(e) => Some(e),
            Error::Lower(e) => Some(e),
        }
    }
}

impl From<ParseError> for Error {
    fn from(e: ParseError) -> Error {
        Error::Parse(e)
    }
}

impl From<LowerError> for Error {
    fn from(e: LowerError) -> Error {
        Error::Lower(e)
    }
}

/// Why a byte string failed to [`cue`](crate::cue).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CueError {
    /// The stream ended mid-value. (Also the verdict on an all-zero or
    /// empty input, on which the reference decoders never terminate.)
    Truncated,
    /// A backreference to a position that holds no completed noun.
    BadBackref,
    /// An atom length so wide its length-of-length exceeds 64 bits.
    TooWide,
}

impl fmt::Display for CueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CueError::Truncated => write!(f, "cue: truncated jam stream"),
            CueError::BadBackref => write!(f, "cue: backreference to no noun"),
            CueError::TooWide => write!(f, "cue: atom length field too wide"),
        }
    }
}

impl std::error::Error for CueError {}
