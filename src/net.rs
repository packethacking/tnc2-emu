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

    /// Accept new clients, read what is available. Returns raw bytes per client.
    pub fn poll(&mut self) -> Vec<(usize, Vec<u8>)> {
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
        let mut got = Vec::new();
        let mut dead = Vec::new();
        for (i, c) in self.clients.iter_mut().enumerate() {
            let mut buf = [0u8; 4096];
            loop {
                match c.stream.read(&mut buf) {
                    Ok(0) => {
                        dead.push(i);
                        break;
                    }
                    Ok(n) => got.push((i, buf[..n].to_vec())),
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
        got
    }

    pub fn send_all(&mut self, data: &[u8]) {
        for c in &mut self.clients {
            if c.out.len() < 1 << 20 {
                c.out.extend_from_slice(data);
            }
        }
    }

    /// Feed bytes from client `i` through its KISS decoder, returning
    /// complete KISS frames (command byte included, escapes removed).
    pub fn kiss_decode(&mut self, i: usize, data: &[u8]) -> Vec<Vec<u8>> {
        let c = &mut self.clients[i];
        let mut frames = Vec::new();
        for &b in data {
            if let Some(f) = c.kiss.push(b) {
                frames.push(f.to_vec());
            }
        }
        frames
    }
}
