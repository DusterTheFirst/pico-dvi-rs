# Copy data from flash into scratch_x and scratch_y ram banks since this
# is not handled already
# See https://github.com/rp-rs/rp-hal/issues/576 for more discusssion.

.section .text
.align 4

data_cpy_table:
    .word _scratch_x_source
    .word _scratch_x_start
    .word _scratch_x_end
    .word _scratch_y_source
    .word _scratch_y_start
    .word _scratch_y_end
    .word 0

.global __pre_init
.type __pre_init,%function

.thumb_func
__pre_init:
    push {r4, lr}
    ldr r4, =data_cpy_table

1:
    ldmia r4!, {r1-r3}
    cmp r1, #0
    beq 2f
    bl data_cpy
    b 1b
2:
    pop {r4, pc}
    data_cpy_loop:
    ldm r1!, {r0}
    stm r2!, {r0}
    data_cpy:
    cmp r2, r3
    blo data_cpy_loop
    bx lr