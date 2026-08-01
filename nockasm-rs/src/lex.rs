//! The tokenizer: a hand-written port of the reference regex lexer.
//!
//! Token boundaries match the Python tokenizer exactly (same character
//! classes, same maximal-munch order), so both implementations accept the
//! same token streams. Comments (`;` to end of line) and whitespace
//! (space, tab, CR, LF) are skipped. Cords may contain any character but
//! the closing quote — newlines included, no escapes — and pack their
//! UTF-8 bytes little-endian.

use crate::ast::Name;
use crate::error::{ParseError, ParseErrorKind, Pos};
use crate::noun::Atom;

/// A token's payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Tok {
    Arrow,
    LParen,
    RParen,
    LCurly,
    RCurly,
    LBrack,
    RBrack,
    Equals,
    Under,
    /// A decimal or hex literal, value already packed.
    Num(Atom),
    /// A `'cord'` literal, value already packed.
    Cord(Atom),
    /// `.name`
    AxisName(Name),
    /// `%name`
    OpName(String),
    /// `#name`
    MacroName(String),
    /// `:name`
    Directive(String),
    /// A bare identifier (`[A-Za-z][A-Za-z0-9_-]*`), e.g. `in`.
    Ident(String),
}

impl Tok {
    /// A short rendering for error messages.
    pub(crate) fn describe(&self) -> String {
        match self {
            Tok::Arrow => "'=>'".into(),
            Tok::LParen => "'('".into(),
            Tok::RParen => "')'".into(),
            Tok::LCurly => "'{'".into(),
            Tok::RCurly => "'}'".into(),
            Tok::LBrack => "'['".into(),
            Tok::RBrack => "']'".into(),
            Tok::Equals => "'='".into(),
            Tok::Under => "'_'".into(),
            Tok::Num(a) => a.to_decimal_string(),
            Tok::Cord(_) => "a cord literal".into(),
            Tok::AxisName(n) => format!(".{n}"),
            Tok::OpName(n) => format!("%{n}"),
            Tok::MacroName(n) => format!("#{n}"),
            Tok::Directive(n) => format!(":{n}"),
            Tok::Ident(s) => format!("{s:?}"),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Token {
    pub tok: Tok,
    pub pos: Pos,
}

fn is_name_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    i: usize,
    line: u32,
    col: u32,
}

impl<'a> Lexer<'a> {
    fn pos(&self) -> Pos {
        Pos {
            line: self.line,
            col: self.col,
        }
    }

    fn err(&self, kind: ParseErrorKind, pos: Pos) -> ParseError {
        ParseError {
            kind,
            pos: Some(pos),
        }
    }

    fn peek(&self, k: usize) -> Option<u8> {
        self.bytes.get(self.i + k).copied()
    }

    /// Advance past `n` bytes, tracking line and (character) column.
    fn bump(&mut self, n: usize) {
        for &b in &self.bytes[self.i..self.i + n] {
            if b == b'\n' {
                self.line += 1;
                self.col = 1;
            } else if b & 0xc0 != 0x80 {
                // count characters, not continuation bytes
                self.col += 1;
            }
        }
        self.i += n;
    }

    /// Length of the name (`[A-Za-z_][A-Za-z0-9_-]*`) starting at
    /// `self.i + off`, or 0 if none starts there.
    fn name_len(&self, off: usize) -> usize {
        match self.peek(off) {
            Some(b) if is_name_start(b) => {}
            _ => return 0,
        }
        let mut n = 1;
        while let Some(b) = self.peek(off + n) {
            if is_name_char(b) {
                n += 1;
            } else {
                break;
            }
        }
        n
    }

    fn take_name(&mut self, off: usize, len: usize) -> String {
        let s = self.src[self.i + off..self.i + off + len].to_string();
        self.bump(off + len);
        s
    }

    fn run(mut self) -> Result<Vec<Token>, ParseError> {
        let mut out = Vec::new();
        while let Some(b) = self.peek(0) {
            let pos = self.pos();
            match b {
                b';' => {
                    let mut n = 1;
                    while let Some(c) = self.peek(n) {
                        if c == b'\n' {
                            break;
                        }
                        n += 1;
                    }
                    self.bump(n);
                }
                b' ' | b'\t' | b'\n' | b'\r' => self.bump(1),
                b'=' if self.peek(1) == Some(b'>') => {
                    self.bump(2);
                    out.push(Token {
                        tok: Tok::Arrow,
                        pos,
                    });
                }
                b'=' => {
                    self.bump(1);
                    out.push(Token {
                        tok: Tok::Equals,
                        pos,
                    });
                }
                b'(' | b')' | b'{' | b'}' | b'[' | b']' => {
                    let tok = match b {
                        b'(' => Tok::LParen,
                        b')' => Tok::RParen,
                        b'{' => Tok::LCurly,
                        b'}' => Tok::RCurly,
                        b'[' => Tok::LBrack,
                        _ => Tok::RBrack,
                    };
                    self.bump(1);
                    out.push(Token { tok, pos });
                }
                b'_' => {
                    self.bump(1);
                    out.push(Token {
                        tok: Tok::Under,
                        pos,
                    });
                }
                b'\'' => {
                    let mut n = 1;
                    loop {
                        match self.peek(n) {
                            None => return Err(self.err(ParseErrorKind::UnterminatedCord, pos)),
                            Some(b'\'') => break,
                            Some(_) => n += 1,
                        }
                    }
                    let value = Atom::from_le_bytes(&self.bytes[self.i + 1..self.i + n]);
                    self.bump(n + 1);
                    out.push(Token {
                        tok: Tok::Cord(value),
                        pos,
                    });
                }
                b'0' if self.peek(1) == Some(b'x')
                    && self.peek(2).is_some_and(|c| c.is_ascii_hexdigit()) =>
                {
                    let mut n = 3;
                    while let Some(c) = self.peek(n) {
                        if c.is_ascii_hexdigit() || c == b'.' || c == b'_' {
                            n += 1;
                        } else {
                            break;
                        }
                    }
                    let digits: String = self.src[self.i + 2..self.i + n]
                        .chars()
                        .filter(|c| *c != '.' && *c != '_')
                        .collect();
                    self.bump(n);
                    out.push(Token {
                        tok: Tok::Num(Atom::from_hex_digits(&digits)),
                        pos,
                    });
                }
                b'0'..=b'9' => {
                    let mut n = 1;
                    while let Some(c) = self.peek(n) {
                        if c.is_ascii_digit() || c == b'.' || c == b'_' {
                            n += 1;
                        } else {
                            break;
                        }
                    }
                    let digits: String = self.src[self.i..self.i + n]
                        .chars()
                        .filter(|c| *c != '.' && *c != '_')
                        .collect();
                    self.bump(n);
                    out.push(Token {
                        tok: Tok::Num(Atom::from_decimal_digits(&digits)),
                        pos,
                    });
                }
                b'.' => {
                    let len = self.name_len(1);
                    if len == 0 {
                        return Err(self.err(ParseErrorKind::UnexpectedChar('.'), pos));
                    }
                    let name = self.take_name(1, len);
                    out.push(Token {
                        tok: Tok::AxisName(Name::new(name).expect("lexer-validated")),
                        pos,
                    });
                }
                b'%' | b'#' | b':' => {
                    let len = self.name_len(1);
                    if len == 0 {
                        return Err(self.err(ParseErrorKind::UnexpectedChar(b as char), pos));
                    }
                    let name = self.take_name(1, len);
                    let tok = match b {
                        b'%' => Tok::OpName(name),
                        b'#' => Tok::MacroName(name),
                        _ => Tok::Directive(name),
                    };
                    out.push(Token { tok, pos });
                }
                b if b.is_ascii_alphabetic() => {
                    // Bare identifier: starts with a letter (underscore
                    // starts lex as Under + Ident, per the reference).
                    let len = self.name_len(0);
                    let name = self.take_name(0, len);
                    out.push(Token {
                        tok: Tok::Ident(name),
                        pos,
                    });
                }
                _ => {
                    let c = self.src[self.i..].chars().next().unwrap_or('\u{fffd}');
                    return Err(self.err(ParseErrorKind::UnexpectedChar(c), pos));
                }
            }
        }
        Ok(out)
    }
}

pub(crate) fn tokenize(src: &str) -> Result<Vec<Token>, ParseError> {
    Lexer {
        src,
        bytes: src.as_bytes(),
        i: 0,
        line: 1,
        col: 1,
    }
    .run()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Vec<Tok> {
        tokenize(src).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn basics() {
        assert_eq!(
            toks("(%inc .x) ; comment\n=> = _"),
            vec![
                Tok::LParen,
                Tok::OpName("inc".into()),
                Tok::AxisName(Name::new("x").unwrap()),
                Tok::RParen,
                Tok::Arrow,
                Tok::Equals,
                Tok::Under,
            ]
        );
    }

    #[test]
    fn numbers() {
        assert_eq!(toks("1.000"), vec![Tok::Num(Atom::from(1000u64))]);
        assert_eq!(toks("0x2a"), vec![Tok::Num(Atom::from(42u64))]);
        assert_eq!(toks("0x1.0000"), vec![Tok::Num(Atom::from(65536u64))]);
        // '0x' with no hex digit after: '0' then ident 'x'
        assert_eq!(
            toks("0x"),
            vec![Tok::Num(Atom::from(0u64)), Tok::Ident("x".into())]
        );
        // trailing separators fold away, as in the reference
        assert_eq!(toks("1..2"), vec![Tok::Num(Atom::from(12u64))]);
    }

    #[test]
    fn cords() {
        assert_eq!(toks("'fast'"), vec![Tok::Cord(Atom::from(0x7473_6166u64))]);
        assert_eq!(toks("''"), vec![Tok::Cord(Atom::ZERO)]);
        assert_eq!(
            toks("'two\nlines'"),
            vec![Tok::Cord(Atom::from_cord("two\nlines"))]
        );
        assert!(tokenize("'oops").is_err());
    }

    #[test]
    fn positions() {
        let ts = tokenize("  42\n .x").unwrap();
        assert_eq!(ts[0].pos, Pos { line: 1, col: 3 });
        assert_eq!(ts[1].pos, Pos { line: 2, col: 2 });
    }

    #[test]
    fn rejects() {
        assert!(tokenize("@").is_err());
        assert!(tokenize(".").is_err());
        assert!(tokenize("%").is_err());
    }
}
