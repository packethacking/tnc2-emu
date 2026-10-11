//! tnc2-emu: a TAPR TNC-2 emulator that runs TheNet X1J NET/ROM firmware.

mod ax25;
mod net;
mod sio;

use rustzx_z80::{Z80, Z80Bus};
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const ROM_HALF: usize = 0x8000;

struct Config {
    rom1: String,
    rom2: String,
    call: Option<String>,
    alias: Option<String>,
    pokes: Vec<(usize, Vec<u8>)>,
    bind: String,
    kiss_port: u16,
    console_port: u16,
    ram_file: Option<String>,
    cold: bool,
    clock_hz: u64,
    speed: f64,
    hdlc_baud: u64,
    async_baud: u64,
    protocol_mode: bool,
    monitor: bool,
    console_echo: bool,
    capture: Option<String>,
    debug: bool,
}

fn usage() -> ! {
    eprintln!(
        "tnc2-emu: TNC-2 emulator running TheNet X1J

Options (environment variable in brackets):
  --rom1 FILE          lower EPROM half (THENET1.X1J) [TNC2_ROM1]
  --rom2 FILE          upper EPROM half (THENET2.X1J) [TNC2_ROM2]
  --call CALL[-SSID]   node callsign patched into the ROM [TNC2_CALL]
  --alias ALIAS        node alias, up to 6 chars [TNC2_ALIAS]
  --poke OFF=HH[,HH..] patch config bytes at OFF (hex) in both halves [TNC2_POKE, ';' separated]
  --bind ADDR          listen address, default 0.0.0.0 [TNC2_BIND]
  --kiss-port N        KISS TCP server port (radio side), default 8001 [TNC2_KISS_PORT]
  --console-port N     raw TCP console port (RS232 side), default 8002 [TNC2_CONSOLE_PORT]
  --ram-file FILE      battery backed RAM image, saved every few seconds [TNC2_RAM_FILE]
  --cold               ignore the saved RAM and cold start
  --speed X            1 = real time (default), 2 = twice as fast, 0 = flat out [TNC2_SPEED]
  --clock HZ           CPU clock, default 4915200
  --hdlc-baud N        radio port bit rate, default 1200 [TNC2_HDLC_BAUD]
  --async-baud N       RS232 bit rate, default 9600
  --protocol-mode      assert RS232 DCD for KISS/crosslink mode instead of dumb terminal
  --monitor            print decoded radio frames to stderr [TNC2_MONITOR=1]
  --console-echo       copy console output to stderr
  --capture FILE       append every radio frame (hex, timestamped) to FILE [TNC2_CAPTURE]
  --debug              chatty diagnostics"
    );
    std::process::exit(2)
}

fn parse_num(s: &str) -> Option<u64> {
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(h, 16).ok()
    } else {
        s.parse().ok()
    }
}

fn parse_poke(s: &str) -> (usize, Vec<u8>) {
    let (o, v) = s.split_once('=').unwrap_or_else(|| usage());
    let off = usize::from_str_radix(o.trim_start_matches("0x"), 16).unwrap_or_else(|_| usage());
    let bytes = v
        .split(',')
        .map(|b| u8::from_str_radix(b.trim().trim_start_matches("0x"), 16).unwrap_or_else(|_| usage()))
        .collect();
    (off, bytes)
}

fn parse_args() -> Config {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let mut c = Config {
        rom1: env("TNC2_ROM1").unwrap_or_else(|| "rom/THENET1.X1J".into()),
        rom2: env("TNC2_ROM2").unwrap_or_else(|| "rom/THENET2.X1J".into()),
        call: env("TNC2_CALL"),
        alias: env("TNC2_ALIAS"),
        pokes: env("TNC2_POKE")
            .map(|p| p.split(';').filter(|x| !x.is_empty()).map(parse_poke).collect())
            .unwrap_or_default(),
        bind: env("TNC2_BIND").unwrap_or_else(|| "0.0.0.0".into()),
        kiss_port: env("TNC2_KISS_PORT").and_then(|v| v.parse().ok()).unwrap_or(8001),
        console_port: env("TNC2_CONSOLE_PORT").and_then(|v| v.parse().ok()).unwrap_or(8002),
        ram_file: env("TNC2_RAM_FILE"),
        cold: false,
        clock_hz: 4_915_200,
        speed: env("TNC2_SPEED").and_then(|v| v.parse().ok()).unwrap_or(1.0),
        hdlc_baud: env("TNC2_HDLC_BAUD").and_then(|v| v.parse().ok()).unwrap_or(1200),
        async_baud: 9600,
        protocol_mode: false,
        monitor: env("TNC2_MONITOR").is_some_and(|v| v != "0"),
        console_echo: false,
        capture: env("TNC2_CAPTURE"),
        debug: env("TNC2_DEBUG").is_some_and(|v| v != "0"),
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = || args.next().unwrap_or_else(|| usage());
        match a.as_str() {
            "--rom1" => c.rom1 = val(),
            "--rom2" => c.rom2 = val(),
            "--call" => c.call = Some(val()),
            "--alias" => c.alias = Some(val()),
            "--poke" => c.pokes.push(parse_poke(&val())),
            "--bind" => c.bind = val(),
            "--kiss-port" => c.kiss_port = val().parse().unwrap_or_else(|_| usage()),
            "--console-port" => c.console_port = val().parse().unwrap_or_else(|_| usage()),
            "--ram-file" => c.ram_file = Some(val()),
            "--cold" => c.cold = true,
            "--speed" => c.speed = val().parse().unwrap_or_else(|_| usage()),
            "--clock" => c.clock_hz = parse_num(&val()).unwrap_or_else(|| usage()),
            "--hdlc-baud" => c.hdlc_baud = parse_num(&val()).unwrap_or_else(|| usage()),
            "--async-baud" => c.async_baud = parse_num(&val()).unwrap_or_else(|| usage()),
            "--protocol-mode" => c.protocol_mode = true,
            "--monitor" => c.monitor = true,
            "--console-echo" => c.console_echo = true,
            "--capture" => c.capture = Some(val()),
            "--debug" => c.debug = true,
            "--no-debug" => c.debug = false,
            _ => usage(),
        }
    }
    c
}

fn load_half(path: &str) -> Vec<u8> {
    let mut d = fs::read(path).unwrap_or_else(|e| {
        eprintln!("cannot read {path}: {e}");
        std::process::exit(1)
    });
    if d.len() > ROM_HALF {
        eprintln!("{path} is larger than 32 KB");
        std::process::exit(1)
    }
    // The distribution files have the trailing erased (0xFF) bytes trimmed.
    d.resize(ROM_HALF, 0xFF);
    d
}

/// Patch the configuration block that sits at the start of both halves.
fn patch(rom: &mut [u8], c: &Config) {
    if let Some(call) = &c.call {
        let (cs, ssid) = match call.split_once('-') {
            Some((a, b)) => (a, b.parse::<u8>().unwrap_or_else(|_| usage())),
            None => (call.as_str(), 0),
        };
        if cs.len() > 6 || ssid > 15 {
            usage();
        }
        let mut f = [b' '; 6];
        f[..cs.len()].copy_from_slice(cs.to_ascii_uppercase().as_bytes());
        rom[0x3B..0x41].copy_from_slice(&f);
        rom[0x41] = 0x60 | (ssid << 1);
    }
    if let Some(alias) = &c.alias {
        if alias.len() > 6 {
            usage();
        }
        let mut f = [b' '; 6];
        f[..alias.len()].copy_from_slice(alias.to_ascii_uppercase().as_bytes());
        rom[0x42..0x48].copy_from_slice(&f);
    }
    for (off, bytes) in &c.pokes {
        rom[*off..*off + bytes.len()].copy_from_slice(bytes);
    }
}

fn fnv(data: &[&[u8]]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for d in data {
        for &b in *d {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

const RAM_MAGIC: &[u8; 8] = b"TNC2RAM1";

static STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// SIGTERM / SIGINT: save the RAM and exit (we may be PID 1 in a container).
fn install_signal_handlers() {
    unsafe {
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
    }
}

fn save_ram(path: &str, fingerprint: u64, ram: &[u8]) {
    let mut d = Vec::with_capacity(16 + ram.len());
    d.extend_from_slice(RAM_MAGIC);
    d.extend_from_slice(&fingerprint.to_le_bytes());
    d.extend_from_slice(ram);
    let tmp = format!("{path}.tmp");
    if fs::write(&tmp, &d).and_then(|_| fs::rename(&tmp, path)).is_err() {
        eprintln!("ram: could not save {path}");
    }
}

struct Board {
    rom: [Vec<u8>; 2],
    ram: Vec<u8>,
    sio: sio::Sio,
    cycles: u64,
    seen_ports: HashSet<(bool, u8)>,
    adc_channel: u8,
}

impl Board {
    fn bank(&self) -> usize {
        // EPROM A15 is wired to the SIO DTRA pin, which is active low:
        // WR5A bit 7 set drives the pin low and selects the lower half.
        if self.sio.a.dtr() { 0 } else { 1 }
    }

    fn note_port(&mut self, is_out: bool, port: u8, v: u8) {
        if self.seen_ports.insert((is_out, port)) {
            eprintln!(
                "note: first {} port 0x{port:02X}{}",
                if is_out { "OUT to" } else { "IN from" },
                if is_out { format!(" value 0x{v:02X}") } else { String::new() }
            );
        }
    }
}

impl Z80Bus for Board {
    fn read_internal(&mut self, addr: u16) -> u8 {
        let a = addr as usize;
        if a < ROM_HALF {
            self.rom[self.bank()][a]
        } else {
            self.ram[a - ROM_HALF]
        }
    }
    fn write_internal(&mut self, addr: u16, data: u8) {
        let a = addr as usize;
        if a >= ROM_HALF {
            self.ram[a - ROM_HALF] = data;
        }
    }
    fn wait_mreq(&mut self, _addr: u16, clk: usize) {
        self.cycles += clk as u64;
    }
    fn wait_no_mreq(&mut self, _addr: u16, clk: usize) {
        self.cycles += clk as u64;
    }
    fn wait_internal(&mut self, clk: usize) {
        self.cycles += clk as u64;
    }
    fn read_io(&mut self, port: u16) -> u8 {
        let p = port as u8;
        self.cycles += 4;
        match p {
            0..=3 => {
                self.sio.run(self.cycles);
                self.sio.read(p)
            }
            0x20 => {
                // ADC0844 on the optional deviation meter / S meter board.
                // Not fitted: return zero for every channel.
                self.note_port(false, p, 0);
                let _ = self.adc_channel;
                0
            }
            _ => {
                self.note_port(false, p, 0);
                0xFF
            }
        }
    }
    fn write_io(&mut self, port: u16, data: u8) {
        let p = port as u8;
        self.cycles += 4;
        match p {
            0..=3 => {
                self.sio.run(self.cycles);
                self.sio.write(p, data, self.cycles);
            }
            0x20 => {
                self.note_port(true, p, data);
                self.adc_channel = data;
            }
            _ => self.note_port(true, p, data),
        }
    }
    fn read_interrupt(&mut self) -> u8 {
        self.sio.vector()
    }
    fn reti(&mut self) {}
    fn halt(&mut self, _halted: bool) {}
    fn int_active(&self) -> bool {
        self.sio.int_active()
    }
    fn nmi_active(&self) -> bool {
        false
    }
    fn pc_callback(&mut self, _addr: u16) {}
}

fn utc_now() -> String {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs();
    let ms = d.subsec_millis();
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    // civil from days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let dd = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{dd:02}T{:02}:{:02}:{:02}.{ms:03}Z",
        rem / 3600,
        (rem / 60) % 60,
        rem % 60
    )
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
}

fn main() {
    let cfg = parse_args();
    install_signal_handlers();
    if cfg.debug {
        sio::log::DEBUG.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    let mut rom1 = load_half(&cfg.rom1);
    let mut rom2 = load_half(&cfg.rom2);
    patch(&mut rom1, &cfg);
    patch(&mut rom2, &cfg);
    let fingerprint = fnv(&[&rom1, &rom2]);

    let mut ram = vec![0u8; 0x8000];
    if let (Some(path), false) = (&cfg.ram_file, cfg.cold) {
        match fs::read(path) {
            Ok(d) if d.len() == 16 + 0x8000 && &d[..8] == RAM_MAGIC => {
                let fp = u64::from_le_bytes(d[8..16].try_into().unwrap());
                if fp == fingerprint {
                    ram.copy_from_slice(&d[16..]);
                    eprintln!("ram: restored {path} (warm start)");
                } else {
                    eprintln!("ram: {path} was saved with a different ROM configuration, cold start");
                }
            }
            Ok(_) => eprintln!("ram: {path} is not a RAM image, cold start"),
            Err(_) => eprintln!("ram: no {path} yet, cold start"),
        }
    }

    let clock = cfg.clock_hz;
    let hdlc_char = clock * 8 / cfg.hdlc_baud;
    let async_char = clock * 10 / cfg.async_baud;
    let mut board = Board {
        rom: [rom1, rom2],
        ram,
        sio: sio::Sio::new(hdlc_char, async_char),
        cycles: 0,
        seen_ports: HashSet::new(),
        adc_channel: 0,
    };
    // RS232 DCD (SIO DCDB) selects the port mode: asserted = dumb terminal.
    board.sio.b.dcd = !cfg.protocol_mode;
    board.sio.b.cts = true;
    board.sio.a.dcd = false;
    board.sio.a.cts = true;
    board.sio.set_ext();

    let mut kiss = net::Server::bind("kiss", &format!("{}:{}", cfg.bind, cfg.kiss_port))
        .unwrap_or_else(|e| {
            eprintln!("kiss: bind failed: {e}");
            std::process::exit(1)
        });
    let mut console = net::Server::bind("console", &format!("{}:{}", cfg.bind, cfg.console_port))
        .unwrap_or_else(|e| {
            eprintln!("console: bind failed: {e}");
            std::process::exit(1)
        });
    let mut capture = cfg.capture.as_ref().map(|p| {
        fs::OpenOptions::new().create(true).append(true).open(p).unwrap_or_else(|e| {
            eprintln!("capture: cannot open {p}: {e}");
            std::process::exit(1)
        })
    });

    let mut cpu = Z80::default();
    eprintln!(
        "{} tnc2-emu: power on, clock {} Hz, speed {}, radio {} bit/s, RS232 {} bit/s, RS232 mode {}",
        utc_now(),
        clock,
        cfg.speed,
        cfg.hdlc_baud,
        cfg.async_baud,
        if cfg.protocol_mode { "protocol (KISS/crosslink)" } else { "dumb terminal" }
    );

    // The firmware's time base: SIO SYNCB is fed with a 600 Hz square wave
    // (4.9152 MHz / 8192) and every edge raises an ext/status interrupt.
    let tick_cycles = clock / 1200;
    let mut next_tick = tick_cycles;
    let mut tick_due: u64 = 0;

    let slice = clock / 1000; // about 1 ms of emulated time
    let start = Instant::now();
    let mut last_save = Instant::now();
    let mut last_debug = Instant::now();
    loop {
        let end = board.cycles + slice;
        while board.cycles < end {
            while board.cycles >= next_tick {
                next_tick += tick_cycles;
                if board.sio.b.wr[1] & 0x01 != 0 {
                    tick_due += 1;
                }
            }
            if tick_due > 0 && !board.sio.b.ext_pending() {
                board.sio.b.sync = !board.sio.b.sync;
                board.sio.set_ext();
                tick_due -= 1;
            }
            board.sio.run(board.cycles);
            if cpu.is_halted() && !board.sio.int_active() {
                board.cycles += 4;
                continue;
            }
            cpu.emulate(&mut board);
        }

        // radio side: transmitted frames out, received frames in
        while let Some(f) = board.sio.a.tx_frames.pop_front() {
            if cfg.monitor {
                eprintln!("{} TX {}", utc_now(), ax25::describe(&f));
            }
            if let Some(cap) = capture.as_mut() {
                let _ = writeln!(cap, "{} TX {}", utc_now(), hex(&f));
            }
            kiss.send_all(&net::kiss_encode(0x00, &f));
        }
        for (i, data) in kiss.poll() {
            for kf in kiss.kiss_decode(i, &data) {
                if kf.is_empty() || kf[0] & 0x0f != 0 {
                    continue; // only data frames; KISS parameters are ignored
                }
                let f = kf[1..].to_vec();
                if cfg.monitor {
                    eprintln!("{} RX {}", utc_now(), ax25::describe(&f));
                }
                if let Some(cap) = capture.as_mut() {
                    let _ = writeln!(cap, "{} RX {}", utc_now(), hex(&f));
                }
                board.sio.a.rx_frames.push_back(f);
            }
        }

        // RS232 side
        if !board.sio.b.tx_bytes.is_empty() {
            let out = std::mem::take(&mut board.sio.b.tx_bytes);
            if cfg.console_echo {
                let s: String = out
                    .iter()
                    .map(|&c| match c {
                        b'\r' => "\n".to_string(),
                        0x20..=0x7e | b'\n' => (c as char).to_string(),
                        _ => format!("<{c:02x}>"),
                    })
                    .collect();
                eprint!("{s}");
            }
            console.send_all(&out);
        }
        for (_, data) in console.poll() {
            board.sio.b.rx_bytes.extend(data);
        }

        if let Some(path) = &cfg.ram_file {
            if last_save.elapsed() > Duration::from_secs(5) {
                last_save = Instant::now();
                save_ram(path, fingerprint, &board.ram);
            }
        }

        if STOP.load(std::sync::atomic::Ordering::Relaxed) {
            if let Some(path) = &cfg.ram_file {
                save_ram(path, fingerprint, &board.ram);
                eprintln!("ram: saved {path}");
            }
            eprintln!("{} tnc2-emu: power off", utc_now());
            std::process::exit(0);
        }

        if cfg.debug && last_debug.elapsed() > Duration::from_secs(5) {
            last_debug = Instant::now();
            eprintln!(
                "debug: t={:.1}s emu={:.1}s pc={:04X} bank={} ticks_due={}",
                start.elapsed().as_secs_f64(),
                board.cycles as f64 / clock as f64,
                cpu.regs.get_pc(),
                board.bank(),
                tick_due
            );
            eprintln!(
                "debug: txstate(8227)={} slot(8229)={} A: {}",
                board.ram[0x227], board.ram[0x229], board.sio.a.debug_state()
            );
        }

        // throttle
        if cfg.speed > 0.0 {
            let target = board.cycles as f64 / (clock as f64 * cfg.speed);
            let now = start.elapsed().as_secs_f64();
            if target > now {
                std::thread::sleep(Duration::from_secs_f64((target - now).min(0.005)));
            }
        }
    }
}
