//! tnc2-core: the TAPR TNC-2 board that runs TheNet X1J, with no I/O.
//!
//! The board is the Z80, the bank switched 27512 EPROM, 32 KB of RAM, the
//! Z80 SIO and the 600 Hz time base on SYNCB. The host owns everything
//! else: it decides how fast emulated time runs, moves radio frames and
//! console bytes in and out, and keeps the RAM between runs.
//!
//! ```ignore
//! let mut board = Board::from_rom(rom1, rom2, &NodeConfig::default(), &BoardConfig::default())?;
//! loop {
//!     board.run(board.clock_hz() / 1000); // about 1 ms
//!     while let Some(f) = board.pop_radio_frame() { /* to the radio */ }
//!     /* board.push_radio_frame(..), board.push_console(..), board.read_console(..) */
//! }
//! ```

#![no_std]

extern crate alloc;

pub mod ax25;
pub mod bench;
pub mod kiss;
pub mod rom;
mod sio;

use alloc::vec::Vec;
use core::fmt;
use rom::RomHalf;
pub use rom::{ConfigError, NodeConfig, Poke, RomError};
use rustzx_z80::{Z80, Z80Bus};

/// Size of each EPROM half, and of the window it is seen through.
pub const ROM_HALF: usize = 0x8000;
/// Battery backed RAM at 0x8000 to 0xFFFF.
pub const RAM_SIZE: usize = 0x8000;

/// Highest CPU clock accepted. Keeps every cycle count and character time
/// far from overflowing a u64 (centuries of emulated time at this rate).
pub const MAX_CLOCK_HZ: u64 = 1_000_000_000;

/// Received radio frames waiting for the SIO, at most this many frames
/// and `RADIO_RX_QUEUE_BYTES` bytes. Past either, new frames are dropped.
pub const RADIO_RX_QUEUE: usize = 64;
/// At 1200 bit/s this is nearly two minutes of back to back frames.
pub const RADIO_RX_QUEUE_BYTES: usize = 16384;
/// Longest received radio frame accepted, in bytes. TheNet's own frames
/// are at most about 330 bytes.
pub const RADIO_RX_MAX_FRAME: usize = 512;
/// Console bytes waiting for the SIO. More than this and new bytes are
/// dropped: at 9600 bit/s it is about 17 seconds of typing.
pub const CONSOLE_RX_QUEUE: usize = 16384;

/// Clocks and the state of the RS232 DCD pin.
#[derive(Clone, Copy, Debug)]
pub struct BoardConfig {
    /// CPU clock, 4.9152 MHz on the TNC-2.
    pub clock_hz: u64,
    /// Radio port bit rate.
    pub hdlc_baud: u64,
    /// RS232 bit rate.
    pub async_baud: u64,
    /// RS232 DCD (SIO DCDB): asserted is the dumb terminal console, clear
    /// is TheNet's KISS/crosslink protocol mode.
    pub rs232_dcd: bool,
}

/// A `BoardConfig` the board cannot run with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoardConfigError {
    /// The clock must be from 1200 Hz (for the 600 Hz time base) to
    /// `MAX_CLOCK_HZ`.
    BadClock,
    /// The radio bit rate must be above 0 and no more than 8 times the clock.
    BadHdlcBaud,
    /// The RS232 bit rate must be above 0 and no more than 10 times the clock.
    BadAsyncBaud,
}

impl fmt::Display for BoardConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            BoardConfigError::BadClock => "CPU clock must be from 1200 Hz to 1 GHz",
            BoardConfigError::BadHdlcBaud => "radio bit rate must be above 0 and at most 8 times the CPU clock",
            BoardConfigError::BadAsyncBaud => "RS232 bit rate must be above 0 and at most 10 times the CPU clock",
        })
    }
}

impl BoardConfig {
    /// Character times in clock cycles (radio, RS232), or why the
    /// configuration cannot work.
    fn char_cycles(&self) -> Result<(u64, u64), BoardConfigError> {
        if !(1200..=MAX_CLOCK_HZ).contains(&self.clock_hz) {
            return Err(BoardConfigError::BadClock);
        }
        let per_char = |bits: u64, baud: u64| (self.clock_hz * bits).checked_div(baud).filter(|&c| c > 0);
        let hdlc = per_char(8, self.hdlc_baud).ok_or(BoardConfigError::BadHdlcBaud)?;
        let async_ = per_char(10, self.async_baud).ok_or(BoardConfigError::BadAsyncBaud)?;
        Ok((hdlc, async_))
    }
}

impl Default for BoardConfig {
    fn default() -> Self {
        BoardConfig { clock_hz: 4_915_200, hdlc_baud: 1200, async_baud: 9600, rs232_dcd: true }
    }
}

/// Something worth telling the operator about. The core only records
/// these; the host decides whether and how to print them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// First access to an I/O port other than the SIO.
    FirstPortAccess { out: bool, port: u8, value: u8 },
    /// The SIO dropped a frame being sent because of a send abort command.
    SendAbort { channel: char },
    /// The transmitter was switched off part way through a frame.
    TxDisabledMidFrame { channel: char, bytes: usize },
    /// Transmit underrun with the EOM latch already set: the frame is lost.
    UnderrunWithEom { channel: char, bytes: usize },
    /// The receiver was switched off part way through a frame.
    RxDisabledMidFrame { channel: char, chars: usize },
    /// The radio receive queue is full: frames are being dropped until
    /// there is room again.
    RadioRxFull,
    /// The radio receive queue has room again after `dropped` frames were
    /// lost; `total` is the count since power on.
    RadioRxResumed { dropped: u64, total: u64 },
    /// A received radio frame was longer than `RADIO_RX_MAX_FRAME` and has
    /// been dropped.
    RadioRxTooLong { bytes: usize },
    /// The console receive queue is full: input is being dropped until
    /// there is room again.
    ConsoleRxFull,
    /// The console receive queue has room again after `dropped` bytes were
    /// lost; `total` is the count since power on.
    ConsoleRxResumed { dropped: u64, total: u64 },
}

impl Event {
    /// Chatty SIO diagnostics, only of interest when debugging.
    pub fn is_debug(&self) -> bool {
        matches!(
            self,
            Event::SendAbort { .. }
                | Event::TxDisabledMidFrame { .. }
                | Event::UnderrunWithEom { .. }
                | Event::RxDisabledMidFrame { .. }
        )
    }
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Event::FirstPortAccess { out: true, port, value } => {
                write!(f, "first OUT to port 0x{port:02X} value 0x{value:02X}")
            }
            Event::FirstPortAccess { out: false, port, .. } => write!(f, "first IN from port 0x{port:02X}"),
            Event::SendAbort { channel } => write!(f, "SIO {channel}: send abort, frame dropped"),
            Event::TxDisabledMidFrame { channel, bytes } => {
                write!(f, "SIO {channel}: transmitter disabled mid frame, {bytes} bytes dropped")
            }
            Event::UnderrunWithEom { channel, bytes } => {
                write!(f, "SIO {channel}: underrun with EOM latch set, {bytes} bytes dropped")
            }
            Event::RxDisabledMidFrame { channel, chars } => {
                write!(f, "SIO {channel}: receiver disabled mid frame, {chars} characters lost")
            }
            Event::RadioRxFull => write!(
                f,
                "radio receive queue full ({RADIO_RX_QUEUE} frames or {RADIO_RX_QUEUE_BYTES} bytes), dropping received frames"
            ),
            Event::RadioRxResumed { dropped, total } => write!(
                f,
                "radio receive queue has room again; {dropped} frames dropped, {total} since power on"
            ),
            Event::RadioRxTooLong { bytes } => write!(
                f,
                "received radio frame of {bytes} bytes is longer than {RADIO_RX_MAX_FRAME}, dropped"
            ),
            Event::ConsoleRxFull => {
                write!(f, "console receive queue full ({CONSOLE_RX_QUEUE} bytes), dropping input")
            }
            Event::ConsoleRxResumed { dropped, total } => write!(
                f,
                "console receive queue has room again; {dropped} bytes dropped, {total} since power on"
            ),
        }
    }
}

/// Events waiting for the host, oldest first. Fixed size: if the host
/// never collects them the newest are dropped.
pub(crate) struct Diags {
    q: [Option<Event>; 32],
    head: usize,
    len: usize,
}

impl Diags {
    const fn new() -> Self {
        Diags { q: [None; 32], head: 0, len: 0 }
    }

    pub(crate) fn push(&mut self, e: Event) {
        if self.len < self.q.len() {
            self.q[(self.head + self.len) % self.q.len()] = Some(e);
            self.len += 1;
        }
    }

    fn pop(&mut self) -> Option<Event> {
        if self.len == 0 {
            return None;
        }
        let e = self.q[self.head].take();
        self.head = (self.head + 1) % self.q.len();
        self.len -= 1;
        e
    }
}

/// What the Z80 sees: memory, I/O ports and the interrupt line.
struct Bus {
    rom: [RomHalf; 2],
    ram: alloc::boxed::Box<[u8; RAM_SIZE]>,
    sio: sio::Sio,
    cycles: u64,
    /// Ports already reported, one bit per (direction, port).
    seen_ports: [u32; 16],
    adc_channel: u8,
    diags: Diags,
}

impl Bus {
    fn bank(&self) -> usize {
        // EPROM A15 is wired to the SIO DTRA pin, which is active low:
        // WR5A bit 7 set drives the pin low and selects the lower half.
        if self.sio.a.dtr() { 0 } else { 1 }
    }

    fn note_port(&mut self, is_out: bool, port: u8, v: u8) {
        let bit = ((is_out as usize) << 8) | port as usize;
        let (w, m) = (bit / 32, 1u32 << (bit % 32));
        if self.seen_ports[w] & m == 0 {
            self.seen_ports[w] |= m;
            self.diags.push(Event::FirstPortAccess { out: is_out, port, value: v });
        }
    }
}

impl Z80Bus for Bus {
    fn read_internal(&mut self, addr: u16) -> u8 {
        let a = addr as usize;
        if a < ROM_HALF {
            self.rom[self.bank()][a]
        } else {
            self.ram[a - ROM_HALF]
        }
    }
    fn write_internal(&mut self, addr: u16, data: u8) {
        let a = addr as usize;
        if a >= ROM_HALF {
            self.ram[a - ROM_HALF] = data;
        }
    }
    fn wait_mreq(&mut self, _addr: u16, clk: usize) {
        self.cycles += clk as u64;
    }
    fn wait_no_mreq(&mut self, _addr: u16, clk: usize) {
        self.cycles += clk as u64;
    }
    fn wait_internal(&mut self, clk: usize) {
        self.cycles += clk as u64;
    }
    fn read_io(&mut self, port: u16) -> u8 {
        let p = port as u8;
        self.cycles += 4;
        match p {
            0..=3 => {
                self.sio.run(self.cycles, &mut self.diags);
                self.sio.read(p)
            }
            0x20 => {
                // ADC0844 on the optional deviation meter / S meter board.
                // Not fitted: return zero for every channel.
                self.note_port(false, p, 0);
                let _ = self.adc_channel;
                0
            }
            _ => {
                self.note_port(false, p, 0);
                0xFF
            }
        }
    }
    fn write_io(&mut self, port: u16, data: u8) {
        let p = port as u8;
        self.cycles += 4;
        match p {
            0..=3 => {
                self.sio.run(self.cycles, &mut self.diags);
                self.sio.write(p, data, self.cycles, &mut self.diags);
            }
            0x20 => {
                self.note_port(true, p, data);
                self.adc_channel = data;
            }
            _ => self.note_port(true, p, data),
        }
    }
    fn read_interrupt(&mut self) -> u8 {
        self.sio.vector()
    }
    fn reti(&mut self) {}
    fn halt(&mut self, _halted: bool) {}
    fn int_active(&self) -> bool {
        self.sio.int_active()
    }
    fn nmi_active(&self) -> bool {
        false
    }
    fn pc_callback(&mut self, _addr: u16) {}
}

/// A powered up TNC-2.
pub struct Board {
    cpu: Z80,
    bus: Bus,
    clock_hz: u64,
    fingerprint: u64,
    tick_cycles: u64,
    next_tick: u64,
    tick_due: u64,
    radio_rx_dropped: u64,
    /// Frames dropped in the current run of drops (queue full).
    radio_rx_burst: u64,
    console_rx_dropped: u64,
    console_rx_burst: u64,
}

impl Board {
    /// Power on a board with two padded and already patched EPROM halves.
    /// The RAM starts zeroed (a cold start) until `load_ram`.
    pub fn new(lower: RomHalf, upper: RomHalf, cfg: &BoardConfig) -> Result<Board, BoardConfigError> {
        let clock = cfg.clock_hz;
        let (hdlc_char, async_char) = cfg.char_cycles()?;
        let fingerprint = rom::fingerprint(&lower[..], &upper[..]);
        let ram = alloc::vec![0u8; RAM_SIZE].into_boxed_slice().try_into().expect("32 KB");
        let mut bus = Bus {
            rom: [lower, upper],
            ram,
            sio: sio::Sio::new(hdlc_char, async_char),
            cycles: 0,
            seen_ports: [0; 16],
            adc_channel: 0,
            diags: Diags::new(),
        };
        bus.sio.b.dcd = cfg.rs232_dcd;
        bus.sio.b.cts = true;
        bus.sio.a.dcd = false;
        bus.sio.a.cts = true;
        bus.sio.set_ext();
        // The firmware's time base: SIO SYNCB is fed with a 600 Hz square
        // wave (4.9152 MHz / 8192) and every edge raises an ext/status
        // interrupt.
        let tick_cycles = clock / 1200;
        Ok(Board {
            cpu: Z80::default(),
            bus,
            clock_hz: clock,
            fingerprint,
            tick_cycles,
            next_tick: tick_cycles,
            tick_due: 0,
            radio_rx_dropped: 0,
            radio_rx_burst: 0,
            console_rx_dropped: 0,
            console_rx_burst: 0,
        })
    }

    /// Pad the two ROM images, patch the configuration block in both and
    /// power on.
    pub fn from_rom(rom1: &[u8], rom2: &[u8], node: &NodeConfig, cfg: &BoardConfig) -> Result<Board, Error> {
        let mut lower = rom::pad_half(rom1).map_err(Error::Rom)?;
        let mut upper = rom::pad_half(rom2).map_err(Error::Rom)?;
        rom::patch(&mut lower, node).map_err(Error::Config)?;
        rom::patch(&mut upper, node).map_err(Error::Config)?;
        Board::new(lower, upper, cfg).map_err(Error::Board)
    }

    /// Run the CPU for at least `cycles` clock cycles (it stops at the end
    /// of an instruction).
    pub fn run(&mut self, cycles: u64) {
        let bus = &mut self.bus;
        let end = bus.cycles + cycles;
        while bus.cycles < end {
            while bus.cycles >= self.next_tick {
                self.next_tick += self.tick_cycles;
                if bus.sio.b.wr[1] & 0x01 != 0 {
                    self.tick_due += 1;
                }
            }
            if self.tick_due > 0 && !bus.sio.b.ext_pending() {
                bus.sio.b.sync = !bus.sio.b.sync;
                bus.sio.set_ext();
                self.tick_due -= 1;
            }
            bus.sio.run(bus.cycles, &mut bus.diags);
            if self.cpu.is_halted() && !bus.sio.int_active() {
                bus.cycles += 4;
                continue;
            }
            self.cpu.emulate(bus);
        }
    }

    /// Clock cycles since power on.
    pub fn cycles(&self) -> u64 {
        self.bus.cycles
    }

    pub fn clock_hz(&self) -> u64 {
        self.clock_hz
    }

    /// Fingerprint of the patched ROM, to store with the RAM.
    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    /// Hand a received radio frame (AX.25, no CRC) to the SIO. Frames are
    /// queued and arrive one after another, as if behind carrier detect.
    /// Returns false, and drops the frame, if it is longer than
    /// `RADIO_RX_MAX_FRAME` or the queue is full (`RADIO_RX_QUEUE` frames
    /// or `RADIO_RX_QUEUE_BYTES` bytes).
    #[must_use]
    pub fn push_radio_frame(&mut self, frame: &[u8]) -> bool {
        if frame.len() > RADIO_RX_MAX_FRAME {
            self.radio_rx_dropped += 1;
            self.bus.diags.push(Event::RadioRxTooLong { bytes: frame.len() });
            return false;
        }
        let q = &mut self.bus.sio.a.rx_frames;
        let bytes: usize = q.iter().map(|f| f.len()).sum();
        if q.len() >= RADIO_RX_QUEUE || bytes + frame.len() > RADIO_RX_QUEUE_BYTES {
            self.radio_rx_dropped += 1;
            if self.radio_rx_burst == 0 {
                self.bus.diags.push(Event::RadioRxFull);
            }
            self.radio_rx_burst += 1;
            return false;
        }
        q.push_back(frame.to_vec());
        if self.radio_rx_burst > 0 {
            let e = Event::RadioRxResumed { dropped: self.radio_rx_burst, total: self.radio_rx_dropped };
            self.bus.diags.push(e);
            self.radio_rx_burst = 0;
        }
        true
    }

    /// Received radio frames waiting to be handed to the SIO.
    pub fn radio_rx_pending(&self) -> usize {
        self.bus.sio.a.rx_frames.len()
    }

    /// Radio frames dropped since power on (queue full or too long).
    pub fn radio_rx_dropped(&self) -> u64 {
        self.radio_rx_dropped
    }

    /// Take the next frame the node has transmitted.
    pub fn pop_radio_frame(&mut self) -> Option<Vec<u8>> {
        self.bus.sio.a.tx_frames.pop_front()
    }

    /// Bytes typed on the RS232 port. Returns how many were taken; the
    /// rest are dropped once `CONSOLE_RX_QUEUE` bytes are waiting. A host
    /// that can push back (TCP) should offer no more than
    /// `CONSOLE_RX_QUEUE - console_rx_pending()`.
    #[must_use]
    pub fn push_console(&mut self, bytes: &[u8]) -> usize {
        let q = &mut self.bus.sio.b.rx_bytes;
        let n = bytes.len().min(CONSOLE_RX_QUEUE.saturating_sub(q.len()));
        q.extend(&bytes[..n]);
        if n > 0 && self.console_rx_burst > 0 {
            let e = Event::ConsoleRxResumed { dropped: self.console_rx_burst, total: self.console_rx_dropped };
            self.bus.diags.push(e);
            self.console_rx_burst = 0;
        }
        if n < bytes.len() {
            let lost = (bytes.len() - n) as u64;
            self.console_rx_dropped += lost;
            if self.console_rx_burst == 0 {
                self.bus.diags.push(Event::ConsoleRxFull);
            }
            self.console_rx_burst += lost;
        }
        n
    }

    /// Console bytes waiting to be handed to the SIO.
    pub fn console_rx_pending(&self) -> usize {
        self.bus.sio.b.rx_bytes.len()
    }

    /// Console bytes dropped since power on because the queue was full.
    pub fn console_rx_dropped(&self) -> u64 {
        self.console_rx_dropped
    }

    /// Copy out what the node has sent on the RS232 port, up to the size
    /// of `buf`. Returns the number of bytes copied.
    pub fn read_console(&mut self, buf: &mut [u8]) -> usize {
        let q = &mut self.bus.sio.b.tx_bytes;
        let n = buf.len().min(q.len());
        for (d, s) in buf.iter_mut().zip(q.drain(..n)) {
            *d = s;
        }
        n
    }

    /// Set the RS232 DCD input: asserted is the console, clear is
    /// TheNet's KISS/crosslink protocol mode.
    pub fn set_rs232_dcd(&mut self, on: bool) {
        self.bus.sio.b.dcd = on;
        self.bus.sio.set_ext();
    }

    /// The battery backed RAM, for saving.
    pub fn ram(&self) -> &[u8; RAM_SIZE] {
        &self.bus.ram
    }

    /// Restore saved RAM. Do this before the first `run` for a warm start.
    pub fn load_ram(&mut self, ram: &[u8; RAM_SIZE]) {
        self.bus.ram.copy_from_slice(ram);
    }

    /// Next diagnostic event, oldest first.
    pub fn pop_event(&mut self) -> Option<Event> {
        self.bus.diags.pop()
    }

    /// Which EPROM half is mapped in: 0 lower (THENET1), 1 upper (THENET2).
    pub fn bank(&self) -> usize {
        self.bus.bank()
    }

    pub fn pc(&self) -> u16 {
        self.cpu.regs.get_pc()
    }

    /// 600 Hz edges waiting for the firmware to take the last one.
    pub fn ticks_due(&self) -> u64 {
        self.tick_due
    }

    /// One line describing the radio channel of the SIO, for debugging.
    pub fn radio_debug(&self) -> impl fmt::Display + '_ {
        self.bus.sio.a.debug_state()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Rom(RomError),
    Config(ConfigError),
    Board(BoardConfigError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Rom(e) => e.fmt(f),
            Error::Config(e) => e.fmt(f),
            Error::Board(e) => e.fmt(f),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;

    /// Two tiny programs, one per half. Both halves start the same way and
    /// switch to the lower half by setting WR5A bit 7; the code after the
    /// switch differs, so RAM shows which half ran it.
    fn halves(switch: bool) -> (RomHalf, RomHalf) {
        let wr5 = if switch { 0x80 } else { 0x00 };
        let prelude = [0x3E, 0x05, 0xD3, 0x01, 0x3E, wr5, 0xD3, 0x01]; // LD A,5; OUT (1),A; LD A,n; OUT (1),A
        let tail = |v: u8| [0x3E, v, 0x32, 0x00, 0x80, 0x76]; // LD A,v; LD (8000h),A; HALT
        let mut lo = rom::pad_half(&[]).unwrap();
        let mut hi = rom::pad_half(&[]).unwrap();
        lo[..8].copy_from_slice(&prelude);
        hi[..8].copy_from_slice(&prelude);
        lo[8..14].copy_from_slice(&tail(0x11));
        hi[8..14].copy_from_slice(&tail(0x22));
        (lo, hi)
    }

    #[test]
    fn starts_in_upper_half() {
        let (lo, hi) = halves(false);
        let mut b = Board::new(lo, hi, &BoardConfig::default()).unwrap();
        b.run(1000);
        assert_eq!(b.bank(), 1);
        assert_eq!(b.ram()[0], 0x22);
    }

    #[test]
    fn dtr_switches_to_lower_half_on_next_fetch() {
        let (lo, hi) = halves(true);
        let mut b = Board::new(lo, hi, &BoardConfig::default()).unwrap();
        b.run(1000);
        assert_eq!(b.bank(), 0);
        assert_eq!(b.ram()[0], 0x11);
    }

    #[test]
    fn bad_board_config_is_rejected() {
        let try_cfg = |cfg: BoardConfig| {
            let (lo, hi) = halves(false);
            Board::new(lo, hi, &cfg).err()
        };
        let d = BoardConfig::default();
        assert_eq!(try_cfg(d), None);
        assert_eq!(try_cfg(BoardConfig { clock_hz: 1199, ..d }), Some(BoardConfigError::BadClock));
        assert_eq!(try_cfg(BoardConfig { clock_hz: 0, ..d }), Some(BoardConfigError::BadClock));
        assert_eq!(try_cfg(BoardConfig { hdlc_baud: 0, ..d }), Some(BoardConfigError::BadHdlcBaud));
        assert_eq!(try_cfg(BoardConfig { async_baud: 0, ..d }), Some(BoardConfigError::BadAsyncBaud));
        assert_eq!(try_cfg(BoardConfig { hdlc_baud: d.clock_hz * 8 + 1, ..d }), Some(BoardConfigError::BadHdlcBaud));
        assert_eq!(try_cfg(BoardConfig { clock_hz: u64::MAX, ..d }), Some(BoardConfigError::BadClock));
        assert_eq!(
            try_cfg(BoardConfig { clock_hz: u64::MAX / 10, async_baud: 1, ..d }),
            Some(BoardConfigError::BadClock)
        );
        assert_eq!(try_cfg(BoardConfig { clock_hz: 1_844_674_407_370_955_161, ..d }), Some(BoardConfigError::BadClock));
        assert_eq!(try_cfg(BoardConfig { clock_hz: MAX_CLOCK_HZ + 1, ..d }), Some(BoardConfigError::BadClock));
        // the fastest clock with the slowest bit rates is fine, and runs
        let fast = BoardConfig { clock_hz: MAX_CLOCK_HZ, hdlc_baud: 1, async_baud: 1, ..d };
        assert_eq!(try_cfg(fast), None);
        let (lo, hi) = halves(true);
        let mut b = Board::new(lo, hi, &fast).unwrap();
        b.run(MAX_CLOCK_HZ / 1000);
        assert_eq!(b.ram()[0], 0x11);
        // the slowest clock that works still runs
        let (lo, hi) = halves(true);
        let mut b = Board::new(lo, hi, &BoardConfig { clock_hz: 1200, hdlc_baud: 1200, async_baud: 1200, ..d }).unwrap();
        b.run(1000);
        assert_eq!(b.ram()[0], 0x11);
    }

    #[test]
    fn radio_input_is_capped_by_frames() {
        let (lo, hi) = halves(false);
        let mut b = Board::new(lo, hi, &BoardConfig::default()).unwrap();
        for _ in 0..RADIO_RX_QUEUE {
            assert!(b.push_radio_frame(&[0; 20]));
        }
        assert_eq!(b.radio_rx_pending(), RADIO_RX_QUEUE);
        assert!(!b.push_radio_frame(&[0; 20]));
        assert!(!b.push_radio_frame(&[0; 20]));
        assert!(!b.push_radio_frame(&[0; RADIO_RX_MAX_FRAME + 1]));
        assert_eq!(b.radio_rx_pending(), RADIO_RX_QUEUE);
        assert_eq!(b.radio_rx_dropped(), 3);
        assert_eq!(b.pop_event(), Some(Event::RadioRxFull));
        assert_eq!(b.pop_event(), Some(Event::RadioRxTooLong { bytes: RADIO_RX_MAX_FRAME + 1 }));
        assert_eq!(b.pop_event(), None);
    }

    #[test]
    fn radio_input_is_capped_by_bytes() {
        let (lo, hi) = halves(false);
        let mut b = Board::new(lo, hi, &BoardConfig::default()).unwrap();
        let n = RADIO_RX_QUEUE_BYTES / RADIO_RX_MAX_FRAME;
        for _ in 0..n {
            assert!(b.push_radio_frame(&[0; RADIO_RX_MAX_FRAME]));
        }
        assert!(!b.push_radio_frame(&[0; 1]));
        assert!(!b.push_radio_frame(&[0; 1]));
        assert_eq!(b.radio_rx_pending(), n);
        assert_eq!(b.pop_event(), Some(Event::RadioRxFull));
        // make room: one frame goes to the SIO (receiver not enabled, but
        // take it straight off the queue as the SIO would)
        b.bus.sio.a.rx_frames.pop_front();
        assert!(b.push_radio_frame(&[0; 100]));
        assert_eq!(b.pop_event(), Some(Event::RadioRxResumed { dropped: 2, total: 2 }));
        assert_eq!(b.pop_event(), None);
    }

    #[test]
    fn console_input_is_capped() {
        let (lo, hi) = halves(false);
        let mut b = Board::new(lo, hi, &BoardConfig::default()).unwrap();
        assert_eq!(b.push_console(&[b'x'; CONSOLE_RX_QUEUE - 10]), CONSOLE_RX_QUEUE - 10);
        assert_eq!(b.push_console(&[b'y'; 25]), 10);
        assert_eq!(b.push_console(b"z"), 0);
        assert_eq!(b.console_rx_pending(), CONSOLE_RX_QUEUE);
        assert_eq!(b.console_rx_dropped(), 16);
        assert_eq!(b.pop_event(), Some(Event::ConsoleRxFull));
        assert_eq!(b.pop_event(), None);
        b.bus.sio.b.rx_bytes.drain(..5);
        assert_eq!(b.push_console(b"ok"), 2);
        assert_eq!(b.pop_event(), Some(Event::ConsoleRxResumed { dropped: 16, total: 16 }));
        assert_eq!(b.pop_event(), None);
    }

    #[test]
    fn errors_display_plain_ascii() {
        use alloc::string::ToString;
        let msgs = [
            Error::Rom(RomError::TooLarge).to_string(),
            Error::Config(ConfigError::BadCall).to_string(),
            Error::Config(ConfigError::BadAlias).to_string(),
            Error::Config(ConfigError::PokeOutOfRange).to_string(),
            Error::Board(BoardConfigError::BadClock).to_string(),
            Error::Board(BoardConfigError::BadHdlcBaud).to_string(),
            Error::Board(BoardConfigError::BadAsyncBaud).to_string(),
            rom::RamImageError::NotImage.to_string(),
            rom::RamImageError::OtherConfig.to_string(),
            Event::RadioRxFull.to_string(),
            Event::RadioRxResumed { dropped: 2, total: 3 }.to_string(),
            Event::RadioRxTooLong { bytes: 600 }.to_string(),
            Event::ConsoleRxFull.to_string(),
            Event::ConsoleRxResumed { dropped: 2, total: 3 }.to_string(),
        ];
        for m in msgs {
            assert!(!m.is_empty() && m.is_ascii(), "{m:?}");
        }
        assert_eq!(Error::Rom(RomError::TooLarge).to_string(), "ROM image is larger than 32 KB");
    }

    #[test]
    fn events_queue_is_bounded() {
        let mut d = Diags::new();
        for p in 0..40 {
            d.push(Event::FirstPortAccess { out: true, port: p, value: 0 });
        }
        let mut n = 0;
        while let Some(e) = d.pop() {
            assert_eq!(e, Event::FirstPortAccess { out: true, port: n, value: 0 });
            n += 1;
        }
        assert_eq!(n, 32);
    }

    fn console(b: &mut Board, out: &mut String) {
        let mut buf = [0u8; 256];
        loop {
            let n = b.read_console(&mut buf);
            if n == 0 {
                break;
            }
            out.extend(buf[..n].iter().map(|&c| c as char));
        }
    }

    fn run_until(b: &mut Board, out: &mut String, want: &str, secs: u64) {
        for _ in 0..secs * 100 {
            b.run(b.clock_hz() / 100);
            console(b, out);
            if out.contains(want) {
                return;
            }
        }
        panic!("did not see {want:?}; got {out:?}");
    }

    /// The real firmware, no host: sign on at the console and send the
    /// first NODES broadcast (brought forward by the interval poke).
    #[test]
    fn boots_thenet() {
        let pokes = [Poke { offset: 0x54, bytes: &[0x0A, 0x00] }];
        let node = NodeConfig { call: Some("N0CALL-7"), alias: Some("TEST"), pokes: &pokes };
        let mut b = Board::from_rom(
            include_bytes!("../../../rom/THENET1.X1J"),
            include_bytes!("../../../rom/THENET2.X1J"),
            &node,
            &BoardConfig::default(),
        )
        .unwrap();
        let mut out = String::new();
        b.run(b.clock_hz() / 2);
        assert_eq!(b.push_console(b"\x1b"), 1);
        run_until(&mut b, &mut out, "* ", 5);
        assert_eq!(b.push_console(b"C\r"), 2);
        run_until(&mut b, &mut out, "CONNECTED to N0CALL-7", 5);
        let mut frame = None;
        for _ in 0..2000 {
            b.run(b.clock_hz() / 100);
            if let Some(f) = b.pop_radio_frame() {
                frame = Some(f);
                break;
            }
        }
        let f = frame.expect("NODES broadcast");
        assert_eq!(
            ax25::describe(&f),
            "N0CALL-7>NODES <UI C> pid=CF len=7 NODES from alias 'TEST'"
        );
    }
}
