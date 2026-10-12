//! The battery backed RAM and the settings, kept in flash.
//!
//! The store is a ring of slots, each a whole number of 4 KB sectors. A
//! save always goes in the slot after the newest one, with a generation
//! one higher, so the newest good copy is never touched while the next is
//! written. Writing a slot erases it, programs the RAM pages, and programs
//! the header page last; the header carries the magic, the generation, the
//! settings and a CRC-32 over the header and the RAM. A power cut at any
//! moment leaves either the new slot complete or no valid header in it,
//! and at start up the valid slot with the highest generation wins. Every
//! write is read back and compared before the store believes it.
//!
//! A slot holds the settings (MYCALL and ALIAS) and, usually, a RAM image
//! (`tnc2_core::rom::ram_image_header` and the 32 KB). A slot without RAM
//! records a setup change: the node cold starts from it, as with a new
//! EPROM.
//!
//! Wear: with the firmware's 7 slots of 9 sectors (252 KB), each save
//! erases 9 sectors and a given sector is erased once every 7 saves.
//! docs/pico.md works out the life.

use crate::crc::Crc32;
use crate::setup::{Alias, Call, parse_alias, parse_call};
use tnc2_core::rom::RAM_IMAGE_LEN;

/// Erase unit.
pub const SECTOR: usize = 4096;
/// Program unit.
pub const PAGE: usize = 256;
/// Sectors in a slot: the header page and the RAM image, rounded up.
pub const SLOT_SECTORS: usize = 9;
pub const SLOT: usize = SECTOR * SLOT_SECTORS;

/// Pages of RAM image after the header page.
const RAM_PAGES: usize = RAM_IMAGE_LEN.div_ceil(PAGE);
const _: () = assert!(PAGE * (1 + RAM_PAGES) <= SLOT);
/// Pages programmed in one `Write::step`.
const PAGES_PER_STEP: usize = SECTOR / PAGE;

const MAGIC: &[u8; 4] = b"TNSL";
const VERSION: u16 = 1;
const HAS_RAM: u16 = 1;
/// Bytes of the header in use; the CRC is the last four.
const HEADER: usize = 64;

/// Flash as the store sees it: offsets from the start of the store.
pub trait Flash {
    fn read(&mut self, offset: usize, buf: &mut [u8]);
    /// Erase the sector at `offset` (a multiple of `SECTOR`) to 0xFF.
    fn erase_sector(&mut self, offset: usize);
    /// Program the page at `offset` (a multiple of `PAGE`).
    fn program_page(&mut self, offset: usize, data: &[u8; PAGE]);
}

/// MYCALL and ALIAS; `None` leaves the ROM's own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    pub call: Option<Call>,
    pub alias: Option<Alias>,
}

/// A valid slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub index: usize,
    pub generation: u32,
    pub settings: Settings,
    pub has_ram: bool,
}

/// A slot that was written but did not read back the same.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerifyFailed {
    pub index: usize,
}

pub struct Store {
    slots: usize,
    newest: Option<Slot>,
    /// Where the next write goes, and its generation.
    next: usize,
    next_gen: u32,
}

impl Store {
    /// Look through the `len` bytes of flash for the newest valid slot.
    pub fn open(flash: &mut impl Flash, len: usize) -> Store {
        let slots = len / SLOT;
        assert!(slots >= 2, "the store needs room for two slots");
        let mut newest: Option<Slot> = None;
        for index in 0..slots {
            if let Some(s) = check(flash, index)
                && newest.is_none_or(|n| s.generation > n.generation)
            {
                newest = Some(s);
            }
        }
        let next = newest.map_or(0, |n| (n.index + 1) % slots);
        let next_gen = newest.map_or(1, |n| n.generation.saturating_add(1));
        Store { slots, newest, next, next_gen }
    }

    pub fn slots(&self) -> usize {
        self.slots
    }

    /// The newest valid slot, if there is one.
    pub fn newest(&self) -> Option<Slot> {
        self.newest
    }

    /// Copy the newest slot's RAM image into `out`. False if it has none.
    pub fn read_ram(&self, flash: &mut impl Flash, out: &mut [u8; RAM_IMAGE_LEN]) -> bool {
        match self.newest {
            Some(s) if s.has_ram => {
                flash.read(s.index * SLOT + PAGE, out);
                true
            }
            _ => false,
        }
    }

    /// Whether the newest slot already holds exactly this RAM image, so
    /// saving it again would only wear the flash.
    pub fn holds(&self, flash: &mut impl Flash, image: &[u8; RAM_IMAGE_LEN]) -> bool {
        let Some(s) = self.newest.filter(|s| s.has_ram) else {
            return false;
        };
        let mut buf = [0u8; PAGE];
        image.chunks(PAGE).enumerate().all(|(i, want)| {
            let got = &mut buf[..want.len()];
            flash.read(s.index * SLOT + PAGE * (1 + i), got);
            got == want
        })
    }

    /// Start writing a new slot: the settings and, unless this is a cold
    /// start record, a RAM image. Drive it with `Write::step`.
    pub fn write<'a>(&'a mut self, settings: Settings, image: Option<&'a [u8; RAM_IMAGE_LEN]>) -> Write<'a> {
        let index = self.next;
        let generation = self.next_gen;
        let header = header(generation, &settings, image);
        Write { store: self, index, generation, settings, image, header, step: 0 }
    }
}

/// A slot being written, a step at a time so the caller can get on with
/// other things in between: each step is one sector erase, or programming
/// one sector's worth of pages, or the final read back.
pub struct Write<'a> {
    store: &'a mut Store,
    index: usize,
    generation: u32,
    settings: Settings,
    image: Option<&'a [u8; RAM_IMAGE_LEN]>,
    header: [u8; PAGE],
    step: usize,
}

impl Write<'_> {
    /// Do the next piece. `None` while there is more to do; at the end,
    /// the new generation, or which slot did not read back the same.
    pub fn step(&mut self, flash: &mut impl Flash) -> Option<Result<u32, VerifyFailed>> {
        let base = self.index * SLOT;
        let ram_steps = if self.image.is_some() { RAM_PAGES.div_ceil(PAGES_PER_STEP) } else { 0 };
        let n = self.step;
        self.step += 1;
        if n < SLOT_SECTORS {
            flash.erase_sector(base + n * SECTOR);
        } else if n < SLOT_SECTORS + ram_steps {
            let first = (n - SLOT_SECTORS) * PAGES_PER_STEP;
            for p in first..(first + PAGES_PER_STEP).min(RAM_PAGES) {
                flash.program_page(base + PAGE * (1 + p), &self.ram_page(p));
            }
        } else if n == SLOT_SECTORS + ram_steps {
            // The header last: until it is down, this slot is not valid.
            flash.program_page(base, &self.header);
        } else {
            return Some(self.finish(flash));
        }
        None
    }

    /// Run every step at once.
    pub fn run(mut self, flash: &mut impl Flash) -> Result<u32, VerifyFailed> {
        loop {
            if let Some(r) = self.step(flash) {
                return r;
            }
        }
    }

    fn ram_page(&self, p: usize) -> [u8; PAGE] {
        let mut page = [0xFF; PAGE];
        if let Some(image) = self.image {
            let part = &image[p * PAGE..((p + 1) * PAGE).min(RAM_IMAGE_LEN)];
            page[..part.len()].copy_from_slice(part);
        }
        page
    }

    fn finish(&mut self, flash: &mut impl Flash) -> Result<u32, VerifyFailed> {
        let base = self.index * SLOT;
        let mut buf = [0u8; PAGE];
        flash.read(base, &mut buf);
        let mut same = buf == self.header;
        if self.image.is_some() {
            for p in 0..RAM_PAGES {
                if !same {
                    break;
                }
                flash.read(base + PAGE * (1 + p), &mut buf);
                same = buf == self.ram_page(p);
            }
        }
        let st = &mut *self.store;
        st.next_gen = self.generation.saturating_add(1);
        st.next = (self.index + 1) % st.slots;
        if same {
            st.newest = Some(Slot {
                index: self.index,
                generation: self.generation,
                settings: self.settings,
                has_ram: self.image.is_some(),
            });
            Ok(self.generation)
        } else {
            // Leave the newest alone and use another slot next time.
            if st.newest.is_some_and(|n| n.index == st.next) {
                st.next = (st.next + 1) % st.slots;
            }
            Err(VerifyFailed { index: self.index })
        }
    }
}

fn header(generation: u32, s: &Settings, image: Option<&[u8; RAM_IMAGE_LEN]>) -> [u8; PAGE] {
    let mut h = [0xFF; PAGE];
    h[..HEADER].fill(0);
    h[0..4].copy_from_slice(MAGIC);
    h[4..6].copy_from_slice(&VERSION.to_le_bytes());
    h[6..8].copy_from_slice(&(if image.is_some() { HAS_RAM } else { 0 }).to_le_bytes());
    h[8..12].copy_from_slice(&generation.to_le_bytes());
    let len = if image.is_some() { RAM_IMAGE_LEN as u32 } else { 0 };
    h[12..16].copy_from_slice(&len.to_le_bytes());
    if let Some(c) = s.call {
        h[16] = c.as_str().len() as u8;
        h[17..17 + c.as_str().len()].copy_from_slice(c.as_str().as_bytes());
    }
    if let Some(a) = s.alias {
        h[26] = a.as_str().len() as u8;
        h[27..27 + a.as_str().len()].copy_from_slice(a.as_str().as_bytes());
    }
    let mut crc = Crc32::new();
    crc.update(&h[..HEADER - 4]);
    if let Some(image) = image {
        crc.update(image);
    }
    h[HEADER - 4..HEADER].copy_from_slice(&crc.finish().to_le_bytes());
    h
}

/// The slot at `index`, if it holds a complete, valid record.
fn check(flash: &mut impl Flash, index: usize) -> Option<Slot> {
    let base = index * SLOT;
    let mut h = [0u8; HEADER];
    flash.read(base, &mut h);
    let u16_at = |i: usize| u16::from_le_bytes([h[i], h[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([h[i], h[i + 1], h[i + 2], h[i + 3]]);
    if &h[0..4] != MAGIC || u16_at(4) != VERSION || u16_at(6) & !HAS_RAM != 0 {
        return None;
    }
    let has_ram = u16_at(6) & HAS_RAM != 0;
    if u32_at(12) != if has_ram { RAM_IMAGE_LEN as u32 } else { 0 } {
        return None;
    }
    let text = |at: usize, max: usize| -> Option<Option<&str>> {
        match h[at] as usize {
            0 => Some(None),
            n if n <= max => core::str::from_utf8(&h[at + 1..at + 1 + n]).ok().map(Some),
            _ => None,
        }
    };
    let call = match text(16, 9)? {
        Some(t) => Some(parse_call(t).ok().filter(|c| c.as_str() == t)?),
        None => None,
    };
    let alias = match text(26, 6)? {
        Some(t) => Some(parse_alias(t).ok().filter(|a| a.as_str() == t)?),
        None => None,
    };
    let mut crc = Crc32::new();
    crc.update(&h[..HEADER - 4]);
    if has_ram {
        let mut buf = [0u8; PAGE];
        let mut left = RAM_IMAGE_LEN;
        let mut at = base + PAGE;
        while left > 0 {
            let n = left.min(PAGE);
            flash.read(at, &mut buf[..n]);
            crc.update(&buf[..n]);
            at += n;
            left -= n;
        }
    }
    if crc.finish() != u32_at(HEADER - 4) {
        return None;
    }
    Some(Slot { index, generation: u32_at(8), settings: Settings { call, alias }, has_ram })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec;
    use std::vec::Vec;

    /// NOR flash: erase sets bytes to 0xFF, programming can only clear
    /// bits. Power can be cut part way through the operation numbered
    /// `cut_at`, which then does half its work, and nothing after it
    /// happens. A byte at `stuck` never programs.
    struct FakeFlash {
        mem: Vec<u8>,
        ops: usize,
        cut_at: Option<usize>,
        stuck: Option<usize>,
    }

    impl FakeFlash {
        fn new(len: usize) -> Self {
            FakeFlash { mem: vec![0xFF; len], ops: 0, cut_at: None, stuck: None }
        }

        /// None: the power is off. Some(true): this operation is cut short.
        fn op(&mut self) -> Option<bool> {
            let n = self.ops;
            self.ops += 1;
            match self.cut_at {
                Some(c) if n > c => None,
                Some(c) => Some(n == c),
                None => Some(false),
            }
        }

        fn power_back(&mut self) {
            self.cut_at = None;
        }
    }

    impl Flash for FakeFlash {
        fn read(&mut self, offset: usize, buf: &mut [u8]) {
            buf.copy_from_slice(&self.mem[offset..offset + buf.len()]);
        }

        fn erase_sector(&mut self, offset: usize) {
            assert_eq!(offset % SECTOR, 0);
            let op = self.op();
            let s = &mut self.mem[offset..offset + SECTOR];
            match op {
                None => {}
                Some(false) => s.fill(0xFF),
                // Half done, in one of two ways: the first half erased, or
                // every byte part erased.
                Some(true) if (offset / SECTOR).is_multiple_of(2) => s[..SECTOR / 2].fill(0xFF),
                Some(true) => s.iter_mut().for_each(|b| *b |= 0xF0),
            }
        }

        fn program_page(&mut self, offset: usize, data: &[u8; PAGE]) {
            assert_eq!(offset % PAGE, 0);
            let cut = match self.op() {
                None => return,
                Some(c) => c,
            };
            for (i, &d) in data.iter().enumerate() {
                let at = offset + i;
                if Some(at) == self.stuck {
                    continue;
                }
                // Cut short: the first half of the page only, and some of
                // the bits of the rest.
                let d = if cut && i >= PAGE / 2 { d | 0x0F } else { d };
                self.mem[at] &= d;
            }
        }
    }

    const LEN: usize = 7 * SLOT;

    fn image(seed: u8) -> Vec<u8> {
        let mut v = tnc2_core::rom::ram_image_header(0x1122_3344_5566_7788 ^ seed as u64).to_vec();
        v.extend((0..tnc2_core::RAM_SIZE).map(|i| (i as u8).wrapping_mul(seed).wrapping_add(seed)));
        v
    }

    fn img(v: &[u8]) -> &[u8; RAM_IMAGE_LEN] {
        v.try_into().unwrap()
    }

    fn settings(call: &str, alias: &str) -> Settings {
        Settings { call: Some(parse_call(call).unwrap()), alias: Some(parse_alias(alias).unwrap()) }
    }

    fn ram_of(store: &Store, f: &mut FakeFlash) -> Option<Vec<u8>> {
        let mut out = vec![0u8; RAM_IMAGE_LEN];
        store.read_ram(f, (&mut out[..]).try_into().unwrap()).then_some(out)
    }

    #[test]
    fn empty_flash_has_nothing() {
        let mut f = FakeFlash::new(LEN);
        let s = Store::open(&mut f, LEN);
        assert_eq!(s.slots(), 7);
        assert_eq!(s.newest(), None);
        assert_eq!(ram_of(&s, &mut f), None);
        // all zeroes is no better than all ones
        f.mem.fill(0);
        assert_eq!(Store::open(&mut f, LEN).newest(), None);
    }

    #[test]
    fn settings_and_ram_round_trip() {
        let mut f = FakeFlash::new(LEN);
        let mut s = Store::open(&mut f, LEN);
        let a = image(3);
        assert_eq!(s.write(settings("g9tnt-1", "tnet"), Some(img(&a))).run(&mut f), Ok(1));
        assert!(s.holds(&mut f, img(&a)));
        assert!(!s.holds(&mut f, img(&image(4))));
        let s = Store::open(&mut f, LEN);
        let n = s.newest().unwrap();
        assert_eq!((n.index, n.generation, n.has_ram), (0, 1, true));
        assert_eq!(n.settings, settings("G9TNT-1", "TNET"));
        assert_eq!(ram_of(&s, &mut f).unwrap(), a);
    }

    #[test]
    fn a_setup_change_has_no_ram() {
        let mut f = FakeFlash::new(LEN);
        let mut s = Store::open(&mut f, LEN);
        s.write(settings("G9TNT-1", "TNET"), Some(img(&image(1)))).run(&mut f).unwrap();
        let only_call = Settings { call: Some(parse_call("G9ABC").unwrap()), alias: None };
        s.write(only_call, None).run(&mut f).unwrap();
        let s = Store::open(&mut f, LEN);
        let n = s.newest().unwrap();
        assert_eq!((n.generation, n.has_ram, n.settings), (2, false, only_call));
        assert_eq!(ram_of(&s, &mut f), None);
        assert!(!s.holds(&mut f, img(&image(1))));
        // and no settings at all is fine too
        let mut s = s;
        s.write(Settings::default(), None).run(&mut f).unwrap();
        assert_eq!(Store::open(&mut f, LEN).newest().unwrap().settings, Settings::default());
    }

    #[test]
    fn ring_goes_round_and_never_touches_the_newest() {
        let mut f = FakeFlash::new(LEN);
        let mut s = Store::open(&mut f, LEN);
        for g in 1..=30u32 {
            let before = s.newest();
            let im = image(g as u8);
            let snapshot = before.map(|b| f.mem[b.index * SLOT..(b.index + 1) * SLOT].to_vec());
            {
                let mut w = s.write(settings("G9TNT-1", "TNET"), Some(img(&im)));
                // the slot being written is never the newest
                assert!(before.is_none_or(|b| b.index != w.index));
                while w.step(&mut f).is_none() {}
            }
            if let (Some(b), Some(old)) = (before, snapshot) {
                assert_eq!(f.mem[b.index * SLOT..(b.index + 1) * SLOT], old[..], "newest slot was disturbed");
            }
            let re = Store::open(&mut f, LEN);
            let n = re.newest().unwrap();
            assert_eq!((n.generation, n.index), (g, (g as usize - 1) % 7));
            assert_eq!(ram_of(&re, &mut f).unwrap(), im);
        }
    }

    /// Cut the power at every flash operation of a save in turn: after
    /// power comes back, the store holds either the old save or the new
    /// one, whole, and the next save works.
    #[test]
    fn a_power_cut_at_any_moment_loses_at_most_the_save_in_progress() {
        for warm_up in [1usize, 9] {
            let mut ops_in_a_save = None;
            for cut in 0.. {
                let mut f = FakeFlash::new(LEN);
                let mut s = Store::open(&mut f, LEN);
                // warm_up 9 wraps the ring, so the slot being written held
                // an older valid save that the erase destroys part way.
                let mut old = Vec::new();
                for g in 1..=warm_up {
                    old = image(g as u8);
                    s.write(settings("G9OLD", "OLD"), Some(img(&old))).run(&mut f).unwrap();
                }
                let new = image(200);
                f.ops = 0;
                f.cut_at = Some(cut);
                let r = s.write(settings("G9NEW-2", "NEW"), Some(img(&new))).run(&mut f);
                if f.ops <= cut {
                    // the save finished before the cut came
                    assert_eq!(r, Ok(warm_up as u32 + 1));
                    ops_in_a_save = Some(f.ops);
                    break;
                }
                f.power_back();
                let mut s = Store::open(&mut f, LEN);
                let n = s.newest().expect("lost every save");
                let ram = ram_of(&s, &mut f).unwrap();
                if n.generation == warm_up as u32 + 1 {
                    assert_eq!((n.settings, &ram), (settings("G9NEW-2", "NEW"), &new));
                } else {
                    assert_eq!(n.generation, warm_up as u32);
                    assert_eq!((n.settings, &ram), (settings("G9OLD", "OLD"), &old), "cut at {cut}");
                }
                // and the store carries on
                let again = image(201);
                s.write(settings("G9AGN", "AGN"), Some(img(&again))).run(&mut f).unwrap();
                let s = Store::open(&mut f, LEN);
                assert_eq!(ram_of(&s, &mut f).unwrap(), again);
                assert!(s.newest().unwrap().generation > n.generation);
            }
            // 9 erases, 129 RAM pages and the header
            assert_eq!(ops_in_a_save, Some(SLOT_SECTORS + RAM_PAGES + 1));
        }
    }

    #[test]
    fn a_bad_write_is_caught_and_the_next_goes_elsewhere() {
        let mut f = FakeFlash::new(LEN);
        let mut s = Store::open(&mut f, LEN);
        let a = image(1);
        s.write(settings("G9TNT-1", "TNET"), Some(img(&a))).run(&mut f).unwrap();
        // slot 1 has a byte that will not program
        f.stuck = Some(SLOT + PAGE + 1000);
        let b = image(2);
        assert_eq!(s.write(settings("G9TNT-1", "TNET"), Some(img(&b))).run(&mut f), Err(VerifyFailed { index: 1 }));
        assert_eq!(s.newest().unwrap().generation, 1);
        assert!(s.holds(&mut f, img(&a)));
        assert_eq!(s.write(settings("G9TNT-1", "TNET"), Some(img(&b))).run(&mut f), Ok(3));
        let s = Store::open(&mut f, LEN);
        assert_eq!((s.newest().unwrap().index, s.newest().unwrap().generation), (2, 3));
        assert_eq!(ram_of(&s, &mut f).unwrap(), b);
    }

    #[test]
    fn a_failure_next_to_the_newest_skips_it() {
        // two slots: after a bad write to slot 1 the next attempt must not
        // go to slot 0, which holds the only good copy
        let len = 2 * SLOT;
        let mut f = FakeFlash::new(len);
        let mut s = Store::open(&mut f, len);
        s.write(Settings::default(), Some(img(&image(1)))).run(&mut f).unwrap();
        f.stuck = Some(SLOT);
        assert!(s.write(Settings::default(), Some(img(&image(2)))).run(&mut f).is_err());
        f.stuck = None;
        let im = image(3);
        let w = s.write(Settings::default(), Some(img(&im)));
        assert_eq!(w.index, 1);
    }

    #[test]
    fn corrupt_slots_are_ignored() {
        let mut f = FakeFlash::new(LEN);
        let mut s = Store::open(&mut f, LEN);
        s.write(settings("G9TNT-1", "TNET"), Some(img(&image(1)))).run(&mut f).unwrap();
        s.write(settings("G9TNT-2", "TNET"), Some(img(&image(2)))).run(&mut f).unwrap();
        // flip a bit in the newest's RAM: the older one wins
        f.mem[SLOT + PAGE + 5] ^= 0x10;
        let s2 = Store::open(&mut f, LEN);
        assert_eq!(s2.newest().unwrap().generation, 1);
        f.mem[SLOT + PAGE + 5] ^= 0x10;
        // a bad callsign with a good CRC (written by something else) is
        // still refused
        let mut h = header(9, &Settings::default(), None);
        h[16] = 3;
        h[17..20].copy_from_slice(b"a b");
        let mut crc = Crc32::new();
        crc.update(&h[..HEADER - 4]);
        h[HEADER - 4..HEADER].copy_from_slice(&crc.finish().to_le_bytes());
        f.mem[2 * SLOT..2 * SLOT + PAGE].copy_from_slice(&h);
        assert_eq!(Store::open(&mut f, LEN).newest().unwrap().generation, 2);
    }
}
