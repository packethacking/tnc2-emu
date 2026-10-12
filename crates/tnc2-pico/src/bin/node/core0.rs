//! Core 0: the Z80 board and nothing else.
//!
//! `tnc2_node::engine` does the work, a slice at a time: it runs the
//! board in 1 ms slices of emulated time, each one only once real time
//! (the RP2350's microsecond timer) has reached its end, so the node's
//! clock never runs ahead; behind, it runs flat out until it has caught up.
//! Between slices it moves frames and console bytes through the pipes,
//! copies the RAM out when core 1 asks, and changes boards when told to.
//! Nothing here waits for core 1.
//!
//! No interrupts are taken on this core, and nothing it runs is in flash
//! (memory.x): core 1 erases and programs the flash while this carries on.
//! To make sure, the memory protection unit refuses to run anything from
//! flash on this core, so a mistake there stops the emulator at once
//! (and the watchdog restarts the chip) rather than one day in a hundred,
//! when a save happens to coincide.

use crate::{BOARD, CONSOLE_PIPE, COUNTERS, RADIO_PIPE};
use embassy_time::Instant;
use tnc2_node::engine::Engine;

pub type Io = tnc2_node::engine::Io<RADIO_PIPE, CONSOLE_PIPE>;

pub fn run(mut io: Io) -> ! {
    no_interrupts();
    no_code_from_flash();
    let mut engine = Engine::new();
    loop {
        engine.step(&mut io, &BOARD, &COUNTERS, || Instant::now().as_micros());
    }
}

/// Mask every interrupt on this core, for good.
fn no_interrupts() {
    cortex_m::interrupt::disable();
    // SAFETY: only this core's own interrupt controller is touched (each
    // core has one), and only to disable lines.
    let nvic = unsafe { &*cortex_m::peripheral::NVIC::PTR };
    for r in nvic.icer.iter() {
        // SAFETY: as above.
        unsafe { r.write(!0) };
    }
}

/// Mark the flash (all of 0x10000000 to 0x1FFFFFFF, the XIP window and
/// its aliases) execute never and read only for this core, with the
/// Armv8-M memory protection unit. Everything else keeps the default
/// memory map. (Reads cannot be refused to privileged code this way; the
/// linker script sees to those.)
fn no_code_from_flash() {
    const MPU_CTRL: *mut u32 = 0xE000_ED94usize as _;
    const MPU_RNR: *mut u32 = 0xE000_ED98usize as _;
    const MPU_RBAR: *mut u32 = 0xE000_ED9Cusize as _;
    const MPU_RLAR: *mut u32 = 0xE000_EDA0usize as _;
    const MPU_MAIR0: *mut u32 = 0xE000_EDC0usize as _;
    // Attribute 0: normal memory, not cacheable (the M33 cores here have
    // no cache anyway).
    const ATTR_NORMAL: u32 = 0x44;
    // Base, not shareable, read only at any privilege (AP 0b11), XN.
    const RBAR: u32 = 0x1000_0000 | (0b11 << 1) | 1;
    // Limit (the last 32 byte block), attribute 0, enabled.
    const RLAR: u32 = 0x1FFF_FFE0 | 1;
    // SAFETY: this core's own MPU, which nothing else here uses
    // (embassy-rp guards stacks with MSPLIM on the RP2350).
    unsafe {
        MPU_CTRL.write_volatile(0);
        cortex_m::asm::dsb();
        MPU_MAIR0.write_volatile(ATTR_NORMAL);
        MPU_RNR.write_volatile(0);
        MPU_RBAR.write_volatile(RBAR);
        MPU_RLAR.write_volatile(RLAR);
        // Enabled, with the default map everywhere else (PRIVDEFENA).
        MPU_CTRL.write_volatile(0b101);
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
    }
}
