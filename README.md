# tnc2-emu

A real 1990s TheNet X1J NET/ROM node, running in a container.

TheNet was the node firmware on thousands of Z80 TNC-2 boards in the late
80s and 90s. It only ever existed as EPROM code, so once the hardware went
in the bin, so did the node. tnc2-emu emulates the TNC-2 (Z80 CPU, Z80 SIO,
bank switched EPROM, battery backed RAM) well enough to run the original,
unmodified TheNet X1J release 2 EPROM images.

The radio port comes out as KISS over TCP and the RS232 port as a plain TCP
console. Link it to XRouter, BPQ, Direwolf or anything else that speaks TCP
KISS, and you have a genuine TheNet node on your network: NODES
broadcasts, level 2 and NET/ROM connects, the lot. Handy for testing how
modern node software gets on with the old guard, and fun for anyone who
remembers typing `C GB7XXX` at one.

## Quick start

```sh
docker run --rm -p 8001:8001 -p 8002:8002 \
  -e TNC2_CALL=G9TNT-1 -e TNC2_ALIAS=TNET \
  -v tnc2-data:/data ghcr.io/packethacking/tnc2-emu
```

Then `nc localhost 8002`, press Escape, type `C` and Enter: you are on the
node's command line. Try `?`, `NODES`, `ROUTES`, `PARMS`, `STATS`.

The image runs on amd64, arm64 and 32 bit ARM (a Raspberry Pi is fine).

### TheNet and XRouter together

`examples/compose` brings up a TheNet node and an XRouter node linked over
TCP KISS:

```sh
cd examples/compose
docker compose up -d
nc localhost 8002          # TheNet
telnet localhost 8023      # XRouter, log in with any callsign
```

After a few minutes each lists the other in `NODES`. From XRouter try
`C TNET`, from TheNet try `C XRNODE`.

## Linking it to your own node

The emulator is a KISS TCP server on port 8001. Point your node's TCP KISS
client at it:

- XRouter: an `INTERFACE` with `TYPE=TCP`, `PROTOCOL=KISS`,
  `IOADDR=<emulator IP>`, `INTNUM=8001`, and a `PORT` on it (see
  `examples/compose/XROUTER.CFG`).
- BPQ: a KISS port with `TYPE=ASYNC`, `PROTOCOL=KISS` and
  `IPADDR`/`TCPPORT` pointing at the emulator.
- Direwolf, or anything else with a TCP KISS client: same idea.

Only XRouter has been tested so far.

Frames are passed whole, as if over a perfect 1200 bit/s radio channel. The
node's NODES broadcast interval defaults to 30 minutes, and its radio port
quality to 10, which is too low to learn routes; most people will want
`TNC2_POKE="4C=C0,00"` (quality 192) and perhaps `54=2C,01` (broadcast every
300 seconds).

## Settings

Environment variables (the container) or flags (the binary):

| Env | Flag | What |
|-----|------|------|
| `TNC2_CALL` | `--call` | node callsign and SSID, e.g. `G9TNT-1` |
| `TNC2_ALIAS` | `--alias` | node alias, up to 6 characters |
| `TNC2_POKE` | `--poke` | patch the ROM configuration block, `OFFSET=HH,HH` in hex; separate several with `;` |
| `TNC2_KISS_PORT` | `--kiss-port` | KISS TCP port, default 8001 |
| `TNC2_CONSOLE_PORT` | `--console-port` | console TCP port, default 8002 |
| `TNC2_RAM_FILE` | `--ram-file` | battery backed RAM, `/data/tnc2.ram` in the image |
| `TNC2_SPEED` | `--speed` | 1 is real time, 2 twice as fast, 0 flat out (the node's clock speeds up too) |
| `TNC2_HDLC_BAUD` | `--hdlc-baud` | radio bit rate, default 1200 |
| `TNC2_MONITOR` | `--monitor` | print decoded radio frames to the log (on in the image) |
| `TNC2_CAPTURE` | `--capture` | append every radio frame, as hex, to a file |

The configuration block layout is in `docs/CONFIGUR.TXT`; the PARMS
numbers map onto 16 bit words from offset 0x48. Changing the callsign,
alias or any poke makes the node cold start, because the saved RAM no
longer matches; otherwise the RAM, and with it anything you changed from the
sysop console, survives restarts.

## What works

- Boots TheNet X1J release 2 and signs on; every sysop command tried so far
  works.
- NODES broadcasts at the right times: the TNC-2's 600 Hz timer signal is
  generated just as the board did, so the node's clock keeps real time.
- Tested against XRouter 505c and 505o with `interop/run.py`: both nodes
  learn each other from NODES broadcasts, XRouter connects to TheNet at
  level 2 and over NET/ROM, and TheNet connects to XRouter over NET/ROM and
  by a level 2 downlink. Raw transcripts and frame captures are in
  `evidence/`.

## Limits

- No bit level HDLC: frames are passed whole with no CRC, and the channel
  never collides or loses anything. A frame that arrives while the node is
  transmitting waits until it has finished, as it would behind carrier
  detect.
- One radio port, as on the real board.
- The RS232 port runs as the sysop console. `--protocol-mode` switches it to
  TheNet's KISS/crosslink mode, but nothing is connected to that side yet.
- The deviation and S meter ADC board is not fitted (reads zero), and the
  CW ident is not audible anywhere.

How the hardware was worked out, port by port, is in `docs/hardware.md`.

## Building

`cargo build --release`, or `docker build -t tnc2-emu .` for the image.
`cargo test` boots the firmware and checks the console and the first NODES
broadcast.

## Credits and licences

- TheNet X1J by Dave Roberts G8KBB, based on TheNet 1.01 by NORD><LINK
  (DF2AU and DC4OX); the deviation meter design is by Neville Pattinson
  G0JVU. The firmware and documentation are free for amateur, non-commercial
  use with the source acknowledged (`rom/COPYRITE.TXT`). The original
  distribution archive is in `docs/`.
- The Z80 core is the rustzx-z80 crate from the RustZX project, MIT
  licensed (`third_party/rustzx-z80-LICENSE.txt`).
- The emulator itself is MIT licensed (`LICENSE`).
