## Which file do I need?

| File | Use it for |
|---|---|
| Docker image `ghcr.io/packethacking/tnc2-emu:{version}` | **The easiest way to run the node**, on a PC or a Raspberry Pi (amd64, arm64 and 32 bit ARM). Not a download here: `docker pull ghcr.io/packethacking/tnc2-emu:{version}`, then see the [quick start](https://github.com/packethacking/tnc2-emu/blob/v{version}/README.md#quick-start). |
| `tnc2-emu-{version}-x86_64-unknown-linux-musl.tar.gz` | **The node without Docker**, on 64 bit Intel or AMD Linux. |
| `tnc2-emu-{version}-aarch64-unknown-linux-musl.tar.gz` | The same, for 64 bit ARM Linux, such as a Raspberry Pi 3, 4 or 5 on a 64 bit OS. |
| `tnc2-emu-{version}-armv7-unknown-linux-musleabihf.tar.gz` | The same, for 32 bit ARM Linux, such as a Raspberry Pi 2 or later on a 32 bit OS. |
| `tnc2-emu-{version}-pico2w-node.uf2` | **The node on a Raspberry Pi Pico 2 W**, with a NinoTNC as the radio. Not yet tried on a real board. See [docs/pico.md](https://github.com/packethacking/tnc2-emu/blob/v{version}/docs/pico.md). |
| `tnc2-emu-{version}-pico2w-speedtest.uf2` | The Pico 2 W speed test, which shows whether a board keeps up with a real TNC-2. Not needed to run the node. See [docs/pico-speedtest.md](https://github.com/packethacking/tnc2-emu/blob/v{version}/docs/pico-speedtest.md). |
| `SHA256SUMS` | Checksums of the files above, to check a download: `sha256sum -c --ignore-missing SHA256SUMS`. |

The Linux binaries are static, so there is nothing else to install. Unpack one and run `./tnc2-emu` in its folder: it finds the TheNet EPROM images in `rom/` there, and takes the flags in the README's [settings](https://github.com/packethacking/tnc2-emu/blob/v{version}/README.md#settings) table.

For the Pico, hold BOOTSEL while plugging it in and copy the UF2 onto the drive that appears. Flashing a new version normally keeps the node's saved RAM and settings.

**Licences.** The emulator is AGPL-3.0. TheNet X1J is copyright Dave Roberts G8KBB (based on TheNet 1.01 by NORD><LINK, DF2AU and DC4OX), free for amateur, non-commercial use with the source acknowledged (`rom/COPYRITE.TXT`). The Z80 core, rustzx-z80, is MIT. The UF2s contain all three; the full texts are in each Linux archive and in the source.
