/* Raspberry Pi Pico 2 W: RP2350A, 4 MB of QSPI flash, 520 KB of SRAM.
 *
 * Flash:
 *   0x10000000  3840 KB  FLASH, the firmware (vector table, IMAGE_DEF, code)
 *   0x103C0000   252 KB  STORE, kept out of the image for the battery backed
 *                        RAM and the node settings (tnc2_node::store)
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

    /* What runs before the code is in SRAM: embassy-rp's __pre_init,
     * which calls nothing, and ram_code.rs's main, which does the copy. */
    .flash_text : ALIGN(4)
    {
        *(.text.__pre_init)
        *(.flash_text .flash_text.*)
    } > FLASH

    /* All the code and read only data, kept in flash and copied to SRAM
     * before main (ram_code.rs). Two reasons:
     * - speed: SRAM has no wait states, while code in flash is fetched
     *   through a 16 KB cache that the emulator alone would overflow;
     * - flash writes: while the boot ROM erases or programs the flash,
     *   any read of it from the other core is a bus fault, so the node's
     *   core 0 must never touch flash. With everything in SRAM it cannot.
     * What stays in flash: the vector table, boot block and start up code
     * above, cortex-m-rt's reset and hard fault handlers (.Reset and
     * .HardFault, which run before the copy or only when all is lost),
     * picotool's binary info, and the EPROM images (.flash_rodata), read
     * only while a board is built, which never overlaps a flash write.
     * This has to come before .text in the script, or .text's own
     * `*(.text .text.*)` would claim these sections first. */
    .ram_code : ALIGN(8)
    {
        __ram_code_start = .;
        *(.ram_code .ram_code.*)
        *(.text .text.*)
        *(.rodata .rodata.*)
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

    /* The EPROM images (rom.rs). */
    .flash_rodata : ALIGN(4)
    {
        *(.flash_rodata .flash_rodata.*)
    } > FLASH
} INSERT AFTER .text;
