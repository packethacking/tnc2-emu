//! The watchdog, driven directly. embassy-rp 0.10's `Watchdog` writes the
//! RP2040's PSM WDSEL mask, which on the RP2350 leaves both processors out
//! of the reset and puts the crystal oscillator in, so this does what the
//! Pico SDK does instead: reset everything but the two oscillators.
//!
//! The watchdog counts 1 us ticks made from the 12 MHz reference clock, not
//! the system clock, so it still fires if a system clock is too fast for
//! the chip and the processor stops. Its scratch registers 0 to 3 survive
//! the reset (the boot ROM uses 4 to 7).

use embassy_rp::pac;

/// Why the chip last reset, if the watchdog did it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// The timer ran out: nobody fed it.
    TimedOut,
    /// `reset_now`.
    Forced,
}

pub fn reason() -> Option<Reason> {
    let r = pac::WATCHDOG.reason().read();
    if r.force() {
        Some(Reason::Forced)
    } else if r.timer() {
        Some(Reason::TimedOut)
    } else {
        None
    }
}

fn select_reset() {
    const ALL: u32 = 0x01ff_ffff;
    const ROSC: u32 = 1 << 2;
    const XOSC: u32 = 1 << 3;
    pac::PSM.wdsel().write_value(pac::psm::regs::Wdsel(ALL & !(ROSC | XOSC)));
}

/// Start the watchdog: the chip resets unless `feed` is called within
/// `timeout_us` microseconds (at most about 16 s).
pub fn start(timeout_us: u32) {
    let w = pac::WATCHDOG;
    w.ctrl().modify(|c| c.set_enable(false));
    select_reset();
    feed(timeout_us);
    w.ctrl().modify(|c| c.set_enable(true));
}

pub fn feed(timeout_us: u32) {
    pac::WATCHDOG.load().write(|l| l.set_load(timeout_us));
}

pub fn stop() {
    pac::WATCHDOG.ctrl().modify(|c| c.set_enable(false));
}

/// Reset the chip now.
pub fn reset_now() -> ! {
    select_reset();
    pac::WATCHDOG.ctrl().modify(|c| c.set_trigger(true));
    loop {
        cortex_m::asm::nop();
    }
}

/// Scratch register 0 to 3.
pub fn scratch(i: usize) -> u32 {
    let w = pac::WATCHDOG;
    match i {
        0 => w.scratch0().read(),
        1 => w.scratch1().read(),
        2 => w.scratch2().read(),
        3 => w.scratch3().read(),
        _ => panic!("scratch {i} belongs to the boot ROM"),
    }
}

pub fn set_scratch(i: usize, v: u32) {
    let w = pac::WATCHDOG;
    match i {
        0 => w.scratch0().write_value(v),
        1 => w.scratch1().write_value(v),
        2 => w.scratch2().write_value(v),
        3 => w.scratch3().write_value(v),
        _ => panic!("scratch {i} belongs to the boot ROM"),
    }
}
