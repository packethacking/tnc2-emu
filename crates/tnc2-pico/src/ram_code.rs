//! The firmware runs from SRAM. memory.x gathers all the code and read
//! only data into the `.ram_code` section, linked to run in SRAM but
//! stored in flash, and `main` below copies it across. Only the start up
//! code, the hard fault handler, the vector table and the EPROM images stay
//! in flash (memory.x says why). A binary defines its entry point as
//!
//! ```ignore
//! #[unsafe(no_mangle)]
//! fn tnc2_main() -> ! { ... }
//! ```
//!
//! Why: code in flash is fetched through a 16 KB cache, and a miss costs a
//! QSPI read of tens of clock cycles. The emulator's hot code is about
//! 20 KB, more than the cache holds, so running it from flash would make
//! its speed depend on what the cache happens to hold. SRAM has no wait
//! states at any clock. And while the flash is being erased or programmed
//! it cannot be read at all, which matters for the node, whose core 0 runs
//! the emulator while core 1 saves the RAM to flash.

// What runs before the copy, from flash: cortex-m-rt's reset handler,
// embassy-rp's `__pre_init` (a few register writes, no calls), and `main`
// below, which copies a word at a time and then jumps to the firmware's
// real entry, `tnc2_main`, in SRAM. memory.x keeps the first two and the
// `.flash_text` section in flash.

unsafe extern "Rust" {
    /// Each binary's entry point, once the code is in SRAM.
    fn tnc2_main() -> !;
}

/// cortex-m-rt's reset handler calls `main` once `.data` and `.bss` are
/// set up.
///
/// # Safety
///
/// Only the reset handler may call it, once.
#[unsafe(naked)]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".flash_text")]
pub unsafe extern "C" fn main() -> ! {
    core::arch::naked_asm!(
        "ldr r0, =__ram_code_start",
        "ldr r1, =__ram_code_end",
        "ldr r2, =__ram_code_load",
        "0:",
        "cmp r0, r1",
        "bhs 1f",
        "ldr r3, [r2], #4",
        "str r3, [r0], #4",
        "b 0b",
        "1:",
        "dsb",
        "isb",
        "ldr r0, ={entry}",
        "bx r0",
        ".ltorg",
        entry = sym tnc2_main,
    )
}

unsafe extern "C" {
    static __ram_code_start: u32;
    static __ram_code_end: u32;
}

/// Size of the code and read only data running from SRAM, in bytes.
pub fn size() -> usize {
    // SAFETY: only the addresses are used.
    unsafe { (&raw const __ram_code_end).offset_from(&raw const __ram_code_start) as usize * 4 }
}
