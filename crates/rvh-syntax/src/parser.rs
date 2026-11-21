//! Line-oriented parser.
//!
//! Grammar, informally:
//!
//! ```text
//! SourceFile := Line*
//! Line       := WS* (Label | Instruction | Directive | Comment)? WS* NL?
//! Label      := (Ident | Number) WS* ':'
//! Instruction:= Ident ArgList
//! Directive  := '.name' ArgList
//! ArgList    := (Arg (',' Arg)*)?
//! Arg        := token+            // parentheses are balanced
//! ```
//!
//! Recovery is per line: whatever cannot be attached to a statement becomes one
//! `Error` node, and parsing resumes at the next newline. This keeps later lines
//! analyzable while the user is still typing.

use crate::cst::{Node, SyntaxKind, SyntaxTree, TreeToken};
use crate::diag::Diagnostic;
use crate::lex;

/// Parse `source` into a lossless tree plus diagnostics.
#[must_use]
pub fn parse(source: &str) -> SyntaxTree {
    let (raw, lex_errors) = lex(source);
    let diagnostics: Vec<Diagnostic> = lex_errors
        .iter()
        .map(|error| {
            let diagnostic = Diagnostic::new(error.code, error.message, error.span);
            match error.replacement {
                Some(replacement) => diagnostic.with_fix(
                    format!("replace with `{replacement}`"),
                    error.span,
                    replacement,
                ),
                None => diagnostic,
            }
        })
        .collect();
    let tokens: Vec<TreeToken> = raw
        .iter()
        .map(|token| TreeToken {
            kind: token.kind.into(),
            span: token.span,
        })
        .collect();

    let mut parser = Parser {
        tokens,
        pos: 0,
        diagnostics,
    };
    let root = parser.parse_file();
    SyntaxTree::new(source.to_owned(), root, parser.diagnostics)
}

struct Parser {
    tokens: Vec<TreeToken>,
    pos: usize,
    diagnostics: Vec<Diagnostic>,
}

impl Parser {
    fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn peek(&self) -> Option<SyntaxKind> {
        self.tokens.get(self.pos).map(|token| token.kind)
    }

    fn span(&self) -> (usize, usize) {
        self.tokens.get(self.pos).map_or((0, 0), |token| token.span)
    }

    fn bump(&mut self, node: &mut Node) {
        if let Some(token) = self.tokens.get(self.pos).copied() {
            node.push_token(token.kind, token.span);
            self.pos += 1;
        }
    }

    /// Kind of the next non-whitespace token at or after `from`.
    fn peek_significant(&self, from: usize) -> Option<SyntaxKind> {
        self.tokens[from..]
            .iter()
            .find(|token| token.kind != SyntaxKind::Whitespace)
            .map(|token| token.kind)
    }

    fn starts_label(&self) -> bool {
        matches!(
            self.peek(),
            Some(SyntaxKind::Ident) | Some(SyntaxKind::Number)
        ) && self.peek_significant(self.pos + 1) == Some(SyntaxKind::Colon)
    }

    fn parse_file(&mut self) -> Node {
        let mut root = Node::new(SyntaxKind::SourceFile);
        while !self.at_end() {
            let line = self.parse_line();
            root.push_child(line);
        }
        root
    }

    fn parse_line(&mut self) -> Node {
        let mut line = Node::new(SyntaxKind::Line);
        loop {
            match self.peek() {
                None => break,
                Some(SyntaxKind::Newline) => {
                    self.bump(&mut line);
                    break;
                }
                Some(SyntaxKind::Whitespace) => self.bump(&mut line),
                Some(SyntaxKind::Comment) => self.bump(&mut line),
                Some(SyntaxKind::Ident) | Some(SyntaxKind::Number) if self.starts_label() => {
                    let label = self.parse_label();
                    line.push_child(label);
                }
                Some(SyntaxKind::DirectiveName) => {
                    let mut directive = Node::new(SyntaxKind::Directive);
                    self.bump(&mut directive);
                    self.parse_args(&mut directive);
                    line.push_child(directive);
                }
                Some(SyntaxKind::Ident) => {
                    let mut instruction = Node::new(SyntaxKind::Instruction);
                    self.bump(&mut instruction);
                    self.parse_args(&mut instruction);
                    line.push_child(instruction);
                }
                Some(_) => {
                    self.diagnostics.push(Diagnostic::new(
                        "syntax.unexpected-token",
                        "unexpected token; expected a label, an instruction or a directive",
                        self.span(),
                    ));
                    let mut error = Node::new(SyntaxKind::ErrorToken);
                    while let Some(kind) = self.peek() {
                        if kind == SyntaxKind::Newline {
                            break;
                        }
                        self.bump(&mut error);
                    }
                    line.push_child(error);
                }
            }
        }
        line
    }

    fn parse_label(&mut self) -> Node {
        let mut label = Node::new(SyntaxKind::Label);
        self.bump(&mut label);
        while self.peek() == Some(SyntaxKind::Whitespace) {
            self.bump(&mut label);
        }
        // `starts_label` guarantees the colon.
        self.bump(&mut label);
        label
    }

    /// Parse the argument list of an instruction or directive.
    ///
    /// The first argument needs no leading comma. Later arguments do, and a
    /// missing one is reported but recovered from.
    fn parse_args(&mut self, parent: &mut Node) {
        let mut have_arg = false;
        let mut expect_arg = false;
        let mut last_comma = (0, 0);

        loop {
            match self.peek() {
                None => break,
                Some(SyntaxKind::Whitespace) => self.bump(parent),
                Some(SyntaxKind::Comment) | Some(SyntaxKind::Newline) => break,
                Some(SyntaxKind::Comma) => {
                    if expect_arg {
                        self.diagnostics.push(Diagnostic::new(
                            "syntax.expected-argument",
                            "expected an argument after `,`",
                            self.span(),
                        ));
                    } else if !have_arg {
                        self.diagnostics.push(Diagnostic::new(
                            "syntax.expected-argument",
                            "expected an argument before `,`",
                            self.span(),
                        ));
                    }
                    last_comma = self.span();
                    self.bump(parent);
                    expect_arg = true;
                }
                Some(_) => {
                    if have_arg && !expect_arg {
                        self.diagnostics.push(Diagnostic::new(
                            "syntax.expected-comma",
                            "expected `,` between arguments",
                            self.span(),
                        ));
                    }
                    let mut operand = Node::new(SyntaxKind::Operand);
                    self.parse_arg_into(&mut operand);
                    parent.push_child(operand);
                    have_arg = true;
                    expect_arg = false;
                }
            }
        }

        if expect_arg {
            self.diagnostics.push(Diagnostic::new(
                "syntax.expected-argument",
                "expected an argument after `,`",
                last_comma,
            ));
        }
    }

    fn parse_arg_into(&mut self, node: &mut Node) {
        // An argument is a run of tokens with no whitespace between them, so
        // `8(sp)`, `-4(sp)` and `%hi(sym)` stay together. Whitespace ends the
        // argument, which is what lets `addi a0 a1 1` be reported as a missing
        // comma instead of one very long operand.
        while let Some(kind) = self.peek() {
            match kind {
                SyntaxKind::Whitespace
                | SyntaxKind::Comma
                | SyntaxKind::Comment
                | SyntaxKind::Newline => break,
                _ => self.bump(node),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cst::Element;

    fn collect_spans(node: &Node, spans: &mut Vec<(usize, usize)>) {
        for child in &node.children {
            match child {
                Element::Node(node) => collect_spans(node, spans),
                Element::Token(token) => spans.push(token.span),
            }
        }
    }

    fn count_tokens(node: &Node, kind: SyntaxKind) -> usize {
        let mut count = 0;
        for child in &node.children {
            match child {
                Element::Node(node) => count += count_tokens(node, kind),
                Element::Token(token) if token.kind == kind => count += 1,
                Element::Token(_) => {}
            }
        }
        count
    }

    fn count_nodes(node: &Node, kind: SyntaxKind) -> usize {
        let mut count = usize::from(node.kind == kind);
        for child in &node.children {
            if let Element::Node(child) = child {
                count += count_nodes(child, kind);
            }
        }
        count
    }

    /// The whole point of a lossless tree: every byte appears exactly once.
    fn assert_lossless(tree: &SyntaxTree) {
        let mut spans = Vec::new();
        collect_spans(tree.root(), &mut spans);
        let mut offset = 0;
        for (start, end) in spans {
            assert_eq!(start, offset, "gap or overlap in the tree");
            assert!(start < end, "empty token");
            offset = end;
        }
        assert_eq!(offset, tree.source().len());
    }

    #[test]
    fn parses_label_and_instruction() {
        let tree = parse("loop:\n  addi a0, a0, 1\n");
        assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
        assert_lossless(&tree);

        let lines: Vec<_> = tree
            .root()
            .children
            .iter()
            .filter_map(|child| match child {
                Element::Node(node) => Some(node),
                Element::Token(_) => None,
            })
            .collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].child(SyntaxKind::Label).is_some());
        assert!(lines[1].child(SyntaxKind::Instruction).is_some());
    }

    #[test]
    fn keeps_comments() {
        let tree = parse("  # hello\naddi a0, a0, 1 # trailing\n");
        assert!(tree.diagnostics().is_empty());
        assert_lossless(&tree);
        assert_eq!(count_tokens(tree.root(), SyntaxKind::Comment), 2);
    }

    #[test]
    fn missing_comma_is_reported_once_per_gap() {
        let tree = parse("addi a0 a1 1\n");
        let codes: Vec<_> = tree.diagnostics().iter().map(|d| d.code).collect();
        assert_eq!(
            codes,
            vec!["syntax.expected-comma", "syntax.expected-comma"]
        );
        assert_lossless(&tree);
    }

    #[test]
    fn trailing_comma_is_reported() {
        let tree = parse("addi a0, a0,\n");
        assert_eq!(tree.diagnostics().len(), 1);
        assert_eq!(tree.diagnostics()[0].code, "syntax.expected-argument");
        assert_eq!(tree.diagnostics()[0].span, (11, 12));
        assert_lossless(&tree);
    }

    #[test]
    fn a_bad_line_does_not_hide_the_next_one() {
        let tree = parse("@\naddi a0, a0, 1\n");
        assert_eq!(tree.diagnostics().len(), 2);
        assert_eq!(tree.diagnostics()[0].code, "syntax.unexpected-character");
        assert_eq!(tree.diagnostics()[1].code, "syntax.unexpected-token");
        let last_line = match tree.root().children.last() {
            Some(Element::Node(node)) => node,
            _ => panic!("expected a line"),
        };
        assert!(last_line.child(SyntaxKind::Instruction).is_some());
        assert_lossless(&tree);
    }

    #[test]
    fn directives_take_arguments() {
        let tree = parse(".section .data\n.word 1, 2\n.string \"hi\"\n");
        assert!(tree.diagnostics().is_empty(), "{:?}", tree.diagnostics());
        assert_lossless(&tree);
        assert_eq!(count_nodes(tree.root(), SyntaxKind::Directive), 3);
    }

    #[test]
    fn an_instruction_may_follow_a_label_on_the_same_line() {
        let tree = parse("start: addi a0, a0, 1\n");
        assert!(tree.diagnostics().is_empty());
        assert_lossless(&tree);
    }

    #[test]
    fn empty_source_is_fine() {
        let tree = parse("");
        assert!(tree.diagnostics().is_empty());
        assert_eq!(tree.root().kind, SyntaxKind::SourceFile);
        assert_lossless(&tree);
    }

    #[test]
    fn unbalanced_parens_do_not_lose_tokens() {
        let tree = parse("lw a0, 4(sp\n");
        assert_lossless(&tree);
    }
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;

    #[test]
    fn semicolon_comment_carries_a_fix() {
        let tree = parse("addi a0, a0, 1 ; note\n");
        assert_eq!(tree.diagnostics().len(), 1);
        let diagnostic = &tree.diagnostics()[0];
        assert_eq!(diagnostic.code, "syntax.semicolon-comment");
        assert_eq!(diagnostic.fixes()[0].replacement, "#");
        // The line still parses: one diagnostic, no error node.
        assert!(!tree.has_errors());
    }

    #[test]
    fn slash_comment_carries_a_fix() {
        let tree = parse("nop // note\n");
        assert_eq!(tree.diagnostics().len(), 1);
        assert_eq!(tree.diagnostics()[0].code, "syntax.slash-comment");
        assert_eq!(tree.diagnostics()[0].fixes()[0].replacement, "#");
    }
}
