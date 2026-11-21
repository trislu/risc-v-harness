# ADR-0014: Who decides a document's target

- Status: accepted
- Date: 2026-11-22
- Related: [ADR-0003](0003-isa-db.md), [ADR-0012](0012-one-base-rv32i.md), [ADR-0013](0013-vendor-extensions.md)

## Context

A workspace can mix ISAs: boot code written for `rv32i`, an algorithm that uses
`M`, a vendor file that needs its own extension. One workspace setting cannot be
right for all of them, and a prompt at start-up would be global, answered once and
invisible afterwards.

GNU `as` already answers the question, and the answer lives in the file:

```asm
.attribute arch, "rv32i2p1_m2p0"   # what this file was written for
.option arch, +m                    # enabled from this line on
.option arch, -m                    # and disabled again
.option arch, rv32im                # replaced outright
```

## Decision

**Precedence: the file, then a per-document pin, then the workspace setting.**

1. `.attribute arch` and `.option arch` are parsed by `rvh-sem`. They change the
   target used for legality checks. `.option` applies from its line on, exactly as
   in GNU `as`; `.attribute` applies from its line on as well, which for the
   common case (first line) is the whole file.
2. Architecture strings are version-tolerant on input and version-free inside:
   `rv32i2p1_m2p0_zicsr2p0` becomes `rv32i_m_zicsr`. The database carries no
   versions yet ([ADR-0003](0003-isa-db.md)), so pretending otherwise would mean
   inventing data.
3. An unknown or unusable target is `sem.bad-target` on the directive, not a
   silent fallback.
4. The session keeps two targets: the *requested* one (pin or setting) that it
   re-resolves from, and the *effective* one that the file declared. Completion
   follows the effective one, because that is what the file can use.
5. The editor's per-document pin ([ADR-0013](0013-vendor-extensions.md)'s status
   item) ranks below the file: a file that declares `rv32im` is analysed as
   `rv32im` whatever the pin says.

## Alternatives

- **Workspace setting only.** Rejected: wrong for the second ISA in the same
  workspace, which is the case that started this.
- **Pin only.** Rejected: the file's author knows more than the reader's picker,
  and `clang` writes `.attribute arch` into what it produces.
- **Prompt at start-up.** Rejected: global, answered once, invisible later.
- **Full version tracking now.** Rejected: it multiplies the data model for no
  consumer; versions are stripped and recorded as a known gap.

## Consequences

- A file that needs `M` declares it, and the reader does not have to know a
  setting exists.
- Extension closure is still missing: GNU adds `zmmul` when `+m` is enabled, and
  this database has no `zmmul`. `+m` therefore enables exactly `m`. The difference
  is recorded rather than papered over.
- `.attribute arch` after instructions is accepted and applied from that point,
  where GNU rejects it. Being more permissive than the assembler is a stated
  choice: the editor sees half-written files, and refusing to analyse them would
  be worse than accepting a file the assembler will reject later.
- The precedence has one visible effect: a reader who pins `rv32i` for a file that
  declares `rv32i_m` still gets M instructions analysed, and the status item says
  `workspace default` or `set for this file` — it does not claim to be the final
  word once the file speaks.
