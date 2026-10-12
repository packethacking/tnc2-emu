//! Z80 SIO/0 emulation at character / frame level.
//!
//! Channel A is the radio (HDLC/SDLC) side, channel B is the RS232 async
//! side. Bits on the wire are not modelled: the transmitter runs a
//! character clock (one character time per byte at the configured bit
//! rate), the receiver is fed whole frames and hands them to the CPU one
//! character at a time with the status the real chip would give.
//!
//! See docs/hardware.md for how TheNet X1J drives this chip.

use crate::{Diags, Event};
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::fmt;

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

#[cfg_attr(test, derive(Clone, Debug))]
pub struct Channel {
    pub(crate) name: char,
    pub(crate) wr: [u8; 8],
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
    pub(crate) rx_frames: VecDeque<Vec<u8>>,
    pub(crate) rx_bytes: VecDeque<u8>,

    // Transmit side
    tx_buf: Option<u8>,
    shifter: Shifter,
    tx_clock_on: bool,
    next_tx_at: u64,
    tx_ip: bool,
    /// "Reset Tx interrupt pending" was the last word: no Tx interrupt
    /// "until after the next character is loaded to the transmit buffer or
    /// until CRC is completely sent" (UM0081, WR0 command 5).
    tx_int_disarmed: bool,
    /// Tx underrun/EOM latch is reset ("armed"): an underrun sends the CRC.
    eom_armed: bool,
    in_frame: bool,
    frame: Vec<u8>,
    /// Completed transmit frames (HDLC) waiting for the host.
    pub(crate) tx_frames: VecDeque<Vec<u8>>,
    /// Transmitted async bytes waiting for the host.
    pub(crate) tx_bytes: VecDeque<u8>,

    // External / status
    pub(crate) dcd: bool,
    pub(crate) cts: bool,
    pub(crate) sync: bool,
    ext_ip: bool,
    last_rr0_ext: u8,

    /// Character time in CPU cycles.
    pub(crate) char_cycles: u64,
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
            tx_int_disarmed: false,
            eom_armed: false,
            in_frame: false,
            frame: Vec::new(),
            tx_frames: VecDeque::new(),
            tx_bytes: VecDeque::new(),
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
        self.tx_int_disarmed = false;
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

    fn write_ctrl(&mut self, v: u8, now: u64, diag: &mut Diags) {
        let reg = self.ptr;
        self.ptr = 0;
        if reg != 0 {
            let old = self.wr[reg];
            self.wr[reg] = v;
            if reg == 5 {
                self.wr5_changed(old, now, diag);
            }
            if reg == 1 {
                self.last_rr0_ext = self.ext_bits();
            }
            return;
        }
        self.wr[0] = v;
        self.ptr = (v & 0x07) as usize;
        match (v >> 3) & 0x07 {
            1 if self.is_sdlc() => {
                // Send abort, SDLC only: "any data being transmitted and
                // any data in the transmit buffer is lost" (Z80 SIO
                // manual, UM0081). If that empties the buffer it is a
                // transmit buffer empty interrupt like any other, unless
                // disarmed by a reset Tx interrupt pending since the last
                // data write. The chip counts the buffer as full while the
                // CRC goes out (MAME's z80sio does the same), though rr0()
                // here reports it empty then. TheNet relies on the
                // interrupt: it aborts the padding after a frame when a new
                // frame is queued, or an underrun mid frame (after writing
                // the next byte), sets its transmit state to 6 and starts
                // the next frame from that interrupt (0x028A, 0x0633,
                // 0x05C2). The abort's 8 to 13 ones are not timed.
                let was_full = self.tx_buf.is_some() || matches!(self.shifter, Shifter::Crc(_));
                if self.in_frame {
                    diag.push(Event::SendAbort { channel: self.name });
                }
                self.in_frame = false;
                self.frame.clear();
                self.tx_buf = None;
                self.shifter = Shifter::Idle;
                if was_full {
                    self.tx_buffer_emptied();
                }
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
                self.tx_int_disarmed = true;
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

    fn wr5_changed(&mut self, old: u8, now: u64, diag: &mut Diags) {
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
                diag.push(Event::TxDisabledMidFrame { channel: self.name, bytes: self.frame.len() });
            }
            self.in_frame = false;
            self.frame.clear();
        }
    }

    /// The transmit buffer became empty: a Tx interrupt if enabled and not
    /// disarmed.
    fn tx_buffer_emptied(&mut self) {
        if self.tx_int_enabled() && !self.tx_int_disarmed {
            self.tx_ip = true;
        }
    }

    fn write_data(&mut self, v: u8, now: u64) {
        self.tx_ip = false;
        self.tx_int_disarmed = false;
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
        self.tx_bytes.push_back(v);
        self.shifter = Shifter::Data;
        self.next_tx_at = now + self.char_cycles;
        self.tx_clock_on = true;
        self.tx_buffer_emptied();
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
    fn run(&mut self, now: u64, diag: &mut Diags) {
        self.run_tx(now, diag);
        self.run_rx(now, diag);
    }

    /// The first cycle at which `run` could change anything, if nothing
    /// else touches the channel first: before it, `run` does nothing. It
    /// may be early (0 is always safe) but never late. Mirrors `run_tx`
    /// and `run_rx`; keep the three in step.
    fn next_event(&self) -> u64 {
        let mut t = u64::MAX;
        if self.tx_clock_on {
            t = self.next_tx_at;
        }
        if !self.rx_enabled() {
            if self.is_sdlc() && !self.rx_stream.is_empty() {
                return 0;
            }
            if let Some(d) = self.rx_dcd_off_at {
                t = t.min(d);
            }
        } else if self.is_sdlc() {
            if !self.rx_stream.is_empty() {
                t = t.min(self.next_rx_at);
            } else if let Some(d) = self.rx_dcd_off_at {
                t = t.min(d);
            } else if !self.rx_frames.is_empty() && !self.tx_clock_on && !self.rts() {
                // a frame waiting and the radio not transmitting
                return 0;
            }
        } else if !self.rx_avail && !self.rx_bytes.is_empty() {
            t = t.min(self.next_rx_at);
        }
        t
    }

    fn run_tx(&mut self, now: u64, diag: &mut Diags) {
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
                self.tx_buffer_emptied();
            } else {
                match self.shifter {
                    Shifter::Data => {
                        // underrun
                        if self.eom_armed {
                            if self.in_frame {
                                let f = core::mem::take(&mut self.frame);
                                self.tx_frames.push_back(f);
                            }
                            self.in_frame = false;
                            self.eom_armed = false;
                            self.shifter = Shifter::Crc(2);
                            self.check_ext();
                        } else {
                            // latch already set: idle with flags
                            if self.in_frame && !self.frame.is_empty() {
                                diag.push(Event::UnderrunWithEom { channel: self.name, bytes: self.frame.len() });
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
                            self.tx_int_disarmed = false;
                            self.tx_buffer_emptied();
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

    fn run_rx(&mut self, now: u64, diag: &mut Diags) {
        if !self.rx_enabled() {
            if self.is_sdlc() && !self.rx_stream.is_empty() {
                // receiver switched off part way through a frame: it is lost
                diag.push(Event::RxDisabledMidFrame { channel: self.name, chars: self.rx_stream.len() });
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

    pub fn debug_state(&self) -> DebugState<'_> {
        DebugState(self)
    }
}

/// One line of channel state for --debug.
pub struct DebugState<'a>(&'a Channel);

impl fmt::Display for DebugState<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = self.0;
        write!(
            f,
            "wr1={:02X} wr3={:02X} wr5={:02X} txbuf={:?} shifter={:?} clk={} ip(rs,rc,tx,ext)={},{},{},{} eom_armed={} rx_frames={} rx_stream={}",
            c.wr[1], c.wr[3], c.wr[5], c.tx_buf, c.shifter, c.tx_clock_on,
            c.rx_special_ip, c.rx_char_ip, c.tx_ip, c.ext_ip, c.eom_armed,
            c.rx_frames.len(), c.rx_stream.len()
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
    pub fn write(&mut self, port: u8, v: u8, now: u64, diag: &mut Diags) {
        let ch = if port & 2 != 0 { &mut self.b } else { &mut self.a };
        if port & 1 != 0 {
            ch.write_ctrl(v, now, diag);
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

    pub fn run(&mut self, now: u64, diag: &mut Diags) {
        self.a.run(now, diag);
        self.b.run(now, diag);
    }

    /// The first cycle at which `run` could change anything (see
    /// `Channel::next_event`).
    pub fn next_event(&self) -> u64 {
        self.a.next_event().min(self.b.next_event())
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Channel A set up as TheNet does (SDLC, Tx and ext interrupts), with
    /// the transmitter on.
    fn sdlc() -> (Channel, Diags) {
        let mut d = Diags::new();
        let mut c = Channel::new('A', 100);
        for (r, v) in [(4, 0x20), (1, 0x1b), (5, 0xeb)] {
            c.write_ctrl(r, 0, &mut d);
            c.write_ctrl(v, 0, &mut d);
        }
        (c, d)
    }

    /// TheNet aborts the padding after a frame when a new one is queued
    /// and starts that frame from the next Tx interrupt (issue 4).
    #[test]
    fn abort_with_a_character_waiting_interrupts() {
        let (mut c, mut d) = sdlc();
        c.write_data(0x00, 0);
        c.run(100, &mut d); // into the shifter
        c.write_ctrl(0x28, 100, &mut d); // reset Tx int pending
        c.write_data(0x00, 100);
        assert_eq!(c.pending(), None);
        c.write_ctrl(0x08, 110, &mut d); // send abort
        assert_eq!(c.pending(), Some(Src::Tx));
        assert_eq!(c.tx_buf, None);
    }

    /// One byte framed and sent, the Tx interrupt for it taken with a
    /// reset Tx interrupt pending, and the underrun reached: the CRC is
    /// going out.
    fn in_crc() -> (Channel, Diags) {
        let (mut c, mut d) = sdlc();
        c.write_ctrl(0x80, 0, &mut d); // reset Tx CRC
        c.write_data(0x41, 0);
        c.write_ctrl(0xc0, 0, &mut d); // reset EOM latch
        c.run(100, &mut d);
        c.write_ctrl(0x28, 100, &mut d);
        c.run(200, &mut d); // underrun: CRC
        assert!(matches!(c.shifter, Shifter::Crc(_)));
        (c, d)
    }

    /// TheNet's late underrun (0x0633): the Tx interrupt during the CRC
    /// writes the next byte, then the ext/status interrupt aborts. The
    /// abort empties the buffer and interrupts.
    #[test]
    fn abort_during_crc_with_a_byte_written_interrupts() {
        let (mut c, mut d) = in_crc();
        c.write_data(0x42, 200);
        c.write_ctrl(0x08, 210, &mut d);
        assert_eq!(c.pending(), Some(Src::Tx));
    }

    /// A reset Tx interrupt pending with nothing written since disarms the
    /// interrupt until the CRC is completely sent, and the abort means it
    /// never is.
    #[test]
    fn abort_during_crc_after_reset_tx_int_is_quiet() {
        let (mut c, mut d) = in_crc();
        c.write_ctrl(0x08, 210, &mut d);
        // the underrun's ext/status interrupt is still pending, nothing else
        assert_eq!(c.pending(), Some(Src::Ext));
        assert!(!c.tx_ip);
    }

    /// Without the abort the CRC completes and rearms the interrupt.
    #[test]
    fn crc_done_interrupts_after_reset_tx_int() {
        let (mut c, mut d) = in_crc();
        assert!(!c.tx_ip);
        c.run(400, &mut d);
        assert_eq!(c.shifter, Shifter::Idle);
        assert!(c.tx_ip);
    }

    /// With the buffer already empty an abort does not interrupt.
    #[test]
    fn abort_with_buffer_empty_is_quiet() {
        let (mut c, mut d) = sdlc();
        c.write_ctrl(0x08, 0, &mut d);
        assert_eq!(c.pending(), None);
    }

    /// An abort with the buffer empty leaves a Tx interrupt that is
    /// already pending alone.
    #[test]
    fn abort_keeps_a_pending_tx_interrupt() {
        let (mut c, mut d) = sdlc();
        c.write_data(0x00, 0);
        c.run(100, &mut d); // into the shifter, buffer empty
        assert_eq!(c.pending(), Some(Src::Tx));
        c.write_ctrl(0x08, 110, &mut d);
        assert_eq!(c.tx_buf, None);
        assert_eq!(c.pending(), Some(Src::Tx));
    }

    /// Send abort is an SDLC command; in async mode it does nothing.
    #[test]
    fn abort_is_ignored_in_async_mode() {
        let mut d = Diags::new();
        let mut c = Channel::new('B', 100);
        for (r, v) in [(4, 0x44), (1, 0x1f), (5, 0xea)] {
            c.write_ctrl(r, 0, &mut d);
            c.write_ctrl(v, 0, &mut d);
        }
        c.write_data(0x41, 0);
        c.write_data(0x42, 10); // waits in the buffer
        c.write_ctrl(0x08, 20, &mut d);
        assert_eq!(c.tx_buf, Some(0x42));
    }

    /// `next_event` is never late: running the channel at any cycle before
    /// it changes nothing. Random register writes, data, reads, received
    /// frames and bytes and carrier changes, in both modes, checked before
    /// every step.
    #[test]
    fn next_event_is_never_late() {
        for sdlc_mode in [true, false] {
            let mut d = Diags::new();
            let mut c = Channel::new('A', 100);
            let setup: &[(u8, u8)] = if sdlc_mode {
                &[(4, 0x20), (1, 0x1b), (3, 0xc9), (5, 0xeb)]
            } else {
                &[(4, 0x44), (1, 0x1f), (3, 0xc1), (5, 0xea)]
            };
            for &(r, v) in setup {
                c.write_ctrl(r, 0, &mut d);
                c.write_ctrl(v, 0, &mut d);
            }
            let regs: &[(u8, u8)] = &[
                (1, 0x1b),
                (1, 0x00),
                (3, 0xc9),
                (3, 0xc8),
                (5, 0xeb),
                (5, 0xe9),
                (5, 0xe3),
                (5, 0xe1),
            ];
            let cmds = [0x08, 0x10, 0x28, 0x30, 0x80, 0xc0];
            let mut rng: u64 = 0x9e3779b97f4a7c15;
            let mut now = 0u64;
            for step in 0..20_000 {
                rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let r = (rng >> 33) as usize;
                now += 1 + (r % 400) as u64;
                let t = c.next_event();
                for probe in [now, t.saturating_sub(1)] {
                    if probe >= now && probe < t {
                        let mut p = c.clone();
                        p.run(probe, &mut Diags::new());
                        assert_eq!(alloc::format!("{p:?}"), alloc::format!("{c:?}"), "step {step}: run at {probe} before {t}");
                    }
                }
                c.run(now, &mut d);
                match (r >> 9) % 9 {
                    0 => {
                        let (reg, v) = regs[(r >> 13) % regs.len()];
                        c.write_ctrl(reg, now, &mut d);
                        c.write_ctrl(v, now, &mut d);
                    }
                    1 => c.write_ctrl(cmds[(r >> 13) % cmds.len()], now, &mut d),
                    2 => c.write_data(r as u8, now),
                    3 => {
                        c.read_data();
                    }
                    4 => {
                        c.read_ctrl();
                    }
                    5 if c.rx_frames.len() < 3 => c.rx_frames.push_back(alloc::vec![r as u8; 1 + (r >> 13) % 40]),
                    6 if c.rx_bytes.len() < 6 => c.rx_bytes.extend([r as u8, (r >> 8) as u8]),
                    7 => {
                        c.dcd = !c.dcd;
                        c.check_ext();
                    }
                    _ => {}
                }
                // the host takes what was sent
                c.tx_frames.clear();
                c.tx_bytes.clear();
                while d.pop().is_some() {}
            }
        }
    }
}
