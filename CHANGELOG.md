# Changelog

All notable changes to Slidewire. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

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

[1.0.0]: https://git.briggen.dev/NilsBriggen/SlideWire/releases/tag/v1.0.0
