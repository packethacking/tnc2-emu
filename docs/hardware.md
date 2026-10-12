# What TheNet X1J expects from the TNC-2 hardware

Notes from disassembling TheNet X1J release 2 (THENET1.X1J and THENET2.X1J)
and from the documentation in the distribution archive. Addresses are in
the firmware unless marked as RAM.

## Memory map

- 0x0000 to 0x7FFF: EPROM, 32 KB visible at a time out of a 27512 (64 KB).
- 0x8000 to 0xFFFF: 32 KB static RAM, battery backed on a real TNC-2.

The two ROM files are the two halves of the 27512. The distributed files are
32632 and 32580 bytes long because the trailing erased bytes were trimmed;
the emulator pads them with 0xFF to 32 KB.

## Bank switching

EPROM pin 1 (A15) is wired to the SIO channel A DTR pin (BANKSWIT.TXT). The
firmware switches banks in the routine at 0x0F3F by rewriting WR5 of
channel A:

- WR5A bit 7 set (DTR pin active, low) selects the lower half, THENET1.X1J.
- WR5A bit 7 clear (DTR pin high) selects the upper half, THENET2.X1J.

After reset WR5 is zero, so the CPU starts in the upper half. Both halves
carry identical reset, interrupt and driver code in their low pages, so
execution continues seamlessly across a switch. The CPU must see the switch
on the very next instruction fetch. Part 1 holds levels 2 to 4, part 2 holds
the command interpreter (the "switch").

## I/O ports

Only these are used:

| Port | Device |
|------|--------|
| 0x00 | SIO channel A data (radio, HDLC) |
| 0x01 | SIO channel A control |
| 0x02 | SIO channel B data (RS232, async) |
| 0x03 | SIO channel B control |
| 0x20 | ADC0844 on the optional deviation / S meter board |

Port address bit 0 is the SIO C/D select and bit 1 the B/A select. The
earlier scan's hits at 0x0B, 0x18 and 0x28 are data being disassembled as
code. There is no CTC and no PIO. The ADC is written with a channel
command (0x04, 0x05) when a frame starts or ends and read in the end of
frame handler; the emulator returns 0 (board not fitted), which only
affects the deviation and S meter columns of MHEARD.

## Interrupts

Interrupt mode 2, I = 0x80. At reset the eight vectors at ROM 0x0174 are
copied to RAM 0x8000. The SIO has "status affects vector" set (WR1B = 0x1F)
and base vector 0 (WR2B = 0), so the vector is (code << 1):

| Code | Source | Handler |
|------|--------|---------|
| 0 | B transmit buffer empty | 0x030F |
| 1 | B external/status | 0x03D1 |
| 2 | B receive character | 0x04CE |
| 3 | B special receive | 0x05A0 |
| 4 | A transmit buffer empty | 0x05C2 |
| 5 | A external/status | 0x0633 |
| 6 | A receive character | 0x069C |
| 7 | A special receive | 0x06CE |

## Time base

There is no timer chip. The time base is a square wave on the SIO SYNCB
input: every edge changes RR0B bit 4 and raises a channel B external/status
interrupt. The handler at 0x03D1 counts edges in RAM 0x8228 and every 12
edges calls 0x0BEA, the 10 ms tick (CW keyer speed is set in 10 ms units,
"6 = 20 WPM", which confirms the 10 ms tick). So the firmware wants 1200
edges a second, a 600 Hz square wave, which is 4.9152 MHz / 8192 from the
TNC-2 clock divider. The emulator generates exactly that and never lets two
edges merge into one interrupt (a missed edge would be invisible to the
handler, which looks for changes).

## SIO channel A: radio, SDLC

Initialised from the table at 0x0168: WR4 = 0x20 (SDLC, x1 clock),
WR5 = 0xE1, WR3 = 0xD9 (8 bits, enter hunt, Rx CRC, Rx enable), WR7 = 0x7E,
WR1 = 0x1B (ext, Tx and all-character Rx interrupts).

Transmit (handler 0x05C2, state byte in RAM 0x8227):

- Channel access: p-persistence and slot time from the parameters, DCD
  (RR0A bit 3) holds off, then WR5 sets RTS and Tx enable, TXDELAY counts
  down in 10 ms ticks.
- A frame starts in 0x07FF: "reset Tx CRC generator" (WR0 = 0x80), write
  the first byte, then "reset Tx underrun/EOM latch" (WR0 = 0xC0).
- Each Tx buffer empty interrupt writes the next byte. When the queue is
  empty the handler writes nothing; the SIO underruns and sends the CRC and
  a closing flag, and the next Tx buffer empty interrupt either starts the
  next queued frame or sends three padding bytes (0x00) before dropping RTS
  and Tx enable. The padding bytes go out after the EOM latch has set, so
  they are not part of any frame.
- An underrun while still in the data state (ext/status interrupt with RR0A
  bit 6) makes the firmware send an abort.
- If a new frame is queued while the padding bytes are going out (state 8
  to 10), 0x028A sets the state back to 6 and sends an abort. Both aborts
  leave the firmware waiting in state 6 for a Tx buffer empty interrupt to
  start the next frame. The SIO gives one: an abort throws away the
  character in the transmit buffer (or the CRC being sent), and the buffer
  becoming empty interrupts as usual. Without that interrupt the node stays
  keyed up in state 6 and never transmits again (issue 4).

The emulator treats every byte written between "reset Tx CRC" and the
underrun as the frame, and sends it out as one KISS frame.

Receive (handlers 0x069C and 0x06CE): the character handler stores every
byte one character late, so the last byte before end of frame (the first
CRC byte) is dropped. The special receive handler reads RR1, expects bit 7
(end of frame), bit 6 clear (CRC good) and residue code 011 (RR1 & 0x0E =
0x06), and does not read the data register itself. The emulator therefore
delivers each frame as its data bytes, a dummy CRC byte as a normal
character, and a second dummy CRC byte flagged end of frame.

DCD (RR0A bit 3) is what stops the node keying up on top of a frame it is
receiving. The emulator raises it two character times before a received
frame starts and drops it after the closing flag, with the ext/status
interrupts the real chip gives; the rest of the time the channel looks
clear, so the node can key up straight away. Without DCD the node would
sometimes start transmitting mid frame (in half duplex it switches its
receiver off to transmit) and lose the frame. CTS is not used on channel A.

## SIO channel B: RS232, async

Initialised from 0x015C: WR4 = 0x44 (x16, 1 stop bit), WR5 = 0xEA,
WR3 = 0xC1, WR1 = 0x1F. The firmware does not program a baud rate; on a
TNC-2 that comes from jumpers.

DCDB (RR0B bit 3, RS232 pin 23) picks the port mode (routine 0x0867):

- DCD asserted: dumb terminal mode, the sysop console. Escape gives "* ",
  then C connects to the node.
- DCD not asserted: protocol mode, KISS or crosslink (a SLIP style framing
  with 0x02/0x03/0x10 for crosslink or 0xC0/0xDB for KISS).

CTSB is used for hardware handshake host mode only; the emulator holds it
asserted.

## Configuration

The configuration block is at 0x003B in both halves (CONFIGUR.TXT has the
full table) and must be patched identically in both. On a cold start the
firmware copies it to RAM; after that the sysop commands change the RAM
copy, which survives power cycles thanks to the battery. Warm start is
detected by the word "ZM" (0x4D5A) at RAM 0x8232.

- 0x003B: callsign, 6 ASCII characters, space padded.
- 0x0041: SSID as 0x60 | (ssid << 1). The distributed image has 0x6A, which
  the node shows as G8KBB-5.
- 0x0042: alias, 6 characters, space padded.
- 0x004C: radio port default quality (word, distribution default 10).
- 0x0054: NODES broadcast interval in seconds (word, default 1800).
- 0x0080: 80 character sysop password.

The emulator patches callsign and alias from the command line, applies any
--poke patches, and keeps the RAM in a file. If the patched ROM differs from
the one the RAM file was saved with, the RAM is discarded and the node cold
starts, so new settings take effect.
