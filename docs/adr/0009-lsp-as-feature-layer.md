# ADR-0009: LSP is the feature layer; wasm keeps it cheap

- **Status:** accepted
- **Date:** 2026-11-21
- **Supersedes:** the transport decision in [ADR-0008](0008-editor-integration.md)

## Context

The original intent was option C: one core serving every editor. ADR-0008 chose
option A (VS Code providers over wasm) for packaging simplicity. That argument
rested on an assumption that turned out to be wrong.

The assumption: hosting an LSP server requires either per-platform native binaries
(release matrix, macOS notarization) or a second implementation of the feature
layer in TypeScript. Neither is necessary. LSP is JSON-RPC over a byte stream, so
the same wasm module that serves the playground can host the whole server loop:
export `handle_message(json) -> json[]`, and let a thin host shim move bytes.

The user's second concern is real and survives this change: LSP normalises the
protocol, not the experience. VS Code has the richest client. Neovim and Zed have
narrower coverage. Zed supports LSP semantic tokens too, but they are off by
default and have to be enabled per language.

## Decision

1. **One feature layer**, `rvh-lsp`, written once in Rust. It maps `rvh-session`
   results to protocol objects and owns no semantics of its own.
2. **Transport-agnostic, compiled to wasm and native.** The wasm build exports a
   message-level entry point; the native build talks on stdio.
3. **Two thin shims, no second implementation:**
   - VS Code: a Transport pair inside the extension host, so nothing is spawned;
   - every other editor: a small Node stdio shim running the same wasm, and a
     native binary only if someone asks for one.
4. **The playground does not use LSP.** It calls `rvh-session` through wasm
   directly. Both paths call the same object, so a diagnostic cannot differ.
5. **Editor-specific code is presentation only.** A status-bar item, a custom
   decoration, a webview: fine. Anything that computes a diagnostic, a fix, or a
   symbol lives in `rvh-session` or `rvh-lsp`.
6. **VS Code is the reference client.** Every feature carries a "degrades to"
   note, and a feature that cannot degrade gracefully waits for a real user.
7. **M5 ships one client.** The VS Code client lands at M5. Zed is the first
   non-VS Code client and lands at M8. The stdio transport is exercised in CI from
   M5, so the second client is a wiring job, not a research job.

## Consequences

### Good

- one implementation of the feature layer, so nvim and Zed cannot disagree with
  VS Code on semantics;
- one platform-agnostic VSIX: wasm plus JavaScript, no per-target builds and no
  code signing;
- the playground, the extension, and the CLI share one analysis path;
- the LSP surface is testable with a JSON-RPC driver, which is much cheaper than
  driving an editor;
- option C's reach, at option A's packaging cost.

### Bad

- the server loop runs in wasm: single-threaded, no true concurrency.
  Cancellation becomes cooperative, and long jobs must stay outside the server;
- protocol plumbing in Rust is real work (`lsp-types` plus a small dispatch loop),
  and version drift in LSP 3.17/3.18 is ours to track;
- editor UX differences remain. Zed will not show semantic tokens, Neovim's
  completion UI is not VS Code's;
- a user who installs the nvim path needs Node, unless a native build exists by
  then.

## Alternatives considered

**A: VS Code providers over wasm (ADR-0008).** Rejected as the target: it locks the
feature layer to one editor, and the day a second editor matters we would either
write a second implementation or rewrite. It remains a useful fallback for one
case: a VS Code-only presentation trick that LSP cannot express.

**C with native binaries first.** Rejected for M5: five to eight build targets,
macOS notarization, and a second release pipeline before anyone has used the
feature. Add a native build later, as a fast path, not as the only path.

**A TypeScript LSP server.** Rejected: it duplicates the analysis path that wasm
already provides, and it would drift from the playground.

## Verify at M5

- that the current `vscode-languageclient` accepts an in-process `Transport`
  cleanly; if not, fall back to spawning the Node shim inside the VSIX;
- the stdio path: a scripted JSON-RPC client in CI sends `initialize`, `hover`,
  `completion`, `definition`, and a `codeAction`, and checks the replies. That
  test is the M8 readiness check, and it costs one fixture file.

## Verify at M8

- Zed's LSP feature set, and whether the tree-sitter grammar is required for
  acceptable highlighting there (probably yes);
- Neovim coverage for semantic tokens, inlay hints, and pull diagnostics at the
  version we document.

## Degradation policy

| feature | VS Code | Neovim | Zed |
| --- | --- | --- | --- |
| hover, completion, goto, references | yes | yes | yes |
| diagnostics (push) | yes | yes | yes |
| code actions and fixes | yes | yes | partial |
| semantic tokens | yes | yes | yes (opt-in) |
| inlay hints | yes | yes | partial |
| pull diagnostics | yes | newer versions | unsure |

Cells marked "newer", "partial", or "unsure" are placeholders to verify at M8.
The rule stands regardless: no feature may exist only as an editor-side
computation.

## Implementation status (2026-11-22)

Done: `rvh-lsp` as the single feature layer over `rvh-session`, with a native
stdio transport, a hand-written JSON reader, UTF-16 positions, hover and quick
fixes.

Done (later): pull diagnostics ([ADR-0010](0010-pull-diagnostics.md)) and
semantic tokens from the CST.

Not done: the wasm-hosted loop. M5b pushes
`publishDiagnostics` on every change; modern clients prefer
`textDocument/diagnostic` plus `workspace/diagnostic/refresh`, which is the
direction modern servers took. Both are open questions in `docs/plan.md`.
