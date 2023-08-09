// This entry point is put in .data so it doesn't generate a thunk.
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

// Hot loops are in .scratch_x to reduce RAM contention.
.section .scratch_x

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

.macro tmds_scan_1bpp_pal_body shift_instr shamt
	\shift_instr r5, r4, #\shamt
	ands r5, r0 // r0 = mask, equals 0x30
	add r5, r8 // r8 = pal
	ldm r5, {r5, r6, r7}
	str r6, [r2, r3] // r3 = stride
	adds r6, r2, r3
	str r7, [r6, r3]
	stmia r2!, {r5}
.endm

1:
    b 4f
// args: count pal
.global tmds_scan_1bpp_pal
.type tmds_scan_1bpp_pal,%function
.thumb_func
tmds_scan_1bpp_pal:
    lsrs r4, r5, #4
    lsls r5, #2
    adds r5, r2
    mov ip, r5 // actual end of output
    mov r8, r6
    mov r9, r0
    lsls r4, #6
    beq 1b
    adds r4, r2
    mov r10, r4 // end of whole part
    movs r0, #0x30
2:
	ldmia r1!, {r4}
	tmds_scan_1bpp_pal_body lsls 4
	tmds_scan_1bpp_pal_body lsls 2
	tmds_scan_1bpp_pal_body lsls 0
	tmds_scan_1bpp_pal_body lsrs 2
	tmds_scan_1bpp_pal_body lsrs 4
	tmds_scan_1bpp_pal_body lsrs 6
	tmds_scan_1bpp_pal_body lsrs 8
	tmds_scan_1bpp_pal_body lsrs 10
	tmds_scan_1bpp_pal_body lsrs 12
	tmds_scan_1bpp_pal_body lsrs 14
	tmds_scan_1bpp_pal_body lsrs 16
	tmds_scan_1bpp_pal_body lsrs 18
	tmds_scan_1bpp_pal_body lsrs 20
	tmds_scan_1bpp_pal_body lsrs 22
	tmds_scan_1bpp_pal_body lsrs 24
	tmds_scan_1bpp_pal_body lsrs 26
	cmp r2, r10
	beq 3f
    b 2b
3:
    cmp r2, ip
    beq 6f
4:
    ldmia r1!, {r4}
    movs r0, #2
5:
	rors r4, r0
    lsrs r5, r4, #30
    lsls r5, #4
	add r5, r8 // r8 = pal
	ldm r5, {r5, r6, r7}
	str r6, [r2, r3] // r3 = stride
	adds r6, r2, r3
	str r7, [r6, r3]
	stmia r2!, {r5}
    cmp r2, ip
    bne 5b
6:
    mov r0, r9
    ldmia r0!, {r4, r5, r6}
    bx r4
