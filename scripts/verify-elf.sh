#!/usr/bin/env bash
#
# Differential test for the ELF loader: build fixtures with the GNU toolchain,
# run them under `rvh run`, and check the result.
#
# This is the only path where a linker has resolved symbols, so it also covers
# what the single-file assembler cannot: `.global`, `.extern` and cross-file calls.

set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cargo build --quiet --manifest-path "$root/Cargo.toml" -p rvh-cli
rvh="$root/target/debug/rvh"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

as_rv32() { riscv64-linux-gnu-as -march=rv32i -mabi=ilp32 -o "$1" "$2"; }
ld_rv32() { riscv64-linux-gnu-ld -m elf32lriscv -Ttext=0 -o "$1" "${@:2}"; }

check() {
    case "$2" in
    *"$1"*) ;;
    *)
        echo "verify-elf: $3" >&2
        echo "  expected: $1" >&2
        echo "  actual:   $2" >&2
        exit 1
        ;;
    esac
}

# 1. A corpus program, assembled and linked by GNU.
as_rv32 "$work/hello.o" "$root/tests/corpus/000_hello_mmio.s"
ld_rv32 "$work/hello.elf" "$work/hello.o"
out=$("$rvh" run "$work/hello.elf")
check "Hi" "$out" "000_hello_mmio.elf printed nothing"
check "ebreak" "$out" "000_hello_mmio.elf did not halt on ebreak"

# 2. Data sections: `la` reaches the linker-placed message.
as_rv32 "$work/directives.o" "$root/tests/corpus/004_directives.s"
ld_rv32 "$work/directives.elf" "$work/directives.o"
out=$("$rvh" run "$work/directives.elf")
check "0x00001010" "$out" "directives.elf: la did not reach .data"

# 3. A cross-file call: two objects, one symbol.
cat > "$work/main.s" <<'EOF'
    .text
    .globl _start
_start:
    call helper
    ebreak
EOF
cat > "$work/helper.s" <<'EOF'
    .text
    .globl helper
helper:
    addi a0, zero, 42
    ret
EOF
as_rv32 "$work/main.o" "$work/main.s"
as_rv32 "$work/helper.o" "$work/helper.s"
ld_rv32 "$work/cross.elf" "$work/main.o" "$work/helper.o"
out=$("$rvh" run "$work/cross.elf")
check "0x0000002a" "$out" "cross.elf: the call did not reach helper"

# 4. Round trip: our assembler writes an ELF, readelf parses it, and our loader
#    runs it to the same result as running the source.
"$rvh" asm "$root/tests/corpus/004_directives.s" --elf "$work/ours.elf" >/dev/null
header=$(riscv64-linux-gnu-readelf -h "$work/ours.elf")
check "ELF32" "$header" "readelf: our ELF is not ELF32"
check "RISC-V" "$header" "readelf: our ELF is not RISC-V"
out=$("$rvh" run "$work/ours.elf")
check "0x00000010" "$out" "our ELF: la did not reach .data"
check "ebreak" "$out" "our ELF did not halt"

# 5. A truncated file is an error, not a crash.
head -c 40 "$work/hello.elf" > "$work/truncated.elf"
if out=$("$rvh" run "$work/truncated.elf" 2>&1); then
    echo "verify-elf: a truncated ELF must fail" >&2
    exit 1
fi
case "$out" in
*panicked*) echo "verify-elf: a truncated ELF panicked: $out" >&2; exit 1 ;;
esac

echo "verify-elf: OK"
