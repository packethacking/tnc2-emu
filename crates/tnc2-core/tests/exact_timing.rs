//! Two emulated minutes of a busy node, to show a change to the core keeps
//! the emulation exactly, cycle for cycle, what it was.
//!
//! The script types at the console, and a neighbour on the radio sends a
//! NODES broadcast, connects, sends commands, acks what it gets, answers
//! the node's own connect, disconnects, and later the RS232 port goes to
//! KISS mode and back. The board runs in steps of irregular length, from
//! one instruction to tens of ms. The test hashes the cycle count, PC,
//! bank and pending ticks after every step, every console byte and radio
//! frame with the cycle count it came out by, and the final RAM, and
//! compares them with values recorded from the core before it was made
//! faster. Any change in when an instruction runs or an interrupt is
//! taken changes them.
//!
//! It runs twice: at the TNC-2's own 1200 bit/s radio and 9600 bit/s
//! RS232, and with a fast RS232 port (115200 bit/s, a character every 427
//! cycles, so well inside one 600 Hz tick) and a 4800 bit/s radio.
//!
//! If a deliberate change to the emulation moves them, check the
//! transcript (cargo test --release --test exact_timing -- --nocapture)
//! and record the new values.

use tnc2_core::{Board, BoardConfig, NodeConfig, Poke, ax25};

static ROM1: &[u8] = include_bytes!("../../../rom/THENET1.X1J");
static ROM2: &[u8] = include_bytes!("../../../rom/THENET2.X1J");

/// (trace, RAM) at the standard and the fast bit rates.
const STANDARD: (u64, u64) = (0x89797e69acd50f5d, 0xf16dae6ebb7b7542);
const FAST: (u64, u64) = (0x64abac3eae50cd19, 0xa135c0962044b233);

struct Fnv(u64);

impl Fnv {
    fn add(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }
    fn add_u64(&mut self, v: u64) {
        self.add(&v.to_le_bytes());
    }
}

/// An AX.25 address field entry.
fn addr(call: &str, ssid: u8, c_bit: bool, last: bool) -> [u8; 7] {
    let mut a = [b' ' << 1; 7];
    for (d, s) in a.iter_mut().zip(call.bytes()) {
        *d = s << 1;
    }
    a[6] = 0x60 | (ssid << 1) | if c_bit { 0x80 } else { 0 } | last as u8;
    a
}

const NODE: (&str, u8) = ("G9TNT", 1);
const PEER: (&str, u8) = ("G9XRT", 0);

/// A frame between the peer and the node, `cmd` for a command.
fn frame(from: (&str, u8), to: (&str, u8), cmd: bool, ctl: u8, rest: &[u8]) -> Vec<u8> {
    let mut f = Vec::new();
    f.extend(addr(to.0, to.1, cmd, false));
    f.extend(addr(from.0, from.1, !cmd, true));
    f.push(ctl);
    f.extend(rest);
    f
}

fn nodes_broadcast(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut info = vec![0xcf, 0xff];
    info.extend(b"XRNODE");
    for (call, alias) in entries {
        info.extend(addr(call, 0, false, false));
        let mut a = [b' '; 6];
        a[..alias.len()].copy_from_slice(alias.as_bytes());
        info.extend(a);
        info.extend(addr(PEER.0, PEER.1, false, false));
        info.push(200);
    }
    frame(PEER, ("NODES", 0), true, 0x03, &info)
}

/// The neighbour: sends what the script says and keeps an AX.25 link with
/// the node going (acks I frames, answers polls, SABM and DISC).
struct Peer {
    /// Next I frame expected from the node, and our own send count.
    vr: u8,
    vs: u8,
    /// Frames to send at a cycle count.
    out: Vec<(u64, Vec<u8>)>,
}

impl Peer {
    fn send_i(&mut self, at: u64, text: &[u8]) {
        let mut info = vec![0xf0];
        info.extend(text);
        let ctl = (self.vr << 5) | (self.vs << 1);
        self.vs = (self.vs + 1) % 8;
        self.out.push((at, frame(PEER, NODE, true, ctl, &info)));
    }

    /// React to a frame the node sent, `reply_at` being when to answer.
    fn heard(&mut self, f: &[u8], reply_at: u64) {
        let is = |a: &[u8], (call, ssid): (&str, u8)| {
            a[..6] == addr(call, ssid, false, false)[..6] && a[6] & 0x1e == ssid << 1
        };
        if f.len() < 15 || !is(&f[..7], PEER) || !is(&f[7..14], NODE) {
            return;
        }
        let cmd = f[6] & 0x80 != 0;
        let ctl = f[14];
        let pf = ctl & 0x10;
        let reply = |ctl: u8| frame(PEER, NODE, false, ctl, &[]);
        if ctl & 1 == 0 {
            if (ctl >> 1) & 7 == self.vr {
                self.vr = (self.vr + 1) % 8;
            }
            self.out.push((reply_at, reply(0x01 | (self.vr << 5) | pf)));
        } else if ctl & 3 == 1 {
            if cmd && pf != 0 {
                self.out.push((reply_at, reply(0x01 | (self.vr << 5) | 0x10)));
            }
        } else if ctl & !0x10 == 0x2f {
            // SABM: the node connects to us
            self.vr = 0;
            self.vs = 0;
            self.out.push((reply_at, reply(0x63 | pf)));
        } else if ctl & !0x10 == 0x43 {
            self.out.push((reply_at, reply(0x63 | pf)));
        }
    }
}

enum Step {
    Console(&'static [u8]),
    Radio(Vec<u8>),
    PeerI(&'static [u8]),
    PeerSabm,
    PeerDisc,
    Dcd(bool),
}

fn script() -> Vec<(u64, Step)> {
    use Step::*;
    let mut kiss = vec![0xc0, 0x00];
    kiss.extend(nodes_broadcast(&[("G9BBB", "BBB")]));
    kiss.push(0xc0);
    let kiss: &'static [u8] = Box::leak(kiss.into_boxed_slice());
    vec![
        (1000, Console(b"\x1b")),
        (1500, Console(b"C\r")),
        (2000, Radio(nodes_broadcast(&[]))),
        (3000, PeerSabm),
        (5000, PeerI(b"NODES\r")),
        (8000, Console(b"NODES\r")),
        (10000, PeerI(b"USERS\r")),
        (12000, Console(b"USERS\r")),
        (15000, Radio(nodes_broadcast(&[("G9AAA", "AAA"), ("G9CCC", "CCC")]))),
        (20000, Console(b"NODES\r")),
        (22000, Console(b"ROUTES\r")),
        (25000, PeerI(b"INFO\r")),
        (30000, PeerI(b"NODES AAA\r")),
        (35000, PeerDisc),
        (40000, Console(b"C G9XRT\r")),
        (45000, Console(b"hello there\r")),
        (50000, PeerI(b"hi back\r")),
        (55000, PeerDisc),
        (60000, Console(b"PARMS\r")),
        (70000, Radio(frame(PEER, ("CQ", 0), true, 0x03, b"\xf0anyone about?\r"))),
        (70000, Radio(frame(PEER, ("G9ZZZ", 0), true, 0x3f, &[]))),
        (70000, Radio(nodes_broadcast(&[("G9AAA", "AAA")]))),
        (80000, Dcd(false)),
        (85000, Console(kiss)),
        (95000, Dcd(true)),
        (96000, Console(b"\x1b")),
        (96500, Console(b"C\r")),
        (100000, Console(b"NODES\r")),
        (105000, Console(b"MHEARD\r")),
        (110000, Radio(frame(PEER, ("ID", 0), true, 0x03, b"\xf0G9XRT\r"))),
        (110000, Radio(frame(PEER, ("CQ", 0), true, 0x03, b"\xf0still here\r"))),
        (110000, Radio(frame(("G9YYY", 0), ("CQ", 0), true, 0x03, b"\xf0and me\r"))),
        (110000, Radio(frame(("G9YYY", 0), PEER, true, 0x3f, &[]))),
        (110000, Radio(frame(PEER, ("G9YYY", 0), false, 0x73, &[]))),
    ]
}

struct Session {
    trace: u64,
    ram: u64,
    console: String,
    frames: Vec<(u64, String)>,
}

fn run_session(cfg: &BoardConfig) -> Session {
    let pokes = [Poke { offset: 0x4c, bytes: &[0xc0, 0x00] }, Poke { offset: 0x54, bytes: &[0x3c, 0x00] }];
    let node = NodeConfig { call: Some("G9TNT-1"), alias: Some("TNET"), pokes: &pokes };
    let mut b = Board::from_rom(ROM1, ROM2, &node, cfg).unwrap();
    let per_ms = b.clock_hz() / 1000;
    let end = 120_000 * per_ms;
    let mut script = script().into_iter().map(|(ms, s)| (ms * per_ms, s)).collect::<Vec<_>>().into_iter().peekable();
    let mut peer = Peer { vr: 0, vs: 0, out: Vec::new() };
    let mut h = Fnv(0xcbf29ce484222325);
    let mut console = String::new();
    let mut frames = Vec::new();
    let mut rng: u64 = 0x2545f4914f6cdd1d;
    let mut buf = [0u8; 256];
    while b.cycles() < end {
        while script.peek().is_some_and(|(t, _)| b.cycles() >= *t) {
            let now = b.cycles();
            match script.next().unwrap().1 {
                Step::Console(s) => assert_eq!(b.push_console(s), s.len()),
                Step::Radio(f) => assert!(b.push_radio_frame(&f)),
                Step::PeerI(t) => peer.send_i(now, t),
                Step::PeerSabm => peer.out.push((now, frame(PEER, NODE, true, 0x3f, &[]))),
                Step::PeerDisc => peer.out.push((now, frame(PEER, NODE, true, 0x53, &[]))),
                Step::Dcd(on) => b.set_rs232_dcd(on),
            }
        }
        let now = b.cycles();
        let (due, later): (Vec<_>, Vec<_>) = peer.out.drain(..).partition(|(t, _)| now >= *t);
        peer.out = later;
        for (_, f) in due {
            assert!(b.push_radio_frame(&f));
        }

        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let r = rng >> 33;
        let step = match r % 16 {
            0 => 1,
            1 => 40_000 + r % 100_000,
            _ => 1 + r % 8000,
        };
        b.run(step);
        h.add_u64(b.cycles());
        h.add(&b.pc().to_le_bytes());
        h.add(&[b.bank() as u8]);
        h.add_u64(b.ticks_due());

        loop {
            let n = b.read_console(&mut buf);
            if n == 0 {
                break;
            }
            h.add_u64(b.cycles());
            h.add(&buf[..n]);
            console.extend(buf[..n].iter().map(|&c| if c == b'\r' { '\n' } else { c as char }));
        }
        while let Some(f) = b.pop_radio_frame() {
            h.add_u64(b.cycles());
            h.add(&f);
            // answer at once (while the node is still keyed up) or a
            // little later
            peer.heard(&f, b.cycles() + (frames.len() as u64 % 3) * 60 * per_ms);
            frames.push((b.cycles(), ax25::describe(&f)));
        }
    }
    Session { trace: h.0, ram: tnc2_core::rom::fingerprint(&b.ram()[..], &[]), console, frames }
}

fn check(cfg: &BoardConfig, want: (u64, u64)) {
    let s = run_session(cfg);
    println!("{}", s.console);
    for (t, f) in &s.frames {
        println!("{t:>10} {f}");
    }
    println!("trace {:016x} ram {:016x}", s.trace, s.ram);
    // the session did what it is meant to
    assert!(s.console.contains("CONNECTED to"), "no console connect");
    let has = |what: &str| s.frames.iter().any(|(_, f)| f.contains(what));
    assert!(has("G9TNT-1>G9XRT <UA"), "node never answered the SABM");
    assert!(has("G9TNT-1>G9XRT <SABM"), "node never connected out");
    assert!(has("G9TNT-1>G9XRT <I"), "node sent no I frames");
    assert!(has("G9TNT-1>NODES"), "no NODES broadcast");
    assert_eq!((s.trace, s.ram), want, "the emulation is not what it was");
}

#[test]
fn emulation_is_unchanged() {
    check(&BoardConfig::default(), STANDARD);
}

#[test]
fn emulation_is_unchanged_fast_rs232() {
    check(&BoardConfig { hdlc_baud: 4800, async_baud: 115200, ..BoardConfig::default() }, FAST);
}
