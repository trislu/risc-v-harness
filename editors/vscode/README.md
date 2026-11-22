# RISC-V harness

RISC-V assembly in VS Code: hover, diagnostics with quick fixes, completion and
go-to-definition from a language server that shares its parser with the
simulator, plus a debugger that steps one instruction at a time with registers
and memory on screen.

![Debugger stopped at entry, with registers and a breakpoint in the gutter](https://raw.githubusercontent.com/trislu/risc-v-harness/master/editors/vscode/media/screenshot-debug.png)

## Features

- diagnostics and quick fixes from the same pipeline as the playground and the CLI
- hover on instructions, pseudos, registers, CSRs, labels and directives
- completion with a candidate set chosen by position
- go-to-definition and semantic highlighting from the CST
- a debug type `rvh`: breakpoints, F5 on the current file, one instruction per `next`
- a **Memory** view that follows `sp` or `pc` and highlights the bytes a step changed
- per-file ISA pins, so one workspace can mix `rv32i`, `rv32im` and `rv32ia`

## Screenshots

Diagnostics come with fixes: `mul` is rejected until the file enables extension
`m`, and the fix inserts the directive that does it.

![A quick fix offering to enable extension m in this file](https://raw.githubusercontent.com/trislu/risc-v-harness/master/editors/vscode/media/screenshot-fix.png)

Completion is filtered by position, with the instruction format as detail.

![Typing ad, with add, addi, and and andi offered](https://raw.githubusercontent.com/trislu/risc-v-harness/master/editors/vscode/media/screenshot-completion.png)

A workspace can mix ISAs: the target is pinned per file from the status bar item.

![The RISC-V target picker listing rv32i, rv32i_zicsr and the workspace default](https://raw.githubusercontent.com/trislu/risc-v-harness/master/editors/vscode/media/screenshot-target.png)

The extension ships the `rvh` binary for its own platform and architecture, so
there is nothing else to install.

Settings, troubleshooting and build steps:
[docs/usage.md](https://github.com/trislu/risc-v-harness/blob/master/docs/usage.md#vs-code-extension).

## License

MIT OR Apache-2.0, at your option. See
[LICENSE.md](https://github.com/trislu/risc-v-harness/blob/master/editors/vscode/LICENSE.md).
