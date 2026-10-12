//! The second USB serial port: the Pico's own, for MYCALL, ALIAS, STATUS
//! and HELP at a simple prompt. Everything else about the node is set
//! from TheNet's own console (PARMS and the rest), and kept in its RAM.

use crate::manager::{self, CHANGES, Change, REPLIES, Reply};
use crate::radio::{RX, TX_TOO_LONG};
use crate::{CLOCK_FELL_BACK, COUNTERS, RATES, SYS_CLOCK};
use alloc::string::String;
use core::fmt::Write;
use core::sync::atomic::Ordering::Relaxed;
use embassy_futures::select::{Either, select};
use embassy_time::{Instant, Timer};
use tnc2_node::setup::{Alias, Call, Command, Key, LineEditor, parse, rom_defaults};
use tnc2_node::status::{Drops, Status};
use tnc2_pico::{clock, usb};

const PROMPT: &str = "pico> ";

const HELP: &str = "\
Commands (upper or lower case):\r\n\
  MYCALL <call>   set the node's callsign, e.g. MYCALL G9TNT-1\r\n\
  ALIAS <alias>   set the node's alias, e.g. ALIAS TNET\r\n\
  MYCALL, ALIAS   on their own, show them\r\n\
  STATUS          how the node is getting on\r\n\
  HELP            this list\r\n\
Changing MYCALL or ALIAS saves it and cold starts TheNet, as fitting a new\r\n\
EPROM would: its RAM (routes, PARMS changes) starts afresh.\r\n\
Everything else is set on the other serial port, TheNet's own console.\r\n";

/// The ROM's own callsign and alias, used until MYCALL and ALIAS are set.
#[derive(Clone, Copy)]
struct Defaults {
    call: Option<Call>,
    alias: Option<Alias>,
}

#[embassy_executor::task]
pub async fn run(mut port: usb::Port, reset: &'static str) {
    let (call, alias) = rom_defaults(tnc2_pico::rom::lower());
    let rom = Defaults { call, alias };
    let mut ed = LineEditor::<64>::new();
    let mut open = false;
    let mut buf = [0u8; usb::PACKET];
    loop {
        // Say hello when a terminal opens the port.
        if port.dtr() && !open {
            let mut out = String::new();
            banner(&mut out, rom);
            out.push_str(PROMPT);
            let _ = usb::send(&mut port, out.as_bytes()).await;
        }
        open = port.dtr();
        match select(port.read_packet(&mut buf), Timer::after_millis(250)).await {
            Either::First(Ok(n)) => {
                let mut out = String::new();
                for &b in &buf[..n] {
                    match ed.key(b) {
                        Key::None => {}
                        Key::Echo(c) => out.push(c as char),
                        Key::Rubout => out.push_str("\x08 \x08"),
                        Key::Bell => out.push('\x07'),
                        Key::Line => {
                            out.push_str("\r\n");
                            command(&mut out, ed.line(), rom, reset).await;
                            out.push_str(PROMPT);
                        }
                    }
                }
                let _ = usb::send(&mut port, out.as_bytes()).await;
            }
            Either::First(Err(_)) => port.wait_connection().await,
            Either::Second(()) => {}
        }
    }
}

fn banner(out: &mut String, rom: Defaults) {
    let _ = write!(
        out,
        "\r\ntnc2-pico node {}: the Pico's own port. TheNet's console is the other serial port.\r\n",
        env!("CARGO_PKG_VERSION")
    );
    if manager::info().is_some_and(|i| i.settings.call.is_none()) {
        let _ = write!(
            out,
            "MYCALL is not set, so TheNet is using the ROM's own callsign, {}. Set yours with MYCALL <call>.\r\n",
            name(rom.call)
        );
    }
    out.push_str("Type HELP for the commands.\r\n");
}

fn name<T: core::fmt::Display>(v: Option<T>) -> String {
    let mut s = String::new();
    match v {
        Some(v) => {
            let _ = write!(s, "{v}");
        }
        None => s.push_str("(unknown)"),
    }
    s
}

async fn command(out: &mut String, line: &str, rom: Defaults, reset: &'static str) {
    let Some(info) = manager::info() else {
        out.push_str("Still starting up; try again in a moment.\r\n");
        return;
    };
    let _ = match parse(line) {
        Command::Empty => Ok(()),
        Command::Help => {
            out.push_str(HELP);
            Ok(())
        }
        Command::Status => status(out, rom, reset),
        Command::ShowCall => match info.settings.call {
            Some(c) => write!(out, "MYCALL {c}\r\n"),
            None => write!(
                out,
                "MYCALL is not set: TheNet is using the ROM's own, {}. Set yours with MYCALL <call>, e.g. MYCALL G9TNT-1.\r\n",
                name(rom.call)
            ),
        },
        Command::ShowAlias => match info.settings.alias {
            Some(a) => write!(out, "ALIAS {a}\r\n"),
            None => write!(
                out,
                "ALIAS is not set: TheNet is using the ROM's own, {}. Set yours with ALIAS <alias>, e.g. ALIAS TNET.\r\n",
                name(rom.alias)
            ),
        },
        Command::SetCall(c) => {
            let r = change(Change::Call(c)).await;
            reply(out, r, "MYCALL", c.as_str())
        }
        Command::SetAlias(a) => {
            let r = change(Change::Alias(a)).await;
            reply(out, r, "ALIAS", a.as_str())
        }
        Command::Bad(why) => write!(out, "No: {why}.\r\n"),
        Command::Unknown(_) => write!(
            out,
            "Not a command here; type HELP. (TheNet's own commands go on the other serial port.)\r\n"
        ),
    };
}

async fn change(c: Change) -> Reply {
    CHANGES.send(c).await;
    REPLIES.receive().await
}

fn reply(out: &mut String, r: Reply, what: &str, value: &str) -> core::fmt::Result {
    match r {
        Reply::Unchanged => write!(out, "{what} is already {value}.\r\n"),
        Reply::Saved => write!(
            out,
            "{what} {value} saved. TheNet has cold started with it, as with a new EPROM.\r\n"
        ),
        Reply::Failed => write!(
            out,
            "Could not save {what}: the flash did not read back the same. Nothing has changed; try again.\r\n"
        ),
    }
}

fn status(out: &mut String, rom: Defaults, reset: &'static str) -> core::fmt::Result {
    let Some(info) = manager::info() else {
        return Ok(());
    };
    let c = &COUNTERS;
    let now = Instant::now();
    let s = Status {
        version: env!("CARGO_PKG_VERSION"),
        call: info.settings.call,
        alias: info.settings.alias,
        rom_call: rom.call,
        rom_alias: rom.alias,
        uptime_s: now.as_secs(),
        rates: RATES.lock(|r| *r.borrow()),
        behind_ms: c.behind_ms.load(Relaxed),
        frames_in: c.frames_in.load(Relaxed),
        frames_out: c.frames_out.load(Relaxed),
        drops: Drops {
            line_errors: RX.line_errors.load(Relaxed),
            too_long: RX.too_long.load(Relaxed),
            queue_full: RX.queue_full.load(Relaxed),
            refused: c.frames_refused.load(Relaxed) + c.frames_no_board.load(Relaxed),
            out: c.frames_out_dropped.load(Relaxed) + TX_TOO_LONG.load(Relaxed),
            console: c.console_dropped.load(Relaxed),
        },
        start: info.start,
        last_save: info.last_save.map(|(g, at)| (g, (now - at).as_secs())),
        save_failures: info.failures,
        next_save_s: info.next_save.saturating_duration_since(now).as_secs(),
        clock_mhz: clock::current().mhz,
        clock_failed_mhz: CLOCK_FELL_BACK.load(Relaxed).then_some(SYS_CLOCK.mhz),
        counted_khz: clock::measure_sys_khz(),
        core_mv: clock::core_mv(),
        reset,
    };
    s.write(out)
}
