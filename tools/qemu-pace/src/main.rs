//! The Pico node's core 0 loop, `tnc2_node::engine::Engine`, on QEMU's
//! Cortex-M33 board with `-icount shift=0`, pacing against the AN505's free
//! running counter. Virtual time then moves on a fixed amount per
//! instruction, so by saying how many instructions make a second of "real"
//! time, the harness can pretend the processor is faster or slower than
//! the emulator needs, and check what the pacing does on the real
//! instruction set:
//!
//! 1. fast enough: TheNet signs on through the console pipes, emulated
//!    time keeps to real time and is never ahead, and the spare time goes
//!    on waiting;
//! 2. too slow: the node falls behind, steadily, at the speed it can do;
//! 3. fast again: it runs flat out until it has caught up.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use core::cell::Cell;
use core::mem::MaybeUninit;
use core::sync::atomic::Ordering::Relaxed;
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use embedded_alloc::LlffHeap;
use panic_semihosting as _;
use tnc2_core::{Board, BoardConfig, NodeConfig};
use tnc2_node::engine::{Engine, Io};
use tnc2_node::handoff::{BoardSlot, Snapshot};
use tnc2_node::pace::Counters;
use tnc2_node::pipe::Pipe;

#[global_allocator]
static HEAP: LlffHeap = LlffHeap::empty();

static ROM1: &[u8] = include_bytes!("../../../rom/THENET1.X1J");
static ROM2: &[u8] = include_bytes!("../../../rom/THENET2.X1J");

static RADIO_IN: Pipe<8192> = Pipe::new();
static RADIO_OUT: Pipe<8192> = Pipe::new();
static CONSOLE_IN: Pipe<4096> = Pipe::new();
static CONSOLE_OUT: Pipe<4096> = Pipe::new();
static SNAPSHOT: Snapshot = Snapshot::new();
static BOARD: BoardSlot<Board> = BoardSlot::new();
static COUNTERS: Counters = Counters::new();

/// About 15 Arm instructions per Z80 clock (tools/m33bench) is 74 million
/// a second for real time. A pretend processor doing 100 million has room
/// to spare; one doing 55 million does not.
const FAST: u64 = 100_000_000;
const SLOW: u64 = 55_000_000;

/// The AN505 FPGAIO free running counter (secure alias), 32 bits.
fn counter() -> u32 {
    // SAFETY: a read only register of the board.
    unsafe { core::ptr::read_volatile(0x5030_2018 as *const u32) }
}

/// `n` times round a two instruction loop.
#[inline(never)]
fn spin(n: u32) {
    // SAFETY: touches nothing but its own register.
    unsafe { core::arch::asm!("1: subs {0}, #1", "bne 1b", inout(reg) n => _, options(nomem, nostack)) }
}

/// "Real" time in microseconds: instructions run, at `per_s` a second.
struct Clock {
    ticks_per_insn: f64,
    last: Cell<u32>,
    ticks: Cell<u64>,
    base_ticks: Cell<u64>,
    base_us: Cell<u64>,
    per_s: Cell<u64>,
}

impl Clock {
    fn ticks(&self) -> u64 {
        let c = counter();
        self.ticks.set(self.ticks.get() + c.wrapping_sub(self.last.get()) as u64);
        self.last.set(c);
        self.ticks.get()
    }

    fn now_us(&self) -> u64 {
        let insns = (self.ticks() - self.base_ticks.get()) as f64 / self.ticks_per_insn;
        self.base_us.get() + (insns * 1e6 / self.per_s.get() as f64) as u64
    }

    /// Carry on from now at a new pretend speed.
    fn set_speed(&self, per_s: u64) {
        let now = self.now_us();
        self.base_ticks.set(self.ticks());
        self.base_us.set(now);
        self.per_s.set(per_s);
    }
}

fn fail(what: &str) -> ! {
    hprintln!("FAIL: {}", what);
    debug::exit(debug::EXIT_FAILURE);
    loop {
        cortex_m::asm::wfi();
    }
}

#[entry]
fn main() -> ! {
    static mut MEM: [MaybeUninit<u8>; 256 * 1024] = [MaybeUninit::uninit(); 256 * 1024];
    // SAFETY: `entry` makes MEM a unique reference; it is only used here.
    unsafe { HEAP.init(MEM.as_mut_ptr() as usize, MEM.len()) }

    const CALIBRATE: u32 = 10_000_000;
    let t = counter();
    spin(CALIBRATE);
    let calib = counter().wrapping_sub(t);
    let clock = Clock {
        ticks_per_insn: calib as f64 / (2 * CALIBRATE) as f64,
        last: Cell::new(counter()),
        ticks: Cell::new(0),
        base_ticks: Cell::new(0),
        base_us: Cell::new(0),
        per_s: Cell::new(FAST),
    };

    let (_radio_in, core0_radio_in) = RADIO_IN.split();
    let (core0_radio_out, _radio_out) = RADIO_OUT.split();
    let (mut console_in, core0_console_in) = CONSOLE_IN.split();
    let (core0_console_out, mut console_out) = CONSOLE_OUT.split();
    let (_saver, serve) = SNAPSHOT.split();
    let mut io = Io {
        radio_in: core0_radio_in,
        radio_out: core0_radio_out,
        console_in: core0_console_in,
        console_out: core0_console_out,
        snapshot: serve,
    };
    let mut engine = Engine::new();
    let c = &COUNTERS;

    let board = Board::from_rom(ROM1, ROM2, &NodeConfig::default(), &BoardConfig::default()).unwrap();
    let start = clock.now_us();
    if BOARD.start(alloc::boxed::Box::new(board)).is_err() {
        fail("no board slot");
    }
    let elapsed = || clock.now_us() - start;
    let emulated = || c.emulated_us.load(Relaxed) as u64;
    let busy = || c.busy_us.load(Relaxed) as u64;

    // 1. Fast enough, 12 s: sign on at the console.
    let mut out = String::new();
    let mut buf = [0u8; 64];
    let mut most_ahead = i64::MIN;
    let (mut sent_esc, mut sent_c) = (false, false);
    while elapsed() < 12_000_000 {
        engine.step(&mut io, &BOARD, c, || clock.now_us());
        let ahead = emulated() as i64 - elapsed() as i64;
        most_ahead = most_ahead.max(ahead);
        let n = console_out.read(&mut buf);
        out.extend(buf[..n].iter().map(|&b| b as char));
        if !sent_esc && elapsed() > 1_000_000 {
            sent_esc = console_in.write_all(b"\x1b");
        }
        if !sent_c && out.contains("* ") {
            sent_c = console_in.write_all(b"C\r");
        }
    }
    let (real1, emu1, busy1) = (elapsed(), emulated(), busy());
    hprintln!(
        "fast: {} ms real, {} ms emulated, busy {:.0}%, at most {} us ahead",
        real1 / 1000,
        emu1 / 1000,
        busy1 as f64 * 100.0 / real1 as f64,
        most_ahead
    );
    if !out.contains("CONNECTED to G8KBB-5") {
        fail("TheNet did not sign on at the console");
    }
    hprintln!("console: TheNet signed on (CONNECTED to G8KBB-5)");
    // Emulated time can only be ahead by the few microseconds the last
    // instruction of a slice overshoots.
    if most_ahead > 20 {
        fail("emulated time ran ahead of real time");
    }
    if real1 - emu1 > 2_000 {
        fail("emulated time fell behind while fast enough");
    }

    // 2. Too slow, 4 s.
    clock.set_speed(SLOW);
    let (r0, e0) = (elapsed(), emulated());
    while elapsed() - r0 < 4_000_000 {
        engine.step(&mut io, &BOARD, c, || clock.now_us());
    }
    let (r1, e1) = (elapsed(), emulated());
    let speed = (e1 - e0) as f64 / (r1 - r0) as f64;
    let behind = c.behind_ms.load(Relaxed);
    hprintln!("slow: speed {:.3}x real time, {} ms behind after 4 s", speed, behind);
    if !(0.55..0.95).contains(&speed) || behind < 200 {
        fail("did not fall behind steadily when too slow");
    }

    // 3. Fast again: flat out until caught up.
    clock.set_speed(FAST);
    let r2 = elapsed();
    while c.behind_ms.load(Relaxed) > 0 {
        engine.step(&mut io, &BOARD, c, || clock.now_us());
        if elapsed() - r2 > 20_000_000 {
            fail("did not catch up within 20 s");
        }
    }
    let took = elapsed() - r2;
    hprintln!("fast again: caught up {} ms in {} ms of real time", behind, took / 1000);
    hprintln!("OK");
    debug::exit(debug::EXIT_SUCCESS);
    loop {
        cortex_m::asm::wfi();
    }
}
