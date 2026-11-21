# ADR-0005: TOML for structured data, Markdown with TOML front-matter for prose; no YAML

- **Status:** accepted
- **Date:** 2026-11-21
- **Context of:** `docs/plan.md` §6.5, `architecture.md` §2.1, §5

## Context

Four kinds of data need a format:

1. hand-edited configuration — `rvh.toml`, `board.toml`;
2. instruction tables — `isa/*.toml` (see ADR-0003);
3. prose content — per-instruction explanations, gotchas, translations;
4. machine-to-machine values — wasm ABI payloads, CLI `--json` output.

YAML was the default in the first draft. It is a poor fit here: indentation is
syntax, so a reformat or a bad paste changes meaning; deeply nested records diff
badly; and unquoted scalars invite the usual type surprises. JSON rejects
comments, which removes the one thing hand-edited files need. INI has no
nesting, no types, and no agreed way to express repeating records.

## Decision

| use | format | rule |
| --- | --- | --- |
| configuration | **TOML** | `rvh.toml`, `board.toml`; every key has a documented span for diagnostics |
| instruction tables | **TOML** | `[[insn]]` array-of-tables, one file per extension |
| prose content | **Markdown + TOML front-matter** | `+++` fence, one file per instruction or extension, per-locale directory |
| machine IPC | **JSON** | generated only, never hand-edited |
| generated tables | **Rust** (`include!`-free codegen) | never hand-edited TOML |

Enforcement:

- `taplo fmt` + `taplo check` in CI for every `.toml`;
- a schema linter for `isa/*.toml` (required fields, canonical extension names,
  sorted operand order, no duplicate id);
- prose files are linted as Markdown (markdownlint-cli2) and their front-matter
  as TOML by the same schema linter.

## Consequences

### Good

- no significant whitespace anywhere in the data path;
- comments are legal in every hand-edited file;
- `toml_edit` produces byte spans, so a malformed instruction record can point at
  the exact key instead of "parse error on line 1";
- one parser and one formatter for config, tables, and front-matter;
- prose stays prose: Markdown files give line-level diffs, reviewable by people
  who do not edit tables.

### Bad

- TOML is verbose per record: about ten lines per instruction, so 400
  instructions is roughly 4000 lines. Mitigation: the hand-edited surface stays
  small. The importer generates encoding tables; humans edit only an overlay
  keyed by `id`.
- arrays-of-tables cannot express a repeated *scalar* list nested inside another
  list. Not needed here; if it appears, split the record.
- TOML lacks `null`. Use omission plus a documented default.

## Alternatives considered

**YAML.** Rejected: indentation as syntax, comments that do not survive
round-trips cleanly, poor diffs for nested records, and the type-coercion
surprises (`no` becomes a boolean in 1.1, `1.0` becomes a float).

**JSON.** Rejected for hand-edited files: no comments, trailing-comma errors, and
a noisy diff for one-field changes. Kept for machine IPC, where none of that
matters.

**INI.** Rejected for structured data: no nesting, no types, no standard for
repeated sections. Acceptable only for flat key-value files, of which we have
none.

**KDL / RON / Starlark / a purpose-built DSL.** Rejected for v1: each adds a
parser, an editor mode, and a formatter we would have to maintain, with no gain
over TOML for these shapes. Revisit only if the record count makes TOML
unreadable even after generation.

**Rust source with a declarative macro.** Rejected: kills the data/prose review
workflow, forces a rebuild for content edits, and excludes non-Rust
contributors.

## Open

Where the *official* upstream text lands. Two candidates: vendored verbatim at
build time under `content/official/<ext>.md` with a pinned commit, or fetched at
build time. The license check in ADR-0003 gates this decision.

## Implementation status (2026-11-22)

Done: TOML for configuration and instruction tables, with a hand-written strict
reader instead of `toml_edit` (the workspace has no external dependencies).

Done (added later): the content layer. Short descriptions — pseudos, directives,
registers — live in flat `content/overlay/<locale>/<kind>.toml` maps and are
compiled into `rvh-content`, with key-by-key English fallback.

Refinement of this ADR: the format splits by length. One-line descriptions use
flat TOML; long prose keeps the Markdown plus front-matter shape decided here.
Both are hand-edited data, which was the point.
