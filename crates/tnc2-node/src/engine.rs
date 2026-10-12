//! Core 0's work, one slice at a time: run the board for 1 ms of emulated
//! time once real time has caught up, then move frames and console bytes,
//! copy the RAM out if asked, and change boards when told to. Nothing here
//! waits for core 1.
//!
//! The firmware calls `Engine::step` in a loop on core 0; the tests here
//! drive it the same way, with a pretend clock.

use crate::handoff::{BoardSlot, Order, Serve};
use crate::pace::{Counters, Pacer};
use crate::pipe::{Consumer, Producer};
use alloc::boxed::Box;
use core::sync::atomic::Ordering::Relaxed;
use tnc2_core::{Board, CONSOLE_RX_QUEUE, RADIO_RX_MAX_FRAME, rom};

/// Core 0's ends of the pipes and the snapshot.
pub struct Io<const R: usize, const C: usize> {
    pub radio_in: Consumer<R>,
    pub radio_out: Producer<R>,
    pub console_in: Consumer<C>,
    pub console_out: Producer<C>,
    pub snapshot: Serve,
}

pub struct Engine {
    board: Option<Box<Board>>,
    pacer: Pacer,
    /// Emulated time on boards before this one, and busy time in all.
    emulated_before: u64,
    busy: u64,
    frame: [u8; RADIO_RX_MAX_FRAME],
    buf: [u8; 256],
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub const fn new() -> Self {
        Engine {
            board: None,
            pacer: Pacer::new(1, 0, 0),
            emulated_before: 0,
            busy: 0,
            frame: [0; RADIO_RX_MAX_FRAME],
            buf: [0; 256],
        }
    }

    /// One turn of core 0's loop. `now` is real time in microseconds.
    pub fn step<const R: usize, const C: usize>(
        &mut self,
        io: &mut Io<R, C>,
        slot: &BoardSlot<Board>,
        c: &Counters,
        now: impl Fn() -> u64,
    ) {
        Counters::add(&c.slices, 1);
        match slot.poll() {
            Order::Carry => {}
            Order::Stop => {
                if let Some(b) = self.board.take() {
                    self.emulated_before += self.pacer.emulated_us(b.cycles());
                }
                slot.stopped();
            }
            Order::Run(b) => {
                self.pacer = Pacer::new(b.clock_hz(), now(), b.cycles());
                self.board = Some(b);
            }
        }
        let Some(b) = self.board.as_deref_mut() else {
            // No board yet, or between boards: frames have nowhere to go.
            while io.radio_in.pop_frame(&mut self.frame).is_some() {
                Counters::add(&c.frames_no_board, 1);
            }
            return;
        };

        let slice = b.clock_hz() / 1000;
        let due = self.pacer.due_us(b.cycles() + slice);
        let mut start = now();
        while start < due {
            start = now();
        }
        b.run(slice);
        exchange(b, io, c, &mut self.frame, &mut self.buf);
        io.snapshot.serve(|img| {
            img[..16].copy_from_slice(&rom::ram_image_header(b.fingerprint()));
            img[16..].copy_from_slice(b.ram());
        });
        let end = now();
        self.busy += end - start;
        c.busy_us.store(self.busy as u32, Relaxed);
        c.emulated_us.store((self.emulated_before + self.pacer.emulated_us(b.cycles())) as u32, Relaxed);
        c.behind_ms.store((self.pacer.behind_us(end, b.cycles()) / 1000) as u32, Relaxed);
    }
}

/// Frames and console bytes, both ways.
fn exchange<const R: usize, const C: usize>(
    b: &mut Board,
    io: &mut Io<R, C>,
    c: &Counters,
    frame: &mut [u8; RADIO_RX_MAX_FRAME],
    buf: &mut [u8; 256],
) {
    while let Some(r) = io.radio_in.pop_frame(frame) {
        match r {
            Ok(n) if b.push_radio_frame(&frame[..n]) => Counters::add(&c.frames_in, 1),
            // TheNet's receive queue is full (or the frame is too long for
            // it): dropped, as a busy TNC would.
            _ => Counters::add(&c.frames_refused, 1),
        }
    }
    while let Some(f) = b.pop_radio_frame() {
        if io.radio_out.push_frame(&f) {
            Counters::add(&c.frames_out, 1);
        } else {
            Counters::add(&c.frames_out_dropped, 1);
        }
    }
    // Console output is always taken, so it cannot pile up in the board;
    // what does not fit is lost, like characters sent to an RS232 port
    // nobody is reading.
    loop {
        let n = b.read_console(buf);
        if n == 0 {
            break;
        }
        let taken = io.console_out.write(&buf[..n]);
        if taken < n {
            Counters::add(&c.console_dropped, (n - taken) as u32);
        }
    }
    // Console input only as fast as the board takes it; the rest waits in
    // the pipe, and behind it in USB.
    let room = CONSOLE_RX_QUEUE.saturating_sub(b.console_rx_pending()).min(buf.len());
    let n = io.console_in.read(&mut buf[..room]);
    if n > 0 {
        let taken = b.push_console(&buf[..n]);
        debug_assert_eq!(taken, n);
    }
    // Notes for an operator; the counters above say the same.
    while b.pop_event().is_some() {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handoff::Snapshot;
    use crate::pipe::Pipe;
    use core::cell::Cell;
    use std::boxed::Box;
    use std::string::String;
    use std::vec::Vec;
    use tnc2_core::{BoardConfig, NodeConfig, Poke, ax25};

    fn board(call: &str, pokes: &[Poke]) -> Box<Board> {
        let node = NodeConfig { call: Some(call), alias: Some("TEST"), pokes };
        Box::new(
            Board::from_rom(
                include_bytes!("../../../rom/THENET1.X1J"),
                include_bytes!("../../../rom/THENET2.X1J"),
                &node,
                &BoardConfig::default(),
            )
            .unwrap(),
        )
    }

    fn leak<T>(t: T) -> &'static T {
        Box::leak(Box::new(t))
    }

    /// The node firmware's core 0, driven here on one thread: core 1's
    /// side is the test itself.
    struct Rig {
        engine: Engine,
        io: Io<8192, 4096>,
        slot: &'static BoardSlot<Board>,
        c: &'static Counters,
        clock: Cell<u64>,
        radio_in: Producer<8192>,
        radio_out: Consumer<8192>,
        console_in: Producer<4096>,
        console_out: Consumer<4096>,
        saver: crate::handoff::Saver,
    }

    impl Rig {
        fn new() -> Rig {
            let (radio_in, core0_radio_in) = leak(Pipe::new()).split();
            let (core0_radio_out, radio_out) = leak(Pipe::new()).split();
            let (console_in, core0_console_in) = leak(Pipe::new()).split();
            let (core0_console_out, console_out) = leak(Pipe::new()).split();
            let (saver, serve) = leak(Snapshot::new()).split();
            Rig {
                engine: Engine::new(),
                io: Io {
                    radio_in: core0_radio_in,
                    radio_out: core0_radio_out,
                    console_in: core0_console_in,
                    console_out: core0_console_out,
                    snapshot: serve,
                },
                slot: leak(BoardSlot::new()),
                c: leak(Counters::new()),
                clock: Cell::new(1_000_000),
                radio_in,
                radio_out,
                console_in,
                console_out,
                saver,
            }
        }

        /// Run `ms` slices. Real time moves on 1 ms each time it is read
        /// while waiting, so the engine always waits for its slice, as on
        /// a Pico that is fast enough.
        fn run(&mut self, ms: u32) {
            for _ in 0..ms {
                let clock = &self.clock;
                self.engine.step(&mut self.io, self.slot, self.c, || {
                    clock.set(clock.get() + 250);
                    clock.get()
                });
            }
        }

        fn console(&mut self) -> String {
            let mut out = String::new();
            let mut buf = [0u8; 256];
            loop {
                let n = self.console_out.read(&mut buf);
                if n == 0 {
                    return out;
                }
                out.extend(buf[..n].iter().map(|&b| b as char));
            }
        }

        fn run_until(&mut self, want: &str, ms: u32) -> String {
            let mut out = String::new();
            for _ in 0..ms / 10 {
                self.run(10);
                out += &self.console();
                if out.contains(want) {
                    return out;
                }
            }
            panic!("did not see {want:?}; got {out:?}");
        }
    }

    #[test]
    fn a_node_on_core_0() {
        let mut r = Rig::new();
        // no board yet: core 0 just turns over, and radio frames are lost
        assert!(r.radio_in.push_frame(b"early"));
        r.run(5);
        assert_eq!(r.c.frames_no_board.load(Relaxed), 1);
        assert_eq!(r.c.slices.load(Relaxed), 5);

        // the NODES interval poke brings the first broadcast forward
        let pokes = [Poke { offset: 0x54, bytes: &[0x0A, 0x00] }];
        r.slot.start(board("N0CALL-7", &pokes)).ok().unwrap();
        r.run(500);
        assert!(r.console_in.write_all(b"\x1b"));
        r.run_until("* ", 5_000);
        assert!(r.console_in.write_all(b"C\r"));
        r.run_until("CONNECTED to N0CALL-7", 5_000);

        // emulated time tracks the pretend real time
        let emu = r.c.emulated_us.load(Relaxed) as u64;
        let real = r.clock.get() - 1_000_000;
        assert!(real - emu < 2_000, "emulated {emu} us, real {real} us");
        assert_eq!(r.c.behind_ms.load(Relaxed), 0);

        // a frame from the radio reaches TheNet, and its NODES broadcast
        // comes out
        assert!(r.radio_in.push_frame(&[0x96, 0x70, 0x9A, 0x9A, 0x9E, 0x40, 0xE0, 0x96, 0x70, 0x9A, 0x9A, 0x9E, 0x40, 0x61, 0x03, 0xF0, b'x']));
        let mut frames = Vec::new();
        let mut f = [0u8; 600];
        for _ in 0..20_000 {
            r.run(1);
            if let Some(Ok(n)) = r.radio_out.pop_frame(&mut f) {
                frames.push(f[..n].to_vec());
                break;
            }
        }
        assert_eq!(r.c.frames_in.load(Relaxed), 1);
        assert_eq!(r.c.frames_out.load(Relaxed), 1);
        assert_eq!(ax25::describe(&frames[0]), "N0CALL-7>NODES <UI C> pid=CF len=7 NODES from alias 'TEST'");

        // a snapshot is the board's RAM image, fingerprint and all
        assert!(r.saver.request());
        r.run(1);
        let fp = board("N0CALL-7", &pokes).fingerprint();
        let img = r.saver.ready().expect("served").to_vec();
        r.saver.release();
        assert!(rom::check_ram_image(&img, fp).is_ok());

        // a MYCALL change: core 0 drops its board when asked, then runs the
        // new one from cold
        r.slot.stop();
        r.run(1);
        assert!(r.slot.is_stopped());
        let before = r.c.emulated_us.load(Relaxed);
        r.slot.start(board("G9TNT-1", &[])).ok().unwrap();
        r.run(500);
        assert!(r.console_in.write_all(b"\x1bC\r"));
        r.run_until("CONNECTED to G9TNT-1", 5_000);
        assert!(r.c.emulated_us.load(Relaxed) > before, "emulated time carries on across boards");
    }

    #[test]
    fn console_output_nobody_reads_is_dropped_not_hoarded() {
        let mut r = Rig::new();
        r.slot.start(board("N0CALL", &[])).ok().unwrap();
        r.run(500);
        // ask for something long, and never read it: the pipe fills and
        // the rest is counted as dropped
        for _ in 0..40 {
            assert!(r.console_in.write_all(b"\x1bC\r?\r"));
            r.run(2_000);
        }
        assert_eq!(r.console_out.len(), 4096);
        assert!(r.c.console_dropped.load(Relaxed) > 0);
    }

    #[test]
    fn behind_runs_flat_out() {
        let mut r = Rig::new();
        r.slot.start(board("N0CALL", &[])).ok().unwrap();
        r.run(1);
        // real time leaps ahead by a second at once: no waiting until
        // emulated time has caught up
        let clock = Cell::new(r.clock.get() + 1_000_000);
        for _ in 0..10 {
            r.engine.step(&mut r.io, r.slot, r.c, || {
                clock.set(clock.get() + 1);
                clock.get()
            });
        }
        let behind = r.c.behind_ms.load(Relaxed);
        assert!((985..=995).contains(&behind), "behind {behind} ms");
    }
}
