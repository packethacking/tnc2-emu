//! Z80 SIO/0 emulation at character / frame level.
//!
//! Channel A is the radio (HDLC/SDLC) side, channel B is the RS232 async
//! side. Bits on the wire are not modelled: the transmitter runs a
//! character clock (one character time per byte at the configured bit
//! rate), the receiver is fed whole frames and hands them to the CPU one
//! character at a time with the status the real chip would give.
//!
//! See docs/hardware.md for how TheNet X1J drives this chip.

use std::collections::VecDeque;

/// Interrupt sources, in daisy-chain priority order (highest first).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Src {
    RxSpecial,
    RxChar,
    Tx,
    Ext,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shifter {
    /// Transmitter enabled but idle: sending flags (SDLC) or marking (async).
    Idle,
    /// Shifting out a data character.
    Data,
    /// Sending the CRC after an underrun, n character times left.
    Crc(u8),
}

pub struct Channel {
    pub name: char,
    pub wr: [u8; 8],
    ptr: usize,

    // Receive side
    rx_data: u8,
    rx_avail: bool,
    /// RR1 error / special bits latched for the character in rx_data.
    rr1_latched: u8,
    rx_special_ip: bool,
    rx_char_ip: bool,
    /// Characters still to arrive for the frame being received:
    /// (byte, is_end_of_frame).
    rx_stream: VecDeque<(u8, bool)>,
    next_rx_at: u64,
    /// When the simulated carrier drops after the last received frame.
    rx_dcd_off_at: Option<u64>,
    /// Whole frames waiting to be received (HDLC) or bytes (async).
    pub rx_frames: VecDeque<Vec<u8>>,
    pub rx_bytes: VecDeque<u8>,

    // Transmit side
    tx_buf: Option<u8>,
    shifter: Shifter,
    tx_clock_on: bool,
    next_tx_at: u64,
    tx_ip: bool,
    /// Tx underrun/EOM latch is reset ("armed"): an underrun sends the CRC.
    eom_armed: bool,
    in_frame: bool,
    frame: Vec<u8>,
    /// Completed transmit frames (HDLC) waiting for the host.
    pub tx_frames: VecDeque<Vec<u8>>,
    /// Transmitted async bytes waiting for the host.
    pub tx_bytes: Vec<u8>,

    // External / status
    pub dcd: bool,
    pub cts: bool,
    pub sync: bool,
    ext_ip: bool,
    last_rr0_ext: u8,

    /// Character time in CPU cycles.
    pub char_cycles: u64,
}

impl Channel {
    fn new(name: char, char_cycles: u64) -> Self {
        Channel {
            name,
            wr: [0; 8],
            ptr: 0,
            rx_data: 0,
            rx_avail: false,
            rr1_latched: 0,
            rx_special_ip: false,
            rx_char_ip: false,
            rx_stream: VecDeque::new(),
            next_rx_at: 0,
            rx_dcd_off_at: None,
            rx_frames: VecDeque::new(),
            rx_bytes: VecDeque::new(),
            tx_buf: None,
            shifter: Shifter::Idle,
            tx_clock_on: false,
            next_tx_at: 0,
            tx_ip: false,
            eom_armed: false,
            in_frame: false,
            frame: Vec::new(),
            tx_frames: VecDeque::new(),
            tx_bytes: Vec::new(),
            dcd: false,
            cts: false,
            sync: false,
            ext_ip: false,
            last_rr0_ext: 0,
            char_cycles,
        }
    }

    fn reset(&mut self) {
        self.wr = [0; 8];
        self.ptr = 0;
        self.rx_avail = false;
        self.rr1_latched = 0;
        self.rx_special_ip = false;
        self.rx_char_ip = false;
        self.rx_stream.clear();
        self.tx_buf = None;
        self.shifter = Shifter::Idle;
        self.tx_clock_on = false;
        self.tx_ip = false;
        self.eom_armed = false;
        self.in_frame = false;
        self.frame.clear();
        self.ext_ip = false;
        self.last_rr0_ext = self.ext_bits();
    }

    /// SDLC mode: WR4 sync mode bits 5:4 = 10 and stop bits 3:2 = 00.
    pub fn is_sdlc(&self) -> bool {
        self.wr[4] & 0x3c == 0x20
    }

    fn rx_enabled(&self) -> bool {
        self.wr[3] & 0x01 != 0
    }

    fn tx_enabled(&self) -> bool {
        self.wr[5] & 0x08 != 0
    }

    /// RTS output (WR5 bit 1).
    pub fn rts(&self) -> bool {
        self.wr[5] & 0x02 != 0
    }

    /// DTR output (WR5 bit 7).
    pub fn dtr(&self) -> bool {
        self.wr[5] & 0x80 != 0
    }

    fn ext_int_enabled(&self) -> bool {
        self.wr[1] & 0x01 != 0
    }

    fn tx_int_enabled(&self) -> bool {
        self.wr[1] & 0x02 != 0
    }

    fn rx_int_mode(&self) -> u8 {
        (self.wr[1] >> 3) & 0x03
    }

    /// The external/status bits of RR0 (the ones that raise ext interrupts).
    fn ext_bits(&self) -> u8 {
        let mut v = 0;
        if self.dcd {
            v |= 0x08;
        }
        if self.sync {
            v |= 0x10;
        }
        if self.cts {
            v |= 0x20;
        }
        if !self.eom_armed {
            v |= 0x40;
        }
        v
    }

    /// Raise an ext/status interrupt if any of the status bits changed.
    fn check_ext(&mut self) {
        let now = self.ext_bits();
        let changed = now ^ self.last_rr0_ext;
        if changed != 0 {
            self.last_rr0_ext = now;
            // the Tx underrun/EOM bit only interrupts when it sets
            let relevant = changed & !(0x40 & !now);
            if relevant != 0 && self.ext_int_enabled() {
                self.ext_ip = true;
            }
        }
    }

    pub fn rr0(&self) -> u8 {
        let mut v = self.ext_bits();
        if self.rx_avail {
            v |= 0x01;
        }
        if self.tx_buf.is_none() {
            v |= 0x04;
        }
        v
    }

    fn all_sent(&self) -> bool {
        self.tx_buf.is_none() && self.shifter == Shifter::Idle
    }

    pub fn rr1(&self) -> u8 {
        let mut v = self.rr1_latched;
        if self.is_sdlc() && v & 0x80 == 0 {
            // residue code for 8 bit characters when not at end of frame
            v |= 0x06;
        }
        if self.all_sent() {
            v |= 0x01;
        }
        v
    }

    fn pending(&self) -> Option<Src> {
        if self.rx_special_ip {
            return Some(Src::RxSpecial);
        }
        if self.rx_char_ip {
            return Some(Src::RxChar);
        }
        if self.tx_ip {
            return Some(Src::Tx);
        }
        if self.ext_ip {
            return Some(Src::Ext);
        }
        None
    }

    pub fn ext_pending(&self) -> bool {
        self.ext_ip
    }

    fn write_ctrl(&mut self, v: u8, now: u64) {
        let reg = self.ptr;
        self.ptr = 0;
        if reg != 0 {
            let old = self.wr[reg];
            self.wr[reg] = v;
            if reg == 5 {
                self.wr5_changed(old, now);
            }
            if reg == 1 {
                self.last_rr0_ext = self.ext_bits();
            }
            return;
        }
        self.wr[0] = v;
        self.ptr = (v & 0x07) as usize;
        match (v >> 3) & 0x07 {
            1 => {
                // send abort
                if self.in_frame {
                    log::debug(&format!("SIO {}: send abort, frame dropped", self.name));
                }
                self.in_frame = false;
                self.frame.clear();
                self.tx_buf = None;
                self.shifter = Shifter::Idle;
            }
            2 => {
                self.ext_ip = false;
                self.last_rr0_ext = self.ext_bits();
            }
            3 => {
                self.reset();
            }
            4 => {
                // enable interrupt on next rx character: nothing to do here
            }
            5 => {
                self.tx_ip = false;
            }
            6 => {
                self.rr1_latched = 0;
                self.rx_special_ip = false;
            }
            _ => {}
        }
        match v >> 6 {
            2 => {
                // reset Tx CRC generator: a new frame starts
                self.in_frame = true;
                self.frame.clear();
            }
            3 => {
                self.eom_armed = true;
                self.check_ext();
            }
            _ => {}
        }
    }

    fn wr5_changed(&mut self, old: u8, now: u64) {
        let was = old & 0x08 != 0;
        let is = self.tx_enabled();
        if is && !was {
            self.tx_clock_on = true;
            self.shifter = Shifter::Idle;
            self.next_tx_at = now + self.char_cycles;
        } else if !is && was {
            self.tx_clock_on = false;
            self.shifter = Shifter::Idle;
            if self.in_frame && !self.frame.is_empty() {
                log::debug(&format!(
                    "SIO {}: transmitter disabled mid frame, {} bytes dropped",
                    self.name,
                    self.frame.len()
                ));
            }
            self.in_frame = false;
            self.frame.clear();
        }
    }

    fn write_data(&mut self, v: u8, now: u64) {
        self.tx_ip = false;
        if self.is_sdlc() {
            self.tx_buf = Some(v);
        } else {
            // async: load the shifter directly if it is idle
            if self.tx_enabled() && self.shifter == Shifter::Idle && self.tx_buf.is_none() {
                self.shift_out_async(v, now);
            } else {
                self.tx_buf = Some(v);
            }
        }
    }

    fn shift_out_async(&mut self, v: u8, now: u64) {
        self.tx_bytes.push(v);
        self.shifter = Shifter::Data;
        self.next_tx_at = now + self.char_cycles;
        self.tx_clock_on = true;
        if self.tx_int_enabled() {
            self.tx_ip = true;
        }
    }

    fn read_data(&mut self) -> u8 {
        self.rx_avail = false;
        self.rx_char_ip = false;
        self.rx_data
    }

    fn read_ctrl(&mut self) -> u8 {
        let reg = self.ptr;
        self.ptr = 0;
        match reg {
            0 => self.rr0(),
            1 => self.rr1(),
            _ => 0,
        }
    }

    /// Advance the channel to cycle `now`.
    fn run(&mut self, now: u64) {
        self.run_tx(now);
        self.run_rx(now);
    }

    fn run_tx(&mut self, now: u64) {
        if !self.is_sdlc() {
            // async transmitter
            while self.tx_clock_on && now >= self.next_tx_at {
                let t = self.next_tx_at;
                if let Some(b) = self.tx_buf.take() {
                    self.shift_out_async(b, t);
                } else {
                    self.shifter = Shifter::Idle;
                    self.tx_clock_on = false;
                }
            }
            return;
        }
        while self.tx_clock_on && now >= self.next_tx_at {
            // character boundary
            if let Some(b) = self.tx_buf.take() {
                if self.in_frame {
                    self.frame.push(b);
                }
                self.shifter = Shifter::Data;
                if self.tx_int_enabled() {
                    self.tx_ip = true;
                }
            } else {
                match self.shifter {
                    Shifter::Data => {
                        // underrun
                        if self.eom_armed {
                            if self.in_frame {
                                let f = std::mem::take(&mut self.frame);
                                self.tx_frames.push_back(f);
                            }
                            self.in_frame = false;
                            self.eom_armed = false;
                            self.shifter = Shifter::Crc(2);
                            self.check_ext();
                        } else {
                            // latch already set: idle with flags
                            if self.in_frame && !self.frame.is_empty() {
                                log::debug(&format!(
                                    "SIO {}: underrun with EOM latch set, {} bytes dropped",
                                    self.name,
                                    self.frame.len()
                                ));
                            }
                            self.in_frame = false;
                            self.frame.clear();
                            self.shifter = Shifter::Idle;
                        }
                    }
                    Shifter::Crc(n) => {
                        if n <= 1 {
                            // CRC done, closing flag loaded
                            self.shifter = Shifter::Idle;
                            if self.tx_int_enabled() {
                                self.tx_ip = true;
                            }
                        } else {
                            self.shifter = Shifter::Crc(n - 1);
                        }
                    }
                    Shifter::Idle => {}
                }
            }
            self.next_tx_at += self.char_cycles;
        }
    }

    fn run_rx(&mut self, now: u64) {
        if !self.rx_enabled() {
            if self.is_sdlc() && !self.rx_stream.is_empty() {
                // receiver switched off part way through a frame: it is lost
                log::debug(&format!(
                    "SIO {}: receiver disabled mid frame, {} characters lost",
                    self.name,
                    self.rx_stream.len()
                ));
                self.rx_stream.clear();
                self.rx_dcd_off_at = Some(now);
            }
            if self.rx_dcd_off_at.is_some_and(|t| now >= t) {
                self.rx_dcd_off_at = None;
                self.dcd = false;
                self.check_ext();
            }
            return;
        }
        if self.is_sdlc() {
            if self.rx_stream.is_empty() {
                let busy = self.tx_clock_on || self.rts();
                if let Some(t) = self.rx_dcd_off_at {
                    if now < t {
                        return;
                    }
                    self.rx_dcd_off_at = None;
                    if busy || self.rx_frames.is_empty() {
                        // carrier drops after the closing flag
                        self.dcd = false;
                        self.check_ext();
                        return;
                    }
                }
                // a simplex radio does not hear while it transmits
                if busy {
                    return;
                }
                if let Some(f) = self.rx_frames.pop_front() {
                    for &b in &f {
                        self.rx_stream.push_back((b, false));
                    }
                    // two CRC characters; the last one carries end of frame
                    self.rx_stream.push_back((0x00, false));
                    self.rx_stream.push_back((0x00, true));
                    // carrier detect comes up, then the opening flag
                    if !self.dcd {
                        self.dcd = true;
                        self.check_ext();
                    }
                    self.next_rx_at = now.max(self.next_rx_at) + 2 * self.char_cycles;
                }
                return;
            }
            while now >= self.next_rx_at {
                let Some((b, eof)) = self.rx_stream.pop_front() else {
                    break;
                };
                if self.rx_avail || self.rx_special_ip {
                    // previous character not taken: overrun
                    self.rr1_latched |= 0x20;
                    self.rx_special_ip = true;
                }
                self.rx_data = b;
                self.rx_avail = true;
                if eof {
                    // end of frame, CRC OK, residue 011 (8 bit chars)
                    self.rr1_latched |= 0x80 | 0x06;
                    if self.rx_int_mode() != 0 {
                        self.rx_special_ip = true;
                    }
                    // closing flag plus a gap before the next frame
                    self.next_rx_at += 3 * self.char_cycles;
                    self.rx_dcd_off_at = Some(self.next_rx_at);
                } else {
                    if self.rx_int_mode() != 0 {
                        self.rx_char_ip = true;
                    }
                    self.next_rx_at += self.char_cycles;
                }
                if self.rx_stream.is_empty() {
                    break;
                }
            }
        } else {
            // async: hand over one byte per character time, never overrun
            if now >= self.next_rx_at && !self.rx_avail {
                if let Some(b) = self.rx_bytes.pop_front() {
                    self.rx_data = b;
                    self.rx_avail = true;
                    if self.rx_int_mode() != 0 {
                        self.rx_char_ip = true;
                    }
                    self.next_rx_at = now + self.char_cycles;
                }
            }
        }
    }

    pub fn debug_state(&self) -> String {
        format!(
            "wr1={:02X} wr3={:02X} wr5={:02X} txbuf={:?} shifter={:?} clk={} ip(rs,rc,tx,ext)={},{},{},{} eom_armed={} rx_frames={} rx_stream={}",
            self.wr[1], self.wr[3], self.wr[5], self.tx_buf, self.shifter, self.tx_clock_on,
            self.rx_special_ip, self.rx_char_ip, self.tx_ip, self.ext_ip, self.eom_armed,
            self.rx_frames.len(), self.rx_stream.len()
        )
    }
}

pub struct Sio {
    pub a: Channel,
    pub b: Channel,
}

impl Sio {
    pub fn new(a_char_cycles: u64, b_char_cycles: u64) -> Self {
        Sio {
            a: Channel::new('A', a_char_cycles),
            b: Channel::new('B', b_char_cycles),
        }
    }

    /// Port offset: bit 0 selects control (1) or data (0), bit 1 selects
    /// channel B (1) or A (0). This matches TheNet's use of ports 0 to 3.
    pub fn write(&mut self, port: u8, v: u8, now: u64) {
        let ch = if port & 2 != 0 { &mut self.b } else { &mut self.a };
        if port & 1 != 0 {
            ch.write_ctrl(v, now);
        } else {
            ch.write_data(v, now);
        }
    }

    pub fn read(&mut self, port: u8) -> u8 {
        let is_b = port & 2 != 0;
        if port & 1 != 0 {
            if is_b && self.b.ptr == 2 {
                self.b.ptr = 0;
                return self.vector();
            }
            let ch = if is_b { &mut self.b } else { &mut self.a };
            ch.read_ctrl()
        } else {
            let ch = if is_b { &mut self.b } else { &mut self.a };
            ch.read_data()
        }
    }

    pub fn run(&mut self, now: u64) {
        self.a.run(now);
        self.b.run(now);
    }

    pub fn set_ext(&mut self) {
        self.a.check_ext();
        self.b.check_ext();
    }

    pub fn int_active(&self) -> bool {
        self.a.pending().is_some() || self.b.pending().is_some()
    }

    /// Interrupt vector, with "status affects vector" (WR1B bit 2) applied.
    pub fn vector(&self) -> u8 {
        let base = self.b.wr[2];
        if self.b.wr[1] & 0x04 == 0 {
            return base;
        }
        let code: u8 = if let Some(s) = self.a.pending() {
            match s {
                Src::Tx => 4,
                Src::Ext => 5,
                Src::RxChar => 6,
                Src::RxSpecial => 7,
            }
        } else if let Some(s) = self.b.pending() {
            match s {
                Src::Tx => 0,
                Src::Ext => 1,
                Src::RxChar => 2,
                Src::RxSpecial => 3,
            }
        } else {
            // no source pending: the chip would not drive the bus
            3
        };
        (base & 0xf1) | (code << 1)
    }
}

pub mod log {
    use std::sync::atomic::{AtomicBool, Ordering};
    pub static DEBUG: AtomicBool = AtomicBool::new(false);
    pub fn debug(s: &str) {
        if DEBUG.load(Ordering::Relaxed) {
            eprintln!("debug: {s}");
        }
    }
}
