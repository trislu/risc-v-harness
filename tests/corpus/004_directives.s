# Data directives, and a symbol reached through la.
    .section .data
message:
    .string "hello"
    .align 2
    .word 0x12345678
    .byte 1, 2, 3
    .equ SIZE, 4

    .section .text
    .globl _start
_start:
    la t0, message
    li t1, SIZE
    ebreak
