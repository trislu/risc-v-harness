#!/usr/bin/env bash
#
# Cross-check the ISA table against a real assembler.
#
# Every case in tests/corpus/rv32i-asm.txt is assembled with GNU as, the first
# word of the result is decoded with `rvh isa decode`, and the mnemonic must
# match. This is the seed of the M2 differential suite; it needs binutils for
# RISC-V and is therefore a developer/CI check, never a runtime dependency.

set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
assembler=${AS:-riscv64-linux-gnu-as}
disassembler=${OBJDUMP:-riscv64-linux-gnu-objdump}
cases="$root/tests/corpus/rv32i-asm.txt"
work="$root/target/verify-asm"

for tool in "$assembler" "$disassembler"; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "verify-asm: $tool not found; install binutils for RISC-V" >&2
        exit 2
    fi
done

cargo build --quiet --manifest-path "$root/Cargo.toml" -p rvh-cli
rvh="$root/target/debug/rvh"
mkdir -p "$work"

pass=0
fail=0
while IFS='|' read -r expected source; do
    case "$expected" in
    '' | '#'*) continue ;;
    esac
    printf '.text\n%s\n1:\n' "$source" >"$work/case.s"
    if ! "$assembler" -march=rv32i_zicsr -o "$work/case.o" "$work/case.s" 2>"$work/as.err"; then
        echo "AS-FAIL   $expected: $(head -n 1 "$work/as.err")"
        fail=$((fail + 1))
        continue
    fi
    word=$("$disassembler" -d "$work/case.o" | awk '/^ +[0-9a-f]+:/{print $2; exit}')
    got=$("$rvh" isa decode "0x$word" | awk 'NR==1{print $2}')
    if [ "$got" = "$expected" ]; then
        pass=$((pass + 1))
    else
        echo "MISMATCH  $expected: 0x$word decoded as '${got:-<none>}'"
        fail=$((fail + 1))
    fi
done <"$cases"

# Every valid corpus file must also be legal input for GNU as. This keeps the
# parser corpus honest: a file the parser accepts but the assembler rejects is a
# fixture bug, not a parser feature.
for file in "$root"/tests/corpus/*.s; do
    case "$(basename "$file")" in
    9*) continue ;;
    esac
    if "$assembler" -march=rv32i_zicsr -o "$work/corpus.o" "$file" 2>"$work/as.err"; then
        pass=$((pass + 1))
    else
        echo "CORPUS-FAIL $(basename "$file"): $(head -n 1 "$work/as.err")"
        fail=$((fail + 1))
    fi
done

# Byte-for-byte agreement with the real toolchain, for the corpus files whose
# .text does not reference the data section. 004_directives is excluded on purpose:
# its `la` points into .data, and placing .data is a linker policy we do not
# emulate yet (see M6 in plan.md).
linker=${LD:-riscv64-linux-gnu-ld}
objcopy=${OBJCOPY:-riscv64-linux-gnu-objcopy}
for file in "$root"/tests/corpus/*.s; do
    base=$(basename "$file" .s)
    case "$base" in
    # 009 ends with a two-byte instruction, so GNU pads .text to four bytes and the
    # comparison would measure padding. Its encodings are covered by the RV32C
    # fixture below.
    9* | 004_directives | 009_compressed) continue ;;
    esac
    "$assembler" -march=rv32i_zicsr -o "$work/$base.o" "$file"
    "$linker" -m elf32lriscv -Ttext=0 --no-relax -o "$work/$base.elf" "$work/$base.o" 2>"$work/ld.err"
    "$objcopy" -O binary -j .text "$work/$base.elf" "$work/$base.gnu.bin"
    "$rvh" asm "$file" --text "$work/$base.ours.bin" >/dev/null
    if cmp -s "$work/$base.gnu.bin" "$work/$base.ours.bin"; then
        pass=$((pass + 1))
    else
        echo "BYTES-DIFF $base: our .text differs from GNU as + ld"
        fail=$((fail + 1))
    fi
done

# RV32M: the same eight instructions through both assemblers. The corpus stays at
# RV32I, so this is the only place the multiply and divide encodings are compared.
cat >"$work/m.s" <<'EOF'
    .text
    .globl _start
_start:
    mul a0, a1, a2
    mulh a0, a1, a2
    mulhsu a0, a1, a2
    mulhu a0, a1, a2
    div a0, a1, a2
    divu a0, a1, a2
    rem a0, a1, a2
    remu a0, a1, a2
    ebreak
EOF
"$assembler" -march=rv32im -mabi=ilp32 -o "$work/m.o" "$work/m.s"
"$linker" -m elf32lriscv -Ttext=0 --no-relax -o "$work/m.elf" "$work/m.o"
"$objcopy" -O binary -j .text "$work/m.elf" "$work/m.gnu.bin"
"$rvh" asm "$work/m.s" --target rv32i_m --text "$work/m.ours.bin" >/dev/null
if cmp -s "$work/m.gnu.bin" "$work/m.ours.bin"; then
    pass=$((pass + 1))
else
    echo "BYTES-DIFF rv32m: our .text differs from GNU as + ld"
    fail=$((fail + 1))
fi

# RV32A: the eleven atomics, assembled by both and compared byte for byte.
cat >"$work/a.s" <<'EOF'
    .text
    .globl _start
_start:
    lr.w a0, (a1)
    sc.w a0, a2, (a1)
    amoswap.w a0, a2, (a1)
    amoadd.w a0, a2, (a1)
    amoxor.w a0, a2, (a1)
    amoand.w a0, a2, (a1)
    amoor.w a0, a2, (a1)
    amomin.w a0, a2, (a1)
    amomax.w a0, a2, (a1)
    amominu.w a0, a2, (a1)
    amomaxu.w a0, a2, (a1)
    ebreak
EOF
"$assembler" -march=rv32ia -mabi=ilp32 -o "$work/a.o" "$work/a.s"
"$linker" -m elf32lriscv -Ttext=0 --no-relax -o "$work/a.elf" "$work/a.o"
"$objcopy" -O binary -j .text "$work/a.elf" "$work/a.gnu.bin"
"$rvh" asm "$work/a.s" --target rv32ia --text "$work/a.ours.bin" >/dev/null
if cmp -s "$work/a.gnu.bin" "$work/a.ours.bin"; then
    pass=$((pass + 1))
else
    echo "BYTES-DIFF rv32a: our .text differs from GNU as + ld"
    fail=$((fail + 1))
fi

# RV32C: every compressed instruction, byte for byte against GNU as.
cat >"$work/c.s" <<'EOF'
    .text
    .globl _start
_start:
    c.addi4spn a0, sp, 16
    c.lw a0, 4(a1)
    c.sw a0, 4(a1)
    c.addi a0, 1
    c.jal a
a:
    c.li a0, 3
    c.addi16sp sp, -16
    c.lui a0, 4
    c.srli a0, 3
    c.srai a0, 3
    c.andi a0, 3
    c.sub a0, a1
    c.xor a0, a1
    c.or a0, a1
    c.and a0, a1
    c.j b
b:
    c.beqz a0, c
c:
    c.bnez a0, b
    c.slli a0, 3
    c.lwsp a0, 4(sp)
    c.jr a0
    c.mv a0, a1
    c.ebreak
    c.jalr a0
    c.add a0, a1
    c.swsp a0, 4(sp)
    add a0, a1, a2
EOF
riscv64-linux-gnu-as -march=rv32ic -mabi=ilp32 -o "$work/c.o" "$work/c.s"
riscv64-linux-gnu-ld -m elf32lriscv -Ttext=0 --no-relax -o "$work/c.elf" "$work/c.o"
riscv64-linux-gnu-objcopy -O binary -j .text "$work/c.elf" "$work/c.gnu.bin"
"$rvh" asm "$work/c.s" --target rv32ic --text "$work/c.ours.bin" >/dev/null
if cmp -s "$work/c.gnu.bin" "$work/c.ours.bin"; then
    pass=$((pass + 1))
else
    echo "BYTES-DIFF rv32c: our .text differs from GNU as + ld"
    fail=$((fail + 1))
fi

# `.insn`: raw encodings. Only the r, i, s and u forms are compared: GNU `as`
# flips bit 0 of the `b` form's funct3 and ignores the `j` form's offset (see the
# dialect note), so those two are checked by unit tests instead.
cat >"$work/raw.s" <<'EOF'
    .text
    .globl _start
_start:
    .insn r 0x33, 0x0, 0x00, a0, a1, a2
    .insn i 0x13, 0x0, a0, a1, 42
    .insn s 0x23, 0x2, a1, 8(a2)
    .insn u 0x37, a0, 0x12345
    ebreak
EOF
riscv64-linux-gnu-as -march=rv32i -mabi=ilp32 -o "$work/raw.o" "$work/raw.s"
riscv64-linux-gnu-ld -m elf32lriscv -Ttext=0 --no-relax -o "$work/raw.elf" "$work/raw.o"
riscv64-linux-gnu-objcopy -O binary -j .text "$work/raw.elf" "$work/raw.gnu.bin"
"$rvh" asm "$work/raw.s" --text "$work/raw.ours.bin" >/dev/null
if cmp -s "$work/raw.gnu.bin" "$work/raw.ours.bin"; then
    pass=$((pass + 1))
else
    echo "BYTES-DIFF .insn: our .text differs from GNU as + ld"
    fail=$((fail + 1))
fi

# RV32F, first slice: loads, stores, arithmetic and the moves.
cat >"$work/f.s" <<'EOF'
    .text
    .globl _start
_start:
    li t0, 0x100
    flw f0, 0(t0)
    flw f1, 4(t0)
    fadd.s f2, f0, f1
    fsub.s f3, f0, f1
    fmul.s f4, f0, f1
    fdiv.s f5, f0, f1
    fmv.x.w a0, f2
    fmv.w.x f6, a0
    fsw f2, 8(t0)
    fsgnj.s f7, f0, f1
    fsgnjn.s f8, f0, f1
    fsgnjx.s f9, f0, f1
    feq.s a1, f0, f1
    flt.s a2, f0, f1
    fle.s a3, f0, f1
    fmin.s f10, f0, f1
    fmax.s f11, f0, f1
    fclass.s a4, f0
    fcvt.w.s a5, f0
    fcvt.wu.s a6, f0
    fcvt.s.w f12, a5
    fcvt.s.wu f13, a5
    fmadd.s f14, f0, f1, f2
    fmsub.s f15, f0, f1, f2
    fnmsub.s f16, f0, f1, f2
    fnmadd.s f17, f0, f1, f2
    ebreak
EOF
riscv64-linux-gnu-as -march=rv32if -mabi=ilp32f -o "$work/f.o" "$work/f.s"
riscv64-linux-gnu-ld -m elf32lriscv -Ttext=0 --no-relax -o "$work/f.elf" "$work/f.o"
riscv64-linux-gnu-objcopy -O binary -j .text "$work/f.elf" "$work/f.gnu.bin"
"$rvh" asm "$work/f.s" --target rv32if --text "$work/f.ours.bin" >/dev/null
if cmp -s "$work/f.gnu.bin" "$work/f.ours.bin"; then
    pass=$((pass + 1))
else
    echo "BYTES-DIFF rv32f: our .text differs from GNU as + ld"
    fail=$((fail + 1))
fi

echo "verify-asm: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
