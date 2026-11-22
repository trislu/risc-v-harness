# Dialect matrix (empirical log)

**Purpose.** Every row here was executed, not remembered. RISC-V assembly is
standardised at the *instruction* level, not the *assembler* level; the divergences
below are silent (both assemblers exit 0 with different bytes), which is exactly
the class of bug our encoder must refuse rather than reproduce.

**Rule for this file:** a row is added only with the command that produced it.
Rows marked *unverified* are hypotheses, not decisions.

## Environment (verified 2026-11-21)

| tool | version |
| --- | --- |
| GNU assembler `riscv64-linux-gnu-as` | 2.46 (Ubuntu) |
| `llvm-mc` | 21.1.8 |
| `rustc` / `cargo` | 1.99.1 |
| `node` / `npm` | 22.22.1 / 9.2.0 |
| missing locally | `wasm32-unknown-unknown` target, `wasm-pack`, spike, qemu-riscv32 |

The two assemblers above are enough for the M2 differential oracle. Both are
CI-only; neither is a runtime dependency.

## Verified divergences

| case | GNU `as` | `llvm-mc` | consequence |
| --- | --- | --- | --- |
| `# comment` | comment | comment | agreed; use as canonical |
| `// comment` | **error** (`bad expression`, then `division by zero`) | **comment** | dialect flag |
| `; comment` | **statement separator** → `unrecognized opcode` | **error** (`unexpected token`) | tutorials use `;` as comment; must become a beginner-facing diagnostic, not a parse error |
| `jal ra, 0x10` (bare number) | treated as **absolute address** → imm=0 + `R_RISCV_JAL *ABS*+0x10`, resolved at link | treated as **relative offset** → imm=0x10 emitted inline | same source, different bytes, both exit 0 |
| `beqz a0, 0x40` | **long form**: inverted branch over a `jal` (assumes target may be out of B-range) | single B-type, offset emitted inline | pseudo-instruction expansion is dialect-dependent |
| `.align 3` | pads to offset **8** (`2^3` bytes) | *unverified* | RISC-V `.align` is power-of-two, unlike ARM's byte count |
| auto-compression, C enabled | **yes**: `addi a0,a0,1` → `0505` (`c.addi`) | **yes**: same | instruction selection is not a 1:1 mnemonic→encoding map |
| `.attribute arch, "rv32i2p1_m2p0"` | accepted; `readelf -A` reports `rv32i2p1_m2p0_zmmul1p0` | *unverified* | implied extensions are added automatically → isa-db needs a dependency closure |

## Consequences for the design

1. **`Dialect` is a first-class parse/encode parameter**, not a polish item. At
   minimum `Gnu` and `Llvm`; vendor toolchains enter as data, not branches.
2. **"Refuse loudly" policy** (see `adr/0004`): where the two assemblers disagree,
   we pick one as canonical (GNU `as` — it is what firmware SDKs use), document
   the divergence, and emit an error with an explanation when the other dialect's
   form is detected. Never emit silently different bytes.
3. **Beginner diagnostics are cheap here.** `;`-as-comment and `//`-as-comment are
   extremely common in tutorial code. `rvh` lexes both as a comment, so recovery
   stays clean, and then reports `syntax.semicolon-comment` or
   `syntax.slash-comment` with a fix that replaces the marker with `#`. This is a
   deliberate divergence from GNU `as` (where `;` separates statements): a
   teaching tool should recover, not reject. The diagnostic names the difference,
   so the file can still be made portable.
4. **Instruction selection ≠ encoding.** With `C` enabled both assemblers compress
   opportunistically, which affects byte counts, source maps (one source line →
   two bytes), and the differential test. The encoder therefore needs a selection
   step; and it needs an off switch for the teaching default, so that "why is my
   `addi` 2 bytes long" never becomes a mystery.
5. **Extension closure is needed.** `M` implies `Zmmul`; `F` implies `Zicsr`; `D`
   implies `F`, etc. Target normalisation must expand implications before any
   legality check, or every diagnostic will be subtly wrong.

## Unverified — next probes

- `.S` + cpp: `#include`, `#define`, line-marker mapping, `-x assembler-with-cpp`
- `.option arch, +m` / `-m`: mid-file extension toggling in both assemblers
- `.macro` / `.endm` / `.irp` / `.rept` semantics; whether the LSP must expand them
- local numeric labels (`1:`, `1f`, `1b`) in both
- `.insn`, `.type` / `.size`, section flags, `.p2align` vs `.align`
- `%hi` / `%lo` / `%pcrel_hi` / `%pcrel_lo` and `-mno-relax` behaviour
- `.align` in `llvm-mc`; `.align` inside `.text` vs `.data`
- whether `llvm-mc` accepts the same numeric-target forms as `as` for `call`/`tail`

## Reproduction

```bash
cd /tmp
printf 'addi a0,a0,1 ; comment\n' > c1.s
riscv64-linux-gnu-as -march=rv32i -o c1.o c1.s   # Error: unrecognized opcode
llvm-mc -triple=riscv32 -filetype=asm c1.s        # error: unexpected token

printf '.text\nstart:\njal ra,0x10\nj 0x20\n' > j.s
riscv64-linux-gnu-as -march=rv32i -o j.o j.s && riscv64-linux-gnu-objdump -dr j.o
llvm-mc -triple=riscv32 -show-encoding -filetype=asm j.s

printf 'addi a0,a0,1\n' > comp.s
riscv64-linux-gnu-as -march=rv32ic -o comp.o comp.s && riscv64-linux-gnu-objdump -d comp.o
llvm-mc -triple=riscv32 -mattr=+c -show-encoding -filetype=asm comp.s
```

## `.option arch, ±ext`

GNU `as` (binutils 2.4x) accepts `.option arch, +m` and rejects `.option arch, -m`
as deprecated:

```text
Error: deprecated - extension `m' in .option arch `-m'
```

`rvh` accepts both. Removing an extension is useful for teaching a file that stops
using one, and being more permissive than the assembler costs nothing here;
`006_arch_directives.s` therefore only uses `+m`, because the corpus must be legal
input for GNU `as`.

## Compression (`-march=rv32ic`)

GNU `as` compresses some plain instructions when the C extension is enabled:
`ebreak` becomes `c.ebreak`, `nop` becomes `c.nop`, and `addi` with small
immediates becomes `c.addi`. `rvh` only emits a compressed word for an explicit
`c.*` mnemonic; `ebreak` stays four bytes.

`verify-asm.sh` therefore compares the explicit `c.*` forms against GNU, byte for
byte, and ends its fixture with a four-byte instruction that GNU does not
compress.
