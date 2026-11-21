# ADR-0006: Playground I/O — memory-mapped console and `ebreak` halt; no `ecall` table in M4

- **Status:** accepted
- **Date:** 2026-11-21
- **Context of:** `docs/plan.md` §2 (first user story), §2.1 (prior art), `architecture.md` §2.2

## Context

The playground needs two things from the platform: a way to print, and a way to
stop. RISC-V defines neither. Every teaching simulator invents something.

Easy RISC-V (the prior art we follow for the curriculum) does this:

- output is a **memory-mapped write at `0x10000000`**: store a byte, see the
  character in the console panel;
- the first lesson ends with **`ebreak`**, which stops the machine;
- `ecall` appears only inside the final lesson, where the student writes a
  minimal OS with two system calls, `putchar` and `exit`.

The `ecall` numbering of that final lesson could not be verified: the site is a
JavaScript app, the repository is not reachable from this environment, and the
available description is secondary. We therefore match what is verifiable and
defer the rest.

## Decision

| concern | M4 behaviour | source |
| --- | --- | --- |
| console | byte write to `0x10000000` = putchar | matches Easy RISC-V, and equals the 16550 THR address |
| halt | `ebreak` raises a breakpoint trap; the platform stops the machine | matches Easy RISC-V |
| exit code | `a0` at halt | ours; Easy RISC-V has no exit status |
| `ecall` | traps to a platform handler with an empty table | deferred to the OS lesson |
| syscall table | data in `board.toml`, never in the core | keeps the profiles swappable |

Consequences for the curriculum: lessons 1–12 of the Easy RISC-V order need no
system call at all, because printing goes through MMIO. Lessons 13 and 14
(privilege, traps, the mini OS) are M7 content and will define the `ecall`
numbering then.

## Consequences

### Good

- M4 ships without a syscall ABI, so no numbering debate blocks the first release;
- the first lesson works as written in current Easy RISC-V material;
- `0x10000000` already being the 16550 transmit register means a future real UART
  model needs no address change;
- a later Linux user-mode profile is a `board.toml` entry, not a code path.

### Bad

- a program that only prints via `ecall` will not run in the playground;
- `ebreak` is a debug trap in the specification, and reusing it as "halt" is a
  platform convention. The diagnostic must say so, rather than pretend otherwise;
- the OS lessons cannot start until M7, so the curriculum is split across two
  milestones.

## Alternatives considered

**SPIM-style `ecall` numbering now** (1 = print int, 4 = print string, 10 = exit).
Rejected: Easy RISC-V does not use it, so copied lesson code would break, and it
makes students learn a convention before they have seen a trap handler.

**Linux-style `a7` = 64 (`write`).** Rejected for M4: it needs more register setup
per call, and it drags user-mode compatibility into the first milestone.

**A custom `print` pseudo-instruction.** Rejected: it teaches a mnemonic that does
not exist in the specification, and it would not survive a move to M7 and to real
hardware.

## Open

The `ecall` numbering for the OS lesson. Align with Easy RISC-V once its source or
lesson text is reachable. Until then `board.toml` carries `abi = "none"`.
