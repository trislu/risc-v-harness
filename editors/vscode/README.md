# RISC-V harness for VS Code

Hover, diagnostics with quick fixes, and the `rvh` language server for RISC-V
assembly files (`.s`, `.S`).

The extension starts `rvh lsp --stdio` and talks LSP over stdio. The flag is
appended by `vscode-languageclient`; `rvh lsp` accepts it. Point `rvh.serverPath`
at the binary if it is not on `PATH`:

```json
{ "rvh.serverPath": "/home/dev/workspace/risc-v/target/release/rvh" }
```

## Target

The status bar carries the target the current file is analysed as: an icon whose
tooltip reads `target rv32i_zicsr · workspace default`. Click it, or run
**RISC-V: Select target for this file**, to pick a target for that one file. A pin
wins over the workspace setting, which is what makes a workspace with mixed ISAs
work; "Use workspace default" removes the pin.

`rvh.target` is the workspace default. The bases are `rv32i` and `rv32e` (16
registers, ADR-0015); extensions `zicsr`, `m` and `a` combine as usual, for
example `rv32i_zicsr`, `rv32im`, `rv32ia`. The 64-bit bases are named but rejected
(ADR-0012): the server logs a warning and stays on `rv32i_zicsr` rather than
running code with the wrong width.

## What works today

- diagnostics from the same pipeline as the playground and the CLI;
- quick fixes, such as `adii` to `addi`;
- hover on instructions, pseudos, registers, CSRs, labels and directives;
- completion of mnemonics, pseudos, directives, registers, CSRs and labels, with
  the candidate set chosen by position;
- go-to-definition from a label reference to its definition, as a `LocationLink`;
- semantic tokens from the CST, including directive arguments and the `+`/`-` of
  `.option arch`;
- hover text follows the locale served by `rvh-content`;
- a debug type `rvh`: breakpoints in the gutter, F5 on the current file, one
  instruction per *next*, and a **Memory** view that follows `sp` or `pc`.

## Building

```bash
pnpm install
pnpm --dir editors/vscode run build     # tsc --noEmit, then esbuild bundles out/extension.js
pnpm --dir editors/vscode run package   # writes risc-v-harness-vscode-0.1.0.vsix
```

`vscode-languageclient` is bundled into `out/extension.js`, so the VSIX has no
`node_modules` and needs no bundler at install time.

## Logs

The output channel **RISC-V harness** records what the client and the server did:

```text
[Info] initialize, target rv32i_zicsr
[Info] didOpen file:///…/902_error_common_mistakes.s (412 bytes)
diagnostic 902_error_common_mistakes.s -> 2 items
completion 902_error_common_mistakes.s:3:5 -> 105 items
definition 902_error_common_mistakes.s:3:5 -> found
hover 902_error_common_mistakes.s:3:5 -> none
```

Lifecycle lines go out as `MessageType.Info`, so the panel prefixes them; request
lines go out as `MessageType.Log`, which the panel shows unadorned. That is as far
as message types go in VS Code: there is no per-line level filter for a language
client's channel.

What is logged is decided by the server, through `rvh.logLevel`:

| value | what appears |
| --- | --- |
| `quiet` | nothing but problems |
| `normal` (default) | lifecycle, completion, hover, definition, and a diagnostic line **only when the count changes** |
| `verbose` | adds `didChange`, `codeAction` and `semanticTokens` |

`languageclient.trace.server: "verbose"` also sets `$/setTrace`, which the server
honours as the same switch.

A missing `completion` line means the editor never asked, which points at
`editor.quickSuggestions`, not at the server.

The top of the channel also records activation facts: the server path and target,
the active document's language, the server capabilities, and the two editor
settings that decide whether completion is requested at all.

Notifications such as `$/cancelRequest` and `$/setTrace` are accepted and never
answered, because a response to a notification is a protocol violation.

## Debugging

Install the extension, open a `.s` file and press F5: the contributed
configuration uses `${file}`, so it debugs the file in front of you.

When developing the extension, the Extension Development Host opens
`tests/corpus`, which carries its own `.vscode/launch.json` with a "Debug this
file" configuration and two prepared sessions.
Breakpoints are source lines mapped through the image's source map; a line with no
instruction is reported unverified.

The debug sidebar has a **Memory** view. It reads 256 bytes from an address and
offers three controls:

| control | meaning |
| --- | --- |
| **Go** | read 256 bytes starting at the address in the box |
| **Stack (sp)** | keep the window on the stack pointer after every stop; saved registers and locals live there |
| **Code (pc)** | keep the window on the program counter, so the bytes of the instruction about to run stay in view |

The gutter of the grid marks the row holding `pc` with `▶` and the row holding
`sp` with `↓`. Bytes that changed since the previous stop are highlighted — that
is why the view is drawn here rather than in a generic hex editor: the simulator
knows which bytes a step touched.

The adapter reports one stack frame, because the simulator models registers and
memory rather than functions, and `next` executes one instruction.

## Colours

Highlighting comes from the language server, not from a TextMate grammar: the
server classifies tokens from the same CST that produces diagnostics, so
`_start:` and a reference to `_start` are coloured differently.

VS Code keeps semantic highlighting off unless either the theme or the language
enables it. The manifest sets `editor.semanticHighlighting.enabled` for
`riscv-assembly`, so colours appear without any user action.

If colours are still missing, check in this order:

1. the status bar language mode says **RISC-V Assembly**, not *Assembly*; if not,
   add `"files.associations": { "*.s": "riscv-assembly" }` or click the language
   name in the status bar;
2. `Output` → **RISC-V harness** shows the client log;
3. `Developer: Inspect Editor Tokens and Scopes` lists `semantic token type`.

## What does not

- a TextMate grammar: highlighting is semantic only, so files stay plain until
  the server is ready.
