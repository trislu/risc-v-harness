# ADR-0011: A hand-written LSP layer instead of a framework

- Status: accepted
- Date: 2026-11-22
- Related: [ADR-0009](0009-lsp-as-feature-layer.md), [ADR-0010](0010-pull-diagnostics.md)

## Context

Rust has mature LSP frameworks. `tower-lsp-server` is the maintained one: async
handlers on tokio, typed protocol structures from `lsp-types`, transports for
stdio and sockets, and the plumbing for capabilities, cancellation and progress.

`rvh-lsp` is instead about six hundred lines written by hand: `Content-Length`
framing, a JSON reader and writer, a synchronous dispatch table, and the method
handlers.

The workspace has a standing rule that a build must work with a bare toolchain,
so the framework was not available to begin with. That is a constraint, not an
argument; the question is whether the constraint is worth keeping.

## Decision

Keep the hand-written layer.

The reasons, in order of weight:

1. **Synchronous and small.** Every document is one file. There is no workspace
   index, no background indexing, and no long-running task, which is exactly the
   workload where an async runtime earns its place.
2. **A testable core.** `Server::handle(body) -> Vec<String>` takes a message and
   returns the messages to send. Tests drive it without a runtime, a client or a
   pipe; the wire is covered separately by `cargo xtask lsp-smoke`.
3. **No dependency tree.** A framework brings tokio, tower, serde and
   `lsp-types`; this layer brings none.

The costs are real and paid every time the protocol changes:

- every capability, notification and edge case is the server's problem;
- JSON is built by hand, so a typo is a test failure rather than a compile error;
- features the framework provides — cancellation, position-encoding negotiation,
  progress, incremental sync — have to be written or knowingly skipped.

Two of those costs have already been paid in this project: the server rejected
the `--stdio` flag the client appends, and it answered notifications, which is a
protocol violation. Both were caught by the wire test, after the fact.

## Alternatives

- **`tower-lsp-server`.** Right for a native server with indexing or other
  background work, several transports, and more than one client. Wrong for a
  synchronous single-document server that must build with no dependencies.
- **`lsp-types` alone.** Keeps the synchronous loop and replaces hand-built JSON
  with typed structures. The middle path, rejected only because it still brings
  serde.

## Consequences

- The protocol obligations this layer owns are recorded as a checklist in
  `docs/notes/lsp-checklist.md`, so "not implemented" is a stated position rather
  than an oversight.
- Revisit this decision if any of these becomes true: a workspace index or
  multi-file resolution, a long-running task that must be cancellable, a second
  transport, or a second product reusing this layer.
