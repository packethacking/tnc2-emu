//! TheNet from power on for 60 emulated seconds with no I/O, the same run
//! as `cargo run --release -p tnc2-core --example bench`, on QEMU's
//! Cortex-M33 board with `-icount shift=0`. Virtual time then advances by
//! a fixed amount per instruction, so the AN505 FPGA counter, which
//! counts in virtual time, counts instructions. A loop of known length
//! calibrates it.

#![no_std]
#![no_main]

extern crate alloc;

use core::mem::MaybeUninit;
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use embedded_alloc::LlffHeap;
use panic_semihosting as _;
use tnc2_core::{Board, BoardConfig, NodeConfig, bench};

#[global_allocator]
static HEAP: LlffHeap = LlffHeap::empty();

static ROM1: &[u8] = include_bytes!("../../../rom/THENET1.X1J");
static ROM2: &[u8] = include_bytes!("../../../rom/THENET2.X1J");

const SECONDS: u32 = 60;
const CALIBRATE: u32 = 10_000_000;

/// The AN505 FPGAIO free running counter (secure alias).
fn counter() -> u64 {
    // SAFETY: a read only register of the board.
    unsafe { core::ptr::read_volatile(0x5030_2018 as *const u32) as u64 }
}

/// `n` times round a two instruction loop.
#[inline(never)]
fn spin(n: u32) {
    // SAFETY: touches nothing but its own register.
    unsafe { core::arch::asm!("1: subs {0}, #1", "bne 1b", inout(reg) n => _, options(nomem, nostack)) }
}

#[entry]
fn main() -> ! {
    static mut MEM: [MaybeUninit<u8>; 256 * 1024] = [MaybeUninit::uninit(); 256 * 1024];
    // SAFETY: `entry` makes MEM a unique reference; it is only used here.
    unsafe { HEAP.init(MEM.as_mut_ptr() as usize, MEM.len()) }

    let t = counter();
    spin(CALIBRATE);
    let calib = counter() - t;
    let per_tick = (2 * CALIBRATE) as f64 / calib as f64;

    let mut board = Board::from_rom(ROM1, ROM2, &NodeConfig::default(), &BoardConfig::default()).unwrap();
    // The counter is 32 bits: 60 emulated seconds are far from wrapping it.
    let r = bench::run(&mut board, SECONDS, counter);
    let insns = r.elapsed_us as f64 * per_tick;
    let per_z80 = insns / r.cycles as f64;
    let mips = per_z80 * r.clock_hz as f64 / 1e6;
    hprintln!("calibration: {} instructions in {} counter ticks", 2 * CALIBRATE, calib);
    hprintln!("{} Z80 cycles, {:.0} million Arm instructions, RAM hash {:016x}", r.cycles, insns / 1e6, r.ram_hash);
    hprintln!("{:.2} Arm instructions per Z80 clock, {:.1} million a second for real time", per_z80, mips);
    hprintln!("at 1.4 to 1.9 cycles an instruction: {:.0} to {:.0} MHz for real time", mips * 1.4, mips * 1.9);
    debug::exit(debug::EXIT_SUCCESS);
    loop {
        cortex_m::asm::wfi();
    }
}
