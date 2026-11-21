//! The lossless concrete syntax tree.
//!
//! Every token of the source appears exactly once, in order, so the tree plus
//! the token texts reconstructs the input byte for byte. The tree is built by
//! [`crate::parse`]; it is immutable afterwards.

use crate::{Diagnostic, TokenKind};

/// Node and token kinds of the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyntaxKind {
    // Nodes.
    SourceFile,
    Line,
    Label,
    Instruction,
    Directive,
    /// One instruction operand, or one directive argument. Syntactically they
    /// have the same shape: a run of tokens up to the next comma.
    Operand,
    /// Recovery node: tokens that could not be attached to a statement.
    ErrorToken,

    // Tokens.
    Ident,
    Number,
    String,
    DirectiveName,
    Comma,
    Colon,
    LParen,
    RParen,
    Percent,
    /// `+` or `-` before an extension name in `.option arch`.
    Plus,
    Minus,
    /// From `#` to the end of the line.
    Comment,
    Whitespace,
    Newline,
}

impl SyntaxKind {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::SourceFile => "SourceFile",
            Self::Line => "Line",
            Self::Label => "Label",
            Self::Instruction => "Insn",
            Self::Directive => "Dir",
            Self::Operand => "Op",
            Self::Comment => "Comment",
            Self::ErrorToken => "Error",
            Self::Ident => "Ident",
            Self::Number => "Number",
            Self::String => "String",
            Self::DirectiveName => "DirName",
            Self::Comma => "Comma",
            Self::Colon => "Colon",
            Self::LParen => "LParen",
            Self::RParen => "RParen",
            Self::Percent => "Percent",
            Self::Plus => "Plus",
            Self::Minus => "Minus",
            Self::Whitespace => "WS",
            Self::Newline => "NL",
        }
    }

    /// True for kinds that appear only as leaves.
    #[must_use]
    pub fn is_token(self) -> bool {
        !matches!(
            self,
            Self::SourceFile
                | Self::Line
                | Self::Label
                | Self::Instruction
                | Self::Directive
                | Self::Operand
                | Self::ErrorToken
        )
    }
}

impl From<TokenKind> for SyntaxKind {
    fn from(kind: TokenKind) -> Self {
        match kind {
            TokenKind::Ident => Self::Ident,
            TokenKind::Number => Self::Number,
            TokenKind::String => Self::String,
            TokenKind::Directive => Self::DirectiveName,
            TokenKind::Comma => Self::Comma,
            TokenKind::Colon => Self::Colon,
            TokenKind::LParen => Self::LParen,
            TokenKind::RParen => Self::RParen,
            TokenKind::Percent => Self::Percent,
            TokenKind::Plus => Self::Plus,
            TokenKind::Minus => Self::Minus,
            TokenKind::Comment => Self::Comment,
            TokenKind::Whitespace => Self::Whitespace,
            TokenKind::Newline => Self::Newline,
            TokenKind::Error => Self::ErrorToken,
        }
    }
}

/// A leaf: a kind and a byte span into the tree's source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeToken {
    pub kind: SyntaxKind,
    pub span: (usize, usize),
}

/// A child of a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Element {
    Node(Node),
    Token(TreeToken),
}

/// An interior node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub kind: SyntaxKind,
    pub children: Vec<Element>,
}

impl Node {
    #[must_use]
    pub fn new(kind: SyntaxKind) -> Self {
        Self {
            kind,
            children: Vec::new(),
        }
    }

    /// Append a child node and return it for further filling.
    pub fn push_node(&mut self, kind: SyntaxKind) -> &mut Self {
        self.children.push(Element::Node(Self::new(kind)));
        match self.children.last_mut() {
            Some(Element::Node(node)) => node,
            _ => unreachable!("just pushed a node"),
        }
    }

    pub fn push_token(&mut self, kind: SyntaxKind, span: (usize, usize)) {
        self.children.push(Element::Token(TreeToken { kind, span }));
    }

    pub fn push_child(&mut self, other: Node) {
        self.children.push(Element::Node(other));
    }

    /// First direct child node of this kind.
    #[must_use]
    pub fn child(&self, kind: SyntaxKind) -> Option<&Self> {
        self.children.iter().find_map(|child| match child {
            Element::Node(node) if node.kind == kind => Some(node),
            _ => None,
        })
    }
}

/// A parsed source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxTree {
    source: String,
    root: Node,
    diagnostics: Vec<Diagnostic>,
}

impl SyntaxTree {
    pub(crate) fn new(source: String, root: Node, diagnostics: Vec<Diagnostic>) -> Self {
        Self {
            source,
            root,
            diagnostics,
        }
    }

    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    #[must_use]
    pub fn root(&self) -> &Node {
        &self.root
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// True when any token was unclassifiable or unattachable.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        fn walk(node: &Node) -> bool {
            node.kind == SyntaxKind::ErrorToken
                || node.children.iter().any(|child| match child {
                    Element::Node(node) => walk(node),
                    Element::Token(token) => token.kind == SyntaxKind::ErrorToken,
                })
        }
        walk(&self.root)
    }

    /// Text of a leaf token.
    #[must_use]
    pub fn token_text(&self, token: &TreeToken) -> &str {
        &self.source[token.span.0..token.span.1]
    }

    /// Indented rendering, used by tests and by `rvh parse --tree`.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        render_node(&mut out, &self.root, 0, &self.source);
        out.push_str("---\n");
        if self.diagnostics.is_empty() {
            out.push_str("diagnostics: none\n");
        } else {
            out.push_str(&format!("diagnostics: {}\n", self.diagnostics.len()));
            for diagnostic in &self.diagnostics {
                out.push_str(&format!(
                    "  {} at {}..{}: {}\n",
                    diagnostic.code, diagnostic.span.0, diagnostic.span.1, diagnostic.message
                ));
                for fix in diagnostic.fixes() {
                    out.push_str(&format!(
                        "    fix {}..{}: {}\n",
                        fix.span.0, fix.span.1, fix.title
                    ));
                }
            }
        }
        out
    }
}

fn render_node(out: &mut String, node: &Node, depth: usize, source: &str) {
    let pad = "  ".repeat(depth);
    out.push_str(&format!("{pad}{}\n", node.kind.name()));
    let child_pad = "  ".repeat(depth + 1);
    for child in &node.children {
        match child {
            Element::Node(node) => render_node(out, node, depth + 1, source),
            Element::Token(token) => {
                out.push_str(&format!(
                    "{child_pad}{} {:?}\n",
                    token.kind.name(),
                    &source[token.span.0..token.span.1]
                ));
            }
        }
    }
}

/// A token plus the chain of nodes that contain it, root first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenAt<'a> {
    pub token: TreeToken,
    pub nodes: Vec<&'a Node>,
}

impl Node {
    /// Span of the first token in this subtree.
    #[must_use]
    pub fn first_token_span(&self) -> Option<(usize, usize)> {
        for child in &self.children {
            match child {
                Element::Node(node) => {
                    if let Some(span) = node.first_token_span() {
                        return Some(span);
                    }
                }
                Element::Token(token) => return Some(token.span),
            }
        }
        None
    }
}

impl SyntaxTree {
    /// The token at `offset`, with its ancestors.
    #[must_use]
    pub fn token_at(&self, offset: usize) -> Option<TokenAt<'_>> {
        fn walk<'a>(
            node: &'a Node,
            offset: usize,
            nodes: &mut Vec<&'a Node>,
            found: &mut Option<TokenAt<'a>>,
        ) {
            if found.is_some() {
                return;
            }
            for child in &node.children {
                match child {
                    Element::Node(child_node) => {
                        nodes.push(child_node);
                        walk(child_node, offset, nodes, found);
                        if found.is_some() {
                            return;
                        }
                        nodes.pop();
                    }
                    Element::Token(token) => {
                        if offset >= token.span.0 && offset < token.span.1 {
                            *found = Some(TokenAt {
                                token: *token,
                                nodes: nodes.clone(),
                            });
                            return;
                        }
                    }
                }
            }
        }

        let mut nodes = vec![&self.root];
        let mut found = None;
        walk(&self.root, offset, &mut nodes, &mut found);
        found
    }
}
