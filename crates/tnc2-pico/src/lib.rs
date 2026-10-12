//! tnc2-pico: the TNC-2 board from tnc2-core on a Raspberry Pi Pico 2 W.
//!
//! The pieces the firmware binaries here share: the heap, the copy of the
//! code into SRAM, the EPROM images, the system clock, the two USB serial
//! ports, the watchdog, and (for the node) the flash store.

#![no_std]

extern crate alloc;

pub mod binary_info;
pub mod clock;
pub mod flash;
pub mod heap;
pub mod ram_code;
pub mod rom;
pub mod usb;
pub mod watchdog;
