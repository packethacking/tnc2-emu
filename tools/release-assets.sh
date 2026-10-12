#!/bin/bash
# Builds everything a GitHub release carries, into OUTDIR:
#
#     tools/release-assets.sh VERSION OUTDIR
#
#   tnc2-emu-VERSION-<target>.tar.gz   static Linux binary, the EPROM images,
#                                      the licences and the README, for
#                                      x86_64, aarch64 and armv7
#   tnc2-emu-VERSION-pico2w-node.uf2       the Pico 2 W node firmware
#   tnc2-emu-VERSION-pico2w-speedtest.uf2  the Pico 2 W speed test
#   SHA256SUMS                         checksums of all of the above
#
# The release workflow runs it; it works the same on a Linux PC with
# Docker buildx and what tools/pico-firmware.sh needs. The Linux binaries
# come from the Dockerfile's build stage, cross compiled on this machine
# like the image's, so no emulation is needed. The Docker image itself is
# publish.yml's job.
set -euo pipefail

version=${1:?usage: tools/release-assets.sh VERSION OUTDIR}
out=$(realpath -m "${2:?usage: tools/release-assets.sh VERSION OUTDIR}")
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
if [ -e "$out" ] && [ -n "$(ls -A "$out")" ]; then
	echo "$out is not empty"
	exit 1
fi
mkdir -p "$out"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# Archive timestamps from the commit, so a rebuild makes the same files.
mtime=$(git log -1 --format=%ct)

for target in x86_64-unknown-linux-musl aarch64-unknown-linux-musl armv7-unknown-linux-musleabihf; do
	case "$target" in
		x86_64-*) platform=linux/amd64 ;;
		aarch64-*) platform=linux/arm64 ;;
		armv7-*) platform=linux/arm/v7 ;;
	esac
	echo "== Building the $target binary ($platform)"
	docker buildx build --platform "$platform" --target binary --output "type=local,dest=$tmp/bin-$target" .
	name="tnc2-emu-$version-$target"
	dir="$tmp/$name"
	mkdir -p "$dir/rom" "$dir/third_party"
	cp "$tmp/bin-$target/tnc2-emu" "$dir/"
	cp rom/THENET1.X1J rom/THENET2.X1J rom/COPYRITE.TXT "$dir/rom/"
	cp LICENSE NOTICE README.md "$dir/"
	cp third_party/rustzx-z80-LICENSE.txt "$dir/third_party/"
	file "$dir/tnc2-emu"
	tar -C "$tmp" --sort=name --owner=0 --group=0 --numeric-owner --mtime="@$mtime" -cf - "$name" | gzip -9n > "$out/$name.tar.gz"
done

echo "== Building the Pico 2 W firmware"
tools/pico-firmware.sh "$tmp/pico"
for bin in node speedtest; do
	cp "$tmp/pico/tnc2-pico-$bin.uf2" "$out/tnc2-emu-$version-pico2w-$bin.uf2"
done

echo "== Checksums"
(cd "$out" && sha256sum -- * > SHA256SUMS && cat SHA256SUMS)
