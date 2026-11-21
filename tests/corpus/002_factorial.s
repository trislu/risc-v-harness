# a0 = 5! by recursion and repeated addition, without the M extension.
    .text
    .globl _start
_start:
    li a0, 5
    jal ra, factorial
    ebreak

factorial:
    addi sp, sp, -16
    sw ra, 12(sp)
    sw a0, 8(sp)
    li t0, 1
    ble a0, t0, base
    addi a0, a0, -1
    jal ra, factorial
    lw t1, 8(sp)
    li t2, 0
accumulate:
    add t2, t2, a0
    addi t1, t1, -1
    bnez t1, accumulate
    mv a0, t2
base:
    lw ra, 12(sp)
    addi sp, sp, 16
    ret
