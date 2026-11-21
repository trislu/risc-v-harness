# ADR-0016: The playground is a demo surface, not a second IDE

- Status: accepted
- Date: 2026-11-22
- Related: [ADR-0001](0001-core-is-rust-wasm.md), [ADR-0006](0006-playground-io.md), [ADR-0009](0009-lsp-as-feature-layer.md)

## Context

The playground and the debugger show the same things: registers, memory, output,
diagnostics. Measured, the two front ends carry about 630 lines that describe one
state, and the wasm ABI underneath is shared, so the data cannot drift — the
*presentation* can.

Both also exist for different reasons. [plan.md §2](../plan.md) puts one of them
first: a student, in a browser, with no install. [Easy RISC-V](https://easyriscv.dram.page/)
already serves that story well, and browser simulators are not scarce: Venus,
riscv-angel, and several smaller ones. A page that only imitates them would be a
worse version of something that already exists.

## Decision

**Keep both, split the jobs, and let the playground do one thing.**

1. **The playground is the zero-install demo.** It exists so a reader can see the
   core work without installing anything, and so the wasm target is exercised by
   real use rather than by a smoke test alone. It is deployed as a static site, so
   a link is the whole onboarding.
2. **The debugger is the workflow surface.** Breakpoints by source line, call
   stack, watch, and debugging ELF produced by a real toolchain stay in the editor
   client; they are not added to the page.
3. **No feature race with teaching tools.** No course system, no exercise
   checker, no progress tracking. Those belong to the tools that already do them.
4. **The page may choose the target.** A reader who switches from `rv32i_zicsr` to
   `rv32im` sees which instructions become legal; that is a property of the core,
   not a second simulator. The picker recreates the session with the same source.
5. **Shared capabilities grow in the core.** If both surfaces need a new way to
   look at state, it goes into `rvh-session` or the wasm ABI first, and each
   surface presents it in its own idiom.

## Consequences

- The duplication that remains is on purpose: a bytegrid in `web/src/App.svelte`
  and a hex grid in `editors/vscode/src/memory.ts`. Both are small, both read the
  same machine, and neither is allowed to grow an editor feature.
- Deployment makes the page a public surface. It is built from `master` after the
  checks pass, under `/<repository>/`, and it becomes the first official URL the
  project has — the anchor that open question 13 (documentation links) needs.
- The playground stays honest about its limits: it cannot debug ELF, it has no
  breakpoints, and it is not a course.

## Alternatives

- **Drop the page.** Cheaper to maintain, and wasm would still serve the
  in-process LSP ([ADR-0009](0009-lsp-as-feature-layer.md)). Rejected because the
  first user story is a browser, and a link is the only onboarding that needs
  nothing installed.
- **Make the page the main product.** Rejected: it would compete directly with
  established teaching simulators on their strongest ground.
- **Embed the page in the extension.** Rejected: two UIs for one state, neither
  maintained well.
