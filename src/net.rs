//! Small non-blocking TCP servers for the KISS and console ports.

use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};

pub struct Client {
    stream: TcpStream,
    out: Vec<u8>,
    pub peer: String,
    /// KISS decoder state
    kiss_buf: Vec<u8>,
    kiss_esc: bool,
    kiss_in_frame: bool,
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
                        kiss_buf: Vec::new(),
                        kiss_esc: false,
                        kiss_in_frame: false,
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
        const FEND: u8 = 0xC0;
        const FESC: u8 = 0xDB;
        let c = &mut self.clients[i];
        let mut frames = Vec::new();
        for &b in data {
            if b == FEND {
                if c.kiss_in_frame && !c.kiss_buf.is_empty() {
                    frames.push(std::mem::take(&mut c.kiss_buf));
                }
                c.kiss_buf.clear();
                c.kiss_in_frame = true;
                c.kiss_esc = false;
                continue;
            }
            if !c.kiss_in_frame {
                continue;
            }
            if c.kiss_esc {
                c.kiss_esc = false;
                c.kiss_buf.push(match b {
                    0xDC => FEND,
                    0xDD => FESC,
                    x => x,
                });
            } else if b == FESC {
                c.kiss_esc = true;
            } else if c.kiss_buf.len() < 2048 {
                c.kiss_buf.push(b);
            }
        }
        frames
    }
}

pub fn kiss_encode(port_cmd: u8, frame: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(frame.len() + 4);
    v.push(0xC0);
    v.push(port_cmd);
    for &b in frame {
        match b {
            0xC0 => v.extend_from_slice(&[0xDB, 0xDC]),
            0xDB => v.extend_from_slice(&[0xDB, 0xDD]),
            x => v.push(x),
        }
    }
    v.push(0xC0);
    v
}
