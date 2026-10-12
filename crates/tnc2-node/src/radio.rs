//! KISS to and from the NinoTNC.
//!
//! The serial link has no checksum of its own and neither does KISS, so a
//! byte the UART flagged (framing, parity, break or overrun error) poisons
//! the frame it falls in: the whole frame is dropped, up to and including
//! the FEND that ends it, and AX.25 retransmits. The same goes for bytes
//! lost outright (`lost`). Only data frames for KISS port 0 are passed on;
//! anything else the TNC says is ignored.

use tnc2_core::kiss::{Decoder, FEND, encode_with};

/// The data register of the RP2350's UART (a PL011) gives each received
/// byte with its error flags in bits 8 to 11: framing, parity, break,
/// overrun.
pub const UART_ERROR_BITS: u32 = 0xF00;

/// A word read from the UART data register: the byte, and whether the
/// UART flagged it.
pub fn uart_word(w: u32) -> (u8, bool) {
    (w as u8, w & UART_ERROR_BITS != 0)
}

/// What became of the frames received so far.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RxCounts {
    /// Port 0 data frames passed on.
    pub frames: u32,
    /// Frames dropped because a serial error or lost bytes touched them.
    pub line_errors: u32,
    /// KISS frames that were not port 0 data, ignored.
    pub ignored: u32,
}

/// Received bytes in, port 0 data frames out.
pub struct KissIn {
    dec: Decoder,
    poisoned: bool,
    counts: RxCounts,
}

impl Default for KissIn {
    fn default() -> Self {
        Self::new()
    }
}

impl KissIn {
    pub const fn new() -> Self {
        KissIn { dec: Decoder::new(), poisoned: false, counts: RxCounts { frames: 0, line_errors: 0, ignored: 0 } }
    }

    pub fn counts(&self) -> RxCounts {
        self.counts
    }

    /// Frames dropped for being longer than the decoder takes.
    pub fn too_long(&self) -> u64 {
        self.dec.dropped()
    }

    /// Bytes went missing (the receive buffer overran): drop the frame in
    /// progress.
    pub fn lost(&mut self) {
        self.poisoned = true;
    }

    /// One received byte, with the UART's error flag. Returns a complete
    /// AX.25 frame (KISS command byte removed) when `b` ends one.
    pub fn byte(&mut self, b: u8, error: bool) -> Option<&[u8]> {
        if error {
            // The byte itself is not to be trusted, so it is not fed in:
            // it may have been the FEND that ended this frame.
            self.poisoned = true;
            return None;
        }
        let poisoned = self.poisoned;
        if b == FEND {
            // Whatever follows starts clean.
            self.poisoned = false;
        }
        let f = self.dec.push(b)?;
        if poisoned {
            self.counts.line_errors += 1;
            return None;
        }
        if f[0] != 0x00 {
            self.counts.ignored += 1;
            return None;
        }
        self.counts.frames += 1;
        Some(&f[1..])
    }
}

/// The longest KISS encoding of a frame of `n` bytes.
pub const fn encoded_len(n: usize) -> usize {
    2 * n + 3
}

/// Encode an AX.25 frame as a KISS port 0 data frame into `out`, which
/// must hold `encoded_len(frame.len())`. Returns the bytes used.
pub fn encode(frame: &[u8], out: &mut [u8]) -> usize {
    let mut n = 0;
    encode_with(0x00, frame, |b| {
        out[n] = b;
        n += 1;
    });
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;
    use tnc2_core::kiss::{FESC, TFEND, encode as kiss};

    /// Feed (byte, error) pairs and collect the frames.
    fn run(k: &mut KissIn, input: &[(u8, bool)]) -> Vec<Vec<u8>> {
        input.iter().filter_map(|&(b, e)| k.byte(b, e).map(|f| f.to_vec())).collect()
    }

    fn clean(bytes: &[u8]) -> Vec<(u8, bool)> {
        bytes.iter().map(|&b| (b, false)).collect()
    }

    #[test]
    fn data_frames_pass() {
        let mut k = KissIn::new();
        let mut input = kiss(0x00, b"hello");
        input.extend(kiss(0x00, &[FEND, 1, FESC]));
        assert_eq!(run(&mut k, &clean(&input)), [b"hello".to_vec(), [FEND, 1, FESC].to_vec()]);
        assert_eq!(k.counts(), RxCounts { frames: 2, line_errors: 0, ignored: 0 });
    }

    #[test]
    fn other_ports_and_commands_are_ignored() {
        let mut k = KissIn::new();
        let mut input = kiss(0x10, b"port 1");
        input.extend(kiss(0x06, b"GETALL reply"));
        input.extend(kiss(0x00, b"ok"));
        assert_eq!(run(&mut k, &clean(&input)), [b"ok".to_vec()]);
        assert_eq!(k.counts().ignored, 2);
    }

    #[test]
    fn an_error_anywhere_in_a_frame_drops_it_and_only_it() {
        let one = kiss(0x00, b"first frame");
        let two = kiss(0x00, b"second");
        for at in 0..one.len() {
            let mut input = clean(&one);
            input[at].1 = true;
            input.extend(clean(&two));
            let mut k = KissIn::new();
            let got = run(&mut k, &input);
            assert_eq!(got, [b"second".to_vec()], "error at {at}");
            assert_eq!(k.counts().frames, 1);
        }
    }

    #[test]
    fn a_lost_closing_fend_drops_both_frames_it_joins() {
        // The FEND between two frames comes in with a framing error: the
        // two run together and must not be delivered as one.
        let mut input = clean(&kiss(0x00, b"aaaa"));
        let last = input.len() - 1;
        input[last].1 = true;
        input.extend(clean(&kiss(0x00, b"bbbb")[1..]));
        input.extend(clean(&kiss(0x00, b"cccc")));
        let mut k = KissIn::new();
        assert_eq!(run(&mut k, &input), [b"cccc".to_vec()]);
        assert_eq!(k.counts().line_errors, 1);
    }

    #[test]
    fn an_error_between_frames_costs_nothing() {
        let mut input = clean(&kiss(0x00, b"one"));
        input.push((0x55, true));
        input.extend(clean(&kiss(0x00, b"two")));
        let mut k = KissIn::new();
        assert_eq!(run(&mut k, &input), [b"one".to_vec(), b"two".to_vec()]);
    }

    #[test]
    fn lost_bytes_drop_the_frame_in_progress() {
        let f = kiss(0x00, b"partial");
        let mut k = KissIn::new();
        assert!(run(&mut k, &clean(&f[..4])).is_empty());
        k.lost();
        assert!(run(&mut k, &clean(&f[4..])).is_empty());
        assert_eq!(run(&mut k, &clean(&kiss(0x00, b"next"))), [b"next".to_vec()]);
        assert_eq!(k.counts().line_errors, 1);
    }

    #[test]
    fn uart_words() {
        assert_eq!(uart_word(0x0C0), (0xC0, false));
        for bit in [0x100, 0x200, 0x400, 0x800] {
            assert_eq!(uart_word(0x41 | bit), (0x41, true));
        }
    }

    #[test]
    fn encodes_port_0_data() {
        let frame = [1, FEND, 2];
        let mut out = [0u8; encoded_len(3)];
        let n = encode(&frame, &mut out);
        assert_eq!(&out[..n], &[FEND, 0x00, 1, FESC, TFEND, 2, FEND]);
        let worst = [FEND; 10];
        let mut out = [0u8; encoded_len(10)];
        assert_eq!(encode(&worst, &mut out), encoded_len(10));
    }
}
