//! The emulator runs from SRAM. memory.x gathers every function of
//! tnc2-core and of the Z80 core into the `.ram_code` section, linked to
//! run in SRAM but stored in flash, and `load` copies it across.
//!
//! With LTO the emulator's main loop (`Board::run` and the Z80's fetch and
//! memory access) is inlined into whatever calls it, so that caller has to
//! go in SRAM too: mark it `#[unsafe(link_section = ".ram_code")]` and
//! `#[inline(never)]`.
//!
//! Why: code in flash is fetched through a 16 KB cache, and a miss costs a
//! QSPI read of tens of clock cycles. The emulator's hot code is about
//! 20 KB, more than the cache holds, so running it from flash would make
//! its speed depend on what the cache happens to hold. SRAM has no wait
//! states at any clock.

unsafe extern "C" {
    static mut __ram_code_start: u32;
    static mut __ram_code_end: u32;
    static __ram_code_load: u32;
}

/// Copy the emulator into SRAM. Call this first thing in `main`, before
/// anything from tnc2-core runs.
pub fn load() {
    // SAFETY: the symbols come from memory.x and bound a word aligned
    // region of SRAM that nothing else uses, and its copy in flash.
    unsafe {
        let start = &raw mut __ram_code_start;
        let words = (&raw mut __ram_code_end).offset_from(start) as usize;
        core::ptr::copy_nonoverlapping(&raw const __ram_code_load, start, words);
    }
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
}

/// Size of the code running from SRAM, in bytes.
pub fn size() -> usize {
    // SAFETY: only the addresses are used.
    unsafe { (&raw const __ram_code_end).offset_from(&raw const __ram_code_start) as usize * 4 }
}
