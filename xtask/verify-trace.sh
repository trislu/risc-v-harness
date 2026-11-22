#!/usr/bin/env bash
#
# Compare the simulator's step trace with spike's commit log, instruction by
# instruction. This is a developer check: it needs spike and binutils for RISC-V,
# and is skipped when spike is absent, so it is never a build dependency.
#
# Set RVH_SPIKE to a spike binary, or have `spike` in PATH.

set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
assembler=${AS:-riscv64-linux-gnu-as}
linker=${LD:-riscv64-linux-gnu-ld}
work="$root/target/verify-trace"
spike=${RVH_SPIKE:-$(command -v spike || true)}

if [ -z "$spike" ] || [ ! -x "$spike" ]; then
    echo "verify-trace: skipped; set RVH_SPIKE to a spike binary" >&2
    exit 0
fi
for tool in "$assembler" "$linker"; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "verify-trace: $tool not found; install binutils for RISC-V" >&2
        exit 2
    fi
done

cargo build --quiet --manifest-path "$root/Cargo.toml" -p rvh-cli
rvh="$root/target/debug/rvh"
mkdir -p "$work"

# Strip leading zeros so `0x41` and `0x00000041` compare equal.
normalise='
function norm(text) {
    sub(/^0x0+/, "0x", text)
    sub(/^0x$/, "0x0", text)
    return text
}
'

# `core   0: 3 <pc> (<word>) x5  0x… mem 0x… 0x…` becomes
# `<pc> <word> x5=0x… mem[0x…]=0x…`, the shape our `--trace` prints.
spike_to_trace() {
    awk "$normalise"'
        $1 == "core" && $3 == "3" {
            word = $5
            gsub(/[()]/, "", word)
            line = norm($4) " " norm(word)
            i = 6
            while (i <= NF) {
                if ($i ~ /^[xf][0-9]+$/) { line = line " " $i "=" norm($(i + 1)); i += 2 }
                else if ($i == "mem" && i + 2 <= NF && $(i + 2) ~ /^0x/) { line = line " mem[" norm($(i + 1)) "]=" norm($(i + 2)); i += 3 }
                else if ($i == "mem") { i += 2 }
                else { i += 1 }
            }
            print line
        }' - | awk '$1 >= "0x80000000" && NF > 2'
}

# Our lines carry padded values; normalise them the same way.
ours_to_trace() {
    awk "$normalise"'
        {
            line = norm($1) " " norm($2)
            for (i = 3; i <= NF; i++) {
                split($i, pair, "=")
                line = line " " pair[1] "=" norm(pair[2])
            }
            print line
        }' -
}

run_case() {
    local name=$1 isa=$2 march=$3 source=$4
    local object="$work/$name.o" elf="$work/$name.elf"
    "$assembler" -march="$march" -mabi=ilp32 -o "$object" "$source"
    "$linker" -m elf32lriscv -Ttext-segment=0x80000000 -e _start -o "$elf" "$object"

    timeout 120 "$rvh" run "$elf" --board spike --trace --max-steps 10000 2>/dev/null | ours_to_trace |
        awk 'NF > 2' >"$work/$name.ours"
    timeout 120 "$spike" -l --log="$work/$name.log" --log-commits --isa="$isa" "$elf" >/dev/null 2>&1 || true
    spike_to_trace <"$work/$name.log" >"$work/$name.spike"

    if diff -u "$work/$name.spike" "$work/$name.ours" >"$work/$name.diff"; then
        pass=$((pass + 1))
    else
        echo "TRACE-DIFF $name: first difference"
        head -n 12 "$work/$name.diff" | sed 's/^/    /'
        fail=$((fail + 1))
    fi
}

pass=0
fail=0

# Each fixture ends with a `tohost` write, which stops spike, and an `ebreak`,
# which stops us one step later; `NF > 2` drops the final write-free step.
read -r -d '' base <<'EOF' || true
    li t0, 0x80001000
    li t1, 0x41
    sw t1, 0(t0)
    lw a0, 0(t0)
    addi a1, zero, 7
    add a2, a0, a1
    sub a3, a2, a1
    and a4, a2, a0
    or a5, a4, a1
    xor a6, a5, a0
    slli a7, a6, 2
    srli s0, a7, 1
    srai s1, s0, 3
    slt s2, s1, a0
    sltu s3, s2, a1
    lui s4, 0x12345
    auipc s5, 0x1
    sw s4, 4(t0)
    lw s6, 4(t0)
EOF

fixture() {
    local name=$1 isa=$2 march=$3 body=$4
    {
        printf '    .text\n    .globl _start\n_start:\n'
        printf '%s\n' "$body"
        printf '    la t2, tohost\n    li t3, 1\n    sw t3, 0(t2)\n    ebreak\n\n'
        printf '    .section .tohost, "aw", @progbits\n    .align 6\n'
        printf '    .globl tohost\ntohost:\n    .word 0\n    .word 0\n'
        printf '    .align 6\n    .globl fromhost\nfromhost:\n    .word 0\n    .word 0\n'
    } >"$work/$name.s"
    run_case "$name" "$isa" "$march" "$work/$name.s"
}

fixture integer rv32i rv32i "$base"
fixture multiply rv32im rv32im "$base
    li t4, 6
    li t5, 7
    mul s7, t4, t5
    mulh s8, t4, t5
    mulhu s9, t4, t5
    mulhsu s10, t4, t5
    div s11, t5, t4
    divu gp, t5, t4
    rem tp, t5, t4
    remu fp, t5, t4"

fixture compressed rv32ic rv32ic "$base
    li s0, 3
    c.li s1, 4
    c.add s0, s1
    c.addi s0, 1
    c.slli s0, 2
    c.mv t6, s0
    c.srli s0, 1
    c.andi s0, 7
    c.sub s0, s1
    c.xor s0, s1
    c.or s0, s1
    c.and s0, s1"

fixture atomics rv32ia rv32ia "$base
    li t4, 0x80001010
    li t5, 5
    sw t5, 0(t4)
    amoswap.w t6, t5, (t4)
    amoadd.w t6, t5, (t4)
    amoxor.w t6, t5, (t4)
    amoor.w t6, t5, (t4)
    amoand.w t6, t5, (t4)
    amomin.w t6, t5, (t4)
    amomax.w t6, t5, (t4)
    lr.w t6, (t4)
    sc.w t6, t5, (t4)"

# Spike traps an F instruction while mstatus.FS is Off; our simulator does not
# model FS, so the fixture enables it for both.
fixture float rv32if rv32if "$base
    li t0, 0x2000
    csrw mstatus, t0
    li t4, 0x3fc00000
    fmv.w.x f0, t4
    li t5, 0x40100000
    fmv.w.x f1, t5
    fadd.s f2, f0, f1
    fsub.s f3, f1, f0
    fmul.s f4, f0, f1
    fdiv.s f5, f1, f0
    fsw f2, 8(t0)
    flw f6, 8(t0)
    fmin.s f7, f0, f1
    fmax.s f8, f0, f1
    feq.s t6, f0, f1
    flt.s t6, f0, f1
    fle.s t6, f0, f1
    fcvt.w.s t6, f2
    fcvt.s.w f9, t6"

echo "verify-trace: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
