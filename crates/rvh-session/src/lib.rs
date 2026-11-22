//! One document, one target, one machine.
//!
//! This is the facade every host shares: the command line tool, the C ABI used
//! by the browser and the extension host, and later the language server. It owns
//! no policy of its own beyond "parse, resolve, assemble, and only run what
//! assembled cleanly".

#![forbid(unsafe_code)]

use rvh_asm::Image;
use rvh_isa::Target;
use rvh_json::{number, object, string, Json};
use rvh_sem::Program;
use rvh_sim::{Change, HaltReason, Machine};
use rvh_syntax::{Diagnostic, Element, Node, SyntaxKind, SyntaxTree, TokenAt};

/// A session over one source document.
#[derive(Debug, Clone)]
pub struct Session {
    target: Target,
    /// The target the file itself declared, which may be wider than the
    /// requested one (`.attribute arch`, `.option arch`).
    effective: Target,
    locale: String,
    source: String,
    tree: SyntaxTree,
    program: Program,
    image: Image,
    diagnostics: Vec<Diagnostic>,
    machine: Option<Machine>,
    /// Memory map every machine in this session is built on.
    board: &'static rvh_sim::Board,
}

impl Session {
    #[must_use]
    pub fn new(target: Target) -> Self {
        let mut session = Self {
            effective: target.clone(),
            target: target.clone(),
            locale: "en".to_string(),
            source: String::new(),
            tree: rvh_syntax::parse(""),
            program: empty_program(target),
            image: empty_image(),
            diagnostics: Vec::new(),
            machine: None,
            board: &rvh_sim::Board::GENERIC,
        };
        session.rebuild();
        session
    }

    /// Replace the document and rebuild everything downstream of it.
    /// Load a statically linked ELF instead of assembling the source.
    ///
    /// The document tree is left as it was; only the machine changes. Hover and
    /// tokens keep working on whatever the editor holds.
    pub fn load_elf(&mut self, bytes: &[u8]) -> Result<(), String> {
        let machine = rvh_sim::Machine::from_elf_on(self.board, bytes)?;
        self.machine = Some(machine);
        self.diagnostics.clear();
        Ok(())
    }

    /// True when the last successful load was an ELF rather than an assemble.
    #[must_use]
    pub fn loaded_elf(&self) -> bool {
        self.machine.is_some() && self.image.sections.is_empty()
    }

    pub fn set_source(&mut self, source: &str) {
        self.source.clear();
        self.source.push_str(source);
        self.rebuild();
    }

    fn rebuild(&mut self) {
        let tree = rvh_syntax::parse(&self.source);
        let program = rvh_sem::resolve(&tree, &self.target);
        self.effective = program.target.clone();
        let (image, asm_diagnostics) = rvh_asm::assemble(&program);

        let mut diagnostics = tree.diagnostics().to_vec();
        diagnostics.extend(program.diagnostics.iter().cloned());
        diagnostics.extend(asm_diagnostics);
        diagnostics.sort_by_key(|diagnostic| (diagnostic.span.0, diagnostic.span.1));

        self.tree = tree;
        self.program = program;
        self.image = image;
        self.machine = None;
        self.diagnostics = diagnostics;
        if self.diagnostics.is_empty() {
            let mut machine = Machine::with_board(self.board);
            machine.load(&self.image);
            self.machine = Some(machine);
        }
    }

    /// Run on another board. Rebuilds the machine, keeping the source.
    pub fn set_board(&mut self, board: &'static rvh_sim::Board) {
        self.board = board;
        self.rebuild();
    }

    #[must_use]
    pub fn board(&self) -> &'static rvh_sim::Board {
        self.board
    }

    #[must_use]
    pub fn target(&self) -> &Target {
        &self.target
    }

    /// The target in effect after the file's own declarations.
    #[must_use]
    pub fn effective_target(&self) -> &Target {
        &self.effective
    }

    /// Locale for content lookups, such as `zh-CN`. Unknown values fall back to
    /// English at lookup time.
    #[must_use]
    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// Change the locale used by hover text.
    pub fn set_locale(&mut self, locale: &str) {
        self.locale.clear();
        self.locale.push_str(locale);
    }

    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    #[must_use]
    pub fn image(&self) -> &Image {
        &self.image
    }

    /// The machine exists only when the document assembled without diagnostics.
    #[must_use]
    pub fn machine(&self) -> Option<&Machine> {
        self.machine.as_ref()
    }

    /// Reload the program from the start.
    pub fn reset(&mut self) {
        if self.machine.is_some() {
            let mut machine = Machine::new();
            machine.load(&self.image);
            self.machine = Some(machine);
        }
    }

    /// Run to a halt or to the step limit.
    pub fn run(&mut self, max_steps: u64) -> Option<HaltReason> {
        self.machine.as_mut()?.run(max_steps)
    }

    /// Execute up to `count` instructions. Returns false when the machine stops.
    pub fn step(&mut self, count: u32) -> bool {
        let Some(machine) = self.machine.as_mut() else {
            return false;
        };
        for _ in 0..count {
            if !machine.step() {
                return false;
            }
        }
        true
    }

    /// Undo up to `count` instructions. Returns false when history runs out.
    pub fn undo(&mut self, count: u32) -> bool {
        let Some(machine) = self.machine.as_mut() else {
            return false;
        };
        for _ in 0..count {
            if !machine.undo() {
                return false;
            }
        }
        true
    }

    /// Diagnostics as JSON, ready for the browser or the extension host.
    #[must_use]
    pub fn diagnostics_json(&self) -> String {
        let items = self
            .diagnostics
            .iter()
            .map(|diagnostic| {
                let (line, column) = line_col(&self.source, diagnostic.span.0);
                let fixes = diagnostic
                    .fixes()
                    .iter()
                    .map(|fix| {
                        object(vec![
                            ("title", string(fix.title.clone())),
                            ("start", number(fix.span.0 as f64)),
                            ("end", number(fix.span.1 as f64)),
                            ("replacement", string(fix.replacement.clone())),
                        ])
                    })
                    .collect();
                object(vec![
                    ("code", string(diagnostic.code)),
                    ("message", string(diagnostic.message.clone())),
                    ("line", number(line as f64)),
                    ("column", number(column as f64)),
                    ("start", number(diagnostic.span.0 as f64)),
                    ("end", number(diagnostic.span.1 as f64)),
                    ("fixes", Json::Array(fixes)),
                ])
            })
            .collect();
        object(vec![
            ("ok", Json::Bool(self.diagnostics.is_empty())),
            ("diagnostics", Json::Array(items)),
        ])
        .to_string()
    }

    /// Machine state as JSON. `changed` describes the last executed step, so a
    /// host can highlight what moved without diffing snapshots.
    #[must_use]
    pub fn state_json(&self) -> String {
        let Some(machine) = self.machine.as_ref() else {
            return object(vec![
                ("ok", Json::Bool(false)),
                ("reason", string("diagnostics")),
                ("pc", number(0.0)),
                ("steps", number(0.0)),
                ("halt", Json::Null),
                ("output", string("")),
                ("text_bytes", number(0.0)),
                ("line", number(0.0)),
                ("column", number(0.0)),
                ("regs", Json::Array(Vec::new())),
                ("changed", Json::Array(Vec::new())),
            ])
            .to_string();
        };

        let regs = (0..32)
            .map(|index| number(machine.reg(index) as f64))
            .collect();
        let changed = match machine.trace().last() {
            Some(step) => step
                .changes
                .iter()
                .map(|change| match change {
                    Change::Reg { index, old } => object(vec![
                        ("reg", number(*index as f64)),
                        ("old", number(*old as f64)),
                        ("new", number(machine.reg(usize::from(*index)) as f64)),
                    ]),
                    Change::Mem { addr, old } => object(vec![
                        ("addr", number(*addr as f64)),
                        ("old", number(*old as f64)),
                        (
                            "new",
                            number(machine.read_mem(*addr, 1).map_or(0, |bytes| bytes[0]) as f64),
                        ),
                    ]),
                })
                .collect(),
            None => Vec::new(),
        };
        let (line, column) = self.line_of_pc().unwrap_or((0, 0));

        object(vec![
            ("ok", Json::Bool(true)),
            ("pc", number(machine.pc() as f64)),
            ("steps", number(machine.steps() as f64)),
            ("halt", halt_json(machine.halt())),
            ("output", string(machine.output_text())),
            ("text_bytes", number(self.image.text().len() as f64)),
            ("line", number(line as f64)),
            ("column", number(column as f64)),
            ("regs", Json::Array(regs)),
            ("changed", Json::Array(changed)),
        ])
        .to_string()
    }

    /// Address of the first instruction on a 1-based source line.
    ///
    /// Used for breakpoints. Returns `None` when the line holds no instruction,
    /// which is the answer a debugger needs for a breakpoint it cannot honour.
    #[must_use]
    pub fn address_of_line(&self, line: usize) -> Option<u32> {
        self.image
            .source_map
            .iter()
            .find(|entry| line_col(&self.source, entry.span.0).0 == line)
            .map(|entry| entry.addr)
    }

    /// Byte span of the token at an offset, for `LocationLink.originSelectionRange`.
    #[must_use]
    pub fn token_span(&self, offset: usize) -> Option<(usize, usize)> {
        self.tree.token_at(offset).map(|at| at.token.span)
    }

    /// Source line of the program counter: what runs next.
    ///
    /// The image's source map is sorted by address, so the nearest entry at or
    /// below `pc` is the instruction that owns it.
    #[must_use]
    pub fn line_of_pc(&self) -> Option<(usize, usize)> {
        let pc = self.machine.as_ref()?.pc();
        let entry = self
            .image
            .source_map
            .iter()
            .rev()
            .find(|entry| entry.addr <= pc)?;
        Some(line_col(&self.source, entry.span.0))
    }

    /// Diagnostics as one line each, for a terminal.
    #[must_use]
    pub fn diagnostics_text(&self) -> String {
        let mut out = String::new();
        for diagnostic in &self.diagnostics {
            let (line, column) = line_col(&self.source, diagnostic.span.0);
            out.push_str(&format!(
                "{line}:{column}: {}: {}\n",
                diagnostic.code, diagnostic.message
            ));
            for fix in diagnostic.fixes() {
                let (fix_line, fix_column) = line_col(&self.source, fix.span.0);
                out.push_str(&format!("{fix_line}:{fix_column}: help: {}\n", fix.title));
            }
        }
        out
    }
}

fn empty_program(target: Target) -> Program {
    Program {
        sections: Vec::new(),
        symbols: Vec::new(),
        diagnostics: Vec::new(),
        target,
    }
}

fn empty_image() -> Image {
    Image {
        sections: Vec::new(),
        entry: 0,
        symbols: Vec::new(),
        source_map: Vec::new(),
    }
}

fn halt_json(halt: Option<HaltReason>) -> Json {
    match halt {
        None => Json::Null,
        Some(HaltReason::Ebreak { .. }) => string("ebreak"),
        Some(HaltReason::Ecall { .. }) => string("ecall"),
        Some(HaltReason::Illegal { .. }) => string("illegal"),
        Some(HaltReason::Fault { .. }) => string("fault"),
        Some(HaltReason::StepLimit) => string("step-limit"),
    }
}

/// 1-based `(line, column)` of a byte offset, counting characters.
#[must_use]
pub fn line_col(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut column = 1;
    for (index, character) in source.char_indices() {
        if index >= offset {
            break;
        }
        if character == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(source: &str) -> Session {
        let mut session = Session::new(Target::parse("rv32i_zicsr").expect("target"));
        session.set_source(source);
        session
    }

    #[test]
    fn a_clean_program_gets_a_machine() {
        let session = session("li a0, 55\nebreak\n");
        assert!(session.diagnostics().is_empty());
        assert!(session.machine().is_some());
        assert_eq!(session.image().entry, 0);
    }

    #[test]
    fn a_broken_program_has_no_machine() {
        let mut session = session("adii a0, a1, 1\n");
        assert_eq!(session.diagnostics().len(), 1);
        assert!(session.machine().is_none());
        assert!(!session.step(1));
    }

    #[test]
    fn diagnostics_json_carries_fixes() {
        let session = session("adii a0, a1, 1\n");
        let json = session.diagnostics_json();
        assert!(json.contains("\"ok\":false"));
        assert!(json.contains("sem.unknown-instruction"));
        assert!(json.contains("replace with `addi`"));
    }

    #[test]
    fn state_json_reports_registers_and_changes() {
        let mut session = session("li a0, 7\nebreak\n");
        session.step(1);
        let json = session.state_json();
        assert!(json.contains("\"ok\":true"));
        assert!(json.contains("\"regs\":["));
        assert!(json.contains("\"changed\":[{\"reg\":10,"));
        session.run(10);
        assert!(session.state_json().contains("\"halt\":\"ebreak\""));
    }

    #[test]
    fn editing_the_source_resets_the_machine() {
        let mut session = session("li a0, 1\nebreak\n");
        session.run(10);
        assert_eq!(session.machine().expect("machine").reg(10), 1);
        session.set_source("li a0, 2\nebreak\n");
        session.run(10);
        assert_eq!(session.machine().expect("machine").reg(10), 2);
        assert_eq!(session.machine().expect("machine").steps(), 2);
    }

    #[test]
    fn undo_walks_back_through_the_session() {
        let mut session = session("li a0, 1\nli a1, 2\nebreak\n");
        session.run(10);
        // Undo the `ebreak` first: it changed nothing.
        assert!(session.undo(1));
        assert_eq!(session.machine().expect("machine").reg(11), 2);
        assert!(session.undo(1));
        assert_eq!(session.machine().expect("machine").reg(11), 0);
        assert!(session.undo(1));
        assert_eq!(session.machine().expect("machine").reg(10), 0);
    }

    #[test]
    fn diagnostics_text_is_one_line_each() {
        let session = session("; note\n");
        let text = session.diagnostics_text();
        assert!(text.contains("syntax.semicolon-comment"), "{text}");
        assert!(text.contains("help: replace with `#`"), "{text}");
    }
}

/// What the cursor is on, with text ready to render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hover {
    pub kind: &'static str,
    pub title: String,
    pub text: String,
    pub span: (usize, usize),
}

impl Hover {
    #[must_use]
    pub fn to_json(&self) -> String {
        object(vec![
            ("kind", string(self.kind)),
            ("title", string(self.title.clone())),
            ("text", string(self.text.clone())),
            ("start", number(self.span.0 as f64)),
            ("end", number(self.span.1 as f64)),
        ])
        .to_string()
    }
}

impl Session {
    /// What the cursor at `offset` is on, if anything.
    #[must_use]
    pub fn hover(&self, offset: usize) -> Option<Hover> {
        let at = self.tree.token_at(offset)?;
        let text = self.source.get(at.token.span.0..at.token.span.1)?;
        let span = at.token.span;
        match at.token.kind {
            SyntaxKind::DirectiveName => Some(directive_hover(&self.locale, text, span)),
            SyntaxKind::Number => number_hover(text, span),
            SyntaxKind::Ident => self.ident_hover(text, &at, span),
            SyntaxKind::Plus | SyntaxKind::Minus => Some(Hover {
                kind: "operator",
                title: text.to_string(),
                text: if text == "+" {
                    "`+` — enable an extension from this line on".to_string()
                } else {
                    "`-` — disable an extension from this line on".to_string()
                },
                span,
            }),
            _ => None,
        }
    }

    /// Hover as JSON: `{"hover": null}` or `{"hover": {...}}`.
    #[must_use]
    pub fn hover_json(&self, offset: usize) -> String {
        match self.hover(offset) {
            Some(hover) => format!("{{\"hover\":{}}}", hover.to_json()),
            None => "{\"hover\":null}".to_string(),
        }
    }

    fn ident_hover(&self, text: &str, at: &TokenAt<'_>, span: (usize, usize)) -> Option<Hover> {
        if at
            .nodes
            .last()
            .is_some_and(|node| node.kind == SyntaxKind::Label)
        {
            return Some(self.label_hover(text, span));
        }
        if let Some(instruction) = at
            .nodes
            .iter()
            .rev()
            .find(|node| node.kind == SyntaxKind::Instruction)
        {
            if instruction.first_token_span() == Some(span) {
                return self.instruction_hover(text, span);
            }
        }
        if let Some(index) = rvh_sem::parse_register(text) {
            return self.register_hover(index, span);
        }
        if let Some(number) = rvh_sem::csr_number(text) {
            return Some(Hover {
                kind: "csr",
                title: text.to_string(),
                text: format!("`{text}` — control and status register `0x{number:03x}`"),
                span,
            });
        }
        if let Some(symbol) = self.image.symbol(text) {
            return Some(Hover {
                kind: "label",
                title: text.to_string(),
                text: format!("`{text}` — label at `0x{:08x}`", symbol.addr),
                span,
            });
        }
        if let Some(value) = self.program.constant(text) {
            return Some(Hover {
                kind: "constant",
                title: text.to_string(),
                text: format!("`{text}` — constant `{value}`"),
                span,
            });
        }
        if let Some(name) = rvh_isa::extensions().iter().find(|name| **name == text) {
            let count = rvh_isa::insns_in(name).count();
            return Some(Hover {
                kind: "extension",
                title: text.to_string(),
                text: format!("`{text}` — extension `{name}`, {count} instruction(s)"),
                span,
            });
        }
        if text == "arch" {
            return Some(Hover {
                kind: "attribute",
                title: text.to_string(),
                text: "`arch` — architecture name, as in `rv32i2p1_m2p0`".to_string(),
                span,
            });
        }
        // A directive argument that is neither a name nor a known value still
        // deserves an answer: the reader is asking "what is this word".
        if at.nodes.iter().any(|node| node.kind == SyntaxKind::Operand) {
            return Some(Hover {
                kind: "argument",
                title: text.to_string(),
                text: format!("`{text}` — directive argument"),
                span,
            });
        }
        None
    }

    fn instruction_hover(&self, text: &str, span: (usize, usize)) -> Option<Hover> {
        if let Some(insn) = rvh_isa::lookup(text) {
            return self.base_instruction_hover(insn, span);
        }
        if rvh_sem::PSEUDOS.contains(&text) {
            let description = rvh_content::text(&self.locale, rvh_content::Kind::Pseudo, text)
                .unwrap_or("pseudo-instruction");
            return Some(Hover {
                kind: "pseudo",
                title: text.to_string(),
                text: format!("`{text}` — {description}"),
                span,
            });
        }
        // An unknown mnemonic still has something to say: the diagnostic that
        // explains it, including the did-you-mean suggestion.
        if let Some(diagnostic) = self
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.span.0 <= span.0 && span.1 <= diagnostic.span.1)
        {
            return Some(Hover {
                kind: "diagnostic",
                title: text.to_string(),
                text: format!("`{}` — {}", diagnostic.code, diagnostic.message),
                span,
            });
        }
        None
    }

    fn base_instruction_hover(
        &self,
        insn: &'static rvh_isa::Insn,
        span: (usize, usize),
    ) -> Option<Hover> {
        // Markdown with a fixed shape: a header line, then a table of operands.
        // A reader scanning a hover sees the same layout for every instruction,
        // and a "See also" section can be appended later without reshuffling.
        let mut lines = vec![format!(
            "**`{}`** · format `{}` · extension `{}` · class `{}`",
            insn.mnemonic,
            insn.format.as_str(),
            insn.ext,
            insn.class
        )];
        if !insn.operands.is_empty() {
            lines.push(String::new());
            lines.push("| operand | bits | range |".to_string());
            lines.push("| --- | --- | --- |".to_string());
            for operand in insn.operands {
                let range = if operand.kind == rvh_isa::OperandKind::Imm && operand.bits < 32 {
                    let (low, high) = operand.range();
                    format!("`{low}`..`{high}`")
                } else {
                    String::new()
                };
                lines.push(format!(
                    "| `{}` | {} | {} |",
                    operand.kind.as_str(),
                    operand.bits,
                    range
                ));
            }
        }
        Some(Hover {
            kind: "instruction",
            title: insn.mnemonic.to_string(),
            text: lines.join("\n"),
            span,
        })
    }

    fn register_hover(&self, index: u8, span: (usize, usize)) -> Option<Hover> {
        let name = rvh_sem::register_name(index)?;
        let role = rvh_content::register_role(&self.locale, index).unwrap_or("");
        let mut text = format!("`{name}` (x{index}) — {role}");
        if let Some(machine) = self.machine.as_ref() {
            let value = machine.reg(usize::from(index));
            text.push_str(&format!(
                "\n\ncurrent value: `0x{value:08x}` ({})",
                value as i32
            ));
        }
        Some(Hover {
            kind: "register",
            title: name.to_string(),
            text,
            span,
        })
    }

    fn label_hover(&self, text: &str, span: (usize, usize)) -> Hover {
        let text_body = match self.image.symbol(text) {
            Some(symbol) => format!("`{text}` — label at `0x{:08x}`", symbol.addr),
            None => match self.program.constant(text) {
                Some(value) => format!("`{text}` — constant `{value}`"),
                None => format!("`{text}` — label"),
            },
        };
        Hover {
            kind: "label",
            title: text.to_string(),
            text: text_body,
            span,
        }
    }
}

fn directive_hover(locale: &str, text: &str, span: (usize, usize)) -> Hover {
    let description = rvh_content::text(locale, rvh_content::Kind::Directive, text)
        .unwrap_or("assembler directive");
    Hover {
        kind: "directive",
        title: text.to_string(),
        text: format!("`{text}` — {description}"),
        span,
    }
}

fn number_hover(text: &str, span: (usize, usize)) -> Option<Hover> {
    let value = rvh_syntax::parse_number(text)?;
    Some(Hover {
        kind: "number",
        title: text.to_string(),
        text: format!("`{text}` = {value} = `{value:#x}`"),
        span,
    })
}

#[cfg(test)]
mod hover_tests {
    use super::*;

    fn session(source: &str) -> Session {
        let mut session = Session::new(Target::parse("rv32i_zicsr").expect("target"));
        session.set_source(source);
        session
    }

    #[test]
    fn hover_explains_the_mnemonic() {
        let session = session("addi a0, a0, 1\n");
        let hover = session.hover(1).expect("hover");
        assert_eq!(hover.kind, "instruction");
        assert!(hover.text.contains("format `I`"), "{}", hover.text);
        assert!(
            hover.text.contains("| operand | bits | range |"),
            "{}",
            hover.text
        );
        assert!(hover.text.contains("`-2048`..`2047`"), "{}", hover.text);
    }

    #[test]
    fn hover_shows_registers_and_their_values() {
        let mut session = session("addi a0, a0, 1\nebreak\n");
        session.run(10);
        let hover = session.hover(5).expect("hover");
        assert_eq!(hover.kind, "register");
        assert!(hover.title == "a0", "{}", hover.title);
        assert!(hover.text.contains("0x00000001"), "{}", hover.text);
    }

    #[test]
    fn hover_knows_csrs() {
        let session = session("start:\n  csrw mtvec, t0\n");
        let hover = session.hover(17).expect("hover");
        assert_eq!(hover.kind, "csr");
        assert!(hover.text.contains("0x305"), "{}", hover.text);
    }

    #[test]
    fn hover_knows_labels_and_directives() {
        let session = session("start:\n  .text\n");
        let label = session.hover(1).expect("label hover");
        assert_eq!(label.kind, "label");
        let directive = session.hover(9).expect("directive hover");
        assert_eq!(directive.kind, "directive");
        assert!(
            directive.text.contains("text section"),
            "{}",
            directive.text
        );
    }

    #[test]
    fn hover_reads_numbers() {
        let session = session("li a0, 0x10\n");
        let hover = session.hover(10).expect("number hover");
        assert_eq!(hover.kind, "number");
        assert!(hover.text.contains("= 16"), "{}", hover.text);
    }

    /// Every word in a valid file must answer a hover. Strings and comments are
    /// content, not names, so they are excluded.
    #[test]
    fn every_word_answers_a_hover() {
        let cases = [
            include_str!("../../../tests/corpus/004_directives.s"),
            include_str!("../../../tests/corpus/006_arch_directives.s"),
            include_str!("../../../tests/corpus/007_arch_attribute.s"),
        ];
        for source in cases {
            let mut session = Session::new(Target::parse("rv32i_zicsr").expect("target"));
            session.set_source(source);
            let content: Vec<(usize, usize)> = session
                .tokens()
                .into_iter()
                .filter(|token| matches!(token.kind, TokenKind::Comment | TokenKind::String))
                .map(|token| token.span)
                .collect();

            let mut missing = Vec::new();
            for (offset, character) in source.char_indices() {
                let word =
                    character.is_alphanumeric() || matches!(character, '_' | '.' | '+' | '-');
                if !word
                    || content
                        .iter()
                        .any(|(start, end)| *start <= offset && offset < *end)
                {
                    continue;
                }
                if session.hover(offset).is_none() {
                    missing.push(character);
                }
            }
            assert!(missing.is_empty(), "no hover for: {missing:?}");
        }
    }

    #[test]
    fn hover_on_an_unknown_mnemonic_shows_the_diagnostic() {
        let session = session("adii a0, a1, 1\n");
        let hover = session.hover(1).expect("hover");
        assert_eq!(hover.kind, "diagnostic");
        assert!(hover.text.contains("did you mean `addi`"), "{}", hover.text);
    }

    #[test]
    fn hover_is_absent_on_whitespace_and_comments() {
        let session = session("nop # note\n");
        assert!(session.hover(3).is_none());
        assert!(session.hover(8).is_none());
    }

    #[test]
    fn hover_json_is_wrapped() {
        let session = session("addi a0, a0, 1\n");
        assert!(session.hover_json(1).contains("\"kind\":\"instruction\""));
        assert_eq!(session.hover_json(21), "{\"hover\":null}");
    }

    #[test]
    fn hover_explains_a_pseudo_instruction() {
        let session = session("nop\n");
        let hover = session.hover(1).expect("hover");
        assert_eq!(hover.kind, "pseudo");
        assert!(hover.text.contains("addi x0, x0, 0"), "{}", hover.text);
    }
}

#[cfg(test)]
mod line_tests {
    use super::*;

    fn session(source: &str) -> Session {
        let mut session = Session::new(Target::parse("rv32i_zicsr").expect("target"));
        session.set_source(source);
        session
    }

    #[test]
    fn the_pc_maps_to_its_source_line() {
        let source = "# first\nli a0, 1\nli a1, 2\nebreak\n";
        let mut session = session(source);
        assert_eq!(session.line_of_pc(), Some((2, 1)));
        session.step(1);
        assert_eq!(session.line_of_pc(), Some((3, 1)));
        session.run(10);
        assert_eq!(session.line_of_pc(), Some((4, 1)));
        assert!(session.state_json().contains("\"line\":4"));
    }

    #[test]
    fn undo_moves_the_line_back() {
        let source = "li a0, 1\nli a1, 2\nebreak\n";
        let mut session = session(source);
        session.run(10);
        assert_eq!(session.line_of_pc(), Some((3, 1)));
        session.undo(1);
        assert_eq!(session.line_of_pc(), Some((3, 1)), "ebreak changed nothing");
        session.undo(1);
        assert_eq!(session.line_of_pc(), Some((2, 1)));
    }

    #[test]
    fn a_broken_program_has_no_line() {
        let session = session("adii a0, a1, 1\n");
        assert_eq!(session.line_of_pc(), None);
        assert!(session.state_json().contains("\"line\":0"));
    }
}

#[cfg(test)]
mod content_tests {
    use super::*;

    #[test]
    fn hover_follows_the_locale() {
        let mut session = Session::new(Target::parse("rv32i_zicsr").expect("target"));
        session.set_source("li a0, 1\n");

        let english = session.hover(1).expect("hover").text;
        assert!(english.contains("load immediate"), "{english}");

        session.set_locale("zh-CN");
        let chinese = session.hover(1).expect("hover").text;
        assert!(chinese.contains("加载"), "{chinese}");
    }

    #[test]
    fn an_untranslated_key_falls_back() {
        let mut session = Session::new(Target::parse("rv32i_zicsr").expect("target"));
        session.set_source("neg a0, a1\n");
        session.set_locale("zh-CN");
        let text = session.hover(1).expect("hover").text;
        assert!(text.contains("negate"), "{text}");
    }

    #[test]
    fn directives_come_from_the_content_layer() {
        let mut session = Session::new(Target::parse("rv32i_zicsr").expect("target"));
        session.set_source(".text\n");
        let text = session.hover(1).expect("hover").text;
        assert!(text.contains("text section"), "{text}");
    }
}

/// What an editor should colour a token as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Mnemonic,
    Pseudo,
    Directive,
    Register,
    Csr,
    Label,
    LabelRef,
    Number,
    String,
    Comment,
    /// `+` or `-`, as in `.option arch, +m`.
    Operator,
    /// A directive argument that is neither a register, a CSR nor a symbol,
    /// such as `arch` in `.option arch` or a `.equ` name.
    Property,
}

/// A classified token, in source order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticToken {
    pub span: (usize, usize),
    pub kind: TokenKind,
}

impl Session {
    /// Classify the tokens an editor should colour.
    ///
    /// Whitespace, commas, colons and parentheses are skipped: every editor
    /// colours those already. The classification comes from the same tree and
    /// tables that produce diagnostics and hover, so it cannot drift from them.
    #[must_use]
    pub fn tokens(&self) -> Vec<SemanticToken> {
        let mut tokens = Vec::new();
        self.walk_tokens(self.tree.root(), &mut tokens);
        tokens
    }

    fn walk_tokens(&self, node: &Node, out: &mut Vec<SemanticToken>) {
        match node.kind {
            SyntaxKind::Label => {
                if let Some(span) = node.first_token_span() {
                    out.push(SemanticToken {
                        span,
                        kind: TokenKind::Label,
                    });
                }
            }
            SyntaxKind::Instruction => {
                let mut head = true;
                for child in &node.children {
                    match child {
                        Element::Node(child) => self.walk_tokens(child, out),
                        Element::Token(token) => {
                            if token.kind == SyntaxKind::Comment {
                                out.push(SemanticToken {
                                    span: token.span,
                                    kind: TokenKind::Comment,
                                });
                            } else if head && token.kind == SyntaxKind::Ident {
                                head = false;
                                let text = &self.source[token.span.0..token.span.1];
                                let kind = if rvh_isa::lookup(text).is_some() {
                                    TokenKind::Mnemonic
                                } else if rvh_sem::PSEUDOS.contains(&text) {
                                    TokenKind::Pseudo
                                } else {
                                    continue;
                                };
                                out.push(SemanticToken {
                                    span: token.span,
                                    kind,
                                });
                            }
                        }
                    }
                }
            }
            SyntaxKind::Directive => {
                for child in &node.children {
                    match child {
                        Element::Node(child) => self.walk_tokens(child, out),
                        Element::Token(token) if token.kind == SyntaxKind::DirectiveName => {
                            out.push(SemanticToken {
                                span: token.span,
                                kind: TokenKind::Directive,
                            });
                        }
                        Element::Token(_) => {}
                    }
                }
            }
            SyntaxKind::Operand => self.walk_operand(node, out),
            _ => {
                for child in &node.children {
                    match child {
                        Element::Node(child) => self.walk_tokens(child, out),
                        Element::Token(token) if token.kind == SyntaxKind::Comment => {
                            out.push(SemanticToken {
                                span: token.span,
                                kind: TokenKind::Comment,
                            });
                        }
                        Element::Token(_) => {}
                    }
                }
            }
        }
    }

    fn walk_operand(&self, node: &Node, out: &mut Vec<SemanticToken>) {
        for child in &node.children {
            match child {
                Element::Node(child) => self.walk_tokens(child, out),
                Element::Token(token) => {
                    let kind = match token.kind {
                        SyntaxKind::String => TokenKind::String,
                        SyntaxKind::Number => TokenKind::Number,
                        SyntaxKind::Plus | SyntaxKind::Minus => TokenKind::Operator,
                        // `.section .data`: a directive name used as an argument.
                        SyntaxKind::DirectiveName => TokenKind::Property,
                        SyntaxKind::Ident => {
                            let text = &self.source[token.span.0..token.span.1];
                            if rvh_sem::register_index(text).is_some() {
                                TokenKind::Register
                            } else if rvh_sem::csr_number(text).is_some() {
                                TokenKind::Csr
                            } else if self.image.symbol(text).is_some() {
                                TokenKind::LabelRef
                            } else {
                                // A directive argument such as `arch`, or the
                                // name in `.equ SIZE, 4`. Leaving it unclassified
                                // means no colour and an empty hover.
                                TokenKind::Property
                            }
                        }
                        _ => continue,
                    };
                    out.push(SemanticToken {
                        span: token.span,
                        kind,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod token_tests {
    use super::*;

    fn tokens(source: &str) -> Vec<SemanticToken> {
        let mut session = Session::new(Target::parse("rv32i_zicsr").expect("target"));
        session.set_source(source);
        session.tokens()
    }

    fn kinds(source: &str) -> Vec<TokenKind> {
        tokens(source).into_iter().map(|token| token.kind).collect()
    }

    #[test]
    fn a_plain_instruction_is_classified() {
        // `addi a0, a1, 1`
        assert_eq!(
            kinds("addi a0, a1, 1\n"),
            vec![
                TokenKind::Mnemonic,
                TokenKind::Register,
                TokenKind::Register,
                TokenKind::Number
            ]
        );
    }

    #[test]
    fn pseudos_are_told_apart_from_real_instructions() {
        assert_eq!(kinds("li a0, 1\n")[0], TokenKind::Pseudo);
        assert_eq!(kinds("lui a0, 1\n")[0], TokenKind::Mnemonic);
    }

    #[test]
    fn labels_definitions_and_references_differ() {
        // start: j start
        let source = "_start:\n    j _start\n    ebreak\n";
        let tokens = tokens(source);
        assert_eq!(tokens[0].kind, TokenKind::Label);
        assert_eq!(tokens[1].kind, TokenKind::Pseudo, "`j` is a pseudo");
        assert_eq!(tokens[2].kind, TokenKind::LabelRef);
    }

    #[test]
    fn directives_csrs_strings_and_comments() {
        let source = ".text\n    csrrw x0, mstatus, t0  # write it\n    .asciz \"hi\"\n";
        let kinds = kinds(source);
        assert_eq!(kinds[0], TokenKind::Directive);
        assert_eq!(kinds[1], TokenKind::Mnemonic);
        assert_eq!(kinds[3], TokenKind::Csr);
        assert_eq!(kinds[4], TokenKind::Register);
        assert!(kinds.contains(&TokenKind::Comment));
        assert!(kinds.contains(&TokenKind::String));
    }

    #[test]
    fn directive_arguments_and_operators_are_classified() {
        // `.option arch, +m`
        assert_eq!(
            kinds(".option arch, +m\n"),
            vec![
                TokenKind::Directive,
                TokenKind::Property,
                TokenKind::Operator,
                TokenKind::Property,
            ]
        );
    }

    #[test]
    fn an_equ_name_is_a_property_and_its_value_a_number() {
        assert_eq!(
            kinds(".equ SIZE, 4\n"),
            vec![TokenKind::Directive, TokenKind::Property, TokenKind::Number]
        );
    }

    #[test]
    fn spans_tile_the_token_text() {
        let source = "addi a0, a1, 1\n";
        let first = tokens(source)[0];
        assert_eq!(&source[first.span.0..first.span.1], "addi");
    }
}

/// What a completion candidate is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Instruction,
    Pseudo,
    Directive,
    Register,
    Csr,
    Label,
}

/// One completion candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub label: String,
    pub kind: CompletionKind,
    /// Short right-hand text: the encoding, the register number, and so on.
    pub detail: String,
}

impl Session {
    /// Completion candidates at a byte offset.
    ///
    /// The context comes from the tree: inside an operand the candidates are
    /// operands, everywhere else they are statement heads.
    #[must_use]
    pub fn completions(&self, offset: usize) -> Vec<Completion> {
        if self.in_operand(offset) {
            self.operand_completions()
        } else {
            self.statement_completions()
        }
    }

    /// Byte span of the definition of the symbol at `offset`, if it has one.
    #[must_use]
    pub fn definition(&self, offset: usize) -> Option<(usize, usize)> {
        let token = self.tree.token_at(offset)?;
        let text = &self.source[token.token.span.0..token.token.span.1];
        if token.token.kind != SyntaxKind::Ident {
            return None;
        }
        self.program
            .symbols
            .iter()
            .find(|symbol| symbol.name == text)
            .map(|symbol| symbol.span)
    }

    fn statement_completions(&self) -> Vec<Completion> {
        let extensions = self.effective.extensions();
        let mut items: Vec<Completion> = rvh_isa::all()
            .iter()
            .filter(|insn| extensions.contains(&insn.ext))
            .map(|insn| Completion {
                label: insn.mnemonic.to_string(),
                kind: CompletionKind::Instruction,
                detail: format!("{} format", insn.format.as_str()),
            })
            .collect();
        items.extend(rvh_sem::PSEUDOS.iter().map(|name| Completion {
            label: (*name).to_string(),
            kind: CompletionKind::Pseudo,
            detail: "pseudo".to_string(),
        }));
        items.extend(rvh_sem::DIRECTIVES.iter().map(|name| Completion {
            label: (*name).to_string(),
            kind: CompletionKind::Directive,
            detail: "directive".to_string(),
        }));
        items
    }

    fn operand_completions(&self) -> Vec<Completion> {
        let mut items = Vec::new();
        for index in 0..32u8 {
            if let Some(name) = rvh_sem::register_name(index) {
                items.push(Completion {
                    label: name.to_string(),
                    kind: CompletionKind::Register,
                    detail: format!("x{index}"),
                });
            }
        }
        items.extend(rvh_sem::CSR_NAMES.iter().map(|(name, number)| Completion {
            label: (*name).to_string(),
            kind: CompletionKind::Csr,
            detail: format!("CSR {number:#05x}"),
        }));
        items.extend(self.program.symbols.iter().map(|symbol| Completion {
            label: symbol.name.clone(),
            kind: CompletionKind::Label,
            detail: "label".to_string(),
        }));
        items
    }

    /// True when `offset` sits inside an operand, or after the head of an
    /// instruction or directive.
    fn in_operand(&self, offset: usize) -> bool {
        let Some(path) = path_to_offset(self.tree.root(), offset) else {
            return false;
        };
        if path.iter().any(|node| node.kind == SyntaxKind::Operand) {
            return true;
        }
        for node in &path {
            if !matches!(node.kind, SyntaxKind::Instruction | SyntaxKind::Directive) {
                continue;
            }
            let head_end = node
                .children
                .iter()
                .find_map(|child| match child {
                    Element::Token(token)
                        if token.kind == SyntaxKind::Ident
                            || token.kind == SyntaxKind::DirectiveName =>
                    {
                        Some(token.span.1)
                    }
                    _ => None,
                })
                .unwrap_or(0);
            return offset > head_end;
        }
        false
    }
}

/// Nodes from the root down to the one holding `offset`.
fn path_to_offset(node: &Node, offset: usize) -> Option<Vec<&Node>> {
    let child = node.children.iter().find_map(|child| match child {
        Element::Node(child) if node_covers(child, offset) => Some(child),
        _ => None,
    });
    if let Some(child) = child {
        let mut path = vec![node];
        if let Some(rest) = path_to_offset(child, offset) {
            path.extend(rest);
            return Some(path);
        }
        return Some(path);
    }
    let holds_token = node.children.iter().any(|child| match child {
        Element::Token(token) => token.span.0 <= offset && offset <= token.span.1,
        Element::Node(_) => false,
    });
    holds_token.then(|| vec![node])
}

fn node_covers(node: &Node, offset: usize) -> bool {
    node.children.iter().any(|child| match child {
        Element::Node(child) => node_covers(child, offset),
        Element::Token(token) => token.span.0 <= offset && offset <= token.span.1,
    })
}

#[cfg(test)]
mod completion_tests {
    use super::*;

    fn session(source: &str) -> Session {
        let mut session = Session::new(Target::parse("rv32i_zicsr").expect("target"));
        session.set_source(source);
        session
    }

    fn labels(items: Vec<Completion>) -> Vec<String> {
        items.into_iter().map(|item| item.label).collect()
    }

    #[test]
    fn statement_position_offers_heads() {
        let session = session("addi a0, a1, 1\n");
        let items = labels(session.completions(0));
        assert!(items.iter().any(|label| label == "addi"), "instruction");
        assert!(items.iter().any(|label| label == "li"), "pseudo");
        assert!(items.iter().any(|label| label == ".text"), "directive");
        assert!(
            !items.iter().any(|label| label == "a0"),
            "no registers here"
        );
    }

    #[test]
    fn operand_position_offers_operands() {
        let session = session("_start:\n    addi a0, a1, 1\n");
        // Offset 18 sits inside `a0`.
        let items = labels(session.completions(18));
        assert!(items.iter().any(|label| label == "a0"), "register");
        assert!(items.iter().any(|label| label == "mstatus"), "csr");
        assert!(items.iter().any(|label| label == "_start"), "label");
        assert!(
            !items.iter().any(|label| label == "addi"),
            "no mnemonics here"
        );
    }

    #[test]
    fn a_trailing_space_switches_to_operands() {
        let session = session("addi \n");
        // Offset 5 is right after the mnemonic and its space.
        let items = labels(session.completions(5));
        assert!(items.iter().any(|label| label == "a0"));
    }

    #[test]
    fn completion_respects_the_target() {
        let mut session = Session::new(Target::parse("rv32i").expect("target"));
        session.set_source("addi a0, a1, 1\n");
        let items = labels(session.completions(0));
        assert!(items.iter().any(|label| label == "addi"));
        assert!(!items.iter().any(|label| label == "csrrw"), "needs zicsr");
    }

    #[test]
    fn a_reference_finds_its_definition() {
        let session = session("_start:\n    j _start\n");
        assert_eq!(session.definition(15), Some((0, 7)));
        assert_eq!(session.definition(12), None, "the mnemonic is not a symbol");
    }
}

#[cfg(test)]
mod target_tests {
    use super::*;

    #[test]
    fn a_declared_target_overrides_the_requested_one() {
        let mut session = Session::new(Target::parse("rv32i_zicsr").expect("target"));
        session.set_source(".attribute arch, \"rv32i2p1_m2p0\"\n_start:\n    mul a0, a1, a2\n");
        assert!(
            session.diagnostics().is_empty(),
            "{:?}",
            session.diagnostics()
        );
        assert!(session.effective_target().has("m"));
        assert_eq!(
            session.target().name(),
            "rv32i_zicsr",
            "the request is unchanged"
        );
    }

    #[test]
    fn completion_follows_the_declared_target() {
        let mut session = Session::new(Target::parse("rv32i_zicsr").expect("target"));
        session.set_source(".option arch, +m\n_start:\n    \n");
        let labels: Vec<String> = session
            .completions(30)
            .into_iter()
            .map(|item| item.label)
            .collect();
        assert!(labels.iter().any(|label| label == "mul"), "{labels:?}");
    }
}

#[cfg(test)]
mod hover_spot_tests {
    use super::*;

    fn hover_at(source: &str, needle: &str) -> Hover {
        let mut session = Session::new(Target::parse("rv32i_zicsr").expect("target"));
        session.set_source(source);
        let offset = source.find(needle).expect("needle");
        session.hover(offset).expect("hover")
    }

    #[test]
    fn the_new_arch_directives_explain_themselves() {
        let hover = hover_at(".option arch, +m\n", ".option");
        assert_eq!(hover.kind, "directive");
        assert!(hover.text.contains("target"), "{}", hover.text);

        let hover = hover_at(".attribute arch, \"rv32i\"\n", ".attribute");
        assert!(hover.text.contains("attribute"), "{}", hover.text);
    }

    #[test]
    fn an_extension_name_says_what_it_adds() {
        let hover = hover_at(".option arch, +m\n", "+");
        assert_eq!(hover.kind, "operator");

        let hover = hover_at(".option arch, +m\n", "m\n");
        assert_eq!(hover.kind, "extension");
        assert!(hover.text.contains("8 instruction"), "{}", hover.text);
    }

    #[test]
    fn the_arch_word_is_explained() {
        let hover = hover_at(".option arch, +m\n", "arch");
        assert_eq!(hover.kind, "attribute");
        assert!(hover.text.contains("rv32i2p1"), "{}", hover.text);
    }
}

#[cfg(test)]
mod board_tests {
    use super::*;

    #[test]
    fn a_board_changes_the_stack_and_the_ram() {
        let mut session = Session::new(Target::parse("rv32i_zicsr").expect("target"));
        session.set_source("_start:\n    ebreak\n");
        assert_eq!(session.machine().expect("machine").reg(2), 0x3_fff0);

        session.set_board(rvh_sim::board("tiny").expect("tiny"));
        assert_eq!(session.board().name, "tiny");
        assert_eq!(session.machine().expect("machine").reg(2), 0x3ff0);
    }

    #[test]
    fn an_elf_that_does_not_fit_the_board_is_rejected() {
        let executable = rvh_elf::Executable {
            entry: 0x8000,
            segments: vec![rvh_elf::Segment {
                addr: 0x8000,
                data: vec![0x13, 0x00, 0x00, 0x00],
                mem_size: 4,
                flags: 5,
            }],
        };
        let bytes = rvh_elf::write(&executable);
        assert!(
            rvh_sim::Machine::from_elf(&bytes).is_ok(),
            "the generic board has room at 0x8000"
        );
        let error = rvh_sim::Machine::from_elf_on(rvh_sim::board("tiny").expect("tiny"), &bytes)
            .expect_err("the tiny board does not");
        assert!(error.contains("tiny"), "{error}");
    }
}
