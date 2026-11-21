# Numeric labels: 1b refers backwards and 1f forwards.
    .text
    .globl _start
_start:
    li a0, 3
1:
    addi a0, a0, -1
    bnez a0, 1b
    j 1f
    addi a0, a0, 100
1:
    addi a0, a0, 1
    ebreak
