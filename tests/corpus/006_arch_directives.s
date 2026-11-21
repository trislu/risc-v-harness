# Change the target mid-file with `.option arch`, the way GNU as allows.
    .text
    .globl _start
_start:
    li a0, 6
    li a1, 7
    .option arch, +m        # M is available from here on
    mul a2, a0, a1
    csrr a3, mstatus
    ebreak
