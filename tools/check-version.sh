#!/bin/sh
# Checks every crate and every lock file carry one version, the root
# Cargo.toml's. Given a tag (vX.Y.Z), also checks the tag is that version.
#
#     tools/check-version.sh            # do the crates agree?
#     tools/check-version.sh v0.2.0     # and is it 0.2.0?
#
# The release workflow runs it with the tag, CI without. To fix a lock
# file, run `cargo update -w` in its directory (docs/releasing.md).
set -eu
cd "$(dirname "$0")/.."

# The version line of a manifest's [package] table.
manifest_version() {
	awk '/^\[/ { p = ($0 == "[package]") } p && /^version *=/ { gsub(/"/, "", $3); print $3; exit }' "$1"
}

want=$(manifest_version Cargo.toml)
bad=0

if [ $# -gt 0 ]; then
	if [ "$1" != "v$want" ]; then
		echo "tag $1 is not v$want, the version in Cargo.toml"
		bad=1
	fi
fi

for m in crates/tnc2-core/Cargo.toml crates/tnc2-node/Cargo.toml crates/tnc2-pico/Cargo.toml; do
	got=$(manifest_version "$m")
	if [ "$got" != "$want" ]; then
		echo "$m: version $got, want $want"
		bad=1
	fi
done

# The repo's own crates in each lock file. Path dependencies are locked
# with their version, so the firmware's and the tools' locks change too.
for l in Cargo.lock crates/tnc2-pico/Cargo.lock tools/qemu-pace/Cargo.lock tools/m33bench/Cargo.lock; do
	out=$(awk -v want="$want" -v lock="$l" '
		/^name = / { gsub(/"/, "", $3); name = $3 }
		/^version = / && name ~ /^tnc2-(emu|core|node|pico)$/ {
			gsub(/"/, "", $3); n++
			if ($3 != want) print lock ": " name " is " $3 ", want " want
		}
		END { if (n == 0) print lock ": none of the repo crates in it" }' "$l")
	if [ -n "$out" ]; then
		echo "$out"
		bad=1
	fi
done

if [ "$bad" -ne 0 ]; then
	echo "versions do not agree"
	exit 1
fi
echo "all crates and lock files are at $want${1:+, matching tag $1}"
