//! Speed test: can a Pico 2 W emulate the TNC-2 in real time?
//!
//! Runs TheNet from power on for 60 emulated seconds with no I/O, flat out,
//! at 150 MHz and then at 200, 250 and 300 MHz, and reports each on the
//! second USB serial port every few seconds. The RAM hash at the end must
//! match the host's, which shows the emulation was right as well as fast.
//!
//! If a clock is too fast for the board and it hangs, or the firmware
//! panics, the watchdog restarts the chip. The results so far are kept in
//! the watchdog's scratch registers, and after the restart the test
//! reports them, says where it stopped, and tries nothing faster.
//! docs/pico.md says how to read it.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use core::cell::RefCell;
use core::fmt::Write;
use embassy_executor::Spawner;
use embassy_futures::select::select;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Instant, Timer};
use tnc2_core::{Board, BoardConfig, NodeConfig, bench};
use tnc2_pico::clock::{self, STEPS};
use tnc2_pico::{heap, ram_code, usb, watchdog};

static ROM1: &[u8] = include_bytes!("../../../../rom/THENET1.X1J");
static ROM2: &[u8] = include_bytes!("../../../../rom/THENET2.X1J");

/// Emulated seconds per clock, and the RAM hash the host gets after them
/// (`cargo run --release -p tnc2-core --example bench -- 60`; CI checks
/// the two agree).
const SECONDS: u32 = 60;
const HOST_HASH: u64 = 0xae7ab48fd2b132e4;

/// The board runs in 1 ms steps, as `tnc2_core::bench::run` does, this
/// many at a time; between batches USB gets a look in. 50 emulated ms is
/// at most about 100 ms of real time.
const BATCH_MS: u32 = 50;

/// The watchdog fires if nothing feeds it for this long.
const WATCHDOG_US: u32 = 3_000_000;

/// Real time is 1.00. A node also has frames and console bytes to move,
/// so a go wants some room above that.
const GO_RATIO: f64 = 1.25;

#[unsafe(link_section = ".bi_entries")]
#[used]
pub static PICOTOOL_ENTRIES: [embassy_rp::binary_info::EntryAddr; 3] = [
    embassy_rp::binary_info::rp_program_name!(c"tnc2-pico speedtest"),
    embassy_rp::binary_info::rp_program_description!(c"TheNet X1J emulator speed test, results on USB serial port 2"),
    embassy_rp::binary_info::rp_cargo_version!(),
];

/// A finished clock.
#[derive(Clone, Copy)]
struct Result {
    /// Clock counted by the chip's frequency counter, kHz.
    counted_khz: u32,
    /// Emulated Z80 clock, kHz.
    z80_khz: u32,
    /// `None` when restored after a restart, which keeps only whether it
    /// matched.
    ram_hash: Option<u64>,
    hash_ok: bool,
}

impl Result {
    fn ratio(&self) -> f64 {
        self.z80_khz as f64 / (BoardConfig::default().clock_hz as f64 / 1000.0)
    }
}

#[derive(Clone, Copy)]
enum State {
    Waiting,
    Switching,
    Running {
        seconds: u32,
    },
    Done(Result),
    /// The chip hung (the watchdog fired) or panicked at this clock.
    Failed {
        panic: bool,
    },
    /// Not tried, because a slower clock failed.
    Skipped,
}

struct Status {
    steps: [State; STEPS.len()],
    finished: bool,
    /// Restarted by the watchdog after a panic outside any clock.
    late_panic: bool,
}

static STATUS: Mutex<CriticalSectionRawMutex, RefCell<Status>> =
    Mutex::new(RefCell::new(Status { steps: [State::Waiting; STEPS.len()], finished: false, late_panic: false }));

/// Wakes the reporter early when something worth saying has happened.
static KICK: Signal<CriticalSectionRawMutex, ()> = Signal::new();

fn update(f: impl FnOnce(&mut Status)) {
    STATUS.lock(|s| f(&mut s.borrow_mut()));
}

/// What survives a restart, in watchdog scratch registers 0 to 3:
///
/// - 0 and 1: two clocks each, 16 bits a clock, 0 for none, otherwise bit
///   15 set if the RAM hash matched and bits 0 to 14 the Z80 speed in kHz;
/// - 2: a byte a clock, the counted clock in MHz less 99 (0 for none);
/// - 3: `MAGIC` in the top half, the clock being tried in bits 0 to 3
///   (`NONE` for none), bit 4 set by the panic handler.
mod saved {
    use super::Result;
    use tnc2_pico::watchdog::{scratch, set_scratch};

    const MAGIC: u32 = 0x7C2B_0000;
    const NONE: u32 = 0xF;
    const PANIC: u32 = 1 << 4;

    pub struct Saved {
        pub results: [Option<Result>; 4],
        pub trying: Option<usize>,
        pub panic: bool,
    }

    pub fn clear() {
        for i in 0..3 {
            set_scratch(i, 0);
        }
        set_scratch(3, MAGIC | NONE);
    }

    pub fn trying(i: Option<usize>) {
        set_scratch(3, MAGIC | i.map_or(NONE, |i| i as u32));
    }

    /// From the panic handler: leaves things alone unless the test set
    /// them up.
    pub fn panicked() {
        let s = scratch(3);
        if s & 0xFFFF_0000 == MAGIC {
            set_scratch(3, s | PANIC);
        }
    }

    pub fn record(i: usize, r: &Result) {
        let v = (r.z80_khz.clamp(1, 0x7FFF) | if r.hash_ok { 0x8000 } else { 0 }) << (16 * (i % 2));
        set_scratch(i / 2, scratch(i / 2) | v);
        let mhz = (r.counted_khz / 1000).saturating_sub(99).clamp(1, 255);
        set_scratch(2, scratch(2) | mhz << (8 * i));
    }

    pub fn load() -> Option<Saved> {
        let s = scratch(3);
        if s & 0xFFFF_0000 != MAGIC {
            return None;
        }
        let mut results = [None; 4];
        for (i, r) in results.iter_mut().enumerate() {
            let v = (scratch(i / 2) >> (16 * (i % 2))) & 0xFFFF;
            let mhz = (scratch(2) >> (8 * i)) & 0xFF;
            if v != 0 {
                *r = Some(Result {
                    counted_khz: (mhz + 99) * 1000,
                    z80_khz: v & 0x7FFF,
                    ram_hash: None,
                    hash_ok: v & 0x8000 != 0,
                });
            }
        }
        let trying = match s & NONE {
            NONE => None,
            i => Some(i as usize),
        };
        Some(Saved { results, trying, panic: s & PANIC != 0 })
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    saved::panicked();
    watchdog::reset_now()
}

#[embassy_executor::main(executor = "embassy_rp::executor::Executor", entry = "cortex_m_rt::entry")]
async fn main(spawner: Spawner) {
    ram_code::load();
    heap::init();
    let p = embassy_rp::init(clock::config());
    let (console, status) = usb::start(spawner, p.USB);
    spawner.spawn(console_note(console).expect("console task"));
    spawner.spawn(reporter(status).expect("reporter task"));

    // Only a restart this test caused counts: a timeout while trying a
    // clock, or the panic handler. picotool, for one, also restarts the
    // chip through the watchdog.
    let restarted = saved::load().filter(|s| match watchdog::reason() {
        Some(watchdog::Reason::TimedOut) => s.trying.is_some(),
        Some(watchdog::Reason::Forced) => s.panic,
        None => false,
    });
    if let Some(s) = restarted {
        // Back from a hang or a panic: show what there was, try nothing
        // more. The chip is at the stock clock again.
        update(|st| {
            for (i, r) in s.results.iter().enumerate() {
                st.steps[i] = match (r, s.trying) {
                    (Some(r), _) => State::Done(*r),
                    (None, Some(t)) if t == i => State::Failed { panic: s.panic },
                    _ => State::Skipped,
                };
            }
            st.late_panic = s.panic && s.trying.is_none();
            st.finished = true;
        });
        saved::trying(None);
        return;
    }
    saved::clear();

    // Let the host enumerate the ports before the busy stretches.
    Timer::after_secs(3).await;

    watchdog::start(WATCHDOG_US);
    for (i, step) in STEPS.iter().enumerate() {
        saved::trying(Some(i));
        if i > 0 {
            // Say what is about to happen first, in case this clock hangs.
            update(|s| s.steps[i] = State::Switching);
            KICK.signal(());
            Timer::after_secs(1).await;
            watchdog::feed(WATCHDOG_US);
            clock::set(*step).await;
        }
        let r = run_step(i).await;
        saved::record(i, &r);
        KICK.signal(());
    }
    saved::trying(None);
    clock::set(clock::STOCK).await;
    watchdog::stop();
    update(|s| s.finished = true);
    KICK.signal(());
}

/// Boot a fresh board and run it for `SECONDS` emulated seconds in
/// batches, timing only the batches. The sequence of `Board::run` calls is
/// the same as `bench::run`'s, so the RAM hash at the end is comparable.
async fn run_step(i: usize) -> Result {
    let counted_khz = clock::measure_sys_khz();
    let mut board = Board::from_rom(ROM1, ROM2, &NodeConfig::default(), &BoardConfig::default()).expect("ROM images");
    let start_cycles = board.cycles();
    let mut elapsed_us = 0;
    for b in 1..=SECONDS * 1000 / BATCH_MS {
        elapsed_us += run_batch(&mut board);
        watchdog::feed(WATCHDOG_US);
        if b % (1000 / BATCH_MS) == 0 {
            update(|st| st.steps[i] = State::Running { seconds: b * BATCH_MS / 1000 });
        }
        Timer::after_millis(2).await;
    }
    let ram_hash = tnc2_core::rom::fingerprint(&board.ram()[..], &[]);
    let report =
        bench::Report { cycles: board.cycles() - start_cycles, elapsed_us, clock_hz: board.clock_hz(), ram_hash };
    let r = Result {
        counted_khz,
        z80_khz: (report.emulated_mhz() * 1000.0) as u32,
        ram_hash: Some(ram_hash),
        hash_ok: ram_hash == HOST_HASH,
    };
    update(|st| st.steps[i] = State::Done(r));
    r
}

/// `BATCH_MS` 1 ms steps, flat out; returns the microseconds taken. It
/// runs from SRAM because the emulator's main loop is inlined into it.
#[unsafe(link_section = ".ram_code")]
#[inline(never)]
fn run_batch(board: &mut Board) -> u64 {
    let step = board.clock_hz() / 1000;
    let start = Instant::now().as_micros();
    for _ in 0..BATCH_MS {
        board.run(step);
    }
    Instant::now().as_micros() - start
}

/// The status port: the whole report every three seconds, so a terminal
/// opened at any time soon has everything.
#[embassy_executor::task]
async fn reporter(mut port: usb::Port) {
    loop {
        port.wait_connection().await;
        let _ = usb::send(&mut port, report().as_bytes()).await;
        select(Timer::after_secs(3), KICK.wait()).await;
    }
}

/// The console port will carry TheNet's console; in this build it only
/// says where to look.
#[embassy_executor::task]
async fn console_note(mut port: usb::Port) {
    let mut open = false;
    loop {
        port.wait_connection().await;
        if port.dtr() && !open {
            let note = b"tnc2-pico speed test: TheNet is not running in this build. The results are on the other serial port.\r\n";
            let _ = usb::send(&mut port, note).await;
        }
        open = port.dtr();
        Timer::after_millis(500).await;
    }
}

fn report() -> String {
    let (steps, finished, late_panic) = STATUS.lock(|s| {
        let s = s.borrow();
        (s.steps, s.finished, s.late_panic)
    });
    let mut out = String::new();
    let o = &mut out;
    let _ = write!(
        o,
        "\r\ntnc2-pico {} speed test: TheNet X1J on an emulated 4.9152 MHz Z80\r\n\
         Each clock runs the node from power on for {SECONDS} emulated seconds.\r\n\
         {} KB of emulator code runs from SRAM.\r\n\r\n\
         clock    counted  core    Z80 MHz  ratio  cycles  RAM hash\r\n",
        env!("CARGO_PKG_VERSION"),
        ram_code::size().div_ceil(1024),
    );
    for (step, state) in STEPS.iter().zip(steps.iter()) {
        let _ = write!(o, "{:3} MHz  ", step.mhz);
        let v = volts(step.mv);
        let _ = match *state {
            State::Waiting => write!(o, "-        {v}  waiting"),
            State::Switching => {
                write!(o, "-        {v}  switching now; if nothing follows, this board stops at this clock")
            }
            State::Running { seconds } => write!(o, "-        {v}  running, {seconds} of {SECONDS} s"),
            State::Done(r) => {
                let counted = r.counted_khz as f64 / 1000.0;
                let z80 = r.z80_khz as f64 / 1000.0;
                let _ = write!(o, "{counted:6.2}   {v}  {z80:6.3}   {:4.2}x  {:5.1}   ", r.ratio(), counted / z80);
                match r.ram_hash {
                    Some(h) => write!(o, "{h:016x} "),
                    None => write!(o, "(before restart) "),
                }
                .and_then(|_| write!(o, "{}", if r.hash_ok { "matches" } else { "WRONG" }))
            }
            State::Failed { panic: false } => write!(o, "-        {v}  HUNG: the watchdog restarted the chip"),
            State::Failed { panic: true } => write!(o, "-        {v}  CRASHED: the firmware panicked and restarted"),
            State::Skipped => write!(o, "-        {v}  not tried"),
        };
        let _ = write!(o, "\r\n");
    }
    let _ = write!(o, "\r\n{}\r\n", verdict(&steps, finished));
    if late_panic {
        let _ = write!(o, "The firmware panicked after the test and restarted.\r\n");
    }
    if finished {
        let _ = write!(o, "Finished, at {} MHz.\r\n", clock::current().mhz);
    }
    out
}

fn volts(mv: u32) -> impl core::fmt::Display {
    struct V(u32);
    impl core::fmt::Display for V {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(f, "{}.{:02} V", self.0 / 1000, self.0 % 1000 / 10)
        }
    }
    V(mv)
}

/// The lowest clock that is fast enough, or the best there was, from the
/// clocks finished so far.
fn verdict(steps: &[State], finished: bool) -> String {
    let mut out = String::new();
    let so_far = if finished { "" } else { " so far" };
    let mut best: Option<(u32, f64)> = None;
    for (step, state) in STEPS.iter().zip(steps.iter()) {
        match *state {
            State::Done(r) if !r.hash_ok => {
                let _ = write!(
                    out,
                    "At {} MHz the RAM hash is wrong: the chip made mistakes, so that clock is no good.\r\n",
                    step.mhz
                );
            }
            State::Done(r) => {
                if best.is_none_or(|(_, b)| r.ratio() > b) {
                    best = Some((step.mhz, r.ratio()));
                }
                if r.ratio() >= GO_RATIO {
                    let _ = write!(
                        out,
                        "GO: fast enough, with room to spare, at {} MHz ({:.2}x real time).",
                        step.mhz,
                        r.ratio()
                    );
                    return out;
                }
            }
            State::Failed { panic } => {
                let what = if panic { "crashed" } else { "hung" };
                let _ = write!(
                    out,
                    "The board {what} at {} MHz and restarted itself; it did not try that clock or faster again.\r\n",
                    step.mhz
                );
            }
            _ => {}
        }
    }
    let _ = match best {
        Some((mhz, ratio)) if ratio >= 1.0 => write!(
            out,
            "MARGINAL{so_far}: real time at {mhz} MHz ({ratio:.2}x), but little room for radio and console traffic."
        ),
        Some((mhz, ratio)) => write!(
            out,
            "NO-GO{so_far}: the best was {ratio:.2}x real time at {mhz} MHz; the emulator needs to get faster."
        ),
        None if finished => write!(out, "NO-GO: no clock gave a correct result."),
        None => write!(out, "Still measuring. 1.00x is exactly real time; {GO_RATIO:.2}x or more is a go."),
    };
    out
}
