//! Keeping emulated time locked to real time.
//!
//! Core 0 runs the board in slices of 1 ms of emulated time. Before each
//! slice it waits until real time has reached the end of that slice, so
//! the node's clock is never ahead of the wall clock; when the emulator
//! has fallen behind (a busy moment) there is nothing to wait for and it
//! runs flat out until it has caught up.
//!
//! Core 0 publishes running totals in `Counters`; core 1 turns two
//! readings of them into the speed and load in STATUS (`Meter`).

use core::sync::atomic::{AtomicU32, Ordering};

pub struct Pacer {
    clock_hz: u64,
    origin_us: u64,
    origin_cycles: u64,
}

impl Pacer {
    /// Start counting from `cycles` of emulated time at `now_us` of real
    /// time.
    pub const fn new(clock_hz: u64, now_us: u64, cycles: u64) -> Self {
        Pacer { clock_hz, origin_us: now_us, origin_cycles: cycles }
    }

    /// Emulated microseconds since the start, for a cycle count.
    pub fn emulated_us(&self, cycles: u64) -> u64 {
        // Cycles over the life of a node stay far below 2^64 / 10^6
        // (that is about 120 years at 4.9 MHz).
        (cycles - self.origin_cycles) * 1_000_000 / self.clock_hz
    }

    /// The real time at which emulated time reaches `cycles`.
    pub fn due_us(&self, cycles: u64) -> u64 {
        self.origin_us + self.emulated_us(cycles)
    }

    /// How far emulated time at `cycles` is behind real time `now_us`.
    pub fn behind_us(&self, now_us: u64, cycles: u64) -> u64 {
        now_us.saturating_sub(self.due_us(cycles))
    }
}

/// Running totals from core 0, read by core 1. Each wraps at 2^32; core 1
/// only ever uses the difference between two readings a few seconds
/// apart.
#[derive(Default)]
pub struct Counters {
    /// Emulated microseconds run.
    pub emulated_us: AtomicU32,
    /// Real microseconds spent running the board and moving data, as
    /// opposed to waiting for real time to catch up.
    pub busy_us: AtomicU32,
    /// Slices run: core 0's heartbeat for the watchdog.
    pub slices: AtomicU32,
    /// How far behind real time the node's clock is, in milliseconds.
    pub behind_ms: AtomicU32,
    /// Radio frames handed to the board, and refused by it (its queue was
    /// full).
    pub frames_in: AtomicU32,
    pub frames_refused: AtomicU32,
    /// Radio frames the node sent, and those dropped because the queue to
    /// core 1 was full.
    pub frames_out: AtomicU32,
    pub frames_out_dropped: AtomicU32,
    /// Console bytes from the node dropped because nobody was reading
    /// them fast enough.
    pub console_dropped: AtomicU32,
    /// Radio frames that arrived while no board was running.
    pub frames_no_board: AtomicU32,
}

impl Counters {
    pub const fn new() -> Self {
        Counters {
            emulated_us: AtomicU32::new(0),
            busy_us: AtomicU32::new(0),
            slices: AtomicU32::new(0),
            behind_ms: AtomicU32::new(0),
            frames_in: AtomicU32::new(0),
            frames_refused: AtomicU32::new(0),
            frames_out: AtomicU32::new(0),
            frames_out_dropped: AtomicU32::new(0),
            console_dropped: AtomicU32::new(0),
            frames_no_board: AtomicU32::new(0),
        }
    }

    /// Add to a counter. Core 0 is the only writer of each, so a plain
    /// load and store does (the RP2350 has atomic adds, but this keeps the
    /// hot loop free of retries).
    pub fn add(c: &AtomicU32, n: u32) {
        c.store(c.load(Ordering::Relaxed).wrapping_add(n), Ordering::Relaxed);
    }
}

/// One reading of the counters, with the real time it was taken.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sample {
    pub real_us: u64,
    pub emulated_us: u32,
    pub busy_us: u32,
}

impl Sample {
    pub fn take(c: &Counters, real_us: u64) -> Self {
        Sample {
            real_us,
            emulated_us: c.emulated_us.load(Ordering::Relaxed),
            busy_us: c.busy_us.load(Ordering::Relaxed),
        }
    }
}

/// Speed and load between two samples.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rates {
    /// Emulated time over real time: 1.00 when keeping up.
    pub speed: f32,
    /// Share of real time core 0 was busy, 0 to 1.
    pub busy: f32,
}

impl Rates {
    pub fn between(a: Sample, b: Sample) -> Option<Rates> {
        let real = b.real_us.checked_sub(a.real_us).filter(|&r| r > 0)? as f32;
        let emu = b.emulated_us.wrapping_sub(a.emulated_us) as f32;
        let busy = b.busy_us.wrapping_sub(a.busy_us) as f32;
        Some(Rates { speed: emu / real, busy: (busy / real).min(1.0) })
    }

    /// The speed core 0 could manage flat out: emulated time over the time
    /// it was busy.
    pub fn flat_out(&self) -> Option<f32> {
        (self.busy > 0.0).then(|| self.speed / self.busy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tnc2_core::{Board, BoardConfig, NodeConfig};

    #[test]
    fn due_and_behind() {
        let p = Pacer::new(4_915_200, 1_000, 49_152);
        assert_eq!(p.emulated_us(49_152), 0);
        assert_eq!(p.emulated_us(49_152 + 4_915_200), 1_000_000);
        assert_eq!(p.due_us(49_152 + 4_915), 1_000 + 999);
        assert_eq!(p.behind_us(500, 49_152), 0);
        assert_eq!(p.behind_us(3_000, 49_152 + 4_915), 1_001);
    }

    #[test]
    fn rates() {
        let a = Sample { real_us: 1_000_000, emulated_us: u32::MAX - 10, busy_us: 5 };
        let b = Sample { real_us: 3_000_000, emulated_us: (u32::MAX - 10).wrapping_add(2_000_000), busy_us: 1_600_005 };
        let r = Rates::between(a, b).unwrap();
        assert!((r.speed - 1.0).abs() < 1e-6);
        assert!((r.busy - 0.8).abs() < 1e-6);
        assert!((r.flat_out().unwrap() - 1.25).abs() < 1e-5);
        assert_eq!(Rates::between(b, b), None);
    }

    /// The core 0 loop's pacing against a pretend clock, with the real
    /// firmware. `cost` is the real time one 1 ms slice takes to run.
    /// Returns emulated and real time at the end, and the most emulated
    /// time was ever ahead of real time.
    fn simulate(slices: usize, cost: impl Fn(usize) -> u64) -> (u64, u64, i64) {
        let mut b = Board::from_rom(
            include_bytes!("../../../rom/THENET1.X1J"),
            include_bytes!("../../../rom/THENET2.X1J"),
            &NodeConfig::default(),
            &BoardConfig::default(),
        )
        .unwrap();
        let slice = b.clock_hz() / 1000;
        let mut now = 10_000u64;
        let p = Pacer::new(b.clock_hz(), now, b.cycles());
        let mut most_ahead = i64::MIN;
        for i in 0..slices {
            let due = p.due_us(b.cycles() + slice);
            // wait for real time to reach the end of the slice
            now = now.max(due);
            b.run(slice);
            now += cost(i);
            let ahead = p.due_us(b.cycles()) as i64 - now as i64;
            most_ahead = most_ahead.max(ahead);
        }
        (p.emulated_us(b.cycles()), now - 10_000, most_ahead)
    }

    #[test]
    fn keeps_real_time_when_fast_enough() {
        // each slice takes 0.8 ms: core 0 waits a little every time
        let (emu, real, ahead) = simulate(3_000, |_| 800);
        assert!(ahead <= 0, "emulated time ran ahead by {ahead} us");
        assert!(real - emu < 1_000, "emulated {emu} us, real {real} us");
    }

    #[test]
    fn falls_behind_steadily_when_too_slow() {
        let (emu, real, _) = simulate(3_000, |_| 1_250);
        let speed = emu as f64 / real as f64;
        assert!((speed - 0.8).abs() < 0.01, "speed {speed}");
    }

    #[test]
    fn catches_up_after_a_stall() {
        // fast, but slice 1000 stalls for 200 ms (as a long flash erase
        // would if core 0 had to wait for it): afterwards core 0 runs flat
        // out until it is back in step, and never overshoots.
        let (emu, real, ahead) = simulate(3_000, |i| if i == 1_000 { 200_000 } else { 500 });
        assert!(ahead <= 0);
        assert!(real - emu < 1_000, "still behind by {} us", real - emu);
    }
}
