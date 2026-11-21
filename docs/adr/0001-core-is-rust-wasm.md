# ADR-0001: The core is Rust, shipped as wasm32 and native; shells are TypeScript

- **Status:** accepted
- **Date:** 2026-11-21
- **Context of:** `docs/plan.md` §2 (first user story), §3 (shape)

## Context

The semantic core (syntax → sem → asm → sim) has to run in three hosts, and all
three are required, not hypothetical:

1. **browser** — the v1 product is a student playground with no backend;
2. **editor process** — LSP now, DAP later;
3. **CI / agent** — a binary that can be dropped into a pipeline and invoked
   with `--json`.

Story 1 alone would argue for TypeScript: the target is the browser, the shell is
TS anyway, and `easyriscv` proves a JS RV32I simulator is viable. So the choice
is not "which language is nicer" but "how many implementations of ISA semantics
can we afford to maintain per year". The answer is one.

## Decision

- The core is **Rust**, compiled to `wasm32-unknown-unknown` for the browser and
  to a native static binary for CLI/CI. Same crates, two targets.
- **Shells are TypeScript**: the web UI (CodeMirror 6 + a framework) and the
  VS Code extension. They talk to the core through either the wasm ABI
  (`docs/architecture.md §2.5`) or the `rvh` CLI, never through duplicated logic.
- The wasm build is a **release-critical artifact from M4**, not a later port. If
  the core cannot ship as wasm, the milestone is not done.
- Single-threaded wasm, no `SharedArrayBuffer`, no workers, no COOP/COEP
  requirements: the playground must be hostable on a plain static host.

## Consequences

### Good

- one implementation of encodings, diagnostics, and execution semantics;
- bit-exact numerics remain possible later (`berkeley-softfloat-3` bindings) —
  JS cannot control rounding modes or NaN payloads, so F/D in TS would be a
  rewrite, not a port;
- `gimli` / `object` are available when the editor story needs DWARF and ELF;
- a static `rvh` binary for CI and agents with no runtime install;
- the performance ceiling (~10x a JS interpreter) is available for free if the
  trace/compliance workloads ever need it.

### Bad

- wasm pipeline + `wasm-pack` in CI; two toolchains (Rust, TS) in one repo;
- UI iteration is slower than a pure-TS playground;
- wasm bundle size is a real budget (~1–2 MB gzipped for the full core) and must
  be watched from M4 onward;
- contributor pool skews systems, not web.

## Alternatives considered

**All TypeScript.** Fastest path to M4, single language, platform-agnostic VSIX.
Rejected because it caps the project at "teaching playground": no bit-exact FP,
no DWARF, no dependency-free CLI, and the sim's hot loop inherits GC behaviour.
It is the right choice *if* the project is declared frozen at RV32I education.

**All Rust, including the UI (Yew/Leptos/Dioxus).** One language, but it makes
the web UI a Rust project — worse for the audience that will contribute content
and lessons, and worse for UI iteration speed. The shell is where churn lives.

**Rust engine + TypeScript LSP over wasm.** Legitimate and still open at M5
(`docs/plan.md §6.6`): a Node LSP hosting the wasm core would give a single
platform-agnostic VSIX. Nothing in this ADR blocks it; the wasm ABI is
deliberately kept coarse enough for either host.

## The bet

This ADR bets that the project grows from "student playground" to "real
development tooling" (ELF in, DWARF, DAP, compliance traces). If that bet is
wrong — if v1 is the whole product — TypeScript is the better engineering
choice, and this ADR should be superseded rather than defended.
