# Slidewire

Control the volume of individual apps and audio devices with a MIDI controller: faders for volume, buttons for mute, assign and default device, and LEDs that show the state. It's an independent Linux app inspired by MIDI Mixer for Windows. It shares none of that app's code and has no accounts, subscriptions or limits.

Built for KDE Plasma on PipeWire (or PulseAudio). The audio, MIDI and mixer parts work on any desktop; focus tracking and the volume pop-up use KDE.

## Build and install

```sh
cargo build --release
./target/release/slidewire --install-icon   # adds a menu entry + icon for your user
./target/release/slidewire
```

Requires `libpulse` and `alsa-lib`, which are already present on any desktop with sound.

## First run

A **Korg nanoKONTROL2** profile is created automatically:

| Control | Does |
|---|---|
| Fader 1–8, Knob 1–8 | volume of whatever the strip is assigned to |
| M 1–8 | mute toggle |
| S 1–8 | assign: press while an app is focused to put it on that strip; press again to clear |
| R 1–8 | run: makes an assigned device the default (or launches an assigned app) |
| Play / Stop / ◀◀ / ▶▶ | media keys (MPRIS: Spotify, browsers, mpv, ...) |

## The window

The **Mixer** page looks like a console:
- knobs on top and faders below, in the same order as the hardware
- each strip has a scribble-strip display with the app or device icon, glowing **S**/**M**/**R** buttons, an LED meter and a % readout
- to assign, drag an app or device from the **Sources** panel on the right onto a strip, or click a strip's **S** button (right-click works too)
- faders and knobs can be dragged, scrolled, or double-clicked to jump to 100%

The **Controller** page draws your nanoKONTROL2 live: touch a control and it lights up, and the LEDs show what the device should be showing. Click a control in the drawing to edit it. Other controllers get a tile view.

Each strip can control any combination of:
- the focused app
- specific apps (matched by name, `*` wildcards or regex, so they survive app restarts)
- an app's recording stream
- "all other apps"
- system sounds
- any output or input device, or the current default device

**nanoKONTROL2 LEDs:** the device only lets the computer drive its button LEDs in *External* LED mode. Set it once with Korg's KONTROL Editor (it runs under Wine). In the default mode everything works, but the LEDs only light while a button is held.

## Other controllers

- **Profiles → Import** reads profiles from MIDI Mixer for Windows (`spec.json` files, found in that app's `profiles/` folder), so presets for the X-Touch Mini, X-Touch Extender, Platform M+, LPD8, MPK Mini and others work directly.
- To build your own, use **Controller → Add control**: new controls start in learn mode, so you just move the knob. Then group controls into strips.
- Supported input types:
  - 7-bit CC and notes
  - 14-bit pitch-bend (motor) faders
  - endless encoders (sign-bit, two's complement and offset-64 encodings)
- Supported feedback:
  - LED rings
  - button LEDs
  - motor faders
  - Mackie-style peak meters

## Command line

```
slidewire --minimized        start in the tray
slidewire --list-ports       show MIDI ports
slidewire --monitor [name]   print incoming MIDI (debug a controller)
```

Running `slidewire` while it's already running brings the window up.

## Files

- `~/.config/slidewire/settings.json`
- `~/.config/slidewire/profiles/*.json` (plain JSON in the same format as the Windows app, plus `assignments` and `buttons`)

Set `SLIDEWIRE_CONFIG=/some/dir` to use another config directory. Settings from the old `~/.config/midimixer/` are copied over on first start.

## Testing without hardware

`cargo run --example fake_nano` creates a virtual nanoKONTROL2. Type commands on its stdin:
- `f 1 64`: fader 1 to the middle
- `b 48`: press M1

It prints the LED messages the app sends back.

`cargo test` checks the MIDI mapping math and the icon lookup. To also test importing the Windows app's profiles, point `SLIDEWIRE_WINDOWS_PROFILES` at a copy of its `profiles/` folder.

`scripts/screenshot.sh OUT_DIR mixer controller ...` screenshots the pages on a private X server (Xvfb), driven by the fake controller and a silent test stream. Nothing shows up on your desktop and none of your own audio streams are touched.

## License

MIT or Apache-2.0, at your option. Bundled fonts: Inter and JetBrains Mono (OFL) and the Phosphor icon font (MIT), see `assets/fonts/README.md`.
