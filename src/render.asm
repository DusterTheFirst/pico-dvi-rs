// These functions are in .data because they may be called from both cores.
.section .data

// r0: command list in direct threaded format
// r1: output buffer
// r2: y
.global render_engine
.type render_engine,%function
.thumb_func
render_engine:
    push {r4, r5, r6, r7}
    movs r3, #0 // outw

    ldmia r0!, {r4, r5, r6, r7}
    bx r4

.global render_stop
.type render_stop,%function
.thumb_func
render_stop:
    pop {r4, r5, r6, r7}
    bx lr

// args: input, stride, [rs1: u8, ls: u8, rs0: u8]
.global render_blit_simple
.type render_blit_simple,%function
.thumb_func
render_blit_simple:
    muls r6, r2
    ldr r4, [r5, r6]
    lsrs r4, r7
    lsrs r7, #8
    lsls r4, r7
    lsrs r7, #8
    lsrs r4, r7
    orrs r3, r4
    ldmia r0!, {r4, r5, r6, r7}
    bx r4

// args: input, stride, [rs1: u8, ls: u8, rs0: u8]
.global render_blit_out
.type render_blit_out,%function
.thumb_func
render_blit_out:
    muls r6, r2
    ldr r4, [r5, r6]
    lsrs r4, r7
    lsrs r7, #8
    lsls r4, r7
    lsrs r7, #8
    lsrs r4, r7
    orrs r3, r4
    stmia r1!, {r3}
    movs r3, #0
    ldmia r0!, {r4, r5, r6, r7}
    bx r4

// args: input, stride, [rs1: u8, ls1: u8, ls0: u8, rs0: u8]
.global render_blit_straddle
.type render_blit_straddle,%function
.thumb_func
render_blit_straddle:
    muls r6, r2
    ldr r4, [r5, r6]
    mov r5, r4
    lsrs r4, r7
    lsrs r7, #8
    lsls r4, r7
    orrs r3, r4
    stmia r1!, {r3}
    lsrs r7, #8
    lsls r5, r7
    lsrs r7, #8
    lsrs r5, r7
    movs r3, r5
    ldmia r0!, {r4, r5, r6, r7}
    bx r4

// args: input, stride, [rs1: u8, ls1: u8, ls0: u8, rs0: u8]
.global render_blit_straddle_out
.type render_blit_straddle_out,%function
.thumb_func
render_blit_straddle_out:
    muls r6, r2
    ldr r4, [r5, r6]
    mov r5, r4
    lsrs r4, r7
    lsrs r7, #8
    lsls r4, r7
    orrs r3, r4
    stmia r1!, {r3}
    lsrs r7, #8
    lsls r5, r7
    lsrs r7, #8
    lsrs r5, r7
    stmia r1!, {r5}
    movs r3, #0
    ldmia r0!, {r4, r5, r6, r7}
    bx r4

// args: input, stride
.global render_blit_64_aligned
.type render_blit_64_aligned,%function
.thumb_func
render_blit_64_aligned:
    muls r6, r2
    adds r5, r6
    ldm r5, {r4, r5}
    stmia r1!, {r4, r5}
    mov r4, r7
    ldmia r0!, {r5, r6, r7}
    bx r4

// args: input, stride, [ls: u8, rs: u8]
.global render_blit_64_straddle
.type render_blit_64_straddle,%function
.thumb_func
render_blit_64_straddle:
    muls r6, r2
    adds r5, r6
    ldm r5, {r4, r5}
    movs r6, r4
    lsls r4, r7
    orrs r4, r3
    movs r3, r5
    lsls r5, r7
    lsrs r7, #8
    lsrs r6, r7
    orrs r5, r6
    stmia r1!, {r4, r5}
    lsrs r3, r7
    ldmia r0!, {r4, r5, r6, r7}
    bx r4
