//! Issue 4: a frame queued while TheNet is sending the padding after the
//! previous frame must still go out.
//!
//! TheNet's transmit kick (0x028A) sees the padding state (RAM 0x8227 = 8
//! to 10), sets the state back to 6 and sends an SIO abort, then waits for
//! a Tx buffer empty interrupt to start the queued frame. If the SIO model
//! never gives that interrupt the node stays keyed up and silent for good.
//!
//! The frames are from the failing interop run 38224130409: XRouter's NODES
//! broadcast, then its SABM. TheNet answers with a UA, and "C XRNODE" typed
//! on the console queues a NET/ROM CONN REQ. The offsets below put that
//! CONN REQ in the padding after the UA.

use tnc2_core::{Board, BoardConfig, NodeConfig, Poke};

static ROM1: &[u8] = include_bytes!("../../../rom/THENET1.X1J");
static ROM2: &[u8] = include_bytes!("../../../rom/THENET2.X1J");

/// G9XRT>NODES UI, NODES from alias XRNODE, no entries.
const XR_NODES: &[u8] = &[
    0x9c, 0x9e, 0x88, 0x8a, 0xa6, 0x40, 0xe0, 0x8e, 0x72, 0xb0, 0xa4, 0xa8, 0x40, 0x61, 0x03, 0xcf, 0xff, 0x58,
    0x52, 0x4e, 0x4f, 0x44, 0x45,
];
/// G9XRT>G9TNT-1 SABM.
const XR_SABM: &[u8] = &[0x8e, 0x72, 0xa8, 0x9c, 0xa8, 0x40, 0xe2, 0x8e, 0x72, 0xb0, 0xa4, 0xa8, 0x40, 0x61, 0x3f];

const SABM_AT_MS: u64 = 3000;

struct Outcome {
    /// The firmware went from the padding state straight back to sending
    /// a frame, which only happens through the abort in 0x028A.
    aborted_padding: bool,
    conn_req: bool,
    last_state: u8,
}

fn is_conn_req(f: &[u8]) -> bool {
    f.len() >= 36 && f[15] == 0xcf && f[35] & 0x0f == 1
}

fn run(c_at_ms: u64) -> Outcome {
    let pokes = [Poke { offset: 0x4c, bytes: &[0xc0, 0x00] }, Poke { offset: 0x54, bytes: &[0x3c, 0x00] }];
    let node = NodeConfig { call: Some("G9TNT-1"), alias: Some("TNET"), pokes: &pokes };
    let mut b = Board::from_rom(ROM1, ROM2, &node, &BoardConfig::default()).unwrap();
    let step = b.clock_hz() / 1000;
    let mut out = Outcome { aborted_padding: false, conn_req: false, last_state: 0 };
    let mut prev = 0u8;
    let mut buf = [0u8; 256];
    for ms in 0..c_at_ms + 4000 {
        match ms {
            1000 => assert_eq!(b.push_console(b"\x1b"), 1),
            1500 => assert_eq!(b.push_console(b"C\r"), 2),
            2000 => assert!(b.push_radio_frame(XR_NODES)),
            SABM_AT_MS => assert!(b.push_radio_frame(XR_SABM)),
            _ => {}
        }
        if ms == c_at_ms {
            assert_eq!(b.push_console(b"C XRNODE\r"), 9);
        }
        b.run(step);
        while b.read_console(&mut buf) > 0 {}
        while let Some(f) = b.pop_radio_frame() {
            out.conn_req |= is_conn_req(&f);
        }
        let s = b.ram()[0x227];
        if (8..=10).contains(&prev) && (s == 5 || s == 6) {
            out.aborted_padding = true;
        }
        prev = s;
        if out.conn_req && ms > c_at_ms {
            break;
        }
    }
    out.last_state = prev;
    out
}

#[test]
fn frame_queued_during_padding_is_sent() {
    // C XRNODE typed this many ms after the SABM arrives. The padding
    // window is 871 to 891 ms (found with find_window); these sit inside.
    let offsets = [874u64, 878, 881, 884, 888];
    let mut hits = 0;
    for o in offsets {
        let r = run(SABM_AT_MS + o);
        hits += r.aborted_padding as usize;
        assert!(
            r.conn_req,
            "offset {o} ms: no CONN REQ, transmit state stuck at {} (aborted padding: {})",
            r.last_state, r.aborted_padding
        );
    }
    // if the firmware timing moves, the window has to be found again
    assert!(hits > 0, "no offset hit the padding window: the test no longer covers the abort");
}

/// Print the offsets that hit the padding window, for when the firmware
/// timing moves: cargo test --release --test abort_tx_interrupt -- --ignored --nocapture
#[test]
#[ignore]
fn find_window() {
    for o in 0..1500u64 {
        let r = run(SABM_AT_MS + o);
        if r.aborted_padding || !r.conn_req {
            println!("offset {o}: aborted_padding={} conn_req={} state={}", r.aborted_padding, r.conn_req, r.last_state);
        }
    }
}
