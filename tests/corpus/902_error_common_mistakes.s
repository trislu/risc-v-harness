# Deliberately broken in the ways tutorials usually are.
    li a0, 0 ; zero the counter
    adii a0, a1, 1
    addi a0, a1, 4096
    csrr a0, mstat
    .wor 1
