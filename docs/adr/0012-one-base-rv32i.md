# ADR-0012: One base for now: rv32i

- Status: accepted
- Date: 2026-11-22
- Related: [ADR-0003](0003-isa-db.md)

## Context

`Target::parse` accepted four bases — `rv32i`, `rv32e`, `rv64i`, `rv64e` — because
[ADR-0003](0003-isa-db.md) describes a target as "base plus extension set" and names
those bases in its canonical-form example. Nothing in the database or the machine
implemented more than one of them, and nothing said so.

The consequences were worse than a missing feature:

- **`rv32e` was unusable.** Every instruction in `isa/*.toml` is tagged `ext = "i"`,
  so an E target rejected all of them with "needs extension `i`". E is a subset of
  I with 16 registers, not a different instruction set.
- **`rv64i` ran 32-bit code.** The table has no RV64 instructions and the machine
  holds `u32` registers, so a program assembled for `rv64i` executed with 32-bit
  wraparound. A wrong answer that looks like a right one is the worst outcome a
  simulator can produce.

Neither state was recorded, so a reader could not tell an unimplemented feature
from a broken one.

## Decision

Only `rv32i` is supported. `Target::parse` keeps the four names in its list of
known bases — so "unknown" and "not implemented" stay different errors — and
rejects everything except `rv32i` with `TargetError::UnsupportedBase`, whose
message names the missing base and points at this record.

## Alternatives

- **Keep accepting the bases.** Rejected: it produces silently wrong results for
  `rv64i`/`rv64e` and a misleading diagnostic for `rv32e`.
- **Implement all four now.** Rejected as out of scope: 64-bit needs a
  width-parameterized datapath, `ld`/`sd`, and 6-bit shift amounts; E needs a
  register-limit layer over the I table, not a new instruction set.
- **Remove the names from the parser.** Rejected: then `--target rv64i` would read
  as a typo, which is the wrong hint for a future feature.

## Consequences

- A user asking for an unimplemented base gets one sentence and a pointer, at
  parse time, before any file is read.
- The editor path reports it too: the language server logs the requested target,
  the reason and the target it kept, and keeps serving the document. Silently
  ignoring the setting was the behaviour this record replaces.
- `rvh isa` is unaffected: the tables never claimed 64-bit instructions.
- E and 64-bit each deserve their own record when they are built. E is a smaller
  job than it looks — the instruction table already applies, and the work is
  restricting the register file to `x0..x15` and diagnosing the rest. 64-bit is
  the larger one: it changes the datapath, not the data.

## Implementation status (2026-11-22)

`rv32e` landed as a register limit over the `i` table
([ADR-0015](0015-rv32e-is-a-register-limit.md)); the 64-bit bases remain rejected,
which is the part of this record that still stands.
