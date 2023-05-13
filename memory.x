MEMORY {
    BOOT2(rx)   : ORIGIN = 0x10000000, LENGTH = 0x100
    FLASH(rx)  : ORIGIN = 0x10000100, LENGTH = 2048K - 0x100
    RAM(rwx)    : ORIGIN = 0x20000000, LENGTH = 256K
    SCRATCH_X(rwx) : ORIGIN = 0x20040000, LENGTH = 4k
    SCRATCH_Y(rwx) : ORIGIN = 0x20041000, LENGTH = 4k
}

EXTERN(BOOT2_FIRMWARE)

SECTIONS {
    /* ### Boot loader */
    .boot2 ORIGIN(BOOT2) : {
        KEEP(*(.boot2));
    } > BOOT2
} INSERT BEFORE .text;

SECTIONS {
    /* ### Main ram section */
    .ram : {
        *(.ram .ram.*)
        . = ALIGN(4);
    } > RAM AT > FLASH
} INSERT AFTER .data;

SECTIONS {
    /* ### Small 4kb memory sections for high bandwidth code or data per core */
    .scratch_x : {
        _scratch_x_start = .;
        *(.scratch_x .scratch_x.*)
        . = ALIGN(4);
        _scratch_x_end = .;
    } > SCRATCH_X AT > FLASH
    _scratch_x_source = LOADADDR(.scratch_x);

    .scratch_y : {
        _scratch_y_start = .;
        *(.scratch_y .scratch_y.*)
        . = ALIGN(4);
        _scratch_y_end = .;
    } > SCRATCH_Y AT > FLASH
    _scratch_y_source = LOADADDR(.scratch_y);
} INSERT AFTER .rodata;
