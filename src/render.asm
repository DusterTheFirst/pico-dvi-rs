.section .data

.global tmds_scan
.type tmds_scan,%function
.thumb_func
// r0: scan list in direct threaded format
// r1: input buffer
// r2: output buffer
// r3: stride (of output buffer)
tmds_scan:
    push {r4, r5, r6, r7}
    mov r4, r8
    mov r5, r9
    mov r6, r10
    push {r4, r5, r6}

    // operation, 2x args
    // should count be single pixels or double?
    ldmia r0!, {r4, r5, r6}
    bx r4

.global tmds_scan_stop
.type tmds_scan_stop,%function
.thumb_func
tmds_scan_stop:
    subs r0, #8
    pop {r4, r5, r6}
    mov r8, r4
    mov r9, r5
    mov r10, r6
    pop {r4, r5, r6, r7}
    bx lr

// args: count tmds_blue tmds_green tmds_red
// Not sure we'll keep this.
.global tmds_scan_solid_tmds
.type tmds_scan_solid_tmds,%function
.thumb_func
tmds_scan_solid_tmds:
    mov r8, r1
    lsls r5, #2
    adds r4, r2, r5
    // ip is actual end of output
    mov ip, r4
    lsls r5, #28
    lsrs r5, #28
    adds r4, r2, r5
    // r10 is end of fractional part (may be == r2)
    mov r10, r4
    adds r7, r2, r3 // beginning of green row
    cmp r2, r10
    beq 2f
1:
    stmia r2!, {r6}
    cmp r2, r10
    bne 1b
    cmp r2, ip
    beq 4f
2:
    mov r5, r6
3:
    stmia r2!, {r5, r6}
    stmia r2!, {r5, r6}
    cmp r2, ip
    bne 3b
4:

    add ip, r3
    add r10, r3
    ldmia r0!, {r4, r6}
    adds r1, r7, r3 // beginning of red row
    cmp r7, r10
    beq 2f
1:
    stmia r7!, {r4}
    cmp r7, r10
    bne 1b
    cmp r7, ip
    beq 4f
2:
    mov r5, r4
3:
    stmia r7!, {r4, r5}
    stmia r7!, {r4, r5}
    cmp r7, ip
    bne 3b
4:
    // write red
    add ip, r3
    add r10, r3
    cmp r1, r10
    beq 2f
1:
    stmia r1!, {r6}
    cmp r1, r10
    bne 1b
    cmp r1, ip
    beq 4f
2:
    mov r7, r6
3:
    stmia r1!, {r6, r7}
    stmia r1!, {r6, r7}
    cmp r1, ip
    bne 3b
4:
    mov r1, r8
    ldmia r0!, {r4, r5, r6}
    bx r4

