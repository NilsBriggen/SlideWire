# Contributing to Slidewire

Thanks for helping. Bug reports, controller profiles, translations and code
are all welcome.

## Reporting a problem

Open an issue and include:
- `slidewire --version`
- your distribution and desktop (KDE Plasma, GNOME, Hyprland, …)
- for controller problems, the output of `slidewire --doctor <part of its name>`
  and, if it connects but does odd things, a few lines of `slidewire --monitor`

## Building

You need Rust 1.95 or newer and the development files for ALSA and
PulseAudio (`alsa-lib`/`libasound2-dev`, `libpulse`/`libpulse-dev`).

```sh
cargo run                       # the app
cargo run --example fake_nano   # a virtual nanoKONTROL2 to test without hardware
cargo test
```

`scripts/screenshot.sh` runs the app on a private X server (Xvfb), driven by
the fake controller and a silent test stream, and takes screenshots, so UI
changes can be checked without touching your desktop or your audio.

## Before sending changes

```sh
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
python3 scripts/i18n-extract.py   # when you changed user-visible text
```

- Match the style of the code around your change; comments say why, not what.
- User-visible text goes through `t!("English text")`, with variables as
  `%{name}` inside whole sentences.
- One topic per commit, with a message that says what changed for the user.
- By contributing you agree that your work is licensed under the
  GPL-3.0-or-later, like the rest of Slidewire.

## Controller profiles

Profiles are plain JSON in `~/.config/slidewire/profiles/`. A profile for a
controller that isn't supported yet is a great contribution: build it with
Controller → Add control (new controls learn from your device), then export it
from the Profiles page and attach it to an issue.

## Translations

Copy `locales/en.json` to `locales/<language code>.json` and translate the
values. Keep `%{…}` placeholders unchanged.
