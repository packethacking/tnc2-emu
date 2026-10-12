//! The system clock.
//!
//! embassy-rp sets the clocks once, in `embassy_rp::init`: 150 MHz from the
//! 12 MHz crystal, the RP2350's rated speed. It can be told a higher clock
//! and a core voltage up to 1.30 V at that point, but has no way to change
//! them afterwards, and leaves the flash interface alone. The speed test
//! wants several clocks in one run, so `set` changes them while running:
//!
//! - core voltage first when speeding up, last when slowing down
//!   (POWMAN VREG, at most 1.30 V, the most the regulator gives without
//!   unlocking its limit, which this does not do);
//! - the flash clock divider and read delay, so the flash keeps seeing the
//!   50 MHz clock and sample point the boot ROM chose at 150 MHz;
//! - the system PLL, with clk_sys parked on the 12 MHz reference while the
//!   PLL relocks, the same sequence embassy-rp uses at start up.
//!
//! The peripheral clock runs from the 48 MHz USB PLL, so UART baud rates
//! do not move with the system clock. USB has its own PLL and the timer
//! counts the reference clock, so neither notices. embassy-rp's record of
//! the system clock (`clk_sys_freq()`) still says 150 MHz afterwards;
//! nothing used here reads it.

use embassy_rp::clocks::{ClockConfig, PeriClkSrc};
use embassy_rp::pac;
use embassy_time::Timer;

/// One system clock setting.
#[derive(Clone, Copy, Debug)]
pub struct Step {
    pub mhz: u32,
    /// Core voltage in millivolts, 1100 to 1300 in 50 mV steps.
    pub mv: u32,
    /// System PLL from the 12 MHz crystal: VCO = 12 MHz * fbdiv (750 to
    /// 1600 MHz), output = VCO / postdiv1 / postdiv2.
    fbdiv: u16,
    postdiv1: u8,
    postdiv2: u8,
}

/// Stock first, then overclocks. 150 MHz at 1.10 V is what embassy-rp and
/// the Pico SDK set up; the rest are guesses on the safe side, nothing
/// Raspberry Pi has rated.
pub const STEPS: [Step; 4] = [
    Step { mhz: 150, mv: 1100, fbdiv: 125, postdiv1: 5, postdiv2: 2 },
    Step { mhz: 200, mv: 1150, fbdiv: 100, postdiv1: 6, postdiv2: 1 },
    Step { mhz: 250, mv: 1250, fbdiv: 125, postdiv1: 6, postdiv2: 1 },
    Step { mhz: 300, mv: 1300, fbdiv: 125, postdiv1: 5, postdiv2: 1 },
];

/// Stock.
pub const STOCK: Step = STEPS[0];

/// The clocks to hand `embassy_rp::init`: stock, with the peripheral clock
/// on the USB PLL.
pub fn config() -> embassy_rp::config::Config {
    let mut clocks = ClockConfig::crystal(12_000_000);
    clocks.peri_clk_src = Some(PeriClkSrc::PllUsb);
    embassy_rp::config::Config::new(clocks)
}

impl Step {
    fn vsel(&self) -> u8 {
        // 0.55 V is 0, then 50 mV a step: 1.10 V is 11, 1.30 V is 15.
        ((self.mv - 550) / 50) as u8
    }

    /// Flash clock divider: keeps SCK at or below 50 MHz.
    fn qmi_clkdiv(&self) -> u8 {
        self.mhz.div_ceil(50) as u8
    }

    /// Flash read delay in half system clocks: the boot ROM uses 2 at
    /// 150 MHz, about 6.7 ns, and this keeps the same time.
    fn qmi_rxdelay(&self) -> u8 {
        ((2 * self.mhz + 75) / 150) as u8
    }
}

static mut CURRENT: Step = STOCK;

/// The step the clocks were last set to.
pub fn current() -> Step {
    // SAFETY: only `set` writes it, from thread mode.
    unsafe { CURRENT }
}

/// Change the system clock, core voltage and flash timing to `to`.
pub async fn set(to: Step) {
    let from = current();
    if to.mv > from.mv {
        set_voltage(to.vsel()).await;
    }
    let timing = (to.qmi_clkdiv(), to.qmi_rxdelay());
    let faster = to.mhz > from.mhz;
    cortex_m::interrupt::free(|_| {
        // SAFETY: interrupts are off and the function runs from SRAM, so
        // nothing reads the flash while its timing and clock change.
        unsafe { switch_pll(to.fbdiv, to.postdiv1, to.postdiv2, timing, faster) }
    });
    if to.mv < from.mv {
        set_voltage(to.vsel()).await;
    }
    // SAFETY: as in `current`.
    unsafe { CURRENT = to }
}

async fn set_voltage(vsel: u8) {
    const PASSWORD: u32 = 0x5AFE << 16;
    let p = pac::POWMAN;
    p.vreg_ctrl().modify(|w| {
        w.0 = (w.0 & 0xFFFF) | PASSWORD;
        w.set_unlock(true);
    });
    while p.vreg().read().update_in_progress() {}
    p.vreg().modify(|w| {
        w.0 = (w.0 & 0xFFFF) | PASSWORD;
        w.set_vsel(vsel);
    });
    while p.vreg().read().update_in_progress() {}
    // Let it settle, as embassy-rp does at start up.
    Timer::after_millis(2).await;
}

/// Runs from SRAM: on the way up the flash timing changes before the
/// clock, on the way down after it, so the flash never sees a faster clock
/// than it was set up for.
#[unsafe(link_section = ".data.ram_func")]
#[inline(never)]
unsafe fn switch_pll(fbdiv: u16, postdiv1: u8, postdiv2: u8, timing: (u8, u8), faster: bool) {
    use pac::clocks::vals::{ClkSysCtrlAuxsrc, ClkSysCtrlSrc};

    cortex_m::asm::dsb();
    cortex_m::asm::isb();
    let qmi = || {
        pac::QMI.mem(0).timing().modify(|w| {
            w.set_clkdiv(timing.0);
            w.set_rxdelay(timing.1);
        })
    };
    if faster {
        qmi();
    }

    let c = pac::CLOCKS;
    c.clk_sys_ctrl().modify(|w| w.set_src(ClkSysCtrlSrc::CLK_REF));
    while c.clk_sys_selected().read().0 != 1 << ClkSysCtrlSrc::CLK_REF as u32 {}

    let pll = pac::PLL_SYS;
    pll.pwr().write(|w| {
        w.set_pd(true);
        w.set_vcopd(true);
        w.set_postdivpd(true);
        w.set_dsmpd(true);
    });
    pll.cs().write(|w| w.set_refdiv(1));
    pll.fbdiv_int().write(|w| w.set_fbdiv_int(fbdiv));
    pll.pwr().write(|w| {
        w.set_pd(false);
        w.set_vcopd(false);
        w.set_postdivpd(true);
        w.set_dsmpd(true);
    });
    while !pll.cs().read().lock() {}
    pll.prim().write(|w| {
        w.set_postdiv1(postdiv1);
        w.set_postdiv2(postdiv2);
    });
    pll.pwr().modify(|w| w.set_postdivpd(false));

    c.clk_sys_ctrl().write(|w| {
        w.set_auxsrc(ClkSysCtrlAuxsrc::CLKSRC_PLL_SYS);
        w.set_src(ClkSysCtrlSrc::CLKSRC_CLK_SYS_AUX);
    });
    while c.clk_sys_selected().read().0 != 1 << ClkSysCtrlSrc::CLKSRC_CLK_SYS_AUX as u32 {}

    if !faster {
        qmi();
    }
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
}

/// Count clk_sys against the 12 MHz reference with the frequency counter,
/// in kHz: the clock the chip is really running at.
pub fn measure_sys_khz() -> u32 {
    use pac::clocks::vals::Fc0src;
    let c = pac::CLOCKS;
    while c.fc0_status().read().running() {}
    c.fc0_ref_khz().write(|w| w.set_fc0_ref_khz(12_000));
    c.fc0_interval().write(|w| w.set_fc0_interval(10));
    c.fc0_min_khz().write(|w| w.set_fc0_min_khz(0));
    c.fc0_max_khz().write(|w| w.set_fc0_max_khz(0x1FF_FFFF));
    c.fc0_src().write(|w| w.set_fc0_src(Fc0src::CLK_SYS));
    while !c.fc0_status().read().done() {}
    c.fc0_result().read().khz()
}

/// The core voltage the regulator is set to, in millivolts (right up to
/// 1.30 V; the steps above that are not 50 mV apart).
pub fn core_mv() -> u32 {
    550 + 50 * pac::POWMAN.vreg().read().vsel() as u32
}
