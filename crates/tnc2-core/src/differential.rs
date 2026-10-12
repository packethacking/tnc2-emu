//! `Board::run` against `Board::run_reference` (a full step before every
//! instruction), in lockstep: two boards get the same random host input at
//! the same cycle and the same irregular run lengths, and must agree after
//! every step. Unlike the recorded hashes in tests/exact_timing.rs this
//! keeps guarding the fast path after a deliberate change to the SIO.

use crate::{Board, BoardConfig, NodeConfig, Poke};
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

static ROM1: &[u8] = include_bytes!("../../../rom/THENET1.X1J");
static ROM2: &[u8] = include_bytes!("../../../rom/THENET2.X1J");

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn addr(call: &str, ssid: u8, c_bit: bool, last: bool) -> [u8; 7] {
    let mut a = [b' ' << 1; 7];
    for (d, s) in a.iter_mut().zip(call.bytes()) {
        *d = s << 1;
    }
    a[6] = 0x60 | (ssid << 1) | if c_bit { 0x80 } else { 0 } | last as u8;
    a
}

fn frame(from: (&str, u8), to: (&str, u8), cmd: bool, ctl: u8, rest: &[u8]) -> Vec<u8> {
    let mut f = Vec::new();
    f.extend(addr(to.0, to.1, cmd, false));
    f.extend(addr(from.0, from.1, !cmd, true));
    f.push(ctl);
    f.extend(rest);
    f
}

const NODE: (&str, u8) = ("G9TNT", 1);
const PEERS: [(&str, u8); 3] = [("G9XRT", 0), ("G9AAA", 3), ("G9BBB", 0)];
const CMDS: &[&[u8]] = &[
    b"\x1b", b"C\r", b"NODES\r", b"USERS\r", b"ROUTES\r", b"PARMS\r", b"MHEARD\r", b"INFO\r", b"C G9XRT\r",
    b"hello\r", b"B\r", b"C G9AAA-3\r", b"STATS\r",
];

fn nodes_broadcast(from: (&str, u8), n: usize, r: &mut Rng) -> Vec<u8> {
    let mut info = vec![0xcf, 0xff];
    info.extend(b"XRNODE");
    for i in 0..n {
        let call = format!("G9N{:02}", (r.below(60) as usize + i) % 100);
        info.extend(addr(&call, 0, false, false));
        info.extend(b"NN    ");
        info.extend(addr(from.0, from.1, false, false));
        info.push(100 + i as u8);
    }
    frame(from, ("NODES", 0), true, 0x03, &info)
}

/// What the host can see of a board, plus the SIO channels in full.
fn state(b: &Board) -> alloc::string::String {
    format!(
        "{} {} {} {} {} {} {:?} {:?}",
        b.cycles(),
        b.pc(),
        b.bank(),
        b.ticks_due(),
        b.radio_rx_pending(),
        b.console_rx_pending(),
        b.bus.sio.a,
        b.bus.sio.b
    )
}

/// `seconds` emulated seconds of random traffic; panics where the two
/// boards first differ.
fn lockstep(seed: u64, cfg: BoardConfig, seconds: u64) {
    let pokes = [Poke { offset: 0x4c, bytes: &[0xc0, 0x00] }, Poke { offset: 0x54, bytes: &[0x3c, 0x00] }];
    let node = NodeConfig { call: Some("G9TNT-1"), alias: Some("TNET"), pokes: &pokes };
    let mut a = Board::from_rom(ROM1, ROM2, &node, &cfg).unwrap();
    let mut b = Board::from_rom(ROM1, ROM2, &node, &cfg).unwrap();
    let mut r = Rng(seed.wrapping_mul(0x9e3779b97f4a7c15) | 1);
    let per_ms = cfg.clock_hz / 1000;
    let end = seconds * 1000 * per_ms;
    let (mut vr, mut vs) = ([0u8; 3], [0u8; 3]);
    let mut pending: Vec<(u64, Vec<u8>)> = Vec::new();
    let (mut buf_a, mut buf_b) = ([0u8; 64], [0u8; 64]);
    let (mut dcd, mut tiny_until, mut next_input, mut steps) = (true, 0, 0, 0u64);
    while a.cycles() < end {
        let now = a.cycles();
        let roll = if now >= next_input {
            next_input = now + r.below(600) * per_ms;
            r.below(20)
        } else {
            1000
        };
        if roll < 6 {
            let c = CMDS[r.below(CMDS.len() as u64) as usize];
            assert_eq!(a.push_console(c), b.push_console(c));
        } else if roll < 8 {
            let v: Vec<u8> = (0..1 + r.below(300)).map(|_| r.next() as u8).collect();
            assert_eq!(a.push_console(&v), b.push_console(&v));
        } else if roll < 14 {
            let i = r.below(3) as usize;
            let p = PEERS[i];
            let f = match r.below(8) {
                0 => nodes_broadcast(p, r.below(12) as usize, &mut r),
                1 => frame(p, NODE, true, 0x3f, &[]),
                2 => frame(p, NODE, true, 0x53, &[]),
                3 => {
                    let ctl = (vr[i] << 5) | (vs[i] << 1);
                    vs[i] = (vs[i] + 1) % 8;
                    let mut info = vec![0xf0];
                    info.extend(CMDS[r.below(CMDS.len() as u64) as usize]);
                    frame(p, NODE, true, ctl, &info)
                }
                4 => frame(p, ("CQ", 0), true, 0x03, b"\xf0hello all\r"),
                5 => (0..1 + r.below(330)).map(|_| r.next() as u8).collect(),
                6 => frame(p, ("G9ZZZ", 0), true, 0x3f, &[]),
                _ => frame(p, NODE, true, 0x11 | (vr[i] << 5), &[]),
            };
            assert_eq!(a.push_radio_frame(&f), b.push_radio_frame(&f));
        } else if roll < 15 {
            dcd = !dcd;
            a.set_rs232_dcd(dcd);
            b.set_rs232_dcd(dcd);
        } else if roll < 17 && !dcd {
            // a KISS frame on the RS232 port
            let f = frame(NODE, PEERS[r.below(3) as usize], true, 0x03, b"\xf0kiss\r");
            let mut k = vec![0xc0, 0x00];
            for &x in &f {
                match x {
                    0xc0 => k.extend([0xdb, 0xdc]),
                    0xdb => k.extend([0xdb, 0xdd]),
                    _ => k.push(x),
                }
            }
            k.push(0xc0);
            assert_eq!(a.push_console(&k), b.push_console(&k));
        } else if roll < 20 {
            // a stretch of very short runs
            tiny_until = now + r.below(200_000);
        }
        let (due, later): (Vec<_>, Vec<_>) = pending.drain(..).partition(|(t, _)| now >= *t);
        pending = later;
        for (_, f) in due {
            assert_eq!(a.push_radio_frame(&f), b.push_radio_frame(&f));
        }

        let x = r.next();
        let step = if now < tiny_until {
            1 + x % 30
        } else {
            match x % 20 {
                0..=2 => 1,
                3 => 2 + (x >> 8) % 40,
                4 => 40_000 + (x >> 8) % 300_000,
                _ => 1 + (x >> 8) % 8000,
            }
        };
        a.run_reference(step);
        b.run(step);
        steps += 1;
        let at = a.cycles();
        assert_eq!(a.cycles(), b.cycles(), "seed {seed} step {steps} at {at}: cycles");
        assert_eq!(a.pc(), b.pc(), "seed {seed} step {steps} at {at}: PC");
        loop {
            let (na, nb) = (a.read_console(&mut buf_a), b.read_console(&mut buf_b));
            assert_eq!(buf_a[..na], buf_b[..nb], "seed {seed} step {steps} at {at}: console");
            if na == 0 {
                break;
            }
        }
        loop {
            let (fa, fb) = (a.pop_radio_frame(), b.pop_radio_frame());
            assert_eq!(fa, fb, "seed {seed} step {steps} at {at}: radio frame");
            let Some(f) = fa else { break };
            // the peers ack and answer, at once or a little later
            for (i, p) in PEERS.iter().enumerate() {
                if f.len() < 15 || f[..6] != addr(p.0, p.1, false, false)[..6] || f[6] & 0x1e != p.1 << 1 {
                    continue;
                }
                let (ctl, at) = (f[14], a.cycles() + r.below(4) * 30 * per_ms);
                let pf = ctl & 0x10;
                if ctl & 1 == 0 {
                    if (ctl >> 1) & 7 == vr[i] {
                        vr[i] = (vr[i] + 1) % 8;
                    }
                    pending.push((at, frame(*p, NODE, false, 0x01 | (vr[i] << 5) | pf, &[])));
                } else if ctl & !0x10 == 0x2f {
                    (vr[i], vs[i]) = (0, 0);
                    pending.push((at, frame(*p, NODE, false, 0x63 | pf, &[])));
                } else if ctl & !0x10 == 0x43 {
                    pending.push((at, frame(*p, NODE, false, 0x63 | pf, &[])));
                } else if ctl & 3 == 1 && f[6] & 0x80 != 0 && pf != 0 {
                    pending.push((at, frame(*p, NODE, false, 0x11 | (vr[i] << 5), &[])));
                }
            }
        }
        loop {
            let (ea, eb) = (a.pop_event(), b.pop_event());
            assert_eq!(ea, eb, "seed {seed} step {steps} at {at}: event");
            if ea.is_none() {
                break;
            }
        }
        if steps % 64 == 0 {
            assert_eq!(state(&a), state(&b), "seed {seed} step {steps} at {at}: state");
            assert!(a.ram()[..] == b.ram()[..], "seed {seed} step {steps} at {at}: RAM");
        }
    }
    assert_eq!(state(&a), state(&b), "seed {seed}: final state");
    assert!(a.ram()[..] == b.ram()[..], "seed {seed}: final RAM");
}

#[test]
fn run_matches_the_reference() {
    let d = BoardConfig::default();
    let cfgs = [
        d,
        BoardConfig { hdlc_baud: 4800, async_baud: 115200, ..d },
        BoardConfig { hdlc_baud: 300, async_baud: 38400, ..d },
        BoardConfig { clock_hz: 2_457_600, hdlc_baud: 9600, async_baud: 19200, ..d },
    ];
    for (seed, cfg) in cfgs.into_iter().enumerate() {
        lockstep(seed as u64 + 1, cfg, 20);
    }
}
