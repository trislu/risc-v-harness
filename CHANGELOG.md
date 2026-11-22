# Changelog

One section per version, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/). The project stays `0.x` until the ISA
data format, the wasm ABI and the content format are stable for someone other than
the author ([ADR-0013](docs/adr/0013-vendor-extensions.md)).

## [0.1.0] - 2026-11-22

First public version: one core behind three faces — a language server, a debug
adapter and a browser playground, all over one assembler and one simulator.

### Added

#### ISA data

- RV32I, Zicsr, M and A: 65 instructions in `isa/*.toml`, compiled to Rust by
  `cargo xtask codegen`, with `--check` in CI.
- Targets `rv32i`, `rv32e` (the same instructions with 16 registers), `rv32im`,
  `rv32ia`, and combinations such as `rv32im_zicsr`.
- A file declares its own target with `.attribute arch` or `.option arch, ±ext`;
  precedence is file, then per-document pin, then workspace setting.

#### Assembler and simulator

- Hand-written lexer and line-oriented parser over a lossless CST, with per-line
  recovery and no dependency on an ISA.
- Diagnostics with fixes: did-you-mean for instructions, CSR names and directives;
  immediate ranges; `#` for `;` and `//`; "enable extension `m`".
- Encoder with branch and `jal` fixups, `la` pairs, `Image` with symbols and a
  source map; byte-identical to GNU `as` for every checked case.
- Simulator: RV32I, Zicsr, M (with the division corner cases) and A (lr/sc
  reservations, the nine AMOs), undo log, per-step trace, MMIO console, traps.
- ELF32 read and write; `rvh run prog.elf` runs statically linked binaries from a
  real toolchain.

#### Editor

- Language server: pull diagnostics with generation ids, hover (Markdown with an
  operand table), completion, go to definition as `LocationLink`, semantic tokens,
  quick fixes.
- Debug adapter: entry stop, breakpoints mapped through source lines,
  one-instruction `next`, register and memory reads, a `rvh/memory` event.
- VS Code extension: language client, `rvh` debug type, memory view in the debug
  sidebar, per-file target picker in the status bar.

#### Playground

- Svelte page over the wasm core: editor, registers, memory, console, diagnostics
  with one-click fixes, target picker.
- Deployed to GitHub Pages; the page is a demo, not a second IDE
  ([ADR-0016](docs/adr/0016-playground-is-a-demo-surface.md)).

#### Tooling

- `cargo xtask codegen` and `cargo xtask clean`.
- Differential checks: `verify-asm.sh` (65 checks), `verify-elf.sh`,
  `lsp-smoke.sh`, `wasm-smoke.mjs`, `engine-smoke.mjs`, Playwright end-to-end.
- Corpus `000`–`008` as a reading order, plus `900`–`902` for broken input.

#### Documentation

- ADR-0001 to ADR-0017, `docs/plan.md`, `docs/architecture.md`,
  `docs/usage.md`, `docs/development.md`.
- Recorded demo captures: a diagnostic and its fix, hover, go to definition,
  completion, the target picker, a debug session, and the playground.

#### Content and boards

- The Chinese overlay covers directives and registers as well as
  pseudo-instructions; an untranslated key still falls back to English.
- Board presets: `generic` (256 KiB, `sp` at `0x3_fff0`) and `tiny` (16 KiB, `sp`
  at `0x3ff0`). `rvh run --board <name>` selects one, the ELF loader checks each
  segment against it, and an unknown name lists the presets.

#### RV32C

- The assembler emits the 26 RV32C instructions, byte-identical to
  `as -march=rv32ic` (`verify-asm` compares all of them). A compressed operand in
  a register field outside `x8`..`x15`, an offset that is not a multiple of its
  alignment, or a base that is not `sp` is a diagnostic, not a truncated field.
- Running a compressed program halts with an illegal word: the decoder and the
  simulator learn the nine layouts next.

#### Groundwork

- [ADR-0018](docs/adr/0018-compressed-instructions-are-formats.md): RV32C
  instructions are nine more encoding formats in code, not a bit-table data
  model. `Format` carries `size()`, an instruction carries `size`, and the fixed
  fields of a compressed instruction use its two-bit quadrant as the opcode. The
  encoder, the decoder and the simulator still have to learn the nine formats;
  until they do, encoding a `C` format returns a stated error rather than a wrong
  word.

### Limits

- The C, F and D extensions are not implemented; `rv64i` and `rv64e` are named but
  rejected rather than silently run as 32-bit ([ADR-0012](docs/adr/0012-one-base-rv32i.md)).
- `.insn` is not implemented. Vendor extensions are data (`isa/x*.toml`), with no
  runtime plugin ([ADR-0013](docs/adr/0013-vendor-extensions.md)).
- No syscalls beyond the MMIO console; no pipeline or cycle model; one hart.
- No upstream ISA importer yet: `isa/` is hand-written from the specification.
- No DWARF. The debug adapter has source maps for code this assembler produced;
  binaries from other toolchains can run but not show source
  ([ADR-0017](docs/adr/0017-dap-is-the-backend-boundary.md)).
- No linker of its own: a cross-file `call` needs one.

### Notes

- Decisions and their corrections live in `docs/adr/`, not here: ADR-0009
  supersedes ADR-0008, ADR-0015 amends ADR-0012. This file records what a version
  contains and what it does not.
