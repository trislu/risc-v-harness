# ADR-0019: XLEN is a parameter, not a constant

- Status: accepted
- Date: 2026-11-22
- Related: [ADR-0012](0012-one-base-rv32i.md), [ADR-0018](0018-compressed-instructions-are-formats.md)

## Context

The first four milestones assumed 32-bit everywhere: `u32` registers, `u32` PC,
ELFCLASS32, a 32-bit word as the unit of a step. `rv64i` was rejected rather than
half-implemented, which was right at the time, but the assumption leaked into
places that will be expensive to change later: the machine's register file, the
ABI's register and memory views, the debugger's display, and the editor's
"register is 32 bits" formatting.

A 64-bit base is a matter of time, not of taste. The cheap moment to leave room is
while the surfaces are small.

## Decision

**Keep the 32-bit implementation, but make every width a named seam.**

1. Where a value's width is a policy rather than a fact, name it: the machine's
   register type is `Xlen` (today `u32`), the PC is `Xlen`, and the ELF layer keeps
   its class beside the data instead of inferring it from the pointer width.
2. Never hard-code `32` in a place that means XLEN. Literal widths belong to the
   instruction encoding (where the spec fixes five bits for a register field), not
   to the machine.
3. The floating-point file becomes `u64` with NaN-boxing now, because RV32D needs
   exactly that layout, and RV64 needs the same 64-bit registers without the
   boxing rule. Doing it for D therefore also pays for RV64.
4. The ABI and the JSON views describe values as "a register" plus a width, not as
   a `u32`. Where the wasm ABI cannot express a width yet, the conversion is
   documented at the boundary rather than assumed silently.
5. Moving to 64 bits requires its own record (M9 in `plan.md`): the instruction
   table, the ELF class, the trap and CSR widths, and the editor's formatting all
   need a decision, and this one only fixes the direction.

## Consequences

- Today nothing changes for a reader: `rvh` still runs RV32 only, and `rv64i` is
  still rejected with a message that says so.
- The seams make the eventual change an extension instead of a rewrite: a `u64`
  register type, an ELFCLASS64 path, and width-aware views in the session.
- The cost now is naming and a few helpers; the cost of not doing it would be
  found by whoever tries `rv64` first, in every layer at once.

## Alternatives

- **Wait for rv64 and change everything then.** Rejected: the surfaces that assume
  32 bits are exactly the ones being written now.
- **Implement rv64 as a second machine type now.** Rejected: two machines diverge,
  and the shared ABI would grow a branch per call. Parameterise one machine later.
- **Leave the float registers 32-bit until RV64.** Rejected: RV32D needs 64-bit
  registers with NaN-boxing, so D forces the wider file anyway.
