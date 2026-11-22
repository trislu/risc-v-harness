# Double precision: 64-bit float registers. A double occupies a whole register,
# so the halves are written and read as two words to keep the arithmetic visible.
    .attribute arch, "rv32i2p1_f2p0_d2p0"
    .text
    .globl _start
_start:
    li t0, 0x100
    li t1, 0x3ff80000         # high half of 1.5
    sw t1, 4(t0)
    li t1, 0x40020000         # high half of 2.25
    sw t1, 12(t0)
    fld f0, 0(t0)
    fld f1, 8(t0)
    fadd.d f2, f0, f1         # 3.75
    fsd f2, 16(t0)
    lw a0, 20(t0)             # high half of the sum
    fmadd.d f3, f0, f1, f0    # 1.5 * 2.25 + 1.5 = 4.875
    fsd f3, 24(t0)
    lw a1, 28(t0)
    fcvt.w.d a2, f2           # 3.75 rounds to 4
    fmv.w.x f4, a2            # a single goes into the low half, boxed
    ebreak
