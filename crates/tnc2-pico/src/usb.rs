//! USB: one composite device with two serial ports (CDC ACM).
//!
//! The first port is TheNet's RS232 console, byte for byte. The second is
//! the Pico's own: setup commands and status, and for now the speed test.
//! On Linux they are usually /dev/ttyACM0 and /dev/ttyACM1, and always
//! /dev/serial/by-id/usb-packethacking_tnc2-pico_<serial>-if00 and -if02.

use embassy_executor::Spawner;
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Driver, InterruptHandler};
use embassy_rp::{Peri, bind_interrupts};
use embassy_usb::UsbDevice;
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::driver::EndpointError;
use static_cell::StaticCell;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

pub type Port = CdcAcmClass<'static, Driver<'static, USB>>;

/// Full speed bulk packets.
pub const PACKET: usize = 64;

/// Start the USB device and return the console port and the status port.
pub fn start(spawner: Spawner, usb: Peri<'static, USB>) -> (Port, Port) {
    // The chip's unique ID, so each board gets its own /dev/serial/by-id
    // names.
    let chip_id = embassy_rp::otp::get_chipid().unwrap_or(0);
    let serial: &'static str = alloc::format!("{chip_id:016X}").leak();

    // pid.codes test VID/PID, fine for development; the node will want a
    // PID of its own.
    let mut config = embassy_usb::Config::new(0x1209, 0x0001);
    config.manufacturer = Some("packethacking");
    config.product = Some("tnc2-pico");
    config.serial_number = Some(serial);
    config.max_power = 100;
    config.max_packet_size_0 = 64;

    static CONFIG_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESC: StaticCell<[u8; 64]> = StaticCell::new();
    static CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();
    let mut builder = embassy_usb::Builder::new(
        Driver::new(usb, Irqs),
        config,
        CONFIG_DESC.init([0; 256]),
        BOS_DESC.init([0; 64]),
        &mut [],
        CONTROL_BUF.init([0; 64]),
    );

    static STATES: StaticCell<[State; 2]> = StaticCell::new();
    let [console_state, status_state] = STATES.init([State::new(), State::new()]);
    let console = CdcAcmClass::new(&mut builder, console_state, PACKET as u16);
    let status = CdcAcmClass::new(&mut builder, status_state, PACKET as u16);

    spawner.spawn(run(builder.build()).expect("USB task"));
    (console, status)
}

#[embassy_executor::task]
async fn run(mut usb: UsbDevice<'static, Driver<'static, USB>>) -> ! {
    usb.run().await
}

/// Send `text` if a terminal has the port open (DTR set); otherwise drop
/// it, so nothing stale is waiting when one connects.
pub async fn send(port: &mut Port, text: &[u8]) -> Result<(), EndpointError> {
    if !port.dtr() {
        return Ok(());
    }
    for chunk in text.chunks(PACKET) {
        port.write_packet(chunk).await?;
    }
    if text.len().is_multiple_of(PACKET) {
        // A full last packet tells the host more is coming; end the
        // transfer.
        port.write_packet(&[]).await?;
    }
    Ok(())
}
