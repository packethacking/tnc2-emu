//! The Pico's own serial port: MYCALL, ALIAS, STATUS and HELP, typed at a
//! simple line prompt.
//!
//! The callsign and alias get the checks the Linux binary's `--call` and
//! `--alias` get from `tnc2_core::rom::patch` (up to 6 characters, an SSID
//! from 0 to 15), plus two of its own: a callsign is letters and digits
//! only, as AX.25 addresses are, and an alias is printable ASCII, so that
//! everything the port prints stays plain ASCII. Both are kept upper
//! case, as the ROM patch makes them.

use core::fmt;

/// A callsign or alias: 1 to `N` printable ASCII bytes, upper case.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Name<const N: usize> {
    len: u8,
    bytes: [u8; N],
}

impl<const N: usize> Name<N> {
    /// `s` upper cased, if it is 1 to `N` printable ASCII characters with
    /// no spaces.
    pub fn new(s: &str) -> Option<Self> {
        if s.is_empty() || s.len() > N || !s.bytes().all(|b| b.is_ascii_graphic()) {
            return None;
        }
        let mut bytes = [0; N];
        bytes[..s.len()].copy_from_slice(s.as_bytes());
        bytes.make_ascii_uppercase();
        Some(Name { len: s.len() as u8, bytes })
    }

    pub fn as_str(&self) -> &str {
        // Only ever built from ASCII.
        core::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("")
    }
}

impl<const N: usize> fmt::Display for Name<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<const N: usize> fmt::Debug for Name<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

/// A callsign with its SSID, "G9TNT-15" at the longest.
pub type Call = Name<9>;
/// A node alias.
pub type Alias = Name<6>;

/// Check a callsign and put it in its usual form: upper case, no "-0",
/// no leading zeros in the SSID.
pub fn parse_call(s: &str) -> Result<Call, &'static str> {
    const BAD: &str = "a callsign is 1 to 6 letters and digits, with an optional SSID from 0 to 15, e.g. G9TNT-1";
    let (base, ssid) = match s.split_once('-') {
        Some((b, n)) if !n.is_empty() && n.len() <= 2 && n.bytes().all(|c| c.is_ascii_digit()) => {
            (b, n.parse::<u8>().map_err(|_| BAD)?)
        }
        Some(_) => return Err(BAD),
        None => (s, 0),
    };
    if base.is_empty() || base.len() > 6 || !base.bytes().all(|c| c.is_ascii_alphanumeric()) || ssid > 15 {
        return Err(BAD);
    }
    let mut buf = [0u8; 9];
    buf[..base.len()].copy_from_slice(base.as_bytes());
    let mut len = base.len();
    if ssid > 0 {
        buf[len] = b'-';
        len += 1;
        if ssid >= 10 {
            buf[len] = b'1';
            len += 1;
        }
        buf[len] = b'0' + ssid % 10;
        len += 1;
    }
    Call::new(core::str::from_utf8(&buf[..len]).map_err(|_| BAD)?).ok_or(BAD)
}

/// Check an alias.
pub fn parse_alias(s: &str) -> Result<Alias, &'static str> {
    Alias::new(s).ok_or("an alias is 1 to 6 printable characters with no spaces, e.g. TNET")
}

/// The callsign and alias the unpatched ROM carries in its configuration
/// block (docs/CONFIGUR.TXT): what the node uses until MYCALL and ALIAS
/// are set.
pub fn rom_defaults(rom: &[u8]) -> (Option<Call>, Option<Alias>) {
    fn text(b: &[u8]) -> Option<&str> {
        core::str::from_utf8(b).ok().map(|s| s.trim_end_matches(' '))
    }
    let call = rom.get(0x3B..0x42).and_then(|b| {
        // Six characters padded with spaces, then the SSID byte; spell it
        // out as "-NN" and let parse_call tidy it up.
        let base = text(&b[..6])?;
        let ssid = (b[6] >> 1) & 0x0F;
        let mut buf = [0u8; 9];
        let n = base.len();
        buf[..n].copy_from_slice(base.as_bytes());
        buf[n..n + 3].copy_from_slice(&[b'-', b'0' + ssid / 10, b'0' + ssid % 10]);
        parse_call(core::str::from_utf8(&buf[..n + 3]).ok()?).ok()
    });
    let alias = rom.get(0x42..0x48).and_then(text).and_then(|s| parse_alias(s).ok());
    (call, alias)
}

/// One line typed at the prompt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command<'a> {
    /// Just Enter.
    Empty,
    Help,
    Status,
    /// MYCALL on its own: show it.
    ShowCall,
    SetCall(Call),
    /// ALIAS on its own: show it.
    ShowAlias,
    SetAlias(Alias),
    /// A known command used wrongly, and what to say about it.
    Bad(&'static str),
    /// Not a command.
    Unknown(&'a str),
}

/// Make sense of a line. Commands are not case sensitive.
pub fn parse(line: &str) -> Command<'_> {
    let mut words = line.split_ascii_whitespace();
    let Some(cmd) = words.next() else {
        return Command::Empty;
    };
    let arg = words.next();
    let extra = words.next().is_some();
    let is = |name: &str| cmd.eq_ignore_ascii_case(name);
    if is("HELP") || cmd == "?" {
        Command::Help
    } else if is("STATUS") {
        if arg.is_some() { Command::Bad("STATUS takes nothing after it") } else { Command::Status }
    } else if is("MYCALL") {
        match (arg, extra) {
            (None, _) => Command::ShowCall,
            (Some(_), true) => Command::Bad("MYCALL takes one callsign, e.g. MYCALL G9TNT-1"),
            (Some(a), false) => parse_call(a).map_or_else(Command::Bad, Command::SetCall),
        }
    } else if is("ALIAS") {
        match (arg, extra) {
            (None, _) => Command::ShowAlias,
            (Some(_), true) => Command::Bad("ALIAS takes one alias, e.g. ALIAS TNET"),
            (Some(a), false) => parse_alias(a).map_or_else(Command::Bad, Command::SetAlias),
        }
    } else {
        Command::Unknown(cmd)
    }
}

/// What the terminal should be sent after a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// Nothing.
    None,
    /// Echo this character.
    Echo(u8),
    /// Rub out the last character on screen.
    Rubout,
    /// The line is full: ring the bell.
    Bell,
    /// Enter: the line is ready in `LineEditor::line`.
    Line,
}

/// A line editor for a dumb terminal: printable characters, Backspace or
/// Delete, and Enter as CR, LF or CR LF.
pub struct LineEditor<const N: usize> {
    buf: [u8; N],
    len: usize,
    done: bool,
    last_cr: bool,
}

impl<const N: usize> Default for LineEditor<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> LineEditor<N> {
    pub const fn new() -> Self {
        LineEditor { buf: [0; N], len: 0, done: false, last_cr: false }
    }

    pub fn key(&mut self, b: u8) -> Key {
        if self.done {
            self.len = 0;
            self.done = false;
        }
        let after_cr = core::mem::replace(&mut self.last_cr, b == b'\r');
        match b {
            b'\n' if after_cr => Key::None,
            b'\r' | b'\n' => {
                self.done = true;
                Key::Line
            }
            0x08 | 0x7F if self.len > 0 => {
                self.len -= 1;
                Key::Rubout
            }
            0x20..=0x7E if self.len < N => {
                self.buf[self.len] = b;
                self.len += 1;
                Key::Echo(b)
            }
            0x20..=0x7E => Key::Bell,
            _ => Key::None,
        }
    }

    /// The line just finished with Enter.
    pub fn line(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callsigns() {
        let ok = |s: &str| std::string::String::from(parse_call(s).unwrap().as_str());
        assert_eq!(ok("g9tnt-1"), "G9TNT-1");
        assert_eq!(ok("G9TNT"), "G9TNT");
        assert_eq!(ok("G9TNT-0"), "G9TNT");
        assert_eq!(ok("G9TNT-07"), "G9TNT-7");
        assert_eq!(ok("M0ABCD-15"), "M0ABCD-15");
        assert_eq!(ok("A"), "A");
        for bad in ["", "-1", "G9TNT-", "G9TNT-16", "G9TNT-1-2", "G9TOOLONG", "G9TNT-X", "G9/P", "G9TNT-123", "G9TNT- 1"] {
            assert!(parse_call(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn aliases() {
        assert_eq!(parse_alias("tnet").unwrap().as_str(), "TNET");
        assert_eq!(parse_alias("#HIDE").unwrap().as_str(), "#HIDE");
        assert!(parse_alias("").is_err());
        assert!(parse_alias("SEVENCH").is_err());
        assert!(parse_alias("A B").is_err());
        assert!(parse_alias("caf\u{e9}").is_err());
    }

    #[test]
    fn accepted_names_patch_like_the_linux_binary() {
        // Whatever the parser accepts, the ROM patch accepts too, and
        // writes the same bytes as for the text as typed.
        use tnc2_core::rom::{NodeConfig, pad_half, patch};
        for (typed, call) in [("g9tnt-0", "G9TNT"), ("g9tnt-07", "G9TNT-7"), ("m0abcd-15", "M0ABCD-15")] {
            let mut a = pad_half(&[]).unwrap();
            let mut b = pad_half(&[]).unwrap();
            let parsed = parse_call(typed).unwrap();
            assert_eq!(parsed.as_str(), call);
            patch(&mut a, &NodeConfig { call: Some(typed), alias: Some("tnet"), pokes: &[] }).unwrap();
            let alias = parse_alias("tnet").unwrap();
            patch(&mut b, &NodeConfig { call: Some(parsed.as_str()), alias: Some(alias.as_str()), pokes: &[] }).unwrap();
            assert_eq!(a[..0x48], b[..0x48]);
        }
    }

    #[test]
    fn rom_default_is_g8kbb() {
        let (call, alias) = rom_defaults(include_bytes!("../../../rom/THENET1.X1J"));
        assert_eq!(call.unwrap().as_str(), "G8KBB-5");
        assert_eq!(alias.unwrap().as_str(), "THENET");
        let (call, alias) = rom_defaults(include_bytes!("../../../rom/THENET2.X1J"));
        assert_eq!((call.unwrap().as_str(), alias.unwrap().as_str()), ("G8KBB-5", "THENET"));
        assert_eq!(rom_defaults(&[]), (None, None));
    }

    #[test]
    fn commands() {
        assert_eq!(parse(""), Command::Empty);
        assert_eq!(parse("   "), Command::Empty);
        assert_eq!(parse("help"), Command::Help);
        assert_eq!(parse("?"), Command::Help);
        assert_eq!(parse("Status"), Command::Status);
        assert!(matches!(parse("status now"), Command::Bad(_)));
        assert_eq!(parse("mycall"), Command::ShowCall);
        assert_eq!(parse(" MYCALL  g9tnt-1 "), Command::SetCall(parse_call("G9TNT-1").unwrap()));
        assert!(matches!(parse("MYCALL G9TNT-99"), Command::Bad(_)));
        assert!(matches!(parse("MYCALL G9TNT TNET"), Command::Bad(_)));
        assert_eq!(parse("alias"), Command::ShowAlias);
        assert_eq!(parse("alias tnet"), Command::SetAlias(parse_alias("TNET").unwrap()));
        assert!(matches!(parse("ALIAS TOOLONG"), Command::Bad(_)));
        assert_eq!(parse("PARMS"), Command::Unknown("PARMS"));
        for line in ["", "status x", "MYCALL G9TNT-99", "MYCALL a b", "ALIAS TOOLONG", "ALIAS a b"] {
            if let Command::Bad(m) = parse(line) {
                assert!(m.is_ascii());
            }
        }
    }

    fn typed<const N: usize>(ed: &mut LineEditor<N>, keys: &[u8]) -> (std::vec::Vec<Key>, std::string::String) {
        let mut out = std::vec::Vec::new();
        let mut line = std::string::String::new();
        for &k in keys {
            let r = ed.key(k);
            if r == Key::Line {
                line = ed.line().into();
            }
            out.push(r);
        }
        (out, line)
    }

    #[test]
    fn line_editing() {
        let mut ed = LineEditor::<8>::new();
        let (keys, line) = typed(&mut ed, b"mycz\x08all\r\n");
        assert_eq!(line, "mycall");
        assert_eq!(keys[4], Key::Rubout);
        assert_eq!(keys[8], Key::Line);
        assert_eq!(keys[9], Key::None, "LF after CR is not a second Enter");
        // the next key starts a new line; LF alone is Enter too
        let (_, line) = typed(&mut ed, b"ab\n");
        assert_eq!(line, "ab");
        let (_, line) = typed(&mut ed, b"\n");
        assert_eq!(line, "");
        // rubout on an empty line, control characters and non-ASCII do nothing
        let (keys, _) = typed(&mut ed, b"\x08\x7f\x01\x1b\xc3");
        assert!(keys.iter().all(|k| *k == Key::None));
        // a full line rings the bell and keeps what fits
        let (keys, line) = typed(&mut ed, b"123456789\r");
        assert_eq!(keys[8], Key::Bell);
        assert_eq!(line, "12345678");
    }
}
