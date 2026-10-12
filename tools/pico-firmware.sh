#!/bin/bash
# Builds the Pico 2 W firmware (the speed test and the node), checks the
# code sits where it has to, and makes the UF2s:
#
#     tools/pico-firmware.sh OUTDIR
#
# leaves OUTDIR/tnc2-pico-speedtest.uf2 and OUTDIR/tnc2-pico-node.uf2.
# CI and the release workflow both run this, so what CI checks is what
# gets released. Needs rustup (crates/tnc2-pico/rust-toolchain.toml picks
# the toolchain), curl, python3 and libusb-1.0 (for picotool), on x86_64
# Linux.
set -euo pipefail

out=$(realpath -m "${1:?usage: tools/pico-firmware.sh OUTDIR}")
root=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$out"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

cd "$root/crates/tnc2-pico"
elf=target/thumbv8m.main-none-eabihf/release
bins="speedtest node"

echo "== Building the firmware"
cargo clippy --release --locked -- -D warnings
cargo build --release --locked

nm="$(rustc --print sysroot)/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-nm"
size="$(rustc --print sysroot)/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-size"

echo "== Checking the emulator's hot code is linked into SRAM"
check() {
	"$nm" -C "$elf/$1" > "$tmp/symbols.txt"
	shift
	for f in "$@"; do
		line=$(grep -E " [tT] $f::h[0-9a-f]+$" "$tmp/symbols.txt" || true)
		echo "$line"
		case "$line" in 20*) ;; *) echo "$f is not in SRAM"; exit 1 ;; esac
	done
}
check speedtest speedtest::run_batch tnc2_core::Board::run tnc2_core::sio::Channel::run \
	rustzx_z80::cpu::Z80::emulate rustzx_z80::opcode::group_nonprefixed::execute_normal
check node node::core0::run tnc2_core::sio::Channel::run \
	rustzx_z80::cpu::Z80::emulate rustzx_z80::opcode::group_nonprefixed::execute_normal

echo "== Checking nothing but start up code is left in flash"
# The node's core 0 keeps running while core 1 writes the flash, when
# reading it is a bus fault: all code and read only data must be in SRAM
# (memory.x). In flash: the reset handler, the copy (main), embassy-rp's
# __pre_init, the hard fault handler and the long branches from them into
# SRAM. Nothing else.
for bin in $bins; do
	"$size" -A "$elf/$bin" | awk '$1 == ".rodata" && $2 != 0 { print "read only data in flash: " $2 " bytes"; bad = 1 } END { exit bad }'
	left=$("$nm" "$elf/$bin" | awk '$1 ~ /^1/ && $2 ~ /[tT]/ { print $3 }' | sort -u \
		| grep -vxE 'Reset|__stext|__etext|main|__pre_init|HardFault|HardFault_|__Thumbv7ABSLongThunk_(main|__pre_init)' || true)
	if [ -n "$left" ]; then echo "$bin has code in flash:"; echo "$left"; exit 1; fi
	echo "$bin: only start up code in flash"
done

echo "== Making the UF2s"
# picotool 2.0 or later knows the RP2350; this is Raspberry Pi's own Linux
# build, pinned by its checksum.
curl -sSLo "$tmp/picotool.tar.gz" https://github.com/raspberrypi/pico-sdk-tools/releases/download/v2.3.1-0/picotool-2.3.1-x86_64-lin.tar.gz
echo "878a1a1e7d8336b8be60300c7218f366d9790c0d3a211b181d6d172e80b1248e  $tmp/picotool.tar.gz" | sha256sum -c
tar xzf "$tmp/picotool.tar.gz" -C "$tmp"
for bin in $bins; do
	"$tmp/picotool/picotool" uf2 convert "$elf/$bin" -t elf "$out/tnc2-pico-$bin.uf2" --family rp2350-arm-s
	"$tmp/picotool/picotool" info -a "$out/tnc2-pico-$bin.uf2"
done

echo "== Checking each UF2 is one RP2350 region the boot ROM can take in one go"
python3 - "$out/tnc2-pico-speedtest.uf2" "$out/tnc2-pico-node.uf2" <<'PY'
import os, struct, sys
for path in sys.argv[1:]:
    name = os.path.basename(path)
    d = open(path, "rb").read()
    blocks = [struct.unpack("<8I", d[i:i + 32]) for i in range(0, len(d), 512)]
    assert {b[7] for b in blocks} == {0xE48BFF59}, f"{name}: family is not rp2350-arm-s"
    addrs = [b[3] for b in blocks]
    assert all(b == a + 256 for a, b in zip(addrs, addrs[1:])), f"{name}: UF2 has gaps"
    # the image must stay clear of the store and the last 4 KB
    assert addrs[-1] + 256 <= 0x103C0000, f"{name}: runs into the store"
    print(f"{name}: {len(blocks)} blocks, 0x{addrs[0]:08x} to 0x{addrs[-1] + 256:08x}, rp2350-arm-s")
PY
