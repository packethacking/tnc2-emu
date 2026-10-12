//! A KISS client that connects, sends one frame and closes must not take
//! the emulator down.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Emu(Child);
impl Drop for Emu {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn connect(port: u16) -> TcpStream {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(s) => return s,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => panic!("connect {port}: {e}"),
        }
    }
}

#[test]
fn one_shot_kiss_client_does_not_crash() {
    let kiss_port = 26000 + (std::process::id() % 1000) as u16;
    let console_port = kiss_port + 1000;
    let mut emu = Emu(
        Command::new(env!("CARGO_BIN_EXE_tnc2-emu"))
            .args(["--rom1", "rom/THENET1.X1J", "--rom2", "rom/THENET2.X1J"])
            .args(["--bind", "127.0.0.1"])
            .args(["--kiss-port", &kiss_port.to_string(), "--console-port", &console_port.to_string()])
            .stderr(Stdio::null())
            .spawn()
            .expect("start emulator"),
    );
    // A UI frame from G9ABC to ID, sent and closed in one go.
    let mut frame = vec![0xC0, 0x00];
    frame.extend(b"ID    ".iter().map(|c| c << 1));
    frame.push(0xE0);
    frame.extend(b"G9ABC ".iter().map(|c| c << 1));
    frame.extend([0x61, 0x03, 0xF0]);
    frame.extend(b"hello");
    frame.push(0xC0);
    let mut k = connect(kiss_port);
    std::thread::sleep(Duration::from_millis(300));
    k.write_all(&frame).unwrap();
    k.shutdown(Shutdown::Both).unwrap();
    drop(k);
    std::thread::sleep(Duration::from_millis(1000));
    assert!(emu.0.try_wait().unwrap().is_none(), "emulator exited after a one-shot KISS client");

    // And it still answers on the console.
    let mut con = connect(console_port);
    con.write_all(b"\x1b").unwrap();
    con.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut got = Vec::new();
    let mut buf = [0u8; 256];
    while Instant::now() < deadline && !got.ends_with(b"* ") {
        if let Ok(n) = con.read(&mut buf) {
            got.extend_from_slice(&buf[..n]);
        }
    }
    assert!(got.ends_with(b"* "), "no console prompt; got {:?}", String::from_utf8_lossy(&got));
}
