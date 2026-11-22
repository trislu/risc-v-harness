# Plan: RISC-V harness

> Working name. Tentative CLI / namespace: `rvh`.

## 1. What this is

A harness for RISC-V. It has one machine-readable ISA description, one semantic
core, and one execution core, then feeds four consumers.

| consumer | purpose | status |
| --- | --- | --- |
| web playground | assemble, single-step, explain, rewind — in a browser | **v1, alpha built** |
| language server | hover / diagnostics / fixes for `.s`, `.S` | **built (M5b)**; completion and goto pending |
| simulator | run real toolchain output (ELF), emit traces, serve CI | **built for our own images**; ELF input at M6 |
| debug adapter | DAP over the same core, in-process first, RSP later | not started (M6) |

The interesting part of the project is the shared middle, not any single
front-end. A consumer must be able to appear or disappear without touching the
core; that is the working definition of "harness" used throughout these docs.

**Non-goals for v1:** Linux boot, hypervisor, vector (V), formal/RVFI, any
server-side state or classroom SaaS.

## 2. First user story

A student, in a browser, with no install. They type RV32I assembly, press
**Step**, and see:

- registers / memory words that changed, highlighted, with `before → after`
- the current instruction decoded into its bit fields
- a one-sentence Chinese explanation of what just happened
- a rewind button that actually works
- error messages that suggest a fix ("immediate out of range [-2048, 2047];
  try `li` + `add`")

Constraints that follow from this, and are treated as hard:

- the core ships as **wasm** from day one; the playground has **no backend**
  and must run on a static host with default HTTP headers
  (no threads, no `SharedArrayBuffer`, no COOP/COEP)
- Chinese explanation text is needed at M4, so the content model starts early in
  a small form (one-liner summaries + operand semantics), not with full spec text
- the playground must stay *honest*: pseudo-instructions and `la` must resolve
  with the same pcrel behaviour the real toolchain uses

Minimum bar for M4 (i.e. what makes it worth using instead of Easy RISC-V):

- RV32I + the pseudo-instructions people actually write (`li`, `la`, `mv`, `j`,
  `ret`, `beqz`, ...), plus `.text/.data/.word/.byte/.string/.align/.globl`
- run / stop / step / **rewind** / reset-to-start
- register file and memory views, with changed cells highlighted `before → after`
- current instruction expanded into bit fields (opcode / rd / funct3 / rs1 / ...)
- one-line Chinese explanation per step, generated from isa-db metadata
- at least four non-trivial examples (hello world, loop+sum, recursion, string)
- shareable URL containing the program (no backend, no account)
- every error carries a suggested fix
- cold load under ~1.5 s on a mid-range laptop

### 2.1 What Easy RISC-V already covers

[Easy RISC-V](https://easyriscv.dram.page/) is a linear course, with one embedded
simulator panel per lesson. The target is `RV32I_Zicsr`, 45 instructions.

Lesson order, as published:

1. introduction;
2. first program: `addi x10, x0, 0x123`, then `ebreak`;
3. simulator controls: start, run, step, dump;
4. processor state: `pc`, general registers, the zero register;
5. instruction syntax: `addi rd, rs1, imm`;
6. arithmetic, logic, compare, shift;
7. interlude: load a large immediate with `lui` + `addi`;
8. branches and jumps, function call and return;
9. memory: `.word`, `.byte`, `lw`, `sw`;
10. functions: calling convention, stack, recursion;
11. interlude: numeric labels `1:`, `1b`, `1f`;
12. position-independent code: `auipc` + `jalr`;
13. privilege basics: M-mode, U-mode, CSR, exceptions;
14. write a minimal OS with `putchar` and `exit`.

Platform conventions, which we copy ([ADR-0006](adr/0006-playground-io.md)):

- output is a byte write to `0x10000000`, shown in a console panel;
- `ebreak` stops the machine;
- `ecall` appears only in lesson 14.

Each panel shows the editor, registers, memory, and console, with start, run, and
step. The course runs in the browser. It is released under CC0, per
[LinuxGizmos](https://linuxgizmos.com/easy-risc-v-provides-an-interactive-way-to-explore-the-risc-v-architecture/).
Confirm the license file in the repository before reusing text.

The list above comes from secondary sources: a
[Chinese write-up](https://yunpan.plus/t/26278-1-1) and the LinuxGizmos note.
Check the site before copying any lesson text.

We do not rebuild the curriculum. We translate it, keep it in Chinese, and add
four things: localized step explanations, rewind, the encoding bit-field view,
and errors that carry a fix. M4 covers lessons 1 to 12. Lessons 13 and 14 need
traps, CSR, and U-mode, so they land in M7.

## 3. Shape

```text
upstream ISA data (riscv-opcodes / unified-db)
            │
            ▼
        isa-db ──codegen──┬─▶ rvh-syntax   (lexer/parser/CST, tgt-agnostic)
                          └─▶ semantic tables (encoding, operand kinds, ext)
                                    │
.s/.S source ──▶ rvh-syntax ──▶ rvh-sem ──┬─▶ diagnostics (LSP + playground)
                                          └─▶ rvh-asm ──▶ Image ──┬─▶ rvh-sim ──▶ Trace
                                                                  │        │
                                    rvh-wasm ──▶ web playground ◀──┘        │
                                    rvh-cli / LSP / DAP ────────────────────┘
```

Two boundary decisions carry the architecture (see ADRs 0003/0004):

- `isa-db` is the **single source of truth** for instruction semantics; every
  other component consumes generated tables, never prose.
- `Image` (bytes + sections + symbols + source map) is the **internal** contract;
  ELF is an interchange format, not the core data model. This lets the playground
  assemble and run with no linker and no ELF writer, while still allowing the
  simulator to consume `clang`/`gcc` output later.

## 4. Milestones

Ordered by user value, not by component. Each one is independently demoable. The
`state` column answers "does it exist", which is the only interesting question.

| M | deliverable and acceptance | state |
| --- | --- | --- |
| **M0** | workspace, `isa-db` for RV32I + Zicsr, codegen, target resolution; `rvh isa lookup addi` prints encoding, extension, content key | **done** 2026-11-21 — 46 instructions, `xtask codegen --check`, 46/46 agreement with GNU `as`; extended to 54 with M on 2026-11-22 |
| **M1** | lexer, parser, lossless CST, per-line recovery, corpus with snapshots | **done** 2026-11-21 — corpus files to 008, spans tile the source, `rvh parse` |
| **M2** | encoder, `Image`, symbols, pseudos, `la`/`li`, branch fixups; byte-identical to GNU `as` over the operand matrix | **done** 2026-11-21 — byte-identical to GNU `as` + `ld` on 5 corpus programs |
| **M3** | `Machine`, RV32I + Zicsr, undo log, trace, console at `0x10000000` | **done** 2026-11-21 — 9 corpus programs run to checked results (55, 120, `"Hi\n"`, 1) |
| **M1.5** | diagnostics with fixes: did-you-mean, immediate hints, `;` and `//` as comments | **done** 2026-11-21 — fixes reach snapshots, CLI text, CLI JSON and LSP |
| **M4a** | `rvh-session` facade and `rvh-wasm` C ABI; one pipeline behind CLI, wasm and editor | **done** 2026-11-21 — wasm artifact driven from node |
| **M4b** | playground: Svelte 5 + Vite + CodeMirror, registers, memory, output, pc line | **done** 2026-11-22 — Playwright covers 6 cases, `svelte-check` clean |
| **M5a** | `hover(offset)` for instruction, pseudo, register, CSR, label, constant, directive and number | **done** 2026-11-21 — C ABI and `rvh hover` |
| **M5b** | LSP feature layer: stdio, UTF-16 positions, full sync, hover, code actions, diagnostics | **done** 2026-11-22 — `xtask/lsp-smoke.sh` drives the real binary. Pull diagnostics with a generation `resultId` ([ADR-0010](adr/0010-pull-diagnostics.md)) |
| **M5c** | VS Code client over `rvh lsp` | **done** 2026-11-22 — language id, stdio client bundled with esbuild, settings for the server path, target and log level, per-file target pin in the status bar, semantic tokens, `rvh dap` debug type with a memory view, and a VSIX that carries no `node_modules` |
| **M6** | ELF read/write, run `clang`-built ELF, DAP in-process | **M6a–M6c done** — `rvh-elf` reads and writes ELF32 RISC-V executables; `rvh run prog.elf` loads one, `rvh asm --elf out.elf` writes one. `rvh dap` speaks the Debug Adapter Protocol over stdio: entry stop, source breakpoints mapped through the image's source map, step, registers, `readMemory` for the extension's hex view, and console output forwarded as `output` events. The VS Code client contributes the `rvh` debug type. `xtask/verify-elf.sh` builds fixtures with GNU `as` + `ld` (including a cross-file `call`), checks that `readelf` parses our output, and round-trips it through our own loader |
| **M7** | C, M, A, F/D, board presets, content overlay v1, trace diff vs spike | **partial** — M: eight multiply/divide instructions with the ISA corner cases, byte-identical to `as -march=rv32im`. A: eleven atomics with `lr`/`sc` reservations, compared with `as -march=rv32ia`. A file declares its target with `.attribute arch` or `.option arch` ([ADR-0014](adr/0014-target-precedence.md)). Board presets `generic` and `tiny`, selectable with `rvh run --board`. Content overlay v1: `en` and `zh-CN` for pseudos, directives and registers, with a key-by-key fallback. C: assembler, decoder and simulator cover all 26 compressed instructions, byte-identical to `as -march=rv32ic`. `.insn` emits raw words for the six format letters ([ADR-0013](adr/0013-vendor-extensions.md)). F is complete: all 25 instructions assemble and run. Not started: `fcsr`, D, and the trace diff against spike |
| **M8** | Zed client, Neovim config; a tree-sitter grammar only if structural features need one | not started — costs a shim, not a rewrite |
| **M9** | 64-bit bases (`rv64i`, `rv64e`): XLEN becomes a parameter, RV64I instructions, ELFCLASS64, and the register-width assumptions in the simulator, the debugger and the editor | not started — needs an ADR on parameterising XLEN first ([ADR-0012](adr/0012-one-base-rv32i.md) refuses a silent 32-bit `rv64`) |

`M4` comes before `M5`: the playground is the product, and it forces the wasm
discipline early. `M8` keeps [ADR-0009](adr/0009-lsp-as-feature-layer.md)
honest without paying for it early.

The build has **no external dependencies**, by policy: a toolchain that vendors
nothing builds anywhere, and every dependency is an audit surface. `xtask`
therefore carries its own strict reader for the TOML subset used by
`isa/*.toml`. A richer reader can replace it later without touching the data
files.

## 5. Decision index

| # | decision | ADR |
| --- | --- | --- |
| 1 | core is Rust, shipped as wasm32 + native; shells (web, VS Code) are TS | [0001](adr/0001-core-is-rust-wasm.md) |
| 2 | hand-written Rust lexer/parser with lossless CST; tree-sitter is a separate, optional front-end | [0002](adr/0002-handwritten-parser.md) |
| 3 | `isa-db` is the single source of truth; upstream is `riscv-opcodes` first, `riscv-unified-db` later; pinned + diffed | [0003](adr/0003-isa-db.md) |
| 4 | `Image` is the internal contract, ELF is interchange; flat mode first | [0004](adr/0004-image-vs-elf.md) |
| 5 | one repository; explicit split points; committed codegen; code MIT/Apache-2.0, content CC-BY-4.0 | [0007](adr/0007-repo-shape.md) |
| 6 | educational UI, engineering core: the playground must behave like the real toolchain | — (this doc) |
| 7 | TOML for structured data, Markdown + TOML front-matter for prose, no YAML | [0005](adr/0005-data-formats.md) |
| 8 | playground I/O: MMIO console at `0x10000000`, `ebreak` halts, no `ecall` table in M4 | [0006](adr/0006-playground-io.md) |
| 9 | one LSP feature layer; wasm keeps the packaging cheap; VS Code is the reference client | [0009](adr/0009-lsp-as-feature-layer.md) |
| 10 | pull diagnostics with a generation `resultId`; no workspace index, no refresh | [0010](adr/0010-pull-diagnostics.md) |
| 11 | the LSP layer stays hand-written; the framework trade-off and its revisit conditions are recorded | [0011](adr/0011-lsp-framework-tradeoff.md) |
| 12 | 64-bit bases are rejected until a wider datapath exists; `rv32i` was the only usable base | [0012](adr/0012-one-base-rv32i.md) |
| 15 | `rv32e` reuses the `i` table and adds a 16-register limit | [0015](adr/0015-rv32e-is-a-register-limit.md) |
| 13 | vendor `X*` extensions are data (`isa/x*.toml`); `.insn` lands in M7; runtime ISA overlay deferred with triggers | [0013](adr/0013-vendor-extensions.md) |
| 14 | a document's target is decided by the file, then a pin, then the setting | [0014](adr/0014-target-precedence.md) |
| 16 | the playground is a zero-install demo surface with a target picker; breakpoints stay in the editor | [0016](adr/0016-playground-is-a-demo-surface.md) |
| 17 | DAP is the editor-facing boundary; the simulator is the first backend, hardware arrives via gdb or RSP | [0017](adr/0017-dap-is-the-backend-boundary.md) |
| 18 | compressed instructions are formats in code, with `size` carried by the instruction | [0018](adr/0018-compressed-instructions-are-formats.md) |

Architecture detail, data contracts, testing strategy: [docs/architecture.md](architecture.md).
The records themselves, with statuses and the rules for correcting one:
[docs/adr/README.md](adr/README.md).

## 6. Open questions

Still undecided, roughly in the order they will bite:

1. **`.S` / cpp strategy** — opaque `#` lines vs built-in mini-cpp vs external cpp
   with line-marker mapping. Decides whether "goto the `#define`" is possible at all.
2. **Dialect matrix** — GNU `as` vs LLVM `mc` vs vendor assemblers: `.align`
   semantics, `;` (separator or comment), `#` vs `//`, `.option arch` support.
   Started empirically in [docs/notes/dialect-matrix.md](notes/dialect-matrix.md):
   GNU `as` and `llvm-mc` already disagree silently on `jal ra, 0x10`
   (absolute vs relative) and on `//` comments. Canonical dialect = GNU `as`;
   anything else is data, and divergence must be an error, never different bytes.
3. **Reverse execution** — write-log undo (chosen shape) vs periodic snapshots;
   confirm after profiling memory writes on realistic programs.
4. **Content licensing** — storage is settled ([ADR-0005](adr/0005-data-formats.md):
   Markdown + TOML front-matter). Still open: whether `content/official/` is
   vendored at build time or fetched, and which terms allow it. The licenses of
   `riscv-unified-db`, `riscv-opcodes`, `riscv-isa-manual`, and `riscv-arch-test`
   **must be verified before any text is vendored**. Easy RISC-V is CC0, so its
   lesson text is reusable once the repository confirms the license.
5. **Completion context model** — syntactic only (operand position → valid
   register/imm/symbol) vs value-range aware (e.g. offer `li` when an imm does
   not fit). Probably starts syntactic, then adds the fix-oriented path.
6. **Instruction selection default** — when `C` lands, do we compress by default
   (matching both real assemblers) or keep the teaching default at 1 instruction
   = 1 word? Proposal: default off for `rv32i` targets, on for any target that
   includes `C`, and always visible in the encoding panel.
7. **`ecall` numbering for the OS lesson** — deferred by
   [ADR-0006](adr/0006-playground-io.md) until Easy RISC-V's source or lesson
   text is reachable. Align with it, then record the table in `board.toml`.
8. **Playground name and branding** — separate from the repository and CLI names
   ([ADR-0007](adr/0007-repo-shape.md)). It should read well in Chinese, and
   it need not share the `rvh` prefix.
9. **Section placement** — `la` into `.data` cannot be byte-compared with GNU
   `ld` yet, because section addresses are a linker policy. Our single-file
   layout puts sections contiguously from 0. Decide at M6 whether to emulate a
   common link script (`-Ttext=0 -Tdata=…`) or to accept the difference.
10. **Content layer** — started: one-line descriptions for pseudos, directives
    and registers come from `content/overlay/<locale>/*.toml` through
    `rvh-content`, with key-by-key English fallback. Still open: per-instruction
    summaries, class-level explanation templates, `zh-CN` for directives and
    registers, and the Markdown-plus-front-matter shape for long prose.
11. **Upstream ISA data** — [ADR-0003](adr/0003-isa-db.md) promises an
    importer plus `isa/upstream.lock`; today `isa/` is hand-written. Decide
    whether to build the importer, and what the lock file should pin.
12. **WASM transport for the LSP** — [ADR-0009](adr/0009-lsp-as-feature-layer.md)
    wants the LSP loop hosted in wasm for the in-process VS Code client. Only the
    native stdio transport exists so far.
13. **Links in hover and diagnostics** — links must point at an official site: a
    repository-relative `file:` URI only works for people who cloned the tree, and
    documentation bundled into the VSIX is not what a documentation link is for.
    No official page exists per instruction yet, so hover carries no links and
    diagnostics carry no `codeDescription.href`. Empty is the deliberate state;
    when the site publishes a stable URL shape, map each message code to a page and
    add a "See also" line to the hover template.

## 7. References

Upstream truth: [RISC-V ISA Explorer](https://tech.riscv.org/isa-explorer/),
[riscv-unified-db](https://github.com/riscv/riscv-unified-db),
[riscv-opcodes](https://github.com/riscv-software-src/riscv-opcodes).

Prior art: [Easy RISC-V](https://easyriscv.dram.page/)
([source](https://github.com/dramforever/easyriscv)), spike, QEMU, OpenOCD,
[tree-sitter-riscv](https://github.com/omeyenburg/tree-sitter-riscv).
