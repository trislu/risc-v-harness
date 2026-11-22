# Floating point: the F extension keeps its own register file.
# Values are loaded as bit patterns, so the arithmetic is visible in the integer
# registers after `fmv.x.w`.
    .attribute arch, "rv32i2p1_f2p0"
    .text
    .globl _start
_start:
    # Floating-point state is off until a program enables it in mstatus.FS.
    li t0, 0x2000
    csrw mstatus, t0
    li t0, 0x3fc00000        # 1.5
    fmv.w.x f0, t0
    li t1, 0x40100000        # 2.25
    fmv.w.x f1, t1
    fadd.s f2, f0, f1        # 3.75
    fmv.x.w a0, f2
    fsw f2, 0x100(zero)
    flw f3, 0x100(zero)
    fsub.s f4, f1, f0        # 0.75
    fmv.x.w a1, f4
    ebreak
