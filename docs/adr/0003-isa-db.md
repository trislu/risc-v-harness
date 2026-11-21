# ADR-0003: `isa-db` is the single source of truth for instruction semantics

- **Status:** accepted
- **Date:** 2026-11-21
- **Context of:** `docs/plan.md` §0 (extension ISAs, text) and §1 (ISA sets)

## Context

Four components need to know what an instruction *is*: the language server (hover,
completion, operand checks, immediate ranges, which extension an instruction
belongs to), the assembler (encoding), the disassembler (decoding), and the
simulator (execution semantics). If each carries its own table, they will disagree
within a month. Disagreement between "what the LSP accepts" and "what the
simulator runs" is the worst failure mode for a teaching tool.

Upstream already publishes machine-readable descriptions: `riscv-opcodes`
(encoding-oriented, community-maintained, broad coverage) and `riscv-unified-db`
(the RISC-V International machine-readable database behind the ISA manual and the
Sail/Spike models; richer, heavier, Ruby-based generators).

## Decision

- A single normalized format, `isa/*.toml`, one file per extension, is the source
  of truth. Everything else is generated from it at build time: Rust tables, JSON
  for tooling, content key stubs.
- The normalized format is **ours**, with upstream as an *importer*. An upstream
  change then lands as a diff in our schema, not as a fork of our code.
- **Upstream order:** `riscv-opcodes` first (simple to ingest, encoding-complete,
  extension membership is encoded in the file layout), with `riscv-unified-db` as
  the planned replacement once normative text, CSR definitions, and versions are
  needed. Both are importers behind the same schema; neither is a runtime
  dependency.
- **Provenance is pinned and diffed:** `isa/upstream.lock` records upstream
  commit + importer version. CI periodically re-runs the importer and opens a PR
  with the resulting diff. "Trustworthy" is implemented as a diff, not as a
  disclaimer.
- The DB carries, per instruction: stable `id`, mnemonic, extension + version,
  format, encoding pattern, operand kinds/classes/signedness/alignment, implicit
  effects, a semantic `class`, and datapath tags for visualization.
- ISA selection is a **target** = base (rv32e/i, rv64i) + extension set with
  versions + privilege set, expressed in canonical form
  (`rv64i2p1_m2p0_a2p1_c2p0_zicsr2p0`, single letters in IMAFDQLCBKJTPVH order,
  `_z*` lexicographic). Profiles (RVA22/RVA23/RVB23) are presets over this, not a
  new concept.
- `.option arch, ±ext` is modelled as a mid-file *increment* to the target. That
  makes "this instruction needs extension M" diagnosable and fixable.
- **Extension closure is computed** (`M` implies `Zmmul`, `F` implies `Zicsr`,
  ...) before any legality check.
- **Instruction selection is separate from encoding.** With `C` enabled both GNU
  `as` and `llvm-mc` compress opportunistically, so the encoder needs a selection
  pass with an off switch.
- The format is TOML, per [ADR-0005](0005-data-formats.md).

## Consequences

### Good

- adding an instruction, an extension, or a whole ISA family is a data change;
- LSP, assembler, disassembler, and simulator cannot drift (single table);
- provenance/trust is auditable per instruction (commit + URL + version);
- the same IDs key the content and i18n layers, so docs cannot orphan silently.

### Bad

- we own a schema and an importer, and upstream format churn is our problem;
- the TOML is a build input, so regeneration is a codegen step in CI;
- content licensing must be verified before any upstream *prose* is vendored.
  Encoding data and normative text have different terms. **Action item:** verify
  licenses of `riscv-unified-db`, `riscv-opcodes`, `riscv-isa-manual`, and
  `riscv-arch-test` before M4 ships any bundled text.

## Alternatives considered

**Consume upstream at runtime** (parse `riscv-opcodes` files at startup). Rejected:
wasm size, startup cost, and no place to attach curated metadata.

**Two tables** (one for the LSP, one for the simulator). Rejected: this is the
drift failure mode described above.

**Take `riscv-unified-db` directly as our schema.** Rejected for v1: Ruby
toolchain, large schema surface, and a much slower iteration loop for the RV32I
subset we need first. Adopt incrementally as an importer.

## Note on custom extensions

Vendor/custom extensions (`x*`) are first-class: `isa/vendor/<name>.toml` plus a
board preset. This keeps the "framework extensibility" requirement honest without
pulling vendor SDKs into the core.

## Implementation status (2026-11-22)

Done: `isa/*.toml`, the strict reader in `xtask`, codegen with a staleness check,
and 46/46 agreement with GNU `as`.

Not done: the upstream importer and `isa/upstream.lock`. The instruction tables
were entered by hand from the published encodings, and 46/46 agreement with GNU
`as` is the check that they are right. Extension closure (`M` implies `Zmmul`)
and the instruction-selection pass (compression) are also missing, and neither
matters until extensions beyond `I` plus `Zicsr` land.
