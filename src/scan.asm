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

// args: count pal
.global tmds_scan_4bpp_pal
.type tmds_scan_4bpp_pal,%function
.thumb_func
tmds_scan_4bpp_pal:
    push {r0, r3} // save registers, freeing them for use
    lsls r4, r5, #2 // number of bytes to output
    add r4, r2 // pointer to end of output buffer, blue channel
    mov ip, r4 // store in hi register
    mov r8, r6 // store palette in hi register
    adds r0, r2, r3 // pointer to output buffer, green channel
    adds r3, r0 // pointer to output buffer, red channel
    lsrs r5, #2 // number of 8-pixel chunks to output
    beq 2f // skip 8-pixel chunk section if zero
    lsls r5, #4 // number of bytes in 8-pixel chunk section
    adds r5, r2 // pointer to end of 8-pixel chunk section
    mov r10, r5 // store in hi register (for comparison)
1:
    ldmia r1!, {r4} // load a word of pixels - 8 pixels, 4bpp each
    uxtb r5, r4 // extract first byte from input pixels
    lsls r5, #4  // palette LUT is 16 bytes per entry
    add r5, r8 // pointer to palette LUT entry
    ldm r5, {r5, r6, r7} // load blue, green, red TMDS pairs
    stmia r2!, {r5} // store blue TMDS pair to output buffer
    stmia r0!, {r6} // store green TMDS pair to output buffer
    stmia r3!, {r7} // store red TMDS pair to output buffer
    lsrs r4, #8 // shift pixels
    uxtb r5, r4 // extract second byte from input pixels
    lsls r5, #4 // above sequence repeats 3 more times
    add r5, r8
    ldm r5, {r5, r6, r7}
    stmia r2!, {r5}
    stmia r0!, {r6}
    stmia r3!, {r7}
    lsrs r4, #8
    uxtb r5, r4
    lsls r5, #4
    add r5, r8
    ldm r5, {r5, r6, r7}
    stmia r2!, {r5}
    stmia r0!, {r6}
    stmia r3!, {r7}
    lsrs r4, #8
    uxtb r5, r4
    lsls r5, #4
    add r5, r8
    ldm r5, {r5, r6, r7}
    stmia r2!, {r5}
    stmia r0!, {r6}
    stmia r3!, {r7}
    cmp r2, r10 // compare output pointer to end of 8-pixel section
    bne 1b // loop if there is more to compute
    cmp r2, ip // compare output pointer to end
    beq 4f // skip (if count is divisible by 8 pixels)
2:
    ldmia r1!, {r4} // load last word of input pixels
3:
    uxtb r5, r4 // extract one byte
    lsls r5, #4 // palette LUT is 16 bytes per entry
    add r5, r8 // pointer to palette LUT entry
    ldm r5, {r5, r6, r7} // load red, green, blue TMDS pairs
    stmia r2!, {r5} // store blue channel
    stmia r0!, {r6} // store green channel
    stmia r3!, {r7} // store red channel
    lsrs r4, #8 // shift pixels to line up next byte
    cmp r2, ip // compare output pointer to end
    bne 3b // loop if there are more pixels
4:
    pop {r0, r3} // restore scratch registers
    ldmia r0!, {r4, r5, r6} // load function ptr and 2 args for next op
    bx r4 // jump to code for op

///

// This entry point is put in .data so it doesn't generate a thunk.
.section .data

.global video_scan
.type video_scan,%function
.thumb_func
// r0: scan list in direct threaded format
// r1: input buffer
// r2: output buffer
video_scan:
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

.global video_scan_stop
.type video_scan_stop,%function
.thumb_func
video_scan_stop:
    subs r0, #8
    pop {r4, r5, r6}
    mov r8, r4
    mov r9, r5
    mov r10, r6
    pop {r4, r5, r6, r7}
    bx lr

// args: count rgb (16 bpp)
.global video_scan_solid_16
.type video_scan_solid_16,%function
.thumb_func
video_scan_solid_16:
    tst r2, #2
    itt ne
    subne r5, #1
    strhne r6, [r2]!
    // Should this be done by caller?
    orr r6, r6, r6, lsl #16
    subs r5, #4
    mov r7, r6
    blo 3f
2:
    subs r5, #4
    stmia r2!, {r6, r7}
    bhs 2b
3:
    adds r5, #2
    it hs
    stmiahs r2!, {r6}
    tst r5, #1
    it ne
    strhne r6, [r2]!
    ldmia r0!, {r4, r5, r6}
    bx r4

.macro video_scan_1bpp_pal_16_2bits lsb0 lsb1
    ubfx r3, r4, \lsb0, #2
    ubfx r7, r4, \lsb1, #2
    ldr r3, [r6, r3, lsl #2]
    ldr r7, [r6, r7, lsl #2]
    stmia r2!, {r3, r7}
.endm

// args: count pal
.global video_scan_1bpp_pal_16
.type video_scan_1bpp_pal_16,%function
.thumb_func
video_scan_1bpp_pal_16:
    tst r2, #2
    bne 6f
    subs r5, #32
    blo 3f
2:
    ldmia r1!, {r4}
    subs r5, #32
    video_scan_1bpp_pal_16_2bits #0 #2
    video_scan_1bpp_pal_16_2bits #4 #6
    video_scan_1bpp_pal_16_2bits #8 #10
    video_scan_1bpp_pal_16_2bits #12 #14
    video_scan_1bpp_pal_16_2bits #16 #18
    video_scan_1bpp_pal_16_2bits #20 #22
    video_scan_1bpp_pal_16_2bits #24 #26
    video_scan_1bpp_pal_16_2bits #28 #30
    bhs 2b
3:
    adds r5, #32 // r5 = count % 32
    beq 5f
    ldmia r1!, {r4}
4:
    subs r5, #2
    ubfx r3, r4, #0, #2
    lsr r4, #2
    ldr r3, [r6, r3, lsl #2]
    stmia r2!, {r3}
    bhi 4b
    it ne
    subne r2, #2
5:
    ldmia r0!, {r4, r5, r6}
    bx r4
6:
    // r2 is aligned to odd pixel
    subs r5, #32
    blo 8f
7:
    ldmia r1!, {r4}
    subs r5, #32
    ubfx r3, r4, #0, #1
    ldr r3, [r6, r3, lsl #2]
    strh r3, [r2]!
    video_scan_1bpp_pal_16_2bits #1 #3
    video_scan_1bpp_pal_16_2bits #5 #7
    video_scan_1bpp_pal_16_2bits #9 #11
    video_scan_1bpp_pal_16_2bits #13 #15
    video_scan_1bpp_pal_16_2bits #17 #19
    video_scan_1bpp_pal_16_2bits #21 #23
    video_scan_1bpp_pal_16_2bits #25 #27
    ubfx r3, r4, #29, #2
    ldr r3, [r6, r3, lsl #2]
    stmia r2!, {r3}
    ubfx r3, r4, #31, #1
    ldr r3, [r6, r3, lsl #2]
    strh r3, [r2]!
    bhs 7b
8:
    adds r5, #32 // r5 = count % 32
    beq 13f
    ldmia r1!, {r4}
    subs r5, #2
    ubfx r3, r4, #0, #1
    ldr r3, [r6, r3, lsl #2]
    strh r3, [r2]!
    bls 12f
9:
    subs r5, #2
    ubfx r3, r4, #1, #2
    lsr r4, #2
    ldr r3, [r6, r3, lsl #2]
    stmia r2!, {r3}
    bhi 9b
12:
    blo 13f
    ubfx r3, r4, #1, #1
    ldr r3, [r6, r3, lsl #2]
    strh r3, [r2]!
13:
    ldmia r0!, {r4, r5, r6}
    bx r4

// args: count pal
// palette has 256 4 byte entries, each two pixels
// currently restricted to even
.global video_scan_4bpp_pal_16
.type video_scan_4bpp_pal_16,%function
.thumb_func
video_scan_4bpp_pal_16:
    subs r5, #8
    blo 3f
2:
    ldmia r1!, {r4}
    subs r5, #8
    uxtb r3, r4
    ubfx r7, r4, #8, #8
    ldr r3, [r6, r3, lsl #2]
    ldr r7, [r6, r7, lsl #2]
    stmia r2!, {r3, r7}
    ubfx r3, r4, #16, #8
    ubfx r7, r4, #24, #8
    ldr r3, [r6, r3, lsl #2]
    ldr r7, [r6, r7, lsl #2]
    stmia r2!, {r3, r7}
    bhs 2b
3:
    adds r5, #8 // r5 = count % 8
    ldmia r0!, {r4}
    beq 5f
    ldmia r1!, {r7}
    subs r5, #2
    uxtb r3, r7
    ldr r3, [r6, r3, lsl #2]
    stmia r2!, {r3}
    bls 4f
    ubfx r3, r7, #8, #8
    ldr r3, [r6, r3, lsl #2]
    subs r5, #2
    stmia r2!, {r3}
    bls 4f
    ubfx r3, r7, #16, #8
    ldr r3, [r6, r3, lsl #2]
    subs r5, #2
    stmia r2!, {r3}
    bls 4f
    // count % 8 == 7
    ubfx r3, r7, #24, #8
    ldrh r3, [r6, r3, lsl #2]
    strh r3, [r2]!
    ldmia r0!, {r5, r6}
    bx r4
    .align
4:
    it ne
    subne r2, #2
5:
    ldmia r0!, {r5, r6}
    bx r4

// TODO finish
.global sprite_4bpp
.type sprite_4bpp,%function
.thumb_func
sprite_4bpp:
    ldmia r1!, {r4}
    ubfx r3, r4, #0, #4
    lsls r3, r3, #1
    itt ne
    ldrhne r3, [r6, r3]
    strhne r3, [r2]
    // 6 elided
    ubfx r3, r4, #28, #4
    lsls r3, r3, #1
    itt ne
    ldrhne r3, [r6, r3]
    strhne r3, [r2, #14]
    bx lr
