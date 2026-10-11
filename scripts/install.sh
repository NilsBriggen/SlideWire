#!/usr/bin/env bash
# Install a built Slidewire into DESTDIR with the usual /usr layout:
#   scripts/install.sh DESTDIR [path/to/slidewire]
# Used by the release archive and the AUR packages.
set -euo pipefail
cd "$(dirname "$0")/.."
dest=${1:?usage: install.sh DESTDIR [BINARY]}
bin=${2:-target/release/slidewire}
install -Dm755 "$bin" "$dest/usr/bin/slidewire"
install -Dm644 packaging/dev.briggen.Slidewire.desktop "$dest/usr/share/applications/dev.briggen.Slidewire.desktop"
install -Dm644 packaging/dev.briggen.Slidewire.metainfo.xml "$dest/usr/share/metainfo/dev.briggen.Slidewire.metainfo.xml"
"$bin" --write-icons "$dest/usr/share/icons"
install -Dm644 packaging/slidewire.service "$dest/usr/lib/systemd/user/slidewire.service"
install -Dm644 packaging/slidewire.1 "$dest/usr/share/man/man1/slidewire.1"
install -Dm644 packaging/completions/slidewire.bash "$dest/usr/share/bash-completion/completions/slidewire"
install -Dm644 packaging/completions/_slidewire "$dest/usr/share/zsh/site-functions/_slidewire"
install -Dm644 packaging/completions/slidewire.fish "$dest/usr/share/fish/vendor_completions.d/slidewire.fish"
install -Dm644 LICENSE "$dest/usr/share/licenses/slidewire/LICENSE"
for f in assets/fonts/*-LICENSE.txt; do
  install -Dm644 "$f" "$dest/usr/share/licenses/slidewire/fonts/$(basename "$f")"
done
install -Dm644 README.md "$dest/usr/share/doc/slidewire/README.md"
install -Dm644 CHANGELOG.md "$dest/usr/share/doc/slidewire/CHANGELOG.md"
