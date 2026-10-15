# Changelog

All notable changes to Slidewire. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Translations: German, French, Spanish, Italian, Portuguese (Brazil), Polish, Hungarian, Russian, Chinese (Simplified), Japanese, Korean and Bärndütsch, with a language picker under Settings → Appearance. Chinese, Japanese and Korean use the desktop's CJK font.
- Balance: a strip can pan its apps and devices left or right, from its menu or with a knob given the new Balance role.
- Scenes can glide in over 0.5 to 5 seconds, be renamed, and be stepped through with a previous/next scene button. A scene button's LED is lit while its scene is the last one recalled.
- Swap with… trades everything on two strips.
- The sources panel shows which strip controls each app and device, and can hide those already on one.
- A strip glows when its control on the hardware moves.
- The window opens at its last size (and, on X11, position).
- Export a whole Slidewire profile (assignments, banks, scenes, buttons, focus rules) as well as MIDI Mixer's spec.json.
- Command line: `mic`, `banks`, `scenes`, `scene save`, `profile`, `profiles` and `state`; `--json` output; `set --volume` for the volume itself rather than the fader position. Strips can be named by the label you gave them.
- Shortcuts: PgUp/PgDn switch banks, Ctrl+Shift+M mutes the microphone.
- The connection check and the log are translated.

### Changed
- The project now lives on GitHub: https://github.com/NilsBriggen/SlideWire. Releases include an aarch64 archive alongside x86_64.

### Fixed
- Moving a fader no longer resets an app's or device's left/right balance.
- Removing a bank left scenes and focus rules pointing at the wrong bank.
- Saving a scene under an existing name replaced it without asking.
- The mute pop-up could show the turned-down volume while ducking.
- Key combinations could not use the plus key (`ctrl++` or `ctrl+plus` now work).
- OBS addresses can be a plain host or host:port; `wss://` gets a clear message.
- `slidewire bank N` accepted banks that don't exist.
- Hints in menus were spread out letter by letter.

## [1.0.0] - 2026-10-11

The first release.

### Mixing
- One channel strip per hardware channel, each with a knob, a fader, a scribble-strip display, glowing S/M/R buttons, an LED meter, and the volume in % and dB.
- A strip controls any mix of:
  - the focused app
  - apps matched by name, wildcard or regex
  - recording streams
  - "all other apps"
  - system sounds
  - any output or input
  - the default device
- Banks give 8 faders 16, 24 or more things to control. Scenes save and recall every strip's volume and mute.
- Soft takeover: a fader only takes over once it reaches the current volume.
- Strips have their own name, colour, icon, range (such as a mic that never goes below 40%) and curve.
- Ducking: a strip can turn the other apps down while it plays.
- Buses: virtual outputs that mix several apps into one fader, each playing on a real output.
- "Always play here" remembers which device an app plays on (or records from).
- A master strip for the default output.

### Buttons
- Buttons outside the strips can:
  - send media keys, to whichever player is playing or to one you name
  - toggle mutes, and set or cycle the default device
  - run a command or press keys
  - switch profile, bank or scene
  - mute all inputs, or act as push-to-talk or push-to-mute
  - hear yourself (a monitor loopback)
  - load an EasyEffects preset
  - control OBS Studio: switch scene, mute a source, record or stream
- Focus rules switch to a bank or scene while an app is in front, and back when you leave it.

### Controllers
- A ready-made Korg nanoKONTROL2 profile with live LEDs, and a drawing of it that lights up as you touch the hardware.
- Imports MIDI Mixer for Windows profiles (`spec.json`), so its presets for other controllers work.
- Learn mode for building a profile, and for re-learning any control from the mixer or the Buttons page.
- Supported input:
  - 7-bit CC and notes
  - 14-bit pitch-bend (motor) faders
  - endless encoders
- Supported feedback:
  - LED rings
  - button LEDs
  - motor faders
  - Mackie-style meters
- A connection check (`slidewire --doctor`) explains why a controller doesn't show up. A raw-MIDI fallback keeps devices working that the ALSA sequencer doesn't list.

### Desktop
- Dark and light themes, accent colours (including the desktop's), UI size, and reduced motion.
- A tray icon with:
  - a mute badge
  - volume on the scroll wheel
  - scenes, banks and mic mute in its menu
- Focus tracking and the volume pop-up on KDE Plasma, Hyprland, sway and others. File dialogs go through the XDG portal.
- Keyboard shortcuts, undo and redo, toasts with Undo, and a first-run welcome.
- Screen readers get names and values for every control; faders and knobs work from the keyboard.
- Text can be translated (rust-i18n). English is the only language so far.
- Command line control of the running app: `slidewire strips|set|mute|bank|scene`.
- A systemd user unit for starting with the session. `--install-icon` and `--uninstall` cover builds without a package.

### Reliability
- Only one instance runs; a second launch shows the first.
- Quitting (including SIGTERM at logout):
  - restores ducked volumes
  - turns the LEDs off
  - saves pending edits
- Settings and profiles are written atomically, readable only by you, and recovered from their backup if damaged.
- Imported profiles that run commands ask before keeping them.
- A crash is written to `~/.local/state/slidewire/crash.log`, and systemd restarts the service.

[1.0.0]: https://github.com/NilsBriggen/SlideWire/releases/tag/v1.0.0
