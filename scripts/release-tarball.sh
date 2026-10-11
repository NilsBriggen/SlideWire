#!/usr/bin/env bash
# Build dist/slidewire-<version>-<arch>.tar.gz: the binary plus everything a
# package installs (desktop entry, icons, metainfo, systemd unit, man page,
# completions, licences), laid out like /usr. Also writes its SHA256.
set -euo pipefail
cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
arch=$(uname -m)
name="slidewire-$version-$arch"
export SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct)}
cargo build --release --locked
stage=$(mktemp -d)/$name
scripts/install.sh "$stage"
mkdir -p dist
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="@$SOURCE_DATE_EPOCH" \
  -C "$(dirname "$stage")" -czf "dist/$name.tar.gz" "$name"
(cd dist && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
echo "dist/$name.tar.gz"
