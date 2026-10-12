# tnc2-emu

A real 1990s TheNet X1J NET/ROM node, running in a container.

TheNet was the node firmware on thousands of Z80 TNC-2 boards in the late
80s and 90s. It only ever existed as EPROM code, so once the hardware went
in the bin, so did the node. tnc2-emu emulates the TNC-2 (Z80 CPU, Z80 SIO,
bank switched EPROM, battery backed RAM) well enough to run the original,
unmodified TheNet X1J release 2 EPROM images.

The radio port comes out as KISS over TCP and the RS232 port as a plain TCP
console. Link it to XRouter, LinBPQ or any other node that speaks TCP
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

### TheNet and LinBPQ together

`examples/compose-linbpq` does the same with a LinBPQ node, using the `m0lte/linbpq` image:

```sh
cd examples/compose-linbpq
docker compose up -d
nc localhost 8002          # TheNet
telnet localhost 8010      # LinBPQ, user sysop, password changeme
```

Both examples put TheNet's console on port 8002, so stop the XRouter one first (`docker compose down` in `examples/compose`). About a minute after start each node lists the other in `NODES`. From LinBPQ try `C TNET` (NET/ROM) or `C 2 !G9TNT-1` (a plain level 2 connect on port 2; without the `!` LinBPQ goes over NET/ROM, because G9TNT-1 is in its `NODES`), from TheNet try `C BPQNOD`. A LinBPQ session looks like this:

```
NODES
BPQNOD:G9BPQ} Nodes
TNET:G9TNT-1
C TNET
BPQNOD:G9BPQ} Connected to TNET:G9TNT-1
NODES
TNET:G9TNT-1} Nodes:
BPQNOD:G9BPQ
```

## Linking it to your own node

The emulator is a KISS TCP server on port 8001. Point your node's TCP KISS
client at it:

- XRouter: an `INTERFACE` with `TYPE=TCP`, `PROTOCOL=KISS`,
  `IOADDR=<emulator IP>`, `INTNUM=8001`, and a `PORT` on it (see
  `examples/compose/XROUTER.CFG`).
- LinBPQ: a `PORT` with `TYPE=ASYNC`, `PROTOCOL=KISS`, `IPADDR=<emulator IP>` and `TCPPORT=8001`, plus a `ROUTES` entry for the TheNet node with `NOV2.2=1` (see below and `examples/compose-linbpq/bpq32.cfg`).
- Any other node with a TCP KISS client: same idea.

XRouter and LinBPQ have been tested; others have not.

Frames are passed whole, as if over a perfect 1200 bit/s radio channel. The
node's NODES broadcast interval defaults to 30 minutes, and its radio port
quality to 10, which is too low to learn routes; most people will want
`TNC2_POKE="4C=C0,00"` (quality 192) and perhaps `54=2C,01` (broadcast every
300 seconds).

### LinBPQ needs `NOV2.2=1`

LinBPQ opens its node to node links with an AX.25 2.2 XID frame. TheNet is an AX.25 2.0 node and ignores XID, and LinBPQ gives up on a neighbour that stays silent instead of falling back to a plain connect. Without the fix TheNet can drop out of LinBPQ's `NODES`, and `C TNET` from LinBPQ fails with `Error - Node TNET:G9TNT-1 has disappeared`. Level 2 connects (`C 2 !G9TNT-1`) and connects from TheNet to LinBPQ still work. Tell LinBPQ to use AX.25 2.0 with that neighbour. From `examples/compose-linbpq/bpq32.cfg`, where `172.29.75.10` is the emulator:

```
PORT
 ID=TheNet TNC2
 TYPE=ASYNC
 PROTOCOL=KISS
 IPADDR=172.29.75.10
 TCPPORT=8001
 QUALITY=192
 MAXFRAME=2
 PACLEN=120
 FRACK=8000
 RESPTIME=1500
 RETRIES=10
ENDPORT

ROUTES:
CALL=G9TNT-1,PORT=2,QUALITY=192,NOV2.2=1
***
```

`CALL` is the TheNet node's callsign and `PORT` is the number of the KISS port above (the second `PORT` block in the example, after Telnet). With that in place LinBPQ connects with a plain SABM and the link comes straight up.

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
- Tested against LinBPQ 6.0.25.41 (`m0lte/linbpq`) with `interop/run_linbpq.py`: the same checks, plus LinBPQ's node link to TheNet coming up. `--without-nov22` repeats the run without the `ROUTES` entry above, to show the failure. Evidence is in `evidence/linbpq-*`.

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

The repo is a Cargo workspace. The board itself (Z80, SIO, EPROM, RAM, KISS framing) lives in `crates/tnc2-core`, a `no_std` library with no I/O of its own, ready for a microcontroller port. The `tnc2-emu` binary is the TCP host around it. To see how fast your machine runs the board flat out:

```sh
cargo run --release -p tnc2-core --example bench
```

## Credits and licences

- TheNet X1J by Dave Roberts G8KBB, based on TheNet 1.01 by NORD><LINK
  (DF2AU and DC4OX); the deviation meter design is by Neville Pattinson
  G0JVU. The firmware and documentation are free for amateur, non-commercial
  use with the source acknowledged (`rom/COPYRITE.TXT`). The original
  distribution archive is in `docs/`.
- The Z80 core is the rustzx-z80 crate from the RustZX project, MIT
  licensed (`third_party/rustzx-z80-LICENSE.txt`).
- The emulator itself is licensed under the AGPL-3.0 (`LICENSE`, `NOTICE`).
