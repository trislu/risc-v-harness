# ADR-0008: Editor features ship as VS Code providers over the wasm core; no LSP server in M5

- **Status:** superseded by [ADR-0009](0009-lsp-as-feature-layer.md)
- **Date:** 2026-11-21
- **Context of:** `docs/plan.md` §1 (consumers), §4 (M5), `architecture.md` §2.5, §3

## Context

M5 needs hover, completion, goto definition, diagnostics, quick fixes, semantic
tokens, and document symbols for `.s` and `.S` files. Three shapes deliver them:

| shape | what the user installs | what we ship | who else can use it |
| --- | --- | --- | --- |
| **A** providers in the extension, core as wasm | one VSIX | TS + wasm | VS Code, vscode.dev |
| **B** extension hosts an LSP server | one VSIX | TS + wasm (Node server) | VS Code only |
| **C** standalone LSP server | extension + binary or Node | per-platform binaries or Node | any editor with an LSP client |

The core already ships as wasm for the playground ([ADR-0001](0001-core-is-rust-wasm.md)).
The VS Code extension API covers every feature on the M5 list: `HoverProvider`,
`CompletionItemProvider`, `DefinitionProvider`, `ReferenceProvider`,
`DocumentSymbolProvider`, `DiagnosticCollection`, `CodeActionProvider`,
`SemanticTokensProvider`, `InlayHintsProvider`, `FoldingRangeProvider`. Only pull
diagnostics and workspace-wide server queries have no direct equivalent, and M5
needs neither.

An LSP adds a protocol, a framed transport, and either a spawned process (per
platform binaries) or a Node server (a second host for the same core).

## Decision

- **M5 registers native `vscode` providers backed by the same wasm module the
  playground uses.** One platform-agnostic VSIX: TypeScript plus wasm.
- **No LSP server in M5.** The editor integration is not a protocol boundary.
- The **session API is transport-agnostic** and lives in `rvh-session`, not in
  `rvh-wasm`. `rvh-wasm` becomes a thin bindgen wrapper. If a second editor
  becomes a real user, `rvh-lsp` wraps the same session API; that is additive, not
  a rewrite.
- **The debug adapter follows the same path**: `DebugAdapterInlineImplementation`
  inside the extension (M6), not a separate DAP process.
- **File access is injected.** wasm has no filesystem, so the host passes file
  contents into the session. The same trait serves the browser (`fetch`) and the
  extension host (`vscode.workspace.fs`). M5 starts with the open document only;
  `.include` and `.S` includes arrive with the trait already in place.

## Consequences

### Good

- one artifact, no per-target builds, so `docs/plan.md §6.5` (VSIX packaging) disappears
  as a question;
- no process to spawn, install, locate, restart, or crash;
- the playground and the extension run literally the same wasm module and the same
  `analyze` entry point, so a diagnostic cannot differ between the two;
- the design stays compatible with web extensions (`vscode.dev`), because it uses
  no Node APIs and no processes;
- startup is one `WebAssembly.instantiate`, and there is no second IPC hop on every
  keystroke.

### Bad

- features reach VS Code only. Neovim, Helix, and Emacs users get the tree-sitter
  grammar and nothing else;
- pull diagnostics, workspace symbol search, and server-defined configuration are
  unavailable. We use push diagnostics and `workspace/configuration` equivalents
  from the extension settings instead;
- a future LSP must not grow features the provider path cannot express, or the two
  paths will drift. Rule: both paths call the same `rvh-session` API, and the
  provider path is the reference;
- the wasm module ships inside the VSIX (about 1–2 MB gzipped), which is a size
  budget to watch, not a blocker.

## Alternatives considered

**C: standalone LSP first.** Rejected for M5: it delivers the same features to one
user (VS Code) at the cost of a protocol, a process model, and per-platform
binaries. It is the right shape when a second editor matters, and the session API
keeps that door open.

**B: Node-hosted LSP inside the extension.** Rejected: it pays the protocol cost
without the portability benefit. If we ever want portability, shape C is the honest
version.

**Extension API only, no reusable core.** Rejected: it duplicates the analysis path
that the playground already needs, and it would let the two disagree.

## Open

- The trigger for building `rvh-lsp`: one editor outside VS Code used by a real
  user who reports a bug we care about. Until then, no.
- Whether the extension should also embed the playground as a webview. Probably
  yes later, at near-zero cost, because it is the same wasm module.
