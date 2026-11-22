# Third-party sources

This file lists what the repository draws on, and under which terms. Code is
`MIT OR Apache-2.0`; documentation and content are `CC-BY-4.0`; the split is
recorded in [ADR-0007](docs/adr/0007-repo-shape.md).

| source | used for | terms | shipped |
| --- | --- | --- | --- |
| [Easy RISC-V](https://easyriscv.dram.page/) ([source](https://github.com/dramforever/easyriscv)) | the teaching sequence behind `tests/corpus/` and parts of the lesson text | CC0-1.0 | lesson text, credited |
| [The RISC-V Instruction Set Manual](https://riscv.org/technical/specifications/) | instruction encodings, operand widths, ISA semantics | CC-BY-4.0 for the prose; encodings are facts | `isa/*.toml`, hand-written |
| [GNU binutils](https://www.gnu.org/software/binutils/) (`as`, `ld`, `objcopy`, `readelf`) | differential oracle in `scripts/verify-*.sh` and the ELF fixture script | GPL-3.0-or-later | not shipped; invoked as a separate program |
| npm packages (`svelte`, `vite`, `codemirror`, `playwright`, `vscode-languageclient`) | web playground and editor client | each package's own license | bundled into `web/` and the VSIX |

The `isa/` data is transcribed from the manual rather than copied from
[riscv-opcodes](https://github.com/riscv-software-src/riscv-opcodes) or
[riscv-unified-db](https://github.com/riscv/riscv-unified-db); if the importer
promised by [ADR-0003](docs/adr/0003-isa-db.md) is built, its output takes on the
upstream license and its pinned commit is recorded in `isa/upstream.lock`.
