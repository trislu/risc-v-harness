# The compressed extension: every instruction here is two bytes wide.
# `--target rv32ic` or `.attribute arch, "rv32i2p1_c2p0"` selects it.
    .attribute arch, "rv32i2p1_c2p0"
    .text
    .globl _start
_start:
    c.li a0, 6
    c.li a1, 7
    c.add a0, a1
    c.addi a0, 1
    c.slli a0, 1
    c.mv a2, a0
    c.ebreak
