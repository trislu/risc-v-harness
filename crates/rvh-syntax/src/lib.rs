//! Lexer, parser and lossless CST for RISC-V assembly.
//!
//! The parser is line-oriented on purpose: assembly is one statement per line,
//! and a line-oriented CST gives exact error recovery (a bad line is one node,
//! and parsing continues at the next newline) without a grammar engine.
//!
//! The syntax layer is ISA-agnostic. It never asks whether `c.addi` exists on a
//! target; it only knows that a line has a mnemonic and some operands.

#![forbid(unsafe_code)]

mod cst;
mod diag;
mod parser;

pub use cst::{Element, Node, SyntaxKind, SyntaxTree, TokenAt, TreeToken};
pub use diag::Diagnostic;
pub use parser::parse;

/// What a token is. Whitespace, comments and newlines are tokens: the CST must
/// be lossless, so nothing in the source may be dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Ident,
    Number,
    /// A quoted string, including the quotes.
    String,
    /// `.text`, `.word`, `.option`, ... The text keeps the leading dot.
    Directive,
    Comma,
    Colon,
    LParen,
    RParen,
    /// `%` before `hi` or `lo`.
    Percent,
    Plus,
    Minus,
    /// From `#` to the end of the line, excluding the newline.
    Comment,
    /// A run of spaces, tabs and carriage returns.
    Whitespace,
    Newline,
    /// A character the lexer cannot classify. Lexing continues after it.
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token<'a> {
    pub kind: TokenKind,
    pub text: &'a str,
    /// Byte offsets, `start..end`.
    pub span: (usize, usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    pub code: &'static str,
    pub message: &'static str,
    pub span: (usize, usize),
    /// When set, the parser offers a fix that replaces `span` with this text.
    pub replacement: Option<&'static str>,
}

/// Tokenize `src`.
///
/// Every byte of the input lands in exactly one token. Errors do not stop the
/// scan: one bad character produces one `Error` token and one [`LexError`].
#[must_use]
pub fn lex(src: &str) -> (Vec<Token<'_>>, Vec<LexError>) {
    let bytes = src.as_bytes();
    let mut tokens = Vec::new();
    let mut errors = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        let start = i;
        let kind = match bytes[i] {
            b' ' | b'\t' | b'\r' => {
                while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\r') {
                    i += 1;
                }
                TokenKind::Whitespace
            }
            b'\n' => {
                i += 1;
                TokenKind::Newline
            }
            b'#' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                TokenKind::Comment
            }
            // `;` and `//` are comment characters in other assemblers, and
            // tutorials use them constantly. Parse them as comments so recovery
            // stays clean, then say why the file is not portable.
            b';' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                errors.push(LexError {
                    code: "syntax.semicolon-comment",
                    message: "`;` is a statement separator in GNU as, not a comment; use `#`",
                    span: (start, start + 1),
                    replacement: Some("#"),
                });
                TokenKind::Comment
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                errors.push(LexError {
                    code: "syntax.slash-comment",
                    message: "`//` is not a comment in GNU as; use `#`",
                    span: (start, start + 2),
                    replacement: Some("#"),
                });
                TokenKind::Comment
            }
            b'"' => {
                i += 1;
                let mut terminated = false;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => i += 2,
                        b'"' => {
                            i += 1;
                            terminated = true;
                            break;
                        }
                        b'\n' => break,
                        _ => i += 1,
                    }
                }
                if terminated {
                    TokenKind::String
                } else {
                    errors.push(LexError {
                        code: "syntax.unterminated-string",
                        message: "unterminated string",
                        span: (start, i),
                        replacement: None,
                    });
                    TokenKind::Error
                }
            }
            b',' => {
                i += 1;
                TokenKind::Comma
            }
            b':' => {
                i += 1;
                TokenKind::Colon
            }
            b'(' => {
                i += 1;
                TokenKind::LParen
            }
            b')' => {
                i += 1;
                TokenKind::RParen
            }
            b'%' => {
                i += 1;
                TokenKind::Percent
            }
            b'+' => {
                i += 1;
                TokenKind::Plus
            }
            b'.' if bytes.get(i + 1).is_some_and(|b| is_ident_start(*b)) => {
                i += 1;
                while i < bytes.len() && is_ident_continue(bytes[i]) {
                    i += 1;
                }
                TokenKind::Directive
            }
            b'0'..=b'9' => {
                let end = scan_number(bytes, i);
                // `1b` and `1f` are numeric label references, not numbers.
                let suffix = bytes.get(end).is_some_and(|b| matches!(b, b'b' | b'f'));
                if suffix && is_label_reference(&src[i..end]) {
                    i = end + 1;
                    TokenKind::Ident
                } else {
                    i = end;
                    TokenKind::Number
                }
            }
            b'-' if bytes.get(i + 1).is_some_and(u8::is_ascii_digit) => {
                i = scan_number(bytes, i + 1);
                TokenKind::Number
            }
            b'-' => {
                // A bare `-` removes an extension in `.option arch, -m`.
                i += 1;
                TokenKind::Minus
            }
            b if is_ident_start(b) => {
                while i < bytes.len() && is_ident_continue(bytes[i]) {
                    i += 1;
                }
                TokenKind::Ident
            }
            _ => {
                // Not ASCII. Consume one whole scalar value: advancing by a
                // single byte would leave the token span inside a character and
                // the slice at the end of the loop would panic.
                let width = src[start..].chars().next().map_or(1, char::len_utf8);
                i = start + width;
                errors.push(LexError {
                    code: "syntax.unexpected-character",
                    message: "unexpected character; assembly is ASCII outside comments and strings",
                    span: (start, i),
                    replacement: None,
                });
                TokenKind::Error
            }
        };
        tokens.push(Token {
            kind,
            text: &src[start..i],
            span: (start, i),
        });
    }

    (tokens, errors)
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'$'
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b == b'.'
}

fn is_label_reference(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

/// Consume `0x…`, `0b…`, `0o…` or a decimal run. The caller already checked the
/// first digit.
fn scan_number(bytes: &[u8], mut i: usize) -> usize {
    if bytes[i] == b'0' {
        if let Some(base) = bytes.get(i + 1) {
            let valid: Option<fn(u8) -> bool> = match base {
                b'x' | b'X' => Some(|b: u8| b.is_ascii_hexdigit()),
                b'b' | b'B' => Some(|b: u8| matches!(b, b'0' | b'1')),
                b'o' | b'O' => Some(|b: u8| (b'0'..=b'7').contains(&b)),
                _ => None,
            };
            if let Some(is_digit) = valid {
                i += 2;
                while i < bytes.len() && (is_digit(bytes[i]) || bytes[i] == b'_') {
                    i += 1;
                }
                return i;
            }
        }
    }
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
        i += 1;
    }
    i
}

/// Parse the text of a [`TokenKind::Number`] token.
#[must_use]
pub fn parse_number(text: &str) -> Option<i64> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let digits = digits.replace('_', "");
    let (radix, body) = if let Some(rest) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        (16, rest)
    } else if let Some(rest) = digits
        .strip_prefix("0b")
        .or_else(|| digits.strip_prefix("0B"))
    {
        (2, rest)
    } else if let Some(rest) = digits
        .strip_prefix("0o")
        .or_else(|| digits.strip_prefix("0O"))
    {
        (8, rest)
    } else {
        (10, digits.as_str())
    };
    if body.is_empty() {
        return None;
    }
    let value = i64::from_str_radix(body, radix).ok()?;
    Some(if negative { -value } else { value })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokenKind> {
        let (tokens, errors) = lex(src);
        assert!(errors.is_empty(), "{errors:?}");
        tokens
            .iter()
            .map(|token| token.kind)
            .filter(|kind| *kind != TokenKind::Whitespace)
            .collect()
    }

    #[test]
    fn lexes_a_simple_instruction() {
        assert_eq!(
            kinds("addi a0, a0, 1"),
            vec![
                TokenKind::Ident,
                TokenKind::Ident,
                TokenKind::Comma,
                TokenKind::Ident,
                TokenKind::Comma,
                TokenKind::Number,
            ]
        );
    }

    #[test]
    fn keeps_comments_newlines_and_whitespace() {
        assert_eq!(
            kinds("# note\n.text"),
            vec![TokenKind::Comment, TokenKind::Newline, TokenKind::Directive]
        );

        let (tokens, _) = lex("  addi");
        assert_eq!(tokens[0].kind, TokenKind::Whitespace);
        assert_eq!(tokens[0].text, "  ");
    }

    #[test]
    fn a_label_is_an_ident_and_a_colon() {
        assert_eq!(kinds("loop:"), vec![TokenKind::Ident, TokenKind::Colon]);
    }

    #[test]
    fn memory_operands_keep_their_parens() {
        assert_eq!(
            kinds("lw a0, 8(sp)"),
            vec![
                TokenKind::Ident,
                TokenKind::Ident,
                TokenKind::Comma,
                TokenKind::Number,
                TokenKind::LParen,
                TokenKind::Ident,
                TokenKind::RParen,
            ]
        );
    }

    #[test]
    fn numeric_label_references_are_idents() {
        assert_eq!(
            kinds("bnez a0, 1b"),
            vec![
                TokenKind::Ident,
                TokenKind::Ident,
                TokenKind::Comma,
                TokenKind::Ident
            ]
        );
        assert_eq!(kinds("1:"), vec![TokenKind::Number, TokenKind::Colon]);
        assert_eq!(kinds("0b101"), vec![TokenKind::Number]);
    }

    #[test]
    fn strings_and_percent_lex() {
        assert_eq!(kinds("\"hi\""), vec![TokenKind::String]);
        assert_eq!(
            kinds("%hi(sym)"),
            vec![
                TokenKind::Percent,
                TokenKind::Ident,
                TokenKind::LParen,
                TokenKind::Ident,
                TokenKind::RParen,
            ]
        );
    }

    #[test]
    fn an_unknown_character_reports_an_error_and_keeps_going() {
        let (tokens, errors) = lex("addi a0, @, 1");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].span, (9, 10));
        assert!(tokens.iter().any(|token| token.kind == TokenKind::Error));
        assert_eq!(tokens.last().expect("tokens").kind, TokenKind::Number);
    }

    #[test]
    fn unterminated_string_is_an_error() {
        let (_, errors) = lex(".string \"oops");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "unterminated string");
    }

    #[test]
    fn numbers_in_every_base() {
        assert_eq!(parse_number("1"), Some(1));
        assert_eq!(parse_number("-2048"), Some(-2048));
        assert_eq!(parse_number("0x8000_0000"), Some(0x8000_0000));
        assert_eq!(parse_number("0b1010"), Some(10));
        assert_eq!(parse_number("0o17"), Some(15));
        assert_eq!(parse_number("0x"), None);
    }

    #[test]
    fn spans_cover_every_byte() {
        let src = "addi a0,a0,1 # x\n";
        let (tokens, _) = lex(src);
        let mut offset = 0;
        for token in &tokens {
            assert_eq!(token.span.0, offset);
            assert_eq!(&src[token.span.0..token.span.1], token.text);
            offset = token.span.1;
        }
        assert_eq!(offset, src.len());
    }
}

#[cfg(test)]
mod robustness_tests {
    use super::*;

    /// Tokens must tile the source: no gaps, no overlaps, no split characters.
    fn assert_tiles(source: &str) {
        let (tokens, _errors) = lex(source);
        let mut offset = 0;
        for token in &tokens {
            assert_eq!(
                token.span.0, offset,
                "gap before {:?} in {source:?}",
                token.text
            );
            assert_eq!(token.span.1, token.span.0 + token.text.len());
            assert!(source.is_char_boundary(token.span.0));
            assert!(source.is_char_boundary(token.span.1));
            offset = token.span.1;
        }
        assert_eq!(offset, source.len(), "tokens stop early in {source:?}");
    }

    #[test]
    fn non_ascii_never_panics() {
        for source in [
            "阿",
            "阿 addi\n",
            "addi a0, 1  # 中文注释\n",
            "li a0, 1\n阿阿阿\n",
            "🙂 = 1\n",
            "\u{feff}addi a0, a1, 1\n",
            "a\tb\r\n",
            "极长的行 ".repeat(100).as_str(),
            "#\n#\n",
            "\"未闭合的字符串\n",
        ] {
            assert_tiles(source);
        }
    }

    #[test]
    fn a_non_ascii_character_is_reported_once() {
        let (_, errors) = lex("阿\n");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].span, (0, 3));
    }
}

#[cfg(test)]
mod exhaustive_tests {
    use super::*;

    /// Every short string over a mixed alphabet must lex without panicking and
    /// tile the source. The alphabet mixes ASCII syntax, CJK and an emoji so that
    /// multi-byte boundaries are always in play.
    #[test]
    fn short_strings_never_panic() {
        let alphabet = ['a', ' ', '\n', '#', '"', '0', '.', '阿', 'é', '😀'];
        let mut buffer = String::new();
        for a in alphabet {
            for b in alphabet {
                for c in alphabet {
                    buffer.clear();
                    buffer.push(a);
                    buffer.push(b);
                    buffer.push(c);
                    let (tokens, _errors) = lex(&buffer);
                    let mut offset = 0;
                    for token in &tokens {
                        assert_eq!(token.span.0, offset, "gap in {buffer:?}");
                        assert!(
                            buffer.is_char_boundary(token.span.1),
                            "split char in {buffer:?}"
                        );
                        offset = token.span.1;
                    }
                    assert_eq!(offset, buffer.len(), "short in {buffer:?}");
                }
            }
        }
    }
}
