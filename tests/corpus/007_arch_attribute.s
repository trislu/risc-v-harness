# Declare the whole file's target with `.attribute arch`.
# The attribute replaces the target, so it has to name every extension the file
# uses: dropping `zicsr` here would make the `csrr` below illegal.
    .attribute arch, "rv32i2p1_m2p0_zicsr2p0"
    .text
    .globl _start
_start:
    li a0, 6
    li a1, 7
    mul a2, a0, a1
    csrr a3, mstatus
    ebreak
