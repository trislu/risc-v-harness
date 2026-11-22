//! Render a machine word as assembly.
//!
//! The rendering is for people, not for round-tripping: registers use their ABI
//! names, immediates are signed where the instruction treats them as signed, and a
//! memory operand is written `imm(rs1)`. The assembler remains the authority on
//! what it accepts.

use rvh_isa::{Format, Insn, OperandKind};

/// One word as assembly, or `None` when no instruction matches.
#[must_use]
pub fn word(word: u32) -> Option<String> {
    if rvh_isa::Insn::is_compressed(word) {
        let decoded = crate::compressed::decode(word as u16)?;
        return Some(render(
            decoded.insn,
            decoded.rd,
            decoded.rs1,
            decoded.rs2,
            i64::from(decoded.imm),
        ));
    }
    let insn = rvh_isa::decode(word).first().copied()?;
    let rd = ((word >> 7) & 0x1f) as u8;
    let rs1 = ((word >> 15) & 0x1f) as u8;
    let rs2 = ((word >> 20) & 0x1f) as u8;
    let imm = match insn.format {
        Format::I => i64::from(crate::imm_i(word)),
        Format::S => i64::from(crate::imm_s(word)),
        Format::B => i64::from(crate::imm_b(word)),
        Format::J => i64::from(crate::imm_j(word)),
        // `lui` and `auipc` carry the upper field: print it as written in source.
        Format::U => i64::from((word & 0xffff_f000) as i32 >> 12),
        _ => 0,
    };
    Some(render(insn, rd, rs1, rs2, imm))
}

fn register(index: u8) -> String {
    rvh_sem::register_name(index).map_or_else(|| format!("x{index}"), str::to_string)
}

fn float_register(index: u8) -> String {
    format!("f{index}")
}

fn immediate(insn: &Insn, kind: OperandKind, imm: i64) -> String {
    match kind {
        OperandKind::Csr => format!("0x{imm:03x}"),
        _ if insn.mnemonic.starts_with("lui") || insn.mnemonic.starts_with("auipc") => {
            format!("0x{:x}", (imm as u64) & 0xfffff)
        }
        _ => format!("{imm}"),
    }
}

fn render(insn: &Insn, rd: u8, rs1: u8, rs2: u8, imm: i64) -> String {
    let memory = matches!(insn.class, "load" | "store") || insn.mnemonic == "jalr";
    let mut parts: Vec<String> = Vec::new();
    let mut memory_base: Option<u8> = None;
    for spec in insn.operands {
        match spec.kind {
            OperandKind::Rd => parts.push(register(rd)),
            OperandKind::Rs1 | OperandKind::Frs1 => {
                if memory {
                    // The base belongs to the operand that follows it.
                    memory_base = Some(rs1);
                } else {
                    parts.push(if spec.kind == OperandKind::Rs1 {
                        register(rs1)
                    } else {
                        float_register(rs1)
                    });
                }
            }
            OperandKind::Rs2 | OperandKind::Frs2 => parts.push(if spec.kind == OperandKind::Rs2 {
                register(rs2)
            } else {
                float_register(rs2)
            }),
            OperandKind::Frd => parts.push(float_register(rd)),
            OperandKind::Frs3 => parts.push(float_register(rs2)),
            kind => {
                let text = immediate(insn, kind, imm);
                parts.push(match memory_base.take() {
                    Some(base) => format!("{text}({})", register(base)),
                    None => text,
                });
            }
        }
    }
    if parts.is_empty() {
        insn.mnemonic.to_string()
    } else {
        format!("{} {}", insn.mnemonic, parts.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_words_render_with_abi_names() {
        assert_eq!(word(0x00c5_8533).as_deref(), Some("add a0, a1, a2"));
        assert_eq!(word(0x02a5_8513).as_deref(), Some("addi a0, a1, 42"));
        assert_eq!(word(0x00b6_2423).as_deref(), Some("sw a1, 8(a2)"));
        assert_eq!(word(0x0002_a503).as_deref(), Some("lw a0, 0(t0)"));
        assert_eq!(word(0x00c5_8463).as_deref(), Some("beq a1, a2, 8"));
        assert_eq!(word(0x1234_5537).as_deref(), Some("lui a0, 0x12345"));
        assert_eq!(word(0x0080_00ef).as_deref(), Some("jal ra, 8"));
    }

    #[test]
    fn compressed_words_render_too() {
        assert_eq!(word(0x0505).as_deref(), Some("c.addi a0, 1"));
        assert_eq!(word(0x41c8).as_deref(), Some("c.lw a0, 4(a1)"));
        assert_eq!(word(0x9002).as_deref(), Some("c.ebreak"));
    }

    #[test]
    fn float_words_use_f_register_names() {
        assert_eq!(word(0xf002_8053).as_deref(), Some("fmv.w.x f0, t0"));
        assert_eq!(word(0x0010_7153).as_deref(), Some("fadd.s f2, f0, f1"));
    }

    #[test]
    fn an_unknown_word_has_no_rendering() {
        assert_eq!(word(0x0000_0000), None);
    }
}
