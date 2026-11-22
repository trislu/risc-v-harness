# ADR-0018: Compressed instructions are formats, not a second data model

- Status: accepted
- Date: 2026-11-22
- Related: [ADR-0003](0003-isa-db.md), [ADR-0004](0004-image-vs-elf.md)

## Context

RV32C instructions are 16 bits wide and two bytes aligned, and their fields are
scrambled: `c.addi4spn` keeps its immediate in bits 12:5 with two bits at 6:5 and
two at 12:11 twice over, while `c.lw` splits a different immediate across three
runs of bits. The six 32-bit formats (`R`, `I`, `S`, `B`, `U`, `J`) are already
implemented as code in `rvh-asm` and `rvh-isa`, with the TOML carrying only the
format name and the fixed fields.

The question: should a 16-bit instruction be described as data (a bit table in
TOML), or as another format implemented in code?

## Decision

**Another format, implemented in code — nine of them.**

1. `Format` gains `Ci`, `Cr`, `Css`, `Ciw`, `Cl`, `Cs`, `Ca`, `Cb`, `Cj`, and
   `Format::size()` returns 2 for those and 4 for the existing six. An instruction
   carries `size` through codegen, so the assembler and the loader know how many
   bytes to emit without asking the encoder twice.
2. `isa/c.toml` carries what the other tables carry: mnemonic, `format`, `opcode`
   (the two-bit quadrant for a compressed instruction), `funct3` where a quadrant
   needs it, operands and class. The bit scrambling lives in the encoder and the
   decoder, next to the six formats that already work that way.
3. A declarative bit table was rejected: it would be a second description language
   used by nine formats while the other six stay code, and every compressed
   immediate still needs range and alignment checks in Rust.
4. Registers in most compressed formats are three bits naming `x8`..`x15`; the
   encoder maps a register number to that field and reports the constraint
   instead of truncating.

## Consequences

- One instruction table, one encoder, one decoder; a compressed instruction is not
  a parallel universe. `rvh isa decode` tries 16 bits first, then 32.
- The layout pass must handle mixed sizes: a section's addresses depend on each
  instruction's size, and a branch into the middle of a 32-bit instruction has to
  be rejected rather than guessed.
- Source maps carry two-byte granularity, so a breakpoint can land on a compressed
  instruction.
- The simulator's step advances by 2 or 4 depending on the low two bits of the
  fetched halfword, which is also how real hardware decides.
