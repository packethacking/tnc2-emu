//! KISS framing: the TCP KISS port on the host, and the serial link to a
//! KISS TNC on the firmware.

use alloc::vec::Vec;

pub const FEND: u8 = 0xC0;
pub const FESC: u8 = 0xDB;
pub const TFEND: u8 = 0xDC;
pub const TFESC: u8 = 0xDD;

/// Longest KISS frame the decoder keeps, command byte included. Anything
/// past this is dropped and the frame is delivered short.
pub const MAX_FRAME: usize = 2048;

/// Encode one frame, handing the output to `out` a byte at a time:
/// FEND, the port/command byte, the escaped frame, FEND.
pub fn encode_with(port_cmd: u8, frame: &[u8], mut out: impl FnMut(u8)) {
    out(FEND);
    out(port_cmd);
    for &b in frame {
        match b {
            FEND => {
                out(FESC);
                out(TFEND);
            }
            FESC => {
                out(FESC);
                out(TFESC);
            }
            x => out(x),
        }
    }
    out(FEND);
}

/// Encode one frame into a new buffer.
pub fn encode(port_cmd: u8, frame: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(frame.len() + 4);
    encode_with(port_cmd, frame, |b| v.push(b));
    v
}

/// Stream decoder with a fixed buffer. Bytes before the first FEND are
/// ignored and empty frames (back to back FENDs) are skipped.
pub struct Decoder {
    buf: [u8; MAX_FRAME],
    len: usize,
    esc: bool,
    in_frame: bool,
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder {
    pub const fn new() -> Self {
        Decoder { buf: [0; MAX_FRAME], len: 0, esc: false, in_frame: false }
    }

    /// Feed one byte. Returns a complete frame, command byte first and
    /// escapes removed, when `b` is the FEND that closes it.
    pub fn push(&mut self, b: u8) -> Option<&[u8]> {
        if b == FEND {
            let n = self.len;
            let done = self.in_frame && n > 0;
            self.len = 0;
            self.in_frame = true;
            self.esc = false;
            return if done { Some(&self.buf[..n]) } else { None };
        }
        if !self.in_frame {
            return None;
        }
        let v = if self.esc {
            self.esc = false;
            match b {
                TFEND => FEND,
                TFESC => FESC,
                x => x,
            }
        } else if b == FESC {
            self.esc = true;
            return None;
        } else {
            b
        };
        if self.len < MAX_FRAME {
            self.buf[self.len] = v;
            self.len += 1;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn decode_all(d: &mut Decoder, data: &[u8]) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for &b in data {
            if let Some(f) = d.push(b) {
                out.push(f.to_vec());
            }
        }
        out
    }

    #[test]
    fn round_trip_with_escapes() {
        let frame = [0x01, FEND, 0x02, FESC, 0x03, TFEND, TFESC];
        let enc = encode(0x00, &frame);
        assert_eq!(enc, [FEND, 0x00, 0x01, FESC, TFEND, 0x02, FESC, TFESC, 0x03, TFEND, TFESC, FEND]);
        let mut d = Decoder::new();
        let got = decode_all(&mut d, &enc);
        let mut want = vec![0x00];
        want.extend_from_slice(&frame);
        assert_eq!(got, [want]);
    }

    #[test]
    fn noise_and_empty_frames_are_skipped() {
        let mut d = Decoder::new();
        let got = decode_all(&mut d, &[0x55, 0x66, FEND, FEND, FEND, 0x00, 0x41, FEND, 0x00, 0x42, FEND]);
        assert_eq!(got, [vec![0x00, 0x41], vec![0x00, 0x42]]);
    }

    #[test]
    fn split_across_pushes() {
        let enc = encode(0x00, b"hello");
        let mut d = Decoder::new();
        assert!(decode_all(&mut d, &enc[..4]).is_empty());
        assert_eq!(decode_all(&mut d, &enc[4..]), [b"\x00hello".to_vec()]);
    }

    #[test]
    fn long_frames_are_truncated() {
        let frame = vec![0x11; MAX_FRAME + 10];
        let mut d = Decoder::new();
        let got = decode_all(&mut d, &encode(0x00, &frame));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].len(), MAX_FRAME);
    }
}
