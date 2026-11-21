# Sum 1..10 into a0, then stop.
    .text
    .globl _start
_start:
    li a0, 0
    li t0, 1
    li t1, 11
loop:
    add a0, a0, t0
    addi t0, t0, 1
    blt t0, t1, loop
    ebreak
