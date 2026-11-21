# ADR-0004: `Image` is the internal contract; ELF is an interchange format

- **Status:** accepted
- **Date:** 2026-11-21
- **Context of:** `docs/plan.md` §1, §2 ("should the parser/LS emit IRs?"), §3 (simulator)

## Context

The original plan asked twice whether the parser and the language server should
emit some IR, on the theory that it would help both LSP and DAP. Separately, the
simulator must eventually run real toolchain output, and the debugger must map
machine code back to source.

There is a temptation to answer "ELF" and be done: it is the format everything
else speaks, and it carries symbols and DWARF. But making ELF the *internal*
contract forces an ELF writer and a linker into the critical path of the first
user story (a browser playground), which needs neither.

## Decision

Five artifacts, each with a named owner and a narrow public shape
(`docs/architecture.md §1`):

| layer | artifact | lifetime |
| --- | --- | --- |
| L1 | lossless CST | per document, edited |
| L2 | sem model (symbols, resolved instrs, diagnostics) | per document, derived |
| L3 | **`Image`** — bytes, sections, symbols, fixups, source map | per build, immutable |
| L4 | `Machine` state | per session |
| L5 | `Trace` (steps + undo info) | per session, ring-buffered |

- **`Image` is the contract** between assembly and everything downstream
  (`rvh-sim`, DAP, disassembly, ELF codec). It is a plain Rust struct, not a file
  format.
- **Two codecs, one model:**
  - *flat* (`M2`): assemble straight into `Image`; emit `.bin` + optional JSON
    symbol sidecar. This is what the playground uses. No linker, no relocations.
  - *ELF* (`M6`): `object` for read/write, `gimli` for DWARF line tables/CFI/
    variables. This is what "run `clang` output" and "DAP stack trace" need.
- **No private binary format.** If a file must be persisted, it is ELF or flat
  binary + JSON; never a bespoke container.
- **L3 carries the source map** (address range → file/line/column), so the
  playground's "which line is executing" and the DAP's reverse mapping use the
  same table, populated from assembler spans first and from DWARF later.
- **`%hi/%lo` and `la`/`li`/branch fixups are resolved inside `Image` layout**,
  using the same pcrel semantics as GNU `as`. Flat mode is a *linking shortcut*,
  not a *semantic shortcut*: a program must behave identically when later
  assembled into ELF.

## Consequences

### Good

- the first user story ships without an ELF writer or linker;
- the simulator loads exactly one thing, and a future ELF loader is an adapter,
  not a second code path in the core;
- DWARF enters as an *enrichment* of the source map (address ↔ source), so
  "goto C from asm" is a data feature rather than a parser feature;
- traces stay cheap and diffable because nothing about them depends on debug
  info.

### Bad

- two codecs to maintain and keep behaviourally identical (mitigated by the
  differential tests in `architecture.md §4`);
- section/segment semantics that the real linker provides (relaxation, `.text`
  merging, `%pcrel_hi` relaxation pairs) are ours to approximate or refuse.
  Policy: **refuse loudly** rather than emit subtly different bytes.

## Alternatives considered

**ELF as the internal contract from day one.** Rejected: adds a large component
(ELF writer + layout + relocation engine) to the critical path of M4, and its
complexity is not visible to the primary user.

**Serialize the CST and call it the IR.** Rejected: a CST is a *syntax* artifact;
simulators need addresses, not tokens. Conflating them forces the simulator to
depend on the parser, which breaks the layering.

**A custom binary "rvimg" format.** Rejected: zero interoperability, and it buys
nothing that flat binary + JSON does not.

## Open

Whether `Image` should also retain original bytes per instruction for lossless
disassembly of *foreign* input (ELF in). Tentatively yes: store `(addr, word,
Option<Span>)` and let the disassembler re-derive mnemonics from isa-db, so we
never trust a foreign disassembler's text.
