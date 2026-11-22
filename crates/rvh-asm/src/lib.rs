//! Layout and encoding: a resolved program becomes an `Image`.
//!
//! This is where addresses exist. Two passes: assign each statement an address,
//! then encode it. Branch targets, `jal` offsets and `la` pairs are resolved in
//! the second pass, when every label address is known.

use std::collections::HashMap;

use rvh_isa::{Format, Insn, OperandKind};
use rvh_sem::{Op, Program, Stmt};
use rvh_syntax::Diagnostic;

/// One loaded section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    pub addr: u32,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub addr: u32,
    pub section: usize,
}

/// Maps an address back to the source statement that produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSpan {
    pub addr: u32,
    pub span: (usize, usize),
}

/// The assembler's output. Plain data: no file format, no relocations left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub sections: Vec<Section>,
    pub entry: u32,
    pub symbols: Vec<Symbol>,
    pub source_map: Vec<SourceSpan>,
}

impl Image {
    #[must_use]
    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|section| section.name == name)
    }

    /// Bytes of the `.text` section.
    #[must_use]
    pub fn text(&self) -> &[u8] {
        self.section("text")
            .map_or(&[], |section| section.data.as_slice())
    }

    /// Every section placed at its address, from 0 to the highest end.
    #[must_use]
    /// The image as a loadable ELF32 executable.
    ///
    /// One `PT_LOAD` segment per non-empty section. Flags follow the section:
    /// `text` is read and execute, `rodata` read-only, everything else read and
    /// write.
    pub fn to_elf(&self) -> Vec<u8> {
        let segments = self
            .sections
            .iter()
            .filter(|section| !section.data.is_empty())
            .map(|section| rvh_elf::Segment {
                addr: section.addr,
                data: section.data.clone(),
                mem_size: section.data.len() as u32,
                flags: match section.name.as_str() {
                    name if name.contains("text") => 5,
                    name if name.contains("rodata") => 4,
                    _ => 6,
                },
            })
            .collect();
        rvh_elf::write(&rvh_elf::Executable {
            entry: self.entry,
            segments,
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let end = self
            .sections
            .iter()
            .map(|section| section.addr as usize + section.data.len())
            .max()
            .unwrap_or(0);
        let mut out = vec![0u8; end];
        for section in &self.sections {
            let start = section.addr as usize;
            out[start..start + section.data.len()].copy_from_slice(&section.data);
        }
        out
    }

    #[must_use]
    pub fn symbol(&self, name: &str) -> Option<&Symbol> {
        self.symbols.iter().find(|symbol| symbol.name == name)
    }
}

/// Assemble a resolved program.
pub fn assemble(program: &Program) -> (Image, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let mut labels: HashMap<String, u32> = HashMap::new();

    // Pass 1: addresses.
    let mut bases = Vec::with_capacity(program.sections.len());
    let mut cursor = 0u32;
    for section in &program.sections {
        let base = align_up(cursor, 16);
        let mut addr = base;
        for stmt in &section.stmts {
            if let Stmt::Label { name, .. } = stmt {
                labels.insert(name.clone(), addr);
            }
            addr = addr.saturating_add(size_of(stmt, addr));
        }
        bases.push(base);
        cursor = addr;
    }

    // Pass 2: encode.
    let mut sections = Vec::with_capacity(program.sections.len());
    let mut source_map = Vec::new();
    for (index, section) in program.sections.iter().enumerate() {
        let base = bases[index];
        let mut addr = base;
        let mut data = Vec::new();
        for stmt in &section.stmts {
            if !matches!(stmt, Stmt::Label { .. }) {
                source_map.push(SourceSpan {
                    addr,
                    span: stmt.span(),
                });
            }
            match stmt {
                Stmt::Label { .. } => {}
                Stmt::Align { power, .. } => {
                    let padding = padding(addr, *power);
                    data.extend(std::iter::repeat_n(0u8, padding));
                    addr += padding as u32;
                }
                Stmt::Zero { len, .. } => {
                    data.extend(std::iter::repeat_n(0u8, *len as usize));
                    addr += *len;
                }
                Stmt::Data { bytes, .. } => {
                    data.extend_from_slice(bytes);
                    addr += bytes.len() as u32;
                }
                Stmt::Raw { word, .. } => {
                    data.extend_from_slice(&word.to_le_bytes());
                    addr += 4;
                }
                Stmt::Insn { insn, ops, span } => {
                    let result = if insn.size == 2 {
                        encode_compressed_fields(insn, ops, addr, &labels)
                            .map(|word| (u32::from(word), 2u32))
                    } else {
                        encode_insn(insn, ops, addr, &labels).map(|word| (word, 4u32))
                    };
                    match result {
                        Ok((word, size)) => {
                            data.extend_from_slice(&word.to_le_bytes()[..size as usize]);
                            addr += size;
                        }
                        Err(message) => {
                            diagnostics.push(Diagnostic::new("asm.encode", message, *span));
                            data.extend(std::iter::repeat_n(0u8, insn.size as usize));
                            addr += u32::from(insn.size);
                        }
                    }
                }
                Stmt::La { rd, sym, span } => match encode_la(*rd, sym, addr, &labels) {
                    Ok(words) => {
                        for word in words {
                            data.extend_from_slice(&word.to_le_bytes());
                        }
                        addr += 8;
                    }
                    Err(message) => {
                        diagnostics.push(Diagnostic::new("asm.encode", message, *span));
                        data.extend_from_slice(&[0; 8]);
                        addr += 8;
                    }
                },
            }
        }
        sections.push(Section {
            name: section.name.clone(),
            addr: base,
            data,
        });
    }

    let symbols = program
        .symbols
        .iter()
        .filter_map(|symbol| match symbol.kind {
            rvh_sem::SymbolKind::Label { section } => labels.get(&symbol.name).map(|addr| Symbol {
                name: symbol.name.clone(),
                addr: *addr,
                section,
            }),
            rvh_sem::SymbolKind::Constant(_) => None,
        })
        .collect::<Vec<_>>();

    let entry = labels
        .get("_start")
        .copied()
        .or_else(|| sections.first().map(|section| section.addr))
        .unwrap_or(0);

    (
        Image {
            sections,
            entry,
            symbols,
            source_map,
        },
        diagnostics,
    )
}

fn align_up(value: u32, align: u32) -> u32 {
    if align <= 1 {
        return value;
    }
    value.div_ceil(align) * align
}

fn padding(addr: u32, power: u8) -> usize {
    let align = 1u32.checked_shl(u32::from(power)).unwrap_or(0);
    if align <= 1 {
        return 0;
    }
    (align_up(addr, align) - addr) as usize
}

fn size_of(stmt: &Stmt, addr: u32) -> u32 {
    match stmt {
        Stmt::Label { .. } => 0,
        Stmt::Insn { insn, .. } => u32::from(insn.size),
        Stmt::Raw { .. } => 4,
        Stmt::La { .. } => 8,
        Stmt::Data { bytes, .. } => bytes.len() as u32,
        Stmt::Align { power, .. } => padding(addr, *power) as u32,
        Stmt::Zero { len, .. } => *len,
    }
}

fn register(op: &Op) -> Result<u32, String> {
    match op {
        Op::Reg(value) => Ok(u32::from(*value)),
        other => Err(format!("expected a register, found {other:?}")),
    }
}

/// One tone for every unresolved reference: this assembler does not link.
fn undefined(name: &str) -> String {
    format!("`{name}` is not defined in this file; cross-file symbols need a linker, which rvh does not implement")
}

fn immediate(op: &Op, pc: u32, labels: &HashMap<String, u32>) -> Result<i64, String> {
    match op {
        Op::Imm(value) => Ok(*value),
        Op::Sym(name) => {
            let target = labels.get(name.as_str()).ok_or_else(|| undefined(name))?;
            Ok(i64::from(*target) - i64::from(pc))
        }
        Op::Reg(_) => Err("expected an immediate, found a register".to_string()),
    }
}

/// Encode a compressed instruction (ADR-0018).
///
/// The nine formats keep their own layouts here rather than in the data file:
/// every immediate is scattered, and each one needs its own range check.
fn encode_compressed_fields(
    insn: &Insn,
    ops: &[Op],
    pc: u32,
    labels: &HashMap<String, u32>,
) -> Result<u16, String> {
    let mut rd = 0u32;
    let mut rs1 = 0u32;
    let mut rs2 = 0u32;
    let mut imm: i64 = 0;
    for (spec, op) in insn.operands.iter().zip(ops.iter()) {
        match spec.kind {
            OperandKind::Rd => rd = register(op)?,
            OperandKind::Rs1 => rs1 = register(op)?,
            OperandKind::Rs2 => rs2 = register(op)?,
            _ => imm = immediate(op, pc, labels)?,
        }
    }
    encode_compressed(insn, rd, rs1, rs2, imm)
}

fn encode_compressed(insn: &Insn, rd: u32, rs1: u32, rs2: u32, imm: i64) -> Result<u16, String> {
    // Registers in CIW, CL, CS, CB and CA are three bits naming x8..x15.
    fn primed(register: u32) -> Result<u32, String> {
        if (8..16).contains(&register) {
            Ok(register - 8)
        } else {
            Err(format!(
                "`{}` needs a register in x8..x15 here, found x{register}",
                register
            ))
        }
    }

    let funct3 = u32::from(insn.funct3.unwrap_or(0)) & 0x7;
    let base = (funct3 << 13) | u32::from(insn.opcode) & 0x3;
    let word = match insn.format {
        Format::Ciw => {
            // nzuimm[5:4|9:6|2|3]
            if rs1 != 2 {
                return Err("`c.addi4spn` needs `sp` as the base".to_string());
            }
            if imm <= 0 || imm > 1020 || imm % 4 != 0 {
                return Err(format!(
                    "`{}` needs a nonzero multiple of 4 up to 1020, found {imm}",
                    insn.mnemonic
                ));
            }
            let value = imm as u32;
            base | ((value >> 4) & 0x3) << 11
                | ((value >> 6) & 0xf) << 7
                | ((value >> 2) & 0x1) << 6
                | ((value >> 3) & 0x1) << 5
                | primed(rd)? << 2
        }
        Format::Cl | Format::Cs => {
            if !(0..=124).contains(&imm) || imm % 4 != 0 {
                return Err(format!(
                    "`{}` needs a multiple of 4 up to 124, found {imm}",
                    insn.mnemonic
                ));
            }
            let value = imm as u32;
            let offset =
                ((value >> 3) & 0x7) << 10 | ((value >> 2) & 0x1) << 6 | ((value >> 6) & 0x1) << 5;
            let reg = if insn.format == Format::Cl {
                primed(rd)?
            } else {
                primed(rs2)?
            };
            let other = primed(rs1)?;
            base | offset | (other << 7) | (reg << 2)
        }
        Format::Ci => match insn.mnemonic {
            "c.addi16sp" => {
                if rs1 != 2 {
                    return Err("`c.addi16sp` needs `sp`".to_string());
                }
                if imm == 0 || imm % 16 != 0 || !(-512..=496).contains(&imm) {
                    return Err(format!(
                        "`c.addi16sp` needs a nonzero multiple of 16, found {imm}"
                    ));
                }
                let value = imm as u32;
                base | ((value >> 9) & 0x1) << 12
                    | ((value >> 4) & 0x1) << 6
                    | ((value >> 6) & 0x1) << 5
                    | ((value >> 7) & 0x3) << 3
                    | ((value >> 5) & 0x1) << 2
                    | (2 << 7)
            }
            "c.lui" => {
                // The operand is the 6-bit upper field, like `lui`, not the scaled value.
                if rd == 0 || rd == 2 || imm == 0 || !(-32..=31).contains(&imm) {
                    return Err(format!(
                        "`c.lui` needs a nonzero 6-bit upper immediate and rd not in {{0, 2}}, found {imm}"
                    ));
                }
                let value = imm as u32 & 0x3f;
                base | ((value >> 5) & 0x1) << 12 | (value & 0x1f) << 2 | (rd << 7)
            }
            "c.lwsp" => {
                if rs1 != 2 {
                    return Err("`c.lwsp` needs `sp`".to_string());
                }
                if !(0..=252).contains(&imm) || imm % 4 != 0 || rd == 0 {
                    return Err(format!(
                        "`c.lwsp` needs a multiple of 4 up to 252 and rd != 0, found {imm}"
                    ));
                }
                let value = imm as u32;
                base | ((value >> 5) & 0x1) << 12
                    | ((value >> 2) & 0x7) << 4
                    | ((value >> 6) & 0x3) << 2
                    | (rd << 7)
            }
            _ => {
                // c.addi, c.li and c.slli: imm[5] at 12, imm[4:0] at 6:2.
                let (low, high) = match insn.mnemonic {
                    "c.addi" | "c.li" => (-32, 31),
                    _ => (1, 31),
                };
                if imm < low || imm > high {
                    return Err(format!(
                        "`{}` immediate {imm} is out of range",
                        insn.mnemonic
                    ));
                }
                let value = imm as u32 & 0x3f;
                base | ((value >> 5) & 0x1) << 12 | (value & 0x1f) << 2 | (rd << 7)
            }
        },
        Format::Cr => {
            let funct4 = u32::from(insn.funct4.unwrap_or(0)) & 0xf;
            let check = |condition: bool, message: &str| -> Result<(), String> {
                if condition {
                    Ok(())
                } else {
                    Err(message.to_string())
                }
            };
            match insn.mnemonic {
                "c.jr" => {
                    check(rs1 != 0, "`c.jr` needs a nonzero register")?;
                    base & 0x3 | (funct4 << 12) | (rs1 << 7)
                }
                "c.jalr" => {
                    check(rs1 != 0, "`c.jalr` needs a nonzero register")?;
                    base & 0x3 | (funct4 << 12) | (rs1 << 7)
                }
                "c.ebreak" => base & 0x3 | (funct4 << 12),
                "c.mv" | "c.add" => {
                    check(rs2 != 0, "`c.mv` and `c.add` need a nonzero rs2")?;
                    base & 0x3 | (funct4 << 12) | (rd << 7) | (rs2 << 2)
                }
                other => return Err(format!("unknown compressed instruction `{other}`")),
            }
        }
        Format::Css => {
            if rs1 != 2 {
                return Err("`c.swsp` needs `sp`".to_string());
            }
            if !(0..=252).contains(&imm) || imm % 4 != 0 {
                return Err(format!(
                    "`c.swsp` needs a multiple of 4 up to 252, found {imm}"
                ));
            }
            let value = imm as u32;
            base | ((value >> 2) & 0xf) << 9 | ((value >> 6) & 0x3) << 7 | (rs2 << 2)
        }
        Format::Ca => {
            let funct6 = u32::from(insn.funct6.unwrap_or(0)) & 0x3f;
            let funct2 = u32::from(insn.funct2.unwrap_or(0)) & 0x3;
            base & 0x3 | (funct6 << 10) | (primed(rd)? << 7) | (funct2 << 5) | (primed(rs2)? << 2)
        }
        Format::Cb => match insn.mnemonic {
            "c.beqz" | "c.bnez" => {
                if imm % 2 != 0 || !(-256..=254).contains(&imm) {
                    return Err(format!("`{}` offset {imm} is out of range", insn.mnemonic));
                }
                let value = imm as u32;
                base | ((value >> 8) & 0x1) << 12
                    | ((value >> 3) & 0x3) << 10
                    | (primed(rs1)? << 7)
                    | ((value >> 6) & 0x3) << 5
                    | ((value >> 1) & 0x3) << 3
                    | ((value >> 5) & 0x1) << 2
            }
            _ => {
                // c.srli, c.srai, c.andi: funct2 at 12:10, shamt or imm at 6:2.
                let funct2 = u32::from(insn.funct2.unwrap_or(0)) & 0x3;
                if !(0..=63).contains(&imm) {
                    return Err(format!(
                        "`{}` immediate {imm} is out of range",
                        insn.mnemonic
                    ));
                }
                let value = imm as u32 & 0x3f;
                base & 0x3
                    | (0x4 << 13)
                    | (funct2 << 10)
                    | (primed(rd)? << 7)
                    | ((value >> 5) & 0x1) << 12
                    | (value & 0x1f) << 2
            }
        },
        Format::Cj => {
            if imm % 2 != 0 || !(-2048..=2046).contains(&imm) {
                return Err(format!("`{}` offset {imm} is out of range", insn.mnemonic));
            }
            let value = imm as u32;
            base | ((value >> 11) & 0x1) << 12
                | ((value >> 4) & 0x1) << 11
                | ((value >> 8) & 0x3) << 9
                | ((value >> 10) & 0x1) << 8
                | ((value >> 6) & 0x1) << 7
                | ((value >> 7) & 0x1) << 6
                | ((value >> 1) & 0x7) << 3
                | ((value >> 5) & 0x1) << 2
        }
        _ => return Err(format!("`{}` is not a compressed format", insn.mnemonic)),
    };
    Ok(word as u16)
}

fn encode_insn(
    insn: &Insn,
    ops: &[Op],
    pc: u32,
    labels: &HashMap<String, u32>,
) -> Result<u32, String> {
    let mut rd = 0u32;
    let mut rs1 = 0u32;
    let mut rs2 = 0u32;
    let mut imm: i64 = 0;

    for (spec, op) in insn.operands.iter().zip(ops.iter()) {
        match spec.kind {
            OperandKind::Rd => rd = register(op)?,
            OperandKind::Rs1 => rs1 = register(op)?,
            OperandKind::Rs2 => rs2 = register(op)?,
            OperandKind::Imm | OperandKind::Csr | OperandKind::Zimm | OperandKind::Shamt => {
                imm = immediate(op, pc, labels)?;
            }
            OperandKind::Pred => {
                let pred = immediate(op, pc, labels)?;
                imm = (pred & 0xf) << 4;
            }
            OperandKind::Succ => {
                let succ = immediate(op, pc, labels)?;
                imm |= succ & 0xf;
            }
        }
    }

    match insn.format {
        Format::B => {
            if imm % 2 != 0 {
                return Err(format!("branch offset {imm} is not a multiple of 2"));
            }
            if !(-4096..=4094).contains(&imm) {
                return Err(format!("branch offset {imm} is out of range"));
            }
        }
        Format::J => {
            if imm % 2 != 0 {
                return Err(format!("jump offset {imm} is not a multiple of 2"));
            }
            if !(-1_048_576..=1_048_574).contains(&imm) {
                return Err(format!("jump offset {imm} is out of range"));
            }
        }
        _ => {}
    }

    let mut word = u32::from(insn.opcode) | (rd << 7) | (rs1 << 15);
    if let Some(funct3) = insn.funct3 {
        word |= u32::from(funct3) << 12;
    }

    match insn.format {
        Format::R => {
            word |= rs2 << 20;
            if let Some(funct7) = insn.funct7 {
                word |= u32::from(funct7) << 25;
            }
        }
        Format::I => {
            let field = if let Some(funct7) = insn.funct7 {
                (u32::from(funct7) << 5) | ((imm as u32) & 0x1f)
            } else if let Some(fixed) = insn.imm12 {
                u32::from(fixed)
            } else {
                (imm as u32) & 0xfff
            };
            word |= field << 20;
        }
        Format::S => {
            let value = imm as u32;
            word |= rs2 << 20;
            word |= ((value >> 5) & 0x7f) << 25;
            word |= (value & 0x1f) << 7;
        }
        Format::B => {
            let value = imm as u32;
            word |= rs2 << 20;
            word |= ((value >> 12) & 1) << 31;
            word |= ((value >> 5) & 0x3f) << 25;
            word |= ((value >> 1) & 0xf) << 8;
            word |= ((value >> 11) & 1) << 7;
        }
        Format::U => {
            word |= ((imm as u32) & 0xfffff) << 12;
        }
        Format::J => {
            let value = imm as u32;
            word |= ((value >> 20) & 1) << 31;
            word |= ((value >> 1) & 0x3ff) << 21;
            word |= ((value >> 11) & 1) << 20;
            word |= ((value >> 12) & 0xff) << 12;
        }
        // The nine compressed formats share nothing with the six above; they are
        // encoded by their own function (ADR-0018, next step).
        Format::Ci
        | Format::Cr
        | Format::Css
        | Format::Ciw
        | Format::Cl
        | Format::Cs
        | Format::Ca
        | Format::Cb
        | Format::Cj => {
            return Err("compressed instruction encoding is not implemented yet".to_string());
        }
    }
    Ok(word)
}

fn encode_la(
    rd: u8,
    sym: &str,
    pc: u32,
    labels: &HashMap<String, u32>,
) -> Result<[u32; 2], String> {
    let target = labels.get(sym).ok_or_else(|| undefined(sym))?;
    let delta = i64::from(*target) - i64::from(pc);
    if !(-2_147_483_648..=2_147_483_647).contains(&delta) {
        return Err(format!("`la {sym}` is out of range"));
    }
    let hi = (delta + 0x800) >> 12;
    let lo = delta - (hi << 12);
    let rd = u32::from(rd);
    let auipc = 0b0010111 | (rd << 7) | (((hi as u32) & 0xfffff) << 12);
    let addi = 0b0010011 | (rd << 7) | (rd << 15) | (((lo as u32) & 0xfff) << 20);
    Ok([auipc, addi])
}

#[cfg(test)]
mod tests {
    use super::*;
    use rvh_isa::Target;

    fn assemble_source(source: &str) -> (Image, Vec<Diagnostic>) {
        let tree = rvh_syntax::parse(source);
        let program = rvh_sem::resolve(&tree, &Target::parse("rv32i_zicsr").expect("target"));
        let mut diagnostics = program.diagnostics.clone();
        let (image, encode_diagnostics) = assemble(&program);
        diagnostics.extend(encode_diagnostics);
        (image, diagnostics)
    }

    #[test]
    fn encodes_addi_like_gnu_as() {
        let (image, diagnostics) = assemble_source("addi a0, a0, 1\n");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(image.text(), &0x0015_0513u32.to_le_bytes());
    }

    #[test]
    fn encodes_ecall_and_ebreak() {
        let (image, diagnostics) = assemble_source("ecall\nebreak\n");
        assert!(diagnostics.is_empty());
        assert_eq!(&image.text()[0..4], &0x0000_0073u32.to_le_bytes());
        assert_eq!(&image.text()[4..8], &0x0010_0073u32.to_le_bytes());
    }

    #[test]
    fn branch_target_is_resolved() {
        // loop: nop; j loop -> offset -4 at pc 4
        let (image, diagnostics) = assemble_source("loop:\n  nop\n  j loop\n");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(&image.text()[4..8], &0xffdf_f06fu32.to_le_bytes());
    }

    #[test]
    fn forward_branch_offset_is_relative() {
        // beq a0, a1, end with one instruction in between
        let (image, diagnostics) = assemble_source("  beq a0, a1, end\n  nop\nend:\n");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let word = u32::from_le_bytes(image.text()[0..4].try_into().expect("word"));
        // offset = 8, rs1 = 10, rs2 = 11, funct3 = 0, opcode = 0x63
        assert_eq!(word, 0b0000_0000_1011_0101_0000_0100_0110_0011);
    }

    #[test]
    fn la_produces_an_auipc_addi_pair() {
        let (image, diagnostics) = assemble_source("  la a0, target\n  nop\ntarget:\n  nop\n");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let auipc = u32::from_le_bytes(image.text()[0..4].try_into().expect("word"));
        assert_eq!(auipc & 0x7f, 0b0010111);
        assert_eq!((auipc >> 7) & 0x1f, 10);
        let addi = u32::from_le_bytes(image.text()[4..8].try_into().expect("word"));
        assert_eq!(addi & 0x7f, 0b0010011);
        // The pair must reconstruct the target address: auipc pc 0, target 12.
        let hi = ((auipc >> 12) & 0xfffff) as i64;
        let lo = ((addi >> 20) & 0xfff) as i64;
        let lo = if lo >= 0x800 { lo - 0x1000 } else { lo };
        assert_eq!((hi << 12) + lo, 12);
    }

    #[test]
    fn undefined_symbol_is_reported() {
        let (_, diagnostics) = assemble_source("j nowhere\n");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "asm.encode");
        assert!(
            diagnostics[0]
                .message
                .contains("is not defined in this file"),
            "{}",
            diagnostics[0].message
        );
    }

    #[test]
    fn entry_prefers_start_label() {
        let (image, _) = assemble_source("  nop\n_start:\n  nop\n");
        assert_eq!(image.entry, 4);
        assert_eq!(image.symbol("_start").map(|symbol| symbol.addr), Some(4));
    }

    #[test]
    fn sections_are_placed_after_text() {
        let (image, diagnostics) = assemble_source("  nop\n.data\n.word 1\n");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let data = image.section("data").expect("data");
        assert_eq!(data.addr, 16);
        assert_eq!(data.data, 1u32.to_le_bytes());
    }

    #[test]
    fn fence_without_operands_uses_full_masks() {
        let (image, diagnostics) = assemble_source("fence\n");
        assert!(diagnostics.is_empty());
        assert_eq!(image.text(), &0x0ff0_000fu32.to_le_bytes());
    }
}
