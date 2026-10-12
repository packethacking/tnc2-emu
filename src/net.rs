//! Small non-blocking TCP servers for the KISS and console ports.

use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use tnc2_core::kiss;

pub struct Client {
    stream: TcpStream,
    out: Vec<u8>,
    pub peer: String,
    kiss: Box<kiss::Decoder>,
}

pub struct Server {
    name: &'static str,
    listener: TcpListener,
    pub clients: Vec<Client>,
}

impl Server {
    pub fn bind(name: &'static str, addr: &str) -> std::io::Result<Server> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        eprintln!("{name}: listening on {addr}");
        Ok(Server { name, listener, clients: Vec::new() })
    }

    /// Accept new clients and read what is available, client by client.
    pub fn poll(&mut self) -> Vec<Vec<u8>> {
        let mut got = Vec::new();
        self.poll_with(|_, data| got.push(data.to_vec()));
        got
    }

    /// As `poll`, but run each client's bytes through its own KISS decoder
    /// and return complete KISS frames (command byte included, escapes
    /// removed). A client's last bytes are decoded even if it then closed.
    pub fn poll_kiss(&mut self) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        self.poll_with(|c, data| {
            for &b in data {
                if let Some(f) = c.kiss.push(b) {
                    frames.push(f.to_vec());
                }
            }
        });
        frames
    }

    /// Accept, read and write. `on_data` sees each chunk read while its
    /// client still exists; clients that went away are removed afterwards.
    fn poll_with(&mut self, mut on_data: impl FnMut(&mut Client, &[u8])) {
        loop {
            match self.listener.accept() {
                Ok((s, a)) => {
                    let _ = s.set_nonblocking(true);
                    let _ = s.set_nodelay(true);
                    eprintln!("{}: client {} connected", self.name, a);
                    self.clients.push(Client {
                        stream: s,
                        out: Vec::new(),
                        peer: a.to_string(),
                        kiss: Box::new(kiss::Decoder::new()),
                    });
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) => {
                    eprintln!("{}: accept error {e}", self.name);
                    break;
                }
            }
        }
        let mut dead = Vec::new();
        for (i, c) in self.clients.iter_mut().enumerate() {
            let mut buf = [0u8; 4096];
            loop {
                match c.stream.read(&mut buf) {
                    Ok(0) => {
                        dead.push(i);
                        break;
                    }
                    Ok(n) => on_data(c, &buf[..n]),
                    Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                    Err(_) => {
                        dead.push(i);
                        break;
                    }
                }
            }
            if !c.out.is_empty() {
                match c.stream.write(&c.out) {
                    Ok(n) => {
                        c.out.drain(..n);
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                    Err(_) => dead.push(i),
                }
            }
        }
        dead.sort();
        dead.dedup();
        for i in dead.into_iter().rev() {
            eprintln!("{}: client {} disconnected", self.name, self.clients[i].peer);
            self.clients.remove(i);
        }
    }

    pub fn send_all(&mut self, data: &[u8]) {
        for c in &mut self.clients {
            if c.out.len() < 1 << 20 {
                c.out.extend_from_slice(data);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn poll_until(srv: &mut Server, frames: &mut Vec<Vec<u8>>, done: impl Fn(&Server) -> bool) {
        for _ in 0..100 {
            frames.extend(srv.poll_kiss());
            if done(srv) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("timed out");
    }

    /// One client sends a whole frame and closes while another is part
    /// way through a frame: each client's bytes must reach its own
    /// decoder, and the closed client's frame must not be lost.
    #[test]
    fn closing_client_keeps_its_own_decoder() {
        let mut srv = Server::bind("kiss", "127.0.0.1:0").unwrap();
        let addr = srv.listener.local_addr().unwrap();
        let mut frames = Vec::new();
        let mut a = TcpStream::connect(addr).unwrap();
        poll_until(&mut srv, &mut frames, |s| s.clients.len() == 1);
        let mut b = TcpStream::connect(addr).unwrap();
        poll_until(&mut srv, &mut frames, |s| s.clients.len() == 2);

        b.write_all(&[0xC0, 0x00, 0x41]).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        frames.extend(srv.poll_kiss());
        a.write_all(&[0xC0, 0x00, 0x42, 0xC0]).unwrap();
        drop(a);
        std::thread::sleep(Duration::from_millis(100));
        poll_until(&mut srv, &mut frames, |s| s.clients.len() == 1);
        b.write_all(&[0x43, 0xC0]).unwrap();
        for _ in 0..100 {
            frames.extend(srv.poll_kiss());
            if frames.len() >= 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(frames, [vec![0x00, 0x42], vec![0x00, 0x41, 0x43]]);
    }
}
