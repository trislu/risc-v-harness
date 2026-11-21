# Write "Hi" and a newline to the console at 0x10000000, then halt.
# The console is a byte-wide memory-mapped write (ADR-0006).
    .section .text
    .globl _start
_start:
    li t0, 0x10000000
    li t1, 72
    sb t1, 0(t0)
    li t1, 105
    sb t1, 0(t0)
    li t1, 10
    sb t1, 0(t0)
    ebreak
