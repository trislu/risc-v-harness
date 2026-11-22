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
    /// Compressed formats (ADR-0018). Scrambled fields live in the encoder and the
    /// decoder, like the six formats above.
    Ci,
    Cr,
    Css,
    Ciw,
    Cl,
    Cs,
    Ca,
    Cb,
    Cj,
    /// The four-register fused multiply-add format.
    R4,
}

impl Format {
    /// Width in bytes of an instruction in this format.
    #[must_use]
    pub fn size(self) -> u8 {
        match self {
            Self::R | Self::I | Self::S | Self::B | Self::U | Self::J | Self::R4 => 4,
            Self::Ci
            | Self::Cr
            | Self::Css
            | Self::Ciw
            | Self::Cl
            | Self::Cs
            | Self::Ca
            | Self::Cb
            | Self::Cj => 2,
        }
    }

    /// Parse the name used in `isa/*.toml`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "R" => Self::R,
            "I" => Self::I,
            "S" => Self::S,
            "B" => Self::B,
            "U" => Self::U,
            "J" => Self::J,
            "CI" => Self::Ci,
            "CR" => Self::Cr,
            "CSS" => Self::Css,
            "CIW" => Self::Ciw,
            "CL" => Self::Cl,
            "CS" => Self::Cs,
            "CA" => Self::Ca,
            "CB" => Self::Cb,
            "CJ" => Self::Cj,
            "R4" => Self::R4,
            _ => return None,
        })
    }

    /// The name used in `isa/*.toml` and in `rvh isa lookup` output.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::R => "R",
            Self::I => "I",
            Self::S => "S",
            Self::B => "B",
            Self::U => "U",
            Self::J => "J",
            Self::Ci => "CI",
            Self::Cr => "CR",
            Self::Css => "CSS",
            Self::Ciw => "CIW",
            Self::Cl => "CL",
            Self::Cs => "CS",
            Self::Ca => "CA",
            Self::Cb => "CB",
            Self::Cj => "CJ",
            Self::R4 => "R4",
        }
    }
}

impl Format {
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
            Self::Ci => &[
                ("funct3", 15, 13),
                ("imm", 12, 2),
                ("rd", 11, 7),
                ("opcode", 1, 0),
            ],
            Self::Cr => &[
                ("rd", 11, 7),
                ("rs2", 6, 2),
                ("funct2", 6, 5),
                ("opcode", 1, 0),
            ],
            Self::Css => &[
                ("funct3", 15, 13),
                ("imm", 12, 7),
                ("rs2", 6, 2),
                ("opcode", 1, 0),
            ],
            Self::Ciw => &[
                ("funct3", 15, 13),
                ("imm", 12, 5),
                ("rd", 4, 2),
                ("opcode", 1, 0),
            ],
            Self::Cl => &[
                ("funct3", 15, 13),
                ("imm", 12, 10),
                ("rs1", 9, 7),
                ("imm2", 6, 5),
                ("rd", 4, 2),
                ("opcode", 1, 0),
            ],
            Self::Cs => &[
                ("funct3", 15, 13),
                ("imm", 12, 10),
                ("rs1", 9, 7),
                ("imm2", 6, 5),
                ("rs2", 4, 2),
                ("opcode", 1, 0),
            ],
            Self::Ca => &[
                ("rd", 9, 7),
                ("funct2", 15, 10),
                ("rs2", 4, 2),
                ("funct2b", 6, 5),
                ("opcode", 1, 0),
            ],
            Self::Cb => &[
                ("funct3", 15, 13),
                ("imm", 12, 10),
                ("rs1", 9, 7),
                ("imm2", 6, 2),
                ("opcode", 1, 0),
            ],
            Self::Cj => &[("funct3", 15, 13), ("imm", 12, 2), ("opcode", 1, 0)],
            Self::R4 => &[
                ("rs3", 31, 27),
                ("fmt", 26, 25),
                ("rs2", 24, 20),
                ("rs1", 19, 15),
                ("funct3", 14, 12),
                ("rd", 11, 7),
                ("opcode", 6, 0),
            ],
        }
    }
}

/// The role a value plays in an instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperandKind {
    Rd,
    Rs1,
    Rs2,
    /// Floating-point registers, which live in their own file.
    Frd,
    Frs1,
    Frs2,
    /// The third source of a fused multiply-add.
    Frs3,
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
            Self::Frd => "frd",
            Self::Frs1 => "frs1",
            Self::Frs2 => "frs2",
            Self::Frs3 => "frs3",
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
    /// Instruction width in bytes: 4, or 2 for a compressed format.
    pub size: u8,
    /// The 7-bit opcode field.
    pub opcode: u8,
    pub funct3: Option<u8>,
    pub funct7: Option<u8>,
    /// `funct5`: the `rs2` field used as a constant, as the conversions do.
    pub funct5: Option<u8>,
    /// `funct6` of a compressed format (bits 15:10).
    pub funct6: Option<u8>,
    /// `funct4` of a compressed format (bits 15:12).
    pub funct4: Option<u8>,
    /// `funct2`, at bits 6:5 in CA and at bits 12:10 in CB.
    pub funct2: Option<u8>,
    /// `fmt` of the R4 layout (bits 26:25): 0 single, 1 double.
    pub fmt: Option<u8>,
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
        // A compressed instruction's opcode field is its two-bit quadrant.
        let mut bits = u32::from(self.opcode);
        let mut mask = if self.size == 2 { 0x3 } else { 0x7f };
        if let Some(funct3) = self.funct3 {
            bits |= u32::from(funct3) << 12;
            mask |= 0x7 << 12;
        }
        if let Some(funct7) = self.funct7 {
            bits |= u32::from(funct7) << 25;
            mask |= 0x7f << 25;
        }
        if let Some(funct5) = self.funct5 {
            bits |= u32::from(funct5) << 20;
            mask |= 0x1f << 20;
        }
        if let Some(funct6) = self.funct6 {
            bits |= u32::from(funct6) << 10;
            mask |= 0x3f << 10;
        }
        if let Some(funct4) = self.funct4 {
            bits |= u32::from(funct4) << 12;
            mask |= 0xf << 12;
        }
        if let Some(fmt) = self.fmt {
            bits |= u32::from(fmt) << 25;
            mask |= 0x3 << 25;
        }
        if let Some(funct2) = self.funct2 {
            // CA keeps `funct2` at 6:5; CB keeps it at 12:10.
            let shift = if self.format == Format::Cb { 10 } else { 5 };
            bits |= u32::from(funct2) << shift;
            mask |= 0x3 << shift;
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

    /// True when the low bits of `halfword` select a compressed instruction.
    #[must_use]
    pub fn is_compressed(halfword: u32) -> bool {
        halfword & 0x3 != 0x3
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
            141,
            "40 RV32I, 6 Zicsr, 8 M, 11 A, 26 RV32C, 25 RV32F and 25 RV32D instructions"
        );
        assert_eq!(extensions(), vec!["a", "c", "d", "f", "i", "m", "zicsr"]);
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
