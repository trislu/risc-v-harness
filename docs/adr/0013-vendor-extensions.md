# ADR-0013: Where vendor extensions plug in

- Status: accepted
- Date: 2026-11-22
- Related: [ADR-0002](0002-handwritten-parser.md), [ADR-0003](0003-isa-db.md), [ADR-0012](0012-one-base-rv32i.md)

## Context

The RISC-V naming convention reserves `X*` for non-standard extensions:
`rv32i_xvendor`. Toolchains accept such a name in `-march`, record it in
`.attribute arch`, and offer `.insn` for instructions the assembler does not know.

A reader asked the obvious follow-up: should the parser therefore have an
extension mechanism? It should not, and this record says why and where the
mechanism actually lives.

## Decision

**1. The data layer is the supported extension point.** A vendor extension is one
file, `isa/x<name>.toml`, with `ext = "x<name>"` on its instructions, plus
`cargo xtask codegen`. The parser needs no change, because it never knows a
mnemonic: [ADR-0002](0002-handwritten-parser.md) keeps `rvh-syntax` free of the ISA,
and `rvh-sem` resolves every name against the generated table. Diagnostics, hover,
completion and semantic tokens pick the extension up for free.

**2. `.insn` joins M7.** It is the escape hatch that matters when a vendor
instruction must be emitted before the data exists: five forms, one per encoding
format, producing a raw word. The simulator cannot execute what it does not model,
so `.insn` assembles and links but halts as an illegal instruction — a stated
limit, not a crash.

**3. A runtime ISA overlay is deferred.** Loading `isa/*.toml` without rebuilding
would require a TOML reader in the binary, an owned instruction table instead of
`&'static`, and a second source of truth for what `cargo xtask codegen --check`
verifies. That is a real refactor with no consumer today. It stays out until one
of these holds:

- a vendor needs to ship instructions without a `rvh` release;
- the table outgrows what one codegen pass reviews comfortably;
- a second product needs different tables from the same binary.

**4. Extension-id validation and canonical order land with the data work.** The
schema currently accepts any `ext` string. When `.option arch` is parsed, one
place validates the id (`i`, `m`, `z*`, `x*`) and `Target::name()` normalises the
order: single letters in `IMAFDQLCBKJTPVH` order, then `z*` alphabetically, then
`x*` alphabetically.

## Consequences

- "Vendor support" is a data change, not a parser change. The test of that claim is
  cheap: a fixture extension can be added and removed in one commit.
- `.insn` gives assembler parity with GNU `as` for unknown encodings, and leaves a
  visible hole in the simulator where semantics are missing.
- The deferral has a price only if a vendor arrives first: the refactor gets done
  then, with a real shape to target instead of a guessed one.
- Versioning follows the same logic. The project stays `0.x` until the ISA data
  format, the wasm ABI and the content format are stable enough that changing them
  is a breaking change somebody else would feel. Vendor participation is the
  clearest signal that this has happened; a second consumer is the other.

## Alternatives

- **A parse-time plugin registry.** Rejected: the parser has nothing to register;
  the resolver does, and the resolver already reads a table.
- **Loading TOML at runtime now.** Rejected as premature, for the reasons above.
- **Ignoring `X*` names.** Rejected: they are the one naming space a vendor gets,
  and rejecting them would make the tool unusable for exactly the extensions it
  should welcome.
