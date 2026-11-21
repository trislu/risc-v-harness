# Install a trap handler and read a CSR.
    .text
    .globl _start
_start:
    la t0, handler
    csrw mtvec, t0
    csrr t1, mstatus
    ebreak

handler:
    csrr t0, mcause
    ebreak
