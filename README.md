# tnc2-emu

A TAPR TNC-2 emulator that runs the real TheNet X1J NET/ROM firmware, so a
1990s TheNet node can sit on a container test network and talk to modern
node software such as XRouter.

The Z80 runs the unmodified EPROM images. The radio port comes out as KISS
over TCP and the RS232 port as a raw TCP console, so you can link it to
anything that speaks TCP KISS and log in as sysop with netcat.

## Run it

With Docker:

```sh
docker build -t tnc2-emu .
docker run --rm -p 8001:8001 -p 8002:8002 \
  -e TNC2_CALL=G9TNT-1 -e TNC2_ALIAS=TNET \
  -v tnc2-data:/data tnc2-emu
```

Or with cargo:

```sh
cargo run --release -- --call G9TNT-1 --alias TNET --ram-file node.ram --monitor
```

Then:

- Point a TCP KISS client at port 8001 (XRouter: `TYPE=TCP`,
  `PROTOCOL=KISS`, `IOADDR=<emulator address>`, `INTNUM=8001`).
- `nc localhost 8002`, press Escape, type `C` and Enter, and you are on the
  node's command line. Try `?`, `NODES`, `ROUTES`, `PARMS`, `STATS`.

Useful settings (flag, or environment variable for the container):

| Flag | Env | What |
|------|-----|------|
| `--call` | `TNC2_CALL` | node callsign and SSID |
| `--alias` | `TNC2_ALIAS` | node alias |
| `--poke 54=3C,00` | `TNC2_POKE` | patch the ROM config block (here: NODES every 60 s); `;` separates several in the env var |
| `--ram-file` | `TNC2_RAM_FILE` | battery backed RAM, default `/data/tnc2.ram` in the image |
| `--speed` | `TNC2_SPEED` | 1 is real time, 0 is flat out (the firmware's clock speeds up too) |
| `--hdlc-baud` | `TNC2_HDLC_BAUD` | radio bit rate, default 1200 |
| `--monitor` | `TNC2_MONITOR` | print decoded radio frames (on in the image) |
| `--capture` | `TNC2_CAPTURE` | log every radio frame as hex |

`--help` lists the rest. The config block layout is in
`docs/CONFIGUR.TXT`; for example `4C=C0,00` sets the radio port quality to
192 (the distributed default is 10). Changing the callsign or any poke
makes the node cold start, because the old RAM no longer matches.

## What works

- Boots TheNet X1J release 2, sign-on and every sysop command we tried.
- NODES broadcasts at the configured interval, with correct timing (the
  firmware's 10 ms tick is generated the way the TNC-2 does it).
- Full NET/ROM interop with XRouter over TCP KISS: both sides learn each
  other, level 2 and NET/ROM connects work in both directions. Run
  `interop/run.py` to repeat it; results and raw transcripts are in
  `evidence/`.
- Battery backed RAM persists across restarts.

## What does not

- No bit level HDLC: frames are passed whole, there is no real CRC and no
  collisions. The channel is ideal: a frame sent while the node is
  transmitting waits until it has finished.
- The RS232 port is dumb terminal only by default; `--protocol-mode` gives
  the KISS/crosslink mode but is untested.
- The deviation meter / S meter ADC reads as zero, and the CW ID keyer is
  not heard anywhere.
- One radio port only, as on the real board.

How the hardware was worked out is in `docs/hardware.md`.

## Licences

The emulator is MIT licensed (`LICENSE`). TheNet X1J and its documentation
are copyright Dave Roberts G8KBB and others, free for amateur,
non-commercial use with the source acknowledged (`rom/COPYRITE.TXT`); the
original distribution archive is in `docs/`. The Z80 core is the MIT
licensed rustzx-z80 crate (`third_party/rustzx-z80-LICENSE.txt`).
