//! Boot the real firmware unthrottled and check the console and the first
//! NODES broadcast.

use std::io::{Read, Write};
use std::net::TcpStream;
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

fn read_until(s: &mut TcpStream, want: &[u8], secs: u64) -> Vec<u8> {
    s.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut got = Vec::new();
    let mut buf = [0u8; 1024];
    while Instant::now() < deadline {
        if let Ok(n) = s.read(&mut buf) {
            got.extend_from_slice(&buf[..n]);
        }
        if got.windows(want.len()).any(|w| w == want) {
            return got;
        }
    }
    panic!("did not see {:?}; got {:?}", String::from_utf8_lossy(want), String::from_utf8_lossy(&got));
}

#[test]
fn boots_signs_on_and_broadcasts_nodes() {
    let kiss_port = 28000 + (std::process::id() % 1000) as u16;
    let console_port = kiss_port + 1000;
    let _emu = Emu(
        Command::new(env!("CARGO_BIN_EXE_tnc2-emu"))
            .args(["--rom1", "rom/THENET1.X1J", "--rom2", "rom/THENET2.X1J"])
            .args(["--call", "N0CALL-7", "--alias", "TEST"])
            .args(["--bind", "127.0.0.1", "--speed", "0"])
            .args(["--kiss-port", &kiss_port.to_string(), "--console-port", &console_port.to_string()])
            .stderr(Stdio::null())
            .spawn()
            .expect("start emulator"),
    );
    let mut kiss = connect(kiss_port);
    let mut con = connect(console_port);
    std::thread::sleep(Duration::from_millis(500));
    con.write_all(b"\x1b").unwrap();
    read_until(&mut con, b"* ", 10);
    con.write_all(b"C\r").unwrap();
    read_until(&mut con, b"CONNECTED to N0CALL-7", 10);
    con.write_all(b"PARMS\r").unwrap();
    read_until(&mut con, b"TEST:N0CALL-7}", 10);

    // The first NODES broadcast comes 60 emulated seconds after power on:
    // UI frame to NODES, PID 0xCF, signature 0xFF, then the alias.
    let f = read_until(&mut kiss, b"TEST  \xc0", 120);
    let start = f.windows(2).position(|w| w == [0xc0, 0x00]).expect("KISS data frame");
    let frame = &f[start + 2..];
    let nodes: Vec<u8> = b"NODES ".iter().map(|c| c << 1).collect();
    assert_eq!(&frame[..6], &nodes[..]);
    assert_eq!(frame[14], 0x03, "UI control");
    assert_eq!(frame[15], 0xCF, "NET/ROM PID");
    assert_eq!(frame[16], 0xFF, "NODES signature");
}
