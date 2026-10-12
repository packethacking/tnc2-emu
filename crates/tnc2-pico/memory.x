/* Raspberry Pi Pico 2 W: RP2350A, 4 MB of QSPI flash, 520 KB of SRAM.
 *
 * Flash:
 *   0x10000000  3840 KB  FLASH, the firmware (vector table, IMAGE_DEF, code)
 *   0x103C0000   252 KB  STORE, kept out of the image for the battery backed
 *                        RAM and the node settings (not used yet)
 *   0x103FF000     4 KB  left alone: UF2s made with picotool --abs-block
 *                        (the Pico SDK's default, for erratum RP2350-E10)
 *                        write a block here
 *
 * SRAM0 to SRAM7 are striped across eight banks, which spreads the load of
 * code and data evenly. SRAM8 and SRAM9 are 4 KB each, not striped.
 */

MEMORY {
    FLASH : ORIGIN = 0x10000000, LENGTH = 4096K - 256K
    STORE : ORIGIN = 0x103C0000, LENGTH = 252K
    RAM   : ORIGIN = 0x20000000, LENGTH = 512K
    SRAM8 : ORIGIN = 0x20080000, LENGTH = 4K
    SRAM9 : ORIGIN = 0x20081000, LENGTH = 4K
}

__store_start = ORIGIN(STORE);
__store_end = ORIGIN(STORE) + LENGTH(STORE);

SECTIONS {
    /* The boot ROM looks for the IMAGE_DEF block in the first 4 KB of
     * flash, so it goes straight after the vector table. */
    .start_block : ALIGN(4)
    {
        __start_block_addr = .;
        KEEP(*(.start_block));
        KEEP(*(.boot_info));
    } > FLASH

    /* The emulator: every function of tnc2-core and of the Z80 core, and
     * the firmware's own functions marked #[link_section = ".ram_code"],
     * kept in flash and copied to SRAM by ram_code::load() at the top of
     * main.
     * Running it from SRAM gives it zero wait state fetches instead of
     * depending on the 16 KB XIP cache, which the emulator would overflow.
     * This has to come before .text in the script, or .text's own
     * `*(.text.*)` would claim these sections first. */
    .ram_code : ALIGN(8)
    {
        __ram_code_start = .;
        *(.ram_code .ram_code.*)
        *(.text.*tnc2_core*)
        *(.text.*rustzx_z80*)
        *(.rodata.*tnc2_core*)
        *(.rodata.*rustzx_z80*)
        . = ALIGN(8);
        __ram_code_end = .;
    } > RAM AT > FLASH
    __ram_code_load = LOADADDR(.ram_code);
} INSERT AFTER .vector_table;

/* .text starts after the copy of the RAM code. */
_stext = ALIGN(LOADADDR(.ram_code) + SIZEOF(.ram_code), 8);

SECTIONS {
    /* picotool 'binary info' entries (program name and version). */
    .bi_entries : ALIGN(4)
    {
        __bi_entries_start = .;
        KEEP(*(.bi_entries));
        . = ALIGN(4);
        __bi_entries_end = .;
    } > FLASH
} INSERT AFTER .text;

SECTIONS {
    .end_block : ALIGN(4)
    {
        __end_block_addr = .;
        KEEP(*(.end_block));
    } > FLASH
} INSERT AFTER .uninit;

PROVIDE(start_to_end = __end_block_addr - __start_block_addr);
PROVIDE(end_to_start = __start_block_addr - __end_block_addr);
