# Architecture decision records

One file per decision. A record is never edited to say something different after
the fact; a decision that changes gets a new record, and the old one is marked
superseded. That rule is what makes the directory useful a year later.

## Index

| # | Decision | Status |
| --- | --- | --- |
| [0001](0001-core-is-rust-wasm.md) | The core is Rust, shipped as wasm and native | accepted |
| [0002](0002-handwritten-parser.md) | A hand-written lexer and parser, with a lossless CST | accepted |
| [0003](0003-isa-db.md) | `isa-db` is the single source of truth for encodings | accepted |
| [0004](0004-image-vs-elf.md) | `Image` internally, ELF at the edges | accepted |
| [0005](0005-data-formats.md) | TOML for data, Markdown with front-matter for prose | accepted |
| [0006](0006-playground-io.md) | Playground I/O: MMIO console, `ebreak` halts | accepted |
| [0007](0007-repo-shape.md) | One repository, committed codegen, `rvh` naming | accepted |
| [0008](0008-editor-integration.md) | Editor integration by generation | **superseded by [0009](0009-lsp-as-feature-layer.md)** |
| [0009](0009-lsp-as-feature-layer.md) | One LSP feature layer, thin clients | accepted |
| [0010](0010-pull-diagnostics.md) | Pull diagnostics, no workspace index | accepted |
| [0011](0011-lsp-framework-tradeoff.md) | A hand-written LSP layer instead of a framework | accepted |
| [0012](0012-one-base-rv32i.md) | One base for now: `rv32i` | accepted |
| [0013](0013-vendor-extensions.md) | Vendor extensions plug in as data; `.insn` in M7; runtime overlay deferred | accepted |
| [0014](0014-target-precedence.md) | Target precedence: file, then per-document pin, then workspace setting | accepted |
| [0015](0015-rv32e-is-a-register-limit.md) | `rv32e` is a register limit over the `i` table, not a second instruction set | accepted |
| [0016](0016-playground-is-a-demo-surface.md) | The playground is a zero-install demo with a target picker, not a second IDE | accepted |
| [0017](0017-dap-is-the-backend-boundary.md) | DAP is the boundary; the simulator is the first backend, real targets arrive through gdb or RSP | accepted |

## Rules

**Decisions are superseded, never deleted.** The old record keeps its reasoning;
the new one links back. A reader who asks "why not X" finds X in the superseded
record.

**Factual errors are corrected in place.** A statement that is simply wrong — a
client capability we got wrong, an API that does not exist — is edited out. There
is nothing to learn from a false fact, and leaving it in misleads readers.

**Implementation status sits in its own section.** Each record ends with
`## Implementation status` when the code has moved past or short of the decision.
That keeps the decision stable while the status stays honest.

**Published history is not rewritten.** Corrections land as new commits. Local,
unpublished work may be squashed or rebased.
