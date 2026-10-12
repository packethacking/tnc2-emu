//! Just enough AX.25 / NET/ROM decoding to print a readable monitor line.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

fn addr(b: &[u8]) -> String {
    let call: String = b[..6]
        .iter()
        .map(|c| (c >> 1) as char)
        .collect::<String>()
        .trim_end()
        .to_string();
    let ssid = (b[6] >> 1) & 0x0f;
    if ssid == 0 {
        call
    } else {
        format!("{call}-{ssid}")
    }
}

fn ctl_name(c: u8) -> String {
    if c & 1 == 0 {
        format!("I{}{}", (c >> 5) & 7, (c >> 1) & 7)
    } else if c & 3 == 1 {
        let n = (c >> 5) & 7;
        match c & 0x0f {
            0x01 => format!("RR{n}"),
            0x05 => format!("RNR{n}"),
            0x09 => format!("REJ{n}"),
            _ => format!("S?{c:02x}"),
        }
    } else {
        match c & !0x10 {
            0x03 => "UI".into(),
            0x2f => "SABM".into(),
            0x43 => "DISC".into(),
            0x0f => "DM".into(),
            0x63 => "UA".into(),
            0x87 => "FRMR".into(),
            _ => format!("U?{c:02x}"),
        }
    }
}

fn printable(b: &[u8]) -> String {
    b.iter()
        .map(|&c| {
            if c == b'\r' {
                "\\r".to_string()
            } else if (0x20..0x7f).contains(&c) {
                (c as char).to_string()
            } else {
                format!("<{c:02x}>")
            }
        })
        .collect()
}

/// One line (or a few) describing the frame, plain ASCII.
pub fn describe(f: &[u8]) -> String {
    if f.len() < 15 {
        return format!("short frame ({} bytes)", f.len());
    }
    let mut i = 0;
    let mut addrs = Vec::new();
    loop {
        if i + 7 > f.len() {
            return format!("bad address field ({} bytes)", f.len());
        }
        addrs.push(&f[i..i + 7]);
        let last = f[i + 6] & 1 != 0;
        i += 7;
        if last {
            break;
        }
    }
    if addrs.len() < 2 || i >= f.len() {
        return "bad address field".into();
    }
    let dst = &addrs[0];
    let src = &addrs[1];
    let cmd = if dst[6] & 0x80 != 0 && src[6] & 0x80 == 0 {
        "C"
    } else if dst[6] & 0x80 == 0 && src[6] & 0x80 != 0 {
        "R"
    } else {
        "v1"
    };
    let mut s = format!("{}>{}", addr(src), addr(dst));
    for d in &addrs[2..] {
        s.push(',');
        s.push_str(&addr(d));
        if d[6] & 0x80 != 0 {
            s.push('*');
        }
    }
    let c = f[i];
    i += 1;
    let pf = if c & 0x10 != 0 { if cmd == "C" { " P" } else { " F" } } else { "" };
    s.push_str(&format!(" <{} {}{}>", ctl_name(c), cmd, pf));
    let has_pid = c & 1 == 0 || (c & !0x10) == 0x03;
    if has_pid && i < f.len() {
        let pid = f[i];
        i += 1;
        let info = &f[i..];
        s.push_str(&format!(" pid={pid:02X} len={}", info.len()));
        if pid == 0xCF && !info.is_empty() && info[0] == 0xFF && (c & !0x10) == 0x03 {
            s.push_str(&nodes(info));
        } else if pid == 0xCF {
            s.push_str(&netrom(info));
        } else if !info.is_empty() {
            s.push_str(&format!(" \"{}\"", printable(info)));
        }
    }
    s
}

fn alias(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect::<String>().trim_end().to_string()
}

fn nodes(info: &[u8]) -> String {
    let mut s = String::new();
    if info.len() < 7 {
        return " NODES (short)".into();
    }
    s.push_str(&format!(" NODES from alias '{}'", alias(&info[1..7])));
    let mut i = 7;
    while i + 21 <= info.len() {
        let e = &info[i..i + 21];
        s.push_str(&format!(
            "\n    {}:{} via {} qual {}",
            alias(&e[7..13]),
            addr(&e[0..7]),
            addr(&e[13..20]),
            e[20]
        ));
        i += 21;
    }
    if i != info.len() {
        s.push_str(&format!("\n    ({} trailing bytes)", info.len() - i));
    }
    s
}

fn netrom(info: &[u8]) -> String {
    if info.len() < 20 {
        return format!(" NET/ROM short ({} bytes)", info.len());
    }
    let src = addr(&info[0..7]);
    let dst = addr(&info[7..14]);
    let ttl = info[14];
    let op = info[19];
    let opname = match op & 0x0f {
        1 => "CONN REQ",
        2 => "CONN ACK",
        3 => "DISC REQ",
        4 => "DISC ACK",
        5 => "INFO",
        6 => "INFO ACK",
        _ => "OP?",
    };
    let mut s = format!(
        " NET/ROM {src}>{dst} ttl={ttl} {opname} ci={:02X}{:02X} {:02X}{:02X} op={op:02X}",
        info[15], info[16], info[17], info[18]
    );
    if op & 0x0f == 5 {
        s.push_str(&format!(" \"{}\"", printable(&info[20..])));
    }
    if op & 0x0f == 1 && info.len() >= 35 {
        s.push_str(&format!(
            " win={} user={} node={}",
            info[20],
            addr(&info[21..28]),
            addr(&info[28..35])
        ));
    }
    s
}
