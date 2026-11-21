# Architecture

Companion to [plan.md](plan.md). Decisions live in [docs/adr/](adr/); this file
holds the *shapes* — layers, contracts, crate boundaries, testing. Anything marked
**TBD** is an open question listed in `docs/plan.md` §6.

## 0. At a glance

One data path, three front ends:

```text
  isa/*.toml          content/overlay/
      │                     │
      └──────► xtask codegen ◄──────┘
                  │        │
                  ▼        ▼
             rvh-isa   rvh-content
                  │        │
  source ──► rvh-syntax ──► rvh-sem ◄──┘
                              │
                              ▼
                           rvh-asm ──► rvh-elf
                              │            │
                              ▼            │
                           rvh-sim ◄───────┘
                              │
                              ▼
                         rvh-session
                        ┌─────┼─────────┬──────────┐
                        ▼     ▼         ▼          ▼
                   rvh-wasm rvh-lsp  rvh-dap    rvh-cli
                        │     │         │          │
                     web page editor  debugger   terminal
```

`rvh-session` is the only crate the front ends touch. It owns one document, its
tree, its program, its image and one machine, so every front end sees the same
state after the same edit.

## 0.1 Where a change starts

| the change | start here |
| --- | --- |
| add or fix an instruction encoding | `isa/<ext>.toml`, then `cargo xtask codegen` |
| add a diagnostic or a fix | `crates/rvh-sem` (legality), `crates/rvh-session` (wording) |
| change what the editor shows on hover | `crates/rvh-session/src/hover`, `content/overlay/` |
| add an LSP or DAP method | `crates/rvh-lsp` / `crates/rvh-dap`, plus `docs/notes/` |
| debug something other than the built-in simulator | behind DAP; see [ADR-0017](adr/0017-dap-is-the-backend-boundary.md) |
| change editor behaviour | `editors/vscode/src` |
| change the playground | `web/src`, `crates/rvh-wasm` for new ABI entry points |
| change the ELF layout | `crates/rvh-elf`, then `xtask/verify-elf.sh` |

## 0.2 The same edit, three ways

Every front end produces the same three artifacts — CST, program, image — because
all three call `Session::set_source`.

```text
editor                    rvh-lsp                      rvh-session
  │  didChange             │                              │
  ├───────────────────────►│  set_source(text)            │
  │                        ├─────────────────────────────►│
  │                        │                              ├─ rvh-syntax::parse   → CST
  │                        │                              ├─ rvh-sem::resolve    → program
  │                        │                              └─ rvh-asm::assemble   → image
  │  textDocument/diagnostic (pull)                       │
  │◄───────────────────────┤                              │
```

```text
VS Code                     rvh-dap                       rvh-session / machine
  │  launch + breakpoints   │                              │
  ├────────────────────────►│  set_breakpoints             │
  │                         ├─────────────────────────────►│
  │  F5                     │  Session::load / run         │
  ├────────────────────────►├─────────────────────────────►│ Machine::run
  │  stopped(pc) + rvh/memory {pc, sp}                    │
  │◄────────────────────────┤◄─────── halt reason ─────────┤
  │  readMemory (base64)    │                              │
  ├────────────────────────►├─────────────────────────────►│ Machine::read_mem
```

```text
page                        rvh-wasm                      rvh-session / machine
  │  rvh_set_source         │                              │
  ├────────────────────────►├─────────────────────────────►│ set_source
  │  rvh_step               │                              │
  ├────────────────────────►├─────────────────────────────►│ run(1)
  │  rvh_registers(rvh_ptr) │                              │
  │◄────────────────────────┤◄──── zero-copy view ─────────┤
```

Breakpoints are image addresses, not lines: `rvh-dap` maps a source line through
`Image.source_map`, and a line with no instruction comes back `verified: false`.
`readMemory` clamps its length to the end of RAM, so a short read near the top of
memory is legal.

## 1. Layers

Each layer has a named artifact, so "IR questions" have concrete answers.

| L | name | artifact | consumed by |
| --- | --- | --- | --- |
| 0 | isa-db | normalized TOML + generated Rust tables | everything |
| 1 | CST | lossless syntax tree per document (hand-written, no dependency) | LSP, playground editor, asm |
| 2 | sem | symbols, resolved instructions, diagnostics, target env | LSP, playground, asm |
| 3 | image | bytes, sections, symbols, fixups, source map | asm → sim, ELF codec, disasm |
| 4 | machine | registers, memory, pc, devices, traps | sim, DAP |
| 5 | trace | per-step log + undo information | playground, DAP, explanation |

Rules that keep the layers honest:

- **syntax never knows the ISA.** `rvh-syntax` accepts any mnemonic and any
  operand list; it produces a CST and syntactic diagnostics only. Whether
  `c.addi` is legal on `rv32e` is a layer-2 question. This is why "different
  files are different ISAs" is not a parser problem at all — it is a *target
  resolution* problem.
- **sem is the only place that reads `(target, isa-db)`.** Both the LSP and the
  assembler call it; neither re-implements operand checking.
- **image is the only thing `machine` loads.** No front-end reaches into sim
  internals; no sim reaches back into source.

## 2. Contracts

### 2.1 `isa-db`

One TOML file per extension ([ADR-0005](adr/0005-data-formats.md)), compiled into
Rust tables at build time. Codegen, not a TOML parser in the wasm binary.

```toml
# isa/rv32i.toml (excerpt, shape only)
[[insn]]
id = "addi"
mnemonic = "addi"
ext = "I"
since = "2.0"
format = "I"                      # R/I/S/B/U/J/R4/...
encoding = "imm[11:0]|rs1|000|rd|0010011"
class = "alu-imm"                 # drives explanation templates
datapath = ["imm-gen", "alu", "regfile"]   # playground visualization

[insn.effects]
reads = ["rs1"]
writes = ["rd"]

[[insn.operands]]
kind = "rd"
class = "gpr"

[[insn.operands]]
kind = "rs1"
class = "gpr"

[[insn.operands]]
kind = "imm"
bits = 12
signed = true
```

Rules:

- `id` is stable and namespaced; **all content keys derive from it**
  (`insn.addi.summary`). Renaming is a breaking change.
- **Extension closure is computed, not declared.** `M` implies `Zmmul`, `F`
  implies `Zicsr`, `D` implies `F`, and so on. The target is normalised to its
  closure before any legality check, or every diagnostic is subtly wrong. (GNU
  `as` does exactly this: assembling with `m2p0` yields `_zmmul1p0` in
  `Tag_RISCV_arch`.)
- **Instruction selection is a separate step from encoding.**
  `--march=rv32ic` makes both GNU `as` and `llvm-mc` compress `addi a0,a0,1`
  into a 2-byte `c.addi`. So "mnemonic + operands → encoding" is not a total
  function: the encoder needs an explicit selection/compression pass with an off
  switch (teaching default: off, until `C` lands). This also means one source
  instruction can produce two bytes, which the source map and the footprint of
  one trace step must tolerate.
- adding an instruction is *data only* when its `format` and operand `class`es
  already exist.
- `class` is the scaling trick for content: explanation templates and hover
  layout are written per class, then overridden per instruction only when
  genuinely different (see §5).
- every field is parsed with `toml_edit`, so a malformed record reports a key
  span, not "parse error on line 1".

**Extensibility checklist.** Every one of these must be an additive change, and
the table records what you touch:

| extension axis | touch |
| --- | --- |
| new instruction, known format | one TOML record |
| new instruction format / operand kind | TOML + one arm in encoder/decoder |
| new architectural state (CSR, FP regfile, V regfile) | TOML + `Machine` state hook |
| new peripheral / board | `board.toml` only |
| new dialect (GNU / LLVM / vendor) | dialect table only |
| new host (browser / editor / CLI) | wasm ABI or CLI subcommand |
| new locale | overlay files only |

If a change requires touching two or more rows *and* the core, the boundary is
wrong — fix the boundary, not the case.

### 2.2 Target and board

`rvh.toml` (project) and `board.toml` (platform). Target resolution chain, in
priority order, with the winning source always visible in the UI:

| # | source | example |
| --- | --- | --- |
| 1 | `rvh.toml` | `arch = "rv32imac"` |
| 2 | `.attribute arch, "rv32i2p1"` | standard, `readelf -A` readable |
| 3 | `# rvh: arch=rv32i` file-header pragma | escape hatch for bare samples |
| 4 | build system (`-march=`) | matches the real build |
| 5 | default (`rv32i`) | must be shown, never silent |

`board.toml` carries what all three hosts need and nothing else:

```toml
[mem]
regions = [
  { name = "ram", base = 0x0,     size = 0x40000, rwx = true },
  { name = "uart", base = 0x10000000, size = 0x1000, mmio = "uart16550" },
]
reset = 0x0
stack_top = 0x3fff0
halt_on_ebreak = true      # ADR-0006

[console]
mmio = 0x10000000          # byte write = putchar; equals 16550 THR

[syscall]
abi = "none"               # "none" | "linux" | "spim"; table lives here, not in the core
```

The playground default follows [ADR-0006](adr/0006-playground-io.md). The console
sits at `0x10000000`. `ebreak` halts the machine. M4 ships no `ecall` table,
because printing already works through MMIO.

One file drives: LSP validation, playground memory view, simulator boot, DAP
memory map. This is the concrete test of the "harness" claim from `plan.md`.

### 2.3 `Image` vs ELF

```rust
struct Image {
    entry: u32,
    sections: Vec<Section>,   // name, addr, flags, data
    symbols: Vec<Symbol>,     // name, addr, size, kind, binding
    fixups: Vec<Fixup>,       // resolved at layout time
    source_map: Vec<Span>,    // addr range -> file/line/col
}
```

- **flat mode (playground, M2–M4):** assemble straight into `Image`; emit
  `.bin` + optional `.json` sidecar. No ELF, no linker, no relocation engine.
- **ELF mode (M6):** `object` crate for read/write; `gimli` later for DWARF line
  tables, CFI, and variables.
- **honesty rule:** even in flat mode, `la`/`li`/branch fixups must resolve with
  the same pcrel/`auipc+addi` behaviour a real toolchain uses. Teaching shortcuts
  that behave differently on real hardware are bugs, not simplifications.

Why not make ELF the contract: it forces an ELF writer and a linker into the
critical path of the *first* user story, and the playground needs neither. Why
not invent a private binary format: because we then cannot read `clang` output,
which is the whole point of M6.

### 2.4 `Trace` and undo

```rust
struct Step {
    pc: u32,
    insn: InsnId,          // index into generated tables
    changed: Vec<Change>,  // reg or word addr, old, new
}
```

- undo is reverse-application of `changed`; no per-step snapshots.
- ring buffer of N steps (configurable), spill-to-disk only for CI traces.
- explanation is *derived* from `Step` + isa-db metadata, never stored in the
  trace: the trace stays cheap, stable, and diffable against spike.

### 2.5 The C ABI

`rvh-wasm` exports plain C functions; there is no `wasm-bindgen`. A host writes a
string into linear memory, calls a function, and reads JSON back from the result
buffer. Register and RAM views are pointers into the same memory, so the UI can
read them every frame without a round trip.

| export | returns | meaning |
| --- | --- | --- |
| `rvh_alloc(len)` | pointer | host-owned buffer for a UTF-8 string |
| `rvh_dealloc(ptr, len)` | — | free it |
| `rvh_new(ptr, len)` | handle or 0 | session for a target such as `rv32i_zicsr` |
| `rvh_free(handle)` | — | destroy the session |
| `rvh_set_source(h, ptr, len)` | length | replace the document; result is diagnostics JSON |
| `rvh_step(h, n)` | length | execute `n` instructions; result is state JSON |
| `rvh_run(h, n)` | length | run to a halt or to the limit; state JSON |
| `rvh_undo(h, n)` | length | step back; state JSON |
| `rvh_reset(h)` | length | reload from the start; state JSON |
| `rvh_state(h)` | length | current state JSON |
| `rvh_hover(h, offset)` | length | `{"hover": ...}` |
| `rvh_result_ptr(h)` | pointer | the buffer written by the last call |
| `rvh_regs_ptr(h)` | pointer | 32 `u32` registers, zero-copy |
| `rvh_mem_ptr(h)` / `rvh_mem_len(h)` | pointer / length | RAM, zero-copy |

Rules that the implementation depends on:

- memory views are invalidated by wasm growth, so a host recreates them after
  every call that may allocate;
- a zero-length document is legal: the wrapper allocates one byte rather than
  passing a null pointer;
- `max_steps` is a `u32` on purpose: an `i64` would force every JavaScript caller
  to pass a `BigInt`.

The TypeScript wrapper lives in `web/src/engine.ts` and is covered by
`xtask/engine-smoke.mjs`, which drives it against the real artifact.

The same functions are callable natively; that is how the Rust tests exercise
them.

## 3. Crates

| crate | responsibility | depends on | state |
| --- | --- | --- | --- |
| `rvh-json` | JSON value, writer, optional parser | — | done |
| `rvh-isa` | isa-db schema, generated tables (RV32I, Zicsr, M, A), target/ext resolution; bases `rv32i` and `rv32e` (ADR-0015), 64-bit rejected (ADR-0012) | — | done |
| `rvh-syntax` | lexer, parser, lossless CST, syntax diagnostics | — | done |
| `rvh-sem` | symbols, resolution, operand checking, pseudos, lints/fixes | `rvh-isa`, `rvh-syntax` | done |
| `rvh-asm` | encoder, layout, `Image`, flat codec | `rvh-sem` | done; ELF at M6 |
| `rvh-sim` | `Machine`, decode/execute, memory, devices, trace | `rvh-isa`, `rvh-asm` | done |
| `rvh-session` | stateful facade: open / analyze / assemble / step / undo / hover, JSON snapshots | `rvh-syntax`, `rvh-sem`, `rvh-asm`, `rvh-sim`, `rvh-json` | done |
| `rvh-rpc` | `Content-Length` framing for LSP and DAP | — | done |
| `rvh-elf` | ELF32 reader and writer: headers, `PT_LOAD` segments, entry | — | done |
| `rvh-dap` | Debug Adapter Protocol over `rvh-session` | `rvh-session`, `rvh-json` | done |
| `rvh-wasm` | C ABI over `rvh-session` | `rvh-session` | done |
| `rvh-lsp` | LSP feature layer over `rvh-session` | `rvh-session`, `rvh-json` | done; wasm transport later |
| `rvh-cli` | native `rvh` binary | all of the above | done |
| `rvh-content` | overlay loading, locale fallback, content lint | `rvh-isa` | **not started** |
| `editors/vscode` | TS extension: language client, debug adapter, memory view, per-file target pin | `rvh lsp`, `rvh dap` | done |
| `web/` | Svelte 5 + TypeScript playground | `rvh-wasm` | done (M4b) |
| `content/` | `official/` (read-only) + `overlay/` | — | **not started** (M7) |

`rvh-syntax` must not depend on `rvh-isa`: this is a build-enforced version of
the "syntax never knows the ISA" rule, and it is what lets the tree-sitter
grammar live outside the repo without forking semantics.

## 4. Testing

Every gate in this table runs today; the commands are in
[docs/development.md](development.md).

| gate | command | covers |
| --- | --- | --- |
| unit tests | `cargo test --workspace` | 221 tests across fourteen crates |
| encoder differential | `./xtask/verify-asm.sh` | 65 checks: the operand matrix, M, A and the corpus programs, byte-identical to GNU `as` + `ld` |
| CST snapshots | `cargo test -p rvh-syntax` | 12 corpus files, diagnostics and fixes included |
| wasm ABI | `node xtask/wasm-smoke.mjs` | the real 258 KiB module driven from JavaScript |
| TS wrapper | `pnpm --dir web run smoke` | `engine.ts` against the real artifact |
| type check | `pnpm --dir web run check` | Svelte templates and scripts |
| browser | `pnpm --dir web run test:e2e` | Playwright, 6 cases over the built page |
| LSP wire | `./xtask/lsp-smoke.sh` | `rvh lsp` over stdio with framed messages |
| prose | `markdownlint-cli2 '**/*.md'` | zero warnings |

Not run yet, and why:

- `riscv-arch-test` signatures: the suite is not vendored;
- trace diff against spike: spike is not installed;
- property tests for `decode(encode(x))` and `undo(step(n)) == identity`: cheap
  candidates, not written.

## 5. Content and i18n

Two layers, joined by the ISA `id` or name:

- `content/official/` — upstream text, verbatim, pinned to a commit (not started);
- `content/overlay/<locale>/` — everything we write ourselves.

What exists today is the short form: `content/overlay/en/pseudos.toml`,
`directives.toml` and `registers.toml` are flat `key = "value"` maps of one-line
descriptions. `cargo xtask codegen` compiles them into `rvh-content`, and
`rvh-content` falls back to English key by key, so a translation can land one line
at a time. `content/overlay/zh-CN/pseudos.toml` is the first such file.

Why compile instead of load at runtime: the wasm binary has no TOML or JSON
parser for content, and rebuilding when content changes is free in a project whose
CI already regenerates ISA tables. A runtime overlay (user-installed content) is a
later feature, not a requirement.

Long prose will use Markdown with TOML front-matter (ADR-0005) and carry
`reviewer` and `status: draft|reviewed`. One-line descriptions do not need that
machinery; splitting the formats by length is the refinement this implementation
adds to ADR-0005.

Rendering in hover: the content line, then the derived facts from isa-db. Machine
translation is allowed but must be marked. Today only the pseudo table has
Chinese; everything else falls back.

**The workload trick:** because isa-db has a `class` field, explanation text and
hover layout are written once per class (`alu-imm`, `load`, `branch`, ...), with
per-instruction overrides only where the prose genuinely differs. Content volume
is therefore O(classes) + O(exceptions), not O(instructions) — which is the only
reason a two-language doc set is affordable for one maintainer.

## 6. LSP: done, and still speculative

The tiering below is what the implementation follows. Syntax diagnostics come from
`rvh-syntax`; everything semantic comes from `rvh-sem`; fixes travel with the
diagnostic, so the CLI and the editor offer the same repair.

| tier | examples | state |
| --- | --- | --- |
| syntax (L1) | unexpected character, missing comma, stray token | done |
| semantic (L2) | unknown mnemonic with did-you-mean, operand count and width, immediate range, CSR or extension missing from the target, duplicate label, register outside `rv32e` | done |
| lints with fixes | redundant sign extension, `addi rd, rs1, 0` → `mv`, unreachable code after `j`, mergeable `lui`/`addi` | not written |

Every diagnostic carries a stable code, and a repair where one exists; that is the
difference between this server and a highlighter. Two things sketched here remain
ideas rather than designs:

- declarative customization (`severity` and rule parameters in a config file,
  inline `# rvh: allow(rule)` suppression) — no config file exists yet;
- cross-language goto (`asm ↔ C`), which is a DWARF and line-marker feature, not a
  parsing one: it needs the build artifact and a way to find it.

## 7. Deliberate deferrals

- no linker (layout only), no relaxation beyond local branch range fixups
- no F/D until the sim is correct on IMC + Zicsr
- no CSR/privilege model beyond M-mode + `Zicsr` until something needs it
- no `tree-sitter-riscv` work until the L1 CST is stable (it is a *consumer* of
  the corpus, not a dependency)
- no MCP server until `rvh --json` is boring and stable
