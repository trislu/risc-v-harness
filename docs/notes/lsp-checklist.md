# LSP obligations of `rvh-lsp`

The framework-free layer owns these. Each line is either done, or a stated gap with
a reason — an unlisted obligation is a bug. See
[ADR-0011](../adr/0011-lsp-framework-tradeoff.md).

## Done

- `Content-Length` framing, both directions, UTF-8 bodies.
- JSON read and write, including surrogate pairs.
- `initialize`, `initialized`, `shutdown`, `exit`.
- Full document sync (`textDocumentSync: 1`).
- `textDocument/publishDiagnostics`: not used; diagnostics are pulled.
- `textDocument/diagnostic` with a per-document generation as `resultId`, and
  `{ kind: "unchanged" }` on a match.
- `textDocument/definition` returns `LocationLink` objects (`targetUri`,
  `targetRange`, `targetSelectionRange`, `originSelectionRange`), not plain
  `Location`s, so the client can mark both ends of the jump.
- Hover is Markdown with a fixed shape: header line, then a table. Diagnostics
  remain plain text. Links are empty on purpose: they must point at an official
  site, and no per-instruction page exists yet (open question 13 in `plan.md`).
- `textDocument/hover`, `textDocument/completion`,
  `textDocument/definition`, `textDocument/codeAction`,
  `textDocument/semanticTokens/full`.
- UTF-16 positions for every range and every incoming position.
- Semantic tokens: legend `keyword, variable, function, number, string, comment,
  operator, property`. Two tests keep it honest: every class the encoder returns
  has a legend slot, and every word character of `004_directives.s` belongs to a
  token. A gap in the second is a test failure, not a discovery in the editor.
- Hover answers for every word: instruction, pseudo, register, CSR, label,
  constant, directive, section name, directive argument, architecture word,
  extension name, and the `+`/`-` of `.option`. A test walks every word character
  of a valid file and fails when one returns nothing.
- A custom `rvh/setDocumentTarget` notification pins a target to one document; the
  server re-analyses it and sends `workspace/diagnostic/refresh`.
- Notifications are never answered; `$/cancelRequest` and `$/setTrace` are
  accepted; unknown notifications are ignored; unknown requests get
  `-32601`.
- `window/logMessage` with `MessageType` 3 for lifecycle and 4 for requests,
  gated by `rvh.logLevel` and `$/setTrace verbose`.

## Not implemented, and why

| obligation | status |
| --- | --- |
| `$/cancelRequest` | accepted and ignored. Requests return in milliseconds and nothing is cancellable mid-way; implementing it without a queue would be theatre. Revisit if a request can become slow. |
| position-encoding negotiation (`general.positionEncodings`) | hard-coded UTF-16, the protocol default. A client that asks for another encoding is not accommodated. |
| incremental sync (`textDocumentSync: 2`) | full sync. Files are small; the client sends the document and the server reparses. |
| `completionItem/resolve`, `completionProvider.resolveProvider` | items carry everything they need. |
| `textDocument/references`, `rename`, `documentSymbol`, `foldingRange` | not implemented; `definition` is the only navigation feature so far. |
| progress, partial results, work-done tokens | no long-running work exists. |
| `workspace/*` folders and configuration | the target and log level arrive through `initializationOptions`; a multi-root workspace would need `workspace/configuration`. |
| file watching | the editor re-opens and re-sends on save. |
