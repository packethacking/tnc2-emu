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
    /// Which client `poll` reads first; it moves on by one each poll.
    first: usize,
}

impl Server {
    pub fn bind(name: &'static str, addr: &str) -> std::io::Result<Server> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        eprintln!("{name}: listening on {addr}");
        Ok(Server { name, listener, clients: Vec::new(), first: 0 })
    }

    /// Accept new clients and read what is available, client by client,
    /// but no more than `budget` bytes in all. Anything past that stays in
    /// the socket, so TCP flow control holds the sender back. The client
    /// read first takes turns, so one busy client cannot starve the others
    /// of the budget.
    pub fn poll(&mut self, budget: usize) -> Vec<Vec<u8>> {
        let mut got = Vec::new();
        let first = self.first;
        self.first = self.first.wrapping_add(1);
        self.poll_with(first, budget, |_, data| got.push(data.to_vec()));
        got
    }

    /// As `poll`, but run each client's bytes through its own KISS decoder
    /// and return complete KISS frames (command byte included, escapes
    /// removed). A client's last bytes are decoded even if it then closed.
    pub fn poll_kiss(&mut self) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        self.poll_with(0, usize::MAX, |c, data| {
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
    /// Clients are read starting from index `first` (mod the client count).
    fn poll_with(&mut self, first: usize, mut budget: usize, mut on_data: impl FnMut(&mut Client, &[u8])) {
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
        let n = self.clients.len();
        for k in 0..n {
            let i = (first + k) % n;
            let c = &mut self.clients[i];
            let mut buf = [0u8; 4096];
            while budget > 0 {
                let want = budget.min(buf.len());
                match c.stream.read(&mut buf[..want]) {
                    Ok(0) => {
                        dead.push(i);
                        break;
                    }
                    Ok(n) => {
                        budget -= n;
                        on_data(c, &buf[..n]);
                    }
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

    /// Reads stop at the budget and the rest waits in the socket.
    #[test]
    fn poll_respects_budget() {
        let mut srv = Server::bind("console", "127.0.0.1:0").unwrap();
        let addr = srv.listener.local_addr().unwrap();
        let mut c = TcpStream::connect(addr).unwrap();
        for _ in 0..100 {
            srv.poll(0);
            if srv.clients.len() == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let data: Vec<u8> = (0..30000u32).map(|i| i as u8).collect();
        c.write_all(&data).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let first: Vec<u8> = srv.poll(1000).concat();
        assert_eq!(first.len(), 1000);
        assert!(srv.poll(0).is_empty());
        let mut got = first;
        for _ in 0..100 {
            got.extend(srv.poll(4096).concat());
            if got.len() >= data.len() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(got, data);
    }

    /// Two console clients with the queue nearly full: a client with a
    /// big backlog must not hold the other one up.
    #[test]
    fn budget_is_shared_between_clients() {
        let mut srv = Server::bind("console", "127.0.0.1:0").unwrap();
        let addr = srv.listener.local_addr().unwrap();
        let mut a = TcpStream::connect(addr).unwrap();
        let mut b = TcpStream::connect(addr).unwrap();
        for _ in 0..100 {
            srv.poll(0);
            if srv.clients.len() == 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        a.write_all(&[b'a'; 20000]).unwrap();
        b.write_all(b"bbbb").unwrap();
        std::thread::sleep(Duration::from_millis(100));
        // about what the SIO takes per millisecond at 9600 bit/s
        let mut got = Vec::new();
        for _ in 0..4 {
            got.extend(srv.poll(2).concat());
        }
        assert_eq!(got.iter().filter(|&&c| c == b'b').count(), 4, "{:?}", String::from_utf8_lossy(&got));
        assert_eq!(got.len(), 8);
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
