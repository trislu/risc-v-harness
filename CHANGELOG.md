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

#### RV32F, first slice

- `flw`, `fsw`, `fadd.s`, `fsub.s`, `fmul.s`, `fdiv.s`, `fmv.w.x` and `fmv.x.w`,
  then `fsgnj`/`fsgnjn`/`fsgnjx`, `fmin`/`fmax`, `feq`/`flt`/`fle`, `fclass` and
  the four conversions: 21 instructions, byte-identical to `as -march=rv32if`.
  The machine gained 32 floating-point registers held as bit patterns, and the
  change log records their writes so undo reaches them.
- Conversions round to nearest, ties to even, and saturate at the integer bounds;
  `fclass` reports the ten categories; a NaN operand loses `fmin`/`fmax`.
- The fused multiply-add family (`fmadd.s`, `fmsub.s`, `fnmsub.s`, `fnmadd.s`)
  rounds its product and addend once, through Rust's `mul_add`; a test pins the
  case where one rounding and two roundings differ.
- `fcsr` is modelled: `frm` selects the rounding mode used by the conversions
  (nearest-even, toward zero, down, up, ties away), and `fflags` collects the
  sticky `NX`, `DZ` and `NV` bits. `fflags`, `frm` and `fcsr` are three views of
  one register. Arithmetic still rounds to nearest even, because the host `f32`
  cannot do otherwise.

#### Tracing

- `rvh run --trace` prints one line per step: `<pc> <word>`, then each write as
  `x5=0x…`, `f0=0x…` or `mem[0x…]=0x…`. The halt reason goes to stderr. This is
  the format the spike comparison is built on, so it is documented as an
  interface rather than debug output.

#### Trace comparison

- `xtask/verify-trace.sh` runs one ELF under `rvh` and spike and compares the
  result step by step: address, word, and every write. Six fixtures (integer,
  multiply/divide, compressed, atomics, single and double precision) agree. It is opt-in:
  set `RVH_SPIKE`, and it skips when spike is absent.
- Writes are now recorded even when the value does not change, so a trace shows
  what an instruction wrote rather than what it altered; undo is unaffected.
- The comparison found a real bug: `fcvt.d.w` and `fcvt.d.wu` took the
  single-precision path, because the check was "does the mnemonic end in `.d`" and
  `fcvt.d.w` ends in `.w`; the destination now decides.
- The double fixture also confirms NaN-boxing from the other side: `fcvt.d.s` of a
  register that holds a double is not a boxed single, so both simulators produce
  the canonical NaN.

#### Floating-point state

- `mstatus.FS` is modelled: an F or D instruction, or an access to `fflags`, `frm`
  or `fcsr`, traps as illegal while `FS` is off, and using the state sets it to
  dirty. The corpus examples enable it first, as a program must.
- Spike does the same, which is how the trace comparison found the gap.

#### Boards

- A `spike` preset puts RAM at `0x8000_0000`, so one ELF runs in both `rvh` and
  spike. Boards gained a `ram_base`, and every access — fetch, load, store, ELF
  segments — is checked against the board's window instead of a zero-based RAM
  (ADR-0019).

#### RV32D

- The 25 double-precision instructions: `fld`/`fsd`, arithmetic, symmetric sign
  injection, `fmin`/`fmax`, comparisons, `fclass`, the conversions (including
  `fcvt.s.d` and `fcvt.d.s`) and the fused forms with `fmt` 1. All byte-identical
  to `as -march=rv32id`.
- The floating-point file is 64 bits wide with NaN-boxing for singles, which the
  conversions and `fclass` read at their own width. This is also the layout RV64
  will use (ADR-0019).
- A measurement worth keeping: for `fcvt.s.w` GNU emits the dynamic rounding
  field, but for `fcvt.d.w` and `fcvt.d.s` it emits 0, so the table matches the
  toolchain bit for bit.
- Not yet: `fmv.x.d`/`fmv.d.x` are RV64-only; Q; and the trace diff against spike. The encodings
  use the dynamic-rounding field, which the simulator treats as
  round-to-nearest-even.

#### Raw encodings

- `.insn r|i|s|b|u|j` emits a word exactly as written, for instructions this
  assembler does not model. Four of the six forms match GNU `as` byte for byte
  (`verify-asm`); `b` and `j` follow the specification where GNU does not, which
  the dialect note records.

#### RV32C

- The assembler emits the 26 RV32C instructions, byte-identical to
  `as -march=rv32ic` (`verify-asm` compares all of them). A compressed operand in
  a register field outside `x8`..`x15`, an offset that is not a multiple of its
  alignment, or a base that is not `sp` is a diagnostic, not a truncated field.
- The simulator decodes and executes the 26 compressed instructions: it reads two
  bytes first, and steps by two when the low bits are not `11`. `rvh isa decode`
  tries 16 bits before 32. `009_compressed.s` runs through `.attribute arch`, and
  a decoder test pins every encoding GNU produced for the fixture.

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
