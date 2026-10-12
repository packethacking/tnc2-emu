//! What the two cores pass each other besides frames and console bytes:
//! the board itself, and snapshots of its RAM for saving.
//!
//! Core 0 owns the board and only looks at these between 1 ms slices, so
//! everything here is a flag it checks and, at most, a 32 KB copy.

use alloc::boxed::Box;
use core::cell::UnsafeCell;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, Ordering};
use tnc2_core::rom::RAM_IMAGE_LEN;

/// A RAM image (`tnc2_core::rom::ram_image_header` and the RAM).
pub type Image = [u8; RAM_IMAGE_LEN];

const IDLE: u8 = 0;
const WANTED: u8 = 1;
const READY: u8 = 2;

/// One RAM image buffer, handed between the cores:
///
/// - idle: core 1's, to read saved RAM into at start up or leave alone;
/// - wanted: core 1 has asked, and it is core 0's until it has copied the
///   board's RAM in;
/// - ready: core 1's again, to save.
pub struct Snapshot {
    state: AtomicU8,
    taken: AtomicBool,
    buf: UnsafeCell<Image>,
}

// SAFETY: the state says which core may touch the buffer, it moves with
// release and is checked with acquire, and each end is held by one core.
unsafe impl Sync for Snapshot {}

impl Default for Snapshot {
    fn default() -> Self {
        Self::new()
    }
}

impl Snapshot {
    pub const fn new() -> Self {
        Snapshot { state: AtomicU8::new(IDLE), taken: AtomicBool::new(false), buf: UnsafeCell::new([0; RAM_IMAGE_LEN]) }
    }

    /// The two ends: (core 1's, core 0's). Panics if called twice.
    pub fn split(&'static self) -> (Saver, Serve) {
        assert!(!self.taken.swap(true, Ordering::AcqRel), "snapshot already split");
        (Saver { s: self }, Serve { s: self })
    }
}

/// Core 1's end.
pub struct Saver {
    s: &'static Snapshot,
}

impl Saver {
    /// The buffer, while nobody has asked core 0 for anything.
    pub fn buffer(&mut self) -> Option<&mut Image> {
        // SAFETY: idle, so core 0 will not touch it until `request`, and
        // that needs `&mut self`, which this borrow holds.
        (self.s.state.load(Ordering::Acquire) == IDLE).then(|| unsafe { &mut *self.s.buf.get() })
    }

    /// Ask core 0 for a snapshot. False if one is already asked for or
    /// waiting.
    pub fn request(&mut self) -> bool {
        self.s.state.compare_exchange(IDLE, WANTED, Ordering::AcqRel, Ordering::Acquire).is_ok()
    }

    /// The snapshot, once core 0 has taken it.
    pub fn ready(&mut self) -> Option<&Image> {
        // SAFETY: ready, so core 0 is done with it until the next request.
        (self.s.state.load(Ordering::Acquire) == READY).then(|| unsafe { &*self.s.buf.get() })
    }

    /// Done with the snapshot, or no longer want one: back to idle.
    pub fn release(&mut self) {
        // If core 0 is copying right now (it saw `wanted`), wait for it.
        loop {
            match self.s.state.compare_exchange(WANTED, IDLE, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) | Err(IDLE) => return,
                Err(READY) => {
                    self.s.state.store(IDLE, Ordering::Release);
                    return;
                }
                Err(_) => core::hint::spin_loop(),
            }
        }
    }
}

/// Core 0's end.
pub struct Serve {
    s: &'static Snapshot,
}

/// Core 0 is copying: core 1 must not take the buffer back meanwhile.
const COPYING: u8 = 3;

impl Serve {
    /// If core 1 wants a snapshot, let `fill` write it, and hand it over.
    pub fn serve(&mut self, fill: impl FnOnce(&mut Image)) {
        if self.s.state.load(Ordering::Relaxed) != WANTED {
            return;
        }
        if self.s.state.compare_exchange(WANTED, COPYING, Ordering::AcqRel, Ordering::Relaxed).is_err() {
            return;
        }
        // SAFETY: copying, which only this end leaves.
        fill(unsafe { &mut *self.s.buf.get() });
        self.s.state.store(READY, Ordering::Release);
    }
}

const RUN: u8 = 0;
const STOP: u8 = 1;
const STOPPED: u8 = 2;
const NEW: u8 = 3;

/// The board, passed from core 1 (which builds it, reading the EPROM
/// images and the saved RAM from flash) to core 0 (which runs it). To
/// change boards, core 1 asks core 0 to stop and drop the one it has
/// first, so there is never more than one on the heap.
pub struct BoardSlot<T> {
    state: AtomicU8,
    new: AtomicPtr<T>,
}

impl<T> Default for BoardSlot<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// What core 0 should do, between slices.
pub enum Order<T> {
    Carry,
    /// Drop the board and wait.
    Stop,
    /// Run this one.
    Run(Box<T>),
}

impl<T> BoardSlot<T> {
    /// Starts with no board, as if just stopped.
    pub const fn new() -> Self {
        BoardSlot { state: AtomicU8::new(STOPPED), new: AtomicPtr::new(ptr::null_mut()) }
    }

    /// Core 1: ask core 0 to stop.
    pub fn stop(&self) {
        let _ = self.state.compare_exchange(RUN, STOP, Ordering::AcqRel, Ordering::Acquire);
    }

    /// Core 1: whether core 0 has dropped its board.
    pub fn is_stopped(&self) -> bool {
        self.state.load(Ordering::Acquire) == STOPPED
    }

    /// Core 1: hand over a board, once stopped. Gives it back otherwise.
    pub fn start(&self, board: Box<T>) -> Result<(), Box<T>> {
        if !self.is_stopped() {
            return Err(board);
        }
        self.new.store(Box::into_raw(board), Ordering::Relaxed);
        self.state.store(NEW, Ordering::Release);
        Ok(())
    }

    /// Core 0: check between slices.
    pub fn poll(&self) -> Order<T> {
        match self.state.load(Ordering::Acquire) {
            STOP => Order::Stop,
            NEW => {
                let p = self.new.swap(ptr::null_mut(), Ordering::Relaxed);
                self.state.store(RUN, Ordering::Release);
                // SAFETY: `start` put it there with `Box::into_raw` and
                // nothing else takes it.
                Order::Run(unsafe { Box::from_raw(p) })
            }
            _ => Order::Carry,
        }
    }

    /// Core 0: the board is dropped.
    pub fn stopped(&self) {
        self.state.store(STOPPED, Ordering::Release);
    }
}

// SAFETY: the board moves from one core to the other through `new`, owned
// by one side at a time as the state says.
unsafe impl<T: Send> Sync for BoardSlot<T> {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::boxed::Box;

    #[test]
    fn snapshot_round_trip() {
        let s: &'static Snapshot = Box::leak(Box::new(Snapshot::new()));
        let (mut saver, mut core0) = s.split();
        assert!(saver.buffer().is_some());
        assert!(saver.ready().is_none());
        // core 0 does nothing until asked
        core0.serve(|_| panic!("not asked"));
        assert!(saver.request());
        assert!(!saver.request(), "one at a time");
        assert!(saver.buffer().is_none(), "core 0's while wanted");
        core0.serve(|b| b[100] = 7);
        assert_eq!(saver.ready().unwrap()[100], 7);
        core0.serve(|_| panic!("already served"));
        saver.release();
        assert!(saver.buffer().is_some());
        // asked for and then not wanted after all
        assert!(saver.request());
        saver.release();
        core0.serve(|_| panic!("withdrawn"));
        assert!(saver.request());
    }

    #[test]
    fn snapshot_across_threads() {
        let s: &'static Snapshot = Box::leak(Box::new(Snapshot::new()));
        let (mut saver, mut core0) = s.split();
        let done: &'static AtomicBool = Box::leak(Box::new(AtomicBool::new(false)));
        let t = std::thread::spawn(move || {
            let mut n = 0u8;
            while !done.load(Ordering::Relaxed) {
                core0.serve(|b| {
                    n = n.wrapping_add(1);
                    b.fill(n);
                });
            }
        });
        for _ in 0..200 {
            assert!(saver.request());
            let img = loop {
                if let Some(i) = saver.ready() {
                    break i;
                }
                std::thread::yield_now();
            };
            // a whole copy, never half of one
            assert!(img.iter().all(|&b| b == img[0]));
            saver.release();
        }
        // withdrawing while core 0 may be copying is safe too
        for _ in 0..200 {
            saver.request();
            saver.release();
            assert!(saver.buffer().is_some());
        }
        done.store(true, Ordering::Relaxed);
        t.join().unwrap();
    }

    #[test]
    fn board_changes() {
        let slot: BoardSlot<u32> = BoardSlot::new();
        assert!(slot.is_stopped());
        assert!(matches!(slot.poll(), Order::Carry));
        slot.start(Box::new(1)).unwrap();
        assert!(!slot.is_stopped());
        let Order::Run(b) = slot.poll() else { panic!() };
        assert_eq!(*b, 1);
        assert!(matches!(slot.poll(), Order::Carry));
        assert_eq!(*slot.start(Box::new(2)).unwrap_err(), 2, "still running one");
        slot.stop();
        assert!(matches!(slot.poll(), Order::Stop));
        assert!(!slot.is_stopped());
        drop(b);
        slot.stopped();
        assert!(slot.is_stopped());
        slot.start(Box::new(3)).unwrap();
        let Order::Run(b) = slot.poll() else { panic!() };
        assert_eq!(*b, 3);
    }
}
