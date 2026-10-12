# TheNet on a Raspberry Pi Pico 2 W

The same untouched TheNet X1J EPROM, running on a Pico 2 W instead of in a container. A NinoTNC wired to the Pico is the radio port, TheNet's console comes up on USB, and the 32 KB of battery backed RAM is kept in the Pico's flash, so the node remembers its routes and settings across power cuts.

There are two firmwares:

- **The node** (`tnc2-pico-node.uf2`): a real TheNet node. This page is mostly about it.
- **The speed test** (`tnc2-pico-speedtest.uf2`): measures whether a Pico 2 W keeps up with a real TNC-2, and at what clock. TheNet never sleeps (it sits in a busy loop waiting for work), so the Pico has to emulate the Z80 at the full 4.9152 MHz all the time. Nobody has run it on a board yet: if you have one, please do (below), because its answer decides the node's clock.

Nothing here has run on real hardware yet. Reports welcome.

## Wiring the NinoTNC

The NinoTNC's serial pins go straight to the Pico's UART1, bypassing the TNC's own USB chip. These are the pins pico-node used and the NinoBLE Rev5 carrier board ([mumrah/ninotnc-ble](https://github.com/mumrah/ninotnc-ble)) wires up:

| Pico 2 W | Pin | NinoTNC |
|----------|-----|---------|
| GP20 (to the TNC) | 26 | RX |
| GP21 (from the TNC) | 27 | TX |
| GND | 28 | GND |

Both sides are 3.3 V. The link runs at 57600 baud, KISS, and the node only uses KISS port 0. It does not set the NinoTNC up: choose its modem mode with its DIP switches (or its own tools) first.

The Pico sends two stop bits and the TNC one. That is on purpose: the NinoTNC garbles bytes that arrive back to back while it is starting to transmit, and the extra idle bit after each byte cures it. A frame that arrives from the TNC with a serial error anywhere in it (framing, parity, break or overrun) is dropped whole, and AX.25 sends it again.

## Flashing the node

1. Get `tnc2-pico-node.uf2`: open the latest run of the `ci` workflow on GitHub (Actions tab) and download the `tnc2-pico-node` artifact, or build it yourself (below).
2. Hold the BOOTSEL button on the Pico 2 W while plugging it in. A drive called `RP2350` appears.
3. Copy the UF2 onto that drive. The Pico restarts by itself and the drive goes away. (Or, with [picotool](https://github.com/raspberrypi/picotool): `picotool load -x tnc2-pico-node.uf2`.)

Flashing a new version keeps the saved RAM and settings: they live in a part of the flash the firmware never touches.

## Two serial ports

Plug the Pico 2 W into a computer and two USB serial ports appear, both from one device called `tnc2-pico`:

1. **TheNet's console**, exactly what the TNC-2's RS232 port would carry, nothing added. Press Escape, then `C` and Enter, and you are on the node's command line, as on the container version. TheNet's own sysop commands (`PARMS` and the rest) are all here, and what they change is kept in its RAM. With no terminal open, what TheNet prints goes nowhere, as on an RS232 port with nothing plugged in.
2. **The Pico's own port**, for the node's callsign and alias and for status. It shows a `pico>` prompt.

On Linux they are usually `/dev/ttyACM0` and `/dev/ttyACM1`. The names under `/dev/serial/by-id/` end in `-if00` (console) and `-if02` (Pico) and never move around. On Windows they are two COM ports; if in doubt, try both. The baud rate setting does not matter. Any terminal works, for example `picocom /dev/ttyACM1` or PuTTY.

## Setting MYCALL and ALIAS

On the Pico's own port:

```
pico> MYCALL G9TNT-1
MYCALL G9TNT-1 saved. TheNet has cold started with it, as with a new EPROM.
pico> ALIAS TNET
ALIAS TNET saved. TheNet has cold started with it, as with a new EPROM.
```

A callsign is up to 6 letters and digits with an optional SSID from 0 to 15; an alias is up to 6 characters. `MYCALL` or `ALIAS` on its own shows the current one, `STATUS` shows how the node is getting on (speed, frames, drops, the last save, the clock, why it last restarted), and `HELP` lists the commands.

Until MYCALL is set, the node runs with the callsign burnt into the ROM (G8KBB-5, alias THENET), just as the container does without `TNC2_CALL`, and the Pico's port says so when you open it.

Changing the callsign or alias is like fitting a new EPROM: TheNet cold starts and its RAM starts afresh, so routes and anything set with `PARMS` go back to the ROM's defaults. Everything else is set from TheNet's console.

## Saving the RAM

Every 10 minutes the Pico copies TheNet's RAM and, if it has changed (it nearly always has), writes it to flash. A MYCALL or ALIAS change is saved straight away. At power on the node starts from the newest good copy, warm, just as a TNC-2 with its battery would.

A power cut loses whatever changed since the last save: up to 10 minutes of RAM changes, such as routes learnt or PARMS changes made in that time. A cut part way through a save loses only that save; the one before it is still there.

How it is kept: 252 KB of flash set aside after the firmware holds 7 slots, each with the settings, the RAM, a generation number and a checksum. Each save goes in the slot after the newest, its header written last and everything read back afterwards, so the newest good copy is never touched while the next one is written.

Flash wears out with erasing. Each save erases one slot (9 sectors of 4 KB), and the 7 slots take turns, so a sector is erased once every 7 saves. Saving every 10 minutes, all day, that is about 21 erases a sector a day, and the flash is rated for at least 100,000: about 13 years of non-stop running.

## The clock

The node runs the Pico at 250 MHz with the core at 1.25 V, set once at start up. That is a provisional choice, with room to spare: 150 MHz is the RP2350's rated speed, and the estimate below says real time needs somewhere between 103 and 140 MHz, so stock may well do. Once the speed test has run on real boards, the node will use the lowest clock it finds to be a GO, at the voltage it used. The setting is one constant, `SYS_CLOCK` in `crates/tnc2-pico/src/bin/node/main.rs`.

If the Pico cannot keep up, the node still works, just slowly: its clock falls behind real time, and STATUS shows the speed (1.00x is real time) and how far behind it is.

## Running the speed test

1. Get `tnc2-pico-speedtest.uf2`: open the latest run of the `ci` workflow on GitHub (Actions tab) and download the `tnc2-pico-speedtest` artifact, or build it yourself (below).
2. Hold the BOOTSEL button on the Pico 2 W while plugging it in. A drive called `RP2350` appears.
3. Copy the UF2 onto that drive. The Pico restarts by itself and the drive goes away. (Or, with [picotool](https://github.com/raspberrypi/picotool): `picotool load -x tnc2-pico-speedtest.uf2`.)
4. Open the second serial port with any terminal, for example `screen -L /dev/ttyACM1` (leave with Ctrl-A, K, y), `picocom --logfile speedtest.log /dev/ttyACM1`, or PuTTY on Windows. Logging to a file, as those two do, is worth it: if the board restarts, the terminal loses the port and you will want what it printed.

The whole report is printed every three seconds, so it does not matter when you connect. The test takes about five minutes: it boots TheNet and runs it for 60 emulated seconds at 150 MHz, then again at 200, 250 and 300 MHz, then drops back to 150 MHz and keeps showing the results. Leave it running until it says `Finished`.

## Reading the result

Each line is one clock speed:

| Column | Meaning |
|--------|---------|
| clock | the system clock asked for |
| counted | the clock the chip actually ran at, in MHz, measured by its own frequency counter |
| core | the core voltage, read back from the regulator; a `*` marks a row from before a restart, which shows the voltage asked for |
| Z80 MHz | how fast the emulated Z80 ran; a real TNC-2 is 4.9152 |
| ratio | emulated time over real time. 1.00 is exactly fast enough |
| cycles | Pico clock cycles per Z80 clock cycle; multiply by 4.9152 for the clock real time needs |
| RAM hash | a fingerprint of TheNet's RAM at the end, which must say `matches` |

The last line sums it up:

- **GO**: a ratio of 1.25 or more at some clock, with the hash matching. Real time with room to spare for radio and console traffic. The lowest such clock is the one to use.
- **MARGINAL**: the best ratio is between 1.00 and 1.25. It keeps up, just, with little room for anything else.
- **NO-GO**: below 1.00 at every clock. The emulator needs to get faster before a node makes sense.
- **WRONG** in the hash column means the chip made mistakes at that clock: ignore its speed, that clock is too fast for this board.
- Until the test finishes, MARGINAL and NO-GO say `so far`: they cover the clocks done up to then.

If a clock is too fast for the board, the chip may simply stop. A watchdog then restarts it within three seconds. You will see the output stop, either after a line saying `switching now` or part way through a clock; both mean the same thing. The serial port disappears and comes back, so reconnect (on Linux it may come back under a new number; the `/dev/serial/by-id/` name stays the same). The report then shows the clocks that finished, `HUNG` at the one that stopped, and `not tried` for the faster ones: the test does not try that clock or anything faster again, and the board stays at 150 MHz. `CRASHED` instead of `HUNG` means the firmware itself went wrong at that clock, which is worth reporting. Nothing is harmed either way. Unplugging the board and plugging it back in starts the whole test again.

Please send the whole output, whatever it says.

What we expect, from counting the instructions the emulator runs (no hardware has run it yet, see `tools/m33bench`): about 15 Arm instructions per Z80 clock cycle, which comes to somewhere between 103 and 140 MHz for real time. So 150 MHz should just about do, and the overclocks show how much room there is.

## The speed test's clocks

The RP2350 is rated for 150 MHz. Everything above that is overclocking, which Raspberry Pi does not promise works, though plenty of people run these chips faster. The test raises the core voltage before each overclock, a little more each time, and never past 1.30 V. That is as high as the regulator goes unless its voltage limit is disabled, and the firmware does not disable it (it sets the regulator's unlock bit, which any voltage change needs, but not the one that lifts the limit):

| Clock | Core voltage |
|-------|--------------|
| 150 MHz | 1.10 V (stock) |
| 200 MHz | 1.15 V |
| 250 MHz | 1.25 V |
| 300 MHz | 1.30 V |

The flash chip's own clock is kept at 50 MHz throughout, as the boot ROM set it.

## Building

You need [rustup](https://rustup.rs). From the repository:

```sh
cd crates/tnc2-pico
cargo build --release
picotool uf2 convert target/thumbv8m.main-none-eabihf/release/node -t elf tnc2-pico-node.uf2 --family rp2350-arm-s
picotool uf2 convert target/thumbv8m.main-none-eabihf/release/speedtest -t elf tnc2-pico-speedtest.uf2 --family rp2350-arm-s
```

The firmware crate pins its own Rust version and target (`rust-toolchain.toml`, `.cargo/config.toml`), and rustup fetches them the first time. It is not part of the repository's Cargo workspace, so host builds never need the embedded toolchain. The parts of the node that need no hardware (the setup commands, the flash store, KISS to the TNC, the queues between the cores, the core 0 loop) are in `crates/tnc2-node`, which is in the workspace and tested by a plain `cargo test`. The UF2 needs picotool 2.0 or later, which knows the RP2350; CI uses the Linux build Raspberry Pi publish with [pico-sdk-tools](https://github.com/raspberrypi/pico-sdk-tools/releases). Each UF2 is a single block of flash from 0x10000000, which BOOTSEL copying handles in one go. With a debug probe, `cargo run --release --bin node` flashes it directly through probe-rs.

## Under the hood

- **Two cores.** Core 0 runs the Z80 and nothing else, with no interrupts. It runs the board in 1 ms slices of emulated time, each only once the hardware timer says real time has reached its end, so the node's clock never runs ahead; when it is behind (after a busy moment) it runs flat out until it has caught up. Core 1 does everything else with Embassy: both USB serial ports, the NinoTNC, the setup port and the flash.
- **Between them**, lock free single producer, single consumer queues in SRAM: radio frames each way (core 1 does the KISS framing), console bytes each way. Core 0 checks them between slices. Core 1 hands over a whole new board when MYCALL or ALIAS changes, and asks for a copy of the RAM when it is time to save: core 0 copies the 32 KB between two slices and carries on.
- **Everything runs from SRAM.** All the code and constants are copied from flash to SRAM at start up (`memory.x`); only the start up code and the EPROM images stay in flash. Code in flash is fetched through a 16 KB cache that the emulator alone would overflow, and while the flash is being erased or written it cannot be read at all. So core 0 keeps emulating while core 1 saves, and core 0's memory protection unit refuses to run code from flash, so a slip there shows up at once.
- **The NinoTNC link.** The UART receives at 8N1 and a DMA channel empties it into a 16 KB ring, each byte with its error flags, so nothing is lost while core 1 is busy with the flash (an erase can take up to 0.4 s). A PIO state machine on GP20 sends with two stop bits; its timing is worked out from the clock constant.
- **Watchdog.** If core 1 stops, or core 0 stops making progress, the watchdog restarts the chip within 2 seconds. STATUS says why the chip last restarted. The watchdog counts from the 12 MHz crystal, so it still works if the system clock does not. (embassy-rp's own watchdog driver is written for the RP2040 and gets the RP2350's reset selection wrong, so the firmware drives it directly.)
- **Changing the clock while running** is something only the speed test does. embassy-rp sets the clocks once at start up; the firmware then raises the voltage, slows the flash interface to match, and moves the system PLL itself, from SRAM with interrupts off (`src/clock.rs`). The node does this once, before it sets anything up. USB has its own 48 MHz PLL, and the UART runs from it too, so neither moves with the system clock.
- **Flash layout.** The firmware may use the first 3840 KB of the 4 MB flash. The 252 KB after that is the store for the saved RAM and node settings. The last 4 KB is left alone, because UF2s from the Pico SDK write a workaround block there.
