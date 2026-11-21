# ADR-0002: Hand-written Rust lexer/parser with a lossless CST; tree-sitter is a separate optional front-end

- **Status:** accepted
- **Date:** 2026-11-21
- **Context of:** `docs/plan.md` §1 (the parser)

## Context

The original plan proposed a header-only C tokenizer embedded as a tree-sitter
external scanner, exposing extra APIs so the language server (over FFI) could
produce better diagnostics than tree-sitter's recovery.

The motivation is sound: tree-sitter's error recovery is not usable as the basis
of a language server. It inserts `ERROR` nodes and skips tokens according to GLR
heuristics, and gives no way to say "you are missing a comma" or "this immediate
does not fit in 12 bits". A separate highlighter audience does exist, though, and
they are well served by tree-sitter.

## Decision

- `rvh-syntax` is a **hand-written Rust lexer + line-oriented parser** producing a
  lossless CST (rowan-style: every byte, including whitespace and comments, is in
  the tree). Error recovery is ours: on failure, emit one precise diagnostic and
  resync at the next line boundary or label, never "skip until it parses".
- The parser is **ISA-agnostic**: it accepts any mnemonic and any operand list,
  and emits no semantic diagnostics. `rvh-syntax` does not depend on `rvh-isa`
  (enforced by the crate graph).
- tree-sitter support, if built, lives in a **separate repository** (`tree-sitter-riscv`),
  consumes the same conformance corpus, and has no FFI relationship with the LSP.
  It is for highlighting-only users (Neovim, Helix, GitHub).
- RISC-V assembly is line-oriented and nearly LALR(1): `label: mnemonic op, op # c`.
  The grammar is not where the difficulty is; semantics and dialect are. We
  therefore spend parser effort on recovery quality, not on grammar theory.

## Consequences

### Good

- diagnostics and code actions are entirely under our control — this is the
  product differentiator, not a nice-to-have;
- one language in the syntax layer removes C/Rust drift;
- incremental reparse and cursor-preserving edits stay simple (rowan + a
  per-line statement parser);
- the same CST feeds LSP semantic tokens and CodeMirror decorations, so there is
  no third tokenizer (no Lezer grammar, no separate highlighter).

### Bad

- we own the grammar, the recovery strategy, and a test corpus;
- we lose the "free" tree-sitter highlighting for editors we do not support;
- the C header-only tokenizer idea (reusable by other tree-sitter grammars) is
  dropped. If lexical consistency with tree-sitter matters later, the corpus —
  not shared code — is the enforcement mechanism.

## Alternatives considered

**tree-sitter as the only parser, with a diagnostics side channel.** Rejected:
recovery is not controllable, external-scanner state is serializable-state only
(no "which operand am I in" memory), and the FFI boundary makes incremental
reparse and error recovery fight the framework.

**C header-only tokenizer shared by tree-sitter and Rust.** Rejected as the
*primary* design, for drift and FFI reasons. Kept as a possible future move only
if a second consumer genuinely needs a C tokenizer.

## Notes on dialect (open)

Dialect is a parser-level concern that we design for now and decide later
(`docs/plan.md §6.2`): `#` vs `//` vs `/* */`, `;` as separator vs comment, `.align`
being 2^n, numeric local labels (`1:` / `1f`), and `.s` vs `.S` (cpp) are all
parameterised behind a `Dialect` value from day one. Guessing these from memory
is how the corpus rots; an empirical matrix against GNU `as` and `llvm-mc` comes
first.

## Implementation status (2026-11-22)

Done: the hand-written lexer, the line-oriented parser, the lossless CST and
per-line recovery are in `rvh-syntax`, with a corpus and snapshots.

Not done: the `Dialect` value. The lexer currently treats `;` and `//` as comment
starters with a diagnostic, which is a deliberate divergence from GNU `as` and is
recorded in [docs/notes/dialect-matrix.md](../notes/dialect-matrix.md). Parameterise
it when a second dialect has to be supported.
