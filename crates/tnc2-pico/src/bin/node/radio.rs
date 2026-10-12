//! The NinoTNC, wired to UART1 as pico-node did: GP20 from the Pico to the
//! TNC's receive line, GP21 from the TNC's transmit line, 57600 baud.
//!
//! Receive: the hardware UART at 8N1, emptied by DMA into a 16 KB ring in
//! SRAM, a 32 bit word a byte so the UART's per byte error flags come too
//! (tnc2_node::radio). DMA keeps going while core 1 has its interrupts off
//! for a flash erase, and the ring holds 0.7 s of back to back bytes, well
//! over the slowest erase.
//!
//! Transmit: a PIO state machine on GP20 with two stop bits. The NinoTNC
//! drops and misreads bytes that arrive back to back while it is starting
//! to transmit, and puts the damage on air with a good checksum; a gap of
//! one bit after each byte cures it (pico-node, measured on the bench).
//! The UART cannot send two stop bits and receive with one, so the
//! transmitter is a separate PIO program, fed by DMA a frame at a time.

use crate::RADIO_PIPE;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, Ordering::Relaxed};
use embassy_rp::dma;
use embassy_rp::gpio::Level;
use embassy_rp::pac;
use embassy_rp::pac::dma::vals::{DataSize, TransCountMode, TreqSel};
use embassy_rp::peripherals::{DMA_CH0, DMA_CH1, PIN_20, PIN_21, PIO0, UART1};
use embassy_rp::pio::{self, Common, Config as PioConfig, Direction, FifoJoin, Pio, ShiftDirection, StateMachine};
use embassy_rp::pio_programs::clock_divider::calculate_pio_clock_divider_value;
use embassy_rp::uart::{self, UartRx};
use embassy_rp::{Peri, bind_interrupts};
use embassy_time::{Duration, Instant, Timer};
use tnc2_core::RADIO_RX_MAX_FRAME;
use tnc2_node::pipe::{Consumer, Producer};
use tnc2_node::radio::{KissIn, encode, encoded_len, uart_word};

pub const BAUD: u32 = 57_600;

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
    DMA_IRQ_0 => dma::InterruptHandler<DMA_CH1>;
});

/// What happened to received frames, for STATUS.
pub struct RxCounts {
    pub line_errors: AtomicU32,
    pub too_long: AtomicU32,
    pub queue_full: AtomicU32,
    /// Times the ring overran (core 1 away too long): whatever was in
    /// progress is dropped as a line error too.
    pub overruns: AtomicU32,
}

pub static RX: RxCounts = RxCounts {
    line_errors: AtomicU32::new(0),
    too_long: AtomicU32::new(0),
    queue_full: AtomicU32::new(0),
    overruns: AtomicU32::new(0),
};

/// Frames TheNet sent that were too long to encode here (none should be).
pub static TX_TOO_LONG: AtomicU32 = AtomicU32::new(0);

const RING_WORDS: usize = 4096;
const RING_BYTES: usize = RING_WORDS * 4;

/// The DMA ring wraps on its own size, so it is aligned to it.
#[repr(C, align(16384))]
struct Ring(UnsafeCell<[u32; RING_WORDS]>);

// SAFETY: written only by the DMA, read only by `Receiver`.
unsafe impl Sync for Ring {}

static RING: Ring = Ring(UnsafeCell::new([0; RING_WORDS]));
const _: () = assert!(RING_BYTES == 1 << 14);

/// How long the ring lasts at full speed (10 bits a byte), less a margin:
/// if core 1 was away longer than this, bytes may have been overwritten.
const RING_LASTS: Duration = Duration::from_micros(RING_WORDS as u64 * 10 * 1_000_000 / BAUD as u64 * 3 / 4);

pub struct Receiver {
    _uart: UartRx<'static, uart::Blocking>,
    _dma: Peri<'static, DMA_CH0>,
}

impl Receiver {
    pub fn new(uart: Peri<'static, UART1>, pin: Peri<'static, PIN_21>, dma: Peri<'static, DMA_CH0>) -> Self {
        let mut cfg = uart::Config::default();
        cfg.baudrate = BAUD;
        let rx = UartRx::new_blocking(uart, pin, cfg);
        // A pull up, so a TNC that is not plugged in reads as an idle line
        // rather than noise.
        pac::PADS_BANK0.gpio(21).modify(|w| {
            w.set_pue(true);
            w.set_pde(false);
        });
        let u = pac::UART1;
        u.uartdmacr().write(|w| w.set_rxdmae(true));
        let ch = pac::DMA.ch(0);
        ch.read_addr().write_value(u.uartdr().as_ptr() as u32);
        ch.write_addr().write_value(RING.0.get() as u32);
        ch.trans_count().write(|w| {
            w.set_mode(TransCountMode::ENDLESS);
            w.set_count(1);
        });
        ch.ctrl_trig().write(|w| {
            // Whole words: the byte and its error flags (bits 8 to 11).
            w.set_data_size(DataSize::SIZE_WORD);
            w.set_incr_read(false);
            w.set_incr_write(true);
            w.set_ring_sel(true);
            w.set_ring_size(RING_BYTES.trailing_zeros() as u8);
            w.set_treq_sel(TreqSel::UART1_RX);
            w.set_chain_to(0);
            w.set_high_priority(true);
            w.set_en(true);
        });
        Receiver { _uart: rx, _dma: dma }
    }

    /// Where the DMA will write next, as a word index into the ring.
    fn written(&self) -> usize {
        let at = pac::DMA.ch(0).write_addr().read() as usize;
        (at - RING.0.get() as usize) / 4 % RING_WORDS
    }

    fn word(&self, i: usize) -> u32 {
        // SAFETY: inside the ring; the DMA writes whole words.
        unsafe { (RING.0.get() as *const u32).add(i).read_volatile() }
    }
}

/// Bytes from the NinoTNC, frames to core 0.
#[embassy_executor::task]
pub async fn receive(rx: Receiver, mut to_core0: Producer<RADIO_PIPE>) {
    let mut kiss = KissIn::new();
    let mut long = 0u32;
    let mut full = 0u32;
    let mut read = rx.written();
    let mut last = Instant::now();
    loop {
        Timer::after_millis(2).await;
        let now = Instant::now();
        let written = rx.written();
        if now - last > RING_LASTS {
            // Away too long: the ring may have gone right round.
            kiss.lost();
            read = written;
            RX.overruns.fetch_add(1, Relaxed);
        }
        last = now;
        while read != written {
            let (b, error) = uart_word(rx.word(read));
            read = (read + 1) % RING_WORDS;
            if let Some(f) = kiss.byte(b, error) {
                if f.len() > RADIO_RX_MAX_FRAME {
                    long += 1;
                } else if !to_core0.push_frame(f) {
                    full += 1;
                }
            }
        }
        RX.line_errors.store(kiss.counts().line_errors, Relaxed);
        RX.too_long.store(long + kiss.too_long() as u32, Relaxed);
        RX.queue_full.store(full, Relaxed);
    }
}

pub struct Transmitter {
    sm: StateMachine<'static, PIO0, 0>,
    dma: dma::Channel<'static>,
    // Holds the PIO block, and with it the loaded program.
    _common: Common<'static, PIO0>,
}

impl Transmitter {
    /// `sys_hz` is the system clock the PIO runs from.
    pub fn new(pio: Peri<'static, PIO0>, pin: Peri<'static, PIN_20>, dma: Peri<'static, DMA_CH1>, sys_hz: u32) -> Self {
        let Pio { mut common, mut sm0, .. } = Pio::new(pio, Irqs);
        // Eight PIO cycles a bit. The line idles high, and `pull` stalls
        // with it high, so the gap after the last byte is plain idle.
        let prg = embassy_rp::pio::program::pio_asm!(
            r#"
                .side_set 1 opt
                    pull       side 1 [7]  ; second stop bit, or idle while waiting
                    set x, 7   side 0 [7]  ; start bit
                bitloop:
                    out pins, 1            ; 8 data bits, least significant first
                    jmp x-- bitloop   [6]
                    nop        side 1 [7]  ; first stop bit
            "#
        );
        let prg = common.load_program(&prg.program);
        let pin = common.make_pio_pin(pin);
        sm0.set_pins(Level::High, &[&pin]);
        sm0.set_pin_dirs(Direction::Out, &[&pin]);
        let mut cfg = PioConfig::default();
        cfg.set_out_pins(&[&pin]);
        cfg.use_program(&prg, &[&pin]);
        cfg.shift_out.auto_fill = false;
        cfg.shift_out.direction = ShiftDirection::Right;
        cfg.fifo_join = FifoJoin::TxOnly;
        cfg.clock_divider = calculate_pio_clock_divider_value(sys_hz, 8 * BAUD);
        sm0.set_config(&cfg);
        sm0.set_enable(true);
        Transmitter { sm: sm0, dma: dma::Channel::new(dma, Irqs), _common: common }
    }
}

/// Frames from core 0, as KISS port 0 data frames, to the NinoTNC.
#[embassy_executor::task]
pub async fn transmit(mut tx: Transmitter, mut from_core0: Consumer<RADIO_PIPE>) {
    const MAX: usize = 1024;
    let mut frame = [0u8; MAX];
    let mut out = [0u8; encoded_len(MAX)];
    loop {
        match from_core0.pop_frame(&mut frame) {
            Some(Ok(n)) => {
                let len = encode(&frame[..n], &mut out);
                // A byte written to the FIFO arrives copied into all four
                // byte lanes of the word; the program shifts out the low 8.
                tx.sm.tx().dma_push(&mut tx.dma, &out[..len], false).await;
            }
            Some(Err(_)) => {
                TX_TOO_LONG.fetch_add(1, Relaxed);
            }
            None => Timer::after_millis(1).await,
        }
    }
}
