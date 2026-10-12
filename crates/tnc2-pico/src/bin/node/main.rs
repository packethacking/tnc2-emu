//! The node: TheNet X1J on a Pico 2 W, with a NinoTNC as its radio.
//!
//! Core 0 runs the Z80 board and nothing else, paced to real time
//! (core0.rs). Core 1 runs Embassy and does all the I/O: the two USB
//! serial ports (TheNet's console, console.rs, and the Pico's own setup
//! and status port, setup.rs), the NinoTNC (radio.rs), and the flash
//! store (manager.rs). Between them are lock free queues and two hand
//! over points from tnc2-node: frames and console bytes go through pipes,
//! the board itself through a `BoardSlot` (core 1 builds it, core 0 runs
//! it), and RAM snapshots for saving through a `Snapshot`.
//!
//! docs/pico.md is the user's side of all this.

#![no_std]
#![no_main]

extern crate alloc;

mod console;
mod core0;
mod manager;
mod radio;
mod setup;

use core::cell::RefCell;
use embassy_executor::Spawner;
use embassy_rp::executor::Executor;
use embassy_rp::interrupt::InterruptExt;
use embassy_rp::multicore::{Stack, spawn_core1};
use embassy_rp::peripherals::{DMA_CH0, DMA_CH1, PIN_20, PIN_21, PIO0, UART1, USB};
use embassy_rp::{Peri, interrupt};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::{Instant, Timer};
use static_cell::StaticCell;
use tnc2_core::Board;
use tnc2_node::handoff::{BoardSlot, Saver, Snapshot};
use tnc2_node::pace::{Counters, Rates, Sample};
use tnc2_node::pipe::{Consumer, Pipe, Producer};
use tnc2_pico::clock::{self, Step};
use tnc2_pico::{heap, usb, watchdog};

/// The system clock, set once at start up. 250 MHz at 1.25 V is
/// provisional: the speed test (docs/pico.md) has not run on a board yet,
/// and its result (the lowest clock that is a GO, and the voltage it ran
/// at) decides the final value. The PIO transmitter's bit timing is worked
/// out from this, so change it here and nowhere else.
const SYS_CLOCK: Step = clock::STEPS[2];
const _: () = assert!(SYS_CLOCK.mhz == 250 && SYS_CLOCK.mv == 1250);

#[unsafe(link_section = ".bi_entries")]
#[used]
pub static PICOTOOL_ENTRIES: [rp_binary_info::EntryAddr; 3] = [
    rp_binary_info::rp_program_name!(c"tnc2-pico node"),
    rp_binary_info::rp_program_description!(
        c"TheNet X1J node: NinoTNC on UART1 (GP20 TX, GP21 RX), console and setup on USB serial"
    ),
    rp_binary_info::rp_cargo_version!(),
];

/// Radio frames each way, and console bytes each way.
pub const RADIO_PIPE: usize = 8192;
pub const CONSOLE_PIPE: usize = 4096;

static RADIO_IN: Pipe<RADIO_PIPE> = Pipe::new();
static RADIO_OUT: Pipe<RADIO_PIPE> = Pipe::new();
static CONSOLE_IN: Pipe<CONSOLE_PIPE> = Pipe::new();
static CONSOLE_OUT: Pipe<CONSOLE_PIPE> = Pipe::new();
static SNAPSHOT: Snapshot = Snapshot::new();

/// The board, on its way from core 1 to core 0.
pub static BOARD: BoardSlot<Board> = BoardSlot::new();
/// Core 0's running totals.
pub static COUNTERS: Counters = Counters::new();
/// Speed and load over the last `METER_SECS`, for STATUS.
pub static RATES: Mutex<CriticalSectionRawMutex, RefCell<Option<Rates>>> = Mutex::new(RefCell::new(None));
const METER_SECS: u64 = 10;

/// The watchdog restarts the chip if core 1 stops feeding it for this
/// long, and core 1 only feeds it while core 0 is making progress. Long
/// enough for the slowest flash sector erase (400 ms) several times over.
const WATCHDOG_US: u32 = 2_000_000;

/// Why the chip last restarted, kept in watchdog scratch register 0
/// (which survives the restart) as `MAGIC` and a code.
mod reset {
    use tnc2_pico::watchdog::{self, Reason, scratch, set_scratch};

    const MAGIC: u32 = 0x7E0D_0000;
    pub const PANIC_CORE0: u32 = 1;
    pub const PANIC_CORE1: u32 = 2;
    pub const CORE0_STALLED: u32 = 3;

    pub fn mark(code: u32) {
        set_scratch(0, MAGIC | code);
    }

    pub fn clear() {
        set_scratch(0, 0);
    }

    /// What happened last time, in words, and start afresh.
    pub fn last() -> &'static str {
        let s = scratch(0);
        let code = (s & 0xFFFF_0000 == MAGIC).then_some(s & 0xFFFF);
        clear();
        match (watchdog::reason(), code) {
            (Some(Reason::TimedOut), Some(CORE0_STALLED)) => "watchdog: the emulator (core 0) stopped",
            (Some(Reason::TimedOut), _) => "watchdog: the firmware (core 1) stopped",
            (Some(Reason::Forced), Some(PANIC_CORE0)) => "the firmware panicked on core 0",
            (Some(Reason::Forced), Some(PANIC_CORE1)) => "the firmware panicked on core 1",
            (Some(Reason::Forced), _) => "asked for (picotool, or a debugger)",
            (None, _) => "power on, or the reset pin",
        }
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    let core1 = embassy_rp::pac::SIO.cpuid().read() != 0;
    reset::mark(if core1 { reset::PANIC_CORE1 } else { reset::PANIC_CORE0 });
    watchdog::reset_now()
}

/// What core 1 is given to work with.
struct Core1 {
    usb: Peri<'static, USB>,
    uart: Peri<'static, UART1>,
    rx_pin: Peri<'static, PIN_21>,
    tx_pin: Peri<'static, PIN_20>,
    pio: Peri<'static, PIO0>,
    dma_rx: Peri<'static, DMA_CH0>,
    dma_tx: Peri<'static, DMA_CH1>,
    radio_in: Producer<RADIO_PIPE>,
    radio_out: Consumer<RADIO_PIPE>,
    console_in: Producer<CONSOLE_PIPE>,
    console_out: Consumer<CONSOLE_PIPE>,
    saver: Saver,
    reset: &'static str,
}

#[unsafe(no_mangle)]
fn tnc2_main() -> ! {
    // In case whatever ran before left the watchdog running.
    watchdog::stop();
    let reset = reset::last();
    heap::init();
    let p = embassy_rp::init(clock::config());

    let (radio_in, core0_radio_in) = RADIO_IN.split();
    let (core0_radio_out, radio_out) = RADIO_OUT.split();
    let (console_in, core0_console_in) = CONSOLE_IN.split();
    let (core0_console_out, console_out) = CONSOLE_OUT.split();
    let (saver, serve) = SNAPSHOT.split();
    let core1 = Core1 {
        usb: p.USB,
        uart: p.UART1,
        rx_pin: p.PIN_21,
        tx_pin: p.PIN_20,
        pio: p.PIO0,
        dma_rx: p.DMA_CH0,
        dma_tx: p.DMA_CH1,
        radio_in,
        radio_out,
        console_in,
        console_out,
        saver,
        reset,
    };

    static CORE1_STACK: StaticCell<Stack<32768>> = StaticCell::new();
    spawn_core1(p.CORE1, CORE1_STACK.init(Stack::new()), move || {
        // The interrupt controller is per core, and embassy_rp::init
        // enabled the time driver's interrupt on core 0's only.
        // SAFETY: the time driver's handler is bound and ready.
        unsafe { interrupt::TIMER0_IRQ_0.enable() };
        static EXECUTOR: StaticCell<Executor> = StaticCell::new();
        EXECUTOR.init(Executor::new()).run(|spawner| spawner.spawn(core1_main(spawner, core1).expect("core 1")))
    });

    core0::run(tnc2_node::engine::Io {
        radio_in: core0_radio_in,
        radio_out: core0_radio_out,
        console_in: core0_console_in,
        console_out: core0_console_out,
        snapshot: serve,
    })
}

#[embassy_executor::task]
async fn core1_main(spawner: Spawner, io: Core1) {
    // The one clock change, before anything that depends on the system
    // clock is set up. embassy-rp's own note of the clock still says
    // 150 MHz afterwards; radio.rs works the PIO divider out from
    // SYS_CLOCK instead, the UART runs from the 48 MHz USB PLL, and
    // nothing else here reads it.
    clock::set(SYS_CLOCK).await;
    spawner.spawn(feed_watchdog().expect("watchdog task"));
    spawner.spawn(meter().expect("meter task"));
    spawner.spawn(manager::run(io.saver).expect("manager task"));

    let (console, setup) = usb::start(spawner, io.usb);
    let (to_host, from_host) = console.split();
    spawner.spawn(console::to_terminal(to_host, io.console_out).expect("console task"));
    spawner.spawn(console::from_terminal(from_host, io.console_in).expect("console task"));
    spawner.spawn(setup::run(setup, io.reset).expect("setup task"));

    let rx = radio::Receiver::new(io.uart, io.rx_pin, io.dma_rx);
    spawner.spawn(radio::receive(rx, io.radio_in).expect("radio task"));
    let tx = radio::Transmitter::new(io.pio, io.tx_pin, io.dma_tx, SYS_CLOCK.hz());
    spawner.spawn(radio::transmit(tx, io.radio_out).expect("radio task"));
}

/// Feed the watchdog while core 0 is making progress. If core 0 stops,
/// note that for STATUS after the restart, and let the watchdog fire.
#[embassy_executor::task]
async fn feed_watchdog() {
    watchdog::start(WATCHDOG_US);
    let mut last = COUNTERS.slices.load(core::sync::atomic::Ordering::Relaxed);
    let mut stuck_since: Option<Instant> = None;
    loop {
        Timer::after_millis(250).await;
        let now = COUNTERS.slices.load(core::sync::atomic::Ordering::Relaxed);
        if now != last {
            watchdog::feed(WATCHDOG_US);
            if stuck_since.take().is_some() {
                reset::clear();
            }
        } else if stuck_since.get_or_insert_with(Instant::now).elapsed().as_millis() >= 500 {
            reset::mark(reset::CORE0_STALLED);
        }
        last = now;
    }
}

/// Work out speed and load every `METER_SECS`.
#[embassy_executor::task]
async fn meter() {
    let mut prev = Sample::take(&COUNTERS, Instant::now().as_micros());
    loop {
        Timer::after_secs(METER_SECS).await;
        let s = Sample::take(&COUNTERS, Instant::now().as_micros());
        let r = Rates::between(prev, s);
        RATES.lock(|c| *c.borrow_mut() = r);
        prev = s;
    }
}
