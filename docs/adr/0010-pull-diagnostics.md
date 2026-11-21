# ADR-0010: Pull diagnostics, and no workspace index

- Status: accepted
- Date: 2026-11-22
- Supersedes: nothing
- Related: [ADR-0009](0009-lsp-as-feature-layer.md)

## Context

M5b pushed `textDocument/publishDiagnostics` after every change. That works, and
it was the shortest path to a visible squiggle. Two things changed the balance:

1. Modern clients prefer pull diagnostics (`textDocument/diagnostic`). VS Code
   uses it when the server advertises `diagnosticProvider`, and pull removes the
   duplicate work of a server deciding when to push.
2. Pull with a generation counter as `resultId` is the shape modern clients
   expect, and servers whose diagnostics depend on a workspace index also send
   `workspace/diagnostic/refresh` to ask for a re-pull.

The second half does not apply here, but not because files are isolated. They are
not: `.global` exports a symbol, `.extern` declares one that another file defines,
and `.weak` changes what the linker may do with it. A reference to a name this
file does not define is a link-time concern.

The server still analyses one document, because that is all it can see: there is
no linker and no workspace index, so an unresolved name is reported as unresolved
instead of being checked against a sibling file. Every diagnostic therefore
depends on exactly the document the client just edited, which is the property pull
diagnostics needs. Files are also small, so a full recompute per edit is cheap; a
catalog would add machinery with nothing to show for it.

## Decision

- Advertise `diagnosticProvider` with `interFileDependencies: false` and
  `workspaceDiagnostics: false`.
- Implement `textDocument/diagnostic` and return
  `{ kind: "full", items, resultId }`.
- Use a per-document generation counter as `resultId`; return
  `{ kind: "unchanged", resultId }` when the client sends back the current one.
- Do not send `publishDiagnostics`.
- Do not send `workspace/diagnostic/refresh`. Every diagnostic depends only on
  the document the client just edited, and clients re-pull after edits. Refresh
  exists for state that changes outside the edited document; we have none.

## Alternatives

- **Keep push only.** Simple, but the client cannot coalesce or cancel, and every
  keystroke costs a notification plus a parse.
- **Push and pull together.** Two code paths, two ways to see stale results.
- **Pull plus refresh.** The reference design. Correct the moment diagnostics can
  depend on another file (`.include`, macros, linker scripts). Not now: it would
  be a request we never need to send.

## Consequences

- Clients older than LSP 3.17 get no diagnostics. Acceptable: the first client is
  VS Code, which supports pull.
- `resultId` has to be bumped on every path that changes the document. Today that
  is `didOpen` and `didChange`; the counter lives next to the session so a new
  path cannot forget it.
- When cross-file features arrive, this ADR gets a successor: add
  `workspace/diagnostic/refresh` and `interFileDependencies: true`. The pull side
  does not change.
