//! tnc2-pico: the TNC-2 board from tnc2-core on a Raspberry Pi Pico 2 W.
//!
//! The pieces every firmware binary here needs: the heap, the copy of the
//! emulator into SRAM, the system clock and the two USB serial ports.

#![no_std]

extern crate alloc;

pub mod clock;
pub mod heap;
pub mod ram_code;
pub mod usb;
