//! Boot TheNet X1J with no I/O and run it flat out.
//!
//! cargo run --release -p tnc2-core --example bench [-- SECONDS]

use std::time::Instant;
use tnc2_core::{Board, BoardConfig, NodeConfig, bench};

static ROM1: &[u8] = include_bytes!("../../../rom/THENET1.X1J");
static ROM2: &[u8] = include_bytes!("../../../rom/THENET2.X1J");

fn main() {
    let seconds = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(60);
    let mut board = Board::from_rom(ROM1, ROM2, &NodeConfig::default(), &BoardConfig::default()).unwrap();
    let t0 = Instant::now();
    let report = bench::run(&mut board, seconds, || t0.elapsed().as_micros() as u64);
    println!("{report}");
}
