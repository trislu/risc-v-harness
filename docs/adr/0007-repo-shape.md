# ADR-0007: One repository, explicit split points, committed codegen

- **Status:** accepted
- **Date:** 2026-11-21
- **Context of:** `docs/plan.md` §5, `architecture.md` §3

## Context

Six artifacts change together: `isa-db`, the syntax layer, the encoder, the
simulator, the wasm ABI, and the content. A change to one usually needs a matching
change in another within the same commit. The playground and the VS Code extension
are TypeScript shells around the same wasm core, so their release must track the
core version.

Three of the four consumers are not yet started (LSP, DAP, extension). Their
packaging depends on a decision that is still open (M5 host). The rest of the
repository shape can be fixed now.

## Decision

### One repository

Single Git repository, two build systems, one entry point each:

- Rust: a `cargo` workspace; `cargo xtask <cmd>` for anything that is not a plain
  build (codegen, corpus fetch, release helpers).
- TypeScript: one `pnpm` workspace covering `web/` and `editors/vscode/`.

```text
risc-v-harness/
├── crates/
│   ├── rvh-json/       # JSON value, writer, optional parser
│   ├── rvh-isa/        # isa-db loading, generated tables, target resolution
│   ├── rvh-syntax/     # lexer, parser, CST (must not depend on rvh-isa)
│   ├── rvh-sem/        # symbols, operand checks, pseudos, diagnostics, fixes
│   ├── rvh-asm/        # encoder, layout, Image, flat codec
│   ├── rvh-sim/        # Machine, decode/execute, devices, trace, undo
│   ├── rvh-session/    # document facade: analyze, assemble, step, hover, JSON
│   ├── rvh-content/    # overlay loading, locale fallback, content lint (M7)
│   ├── rvh-wasm/       # C ABI over rvh-session
│   ├── rvh-lsp/        # LSP feature layer over rvh-session
│   └── rvh-cli/        # the `rvh` binary
├── xtask/              # codegen (Rust) plus the smoke scripts (.sh, .mjs)
├── isa/                # hand-written TOML, one file per extension
├── content/            # official/ (read-only) + overlay/<locale>/ (M7)
├── web/                # Svelte 5 + Vite playground, Playwright tests
├── editors/vscode/     # VSIX (M5c)
├── tests/corpus/       # .s files, error cases, committed snapshots
├── docs/               # architecture, adr/, notes/, images/
└── .github/workflows/
```

### Committed codegen

`cargo xtask codegen` turns `isa/*.toml` into `crates/rvh-isa/src/generated/`.
The output is **committed**, and CI runs `cargo xtask codegen --check` to fail on
drift.

Reason: a `build.rs` step hides the generated diff, slows the wasm build, and makes
the binary depend on build-time file reads. A committed file makes every ISA change
reviewable as a normal diff.

### Two split points, with triggers

| what | stays here | leaves when |
| --- | --- | --- |
| `tree-sitter-riscv` | — | never enters this repo; separate from day one |
| `content/` | yes | two or more active translators, **and** content PRs start blocking code review |
| playground deploy | yes | never; CI deploys static assets |
| `editors/vscode` | yes | never; the shell shares the core version |

A split has a cost: a second CI, a second release process, and a schema that has to
be published. Until a trigger fires, the directory is the right unit.

### No submodules

Large external suites (`riscv-arch-test`) are fetched by `cargo xtask corpus fetch`
into a git-ignored directory and pinned by commit hash. Small excerpted `.s` files
live in `tests/corpus/` with a license header each.

### License split

| part | license |
| --- | --- |
| code (`crates/`, `xtask/`, `web/`, `editors/`) | MIT OR Apache-2.0 |
| docs and content | CC-BY-4.0 |
| `isa/` data | pending upstream verification (ADR-0003) |

Lesson text derived from Easy RISC-V gets an explicit credit line, even though CC0
does not require attribution. `THIRD_PARTY.md` at the root lists every source.

### Naming

| thing | name | why |
| --- | --- | --- |
| repository | `risc-v-harness` | the string people search for |
| product | `RISC-V harness` | how the docs spell it |
| crate prefix | `rvh-` | short; crates.io names are namespaced by prefix |
| binary | `rvh` | typed, not searched |
| local checkout directory | unchanged | renaming the session workspace root breaks tooling |

The RISC-V ecosystem owns `rv` plus one letter: `rvi`, `rvm`, `rva`, `rvc`,
`rvf`, `rvd`, `rvs`, and `rvh` (the hypervisor extension). So `rvh` is a poor
search term, while `RISC-V harness` is a good one. Splitting the discoverable name
from the typed name removes the collision without giving up the short command.
Industry writes the name with a hyphen, so the repository and the extension id use
`risc-v-harness`; the crate prefix stays `rvh-`.

Playground branding is separate. It should read well in Chinese, and it does not
need to share the prefix.

### Versioning and CI

- one workspace version (`0.1.0-dev` today); the VSIX version tracks the core
  version; automation (`release-plz`) only after the first public release;
- trunk-based development; every decision lands as an ADR under `docs/adr/`;
- CI jobs, by milestone:

| job | scope | from |
| --- | --- | --- |
| `rust` | fmt, clippy `-D warnings`, test, doc | M0 |
| `codegen-check` | `xtask codegen --check` | M0 |
| `lint` | `markdownlint-cli2`, `taplo check` | M0 |
| `differential` | encoder vs GNU `as` | M2 |
| `arch-test` | signature suite, nightly | M3 |
| `wasm` | build plus a size budget | M4 |
| `web-e2e` | Playwright against the built artifact | M4 |
| `release` | per-target VSIX, crates, static deploy | first release |

## Consequences

### Good

- one commit can change ISA data, encoder, simulator, and docs together;
- the dependency rule (syntax does not depend on isa) is enforced by the crate
  graph, not by review;
- committed codegen keeps ISA changes visible in review;
- a single version number across crates, wasm, and VSIX removes the usual
  "which core built this extension" question.

### Bad

- two build systems in one repo, so two toolchain pins and two caches;
- `cargo` and `pnpm` both need to be present for a full local build;
- one CI configuration carries jobs for milestones that do not exist yet; they stay
  disabled until their trigger.

## Alternatives considered

**One repository per component.** Rejected: a change to an encoding would need a
matching release of the ISA crate, a version bump of the assembler, and a rebuild
of two shells. The coupling is real, so the repo should reflect it.

**Separate `content` repository now.** Rejected: content needs the schema linter,
the CI, and the same review process as code. Splitting before there is a second
maintainer doubles the machinery and gains nothing.

**Git submodules for the test corpus.** Rejected: submodules break the default
clone, complicate CI, and this repository would pin them to one commit anyway.

**Generated code at build time (`build.rs`).** Rejected: hides diffs, slows the
wasm build, and makes the build depend on files outside the crate graph.

## Not decided here

- VSIX packaging: settled by [ADR-0009](0009-lsp-as-feature-layer.md) — one
  platform-agnostic package holding TypeScript plus wasm.
- Whether `rvh-isa` and `rvh-sem` merge once the boundary stops paying rent.
- `isa/` upstream license, which gates whether data ships with the repo or is
  fetched.

## Implementation status (2026-11-22)

Done: one repository, the workspace with fourteen crates, committed codegen with a
staleness check, one build entry per ecosystem, and the CI jobs.

Done (2026-11-22): `LICENSE-MIT` (SPDX text), `LICENSE-APACHE` (binutils' copy of
the official text), `LICENSE-CC-BY-4.0` (Creative Commons legal code) and
`THIRD_PARTY.md`. The GitHub repository is `risc-v-harness`; the product name in
this record stays `RISC-V harness`.

Not done: `isa/` upstream license verification, which still gates whether imported
data ships with the repository.
