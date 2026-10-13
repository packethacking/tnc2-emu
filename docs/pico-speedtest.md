# The Pico 2 W speed test

TheNet never sleeps: it sits in a busy loop waiting for work, so a Pico 2 W running the node has to emulate the Z80 at the full 4.9152 MHz all the time. The speed test measures whether it can, and at what clock. Its result settled the node's clock, see [pico.md](pico.md#the-clock).

It is a separate firmware, the speed test, with the same two USB serial ports as the node. The first only prints a note; the results are on the second. Building it is covered in [pico.md](pico.md#building).

## Running the speed test

1. Get the speed test: `tnc2-emu-<version>-pico2w-speedtest.uf2` from the latest [release](https://github.com/packethacking/tnc2-emu/releases), or for the newest code the `tnc2-pico-speedtest` artifact of the latest run of the `ci` workflow (Actions tab), or build it yourself ([pico.md](pico.md#building)).
2. Hold the BOOTSEL button on the Pico 2 W while plugging it in. A drive called `RP2350` appears.
3. Copy the UF2 onto that drive. The Pico restarts by itself and the drive goes away. (Or, with [picotool](https://github.com/raspberrypi/picotool): `picotool load -x tnc2-emu-<version>-pico2w-speedtest.uf2`, or the name of the file you built.)
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

What we expect, from counting the instructions the emulator runs (see `tools/m33bench`): about 15 Arm instructions per Z80 clock cycle, which comes to somewhere between 103 and 140 MHz for real time. The measured 23 cycles per Z80 clock, below, matches that estimate.

## Results

The first real board, October 2026:

```
tnc2-pico 0.2.0 speed test: TheNet X1J on an emulated 4.9152 MHz Z80
Each clock runs the node from power on for 60 emulated seconds.
103 KB of code and constants run from SRAM.

clock    counted  core    Z80 MHz  ratio  cycles  RAM hash
150 MHz  150.00   1.10 V   6.479   1.32x   23.2   ae7ab48fd2b132e4 matches
200 MHz  200.00   1.15 V   8.639   1.76x   23.2   ae7ab48fd2b132e4 matches
250 MHz  250.00   1.25 V  10.799   2.20x   23.2   ae7ab48fd2b132e4 matches
300 MHz  300.00   1.30 V  12.959   2.64x   23.1   ae7ab48fd2b132e4 matches

GO: fast enough, with room to spare, at 150 MHz (1.32x real time).
Finished, at 150 MHz.
```

GO at the RP2350's stock 150 MHz, so that is what the node uses, see [pico.md](pico.md#the-clock).

## The clocks

The RP2350 is rated for 150 MHz. Everything above that is overclocking, which Raspberry Pi does not promise works, though plenty of people run these chips faster. The test raises the core voltage before each overclock, a little more each time, and never past 1.30 V. That is as high as the regulator goes unless its voltage limit is disabled, and the firmware does not disable it (it sets the regulator's unlock bit, which any voltage change needs, but not the one that lifts the limit):

| Clock | Core voltage |
|-------|--------------|
| 150 MHz | 1.10 V (stock) |
| 200 MHz | 1.15 V |
| 250 MHz | 1.25 V |
| 300 MHz | 1.30 V |

The flash chip's own clock is kept at 50 MHz throughout, as the boot ROM set it.
