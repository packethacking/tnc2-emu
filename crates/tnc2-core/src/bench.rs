//! Flat out speed test: run the board with no I/O and time it. The same
//! loop runs on the host (examples/bench.rs) and on a microcontroller; the
//! only thing the caller supplies is a microsecond clock.

use crate::Board;
use core::fmt;

pub struct Report {
    /// Emulated clock cycles run.
    pub cycles: u64,
    /// Wall clock time taken, in microseconds.
    pub elapsed_us: u64,
    /// The board's nominal clock.
    pub clock_hz: u64,
    /// FNV-1a of the RAM at the end. With no I/O the emulation is
    /// deterministic, so this should match on every machine.
    pub ram_hash: u64,
}

impl Report {
    /// Emulated seconds.
    pub fn emulated_s(&self) -> f64 {
        self.cycles as f64 / self.clock_hz as f64
    }

    /// Real seconds.
    pub fn elapsed_s(&self) -> f64 {
        self.elapsed_us as f64 / 1e6
    }

    /// The CPU clock this machine managed, in MHz.
    pub fn emulated_mhz(&self) -> f64 {
        self.cycles as f64 / self.elapsed_us.max(1) as f64
    }

    /// How many times faster than a real TNC-2. Below 1 cannot keep up.
    pub fn ratio(&self) -> f64 {
        self.emulated_s() / self.elapsed_s().max(1e-6)
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:.1} emulated s in {:.3} s: {:.1} MHz emulated, {:.1}x real time (RAM hash {:016x})",
            self.emulated_s(),
            self.elapsed_s(),
            self.emulated_mhz(),
            self.ratio(),
            self.ram_hash
        )
    }
}

/// Run `board` for `seconds` emulated seconds in 1 ms steps, as a host
/// loop would, and report the speed. `now_us` is any monotonic
/// microsecond clock.
pub fn run(board: &mut Board, seconds: u32, mut now_us: impl FnMut() -> u64) -> Report {
    let step = board.clock_hz() / 1000;
    let start_cycles = board.cycles();
    let start = now_us();
    for _ in 0..seconds as u64 * 1000 {
        board.run(step);
    }
    let elapsed_us = now_us() - start;
    Report {
        cycles: board.cycles() - start_cycles,
        elapsed_us,
        clock_hz: board.clock_hz(),
        ram_hash: crate::rom::fingerprint(&board.ram()[..], &[]),
    }
}
