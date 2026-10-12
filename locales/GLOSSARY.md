# Translating Slidewire

Slidewire is a Linux desktop app that turns a MIDI controller (a box with
faders, knobs and buttons, like the Korg nanoKONTROL2) into a volume mixer:
faders set the volume of apps and audio devices, buttons mute and switch them,
and the controller's LEDs show the state. The window looks like a small mixing
console.

`en.json` maps each English text to itself. A translation is a copy with the
values translated: `"Hide sources": "Quellen ausblenden"`.

## Rules

Writing Bärndütsch (`gsw.json`)? Read [BAERNDUETSCH.md](BAERNDUETSCH.md) first.


- Keep every key exactly as it is; translate only the values. Same keys, no
  extra ones.
- Keep `%{name}` placeholders exactly (same names, same count); move them
  where your grammar needs them.
- Keep these as they are: Slidewire, OBS Studio, EasyEffects, MIDI Mixer,
  nanoKONTROL2, Korg, PipeWire, PulseAudio, KONTROL Editor, MIDI, CC, dB,
  key names (Ctrl+F, Esc, F1), and the single letters S, M and R (they are
  printed on the buttons).
- Use your desktop's usual wording (as in KDE Plasma or GNOME in your
  language). Short and plain. No exclamation marks.
- Keep the English sentence case (only the first word capitalised, unless
  your language capitalises more, like German nouns) and the "…" endings.
- Labels on buttons and strips have little room: keep translations about as
  long as the English where you can.
- Values must be valid JSON strings: escape `"` and `\`.

## Terms

| English | Meaning |
|---|---|
| strip / channel strip | one column of the mixer: a fader (or knob) with its buttons |
| fader | vertical slider on the controller or in the window |
| knob | rotary control |
| bank | a set of assignments; switching banks lets 8 faders control 16, 24, … things |
| scene | a saved snapshot of every strip's volume and mute |
| bus | a virtual output that mixes several apps into one fader |
| ducking / turn other apps down | lowering other apps while one plays (e.g. voice chat) |
| pick up (soft takeover) | a fader only takes over once it reaches the current volume |
| assign | put an app or device on a strip |
| run (R button) | make a device the default, or start an app |
| monitor / hear yourself | play a microphone live on headphones |
| profile | the layout and settings of one controller |
| source | something that plays or records: an app or a device |
| output / input | audio device for playback / recording |
| tray | the system tray / notification area |
| learn | set a control's MIDI address by moving it on the device |
| LED | the lights on the controller's buttons |
