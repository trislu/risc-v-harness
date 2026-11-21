# ADR-0015: rv32e is a register limit, not a second instruction set

- Status: accepted
- Date: 2026-11-22
- Related: [ADR-0012](0012-one-base-rv32i.md), [ADR-0014](0014-target-precedence.md)

## Context

[ADR-0012](0012-one-base-rv32i.md) rejected `rv32e` along with the 64-bit bases,
because an E target rejected every instruction: the database tags instructions
`ext = "i"`, and E resolved to the extension `e`.

That reason does not hold. RV32E is RV32I with a 16-register file; the
instructions, encodings and semantics are the same. What differs is which
registers an instruction may name.

## Decision

**E reuses the `i` table and adds a register limit.**

1. `Target::parse("rv32e")` succeeds. Its instruction extensions start with `i`,
   so every RV32I instruction is available; `Target::name()` still prints `rv32e`,
   and `rv32e_m` and similar combinations work.
2. `Target::registers()` returns 16 for E and 32 otherwise.
3. `rvh-sem` checks every register operand against that limit and reports
   `sem.register-unavailable` on the operand, naming the register and the base:
   "`x16` is not available on `rv32e`, which has 16 registers".
4. The 64-bit bases stay rejected: they need a wider datapath, not a smaller
   register file (ADR-0012).

## Alternatives

- **A second instruction table for E.** Rejected: it would duplicate 54 entries to
  express one integer.
- **Silent truncation of high registers.** Rejected: `x16` becoming `x0` is the
  kind of wrong answer a simulator must never give.
- **Rejecting E until a project needs it.** Rejected once the check proved small:
  the table, the encoder and the machine were already correct for E.

## Consequences

- `rv32e` is usable: `008_rv32e.s` declares it with `.attribute arch, "rv32e2p0"`
  and runs, and the corpus keeps it byte-identical to GNU `as`.
- The register limit is a property of the target, so a file that switches target
  with `.option arch` gets the right limit from that line on.
- `rvh-sim` keeps a 32-entry register file: the limit is a legality rule, and the
  unused half is never addressed by correct code. Reserving memory for it would
  cost nothing but would suggest the registers exist.
