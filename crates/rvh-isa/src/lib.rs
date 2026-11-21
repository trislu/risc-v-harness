//! Instruction metadata for RISC-V.
//!
//! The tables in [`generated`] are built from `isa/*.toml` by
//! `cargo xtask codegen`. This crate knows nothing about syntax or execution: it
//! answers "what is this instruction, and where are its bits?"

#![forbid(unsafe_code)]

pub mod generated;

mod target;

pub use target::{Target, TargetError};

/// Instruction format. The format decides where the operands sit in the word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    R,
    I,
    S,
    B,
    U,
    J,
}

impl Format {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::R => "R",
            Self::I => "I",
            Self::S => "S",
            Self::B => "B",
            Self::U => "U",
            Self::J => "J",
        }
    }

    /// Bit fields of the format, most significant first, as `(name, hi, lo)`.
    ///
    /// The names follow the instruction set manual. `imm` appears more than once
    /// for the formats that scatter the immediate.
    #[must_use]
    pub fn fields(self) -> &'static [(&'static str, u8, u8)] {
        match self {
            Self::R => &[
                ("funct7", 31, 25),
                ("rs2", 24, 20),
                ("rs1", 19, 15),
                ("funct3", 14, 12),
                ("rd", 11, 7),
                ("opcode", 6, 0),
            ],
            Self::I => &[
                ("imm", 31, 20),
                ("rs1", 19, 15),
                ("funct3", 14, 12),
                ("rd", 11, 7),
                ("opcode", 6, 0),
            ],
            Self::S => &[
                ("imm", 31, 25),
                ("rs2", 24, 20),
                ("rs1", 19, 15),
                ("funct3", 14, 12),
                ("imm", 11, 7),
                ("opcode", 6, 0),
            ],
            Self::B => &[
                ("imm", 31, 25),
                ("rs2", 24, 20),
                ("rs1", 19, 15),
                ("funct3", 14, 12),
                ("imm", 11, 8),
                ("imm", 7, 7),
                ("opcode", 6, 0),
            ],
            Self::U => &[("imm", 31, 12), ("rd", 11, 7), ("opcode", 6, 0)],
            Self::J => &[("imm", 31, 12), ("rd", 11, 7), ("opcode", 6, 0)],
        }
    }
}

/// The role a value plays in an instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperandKind {
    Rd,
    Rs1,
    Rs2,
    Imm,
    Shamt,
    Csr,
    Zimm,
    Pred,
    Succ,
}

impl OperandKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rd => "rd",
            Self::Rs1 => "rs1",
            Self::Rs2 => "rs2",
            Self::Imm => "imm",
            Self::Shamt => "shamt",
            Self::Csr => "csr",
            Self::Zimm => "zimm",
            Self::Pred => "pred",
            Self::Succ => "succ",
        }
    }

    /// True for operands that name a general purpose register.
    #[must_use]
    pub fn is_register(self) -> bool {
        matches!(self, Self::Rd | Self::Rs1 | Self::Rs2)
    }
}

/// One operand of an instruction, as declared in `isa/*.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Operand {
    pub kind: OperandKind,
    pub bits: u8,
    pub signed: bool,
    /// Required alignment in bytes. `1` means unconstrained.
    pub align: u8,
}

impl Operand {
    /// Inclusive value range, after alignment.
    #[must_use]
    pub fn range(self) -> (i64, i64) {
        if self.signed {
            let half = 1i64 << (self.bits - 1);
            (-half, half - 1)
        } else {
            (0, (1i64 << self.bits) - 1)
        }
    }
}

/// One instruction, as declared in `isa/*.toml`.
///
/// `Static` fields only: the table is compiled into the binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Insn {
    pub id: &'static str,
    pub mnemonic: &'static str,
    pub ext: &'static str,
    pub format: Format,
    /// The 7-bit opcode field.
    pub opcode: u8,
    pub funct3: Option<u8>,
    pub funct7: Option<u8>,
    /// A fixed value in the 12-bit immediate field (system instructions).
    pub imm12: Option<u16>,
    /// Semantic class, used to group content and explanation templates.
    pub class: &'static str,
    pub operands: &'static [Operand],
}

impl Insn {
    /// `(bits, mask)` of the fields that are constant for this instruction.
    #[must_use]
    pub fn fixed(&self) -> (u32, u32) {
        let mut bits = u32::from(self.opcode);
        let mut mask = 0x7f;
        if let Some(funct3) = self.funct3 {
            bits |= u32::from(funct3) << 12;
            mask |= 0x7 << 12;
        }
        if let Some(funct7) = self.funct7 {
            bits |= u32::from(funct7) << 25;
            mask |= 0x7f << 25;
        }
        if let Some(imm12) = self.imm12 {
            bits |= u32::from(imm12) << 20;
            mask |= 0xfff << 20;
        }
        (bits, mask)
    }

    /// True when `word` is this instruction.
    #[must_use]
    pub fn matches(&self, word: u32) -> bool {
        let (bits, mask) = self.fixed();
        word & mask == bits
    }

    /// Content key for a documentation field, e.g. `insn.addi.summary`.
    #[must_use]
    pub fn content_key(&self, field: &str) -> String {
        format!("insn.{}.{}", self.id, field)
    }

    #[must_use]
    pub fn operand(&self, kind: OperandKind) -> Option<&'static Operand> {
        self.operands.iter().find(|operand| operand.kind == kind)
    }
}

/// Every instruction known to the database.
#[must_use]
pub fn all() -> &'static [Insn] {
    generated::INSNS
}

/// The first instruction with this mnemonic, if any.
#[must_use]
pub fn lookup(mnemonic: &str) -> Option<&'static Insn> {
    all().iter().find(|insn| insn.mnemonic == mnemonic)
}

/// Every instruction with this mnemonic. Mnemonics are unique per extension, and
/// extensions may reuse one, so this is the honest lookup.
pub fn lookups(mnemonic: &str) -> impl Iterator<Item = &'static Insn> + use<'_> {
    all().iter().filter(move |insn| insn.mnemonic == mnemonic)
}

/// Extension identifiers present in the database, sorted and deduplicated.
#[must_use]
pub fn extensions() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = all().iter().map(|insn| insn.ext).collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// Instructions in one extension.
pub fn insns_in(ext: &'static str) -> impl Iterator<Item = &'static Insn> {
    all().iter().filter(move |insn| insn.ext == ext)
}

/// Decode a word into the instructions it could be. More than one match means
/// the database is ambiguous, which the codegen check should have caught.
#[must_use]
pub fn decode(word: u32) -> Vec<&'static Insn> {
    all().iter().filter(|insn| insn.matches(word)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_complete() {
        assert_eq!(
            all().len(),
            65,
            "40 RV32I instructions, 6 Zicsr, 8 M and 11 A"
        );
        assert_eq!(extensions(), vec!["a", "i", "m", "zicsr"]);
    }

    #[test]
    fn addi_matches_a_real_word() {
        // addi a0, a0, 1, as produced by GNU as.
        let word = 0x0015_0513;
        let insn = lookup("addi").expect("addi is in the table");
        assert!(insn.matches(word));
        assert_eq!(decode(word).len(), 1);
    }

    #[test]
    fn add_does_not_match_an_addi_word() {
        let word = 0x0015_0513;
        assert!(!lookup("add").expect("add is in the table").matches(word));
    }

    #[test]
    fn ecall_and_ebreak_differ_only_in_the_immediate_field() {
        let ecall = lookup("ecall").expect("ecall");
        let ebreak = lookup("ebreak").expect("ebreak");
        assert!(ecall.matches(0x0000_0073));
        assert!(!ebreak.matches(0x0000_0073));
        assert!(ebreak.matches(0x0010_0073));
        assert!(!ecall.matches(0x0010_0073));
    }

    #[test]
    fn content_keys_derive_from_the_id() {
        assert_eq!(
            lookup("addi").expect("addi").content_key("summary"),
            "insn.addi.summary"
        );
    }

    #[test]
    fn immediate_ranges_follow_signedness() {
        let addi = lookup("addi").expect("addi");
        let imm = addi
            .operand(OperandKind::Imm)
            .expect("addi has an immediate");
        assert_eq!(imm.range(), (-2048, 2047));
    }

    #[test]
    fn unknown_mnemonic_is_none() {
        assert!(lookup("frobnicate").is_none());
    }
}
