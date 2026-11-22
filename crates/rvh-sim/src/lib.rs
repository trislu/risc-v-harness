//! RV32I execution.
//!
//! The machine is deliberately small and plain: registers, a flat RAM, 4096
//! CSR slots, a byte-wide console, and an undo log. Every executed step records
//! what it changed, so the playground can step backwards for free.
//!
//! Timing is not modelled. One instruction takes one step; there are no stalls,
//! no pipeline, and no traps beyond `ebreak` and `ecall` (see ADR-0006).

#![forbid(unsafe_code)]

use rvh_asm::Image;
use rvh_isa::Insn;

/// A board preset: the memory map a machine runs on.
///
/// The values are code rather than `board.toml` (ADR-0006 sketched the file) until
/// a third preset or a second consumer appears; a board is three numbers, and a
/// TOML file for three numbers would only add a schema to keep in sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Board {
    pub name: &'static str,
    pub ram_size: usize,
    pub console: u32,
    pub stack_top: u32,
}

impl Board {
    /// 256 KiB of RAM at address 0, the console at `0x1000_0000`.
    pub const GENERIC: Self = Self {
        name: "generic",
        ram_size: 0x4_0000,
        console: 0x1000_0000,
        stack_top: 0x3_fff0,
    };

    /// 16 KiB of RAM: small enough that an overflow shows up early.
    pub const TINY: Self = Self {
        name: "tiny",
        ram_size: 0x4_000,
        console: 0x1000_0000,
        stack_top: 0x3ff0,
    };
}

/// Every preset, in the order the CLI lists them.
pub const BOARDS: &[Board] = &[Board::GENERIC, Board::TINY];

/// Look a preset up by name.
#[must_use]
pub fn board(name: &str) -> Option<&'static Board> {
    BOARDS.iter().find(|board| board.name == name)
}

/// RAM size of the generic board: 256 KiB at address 0.
pub const RAM_SIZE: usize = Board::GENERIC.ram_size;
/// Byte-wide console address of the generic board. A store here prints the low byte.
pub const CONSOLE: u32 = Board::GENERIC.console;
/// Initial `sp` of the generic board.
pub const STACK_TOP: u32 = Board::GENERIC.stack_top;
/// How many steps of undo history to keep.
pub const TRACE_LIMIT: usize = 100_000;

/// One piece of state a step changed, with the value to restore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Reg { index: u8, old: u32 },
    Mem { addr: u32, old: u8 },
}

/// Why the machine stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HaltReason {
    Ebreak { pc: u32 },
    Ecall { pc: u32 },
    Illegal { pc: u32, word: u32 },
    Fault { pc: u32, addr: u32 },
    StepLimit,
}

/// One executed step, enough to undo it and to explain it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub pc: u32,
    pub word: u32,
    /// `None` when the word could not be decoded.
    pub insn: Option<&'static Insn>,
    pub changes: Vec<Change>,
    /// Console length before the step, so undo can truncate output.
    pub output_len: usize,
}

/// The machine.
#[derive(Debug, Clone)]
pub struct Machine {
    pc: u32,
    regs: [u32; 32],
    ram: Vec<u8>,
    csrs: Vec<u32>,
    output: Vec<u8>,
    steps: u64,
    halt: Option<HaltReason>,
    trace: Vec<Step>,
    /// Address reserved by `lr.w`, consumed by the next `sc.w`.
    reservation: Option<u32>,
    /// Memory map the machine was built on.
    board: &'static Board,
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

impl Machine {
    /// Load a statically linked ELF32 RISC-V executable.
    ///
    /// Memory is flat: every segment must fall inside [`RAM_SIZE`]. That is
    /// enough for the bare-metal images this project builds (`-Ttext=0`) and is
    /// checked rather than assumed.
    pub fn from_elf(bytes: &[u8]) -> Result<Self, String> {
        Self::from_elf_on(&Board::GENERIC, bytes)
    }

    /// Load an ELF onto another board, checking every segment against its RAM.
    pub fn from_elf_on(board: &'static Board, bytes: &[u8]) -> Result<Self, String> {
        let executable = rvh_elf::parse(bytes).map_err(|error| error.to_string())?;
        let entry = executable.entry as usize;
        if entry >= board.ram_size {
            return Err(format!(
                "entry {:#010x} is outside the {} KiB of RAM on `{}`",
                executable.entry,
                board.ram_size / 1024,
                board.name
            ));
        }

        let mut machine = Self::with_board(board);
        for segment in &executable.segments {
            let start = segment.addr as usize;
            let end = start
                .checked_add(segment.mem_size as usize)
                .ok_or_else(|| {
                    format!("segment at {:#010x} has an impossible size", segment.addr)
                })?;
            if end > board.ram_size {
                return Err(format!(
                    "segment at {:#010x}..{:#010x} does not fit in {} KiB of RAM on `{}`",
                    segment.addr,
                    end,
                    board.ram_size / 1024,
                    board.name
                ));
            }
            machine.ram[start..start + segment.data.len()].copy_from_slice(&segment.data);
        }
        machine.pc = executable.entry;
        Ok(machine)
    }

    pub fn new() -> Self {
        Self::with_board(&Board::GENERIC)
    }

    /// A machine with another memory map: RAM size, console address and initial
    /// `sp` come from `board`.
    pub fn with_board(board: &'static Board) -> Self {
        let mut regs = [0u32; 32];
        regs[2] = board.stack_top;
        Self {
            pc: 0,
            regs,
            ram: vec![0; board.ram_size],
            csrs: vec![0; 4096],
            output: Vec::new(),
            steps: 0,
            halt: None,
            trace: Vec::new(),
            reservation: None,
            board,
        }
    }

    /// The board this machine runs on.
    #[must_use]
    pub fn board(&self) -> &'static Board {
        self.board
    }

    /// Load an image and reset the machine.
    pub fn load(&mut self, image: &Image) {
        self.pc = image.entry;
        for section in &image.sections {
            let start = section.addr as usize;
            let end = start + section.data.len();
            if end > self.ram.len() {
                self.ram.resize(end, 0);
            }
            self.ram[start..end].copy_from_slice(&section.data);
        }
    }

    #[must_use]
    pub fn pc(&self) -> u32 {
        self.pc
    }

    #[must_use]
    pub fn reg(&self, index: usize) -> u32 {
        self.regs.get(index).copied().unwrap_or(0)
    }

    #[must_use]
    pub fn output(&self) -> &[u8] {
        &self.output
    }

    /// Console output as text. Invalid UTF-8 is replaced, never dropped.
    #[must_use]
    pub fn output_text(&self) -> String {
        String::from_utf8_lossy(&self.output).into_owned()
    }

    #[must_use]
    pub fn steps(&self) -> u64 {
        self.steps
    }

    #[must_use]
    pub fn halt(&self) -> Option<HaltReason> {
        self.halt
    }

    #[must_use]
    pub fn halted(&self) -> bool {
        self.halt.is_some()
    }

    #[must_use]
    pub fn trace(&self) -> &[Step] {
        &self.trace
    }

    /// The whole register file, for a zero-copy view in a host.
    #[must_use]
    pub fn regs(&self) -> &[u32; 32] {
        &self.regs
    }

    /// Backing RAM, for a zero-copy view in a host.
    #[must_use]
    pub fn ram(&self) -> &[u8] {
        &self.ram
    }

    #[must_use]
    pub fn read_mem(&self, addr: u32, len: u32) -> Option<&[u8]> {
        let start = addr as usize;
        let end = start.checked_add(len as usize)?;
        self.ram.get(start..end)
    }

    /// Execute one instruction. Returns false when the machine has stopped.
    pub fn step(&mut self) -> bool {
        if self.halt.is_some() {
            return false;
        }
        let pc = self.pc;
        let Some(word) = self.fetch(pc) else {
            self.halt = Some(HaltReason::Fault { pc, addr: pc });
            self.steps += 1;
            self.trace.push(Step {
                pc,
                word: 0,
                insn: None,
                changes: Vec::new(),
                output_len: self.output.len(),
            });
            return false;
        };

        // A compressed instruction is half a word; until the decoder learns the
        // nine RVC layouts (ADR-0018), say so instead of decoding two
        // instructions as one.
        if rvh_isa::Insn::is_compressed(word) {
            self.halt = Some(HaltReason::Illegal { pc, word });
            self.steps += 1;
            self.trace.push(Step {
                pc,
                word,
                insn: None,
                changes: Vec::new(),
                output_len: self.output.len(),
            });
            return false;
        }

        let insn = rvh_isa::decode(word).first().copied();
        let output_len = self.output.len();
        let mut changes = Vec::new();
        let outcome = match insn {
            Some(insn) => self.execute(insn, word, &mut changes),
            None => Err(HaltReason::Illegal { pc, word }),
        };

        self.steps += 1;
        self.trace.push(Step {
            pc,
            word,
            insn,
            changes,
            output_len,
        });
        if self.trace.len() > TRACE_LIMIT {
            self.trace.remove(0);
        }

        match outcome {
            Ok(next) => {
                self.pc = next;
                true
            }
            Err(reason) => {
                self.halt = Some(reason);
                false
            }
        }
    }

    /// Run until the machine stops or `max_steps` is reached.
    pub fn run(&mut self, max_steps: u64) -> Option<HaltReason> {
        while self.halt.is_none() && self.steps < max_steps {
            self.step();
        }
        if self.halt.is_none() {
            self.halt = Some(HaltReason::StepLimit);
        }
        self.halt
    }

    /// Undo the last step. Returns false when the history is empty.
    pub fn undo(&mut self) -> bool {
        let Some(step) = self.trace.pop() else {
            return false;
        };
        for change in step.changes.iter().rev() {
            match change {
                Change::Reg { index, old } => self.regs[*index as usize] = *old,
                Change::Mem { addr, old } => {
                    if let Some(slot) = self.ram.get_mut(*addr as usize) {
                        *slot = *old;
                    }
                }
            }
        }
        self.output.truncate(step.output_len);
        self.pc = step.pc;
        self.steps = self.steps.saturating_sub(1);
        self.halt = None;
        true
    }

    fn fetch(&self, addr: u32) -> Option<u32> {
        self.read_value(addr, 4, false)
    }

    fn read_value(&self, addr: u32, size: usize, signed: bool) -> Option<u32> {
        let mut value = 0u32;
        for offset in 0..size {
            let byte = self.read_byte(addr.checked_add(offset as u32)?)?;
            value |= u32::from(byte) << (8 * offset);
        }
        if signed {
            let shift = 32 - 8 * size as u32;
            value = (((value << shift) as i32) >> shift) as u32;
        }
        Some(value)
    }

    fn store(&mut self, addr: u32, size: usize, value: u32, changes: &mut Vec<Change>) -> bool {
        // A store may invalidate the reservation: `sc.w` is allowed to fail
        // spuriously, and this keeps the model simple.
        self.reservation = None;
        for offset in 0..size {
            let Some(target) = addr.checked_add(offset as u32) else {
                return false;
            };
            if !self.write_byte(target, (value >> (8 * offset)) as u8, changes) {
                return false;
            }
        }
        true
    }

    fn read_byte(&self, addr: u32) -> Option<u8> {
        if addr == self.board.console {
            return Some(0);
        }
        self.ram.get(addr as usize).copied()
    }

    fn write_byte(&mut self, addr: u32, value: u8, changes: &mut Vec<Change>) -> bool {
        if addr == self.board.console {
            self.output.push(value);
            return true;
        }
        match self.ram.get_mut(addr as usize) {
            Some(slot) => {
                if *slot != value {
                    changes.push(Change::Mem { addr, old: *slot });
                    *slot = value;
                }
                true
            }
            None => false,
        }
    }

    fn write_reg(&mut self, index: usize, value: u32, changes: &mut Vec<Change>) {
        if index == 0 {
            return;
        }
        let old = self.regs[index];
        if old == value {
            return;
        }
        changes.push(Change::Reg {
            index: index as u8,
            old,
        });
        self.regs[index] = value;
    }

    fn csr_read(&self, index: u32) -> u32 {
        self.csrs.get(index as usize).copied().unwrap_or(0)
    }

    fn csr_write(&mut self, index: u32, value: u32) {
        if let Some(slot) = self.csrs.get_mut(index as usize) {
            *slot = value;
        }
    }

    fn execute(
        &mut self,
        insn: &'static Insn,
        word: u32,
        changes: &mut Vec<Change>,
    ) -> Result<u32, HaltReason> {
        let pc = self.pc;
        let next = pc.wrapping_add(4);
        let rd = ((word >> 7) & 0x1f) as usize;
        let rs1 = ((word >> 15) & 0x1f) as usize;
        let rs2 = ((word >> 20) & 0x1f) as usize;
        let a = self.regs[rs1];
        let b = self.regs[rs2];
        let imm_i = imm_i(word);
        let csr = (word >> 20) & 0xfff;
        let zimm = (word >> 15) & 0x1f;

        match insn.mnemonic {
            "add" => self.write_reg(rd, a.wrapping_add(b), changes),
            "sub" => self.write_reg(rd, a.wrapping_sub(b), changes),
            "sll" => self.write_reg(rd, a.wrapping_shl(b & 31), changes),
            "slt" => self.write_reg(rd, u32::from((a as i32) < (b as i32)), changes),
            "sltu" => self.write_reg(rd, u32::from(a < b), changes),
            "xor" => self.write_reg(rd, a ^ b, changes),
            "srl" => self.write_reg(rd, a.wrapping_shr(b & 31), changes),
            "sra" => self.write_reg(rd, ((a as i32) >> (b & 31)) as u32, changes),
            "or" => self.write_reg(rd, a | b, changes),
            "and" => self.write_reg(rd, a & b, changes),

            // RV32M. The divides follow the ISA's defined corner cases rather
            // than Rust's: division by zero yields all ones, and INT_MIN / -1
            // overflows to INT_MIN with a zero remainder.
            "mul" => self.write_reg(rd, a.wrapping_mul(b), changes),
            "mulh" => self.write_reg(rd, mul_high_signed(a, b), changes),
            "mulhsu" => self.write_reg(rd, mul_high_signed_unsigned(a, b), changes),
            "mulhu" => self.write_reg(rd, mul_high_unsigned(a, b), changes),
            "div" => {
                let (quotient, _) = divide(a, b);
                self.write_reg(rd, quotient, changes);
            }
            "divu" => {
                let (quotient, _) = divide_unsigned(a, b);
                self.write_reg(rd, quotient, changes);
            }
            "rem" => {
                let (_, remainder) = divide(a, b);
                self.write_reg(rd, remainder, changes);
            }
            "remu" => {
                let (_, remainder) = divide_unsigned(a, b);
                self.write_reg(rd, remainder, changes);
            }

            "addi" => self.write_reg(rd, a.wrapping_add(imm_i as u32), changes),
            "slti" => self.write_reg(rd, u32::from((a as i32) < imm_i), changes),
            "sltiu" => self.write_reg(rd, u32::from(a < imm_i as u32), changes),
            "xori" => self.write_reg(rd, a ^ imm_i as u32, changes),
            "ori" => self.write_reg(rd, a | imm_i as u32, changes),
            "andi" => self.write_reg(rd, a & imm_i as u32, changes),
            "slli" => {
                self.write_reg(rd, a.wrapping_shl((word >> 20) & 0x1f), changes);
            }
            "srli" => {
                self.write_reg(rd, a.wrapping_shr((word >> 20) & 0x1f), changes);
            }
            "srai" => {
                let shift = (word >> 20) & 0x1f;
                self.write_reg(rd, ((a as i32) >> shift) as u32, changes);
            }

            // RV32A. The address must be word-aligned; the reservation is the
            // simulator's single-hart model of `lr`/`sc`.
            "lr.w" => {
                if a % 4 != 0 {
                    return Err(HaltReason::Fault { pc, addr: a });
                }
                let Some(value) = self.read_value(a, 4, false) else {
                    return Err(HaltReason::Fault { pc, addr: a });
                };
                self.reservation = Some(a);
                self.write_reg(rd, value, changes);
            }
            "sc.w" => {
                if a % 4 != 0 {
                    return Err(HaltReason::Fault { pc, addr: a });
                }
                let reserved = self.reservation == Some(a);
                if reserved && !self.store(a, 4, b, changes) {
                    return Err(HaltReason::Fault { pc, addr: a });
                }
                self.write_reg(rd, u32::from(!reserved), changes);
            }
            "amoswap.w" | "amoadd.w" | "amoxor.w" | "amoand.w" | "amoor.w" | "amomin.w"
            | "amomax.w" | "amominu.w" | "amomaxu.w" => {
                if a % 4 != 0 {
                    return Err(HaltReason::Fault { pc, addr: a });
                }
                let Some(old) = self.read_value(a, 4, false) else {
                    return Err(HaltReason::Fault { pc, addr: a });
                };
                let new = match insn.mnemonic {
                    "amoswap.w" => b,
                    "amoadd.w" => old.wrapping_add(b),
                    "amoxor.w" => old ^ b,
                    "amoand.w" => old & b,
                    "amoor.w" => old | b,
                    "amomin.w" => {
                        if (old as i32) < (b as i32) {
                            old
                        } else {
                            b
                        }
                    }
                    "amomax.w" => {
                        if (old as i32) > (b as i32) {
                            old
                        } else {
                            b
                        }
                    }
                    "amominu.w" => {
                        if old < b {
                            old
                        } else {
                            b
                        }
                    }
                    _ => {
                        if old > b {
                            old
                        } else {
                            b
                        }
                    }
                };
                if !self.store(a, 4, new, changes) {
                    return Err(HaltReason::Fault { pc, addr: a });
                }
                self.write_reg(rd, old, changes);
            }

            "lb" | "lh" | "lw" | "lbu" | "lhu" => {
                let (size, signed) = match insn.mnemonic {
                    "lb" => (1, true),
                    "lh" => (2, true),
                    "lw" => (4, false),
                    "lbu" => (1, false),
                    _ => (2, false),
                };
                let addr = a.wrapping_add(imm_i as u32);
                match self.read_value(addr, size, signed) {
                    Some(value) => self.write_reg(rd, value, changes),
                    None => return Err(HaltReason::Fault { pc, addr }),
                }
            }
            "sb" | "sh" | "sw" => {
                let size = match insn.mnemonic {
                    "sb" => 1,
                    "sh" => 2,
                    _ => 4,
                };
                let addr = a.wrapping_add(imm_s(word) as u32);
                if !self.store(addr, size, b, changes) {
                    return Err(HaltReason::Fault { pc, addr });
                }
            }

            "beq" | "bne" | "blt" | "bge" | "bltu" | "bgeu" => {
                let taken = match insn.mnemonic {
                    "beq" => a == b,
                    "bne" => a != b,
                    "blt" => (a as i32) < (b as i32),
                    "bge" => (a as i32) >= (b as i32),
                    "bltu" => a < b,
                    _ => a >= b,
                };
                if taken {
                    return Ok(pc.wrapping_add(imm_b(word) as u32));
                }
            }

            "lui" => self.write_reg(rd, imm_u(word), changes),
            "auipc" => self.write_reg(rd, pc.wrapping_add(imm_u(word)), changes),
            "jal" => {
                self.write_reg(rd, next, changes);
                return Ok(pc.wrapping_add(imm_j(word) as u32));
            }
            "jalr" => {
                let target = a.wrapping_add(imm_i as u32) & !1;
                self.write_reg(rd, next, changes);
                return Ok(target);
            }

            "fence" => {}

            "ecall" => return Err(HaltReason::Ecall { pc }),
            "ebreak" => return Err(HaltReason::Ebreak { pc }),

            "csrrw" => {
                let old = self.csr_read(csr);
                self.csr_write(csr, a);
                self.write_reg(rd, old, changes);
            }
            "csrrs" => {
                let old = self.csr_read(csr);
                if a != 0 {
                    self.csr_write(csr, old | a);
                }
                self.write_reg(rd, old, changes);
            }
            "csrrc" => {
                let old = self.csr_read(csr);
                if a != 0 {
                    self.csr_write(csr, old & !a);
                }
                self.write_reg(rd, old, changes);
            }
            "csrrwi" => {
                let old = self.csr_read(csr);
                self.csr_write(csr, zimm);
                self.write_reg(rd, old, changes);
            }
            "csrrsi" => {
                let old = self.csr_read(csr);
                if zimm != 0 {
                    self.csr_write(csr, old | zimm);
                }
                self.write_reg(rd, old, changes);
            }
            "csrrci" => {
                let old = self.csr_read(csr);
                if zimm != 0 {
                    self.csr_write(csr, old & !zimm);
                }
                self.write_reg(rd, old, changes);
            }

            // Every mnemonic in the database has an arm above. Reaching this
            // point means the database and the simulator are out of sync; report
            // it instead of panicking, so a test can catch it.
            _ => return Err(HaltReason::Illegal { pc, word }),
        }
        Ok(next)
    }
}

fn mul_high_signed(a: u32, b: u32) -> u32 {
    ((i64::from(a as i32) * i64::from(b as i32)) >> 32) as u32
}

fn mul_high_signed_unsigned(a: u32, b: u32) -> u32 {
    ((i64::from(a as i32) * i64::from(b)) >> 32) as u32
}

fn mul_high_unsigned(a: u32, b: u32) -> u32 {
    ((u64::from(a) * u64::from(b)) >> 32) as u32
}

/// RISC-V `div`/`rem`: division by zero yields `-1` and the dividend.
fn divide(a: u32, b: u32) -> (u32, u32) {
    let (x, y) = (a as i32, b as i32);
    if y == 0 {
        (u32::MAX, a)
    } else if x == i32::MIN && y == -1 {
        (i32::MIN as u32, 0)
    } else {
        ((x / y) as u32, (x % y) as u32)
    }
}

fn divide_unsigned(a: u32, b: u32) -> (u32, u32) {
    // `checked_div` keeps the zero case explicit instead of tripping the lint
    // that flags a hand-written guard before a division.
    a.checked_div(b)
        .map_or((u32::MAX, a), |quotient| (quotient, a % b))
}

fn imm_i(word: u32) -> i32 {
    (word as i32) >> 20
}

fn imm_s(word: u32) -> i32 {
    (((word as i32) >> 25) << 5) | ((word >> 7) & 0x1f) as i32
}

fn imm_b(word: u32) -> i32 {
    let value = (((word >> 31) & 1) << 12)
        | (((word >> 7) & 1) << 11)
        | (((word >> 25) & 0x3f) << 5)
        | (((word >> 8) & 0xf) << 1);
    ((value << 19) as i32) >> 19
}

fn imm_u(word: u32) -> u32 {
    word & 0xffff_f000
}

fn imm_j(word: u32) -> i32 {
    let value = (((word >> 31) & 1) << 20)
        | (((word >> 12) & 0xff) << 12)
        | (((word >> 20) & 1) << 11)
        | (((word >> 21) & 0x3ff) << 1);
    ((value << 11) as i32) >> 11
}

#[cfg(test)]
mod tests {
    use super::*;
    use rvh_isa::Target;

    fn machine(source: &str) -> Machine {
        let tree = rvh_syntax::parse(source);
        let program = rvh_sem::resolve(&tree, &Target::parse("rv32i_zicsr").expect("target"));
        assert!(program.diagnostics.is_empty(), "{:?}", program.diagnostics);
        let (image, diagnostics) = rvh_asm::assemble(&program);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let mut machine = Machine::new();
        machine.load(&image);
        machine
    }

    fn run(source: &str) -> Machine {
        let mut machine = machine(source);
        machine.run(10_000);
        machine
    }

    #[test]
    fn arithmetic() {
        let machine = run("li a0, 7\nli a1, 5\nadd a2, a0, a1\nsub a3, a0, a1\nebreak\n");
        assert_eq!(machine.reg(12), 12);
        assert_eq!(machine.reg(13), 2);
        assert_eq!(machine.halt(), Some(HaltReason::Ebreak { pc: 16 }));
    }

    #[test]
    fn loop_sums_1_to_10() {
        let source = "\
            li a0, 0\n\
            li t0, 1\n\
            li t1, 11\n\
        loop:\n\
            add a0, a0, t0\n\
            addi t0, t0, 1\n\
            blt t0, t1, loop\n\
            ebreak\n";
        let machine = run(source);
        assert_eq!(machine.reg(10), 55);
    }

    #[test]
    fn memory_round_trip() {
        let machine = run("li t0, 0x100\nli t1, 0x12345678\nsw t1, 0(t0)\nlw a0, 0(t0)\nebreak\n");
        assert_eq!(machine.reg(10), 0x1234_5678);
    }

    #[test]
    fn console_write_collects_output() {
        let machine =
            run("li t0, 0x10000000\nli t1, 72\nsb t1, 0(t0)\nli t1, 105\nsb t1, 0(t0)\nebreak\n");
        assert_eq!(machine.output_text(), "Hi");
    }

    #[test]
    fn csr_write_and_read() {
        let machine = run("li t0, 0x1234\ncsrw mscratch, t0\ncsrr a0, mscratch\nebreak\n");
        assert_eq!(machine.reg(10), 0x1234);
    }

    #[test]
    fn jal_and_ret() {
        let machine = run("jal ra, f\nebreak\nf:\naddi a0, x0, 42\nret\n");
        assert_eq!(machine.reg(10), 42);
        assert_eq!(machine.halt(), Some(HaltReason::Ebreak { pc: 4 }));
    }

    #[test]
    fn undo_restores_registers_memory_and_output() {
        let mut machine = machine("li t0, 0x100\nli t1, 1\nsw t1, 0(t0)\nli t0, 0x10000000\nli t1, 65\nsb t1, 0(t0)\nebreak\n");
        machine.run(100);
        assert_eq!(machine.output_text(), "A");

        let steps = machine.steps();
        // Undo the `ebreak` first: it changed nothing but cleared the halt.
        assert!(machine.undo());
        assert_eq!(machine.output_text(), "A");
        assert!(!machine.halted());
        // Now undo the store that printed.
        assert!(machine.undo());
        assert_eq!(machine.output_text(), "");
        assert_eq!(machine.steps(), steps - 2);

        while machine.undo() {}
        assert_eq!(machine.reg(5), 0);
        assert_eq!(machine.pc(), 0);
        assert_eq!(machine.steps(), 0);
    }

    #[test]
    fn illegal_word_stops_the_machine() {
        let mut machine = machine("nop\n");
        machine.ram[0..4].copy_from_slice(&0xffff_ffffu32.to_le_bytes());
        machine.run(10);
        assert!(matches!(machine.halt(), Some(HaltReason::Illegal { .. })));
    }

    #[test]
    fn step_limit_is_reported() {
        let mut machine = machine("1:\nj 1b\n");
        let halt = machine.run(20);
        assert_eq!(halt, Some(HaltReason::StepLimit));
        assert_eq!(machine.steps(), 20);
    }
}

#[cfg(test)]
mod elf_tests {
    use super::*;

    /// Minimal ELF32 executable with one PT_LOAD segment.
    fn sample(entry: u32, addr: u32, code: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0u8; 52 + 32];
        bytes[..4].copy_from_slice(&[0x7f, b'E', b'L', b'F']);
        bytes[4] = 1;
        bytes[5] = 1;
        bytes[6] = 1;
        bytes[16..18].copy_from_slice(&2u16.to_le_bytes());
        bytes[18..20].copy_from_slice(&243u16.to_le_bytes());
        bytes[24..28].copy_from_slice(&entry.to_le_bytes());
        bytes[28..32].copy_from_slice(&52u32.to_le_bytes());
        bytes[42..44].copy_from_slice(&32u16.to_le_bytes());
        bytes[44..46].copy_from_slice(&1u16.to_le_bytes());
        bytes[52..56].copy_from_slice(&1u32.to_le_bytes());
        bytes[56..60].copy_from_slice(&84u32.to_le_bytes());
        bytes[60..64].copy_from_slice(&addr.to_le_bytes());
        bytes[64..68].copy_from_slice(&addr.to_le_bytes());
        bytes[68..72].copy_from_slice(&(code.len() as u32).to_le_bytes());
        bytes[72..76].copy_from_slice(&(code.len() as u32).to_le_bytes());
        bytes[76..80].copy_from_slice(&5u32.to_le_bytes());
        bytes.extend_from_slice(code);
        bytes
    }

    #[test]
    fn runs_a_segment_from_an_elf() {
        // addi a0, zero, 7 ; ebreak
        let code = [
            0x13, 0x05, 0x70, 0x00, // addi a0, zero, 7
            0x73, 0x00, 0x10, 0x00, // ebreak
        ];
        let mut machine = Machine::from_elf(&sample(0, 0, &code)).expect("loads");
        machine.run(10);
        assert_eq!(machine.reg(10), 7);
        assert!(matches!(machine.halt(), Some(HaltReason::Ebreak { .. })));
        assert_eq!(machine.steps(), 2);
    }

    #[test]
    fn loads_at_the_address_the_segment_claims() {
        let code = [0x13, 0x05, 0x70, 0x00, 0x73, 0x00, 0x10, 0x00];
        let mut machine = Machine::from_elf(&sample(0x100, 0x100, &code)).expect("loads");
        assert_eq!(machine.pc(), 0x100);
        machine.run(10);
        assert_eq!(machine.reg(10), 7);
    }

    #[test]
    fn a_segment_outside_ram_is_an_error() {
        let code = [0x73, 0x00, 0x10, 0x00];
        // Four bytes starting two bytes before the end of RAM.
        match Machine::from_elf(&sample(0, 0x3_fffe, &code)) {
            Err(error) => assert!(error.contains("does not fit"), "{error}"),
            Ok(_) => panic!("a segment past the end of RAM must be rejected"),
        }
    }

    #[test]
    fn a_bad_file_is_an_error_not_a_panic() {
        assert!(Machine::from_elf(b"not an elf").is_err());
    }
}

#[cfg(test)]
mod multiply_tests {
    use super::*;
    use rvh_isa::Target;

    /// Assemble and run a program under a target that includes `m`.
    fn run(source: &str) -> Machine {
        let tree = rvh_syntax::parse(source);
        let program = rvh_sem::resolve(&tree, &Target::parse("rv32i_m").expect("target"));
        assert!(program.diagnostics.is_empty(), "{:?}", program.diagnostics);
        let (image, diagnostics) = rvh_asm::assemble(&program);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let mut machine = Machine::new();
        machine.load(&image);
        machine.run(1000);
        machine
    }

    #[test]
    fn multiplication_takes_the_low_word() {
        let machine = run(
            "_start:\n    li t0, 0x10000\n    li t1, 0x10000\n    mul a0, t0, t1\n    ebreak\n",
        );
        assert_eq!(machine.reg(10), 0, "the product's low word is zero");
    }

    #[test]
    fn the_high_words_respect_sign() {
        // -1 * -1 = 1 -> high word 0; -1 * 1 = -1 -> high word 0xffffffff.
        let machine = run(
            "_start:\n    li t0, -1\n    li t1, 1\n    mulh a0, t0, t0\n    mulh a1, t0, t1\n    mulhu a2, t0, t0\n    ebreak\n",
        );
        assert_eq!(machine.reg(10), 0, "(-1 * -1) high word");
        assert_eq!(machine.reg(11), 0xffff_ffff, "(-1 * 1) high word");
        assert_eq!(
            machine.reg(12),
            0xffff_fffe,
            "unsigned: (2^32-1)^2 high word"
        );
    }

    #[test]
    fn signed_and_unsigned_halves_differ() {
        // mulhsu of -1 and 0xffffffff: (-1 * 4294967295) >> 32 == -1.
        let machine =
            run("_start:\n    li t0, -1\n    li t1, -1\n    mulhsu a0, t0, t1\n    ebreak\n");
        assert_eq!(machine.reg(10), 0xffff_ffff);
    }

    #[test]
    fn division_by_zero_yields_minus_one_and_the_dividend() {
        let machine = run(
            "_start:\n    li t0, 7\n    div a0, t0, zero\n    rem a1, t0, zero\n    divu a2, t0, zero\n    remu a3, t0, zero\n    ebreak\n",
        );
        assert_eq!(machine.reg(10) as i32, -1);
        assert_eq!(machine.reg(11), 7);
        assert_eq!(machine.reg(12) as i32, -1);
        assert_eq!(machine.reg(13), 7);
    }

    #[test]
    fn the_overflow_case_does_not_trap() {
        let machine = run(
            "_start:\n    li t0, -2147483648\n    li t1, -1\n    div a0, t0, t1\n    rem a1, t0, t1\n    ebreak\n",
        );
        assert_eq!(machine.reg(10), 0x8000_0000);
        assert_eq!(machine.reg(11), 0);
    }

    #[test]
    fn remainders_take_the_dividend_sign() {
        let machine = run(
            "_start:\n    li t0, -7\n    li t1, 3\n    rem a0, t0, t1\n    div a1, t0, t1\n    remu a2, t0, t1\n    ebreak\n",
        );
        assert_eq!(machine.reg(10) as i32, -1);
        assert_eq!(machine.reg(11) as i32, -2);
        assert_eq!(machine.reg(12), 0, "(2^32-7) is divisible by 3");
    }
}

#[cfg(test)]
mod atomic_tests {
    use super::*;
    use rvh_isa::Target;

    fn run(source: &str) -> Machine {
        let tree = rvh_syntax::parse(source);
        let program = rvh_sem::resolve(&tree, &Target::parse("rv32ia").expect("target"));
        assert!(program.diagnostics.is_empty(), "{:?}", program.diagnostics);
        let (image, diagnostics) = rvh_asm::assemble(&program);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let mut machine = Machine::new();
        machine.load(&image);
        machine.run(100);
        machine
    }

    #[test]
    fn a_reserved_load_and_store_succeeds() {
        let machine = run(
            "_start:\n    li t0, 0x100\n    li t1, 7\n    sw t1, 0(t0)\n    lr.w a0, (t0)\n    li t1, 9\n    sc.w a1, t1, (t0)\n    lw a2, 0(t0)\n    ebreak\n",
        );
        assert_eq!(machine.reg(10), 7, "lr returned the old word");
        assert_eq!(machine.reg(11), 0, "sc succeeded");
        assert_eq!(machine.reg(12), 9, "the store landed");
    }

    #[test]
    fn a_store_between_load_and_store_breaks_the_reservation() {
        let machine = run(
            "_start:\n    li t0, 0x100\n    lr.w a0, (t0)\n    li t1, 1\n    sw t1, 0(t0)\n    sc.w a1, t1, (t0)\n    ebreak\n",
        );
        assert_eq!(machine.reg(11), 1, "sc failed and wrote 1");
    }

    #[test]
    fn the_amo_operations_return_the_old_word() {
        let machine = run(
            "_start:\n    li t0, 0x100\n    li t1, 5\n    sw t1, 0(t0)\n    li t2, 3\n    amoadd.w a0, t2, (t0)\n    amoswap.w a1, t2, (t0)\n    lw a2, 0(t0)\n    ebreak\n",
        );
        assert_eq!(machine.reg(10), 5, "amoadd saw 5");
        assert_eq!(machine.reg(11), 8, "amoswap saw the sum");
        assert_eq!(machine.reg(12), 3, "and left 3 behind");
    }

    #[test]
    fn the_min_max_operations_respect_sign() {
        let machine = run(
            "_start:\n    li t0, 0x100\n    li t1, -1\n    sw t1, 0(t0)\n    li t2, 1\n    amomin.w a0, t2, (t0)\n    amomaxu.w a1, t2, (t0)\n    ebreak\n",
        );
        assert_eq!(machine.reg(10) as i32, -1, "signed: -1 is the minimum");
        assert_eq!(machine.reg(11), 0xffff_ffff, "unsigned: -1 is the maximum");
    }

    #[test]
    fn an_unaligned_atomic_faults() {
        let mut machine = run("_start:\n    li t0, 0x101\n    lr.w a0, (t0)\n    ebreak\n");
        assert!(matches!(machine.halt(), Some(HaltReason::Fault { .. })));
        assert_eq!(machine.reg(10), 0);
        let _ = &mut machine;
    }
}

#[cfg(test)]
mod board_tests {
    use super::*;

    static CUSTOM: Board = Board {
        name: "custom",
        ram_size: 0x40,
        console: 0x20,
        stack_top: 0x30,
    };

    #[test]
    fn the_generic_board_keeps_the_documented_map() {
        let board = board("generic").expect("generic");
        assert_eq!(board.ram_size, 0x4_0000);
        assert_eq!(board.stack_top, 0x3_fff0);
        assert_eq!(board.console, 0x1000_0000);
    }

    #[test]
    fn a_named_board_changes_the_ram_and_the_stack() {
        let tiny = board("tiny").expect("tiny");
        let machine = Machine::with_board(tiny);
        assert_eq!(machine.board().name, "tiny");
        assert_eq!(machine.reg(2), tiny.stack_top);
        assert_eq!(machine.ram.len(), tiny.ram_size);
    }

    #[test]
    fn the_console_address_follows_the_board() {
        let mut machine = Machine::with_board(&CUSTOM);
        assert!(machine.store(CUSTOM.console, 1, u32::from(b'A'), &mut Vec::new()));
        assert_eq!(machine.output(), b"A");
        // The generic console address is not special here, and it is outside
        // this board's RAM, so a store there fails instead of printing.
        assert!(!machine.store(0x1000_0000, 1, u32::from(b'B'), &mut Vec::new()));
    }

    #[test]
    fn an_unknown_board_is_none() {
        assert!(board("no-such-board").is_none());
    }
}
