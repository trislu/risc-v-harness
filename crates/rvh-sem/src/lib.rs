//! Semantic resolution: syntax tree to a program of concrete statements.
//!
//! This layer is the only place that reads `(target, isa-db)`. It answers three
//! questions and nothing more:
//!
//! 1. is this mnemonic an instruction, and do its operands fit its operand list?
//! 2. if it is a pseudo-instruction, what concrete instructions does it become?
//! 3. which labels and constants exist?
//!
//! Addresses are *not* assigned here. `rvh-asm` lays statements out and resolves
//! branch targets and `la` pairs, because those need addresses.

use std::collections::HashMap;

use rvh_isa::{Format, Insn, OperandKind, Target};
use rvh_syntax::{Diagnostic, Element, Node, SyntaxKind, SyntaxTree};

/// CSR names the assembler accepts. The list is short on purpose: it covers what
/// the tutorial and a bare-metal trap handler need, and it is data, so it grows
/// without code changes.
pub const CSR_NAMES: &[(&str, i64)] = &[
    ("mstatus", 0x300),
    ("misa", 0x301),
    ("medeleg", 0x302),
    ("mideleg", 0x303),
    ("mie", 0x304),
    ("mtvec", 0x305),
    ("mscratch", 0x340),
    ("mepc", 0x341),
    ("mcause", 0x342),
    ("mtval", 0x343),
    ("mip", 0x344),
    ("mhartid", 0xf14),
    ("cycle", 0xc00),
    ("time", 0xc01),
    ("instret", 0xc02),
];

/// Pseudo-instructions the assembler expands. Kept as data so that a new pseudo
/// is added in one place, and so that "did you mean" can offer one.
pub const PSEUDOS: &[&str] = &[
    "beqz", "bge", "bgeu", "bgez", "bgt", "bgtu", "bgtz", "ble", "bleu", "blez", "blt", "bltu",
    "bltz", "bnez", "csrc", "csrr", "csrs", "csrw", "j", "jr", "la", "li", "mv", "neg", "nop",
    "not", "ret", "seqz", "snez",
];

/// Directives the assembler accepts; completion and did-you-mean share it.
pub const DIRECTIVES: &[&str] = &[
    ".2byte",
    ".4byte",
    ".8byte",
    ".align",
    ".asciz",
    ".asciiz",
    ".balign",
    ".bss",
    ".byte",
    ".data",
    ".dword",
    ".equ",
    ".file",
    ".global",
    ".globl",
    ".half",
    ".local",
    ".p2align",
    ".rodata",
    ".section",
    ".set",
    ".size",
    ".skip",
    ".space",
    ".string",
    ".text",
    ".type",
    ".weak",
    ".word",
    ".zero",
    ".extern",
    ".option",
    ".attribute",
];

/// One syntactic argument, before flattening.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arg {
    Reg(u8),
    Imm(i64),
    /// A label or constant.
    Sym(String),
    /// `offset(base)`, as in `8(sp)`. Flattens to a base register and an offset.
    Mem {
        base: u8,
        offset: i64,
    },
}

/// One operand value of a concrete instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Reg(u8),
    Imm(i64),
    /// A label or constant. Branches and `jal` keep it symbolic until layout.
    Sym(String),
}

/// One concrete statement of a section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    Label {
        name: String,
        span: (usize, usize),
    },
    Insn {
        insn: &'static Insn,
        ops: Vec<Op>,
        span: (usize, usize),
    },
    /// `la rd, sym`: two words, encoded together so the pair shares one base PC.
    La {
        rd: u8,
        sym: String,
        span: (usize, usize),
    },
    Data {
        bytes: Vec<u8>,
        span: (usize, usize),
    },
    /// Pad to `2^power` bytes.
    Align {
        power: u8,
        span: (usize, usize),
    },
    Zero {
        len: u32,
        span: (usize, usize),
    },
}

impl Stmt {
    #[must_use]
    pub fn span(&self) -> (usize, usize) {
        match self {
            Self::Label { span, .. }
            | Self::Insn { span, .. }
            | Self::La { span, .. }
            | Self::Data { span, .. }
            | Self::Align { span, .. }
            | Self::Zero { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramSection {
    pub name: String,
    pub stmts: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolKind {
    Label { section: usize },
    Constant(i64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolDef {
    pub name: String,
    pub kind: SymbolKind,
    pub span: (usize, usize),
}

/// A resolved program: sections of statements, plus labels and constants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub sections: Vec<ProgramSection>,
    pub symbols: Vec<SymbolDef>,
    pub diagnostics: Vec<Diagnostic>,
    /// The target in effect after the file's own `.attribute` / `.option arch`
    /// directives. The caller's target is the starting point, not the answer.
    pub target: Target,
}

impl Program {
    #[must_use]
    pub fn section(&self, name: &str) -> Option<&ProgramSection> {
        self.sections.iter().find(|section| section.name == name)
    }

    /// Value of a constant defined with `.equ` or `.set`.
    #[must_use]
    pub fn constant(&self, name: &str) -> Option<i64> {
        self.symbols.iter().find_map(|symbol| match symbol.kind {
            SymbolKind::Constant(value) if symbol.name == name => Some(value),
            _ => None,
        })
    }
}

/// Resolve a syntax tree against a target.
#[must_use]
pub fn resolve(tree: &SyntaxTree, target: &Target) -> Program {
    let mut ctx = Ctx {
        source: tree.source(),
        target: target.clone(),
        sections: vec![ProgramSection {
            name: "text".to_string(),
            stmts: Vec::new(),
        }],
        current: 0,
        symbols: Vec::new(),
        constants: HashMap::new(),
        numeric_counts: HashMap::new(),
        diagnostics: Vec::new(),
    };

    for child in &tree.root().children {
        let Element::Node(node) = child else { continue };
        if node.kind != SyntaxKind::Line {
            continue;
        }
        ctx.line(node);
    }
    ctx.resolve_numeric_labels();

    Program {
        sections: ctx.sections,
        symbols: ctx.symbols,
        diagnostics: ctx.diagnostics,
        target: ctx.target,
    }
}

struct Ctx<'a> {
    source: &'a str,
    target: Target,
    sections: Vec<ProgramSection>,
    current: usize,
    symbols: Vec<SymbolDef>,
    constants: HashMap<String, i64>,
    /// Each numeric label is renamed to `<number>#<count>` so that `1:` may
    /// appear many times.
    numeric_counts: HashMap<String, usize>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Ctx<'a> {
    fn line(&mut self, line: &Node) {
        for child in &line.children {
            let Element::Node(node) = child else { continue };
            match node.kind {
                SyntaxKind::Label => self.label(node),
                SyntaxKind::Instruction => self.instruction(node),
                SyntaxKind::Directive => self.directive(node),
                _ => {}
            }
        }
    }

    fn label(&mut self, node: &Node) {
        let raw = self
            .node_text(node)
            .trim()
            .trim_end_matches(':')
            .to_string();
        let name = if raw.chars().all(|character| character.is_ascii_digit()) {
            let counter = self.numeric_counts.entry(raw.clone()).or_insert(0);
            let unique = format!("{raw}#{counter}");
            *counter += 1;
            unique
        } else {
            raw
        };
        let span = node_range(node);
        if self.symbols.iter().any(|symbol| symbol.name == name) {
            self.diagnostics.push(Diagnostic::new(
                "sem.duplicate-label",
                format!("label `{name}` is already defined"),
                span,
            ));
            return;
        }
        self.symbols.push(SymbolDef {
            name: name.clone(),
            kind: SymbolKind::Label {
                section: self.current,
            },
            span,
        });
        self.push(Stmt::Label { name, span });
    }

    fn instruction(&mut self, node: &Node) {
        let span = node_range(node);
        let Some(mnemonic) = self.first_text(node) else {
            return;
        };
        let mnemonic = mnemonic.to_string();

        let head = node_head(node);
        let mut args = Vec::new();
        let mut arg_spans = Vec::new();
        for operand in operand_nodes(node) {
            let text = self.node_text(operand).trim();
            match classify(text) {
                Ok(arg) => {
                    args.push(arg);
                    arg_spans.push(node_range(operand));
                }
                Err(message) => {
                    self.diagnostics.push(Diagnostic::new(
                        "sem.bad-operand",
                        message,
                        node_range(operand),
                    ));
                    return;
                }
            }
        }
        // An atomic's address is `0(rs1)`: the memory operand names the base
        // register, and the offset must be zero because the encoding has no
        // immediate.
        if rvh_isa::lookups(&mnemonic).any(|insn| insn.ext == "a") {
            for (index, arg) in args.iter_mut().enumerate() {
                let Arg::Mem { base, offset } = *arg else {
                    continue;
                };
                if offset != 0 {
                    self.diagnostics.push(Diagnostic::new(
                        "sem.atomic-offset",
                        "an atomic operation addresses `(rs1)` only; adjust the register instead",
                        arg_spans.get(index).copied().unwrap_or(head),
                    ));
                    return;
                }
                *arg = Arg::Reg(base);
            }
        }
        let (ops, spans) = flatten_with_spans(&args, &arg_spans);

        // RV32E has 16 registers: x16..x31 are not encodable in a way the base
        // guarantees, so name the register instead of assembling it silently
        // (ADR-0015).
        let limit = self.target.registers();
        if let Some((index, span)) = ops
            .iter()
            .zip(spans.iter())
            .find_map(|(op, span)| match op {
                Op::Reg(register) if *register >= limit => Some((*register, *span)),
                _ => None,
            })
        {
            self.diagnostics.push(Diagnostic::new(
                "sem.register-unavailable",
                format!(
                    "`x{index}` is not available on `{}`, which has {limit} registers",
                    self.target.name()
                ),
                span,
            ));
            return;
        }

        // `fence` with no operands means "fence iorw, iorw".
        if mnemonic == "fence" && ops.is_empty() {
            let insn = rvh_isa::lookup("fence").expect("fence is in the database");
            self.push(Stmt::Insn {
                insn,
                ops: vec![Op::Imm(15), Op::Imm(15)],
                span,
            });
            return;
        }

        if let Some(insn) = rvh_isa::lookup(&mnemonic) {
            if let Some(stmt) = self.build_insn(insn, ops, &spans, head, span) {
                self.push(stmt);
            }
            return;
        }
        if !PSEUDOS.contains(&mnemonic.as_str()) {
            self.unknown_instruction(&mnemonic, node);
            return;
        }

        for stmt in self.expand(&mnemonic, ops, &spans, head, span) {
            self.push(stmt);
        }
    }

    fn unknown_instruction(&mut self, mnemonic: &str, node: &Node) {
        let mut candidates: Vec<&'static str> = rvh_isa::all()
            .iter()
            .filter(|insn| self.target.allows(insn))
            .map(|insn| insn.mnemonic)
            .collect();
        candidates.extend_from_slice(PSEUDOS);

        let suggestion = suggest(mnemonic, &candidates);
        let head = node_head(node);
        let mut diagnostic = Diagnostic::new(
            "sem.unknown-instruction",
            match suggestion {
                Some(suggestion) => {
                    format!("unknown instruction `{mnemonic}`; did you mean `{suggestion}`?")
                }
                None => format!("unknown instruction `{mnemonic}`"),
            },
            head,
        );
        if let Some(suggestion) = suggestion {
            diagnostic =
                diagnostic.with_fix(format!("replace with `{suggestion}`"), head, suggestion);
        }
        self.diagnostics.push(diagnostic);
    }

    fn build_insn(
        &mut self,
        insn: &'static Insn,
        ops: Vec<Op>,
        spans: &[(usize, usize)],
        head: (usize, usize),
        span: (usize, usize),
    ) -> Option<Stmt> {
        if !self.target.allows(insn) {
            // The fix is a one-line declaration at the top of the file, which is
            // exactly what the file would say if its author had remembered.
            self.diagnostics.push(
                Diagnostic::new(
                    "sem.extension-not-enabled",
                    format!(
                        "`{}` needs extension `{}`, which target `{}` does not include",
                        insn.mnemonic,
                        insn.ext,
                        self.target.name()
                    ),
                    head,
                )
                .with_fix(
                    format!("enable extension `{}` in this file", insn.ext),
                    (0, 0),
                    format!(".option arch, +{}\n", insn.ext),
                ),
            );
            return None;
        }
        if ops.len() != insn.operands.len() {
            self.diagnostics.push(Diagnostic::new(
                "sem.operand-count",
                format!(
                    "`{}` takes {} operand(s), found {}",
                    insn.mnemonic,
                    insn.operands.len(),
                    ops.len()
                ),
                head,
            ));
            return None;
        }

        for (index, (spec, op)) in insn.operands.iter().zip(ops.iter()).enumerate() {
            if let Err(message) = check_operand(insn, *spec, op, index) {
                let span = spans.get(index).copied().unwrap_or(span);
                self.diagnostics
                    .push(Diagnostic::new("sem.operand-mismatch", message, span));
                return None;
            }
        }
        Some(Stmt::Insn { insn, ops, span })
    }

    fn expand(
        &mut self,
        mnemonic: &str,
        ops: Vec<Op>,
        spans: &[(usize, usize)],
        head: (usize, usize),
        span: (usize, usize),
    ) -> Vec<Stmt> {
        let reg = |op: &Op| match op {
            Op::Reg(register) => Some(*register),
            _ => None,
        };
        let one = |stmt: Stmt| vec![stmt];

        match (mnemonic, ops.as_slice()) {
            ("nop", []) => one(self.insn("addi", vec![Op::Reg(0), Op::Reg(0), Op::Imm(0)], span)),
            ("mv", [rd, rs]) if reg(rd).is_some() && reg(rs).is_some() => one(self.insn(
                "addi",
                vec![
                    Op::Reg(reg(rd).unwrap()),
                    Op::Reg(reg(rs).unwrap()),
                    Op::Imm(0),
                ],
                span,
            )),
            ("not", [rd, rs]) if reg(rd).is_some() && reg(rs).is_some() => one(self.insn(
                "xori",
                vec![
                    Op::Reg(reg(rd).unwrap()),
                    Op::Reg(reg(rs).unwrap()),
                    Op::Imm(-1),
                ],
                span,
            )),
            ("neg", [rd, rs]) if reg(rd).is_some() && reg(rs).is_some() => one(self.insn(
                "sub",
                vec![
                    Op::Reg(reg(rd).unwrap()),
                    Op::Reg(0),
                    Op::Reg(reg(rs).unwrap()),
                ],
                span,
            )),
            ("seqz", [rd, rs]) if reg(rd).is_some() && reg(rs).is_some() => one(self.insn(
                "sltiu",
                vec![
                    Op::Reg(reg(rd).unwrap()),
                    Op::Reg(reg(rs).unwrap()),
                    Op::Imm(1),
                ],
                span,
            )),
            ("snez", [rd, rs]) if reg(rd).is_some() && reg(rs).is_some() => one(self.insn(
                "sltu",
                vec![
                    Op::Reg(reg(rd).unwrap()),
                    Op::Reg(0),
                    Op::Reg(reg(rs).unwrap()),
                ],
                span,
            )),
            ("j", [Op::Sym(target)]) => {
                one(self.insn("jal", vec![Op::Reg(0), Op::Sym(target.clone())], span))
            }
            ("jr", [rs]) if reg(rs).is_some() => one(self.insn(
                "jalr",
                vec![Op::Reg(0), Op::Reg(reg(rs).unwrap()), Op::Imm(0)],
                span,
            )),
            ("ret", []) => one(self.insn("jalr", vec![Op::Reg(0), Op::Reg(1), Op::Imm(0)], span)),
            ("beqz", [rs, Op::Sym(target)]) if reg(rs).is_some() => one(self.insn(
                "beq",
                vec![
                    Op::Reg(reg(rs).unwrap()),
                    Op::Reg(0),
                    Op::Sym(target.clone()),
                ],
                span,
            )),
            ("bnez", [rs, Op::Sym(target)]) if reg(rs).is_some() => one(self.insn(
                "bne",
                vec![
                    Op::Reg(reg(rs).unwrap()),
                    Op::Reg(0),
                    Op::Sym(target.clone()),
                ],
                span,
            )),
            ("blez", [rs, Op::Sym(target)]) if reg(rs).is_some() => one(self.insn(
                "bge",
                vec![
                    Op::Reg(0),
                    Op::Reg(reg(rs).unwrap()),
                    Op::Sym(target.clone()),
                ],
                span,
            )),
            ("bgez", [rs, Op::Sym(target)]) if reg(rs).is_some() => one(self.insn(
                "bge",
                vec![
                    Op::Reg(reg(rs).unwrap()),
                    Op::Reg(0),
                    Op::Sym(target.clone()),
                ],
                span,
            )),
            ("bltz", [rs, Op::Sym(target)]) if reg(rs).is_some() => one(self.insn(
                "blt",
                vec![
                    Op::Reg(reg(rs).unwrap()),
                    Op::Reg(0),
                    Op::Sym(target.clone()),
                ],
                span,
            )),
            ("bgtz", [rs, Op::Sym(target)]) if reg(rs).is_some() => one(self.insn(
                "blt",
                vec![
                    Op::Reg(0),
                    Op::Reg(reg(rs).unwrap()),
                    Op::Sym(target.clone()),
                ],
                span,
            )),
            ("ble" | "bge" | "bleu" | "bgeu", [a, b, Op::Sym(target)])
                if reg(a).is_some() && reg(b).is_some() =>
            {
                let (a, b) = (reg(a).unwrap(), reg(b).unwrap());
                let (name, left, right) = match mnemonic {
                    "ble" => ("bge", b, a),
                    "bleu" => ("bgeu", b, a),
                    other => (other, a, b),
                };
                one(self.insn(
                    name,
                    vec![Op::Reg(left), Op::Reg(right), Op::Sym(target.clone())],
                    span,
                ))
            }
            ("bgt" | "blt" | "bgtu" | "bltu", [a, b, Op::Sym(target)])
                if reg(a).is_some() && reg(b).is_some() =>
            {
                let (a, b) = (reg(a).unwrap(), reg(b).unwrap());
                let (name, left, right) = match mnemonic {
                    "bgt" => ("blt", b, a),
                    "bgtu" => ("bltu", b, a),
                    other => (other, a, b),
                };
                one(self.insn(
                    name,
                    vec![Op::Reg(left), Op::Reg(right), Op::Sym(target.clone())],
                    span,
                ))
            }
            ("li", [rd, value]) if reg(rd).is_some() => {
                let register = reg(rd).unwrap();
                let value = match value {
                    Op::Imm(value) => *value,
                    Op::Sym(name) => match self.constants.get(name) {
                        Some(value) => *value,
                        None => {
                            self.diagnostics.push(Diagnostic::new(
                                "sem.unknown-symbol",
                                format!("`{name}` is not a known constant"),
                                span,
                            ));
                            return Vec::new();
                        }
                    },
                    Op::Reg(_) => return Vec::new(),
                };
                if !(-2_147_483_648..=2_147_483_647).contains(&value) {
                    self.diagnostics.push(Diagnostic::new(
                        "sem.immediate-range",
                        format!("`li` cannot load {value}"),
                        span,
                    ));
                    return Vec::new();
                }
                if (-2048..=2047).contains(&value) {
                    return one(self.insn(
                        "addi",
                        vec![Op::Reg(register), Op::Reg(0), Op::Imm(value)],
                        span,
                    ));
                }
                let hi = (value + 0x800) >> 12;
                let lo = value - (hi << 12);
                let mut out = vec![self.insn("lui", vec![Op::Reg(register), Op::Imm(hi)], span)];
                if lo != 0 {
                    out.push(self.insn(
                        "addi",
                        vec![Op::Reg(register), Op::Reg(register), Op::Imm(lo)],
                        span,
                    ));
                }
                out
            }
            ("la", [rd, Op::Sym(sym)]) if reg(rd).is_some() => one(Stmt::La {
                rd: reg(rd).unwrap(),
                sym: sym.clone(),
                span,
            }),
            ("csrr", [rd, csr]) if reg(rd).is_some() => {
                let csr_span = spans.get(1).copied().unwrap_or(span);
                let Some(value) = self.csr(csr, csr_span) else {
                    return Vec::new();
                };
                one(self.insn(
                    "csrrs",
                    vec![Op::Reg(reg(rd).unwrap()), Op::Imm(value), Op::Reg(0)],
                    span,
                ))
            }
            ("csrw" | "csrs" | "csrc", [csr, rs]) if reg(rs).is_some() => {
                let csr_span = spans.first().copied().unwrap_or(span);
                let Some(value) = self.csr(csr, csr_span) else {
                    return Vec::new();
                };
                let name = match mnemonic {
                    "csrw" => "csrrw",
                    "csrs" => "csrrs",
                    _ => "csrrc",
                };
                one(self.insn(
                    name,
                    vec![Op::Reg(0), Op::Imm(value), Op::Reg(reg(rs).unwrap())],
                    span,
                ))
            }
            _ => {
                self.diagnostics.push(Diagnostic::new(
                    "sem.operand-mismatch",
                    format!("`{mnemonic}` does not take these operands"),
                    head,
                ));
                Vec::new()
            }
        }
    }

    /// `.attribute arch, "rv32i2p1_m2p0"`: the file declares its target.
    fn attribute(&mut self, args: &[&str], span: (usize, usize)) {
        if args.first().map(|name| name.trim()) != Some("arch") {
            return;
        }
        let Some(value) = args.get(1) else {
            return;
        };
        let text = value.trim().trim_matches('"');
        match arch_name(text) {
            Some(name) => self.adopt(&name, text, span),
            None => self.bad_arch(text, span),
        }
    }

    /// `.option arch, +m`, `.option arch, -m`, `.option arch, rv32im`.
    ///
    /// Applies from this line on, which is what GNU `as` does.
    fn option(&mut self, args: &[&str], span: (usize, usize)) {
        if args.first().map(|name| name.trim()) != Some("arch") {
            return;
        }
        let Some(spec) = args.get(1) else {
            return;
        };
        let spec = spec.trim();
        if let Some(added) = spec.strip_prefix('+') {
            self.with_extensions(|extensions| {
                if !extensions.iter().any(|extension| extension == added) {
                    extensions.push(added.to_string());
                }
            });
        } else if let Some(removed) = spec.strip_prefix('-') {
            self.with_extensions(|extensions| extensions.retain(|extension| extension != removed));
        } else if spec.starts_with("rv") {
            match arch_name(spec) {
                Some(name) => self.adopt(&name, spec, span),
                None => self.bad_arch(spec, span),
            }
        }
    }

    fn adopt(&mut self, name: &str, text: &str, span: (usize, usize)) {
        match Target::parse(name) {
            Ok(target) => self.target = target,
            Err(_) => self.bad_arch(text, span),
        }
    }

    fn with_extensions(&mut self, change: impl FnOnce(&mut Vec<String>)) {
        let base = self.target.base().to_string();
        let mut extensions: Vec<String> = self
            .target
            .extensions()
            .iter()
            .filter(|extension| !matches!(**extension, "i" | "e"))
            .map(|extension| (*extension).to_string())
            .collect();
        change(&mut extensions);
        let mut name = base;
        for extension in extensions {
            name.push('_');
            name.push_str(&extension);
        }
        if let Ok(target) = Target::parse(&name) {
            self.target = target;
        }
    }

    fn bad_arch(&mut self, text: &str, span: (usize, usize)) {
        self.diagnostics.push(Diagnostic::new(
            "sem.bad-target",
            format!("`{text}` is not a target this database knows"),
            span,
        ));
    }

    fn csr(&mut self, op: &Op, span: (usize, usize)) -> Option<i64> {
        let value = match op {
            Op::Imm(value) => *value,
            Op::Sym(name) => match CSR_NAMES.iter().find(|(csr, _)| csr == name) {
                Some((_, value)) => *value,
                None => {
                    let names: Vec<&str> = CSR_NAMES.iter().map(|(name, _)| *name).collect();
                    let message = match suggest(name, &names) {
                        Some(suggestion) => {
                            format!("unknown CSR `{name}`; did you mean `{suggestion}`?")
                        }
                        None => format!("unknown CSR `{name}`"),
                    };
                    self.diagnostics
                        .push(Diagnostic::new("sem.unknown-csr", message, span));
                    return None;
                }
            },
            Op::Reg(_) => return None,
        };
        if !(0..=0xfff).contains(&value) {
            self.diagnostics.push(Diagnostic::new(
                "sem.immediate-range",
                format!("CSR number {value} does not fit in 12 bits"),
                span,
            ));
            return None;
        }
        Some(value)
    }

    fn insn(&mut self, mnemonic: &str, ops: Vec<Op>, span: (usize, usize)) -> Stmt {
        let insn =
            rvh_isa::lookup(mnemonic).unwrap_or_else(|| panic!("missing instruction `{mnemonic}`"));
        Stmt::Insn { insn, ops, span }
    }

    fn directive(&mut self, node: &Node) {
        let span = node_range(node);
        let head = node_head(node);
        let Some(name) = self.first_text(node) else {
            return;
        };
        let name = name.to_string();
        let operands = operand_nodes(node);
        let arg_spans: Vec<(usize, usize)> = operands.iter().map(|node| node_range(node)).collect();
        let args: Vec<&'a str> = operands
            .into_iter()
            .map(|operand| self.node_text(operand).trim())
            .collect();

        match name.as_str() {
            ".text" => self.switch_section("text"),
            ".data" => self.switch_section("data"),
            ".rodata" => self.switch_section("rodata"),
            ".bss" => self.switch_section("bss"),
            ".section" => {
                let section = args.first().copied().unwrap_or(".text");
                let section = section.trim_start_matches('.');
                self.switch_section(section);
            }
            ".globl" | ".global" | ".local" | ".weak" | ".type" | ".size" | ".file" | ".extern" => {
            }
            ".attribute" => self.attribute(&args, head),
            ".option" => self.option(&args, head),
            ".align" | ".p2align" | ".balign" => {
                let power = args.first().and_then(|text| parse_int(text)).unwrap_or(0);
                if (0..=16).contains(&power) {
                    self.push(Stmt::Align {
                        power: power as u8,
                        span,
                    });
                } else {
                    self.diagnostics.push(Diagnostic::new(
                        "sem.bad-directive",
                        format!("`.align` takes 0..=16, found {power}"),
                        arg_spans.first().copied().unwrap_or(head),
                    ));
                }
            }
            ".byte" | ".half" | ".2byte" | ".word" | ".4byte" | ".dword" | ".8byte" => {
                let width = match name.as_str() {
                    ".byte" => 1,
                    ".half" | ".2byte" => 2,
                    ".word" | ".4byte" => 4,
                    _ => 8,
                };
                let mut bytes = Vec::new();
                for text in &args {
                    match parse_int(text) {
                        Some(value) => bytes.extend_from_slice(&value.to_le_bytes()[..width]),
                        None => {
                            self.diagnostics.push(Diagnostic::new(
                                "sem.bad-directive",
                                format!("`{name}` needs integer arguments, found `{text}`"),
                                span,
                            ));
                            return;
                        }
                    }
                }
                self.push(Stmt::Data { bytes, span });
            }
            ".string" | ".asciz" | ".asciiz" => {
                let mut bytes = Vec::new();
                for text in &args {
                    bytes.extend_from_slice(&unescape(text));
                    bytes.push(0);
                }
                self.push(Stmt::Data { bytes, span });
            }
            ".zero" | ".space" | ".skip" => {
                let len = args.first().and_then(|text| parse_int(text)).unwrap_or(0);
                if len < 0 {
                    self.diagnostics.push(Diagnostic::new(
                        "sem.bad-directive",
                        format!("`{name}` needs a non-negative length"),
                        span,
                    ));
                } else {
                    self.push(Stmt::Zero {
                        len: len as u32,
                        span,
                    });
                }
            }
            ".equ" | ".set" => {
                let symbol = args.first().map(|text| (*text).to_string());
                let value = args.get(1).and_then(|text| {
                    parse_int(text).or_else(|| self.constants.get(*text).copied())
                });
                let (Some(symbol), Some(value)) = (symbol, value) else {
                    self.diagnostics.push(Diagnostic::new(
                        "sem.bad-directive",
                        format!("`{name}` needs a name and an integer value"),
                        span,
                    ));
                    return;
                };
                if self.symbols.iter().any(|existing| existing.name == symbol) {
                    self.diagnostics.push(Diagnostic::new(
                        "sem.duplicate-label",
                        format!("`{symbol}` is already defined"),
                        span,
                    ));
                    return;
                }
                self.constants.insert(symbol.clone(), value);
                self.symbols.push(SymbolDef {
                    name: symbol,
                    kind: SymbolKind::Constant(value),
                    span,
                });
            }
            _ => {
                let message = match suggest(&name, DIRECTIVES) {
                    Some(suggestion) => {
                        format!("unknown directive `{name}`; did you mean `{suggestion}`?")
                    }
                    None => format!("unknown directive `{name}`"),
                };
                self.diagnostics
                    .push(Diagnostic::new("sem.unknown-directive", message, head));
            }
        }
    }

    fn switch_section(&mut self, name: &str) {
        if let Some(index) = self
            .sections
            .iter()
            .position(|section| section.name == name)
        {
            self.current = index;
            return;
        }
        self.sections.push(ProgramSection {
            name: name.to_string(),
            stmts: Vec::new(),
        });
        self.current = self.sections.len() - 1;
    }

    fn push(&mut self, stmt: Stmt) {
        if let Some(section) = self.sections.get_mut(self.current) {
            section.stmts.push(stmt);
        }
    }

    fn first_text(&self, node: &Node) -> Option<&'a str> {
        let mut span = None;
        first_token_span(node, &mut span);
        span.map(|(start, end)| &self.source[start..end])
    }

    fn node_text(&self, node: &Node) -> &'a str {
        let (start, end) = node_range(node);
        &self.source[start..end]
    }

    /// Resolve `1b` and `1f` against the numeric labels of each section.
    ///
    /// Labels were renamed to `1#0`, `1#1`, ... when they were defined, so a
    /// reference becomes a plain symbol lookup once the section is complete.
    fn resolve_numeric_labels(&mut self) {
        let mut unresolved: Vec<(String, bool, (usize, usize))> = Vec::new();

        for section in &mut self.sections {
            let defined: Vec<(usize, String, String)> = section
                .stmts
                .iter()
                .enumerate()
                .filter_map(|(index, stmt)| match stmt {
                    Stmt::Label { name, .. } => numeric_label_number(name)
                        .map(|number| (index, number.to_string(), name.clone())),
                    _ => None,
                })
                .collect();

            for (index, stmt) in section.stmts.iter_mut().enumerate() {
                let Some((number, forward)) = statement_numeric_reference(stmt) else {
                    continue;
                };
                let target = if forward {
                    defined
                        .iter()
                        .find(|(position, label, _)| *position > index && *label == number)
                } else {
                    defined
                        .iter()
                        .rev()
                        .find(|(position, label, _)| *position < index && *label == number)
                };
                match target {
                    Some((_, _, unique)) => set_statement_symbol(stmt, unique.clone()),
                    None => unresolved.push((number, forward, stmt.span())),
                }
            }
        }

        for (number, forward, span) in unresolved {
            let suffix = if forward { 'f' } else { 'b' };
            self.diagnostics.push(Diagnostic::new(
                "sem.unknown-symbol",
                format!("numeric label `{number}{suffix}` has no matching definition"),
                span,
            ));
        }
    }
}

fn first_token_span(node: &Node, found: &mut Option<(usize, usize)>) {
    if found.is_some() {
        return;
    }
    for child in &node.children {
        match child {
            Element::Node(node) => first_token_span(node, found),
            Element::Token(token) => {
                *found = Some(token.span);
                return;
            }
        }
    }
}

/// Span of the first token, which is the mnemonic or directive name.
fn node_head(node: &Node) -> (usize, usize) {
    let mut span = None;
    first_token_span(node, &mut span);
    span.unwrap_or_else(|| node_range(node))
}

fn node_range(node: &Node) -> (usize, usize) {
    let mut start = usize::MAX;
    let mut end = 0;
    fn walk(node: &Node, start: &mut usize, end: &mut usize) {
        for child in &node.children {
            match child {
                Element::Node(node) => walk(node, start, end),
                Element::Token(token) => {
                    *start = (*start).min(token.span.0);
                    *end = (*end).max(token.span.1);
                }
            }
        }
    }
    walk(node, &mut start, &mut end);
    if start == usize::MAX {
        (0, 0)
    } else {
        (start, end)
    }
}

fn operand_nodes(node: &Node) -> Vec<&Node> {
    node.children
        .iter()
        .filter_map(|child| match child {
            Element::Node(node) if node.kind == SyntaxKind::Operand => Some(node),
            _ => None,
        })
        .collect()
}

/// `1#3` -> `1`; ordinary labels return `None`.
fn numeric_label_number(name: &str) -> Option<&str> {
    let (number, _) = name.split_once('#')?;
    if number.chars().all(|character| character.is_ascii_digit()) {
        Some(number)
    } else {
        None
    }
}

/// The first symbolic operand of a statement, if it is a numeric reference.
fn statement_numeric_reference(stmt: &Stmt) -> Option<(String, bool)> {
    let symbol = match stmt {
        Stmt::Insn { ops, .. } => ops.iter().find_map(|op| match op {
            Op::Sym(name) => Some(name.as_str()),
            _ => None,
        })?,
        Stmt::La { sym, .. } => sym.as_str(),
        _ => return None,
    };
    let (digits, suffix) = symbol.split_at(symbol.len().checked_sub(1)?);
    if digits.is_empty() || !digits.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    match suffix {
        "b" => Some((digits.to_string(), false)),
        "f" => Some((digits.to_string(), true)),
        _ => None,
    }
}

fn set_statement_symbol(stmt: &mut Stmt, target: String) {
    match stmt {
        Stmt::Insn { ops, .. } => {
            for op in ops.iter_mut() {
                if matches!(op, Op::Sym(_)) {
                    *op = Op::Sym(target.clone());
                    break;
                }
            }
        }
        Stmt::La { sym, .. } => *sym = target,
        _ => {}
    }
}

/// Parse a register name, including the ABI names.
#[must_use]
pub fn parse_register(text: &str) -> Option<u8> {
    let named = match text {
        "zero" => Some(0),
        "ra" => Some(1),
        "sp" => Some(2),
        "gp" => Some(3),
        "tp" => Some(4),
        "fp" => Some(8),
        _ => None,
    };
    if named.is_some() {
        return named;
    }
    if let Some(number) = text.strip_prefix('x') {
        return number.parse::<u8>().ok().filter(|value| *value < 32);
    }
    if let Some(rest) = text.strip_prefix('t') {
        return match rest {
            "0" => Some(5),
            "1" => Some(6),
            "2" => Some(7),
            "3" => Some(28),
            "4" => Some(29),
            "5" => Some(30),
            "6" => Some(31),
            _ => None,
        };
    }
    if let Some(rest) = text.strip_prefix('s') {
        return match rest.parse::<u8>() {
            Ok(0) => Some(8),
            Ok(value @ 1..=11) => Some(8 + value),
            _ => None,
        };
    }
    if let Some(rest) = text.strip_prefix('a') {
        return match rest.parse::<u8>() {
            Ok(value @ 0..=7) => Some(10 + value),
            _ => None,
        };
    }
    None
}

/// Classify one syntactic argument.
pub fn classify(text: &str) -> Result<Arg, String> {
    if let Some(register) = parse_register(text) {
        return Ok(Arg::Reg(register));
    }
    if let Some(value) = parse_int(text) {
        return Ok(Arg::Imm(value));
    }
    if text.ends_with(')') {
        if let Some(open) = text.find('(') {
            let offset = text[..open].trim();
            let base = &text[open + 1..text.len() - 1];
            let offset = if offset.is_empty() {
                0
            } else {
                parse_int(offset).ok_or_else(|| format!("bad offset in `{text}`"))?
            };
            let base =
                parse_register(base).ok_or_else(|| format!("bad base register in `{text}`"))?;
            return Ok(Arg::Mem { base, offset });
        }
    }
    if text.starts_with('%') {
        return Err(format!("relocation `{text}` is not supported yet"));
    }
    // `1b` and `1f` are numeric label references, not numbers.
    if is_numeric_reference(text) {
        return Ok(Arg::Sym(text.to_string()));
    }
    if text.is_empty()
        || !text
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '.')
    {
        return Err(format!("cannot parse operand `{text}`"));
    }
    Ok(Arg::Sym(text.to_string()))
}

/// A memory argument contributes two operands: the base register, then the offset.
/// Canonical `base_extension` string from a versioned architecture string.
///
/// `rv32i2p1_m2p0_zicsr2p0` becomes `rv32i_m_zicsr`: versions are dropped because
/// this database does not carry them yet (ADR-0003).
fn arch_name(text: &str) -> Option<String> {
    let mut parts = text.trim().split('_');
    let base = parts.next()?;
    let letters: String = base
        .chars()
        .take_while(|character| character.is_ascii_alphabetic())
        .collect();
    let digits: String = base[letters.len()..]
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect();
    let suffix: String = base[letters.len() + digits.len()..]
        .chars()
        .take_while(|character| character.is_ascii_alphabetic())
        .collect();
    if letters != "rv" || digits.is_empty() || suffix.is_empty() {
        return None;
    }
    let mut name = format!("rv{digits}{suffix}");
    for part in parts {
        let extension: String = part
            .chars()
            .take_while(|character| character.is_ascii_alphabetic())
            .collect();
        if extension.is_empty() {
            return None;
        }
        name.push('_');
        name.push_str(&extension);
    }
    Some(name)
}

/// Flatten args to ops, carrying the operand span onto every op it produces.
fn flatten_with_spans(args: &[Arg], spans: &[(usize, usize)]) -> (Vec<Op>, Vec<(usize, usize)>) {
    let (mut ops, mut op_spans) = (Vec::new(), Vec::new());
    for (index, arg) in args.iter().enumerate() {
        let span = spans.get(index).copied().unwrap_or((0, 0));
        let mut push = |op: Op| {
            ops.push(op);
            op_spans.push(span);
        };
        match arg {
            Arg::Reg(register) => push(Op::Reg(*register)),
            Arg::Imm(value) => push(Op::Imm(*value)),
            Arg::Sym(name) => push(Op::Sym(name.clone())),
            Arg::Mem { base, offset } => {
                push(Op::Reg(*base));
                push(Op::Imm(*offset));
            }
        }
    }
    (ops, op_spans)
}

/// `1b` / `2f`: a reference to a numeric local label.
fn is_numeric_reference(text: &str) -> bool {
    let Some((digits, suffix)) = text.split_at_checked(text.len().saturating_sub(1)) else {
        return false;
    };
    !digits.is_empty()
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && matches!(suffix, "b" | "f")
}

/// Nearest candidate within a small edit distance. `None` when nothing is close.
fn suggest<'c>(name: &str, candidates: &[&'c str]) -> Option<&'c str> {
    let limit = if name.chars().count() <= 3 { 1 } else { 2 };
    let mut best: Option<(usize, &'c str)> = None;
    for candidate in candidates {
        if *candidate == name {
            continue;
        }
        let distance = levenshtein(name, candidate);
        if distance <= limit && best.is_none_or(|(best_distance, _)| distance < best_distance) {
            best = Some((distance, candidate));
        }
    }
    best.map(|(_, candidate)| candidate)
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0usize; b.len() + 1];
    for (i, left) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, right) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(left != right);
            current[j + 1] = (previous[j + 1] + 1).min(current[j] + 1).min(substitution);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

/// A concrete next step when an immediate does not fit.
fn immediate_hint(insn: &Insn) -> String {
    let register_form = match insn.mnemonic {
        "addi" => Some("add"),
        "slti" => Some("slt"),
        "sltiu" => Some("sltu"),
        "xori" => Some("xor"),
        "ori" => Some("or"),
        "andi" => Some("and"),
        _ => None,
    };
    match (insn.class, register_form) {
        ("alu-imm", Some(form)) => {
            format!("; load the value with `li` into a temporary, then use `{form}`")
        }
        ("shift-imm", _) => "; the shift amount must be 0..=31".to_string(),
        ("load" | "store", _) => "; the offset must fit in 12 signed bits".to_string(),
        _ => String::new(),
    }
}

fn parse_int(text: &str) -> Option<i64> {
    rvh_syntax::parse_number(text)
}

fn check_operand(insn: &Insn, spec: rvh_isa::Operand, op: &Op, index: usize) -> Result<(), String> {
    match spec.kind {
        OperandKind::Rd | OperandKind::Rs1 | OperandKind::Rs2 => match op {
            Op::Reg(_) => Ok(()),
            other => Err(format!(
                "operand {} of `{}` must be a register, found {}",
                index + 1,
                insn.mnemonic,
                describe(other)
            )),
        },
        OperandKind::Csr => match op {
            Op::Imm(value) if (0..=0xfff).contains(value) => Ok(()),
            Op::Imm(value) => Err(format!("CSR {value} does not fit in 12 bits")),
            other => Err(format!("expected a CSR, found {}", describe(other))),
        },
        OperandKind::Shamt | OperandKind::Zimm => match op {
            Op::Imm(value) if (0..=31).contains(value) => Ok(()),
            Op::Imm(value) => Err(format!(
                "`{}` needs a 5-bit value, found {value}",
                insn.mnemonic
            )),
            other => Err(format!("expected an immediate, found {}", describe(other))),
        },
        OperandKind::Pred | OperandKind::Succ => match op {
            Op::Imm(value) if (0..=15).contains(value) => Ok(()),
            _ => Err(format!("`{}` needs a 4-bit mask here", insn.mnemonic)),
        },
        OperandKind::Imm => {
            let branch = matches!(insn.format, Format::B | Format::J | Format::Cb | Format::Cj);
            match op {
                Op::Sym(_) if branch => Ok(()),
                Op::Sym(name) => Err(format!("`{name}` cannot be used as an immediate here")),
                Op::Reg(_) => Err(format!("expected an immediate, found {}", describe(op))),
                Op::Imm(value) => {
                    let (low, high) = spec.range();
                    if *value < low || *value > high {
                        return Err(format!(
                            "immediate {value} is out of range [{low}, {high}] for `{}`{}",
                            insn.mnemonic,
                            immediate_hint(insn)
                        ));
                    }
                    if spec.align > 1 && value % i64::from(spec.align) != 0 {
                        return Err(format!(
                            "immediate {value} is not a multiple of {} for `{}`",
                            spec.align, insn.mnemonic
                        ));
                    }
                    Ok(())
                }
            }
        }
    }
}

fn describe(op: &Op) -> String {
    match op {
        Op::Reg(register) => format!("register x{register}"),
        Op::Imm(value) => format!("immediate {value}"),
        Op::Sym(name) => format!("symbol `{name}`"),
    }
}

fn unescape(text: &str) -> Vec<u8> {
    let inner = text
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(text);
    let mut out = Vec::new();
    let mut chars = inner.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            let mut buffer = [0u8; 4];
            out.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
            continue;
        }
        match chars.next() {
            Some('n') => out.push(b'\n'),
            Some('t') => out.push(b'\t'),
            Some('r') => out.push(b'\r'),
            Some('0') => out.push(0),
            Some('"') => out.push(b'"'),
            Some('\\') => out.push(b'\\'),
            Some(other) => out.push(other as u8),
            None => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> Target {
        Target::parse("rv32i_zicsr").expect("rv32i_zicsr")
    }

    fn resolve_source(source: &str) -> Program {
        let tree = rvh_syntax::parse(source);
        resolve(&tree, &target())
    }

    #[test]
    fn a_missing_extension_offers_a_fix_that_declares_it() {
        let tree = rvh_syntax::parse("_start:\n    mul a0, a1, a2\n");
        let program = resolve(&tree, &Target::parse("rv32i_zicsr").expect("target"));
        let diagnostic = &program.diagnostics[0];
        assert_eq!(diagnostic.code, "sem.extension-not-enabled");
        let fix = &diagnostic.fixes()[0];
        assert_eq!(fix.span, (0, 0));
        assert_eq!(fix.replacement, ".option arch, +m\n");
        assert!(fix.title.contains("m"), "{}", fix.title);
    }

    #[test]
    fn the_embedded_base_refuses_high_registers() {
        let tree = rvh_syntax::parse("_start:\n    add x16, x0, x0\n    add a0, a1, a2\n");
        let program = resolve(&tree, &Target::parse("rv32e").expect("target"));
        let codes: Vec<&str> = program
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect();
        assert_eq!(
            codes,
            vec!["sem.register-unavailable"],
            "{:?}",
            program.diagnostics
        );
        assert!(program.diagnostics[0].message.contains("16 registers"));
        assert!(
            program.diagnostics[0].message.contains("rv32e"),
            "{}",
            program.diagnostics[0].message
        );
    }

    #[test]
    fn diagnostics_point_at_the_offending_token() {
        let source = "addi a0, a1, 4096\ncsrr a0, mstat\nadii a0, a1, 1\n.wor 1\n.align 99\n";
        let program = resolve_source(source);
        let spans: Vec<(&str, &str)> = program
            .diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.code,
                    &source[diagnostic.span.0..diagnostic.span.1],
                )
            })
            .collect();
        assert_eq!(
            spans,
            vec![
                ("sem.operand-mismatch", "4096"),
                ("sem.unknown-csr", "mstat"),
                ("sem.unknown-instruction", "adii"),
                ("sem.unknown-directive", ".wor"),
                ("sem.bad-directive", "99"),
            ]
        );
    }

    #[test]
    fn resolves_instructions_and_labels() {
        let program = resolve_source("start:\n  addi a0, a0, 1\n  ret\n");
        assert!(program.diagnostics.is_empty(), "{:?}", program.diagnostics);
        let text = program.section("text").expect("text section");
        assert_eq!(text.stmts.len(), 3);
        assert!(matches!(text.stmts[0], Stmt::Label { .. }));
        assert!(program.symbols.iter().any(|symbol| symbol.name == "start"));
    }

    #[test]
    fn li_expands_to_one_or_two_words() {
        let program = resolve_source("li a0, 10\nli a1, 0x12345\n");
        assert!(program.diagnostics.is_empty(), "{:?}", program.diagnostics);
        let text = program.section("text").expect("text");
        assert_eq!(text.stmts.len(), 3);
        assert!(matches!(&text.stmts[0], Stmt::Insn { insn, .. } if insn.mnemonic == "addi"));
        assert!(matches!(&text.stmts[1], Stmt::Insn { insn, .. } if insn.mnemonic == "lui"));
        assert!(matches!(&text.stmts[2], Stmt::Insn { insn, .. } if insn.mnemonic == "addi"));
    }

    #[test]
    fn csr_names_resolve() {
        let program = resolve_source("csrw mtvec, t0\ncsrr t1, mstatus\n");
        assert!(program.diagnostics.is_empty(), "{:?}", program.diagnostics);
        let text = program.section("text").expect("text");
        let Stmt::Insn { ops, .. } = &text.stmts[0] else {
            panic!("expected instruction")
        };
        assert_eq!(ops[1], Op::Imm(0x305));
    }

    #[test]
    fn memory_operand_flattens_to_base_and_offset() {
        let program = resolve_source("lw a0, 8(sp)\n");
        assert!(program.diagnostics.is_empty(), "{:?}", program.diagnostics);
        let text = program.section("text").expect("text");
        let Stmt::Insn { ops, .. } = &text.stmts[0] else {
            panic!("expected instruction")
        };
        assert_eq!(ops, &[Op::Reg(10), Op::Reg(2), Op::Imm(8)]);
    }

    #[test]
    fn unknown_instruction_is_reported() {
        let program = resolve_source("frobnicate a0\n");
        assert_eq!(program.diagnostics.len(), 1);
        assert_eq!(program.diagnostics[0].code, "sem.unknown-instruction");
    }

    #[test]
    fn wrong_operand_count_is_reported() {
        let program = resolve_source("addi a0, a1\n");
        assert_eq!(program.diagnostics[0].code, "sem.operand-count");
    }

    #[test]
    fn immediate_range_is_checked() {
        let program = resolve_source("addi a0, a1, 4096\n");
        assert_eq!(program.diagnostics[0].code, "sem.operand-mismatch");
        assert!(program.diagnostics[0].message.contains("out of range"));
    }

    #[test]
    fn extension_must_be_enabled() {
        let tree = rvh_syntax::parse("csrrw a0, mstatus, a1\n");
        let program = resolve(&tree, &Target::rv32i());
        assert_eq!(program.diagnostics[0].code, "sem.extension-not-enabled");
    }

    #[test]
    fn data_directives_emit_bytes() {
        let program = resolve_source(".data\n.word 0x12345678\n.byte 1, 2\n.string \"hi\"\n");
        assert!(program.diagnostics.is_empty(), "{:?}", program.diagnostics);
        let data = program.section("data").expect("data section");
        let Stmt::Data { bytes, .. } = &data.stmts[0] else {
            panic!("expected data")
        };
        assert_eq!(bytes, &[0x78, 0x56, 0x34, 0x12]);
        let Stmt::Data { bytes, .. } = &data.stmts[2] else {
            panic!("expected data")
        };
        assert_eq!(bytes, b"hi\0");
    }

    #[test]
    fn equ_defines_a_constant() {
        let program = resolve_source(".equ SIZE, 4\nli a0, SIZE\n");
        assert!(program.diagnostics.is_empty(), "{:?}", program.diagnostics);
        assert_eq!(program.constant("SIZE"), Some(4));
    }

    #[test]
    fn classification_covers_every_argument_shape() {
        assert_eq!(classify("8(sp)"), Ok(Arg::Mem { base: 2, offset: 8 }));
        assert_eq!(classify("a0"), Ok(Arg::Reg(10)));
        assert_eq!(classify("loop"), Ok(Arg::Sym("loop".to_string())));
        assert_eq!(classify("0x10"), Ok(Arg::Imm(16)));
        assert!(classify("%hi(loop)").is_err());
    }

    #[test]
    fn abi_register_names_map_to_numbers() {
        assert_eq!(parse_register("zero"), Some(0));
        assert_eq!(parse_register("ra"), Some(1));
        assert_eq!(parse_register("sp"), Some(2));
        assert_eq!(parse_register("t3"), Some(28));
        assert_eq!(parse_register("s0"), Some(8));
        assert_eq!(parse_register("fp"), Some(8));
        assert_eq!(parse_register("s11"), Some(19));
        assert_eq!(parse_register("a7"), Some(17));
        assert_eq!(parse_register("x31"), Some(31));
        assert_eq!(parse_register("x32"), None);
    }
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;

    fn program(source: &str) -> Program {
        let tree = rvh_syntax::parse(source);
        resolve(&tree, &Target::parse("rv32i_zicsr").expect("target"))
    }

    #[test]
    fn unknown_instruction_suggests_a_neighbour() {
        let program = program("adii a0, a1, 1\n");
        let diagnostic = &program.diagnostics[0];
        assert_eq!(diagnostic.code, "sem.unknown-instruction");
        assert!(
            diagnostic.message.contains("`addi`"),
            "{}",
            diagnostic.message
        );
        assert_eq!(diagnostic.fixes().len(), 1);
    }

    #[test]
    fn unknown_csr_suggests_a_neighbour() {
        let program = program("csrr a0, mstat\n");
        let diagnostic = &program.diagnostics[0];
        assert_eq!(diagnostic.code, "sem.unknown-csr");
        assert!(
            diagnostic.message.contains("`mstatus`"),
            "{}",
            diagnostic.message
        );
    }

    #[test]
    fn extern_is_accepted_like_global() {
        let tree = rvh_syntax::parse(".extern helper\n.globl mine\nmine:\n    ebreak\n");
        let program = resolve(&tree, &Target::parse("rv32i_zicsr").expect("target"));
        assert!(program.diagnostics.is_empty(), "{:?}", program.diagnostics);
    }

    #[test]
    fn unknown_directive_suggests_a_neighbour() {
        let program = program(".wor 1\n");
        let diagnostic = &program.diagnostics[0];
        assert_eq!(diagnostic.code, "sem.unknown-directive");
        assert!(
            diagnostic.message.contains("`.word`"),
            "{}",
            diagnostic.message
        );
    }

    #[test]
    fn out_of_range_immediate_names_the_next_step() {
        let program = program("addi a0, a1, 4096\n");
        let diagnostic = &program.diagnostics[0];
        assert!(
            diagnostic.message.contains("`li`"),
            "{}",
            diagnostic.message
        );
        assert!(
            diagnostic.message.contains("`add`"),
            "{}",
            diagnostic.message
        );
    }

    #[test]
    fn a_pseudo_with_wrong_operands_is_not_reported_as_unknown() {
        let program = program("li a0, a1, a2\n");
        assert_eq!(program.diagnostics[0].code, "sem.operand-mismatch");
    }
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    fn program(source: &str) -> Program {
        let tree = rvh_syntax::parse(source);
        resolve(&tree, &Target::parse("rv32i_zicsr").expect("target"))
    }

    /// Every pseudo the expander handles must also be in `PSEUDOS`, or the gate
    /// in `instruction` rejects it before the expander runs.
    #[test]
    fn every_pseudo_is_registered_and_expands() {
        let samples = [
            ("nop", "nop"),
            ("mv", "mv a0, a1"),
            ("not", "not a0, a1"),
            ("neg", "neg a0, a1"),
            ("seqz", "seqz a0, a1"),
            ("snez", "snez a0, a1"),
            ("j", "self:\n j self"),
            ("jr", "jr a0"),
            ("ret", "ret"),
            ("beqz", "self:\n beqz a0, self"),
            ("bnez", "self:\n bnez a0, self"),
            ("blez", "self:\n blez a0, self"),
            ("bgez", "self:\n bgez a0, self"),
            ("bltz", "self:\n bltz a0, self"),
            ("bgtz", "self:\n bgtz a0, self"),
            ("ble", "self:\n ble a0, a1, self"),
            ("bge", "self:\n bge a0, a1, self"),
            ("bleu", "self:\n bleu a0, a1, self"),
            ("bgeu", "self:\n bgeu a0, a1, self"),
            ("blt", "self:\n blt a0, a1, self"),
            ("bgt", "self:\n bgt a0, a1, self"),
            ("bltu", "self:\n bltu a0, a1, self"),
            ("bgtu", "self:\n bgtu a0, a1, self"),
            ("li", "li a0, 1"),
            ("la", "self:\n la a0, self"),
            ("csrr", "csrr a0, mstatus"),
            ("csrw", "csrw mstatus, a0"),
            ("csrs", "csrs mstatus, a0"),
            ("csrc", "csrc mstatus, a0"),
        ];
        for (name, source) in samples {
            assert!(PSEUDOS.contains(&name), "`{name}` is missing from PSEUDOS");
            let program = program(source);
            assert!(
                program.diagnostics.is_empty(),
                "`{name}`: {:?}",
                program.diagnostics
            );
        }
    }
}

/// ABI names, indexed by register number.
pub const REGISTER_NAMES: [&str; 32] = [
    "zero", "ra", "sp", "gp", "tp", "t0", "t1", "t2", "s0", "s1", "a0", "a1", "a2", "a3", "a4",
    "a5", "a6", "a7", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11", "t3", "t4",
    "t5", "t6",
];

/// ABI name of a register number.
#[must_use]
pub fn register_name(index: u8) -> Option<&'static str> {
    REGISTER_NAMES.get(usize::from(index)).copied()
}

/// One short role per register, from the psABI.
#[must_use]
pub fn register_role(index: u8) -> Option<&'static str> {
    const ROLES: [&str; 32] = [
        "constant zero; writes are ignored",
        "return address",
        "stack pointer",
        "global pointer",
        "thread pointer",
        "temporary",
        "temporary",
        "temporary",
        "saved register, also the frame pointer",
        "saved register",
        "argument and return value",
        "argument and return value",
        "argument",
        "argument",
        "argument",
        "argument",
        "argument",
        "argument",
        "saved register",
        "saved register",
        "saved register",
        "saved register",
        "saved register",
        "saved register",
        "saved register",
        "saved register",
        "saved register",
        "saved register",
        "temporary",
        "temporary",
        "temporary",
        "temporary",
    ];
    ROLES.get(usize::from(index)).copied()
}

/// Register number for an ABI name or an `x<N>` spelling.
#[must_use]
pub fn register_index(name: &str) -> Option<u8> {
    let lowered = name.to_ascii_lowercase();
    if let Some(rest) = lowered.strip_prefix('x') {
        if let Ok(index) = rest.parse::<u8>() {
            if index < 32 {
                return Some(index);
            }
        }
    }
    (0..32u8).find(|index| register_name(*index).is_some_and(|abi| abi == lowered))
}

/// Name of a CSR number, if the table knows it.
#[must_use]
pub fn csr_name(number: u32) -> Option<&'static str> {
    CSR_NAMES
        .iter()
        .find(|(_, value)| *value == i64::from(number))
        .map(|(name, _)| *name)
}

/// Number of a CSR name.
#[must_use]
pub fn csr_number(name: &str) -> Option<u32> {
    CSR_NAMES
        .iter()
        .find(|(csr, _)| *csr == name)
        .map(|(_, value)| *value as u32)
}

#[cfg(test)]
mod arch_tests {
    use super::*;

    fn resolve_with(source: &str, arch: &str) -> Program {
        let tree = rvh_syntax::parse(source);
        resolve(&tree, &Target::parse(arch).expect("target"))
    }

    #[test]
    fn versions_are_stripped() {
        assert_eq!(arch_name("rv32i2p1").as_deref(), Some("rv32i"));
        assert_eq!(
            arch_name("rv32i2p1_m2p0_zicsr2p0").as_deref(),
            Some("rv32i_m_zicsr")
        );
        assert_eq!(arch_name("nonsense"), None);
    }

    #[test]
    fn an_attribute_enables_an_extension_for_the_whole_file() {
        let program = resolve_with(
            ".attribute arch, \"rv32i2p1_m2p0\"\n_start:\n    mul a0, a1, a2\n",
            "rv32i_zicsr",
        );
        assert!(program.diagnostics.is_empty(), "{:?}", program.diagnostics);
        assert!(program.target.has("m"));
    }

    #[test]
    fn an_option_applies_from_its_line_on() {
        let program = resolve_with(
            "_start:\n    mul a0, a1, a2\n.option arch, +m\n    mul a0, a1, a2\n",
            "rv32i_zicsr",
        );
        assert_eq!(
            program
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "sem.extension-not-enabled")
                .count(),
            1,
            "only the instruction before the directive is rejected: {:?}",
            program.diagnostics
        );
    }

    #[test]
    fn an_option_can_remove_an_extension_again() {
        let program = resolve_with(
            ".option arch, +m\n.option arch, -m\n_start:\n    mul a0, a1, a2\n",
            "rv32i_zicsr",
        );
        assert!(
            program
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "sem.extension-not-enabled"),
            "{:?}",
            program.diagnostics
        );
    }

    #[test]
    fn an_unknown_target_is_a_diagnostic() {
        let program = resolve_with(".attribute arch, \"rv32i_q\"\n", "rv32i");
        assert!(
            program
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "sem.bad-target"),
            "{:?}",
            program.diagnostics
        );
    }
}
