# qemu-pace: the node's core 0 loop on a Cortex-M33

Runs the Pico node's core 0 loop (`tnc2_node::engine`, the same code the firmware runs) on QEMU's Cortex-M33 board `mps2-an505` with `-icount shift=0`, pacing against the board's free running counter. Virtual time then moves on a fixed amount per instruction, so the harness can say how many instructions make a second of "real" time and so pretend the processor is faster or slower than the emulator needs. It needs `qemu-system-arm`:

```sh
cd tools/qemu-pace && cargo run --release
```

It checks three things and prints what it saw:

1. With a processor that has room to spare (100 million instructions a second), TheNet signs on through the console pipes, emulated time stays within 2 ms of real time and is never ahead of it, and the loop spends the spare time waiting.
2. With one that is too slow (55 million), the node falls behind steadily, at the speed it can manage.
3. Fast again, it runs flat out until it has caught up.

QEMU does not model timing, so this checks the pacing logic on the real instruction set, not how fast a Pico 2 W is; the speed test on a real board answers that.
