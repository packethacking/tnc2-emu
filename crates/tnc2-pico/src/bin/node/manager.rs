//! The flash store and the board: building the board at start up from
//! what was saved, saving the RAM every 10 minutes when it has changed,
//! and MYCALL and ALIAS changes. One task does all three, so nothing else
//! reads or writes the flash meanwhile.
//!
//! A save asks core 0 for a snapshot of the RAM (a 32 KB copy between two
//! slices), compares it with the newest saved copy, and if it differs
//! writes it to the next slot of the store a sector at a time, yielding in
//! between so USB and the radio keep moving. Core 0 keeps running
//! throughout.
//!
//! A MYCALL or ALIAS change is saved straight away, as a slot with no RAM,
//! and the node cold starts with it, as it would with a new EPROM: core 0
//! drops its board, and gets a fresh one built with the new settings.

use crate::BOARD;
use alloc::boxed::Box;
use core::cell::RefCell;
use embassy_futures::select::{Either, select};
use embassy_futures::yield_now;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Instant, Timer};
use tnc2_core::{Board, BoardConfig, NodeConfig, rom};
use tnc2_node::handoff::{Image, Saver};
use tnc2_node::setup::{Alias, Call};
use tnc2_node::status::Start;
use tnc2_node::store::{Settings, Store, VerifyFailed};
use tnc2_pico::flash::StoreFlash;

pub const SAVE_EVERY: Duration = Duration::from_secs(600);

/// A change typed on the setup port.
pub enum Change {
    Call(Call),
    Alias(Alias),
}

#[derive(Clone, Copy)]
pub enum Reply {
    Unchanged,
    Saved,
    /// The flash did not read back the same; nothing changed.
    Failed,
}

pub static CHANGES: Channel<CriticalSectionRawMutex, Change, 1> = Channel::new();
pub static REPLIES: Channel<CriticalSectionRawMutex, Reply, 1> = Channel::new();

/// For STATUS and the setup port.
#[derive(Clone, Copy)]
pub struct Info {
    pub settings: Settings,
    pub start: Start,
    pub last_save: Option<(u32, Instant)>,
    pub failures: u32,
    pub next_save: Instant,
}

static INFO: Mutex<CriticalSectionRawMutex, RefCell<Option<Info>>> = Mutex::new(RefCell::new(None));

/// The latest `Info`, once the node has started.
pub fn info() -> Option<Info> {
    INFO.lock(|i| *i.borrow())
}

fn update(f: impl FnOnce(&mut Info)) {
    INFO.lock(|i| {
        if let Some(i) = i.borrow_mut().as_mut() {
            f(i)
        }
    });
}

fn record(r: Result<u32, VerifyFailed>) {
    match r {
        Ok(g) => update(|i| i.last_save = Some((g, Instant::now()))),
        Err(_) => update(|i| i.failures += 1),
    }
}

fn build(s: &Settings) -> Box<Board> {
    let node = NodeConfig {
        call: s.call.as_ref().map(|c| c.as_str()),
        alias: s.alias.as_ref().map(|a| a.as_str()),
        pokes: &[],
    };
    let board = Board::from_rom(tnc2_pico::rom::lower(), tnc2_pico::rom::upper(), &node, &BoardConfig::default());
    // The settings were checked when typed, and again when read back.
    Box::new(board.expect("EPROM images and settings"))
}

struct Manager {
    flash: StoreFlash,
    store: Store,
    saver: Saver,
    settings: Settings,
}

#[embassy_executor::task]
pub async fn run(saver: Saver) {
    let mut flash = StoreFlash::take();
    let len = flash.len();
    let store = Store::open(&mut flash, len);
    let settings = store.newest().map(|s| s.settings).unwrap_or_default();
    let mut m = Manager { flash, store, saver, settings };

    let start = m.first_board();
    let mut next = Instant::now() + SAVE_EVERY;
    INFO.lock(|i| *i.borrow_mut() = Some(Info { settings, start, last_save: None, failures: 0, next_save: next }));

    loop {
        match select(Timer::at(next), CHANGES.receive()).await {
            Either::First(()) => m.save().await,
            Either::Second(change) => {
                let reply = m.change(change).await;
                REPLIES.send(reply).await;
            }
        }
        // A change saves too, so either way the next check is a full
        // interval away.
        next = Instant::now() + SAVE_EVERY;
        update(|i| i.next_save = next);
    }
}

impl Manager {
    /// Build the board from the newest save and hand it to core 0.
    fn first_board(&mut self) -> Start {
        let mut board = build(&self.settings);
        let start = match self.store.newest() {
            None => Start::ColdEmpty,
            Some(s) if !s.has_ram => Start::ColdSetup,
            Some(s) => {
                let buf = self.saver.buffer().expect("nobody has asked core 0 for anything yet");
                self.store.read_ram(&mut self.flash, buf);
                match rom::check_ram_image(&buf[..], board.fingerprint()) {
                    Ok(ram) => {
                        board.load_ram(ram);
                        Start::Warm(s.generation)
                    }
                    Err(_) => Start::ColdMismatch,
                }
            }
        };
        if BOARD.start(board).is_err() {
            unreachable!("core 0 starts with no board");
        }
        start
    }

    /// Write a slot, a step at a time.
    async fn write(&mut self, settings: Settings, image: Option<&Image>) -> Result<u32, VerifyFailed> {
        let mut w = self.store.write(settings, image);
        loop {
            if let Some(r) = w.step(&mut self.flash) {
                return r;
            }
            yield_now().await;
        }
    }

    /// The 10 minute save.
    async fn save(&mut self) {
        // Core 0 copies the RAM between two slices, well within a
        // millisecond; with no board running it never does.
        self.saver.release();
        self.saver.request();
        let asked = Instant::now();
        while self.saver.ready().is_none() {
            if asked.elapsed() > Duration::from_secs(1) {
                self.saver.release();
                return;
            }
            Timer::after_millis(1).await;
        }
        let Manager { flash, store, saver, settings } = self;
        let image = saver.ready().expect("ready");
        if store.holds(flash, image) {
            // Saving it again would only wear the flash.
            saver.release();
            return;
        }
        let image: &Image = image;
        let r = {
            let mut w = store.write(*settings, Some(image));
            loop {
                if let Some(r) = w.step(flash) {
                    break r;
                }
                yield_now().await;
            }
        };
        saver.release();
        record(r);
    }

    /// MYCALL or ALIAS.
    async fn change(&mut self, change: Change) -> Reply {
        let mut new = self.settings;
        match change {
            Change::Call(c) => new.call = Some(c),
            Change::Alias(a) => new.alias = Some(a),
        }
        if new == self.settings {
            return Reply::Unchanged;
        }
        let r = self.write(new, None).await;
        record(r);
        if r.is_err() {
            return Reply::Failed;
        }
        self.settings = new;
        // A cold start, as with a new EPROM: core 0 drops the board it
        // has, then gets one built with the new settings.
        loop {
            BOARD.stop();
            if BOARD.is_stopped() {
                break;
            }
            Timer::after_millis(1).await;
        }
        if BOARD.start(build(&new)).is_err() {
            unreachable!("core 0 has stopped");
        }
        update(|i| {
            i.settings = new;
            i.start = Start::ColdSetup;
        });
        Reply::Saved
    }
}
