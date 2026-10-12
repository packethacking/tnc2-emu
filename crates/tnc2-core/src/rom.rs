//! The two EPROM halves, the configuration block at the start of each, and
//! the fingerprint that ties a saved RAM image to one ROM configuration.

use crate::{RAM_SIZE, ROM_HALF};
use alloc::boxed::Box;
use alloc::vec;

/// One 32 KB half of the 27512 EPROM.
pub type RomHalf = Box<[u8; ROM_HALF]>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RomError {
    /// The image is larger than 32 KB.
    TooLarge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// Callsign longer than 6 characters, or SSID not 0 to 15.
    BadCall,
    /// Alias longer than 6 characters.
    BadAlias,
    /// A poke runs past the end of the ROM half.
    PokeOutOfRange,
}

/// Pad a ROM file to 32 KB. The distribution files have the trailing
/// erased (0xFF) bytes trimmed.
pub fn pad_half(data: &[u8]) -> Result<RomHalf, RomError> {
    if data.len() > ROM_HALF {
        return Err(RomError::TooLarge);
    }
    let mut v = vec![0xFF; ROM_HALF];
    v[..data.len()].copy_from_slice(data);
    Ok(v.into_boxed_slice().try_into().expect("32 KB"))
}

/// Bytes written over the configuration block at `offset`.
#[derive(Clone, Copy, Debug)]
pub struct Poke<'a> {
    pub offset: usize,
    pub bytes: &'a [u8],
}

/// What to patch into the configuration block (docs/CONFIGUR.TXT).
#[derive(Clone, Copy, Debug, Default)]
pub struct NodeConfig<'a> {
    /// Callsign with optional SSID, e.g. "G9TNT-1".
    pub call: Option<&'a str>,
    /// Alias, up to 6 characters.
    pub alias: Option<&'a str>,
    pub pokes: &'a [Poke<'a>],
}

/// Patch the configuration block that sits at the start of a half. Both
/// halves carry a copy and both must be patched the same way.
pub fn patch(rom: &mut [u8; ROM_HALF], c: &NodeConfig) -> Result<(), ConfigError> {
    if let Some(call) = c.call {
        let (cs, ssid) = match call.split_once('-') {
            Some((a, b)) => (a, b.parse::<u8>().map_err(|_| ConfigError::BadCall)?),
            None => (call, 0),
        };
        if cs.len() > 6 || ssid > 15 {
            return Err(ConfigError::BadCall);
        }
        let mut f = [b' '; 6];
        f[..cs.len()].copy_from_slice(cs.as_bytes());
        f.make_ascii_uppercase();
        rom[0x3B..0x41].copy_from_slice(&f);
        rom[0x41] = 0x60 | (ssid << 1);
    }
    if let Some(alias) = c.alias {
        if alias.len() > 6 {
            return Err(ConfigError::BadAlias);
        }
        let mut f = [b' '; 6];
        f[..alias.len()].copy_from_slice(alias.as_bytes());
        f.make_ascii_uppercase();
        rom[0x42..0x48].copy_from_slice(&f);
    }
    for p in c.pokes {
        let end = p.offset.checked_add(p.bytes.len()).filter(|&e| e <= ROM_HALF);
        let Some(end) = end else {
            return Err(ConfigError::PokeOutOfRange);
        };
        rom[p.offset..end].copy_from_slice(p.bytes);
    }
    Ok(())
}

/// FNV-1a over both (patched) halves. A RAM image saved under one
/// fingerprint is only restored under the same one.
pub fn fingerprint(lower: &[u8], upper: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for d in [lower, upper] {
        for &b in d {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// Saved RAM image: this magic, the fingerprint (little endian), the RAM.
pub const RAM_MAGIC: &[u8; 8] = b"TNC2RAM1";
pub const RAM_IMAGE_LEN: usize = 16 + RAM_SIZE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RamImageError {
    /// Wrong length or magic.
    NotImage,
    /// Saved with a different ROM configuration.
    OtherConfig,
}

/// The 16 byte header that goes in front of the RAM in a saved image.
pub fn ram_image_header(fingerprint: u64) -> [u8; 16] {
    let mut h = [0; 16];
    h[..8].copy_from_slice(RAM_MAGIC);
    h[8..].copy_from_slice(&fingerprint.to_le_bytes());
    h
}

/// Check a saved image and return the RAM in it.
pub fn check_ram_image(data: &[u8], fingerprint: u64) -> Result<&[u8; RAM_SIZE], RamImageError> {
    if data.len() != RAM_IMAGE_LEN || &data[..8] != RAM_MAGIC {
        return Err(RamImageError::NotImage);
    }
    let fp = u64::from_le_bytes(data[8..16].try_into().unwrap());
    if fp != fingerprint {
        return Err(RamImageError::OtherConfig);
    }
    Ok(data[16..].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    static ROM1: &[u8] = include_bytes!("../../../rom/THENET1.X1J");
    static ROM2: &[u8] = include_bytes!("../../../rom/THENET2.X1J");

    fn patched(c: &NodeConfig) -> u64 {
        let mut lo = pad_half(ROM1).unwrap();
        let mut hi = pad_half(ROM2).unwrap();
        patch(&mut lo, c).unwrap();
        patch(&mut hi, c).unwrap();
        fingerprint(&lo[..], &hi[..])
    }

    #[test]
    fn pads_with_erased_bytes() {
        let h = pad_half(&[1, 2, 3]).unwrap();
        assert_eq!(&h[..4], &[1, 2, 3, 0xFF]);
        assert_eq!(h[ROM_HALF - 1], 0xFF);
        assert_eq!(pad_half(&vec![0; ROM_HALF + 1]), Err(RomError::TooLarge));
    }

    #[test]
    fn patches_call_alias_and_pokes() {
        let mut h = pad_half(&[]).unwrap();
        let pokes = [Poke { offset: 0x54, bytes: &[0x0A, 0x00] }];
        let c = NodeConfig { call: Some("g9tnt-1"), alias: Some("tnet"), pokes: &pokes };
        patch(&mut h, &c).unwrap();
        assert_eq!(&h[0x3B..0x41], b"G9TNT ");
        assert_eq!(h[0x41], 0x62);
        assert_eq!(&h[0x42..0x48], b"TNET  ");
        assert_eq!(&h[0x54..0x56], &[0x0A, 0x00]);
        assert_eq!(h[0x56], 0xFF);
    }

    #[test]
    fn rejects_bad_config() {
        let mut h = pad_half(&[]).unwrap();
        let bad = |c: NodeConfig, h: &mut RomHalf| patch(h, &c).unwrap_err();
        assert_eq!(bad(NodeConfig { call: Some("G9TOOLONG"), ..Default::default() }, &mut h), ConfigError::BadCall);
        assert_eq!(bad(NodeConfig { call: Some("G9TNT-16"), ..Default::default() }, &mut h), ConfigError::BadCall);
        assert_eq!(bad(NodeConfig { call: Some("G9TNT-X"), ..Default::default() }, &mut h), ConfigError::BadCall);
        assert_eq!(bad(NodeConfig { alias: Some("SEVENCH"), ..Default::default() }, &mut h), ConfigError::BadAlias);
        let pokes = [Poke { offset: ROM_HALF - 1, bytes: &[0, 0] }];
        assert_eq!(bad(NodeConfig { pokes: &pokes, ..Default::default() }, &mut h), ConfigError::PokeOutOfRange);
    }

    // Fingerprints written by the emulator before the core split, so that
    // RAM images in existing /data volumes keep warm starting.
    #[test]
    fn fingerprints_match_saved_images() {
        assert_eq!(patched(&NodeConfig::default()), 0xbacc3872dbbe52aa);
        let pokes = [Poke { offset: 0x54, bytes: &[0x0A, 0x00] }];
        let c = NodeConfig { call: Some("N0CALL-7"), alias: Some("TEST"), pokes: &pokes };
        assert_eq!(patched(&c), 0xc79d74d7947dcbae);
    }

    #[test]
    fn ram_image_round_trip() {
        let mut img: Vec<u8> = ram_image_header(0x1122334455667788).to_vec();
        img.extend((0..RAM_SIZE).map(|i| i as u8));
        let ram = check_ram_image(&img, 0x1122334455667788).unwrap();
        assert_eq!(ram[0x1234], 0x34);
        assert_eq!(check_ram_image(&img, 1), Err(RamImageError::OtherConfig));
        assert_eq!(check_ram_image(&img[1..], 0x1122334455667788), Err(RamImageError::NotImage));
    }
}
