//! Decoding and execution of the 16-bit instructions (ADR-0018).

use rvh_isa::Insn;

/// One decoded compressed instruction: the table entry plus its fields.
#[derive(Debug, Clone, Copy)]
pub struct Decoded {
    pub insn: &'static Insn,
    pub rd: u8,
    pub rs1: u8,
    pub rs2: u8,
    pub imm: i32,
}

fn sign_extend(value: u32, bits: u32) -> i32 {
    let shift = 32 - bits;
    ((value << shift) as i32) >> shift
}

fn insn(mnemonic: &str) -> Option<&'static Insn> {
    rvh_isa::lookup(mnemonic)
}

/// Decode one 16-bit word, or `None` when the quadrant and fields name nothing.
#[must_use]
pub fn decode(halfword: u16) -> Option<Decoded> {
    let word = u32::from(halfword);
    let op = word & 0x3;
    let funct3 = (word >> 13) & 0x7;
    let rd5 = ((word >> 7) & 0x1f) as u8;
    let rs2_5 = ((word >> 2) & 0x1f) as u8;
    let rd3 = 8 + ((word >> 2) & 0x7) as u8;
    let rs2_3 = 8 + ((word >> 2) & 0x7) as u8;
    let rs1_3 = 8 + ((word >> 7) & 0x7) as u8;

    let (name, rd, rs1, rs2, imm) = match (op, funct3) {
        (0, 0b000) => {
            let value = ((word >> 11) & 0x3) << 4
                | ((word >> 7) & 0xf) << 6
                | ((word >> 6) & 0x1) << 2
                | ((word >> 5) & 0x1) << 3;
            // The all-zero encoding is reserved: `c.addi4spn` needs a nonzero imm.
            if value == 0 {
                return None;
            }
            ("c.addi4spn", rd3, 2, 0, value as i32)
        }
        (0, 0b010) | (0, 0b110) => {
            let value =
                ((word >> 10) & 0x7) << 3 | ((word >> 6) & 0x1) << 2 | ((word >> 5) & 0x1) << 6;
            if funct3 == 0b010 {
                ("c.lw", rd3, rs1_3, 0, value as i32)
            } else {
                ("c.sw", 0, rs1_3, rs2_3, value as i32)
            }
        }
        (1, 0b000) | (1, 0b010) => {
            let value = sign_extend(((word >> 12) & 0x1) << 5 | ((word >> 2) & 0x1f), 6);
            if funct3 == 0b000 {
                ("c.addi", rd5, rd5, 0, value)
            } else {
                ("c.li", rd5, 0, 0, value)
            }
        }
        (1, 0b011) => {
            if rd5 == 2 {
                let value = ((word >> 12) & 0x1) << 9
                    | ((word >> 3) & 0x3) << 7
                    | ((word >> 5) & 0x1) << 6
                    | ((word >> 2) & 0x1) << 5
                    | ((word >> 6) & 0x1) << 4;
                ("c.addi16sp", 2, 2, 0, sign_extend(value, 10))
            } else {
                let value = sign_extend(((word >> 12) & 0x1) << 5 | ((word >> 2) & 0x1f), 6);
                ("c.lui", rd5, 0, 0, value << 12)
            }
        }
        (1, 0b001) | (1, 0b101) => {
            let value = ((word >> 12) & 0x1) << 11
                | ((word >> 11) & 0x1) << 4
                | ((word >> 9) & 0x3) << 8
                | ((word >> 8) & 0x1) << 10
                | ((word >> 7) & 0x1) << 6
                | ((word >> 6) & 0x1) << 7
                | ((word >> 3) & 0x7) << 1
                | ((word >> 2) & 0x1) << 5;
            if funct3 == 0b001 {
                ("c.jal", 1, 0, 0, sign_extend(value, 12))
            } else {
                ("c.j", 0, 0, 0, sign_extend(value, 12))
            }
        }
        (1, 0b100) => match (word >> 10) & 0x3 {
            0b00 | 0b01 => {
                let value = ((word >> 12) & 0x1) << 5 | ((word >> 2) & 0x1f);
                let name = if (word >> 10) & 0x3 == 0 {
                    "c.srli"
                } else {
                    "c.srai"
                };
                (name, 0, rs1_3, 0, value as i32)
            }
            0b10 => {
                let value = sign_extend(((word >> 12) & 0x1) << 5 | ((word >> 2) & 0x1f), 6);
                ("c.andi", 0, rs1_3, 0, value)
            }
            _ => {
                let name = match (word >> 5) & 0x3 {
                    0b00 => "c.sub",
                    0b01 => "c.xor",
                    0b10 => "c.or",
                    _ => "c.and",
                };
                (name, rs1_3, rs1_3, rs2_3, 0)
            }
        },
        (1, 0b110) | (1, 0b111) => {
            let value = ((word >> 12) & 0x1) << 8
                | ((word >> 10) & 0x3) << 3
                | ((word >> 5) & 0x3) << 6
                | ((word >> 3) & 0x3) << 1
                | ((word >> 2) & 0x1) << 5;
            let name = if funct3 == 0b110 { "c.beqz" } else { "c.bnez" };
            (name, 0, rs1_3, 0, sign_extend(value, 9))
        }
        (2, 0b000) => {
            let value = ((word >> 12) & 0x1) << 5 | ((word >> 2) & 0x1f);
            ("c.slli", rd5, rd5, 0, value as i32)
        }
        (2, 0b010) => {
            let value =
                ((word >> 12) & 0x1) << 5 | ((word >> 4) & 0x7) << 2 | ((word >> 2) & 0x3) << 6;
            ("c.lwsp", rd5, 2, 0, value as i32)
        }
        (2, 0b100) => {
            let high = (word >> 12) & 0x1;
            if high == 1 {
                match (rd5, rs2_5) {
                    (0, 0) => ("c.ebreak", 0, 0, 0, 0),
                    (_, 0) => ("c.jalr", 1, rd5, 0, 0),
                    _ => ("c.add", rd5, 0, rs2_5, 0),
                }
            } else {
                match (rd5, rs2_5) {
                    (0, _) => return None,
                    (_, 0) => ("c.jr", 0, rd5, 0, 0),
                    _ => ("c.mv", rd5, 0, rs2_5, 0),
                }
            }
        }
        (2, 0b110) => {
            let value = ((word >> 9) & 0xf) << 2 | ((word >> 7) & 0x3) << 6;
            ("c.swsp", 0, 2, rs2_5, value as i32)
        }
        _ => return None,
    };
    Some(Decoded {
        insn: insn(name)?,
        rd,
        rs1,
        rs2,
        imm,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every word from the fixture in `scripts/verify-asm.sh`, with the operands
    /// GNU's disassembler printed.
    #[test]
    fn the_gnu_words_decode() {
        let cases: [(u16, &str); 26] = [
            (0x0808, "c.addi4spn"),
            (0x41c8, "c.lw"),
            (0xc1c8, "c.sw"),
            (0x0505, "c.addi"),
            (0x2009, "c.jal"),
            (0x450d, "c.li"),
            (0x717d, "c.addi16sp"),
            (0x6511, "c.lui"),
            (0x810d, "c.srli"),
            (0x850d, "c.srai"),
            (0x890d, "c.andi"),
            (0x8d0d, "c.sub"),
            (0x8d2d, "c.xor"),
            (0x8d4d, "c.or"),
            (0x8d6d, "c.and"),
            (0xa009, "c.j"),
            (0xc109, "c.beqz"),
            (0xfd7d, "c.bnez"),
            (0x050e, "c.slli"),
            (0x4512, "c.lwsp"),
            (0x8502, "c.jr"),
            (0x852e, "c.mv"),
            (0x9002, "c.ebreak"),
            (0x9502, "c.jalr"),
            (0x952e, "c.add"),
            (0xc22a, "c.swsp"),
        ];
        for (word, mnemonic) in cases {
            let decoded = decode(word).unwrap_or_else(|| panic!("no decode for {word:#06x}"));
            assert_eq!(decoded.insn.mnemonic, mnemonic, "{word:#06x}");
        }
    }

    #[test]
    fn the_operands_match_the_encoding() {
        let addi = decode(0x0505).expect("c.addi a0, 1");
        assert_eq!((addi.rd, addi.imm), (10, 1));
        let lw = decode(0x41c8).expect("c.lw a0, 4(a1)");
        assert_eq!((lw.rd, lw.rs1, lw.imm), (10, 11, 4));
        let lui = decode(0x6511).expect("c.lui a0, 4");
        assert_eq!((lui.rd, lui.imm), (10, 4 << 12));
        let beqz = decode(0xc109).expect("c.beqz a0, +2");
        assert_eq!((beqz.rs1, beqz.imm), (10, 2));
        let jr = decode(0x8502).expect("c.jr a0");
        assert_eq!((jr.rs1, jr.rs2), (10, 0));
    }

    #[test]
    fn an_invalid_quadrant_is_none() {
        assert!(decode(0x0000).is_none(), "all-zero is not an instruction");
    }
}
