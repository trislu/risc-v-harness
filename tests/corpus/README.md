# Corpus

Every file here is a test fixture **and** a reading exercise. The numbering is the
reading order: start at `000`, read down. `900` and up are deliberately broken
files that exercise diagnostics.

Three digits, because the visible catalogue will not pass a thousand. Programs
take `000`–`899`, broken inputs take `900` and up, so a sorted listing shows the
teaching sequence first and the negative cases last.

| # | file | what it shows |
| --- | --- | --- |
| 000 | [000_hello_mmio.s](000_hello_mmio.s) | the console at `0x10000000`: `li`, `sb`, `ebreak` |
| 001 | [001_loop_sum.s](001_loop_sum.s) | a counted loop with `bnez` and an accumulator |
| 002 | [002_factorial.s](002_factorial.s) | recursion: `jal`/`ret`, a frame on the stack, repeated addition because `M` is absent |
| 003 | [003_numeric_labels.s](003_numeric_labels.s) | local labels `1f` / `1b` and why they keep names short |
| 004 | [004_directives.s](004_directives.s) | sections, `.string`, `.word`, `.align`, `.equ`, and `la` into `.data` |
| 005 | [005_trap_csr.s](005_trap_csr.s) | CSRs: install `mtvec`, read `mstatus` and `mcause` |
| 006 | [006_arch_directives.s](006_arch_directives.s) | `.option arch, +m` changes the target mid-file |
| 007 | [007_arch_attribute.s](007_arch_attribute.s) | `.attribute arch` declares the whole file's target |
| 008 | [008_rv32e.s](008_rv32e.s) | the embedded base: same instructions, 16 registers |
| 009 | [009_compressed.s](009_compressed.s) | the compressed extension: two-byte instructions |
| 900 | [900_error_missing_comma.s](900_error_missing_comma.s) | a missing comma, with a fix |
| 901 | [901_error_stray_token.s](901_error_stray_token.s) | per-line recovery after a stray token |
| 902 | [902_error_common_mistakes.s](902_error_common_mistakes.s) | did-you-mean, immediate range, CSR spelling, `;` as a comment |

## Debugging

`.vscode/launch.json` in this directory is the Extension Development Host's
workspace configuration: it offers "Debug this file" plus two prepared sessions.
That is what makes F5 work in the host without creating a configuration by hand.

## Settings

`.vscode/settings.json` pins `rvh.target` and `rvh.logLevel` for these demos, so a
user-level setting cannot change what the corpus exercises. Setting
`"rvh.target": "rv64i"` shows what an unimplemented base does: the server logs a
warning and keeps rv32i_zicsr. A settings change restarts the language server; no
window reload is needed.

## Rules

- **A number is fixed once published.** Inserting an example in the middle means
  renumbering, which breaks saved links and the taught order; append instead.
- **One idea per file.** If a file needs two paragraphs to explain, it is two
  files.
- **Snapshots live in `expected/<stem>.snap`** and are regenerated with
  `RVH_BLESS=1 cargo test -p rvh-syntax`.
- `rv32i-asm.txt` is the operand matrix used by `xtask/verify-asm.sh`; it is not a
  numbered example.
- A file may declare its own target (006, 007). It still has to assemble under
  `-march=rv32i_zicsr` plus those declarations, because that is how the
  differential check runs GNU `as`.
