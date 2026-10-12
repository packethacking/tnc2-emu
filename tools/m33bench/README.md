# m33bench: Arm instructions per Z80 clock

Runs the same 60 emulated seconds as `cargo run --release -p tnc2-core --example bench`, built the way the Pico firmware is (Cortex-M33, fat LTO, `opt-level = 3`), on QEMU's Cortex-M33 board `mps2-an505` with `-icount shift=0`, and counts the Arm instructions it takes. It needs `qemu-system-arm`:

```sh
cd tools/m33bench && cargo run --release
```

It prints the RAM hash, which must match the host's, the Arm instructions per Z80 clock, and the RP2350 clock real time would need at 1.4 to 1.9 cycles per instruction. QEMU does not model timing, so that range is an estimate from the instruction mix, not a measurement; the build is also not byte for byte the firmware's. Use it to compare one version of the core with another, and the speed test on a real board for the real answer.
