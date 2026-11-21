# The embedded base: same instructions, 16 registers.
# `.attribute arch` switches the file to rv32e, so only x0..x15 exist here.
    .attribute arch, "rv32e2p0"
    .text
    .globl _start
_start:
    li a0, 3
    li a1, 4
    add a2, a0, a1          # a2 = x12, the highest register pair E allows here
    ebreak
