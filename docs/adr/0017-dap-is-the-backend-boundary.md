# ADR-0017: DAP is the boundary between the editor and whatever debugs

- Status: accepted
- Date: 2026-11-22
- Related: [ADR-0004](0004-image-vs-elf.md), [ADR-0016](0016-playground-is-a-demo-surface.md), [ADR-0011](0011-lsp-framework-tradeoff.md)

## Context

The debug adapter speaks DAP to the editor and drives `Session` in process. That
is enough to debug what `rvh asm` produces, and nothing more: there is no way to
point it at a board, at QEMU, or at spike.

The question this record answers: does DAP keep real hardware debugging possible,
or does it tie the project to its own simulator?

## Decision

**DAP is the editor-facing contract; the thing being debugged sits behind it.**

1. **The editor side never knows the backend.** VS Code is driven entirely through
   DAP requests, so replacing the backend does not touch `editors/vscode`.
2. **The simulator is the first backend, not the only one.** Today `Server` holds
   a `Session`; the extraction that matters later is a small trait over the
   operations DAP already uses — read and write registers, read and write memory,
   step, continue, halt, set and clear breakpoints, report the program counter.
   That extraction is deliberately deferred: it is cheaper to lift an interface out
   of two implementations than to invent one for a single implementation.
3. **Real debugging arrives through an existing protocol, not a new one.** Two
   routes, in order of preference:
   - drive `gdb` through its machine interface, the way `cppdbg` does, and let gdb
     own RSP, DWARF, multi-hart and CSRs;
   - or speak RSP directly to OpenOCD or a gdbstub if shelling out becomes a
     problem.
   Writing our own RSP client *and* our own debug stub is explicitly rejected:
   both already exist, and a hand-written one would be a worse copy.
4. **A runtime dependency on `gdb` is allowed; a build-time one is not.** The
   "no external dependencies" rule is about building this repository with a bare
   toolchain. Calling a `gdb` the user installed, at run time, to talk to hardware
   they own, is a different thing and belongs in the adapter's documentation.
5. **The simulator stays honest.** It is not a cycle-accurate model and it does not
   claim to be real hardware: it has no cache, no interrupts beyond the trap
   vector, one hart, and a fixed memory map.

## Consequences

- The door stays open at the cost of one field (`session: Option<Session>`) that
  will become a trait object. No editor code changes.
- Debugging a real target will need symbol information that this repository does
  not produce: the source map in `Image` covers our own assembler, and DWARF is
  the missing piece for binaries from other toolchains.
- The protocol layer keeps its own obligations (ADR-0011): a fast request path, no
  per-keystroke traffic, and a halt reason that distinguishes a deliberate stop
  from a fault.
- The trigger to do the extraction is concrete: a board on the desk, or a QEMU
  image that must be debugged with its own gdbstub.

## Alternatives

- **Custom VS Code commands instead of DAP.** Rejected at M6a: it would have
  coupled the UI to this simulator and closed the door this record keeps open.
- **A hand-written RSP client now.** Rejected: it buys nothing until there is a
  target to connect to, and gdb already does it better.
- **Abstraction first.** Rejected: an interface with one implementation is a guess
  about the second one.
