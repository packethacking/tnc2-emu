//! Speed test: can a Pico 2 W emulate the TNC-2 in real time?
//!
//! Runs TheNet from power on for 60 emulated seconds with no I/O, flat out,
//! at 150 MHz and then at 200, 250 and 300 MHz, and reports each on the
//! second USB serial port every few seconds. The RAM hash at the end must
//! match the host's, which shows the emulation was right as well as fast.
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
use panic_halt as _;
use tnc2_core::{Board, BoardConfig, NodeConfig, bench};
use tnc2_pico::clock::{self, STEPS, Step};
use tnc2_pico::{heap, ram_code, usb};

static ROM1: &[u8] = include_bytes!("../../../../rom/THENET1.X1J");
static ROM2: &[u8] = include_bytes!("../../../../rom/THENET2.X1J");

/// Emulated seconds per clock, and the RAM hash the host gets after them
/// (`cargo run --release -p tnc2-core --example bench -- 60`).
const SECONDS: u32 = 60;
const HOST_HASH: u64 = 0xae7ab48fd2b132e4;

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

#[derive(Clone, Copy)]
enum State {
    Waiting,
    Switching,
    Running { seconds: u32 },
    Done { khz: u32, mv: u32, cycles: u64, elapsed_us: u64, ram_hash: u64 },
}

struct Status {
    steps: [State; STEPS.len()],
    finished: bool,
}

static STATUS: Mutex<CriticalSectionRawMutex, RefCell<Status>> =
    Mutex::new(RefCell::new(Status { steps: [State::Waiting; STEPS.len()], finished: false }));

/// Wakes the reporter early when something worth saying has happened.
static KICK: Signal<CriticalSectionRawMutex, ()> = Signal::new();

fn update(f: impl FnOnce(&mut Status)) {
    STATUS.lock(|s| f(&mut s.borrow_mut()));
}

#[embassy_executor::main(executor = "embassy_rp::executor::Executor", entry = "cortex_m_rt::entry")]
async fn main(spawner: Spawner) {
    ram_code::load();
    heap::init();
    let p = embassy_rp::init(clock::config());
    let (console, status) = usb::start(spawner, p.USB);
    spawner.spawn(console_note(console).expect("console task"));
    spawner.spawn(reporter(status).expect("reporter task"));

    // Let the host enumerate the ports before the long busy stretches.
    Timer::after_secs(3).await;

    for (i, step) in STEPS.iter().enumerate() {
        if i > 0 {
            // Say what is about to happen first, in case this clock hangs.
            update(|s| s.steps[i] = State::Switching);
            KICK.signal(());
            Timer::after_secs(1).await;
            clock::set(*step).await;
        }
        run_step(i).await;
        KICK.signal(());
    }
    clock::set(clock::STOCK).await;
    update(|s| s.finished = true);
    KICK.signal(());
}

/// Boot a fresh board and run it for `SECONDS` emulated seconds, a second
/// at a time. Between seconds the USB stack and the reporter get a moment,
/// outside the timed part.
async fn run_step(i: usize) {
    let khz = clock::measure_sys_khz();
    let mv = clock::core_mv();
    let mut board = Board::from_rom(ROM1, ROM2, &NodeConfig::default(), &BoardConfig::default()).expect("ROM images");
    let (mut cycles, mut elapsed_us, mut ram_hash) = (0, 0, 0);
    for s in 1..=SECONDS {
        let r = bench_second(&mut board);
        cycles += r.cycles;
        elapsed_us += r.elapsed_us;
        ram_hash = r.ram_hash;
        update(|st| st.steps[i] = State::Running { seconds: s });
        Timer::after_millis(20).await;
    }
    update(|st| st.steps[i] = State::Done { khz, mv, cycles, elapsed_us, ram_hash });
}

/// One emulated second, flat out. It runs from SRAM because the emulator's
/// main loop is inlined into it.
#[unsafe(link_section = ".ram_code")]
#[inline(never)]
fn bench_second(board: &mut Board) -> bench::Report {
    bench::run(board, 1, || Instant::now().as_micros())
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
    let (steps, finished) = STATUS.lock(|s| {
        let s = s.borrow();
        (s.steps, s.finished)
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
        match *state {
            State::Waiting => {
                let _ = write!(o, "-        {}  waiting", volts(step.mv));
            }
            State::Switching => {
                let _ = write!(
                    o,
                    "-        {}  switching now; if nothing follows, this board stops at this clock",
                    volts(step.mv)
                );
            }
            State::Running { seconds } => {
                let _ = write!(o, "-        {}  running, {seconds} of {SECONDS} s", volts(step.mv));
            }
            State::Done { khz, mv, .. } => {
                let r = as_report(state).unwrap();
                let _ = write!(
                    o,
                    "{:6.2}   {}  {:6.3}   {:4.2}x  {:5.1}   {:016x} {}",
                    khz as f64 / 1000.0,
                    volts(mv),
                    r.emulated_mhz(),
                    r.ratio(),
                    khz as f64 / 1000.0 / r.emulated_mhz(),
                    r.ram_hash,
                    if r.ram_hash == HOST_HASH { "matches" } else { "WRONG" },
                );
            }
        }
        let _ = write!(o, "\r\n");
    }
    let _ = write!(o, "\r\n{}\r\n", verdict(&steps, finished));
    if finished {
        let _ = write!(o, "Finished, back at {} MHz.\r\n", clock::current().mhz);
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

fn as_report(state: &State) -> Option<bench::Report> {
    match *state {
        State::Done { cycles, elapsed_us, ram_hash, .. } => {
            Some(bench::Report { cycles, elapsed_us, clock_hz: BoardConfig::default().clock_hz, ram_hash })
        }
        _ => None,
    }
}

/// The lowest clock that is fast enough, or the best there was.
fn verdict(steps: &[State], finished: bool) -> String {
    let mut out = String::new();
    let done = STEPS.iter().zip(steps.iter()).filter_map(|(st, s)| Some((st, as_report(s)?)));
    let mut best: Option<(&Step, f64)> = None;
    for (step, r) in done {
        if r.ram_hash != HOST_HASH {
            let _ = write!(
                out,
                "At {} MHz the RAM hash is wrong: the chip made mistakes, so that clock is no good.\r\n",
                step.mhz
            );
            continue;
        }
        if best.is_none_or(|(_, b)| r.ratio() > b) {
            best = Some((step, r.ratio()));
        }
        if r.ratio() >= GO_RATIO {
            let _ =
                write!(out, "GO: fast enough, with room to spare, at {} MHz ({:.2}x real time).", step.mhz, r.ratio());
            return out;
        }
    }
    match best {
        Some((step, ratio)) if ratio >= 1.0 && finished => {
            let _ = write!(
                out,
                "MARGINAL: real time at {} MHz ({ratio:.2}x), but little room for radio and console traffic.",
                step.mhz
            );
        }
        Some((step, ratio)) if finished => {
            let _ = write!(
                out,
                "NO-GO: the best was {ratio:.2}x real time at {} MHz; the emulator needs to get faster.",
                step.mhz
            );
        }
        _ if finished => {
            let _ = write!(out, "NO-GO: no clock gave a correct result.");
        }
        _ => {
            let _ = write!(out, "Still measuring. 1.00x is exactly real time; {GO_RATIO:.2}x or more is a go.");
        }
    }
    out
}
