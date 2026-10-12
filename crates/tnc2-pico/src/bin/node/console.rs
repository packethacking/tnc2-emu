//! The first USB serial port: TheNet's RS232 console, byte for byte,
//! nothing added. The board's RS232 DCD is asserted, so TheNet treats it
//! as a dumb terminal.

use crate::CONSOLE_PIPE;
use embassy_rp::peripherals::USB;
use embassy_rp::usb::Driver;
use embassy_time::Timer;
use embassy_usb::class::cdc_acm::{Receiver, Sender};
use tnc2_node::pipe::{Consumer, Producer};
use tnc2_pico::usb::PACKET;

/// Typed at the terminal, to TheNet. When core 0 has no room, this stops
/// reading and USB holds the terminal back.
#[embassy_executor::task]
pub async fn from_terminal(mut usb: Receiver<'static, Driver<'static, USB>>, mut to_core0: Producer<CONSOLE_PIPE>) {
    let mut buf = [0u8; PACKET];
    loop {
        usb.wait_connection().await;
        loop {
            while to_core0.space() < PACKET {
                Timer::after_millis(2).await;
            }
            match usb.read_packet(&mut buf).await {
                Ok(n) => {
                    let all = to_core0.write_all(&buf[..n]);
                    debug_assert!(all);
                }
                Err(_) => break,
            }
        }
    }
}

/// From TheNet to the terminal. With no terminal open (DTR clear) the
/// bytes go nowhere, as on an RS232 port with nothing plugged in.
#[embassy_executor::task]
pub async fn to_terminal(mut usb: Sender<'static, Driver<'static, USB>>, mut from_core0: Consumer<CONSOLE_PIPE>) {
    let mut buf = [0u8; PACKET];
    // A full packet tells the host more is coming; if nothing does, an
    // empty one ends the transfer.
    let mut end_transfer = false;
    loop {
        let n = from_core0.read(&mut buf);
        if n == 0 {
            if end_transfer && usb.dtr() {
                let _ = usb.write_packet(&[]).await;
            }
            end_transfer = false;
            Timer::after_millis(1).await;
            continue;
        }
        if !usb.dtr() {
            end_transfer = false;
            continue;
        }
        end_transfer = usb.write_packet(&buf[..n]).await.is_ok() && n == PACKET;
    }
}
