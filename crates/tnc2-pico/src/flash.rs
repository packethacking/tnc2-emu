//! The store region of flash (memory.x: STORE, 252 KB), erased and
//! programmed through the boot ROM's flash routines.
//!
//! embassy-rp's flash driver only works from core 0 and pauses core 1
//! while it writes. The node is the other way round: core 0 never stops
//! running the emulator, and core 1 writes. That is safe because nothing
//! core 0 touches is in flash (memory.x puts all code and read only data in
//! SRAM), DMA only moves data between SRAM and peripherals, and core 1
//! itself runs each ROM call from SRAM with its interrupts off, so none of
//! its interrupt handlers (whose vectors are in flash) can run meanwhile.
//! One call erases one 4 KB sector or programs one 256 byte page, which
//! keeps each spell with core 1's interrupts off short: a sector erase is
//! typically 45 ms (400 ms at worst), a page 0.4 ms (3 ms at worst). The
//! UART receiver keeps going by DMA all the while (radio.rs).
//!
//! Getting back to reading the flash afterwards: the ROM's
//! `flash_exit_xip` leaves plain 03h serial reads at a slow, safe clock
//! divider. If the read mode in use before was one that sends its command
//! every time (as the boot ROM's default does), the saved QMI settings,
//! with the clock divider clock.rs chose, are put straight back. A
//! continuous read mode would need the flash chip itself set up again, so
//! then the slow mode is simply kept: nothing runs from flash anyway.

use core::sync::atomic::{AtomicBool, Ordering};
use embassy_rp::pac;
use embassy_rp::pac::qmi::vals::PrefixLen;
use embassy_rp::rom_data;
use tnc2_node::store::{self, PAGE, SECTOR};

unsafe extern "C" {
    static __store_start: u8;
    static __store_end: u8;
}

const FLASH_BASE: usize = 0x1000_0000;

/// The store region. There is one, owned by whoever writes it.
pub struct StoreFlash {
    start: usize,
    len: usize,
}

impl StoreFlash {
    /// The region, once.
    pub fn take() -> StoreFlash {
        static TAKEN: AtomicBool = AtomicBool::new(false);
        assert!(!TAKEN.swap(true, Ordering::AcqRel), "store already taken");
        let start = &raw const __store_start as usize;
        let len = &raw const __store_end as usize - start;
        StoreFlash { start, len }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl store::Flash for StoreFlash {
    fn read(&mut self, offset: usize, buf: &mut [u8]) {
        assert!(offset + buf.len() <= self.len);
        // SAFETY: inside the store, which is mapped flash and only changes
        // through this object.
        unsafe { core::ptr::copy_nonoverlapping((self.start + offset) as *const u8, buf.as_mut_ptr(), buf.len()) }
    }

    fn erase_sector(&mut self, offset: usize) {
        assert!(offset.is_multiple_of(SECTOR) && offset + SECTOR <= self.len);
        rom_op((self.start - FLASH_BASE + offset) as u32, None);
    }

    fn program_page(&mut self, offset: usize, data: &[u8; PAGE]) {
        assert!(offset.is_multiple_of(PAGE) && offset + PAGE <= self.len);
        // The ROM reads the data while the flash is unreadable, so it must
        // be in SRAM: this copy is on the stack.
        let page = *data;
        rom_op((self.start - FLASH_BASE + offset) as u32, Some(&page));
    }
}

struct Rom {
    connect: unsafe extern "C" fn(),
    exit_xip: unsafe extern "C" fn(),
    erase: unsafe extern "C" fn(u32, usize, u32, u8),
    program: unsafe extern "C" fn(u32, *const u8, usize),
    flush: unsafe extern "C" fn(),
}

/// Erase the sector at `addr` (an offset from the start of flash), or
/// program a page there.
fn rom_op(addr: u32, page: Option<&[u8; PAGE]>) {
    // Look the routines up first: the lookup itself reads the ROM's table,
    // which is fine, but best done with the flash still readable.
    let rom = Rom {
        connect: rom_data::connect_internal_flash::ptr(),
        exit_xip: rom_data::flash_exit_xip::ptr(),
        erase: rom_data::flash_range_erase::ptr(),
        program: rom_data::flash_range_program::ptr(),
        flush: rom_data::flash_flush_cache::ptr(),
    };
    // SAFETY: interrupts are off on this core, the code is in SRAM, and
    // nothing else reads the flash (see the top of this file).
    cortex_m::interrupt::free(|_| unsafe { run(&rom, addr, page.map_or(core::ptr::null(), |p| p.as_ptr())) });
}

#[inline(never)]
unsafe fn run(rom: &Rom, addr: u32, data: *const u8) {
    let m0 = pac::QMI.mem(0);
    let (timing, rfmt, rcmd) = (m0.timing().read(), m0.rfmt().read(), m0.rcmd().read());
    cortex_m::asm::dsb();
    // SAFETY: the ROM's own sequence for erasing and programming, as in
    // its documentation (RP2350 datasheet 5.5.7).
    unsafe {
        (rom.connect)();
        (rom.exit_xip)();
        if data.is_null() {
            // A 4 KB sector erase; no block erase command.
            (rom.erase)(addr, SECTOR, 1 << 31, 0);
        } else {
            (rom.program)(addr, data, PAGE);
        }
        (rom.flush)();
    }
    if rfmt.prefix_len() != PrefixLen::NONE {
        m0.timing().write_value(timing);
        m0.rfmt().write_value(rfmt);
        m0.rcmd().write_value(rcmd);
    }
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
}
