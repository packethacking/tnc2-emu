//! Lock free queues between the two cores: one core writes, the other
//! reads, and neither ever waits for the other.
//!
//! A `Pipe` is a ring of `N` bytes (a power of two) with a write count
//! owned by the producer and a read count owned by the consumer, both
//! counting up for ever and wrapping. Data is copied in before the write
//! count moves (release), and the reader only looks at bytes the write
//! count it loaded (acquire) covers, and the other way round for space.
//! `split` hands out the two ends once, so each side is used from one
//! place only.
//!
//! Frames go through a pipe as a two byte length and the bytes, written
//! whole or not at all.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub struct Pipe<const N: usize> {
    buf: UnsafeCell<[u8; N]>,
    written: AtomicUsize,
    read: AtomicUsize,
    taken: AtomicBool,
}

// SAFETY: the producer only writes bytes outside [read, written) and the
// consumer only reads bytes inside it, and the counts are published with
// release and loaded with acquire, so no byte is touched by both at once.
unsafe impl<const N: usize> Sync for Pipe<N> {}

impl<const N: usize> Default for Pipe<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Pipe<N> {
    pub const fn new() -> Self {
        assert!(N.is_power_of_two() && N >= 4);
        Pipe { buf: UnsafeCell::new([0; N]), written: AtomicUsize::new(0), read: AtomicUsize::new(0), taken: AtomicBool::new(false) }
    }

    /// The two ends. Panics if called twice.
    pub fn split(&'static self) -> (Producer<N>, Consumer<N>) {
        assert!(!self.taken.swap(true, Ordering::AcqRel), "pipe already split");
        (Producer { p: self }, Consumer { p: self })
    }

    fn base(&self) -> *mut u8 {
        self.buf.get().cast()
    }
}

/// The writing end.
pub struct Producer<const N: usize> {
    p: &'static Pipe<N>,
}

impl<const N: usize> Producer<N> {
    /// Bytes that can be written now.
    pub fn space(&self) -> usize {
        let w = self.p.written.load(Ordering::Relaxed);
        let r = self.p.read.load(Ordering::Acquire);
        N - w.wrapping_sub(r)
    }

    /// Write as much of `data` as fits; returns how much.
    pub fn write(&mut self, data: &[u8]) -> usize {
        let n = data.len().min(self.space());
        self.put(&[data], n);
        n
    }

    /// Write all of `data`, or nothing if it does not fit.
    pub fn write_all(&mut self, data: &[u8]) -> bool {
        if data.len() > self.space() {
            return false;
        }
        self.put(&[data], data.len());
        true
    }

    /// Write a frame, or nothing if it does not fit (or is over 65535
    /// bytes).
    pub fn push_frame(&mut self, frame: &[u8]) -> bool {
        let Ok(len) = u16::try_from(frame.len()) else {
            return false;
        };
        if 2 + frame.len() > self.space() {
            return false;
        }
        self.put(&[&len.to_le_bytes(), frame], 2 + frame.len());
        true
    }

    /// Copy `total` bytes from `parts`, then publish them all at once.
    fn put(&mut self, parts: &[&[u8]], total: usize) {
        let mut w = self.p.written.load(Ordering::Relaxed);
        let start = w;
        for part in parts {
            for &b in part.iter() {
                if w.wrapping_sub(start) == total {
                    break;
                }
                // SAFETY: the slot is free space, which only the producer
                // touches until `written` moves past it.
                unsafe { self.p.base().add(w % N).write_volatile(b) };
                w = w.wrapping_add(1);
            }
        }
        self.p.written.store(w, Ordering::Release);
    }
}

/// The reading end.
pub struct Consumer<const N: usize> {
    p: &'static Pipe<N>,
}

impl<const N: usize> Consumer<N> {
    /// Bytes waiting.
    pub fn len(&self) -> usize {
        let w = self.p.written.load(Ordering::Acquire);
        let r = self.p.read.load(Ordering::Relaxed);
        w.wrapping_sub(r)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn byte(&self, i: usize) -> u8 {
        let r = self.p.read.load(Ordering::Relaxed);
        // SAFETY: callers only ask for bytes below `len()`, which the
        // producer has published and will not touch again until `read`
        // moves past them.
        unsafe { self.p.base().add(r.wrapping_add(i) % N).read_volatile() }
    }

    fn consume(&mut self, n: usize) {
        let r = self.p.read.load(Ordering::Relaxed);
        self.p.read.store(r.wrapping_add(n), Ordering::Release);
    }

    /// Read up to `buf.len()` bytes; returns how many.
    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        let n = buf.len().min(self.len());
        for (i, d) in buf[..n].iter_mut().enumerate() {
            *d = self.byte(i);
        }
        self.consume(n);
        n
    }

    /// Take the next frame into `buf`. `Some(Ok(len))` for a frame,
    /// `Some(Err(len))` for one too long for `buf`, which is skipped.
    pub fn pop_frame(&mut self, buf: &mut [u8]) -> Option<Result<usize, usize>> {
        let have = self.len();
        if have < 2 {
            return None;
        }
        let len = u16::from_le_bytes([self.byte(0), self.byte(1)]) as usize;
        // A producer publishes a frame whole, so it is all there.
        debug_assert!(have >= 2 + len);
        if len > buf.len() {
            self.consume(2 + len);
            return Some(Err(len));
        }
        for (i, d) in buf[..len].iter_mut().enumerate() {
            *d = self.byte(2 + i);
        }
        self.consume(2 + len);
        Some(Ok(len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::boxed::Box;
    use std::vec::Vec;

    fn pipe<const N: usize>() -> (Producer<N>, Consumer<N>) {
        Box::leak(Box::new(Pipe::<N>::new())).split()
    }

    #[test]
    fn bytes_wrap_round() {
        let (mut p, mut c) = pipe::<8>();
        assert_eq!(p.space(), 8);
        assert_eq!(p.write(b"abcdef"), 6);
        let mut buf = [0u8; 4];
        assert_eq!(c.read(&mut buf), 4);
        assert_eq!(&buf, b"abcd");
        assert_eq!(p.write(b"ghijklmn"), 6, "only the free space is taken");
        assert_eq!(c.len(), 8);
        assert!(!p.write_all(b"x"));
        let mut buf = [0u8; 16];
        assert_eq!(c.read(&mut buf), 8);
        assert_eq!(&buf[..8], b"efghijkl");
        assert!(c.is_empty());
        assert!(p.write_all(b"12345678"));
    }

    #[test]
    fn frames_go_whole_or_not_at_all() {
        let (mut p, mut c) = pipe::<16>();
        assert!(p.push_frame(b"hello"));
        assert!(p.push_frame(b""));
        assert!(!p.push_frame(b"too big now"), "7 + 2 used, 13 does not fit");
        let mut buf = [0u8; 8];
        assert_eq!(c.pop_frame(&mut buf), Some(Ok(5)));
        assert_eq!(&buf[..5], b"hello");
        assert_eq!(c.pop_frame(&mut buf), Some(Ok(0)));
        assert_eq!(c.pop_frame(&mut buf), None);
        assert!(p.push_frame(b"nine byte"));
        let mut small = [0u8; 4];
        assert_eq!(c.pop_frame(&mut small), Some(Err(9)), "too long for the buffer: skipped");
        assert!(c.is_empty());
    }

    #[test]
    #[should_panic(expected = "already split")]
    fn split_once() {
        let p: &'static Pipe<8> = Box::leak(Box::new(Pipe::new()));
        let _a = p.split();
        let _b = p.split();
    }

    /// Two threads hammering one pipe: every frame arrives, in order and
    /// intact.
    #[test]
    fn threads() {
        let (mut p, mut c) = pipe::<256>();
        const FRAMES: u32 = 20_000;
        let t = std::thread::spawn(move || {
            let mut sent = 0u32;
            while sent < FRAMES {
                let len = (sent % 40) as usize;
                let f: Vec<u8> = (0..len).map(|i| (sent as usize + i) as u8).collect();
                if p.push_frame(&f) {
                    sent += 1;
                } else {
                    std::thread::yield_now();
                }
            }
        });
        let mut got = 0u32;
        let mut buf = [0u8; 64];
        while got < FRAMES {
            match c.pop_frame(&mut buf) {
                Some(Ok(n)) => {
                    assert_eq!(n, (got % 40) as usize);
                    assert!(buf[..n].iter().enumerate().all(|(i, &b)| b == (got as usize + i) as u8));
                    got += 1;
                }
                Some(Err(_)) => panic!("frame too long"),
                None => std::thread::yield_now(),
            }
        }
        t.join().unwrap();
    }
}
