# TheNet on a Raspberry Pi Pico 2 W

The aim: the same untouched TheNet X1J EPROM, running on a Pico 2 W instead of in a container. A NinoTNC on one of the Pico's UARTs is the radio port (KISS), the sysop console comes up on USB, and the 32 KB of battery backed RAM is kept in the Pico's flash, so the node remembers its routes and settings across power cuts.

TheNet never sleeps: it sits in a busy loop waiting for work. So the Pico has to emulate the Z80 at the full 4.9152 MHz all the time, and the first question is whether it can. That is what this first firmware answers. It is a speed test, not a node yet.

## Two serial ports

Plug the Pico 2 W into a computer and two USB serial ports appear, both from one device called `tnc2-pico`:

1. **TheNet's console**, exactly what the TNC-2's RS232 port would carry. In the speed test it only prints a note saying where to look.
2. **The Pico's own port**, for its settings and status. In the speed test this is where the results go.

On Linux they are usually `/dev/ttyACM0` and `/dev/ttyACM1`. The names under `/dev/serial/by-id/` end in `-if00` (console) and `-if02` (Pico) and never move around. On Windows they are two COM ports; if in doubt, try both. The baud rate setting does not matter.

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

What we expect, from counting the instructions the emulator runs (no hardware has run it yet): about 29 Arm instructions per Z80 clock cycle, which comes to somewhere between 200 and 275 MHz for real time. So 150 MHz will very likely fall short, and the answer is in the overclocks.

## The clocks

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
picotool uf2 convert target/thumbv8m.main-none-eabihf/release/speedtest -t elf tnc2-pico-speedtest.uf2 --family rp2350-arm-s
```

The firmware crate pins its own Rust version and target (`rust-toolchain.toml`, `.cargo/config.toml`), and rustup fetches them the first time. It is not part of the repository's Cargo workspace, so host builds never need the embedded toolchain. The UF2 needs picotool 2.0 or later, which knows the RP2350; CI uses the Linux build Raspberry Pi publish with [pico-sdk-tools](https://github.com/raspberrypi/pico-sdk-tools/releases). The UF2 is a single block of flash from 0x10000000, which BOOTSEL copying handles in one go. With a debug probe, `cargo run --release` flashes it directly through probe-rs.

## Under the hood

- **The emulator runs from SRAM.** Code in flash is fetched through a 16 KB cache, and the emulator's hot code is about 20 KB, so it would keep missing. The linker script (`memory.x`) collects all of tnc2-core and the Z80 core, plus the firmware function that calls the main loop, into one section that is copied to SRAM at start up. Everything else (USB, start up, printing) stays in flash.
- **Watchdog.** While the test runs, the watchdog restarts the chip if the firmware stops feeding it for three seconds. It counts from the 12 MHz crystal, so it still works when the system clock does not. The results so far, and which clock was being tried, sit in four watchdog scratch registers, which survive the restart; a panic records itself there too and restarts the chip at once.
- **Changing the clock while running.** embassy-rp sets the clocks once at start up. The firmware then raises the voltage, slows the flash interface to match, and moves the system PLL itself, from SRAM with interrupts off (`src/clock.rs`). USB has its own 48 MHz PLL and carries on regardless. The peripheral clock also runs from that PLL, so UART speeds will not move with the system clock.
- **Flash layout.** The firmware may use the first 3840 KB of the 4 MB flash. The 252 KB after that is set aside for the saved RAM and node settings. The last 4 KB is left alone, because UF2s from the Pico SDK write a workaround block there.

## Next: the node on two cores

The RP2350 has two Arm cores, and the plan uses both:

- **Core 0 runs the Z80 and nothing else.** No interrupts. It runs the board in slices of about a millisecond, reading the hardware timer to keep emulated time locked to real time: when it is ahead it waits, when it is behind (after a busy moment) it catches up. All the code it touches, the memory allocator included, will live in SRAM, so it never waits on flash.
- **Core 1 does all the I/O**, with Embassy: both USB serial ports, the NinoTNC on UART1 (receive at 57600 8N1 on the UART, transmit with two stop bits from a PIO state machine, which the NinoTNC needs for back to back frames), the setup commands, and flash writes.
- **Between them**, a few single producer, single consumer queues in shared SRAM: radio frames each way (core 1 does the KISS framing), console bytes each way, and a request to snapshot the RAM. Core 0 checks them between slices, which costs it nothing measurable. Core 1 is woken through the inter-core FIFO when core 0 has something to send.
- **One clock, set once.** The node will pick its clock at start up rather than step through several. That matters because embassy-rp keeps its own note of the system clock (`clk_sys_freq()`) from start up, and the PIO program sending to the NinoTNC works out its divider from that, so after a change at run time the baud rate would be wrong. Setting the final clock before anything reads it avoids the problem.
- **Saving the RAM.** Every few minutes core 1 asks for a snapshot; core 0 copies the 32 KB between two slices, and core 1 writes it to flash only if it changed since the last save, alternating between two slots so a power cut during a write still leaves the previous copy. Because core 0 runs entirely from SRAM, it keeps emulating while core 1 erases and writes the flash.
