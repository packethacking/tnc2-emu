//! tnc2-node: the parts of the Raspberry Pi Pico 2 W node firmware
//! (crates/tnc2-pico, the `node` binary) that need no hardware, so they
//! build and test on the host with a plain `cargo test`.
//!
//! - `setup`: the commands on the Pico's own serial port, the MYCALL and
//!   ALIAS checks, and the line editor.
//! - `store`: the battery backed RAM and the settings in flash, as a ring
//!   of slots that survives a power cut at any moment.
//! - `radio`: KISS to and from the NinoTNC, dropping any frame a serial
//!   error touched.
//! - `pipe`: lock free single producer, single consumer queues between the
//!   two cores.
//! - `handoff`: the RAM snapshot and the board passed between the cores.
//! - `engine`: core 0's loop, a slice at a time.
//! - `pace`: keeping emulated time locked to real time, and measuring how
//!   well it does.
//! - `status`: the STATUS report.

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod crc;
pub mod engine;
pub mod handoff;
pub mod pace;
pub mod pipe;
pub mod radio;
pub mod setup;
pub mod status;
pub mod store;
