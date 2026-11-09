#!/usr/bin/env bash
# Builds Everwall and packs the Linux tar.gz attached to each GitHub release:
# the everwall program, its icon, install.sh and the licence.
#   ./package.sh             build and pack
#   ./package.sh --no-build  pack the last build
# The package is written next to the build (CARGO_TARGET_DIR or
# src-tauri/target), never inside the repository.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tauri="$here/src-tauri"
# The program keeps source paths for its error messages: they must not carry
# the home folder of whoever built it.
export CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=$HOME=~"$'\x1f'"--remap-path-prefix=$here=everwall"
if [[ "${1:-}" != "--no-build" ]]; then
    (cd "$tauri" && cargo build --release)
fi

version="$(sed -n 's/^ *"version": *"\([^"]*\)".*/\1/p' "$tauri/tauri.conf.json" | head -n 1)"
target="${CARGO_TARGET_DIR:-$tauri/target}"
name="Everwall-$version"
stage="$target/$name"
package="$target/$name-linux-x64.tar.gz"

rm -rf "$stage"
mkdir -p "$stage"
install -m755 "$target/release/everwall" "$stage/everwall"
install -m755 "$here/install.sh" "$stage/install.sh"
install -m644 "$tauri/icons/icon.png" "$stage/everwall.png"
install -m644 "$here/LICENSE" "$stage/LICENSE"

# owned by root in the archive, not by whoever built it
tar --owner=0 --group=0 --numeric-owner -czf "$package" -C "$target" "$name"
rm -rf "$stage"
echo "$package"
echo "SHA-256 $(sha256sum "$package" | cut -d ' ' -f 1)"
