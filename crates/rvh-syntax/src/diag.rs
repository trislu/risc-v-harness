//! Diagnostics produced by the syntax layer, with optional fixes.

/// A concrete edit that resolves a diagnostic.
///
/// The shape is deliberate: `(span, replacement)` maps to an LSP `TextEdit` and
/// to a CLI `--fix` without a translation layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    pub title: String,
    pub span: (usize, usize),
    pub replacement: String,
}

/// One problem, with a stable code so that a fix can be attached later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Stable identifier, e.g. `syntax.expected-comma`.
    pub code: &'static str,
    pub message: String,
    /// Byte span into the source.
    pub span: (usize, usize),
    fixes: Vec<Fix>,
}

impl Diagnostic {
    #[must_use]
    pub fn new(code: &'static str, message: impl Into<String>, span: (usize, usize)) -> Self {
        Self {
            code,
            message: message.into(),
            span,
            fixes: Vec::new(),
        }
    }

    /// Attach a fix that replaces `span` with `replacement`.
    #[must_use]
    pub fn with_fix(
        mut self,
        title: impl Into<String>,
        span: (usize, usize),
        replacement: impl Into<String>,
    ) -> Self {
        self.fixes.push(Fix {
            title: title.into(),
            span,
            replacement: replacement.into(),
        });
        self
    }

    #[must_use]
    pub fn fixes(&self) -> &[Fix] {
        &self.fixes
    }
}
