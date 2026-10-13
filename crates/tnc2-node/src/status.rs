//! The STATUS report on the Pico's own serial port. Plain ASCII, CR LF
//! line ends.

use crate::pace::Rates;
use crate::setup::{Alias, Call};
use core::fmt::{self, Write};

/// How the node started this time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    /// From the RAM saved in this generation.
    Warm(u32),
    /// Nothing saved yet.
    ColdEmpty,
    /// The newest save was a MYCALL or ALIAS change.
    ColdSetup,
    /// The saved RAM did not match the ROM (a firmware with other EPROM
    /// images, say).
    ColdMismatch,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Drops {
    /// Frames from the NinoTNC hit by a serial error or lost bytes.
    pub line_errors: u32,
    /// Frames too long for TheNet's receive buffer.
    pub too_long: u32,
    /// Frames lost because the queue to core 0 was full.
    pub queue_full: u32,
    /// Frames TheNet's receive queue had no room for.
    pub refused: u32,
    /// Frames TheNet sent that were dropped on the way to the NinoTNC.
    pub out: u32,
    /// Console bytes from TheNet nobody read in time.
    pub console: u32,
}

pub struct Status<'a> {
    pub version: &'a str,
    pub call: Option<Call>,
    pub alias: Option<Alias>,
    /// What the ROM uses when they are not set.
    pub rom_call: Option<Call>,
    pub rom_alias: Option<Alias>,
    pub uptime_s: u64,
    pub rates: Option<Rates>,
    pub behind_ms: u32,
    pub frames_in: u32,
    pub frames_out: u32,
    pub drops: Drops,
    pub start: Start,
    /// The last save this run: generation and seconds ago.
    pub last_save: Option<(u32, u64)>,
    pub save_failures: u32,
    pub next_save_s: u64,
    pub clock_mhz: u32,
    pub counted_khz: u32,
    pub core_mv: u32,
    pub reset: &'a str,
}

struct Dur(u64);

impl fmt::Display for Dur {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = self.0;
        match s {
            0..60 => write!(f, "{s} s"),
            60..3600 => write!(f, "{} min {} s", s / 60, s % 60),
            3600..86400 => write!(f, "{} h {} min", s / 3600, s % 3600 / 60),
            _ => write!(f, "{} d {} h", s / 86400, s % 86400 / 3600),
        }
    }
}

impl Status<'_> {
    pub fn write(&self, o: &mut impl Write) -> fmt::Result {
        write!(o, "tnc2-pico node {}: TheNet X1J on an emulated TNC-2\r\n", self.version)?;
        match self.call {
            Some(c) => write!(o, "Callsign:   {c}\r\n")?,
            None => write!(o, "Callsign:   not set, so the ROM's own: {} (set yours with MYCALL)\r\n", or_none(self.rom_call))?,
        }
        match self.alias {
            Some(a) => write!(o, "Alias:      {a}\r\n")?,
            None => write!(o, "Alias:      not set, so the ROM's own: {} (set yours with ALIAS)\r\n", or_none(self.rom_alias))?,
        }
        write!(o, "Uptime:     {}\r\n", Dur(self.uptime_s))?;
        match self.rates {
            Some(r) => {
                write!(o, "Speed:      {:.2}x real time, core 0 busy {:.0}%", r.speed, r.busy * 100.0)?;
                if let Some(max) = r.flat_out() {
                    write!(o, " (could run {max:.2}x)")?;
                }
                if self.behind_ms >= 1000 {
                    write!(o, ", {} behind", Dur(self.behind_ms as u64 / 1000))?;
                }
                write!(o, "\r\n")?;
            }
            None => write!(o, "Speed:      still measuring\r\n")?,
        }
        write!(o, "Radio:      {} frames in, {} out\r\n", self.frames_in, self.frames_out)?;
        let d = &self.drops;
        write!(
            o,
            "Dropped:    {} with serial errors, {} too long, {} queue full, {} refused by TheNet, {} on the way out; {} console bytes\r\n",
            d.line_errors, d.too_long, d.queue_full, d.refused, d.out, d.console
        )?;
        match self.start {
            Start::Warm(g) => write!(o, "Start:      warm, from the RAM saved as generation {g}\r\n")?,
            Start::ColdEmpty => write!(o, "Start:      cold, nothing saved yet\r\n")?,
            Start::ColdSetup => write!(o, "Start:      cold, after a MYCALL or ALIAS change\r\n")?,
            Start::ColdMismatch => write!(o, "Start:      cold, the saved RAM was for other EPROM images\r\n")?,
        }
        match self.last_save {
            Some((g, ago)) => write!(o, "Last save:  generation {g}, {} ago", Dur(ago))?,
            None => write!(o, "Last save:  none since start up")?,
        }
        write!(o, "; next check in {}", Dur(self.next_save_s))?;
        if self.save_failures > 0 {
            write!(o, "; {} failed (flash did not read back the same)", self.save_failures)?;
        }
        write!(o, "\r\n")?;
        write!(
            o,
            "Clock:      {} MHz (counted {}.{} MHz), core {}.{:02} V\r\n",
            self.clock_mhz,
            self.counted_khz / 1000,
            self.counted_khz % 1000 / 100,
            self.core_mv / 1000,
            self.core_mv % 1000 / 10
        )?;
        write!(o, "Last reset: {}\r\n", self.reset)
    }
}

fn or_none<T: fmt::Display>(v: Option<T>) -> impl fmt::Display {
    struct D<T>(Option<T>);
    impl<T: fmt::Display> fmt::Display for D<T> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match &self.0 {
                Some(v) => v.fmt(f),
                None => f.write_str("unknown"),
            }
        }
    }
    D(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::{parse_alias, parse_call};
    use std::string::String;

    fn base() -> Status<'static> {
        Status {
            version: "0.1.0",
            call: Some(parse_call("G9TNT-1").unwrap()),
            alias: Some(parse_alias("TNET").unwrap()),
            rom_call: Some(parse_call("G8KBB-5").unwrap()),
            rom_alias: Some(parse_alias("THENET").unwrap()),
            uptime_s: 3 * 86400 + 4 * 3600 + 5,
            rates: Some(Rates { speed: 1.0, busy: 0.78 }),
            behind_ms: 0,
            frames_in: 12,
            frames_out: 34,
            drops: Drops { line_errors: 1, ..Default::default() },
            start: Start::Warm(41),
            last_save: Some((42, 192)),
            save_failures: 0,
            next_save_s: 408,
            clock_mhz: 150,
            counted_khz: 150_001,
            core_mv: 1100,
            reset: "power on",
        }
    }

    fn text(s: &Status) -> String {
        let mut out = String::new();
        s.write(&mut out).unwrap();
        assert!(out.is_ascii());
        assert!(out.ends_with("\r\n"));
        out
    }

    #[test]
    fn report() {
        let t = text(&base());
        assert!(t.contains("Callsign:   G9TNT-1\r\n"), "{t}");
        assert!(t.contains("Uptime:     3 d 4 h\r\n"), "{t}");
        assert!(t.contains("Speed:      1.00x real time, core 0 busy 78% (could run 1.28x)\r\n"), "{t}");
        assert!(t.contains("Radio:      12 frames in, 34 out\r\n"), "{t}");
        assert!(t.contains("1 with serial errors"), "{t}");
        assert!(t.contains("warm, from the RAM saved as generation 41"), "{t}");
        assert!(t.contains("Last save:  generation 42, 3 min 12 s ago; next check in 6 min 48 s\r\n"), "{t}");
        assert!(t.contains("Clock:      150 MHz (counted 150.0 MHz), core 1.10 V\r\n"), "{t}");
        assert!(t.contains("Last reset: power on\r\n"), "{t}");
    }

    #[test]
    fn unset_and_behind() {
        let s = Status {
            call: None,
            alias: None,
            rates: Some(Rates { speed: 0.9, busy: 1.0 }),
            behind_ms: 65_000,
            start: Start::ColdEmpty,
            last_save: None,
            save_failures: 2,
            ..base()
        };
        let t = text(&s);
        assert!(t.contains("not set, so the ROM's own: G8KBB-5 (set yours with MYCALL)"), "{t}");
        assert!(t.contains("not set, so the ROM's own: THENET (set yours with ALIAS)"), "{t}");
        assert!(t.contains("0.90x real time, core 0 busy 100% (could run 0.90x), 1 min 5 s behind"), "{t}");
        assert!(t.contains("none since start up"), "{t}");
        assert!(t.contains("2 failed"), "{t}");
        let s = Status { rates: None, start: Start::ColdSetup, ..base() };
        assert!(text(&s).contains("still measuring"));
        let s = Status { start: Start::ColdMismatch, rom_call: None, call: None, ..base() };
        assert!(text(&s).contains("ROM's own: unknown"));
    }
}
