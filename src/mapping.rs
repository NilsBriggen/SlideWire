//! Pure MIDI <-> control value conversion.

use crate::profile::{Command, Control, ControlKind, Delta};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Msg {
    pub command: Command,
    pub channel: u8,
    pub data1: u8,
    pub data2: u8,
}

impl Msg {
    pub fn parse(bytes: &[u8]) -> Option<Msg> {
        let status = *bytes.first()?;
        if !(0x80..0xF0).contains(&status) {
            return None;
        }
        let command = match status & 0xF0 {
            0x80 => Command::NoteOff,
            0x90 => Command::NoteOn,
            0xA0 => Command::PolyAftertouch,
            0xB0 => Command::Cc,
            0xC0 => Command::Program,
            0xD0 => Command::ChannelAftertouch,
            0xE0 => Command::Pitch,
            _ => return None,
        };
        Some(Msg {
            command,
            channel: status & 0x0F,
            data1: bytes.get(1).copied().unwrap_or(0) & 0x7F,
            data2: bytes.get(2).copied().unwrap_or(0) & 0x7F,
        })
    }

    /// The value carried by the message (14 bit for pitch bend).
    pub fn value(&self) -> u16 {
        match self.command {
            Command::Pitch => self.data1 as u16 | ((self.data2 as u16) << 7),
            Command::Program | Command::ChannelAftertouch => self.data1 as u16,
            Command::NoteOff => 0,
            _ => self.data2 as u16,
        }
    }

    pub fn describe(&self) -> String {
        if self.command.has_control() {
            format!("{} ch{} #{} = {}", self.command.label(), self.channel + 1, self.data1, self.value())
        } else {
            format!("{} ch{} = {}", self.command.label(), self.channel + 1, self.value())
        }
    }
}

/// What a control did, after interpreting the raw message.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Input {
    /// Absolute position 0..=1.
    Absolute(f32),
    /// Relative encoder clicks (positive = up).
    Relative(i32),
    Press,
    Release,
}

/// Does `msg` come from control `c`?
pub fn matches(c: &Control, msg: &Msg) -> bool {
    if c.channel != msg.channel {
        return false;
    }
    let cmd_ok = match c.command {
        // A note button also releases through note-off.
        Command::NoteOn | Command::NoteOff => matches!(msg.command, Command::NoteOn | Command::NoteOff),
        other => other == msg.command,
    };
    cmd_ok && (!c.command.has_control() || c.control == msg.data1)
}

pub fn interpret(c: &Control, msg: &Msg) -> Option<Input> {
    let v = msg.value();
    match c.kind {
        ControlKind::Fader => {
            if let Some(d) = &c.delta {
                let clicks = decode_relative(d, v as u8);
                (clicks != 0).then_some(Input::Relative(clicks))
            } else {
                let (min, max) = (c.min() as f32, c.max() as f32);
                if max <= min {
                    return None;
                }
                Some(Input::Absolute(((v as f32 - min) / (max - min)).clamp(0.0, 1.0)))
            }
        }
        ControlKind::Button => {
            if msg.command == Command::NoteOff || v == 0 {
                return Some(Input::Release);
            }
            match c.push_value {
                Some(p) if !c.velocity_sensitive && v != p => Some(Input::Release),
                _ => Some(Input::Press),
            }
        }
        ControlKind::Output => None,
    }
}

/// Decode a relative encoder value into clicks.
///
/// The profile names the value for one click up and one click down; faster
/// turns send larger offsets in one of the common encodings, which are
/// inferred from those two values.
pub fn decode_relative(d: &Delta, v: u8) -> i32 {
    if v == d.up {
        return 1;
    }
    if v == d.down {
        return -1;
    }
    let v = v as i32;
    let (up, down) = (d.up as i32, d.down as i32);
    if down == 127 && up < 64 {
        // Two's complement: 1..63 up, 127..64 down.
        if v < 64 { v } else { v - 128 }
    } else if up < 64 && down > 64 {
        // Sign bit: 1..63 up, 65..127 down.
        if v < 64 { v } else { -(v - 64) }
    } else if up > 64 && down < 64 {
        // Binary offset around 64.
        v - 64
    } else {
        0
    }
}

/// Message that shows `level` (0..=1) on an output control, e.g. an LED ring
/// or a motor fader.
pub fn level_message(c: &Control, level: f32) -> Vec<u8> {
    let (min, max) = (c.min() as f32, c.max() as f32);
    let v = (min + level.clamp(0.0, 1.0) * (max - min)).round() as u16;
    raw_message(c, v)
}

/// Message that turns an indicator on or off.
pub fn indicator_message(c: &Control, on: bool) -> Vec<u8> {
    let v = if on { c.push_value.filter(|p| *p > 0).unwrap_or_else(|| c.max()) } else { c.min() };
    raw_message(c, v)
}

/// Mackie-style peak meter: channel aftertouch with the strip in the high
/// nibble and a 0..=13 level in the low nibble. Other commands get a scaled
/// value.
pub fn meter_message(c: &Control, peak: f32) -> Vec<u8> {
    if c.command == Command::ChannelAftertouch {
        let level = (peak.clamp(0.0, 1.0) * 13.0).round() as u8;
        vec![0xD0 | (c.channel & 0x0F), ((c.control & 0x07) << 4) | level]
    } else {
        level_message(c, peak)
    }
}

pub fn raw_message(c: &Control, v: u16) -> Vec<u8> {
    let status = c.command.status() | (c.channel & 0x0F);
    match c.command {
        Command::Pitch => vec![status, (v & 0x7F) as u8, ((v >> 7) & 0x7F) as u8],
        Command::Program | Command::ChannelAftertouch => vec![status, (v & 0x7F) as u8],
        _ => vec![status, c.control & 0x7F, (v & 0x7F) as u8],
    }
}

/// Slider position (0..=1) -> PulseAudio volume fraction (1.0 = 100%).
pub fn curve(pos: f32, exponent: f32) -> f32 {
    pos.clamp(0.0, 1.0).powf(exponent.max(0.1))
}

/// PulseAudio volume fraction -> slider position.
pub fn inverse_curve(vol: f32, exponent: f32) -> f32 {
    vol.clamp(0.0, 1.0).powf(1.0 / exponent.max(0.1))
}

/// Where the dB marks sit on a fader: (position 0..=1, dB) for the marks
/// that fit. `range` is the volume at the top (max volume times boost);
/// PulseAudio's volume scale is cubic, so dB = 60·log10(volume).
pub fn db_marks(range: f32, exponent: f32) -> Vec<(f32, i32)> {
    [6, 0, -6, -12, -24, -48]
        .into_iter()
        .filter_map(|db| {
            let vol = 10f32.powf(db as f32 / 60.0);
            (vol <= range + 1e-4).then(|| (inverse_curve(vol / range, exponent), db))
        })
        .collect()
}

/// Gain envelope for ducking: follows "something is playing" with a fast
/// attack, holds briefly through pauses between words, then releases
/// slowly. The gain multiplies volumes on PulseAudio's cubic scale.
#[derive(Debug, Clone)]
pub struct Ducker {
    pub gain: f32,
    /// Seconds since the source was last above its threshold.
    quiet_for: f32,
}

impl Default for Ducker {
    fn default() -> Self {
        Ducker { gain: 1.0, quiet_for: f32::INFINITY }
    }
}

impl Ducker {
    const ATTACK: f32 = 0.05;
    const RELEASE: f32 = 0.6;
    const HOLD: f32 = 0.4;

    /// Advance by `dt` seconds. `amount_db` is the duck depth while a
    /// source plays (None: nothing is set to duck).
    pub fn step(&mut self, loud: bool, amount_db: Option<f32>, dt: f32) -> f32 {
        self.quiet_for = if loud { 0.0 } else { self.quiet_for + dt };
        let target = match amount_db {
            Some(db) if self.quiet_for <= Self::HOLD => 10f32.powf(db / 60.0),
            _ => 1.0,
        };
        let tau = if target < self.gain { Self::ATTACK } else { Self::RELEASE };
        self.gain += (target - self.gain) * (1.0 - (-dt / tau).exp());
        if (self.gain - target).abs() < 0.002 {
            self.gain = target;
        }
        self.gain
    }
}

/// Control as learned from an incoming message.
pub fn learn(msg: &Msg, kind: ControlKind) -> Control {
    Control {
        kind,
        channel: msg.channel,
        command: if msg.command == Command::NoteOff { Command::NoteOn } else { msg.command },
        control: if msg.command.has_control() { msg.data1 } else { 0 },
        ..Default::default()
    }
}

/// Soft takeover for faders without a motor: after the volume changed
/// elsewhere, a fader only takes over once it reaches (or moves across) the
/// current value, so the volume never jumps. `last` is the fader's previous
/// position, `pos` the new one, `current` where the volume is now.
pub fn picks_up(last: Option<f32>, pos: f32, current: f32) -> bool {
    const CLOSE: f32 = 0.03;
    if (pos - current).abs() <= CLOSE {
        return true;
    }
    last.is_some_and(|last| (last - current).abs() <= CLOSE || (last - current) * (pos - current) < 0.0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn ducking_envelope() {
        let mut d = super::Ducker::default();
        let target = 10f32.powf(-12.0 / 60.0);
        // Fast attack: most of the way down within 0.1 s.
        for _ in 0..2 {
            d.step(true, Some(-12.0), 0.05);
        }
        assert!(d.gain < 1.0 - 0.8 * (1.0 - target), "{}", d.gain);
        for _ in 0..10 {
            d.step(true, Some(-12.0), 0.05);
        }
        assert_eq!(d.gain, target);
        // Short pauses (between words) keep it down.
        d.step(false, Some(-12.0), 0.3);
        assert_eq!(d.gain, target);
        // Then it comes back slowly.
        d.step(false, Some(-12.0), 0.2);
        let after = d.step(false, Some(-12.0), 0.1);
        assert!(after > target && after < 0.95, "{after}");
        for _ in 0..80 {
            d.step(false, Some(-12.0), 0.05);
        }
        assert_eq!(d.gain, 1.0);
    }

    #[test]
    fn db_marks() {
        let m = super::db_marks(1.0, 1.0);
        assert_eq!(m.iter().map(|m| m.1).collect::<Vec<_>>(), [0, -6, -12, -24, -48]);
        assert!((m[0].0 - 1.0).abs() < 1e-4);
        assert!((m[1].0 - 0.794).abs() < 0.01); // -6 dB is 79% on the cubic scale
        // With boost, +6 dB fits and 0 dB moves down.
        let m = super::db_marks(1.5, 1.0);
        assert_eq!(m[0].1, 6);
        assert!((m[1].0 - 1.0 / 1.5).abs() < 1e-3);
        // A steeper curve spreads the low end.
        assert!(super::db_marks(1.0, 2.0)[4].0 > super::db_marks(1.0, 1.0)[4].0);
    }

    use super::*;

    fn cc(control: u8, kind: ControlKind) -> Control {
        Control { kind, control, ..Default::default() }
    }

    #[test]
    fn parse_and_value() {
        let m = Msg::parse(&[0xB1, 7, 100]).unwrap();
        assert_eq!((m.command, m.channel, m.data1, m.value()), (Command::Cc, 1, 7, 100));
        let p = Msg::parse(&[0xE0, 0x7F, 0x7F]).unwrap();
        assert_eq!(p.value(), 16383);
        assert!(Msg::parse(&[0xF8]).is_none());
        assert!(Msg::parse(&[0x40]).is_none());
    }

    #[test]
    fn absolute_fader() {
        let c = cc(0, ControlKind::Fader);
        let m = Msg::parse(&[0xB0, 0, 127]).unwrap();
        assert!(matches(&c, &m));
        assert_eq!(interpret(&c, &m), Some(Input::Absolute(1.0)));
        let m = Msg::parse(&[0xB0, 0, 0]).unwrap();
        assert_eq!(interpret(&c, &m), Some(Input::Absolute(0.0)));
        assert!(!matches(&c, &Msg::parse(&[0xB0, 1, 0]).unwrap()));
        assert!(!matches(&c, &Msg::parse(&[0xB1, 0, 0]).unwrap()));
    }

    #[test]
    fn custom_range() {
        let mut c = cc(0, ControlKind::Fader);
        c.min_value = Some(27);
        c.max_value = Some(127);
        let m = Msg::parse(&[0xB0, 0, 77]).unwrap();
        assert_eq!(interpret(&c, &m), Some(Input::Absolute(0.5)));
        let m = Msg::parse(&[0xB0, 0, 10]).unwrap();
        assert_eq!(interpret(&c, &m), Some(Input::Absolute(0.0)));
    }

    #[test]
    fn pitch_fader_14bit() {
        let c = Control { kind: ControlKind::Fader, command: Command::Pitch, channel: 2, ..Default::default() };
        let m = Msg::parse(&[0xE2, 0x00, 0x40]).unwrap();
        assert!(matches(&c, &m));
        match interpret(&c, &m) {
            Some(Input::Absolute(v)) => assert!((v - 0.5).abs() < 0.001),
            other => panic!("{other:?}"),
        }
        assert_eq!(level_message(&c, 1.0), vec![0xE2, 0x7F, 0x7F]);
    }

    #[test]
    fn relative_encoders() {
        let sign = Delta::new(1, 65);
        assert_eq!(decode_relative(&sign, 1), 1);
        assert_eq!(decode_relative(&sign, 65), -1);
        assert_eq!(decode_relative(&sign, 3), 3);
        assert_eq!(decode_relative(&sign, 67), -3);
        let twos = Delta::new(1, 127);
        assert_eq!(decode_relative(&twos, 126), -2);
        assert_eq!(decode_relative(&twos, 2), 2);
        let offset = Delta::new(65, 63);
        assert_eq!(decode_relative(&offset, 67), 3);
        assert_eq!(decode_relative(&offset, 61), -3);

        let mut c = cc(16, ControlKind::Fader);
        c.delta = Some(sign.clone());
        assert_eq!(interpret(&c, &Msg::parse(&[0xB0, 16, 65]).unwrap()), Some(Input::Relative(-1)));
    }

    #[test]
    fn buttons() {
        let c = cc(48, ControlKind::Button);
        assert_eq!(interpret(&c, &Msg::parse(&[0xB0, 48, 127]).unwrap()), Some(Input::Press));
        assert_eq!(interpret(&c, &Msg::parse(&[0xB0, 48, 0]).unwrap()), Some(Input::Release));

        let note = Control { kind: ControlKind::Button, command: Command::NoteOn, control: 89, ..Default::default() };
        let off = Msg::parse(&[0x80, 89, 64]).unwrap();
        assert!(matches(&note, &off));
        assert_eq!(interpret(&note, &off), Some(Input::Release));
        assert_eq!(interpret(&note, &Msg::parse(&[0x90, 89, 0]).unwrap()), Some(Input::Release));
        assert_eq!(interpret(&note, &Msg::parse(&[0x90, 89, 30]).unwrap()), Some(Input::Press));

        let mut pv = note.clone();
        pv.push_value = Some(127);
        assert_eq!(interpret(&pv, &Msg::parse(&[0x90, 89, 30]).unwrap()), Some(Input::Release));
        pv.velocity_sensitive = true;
        assert_eq!(interpret(&pv, &Msg::parse(&[0x90, 89, 30]).unwrap()), Some(Input::Press));
    }

    #[test]
    fn outputs() {
        let mut ring = cc(48, ControlKind::Output);
        ring.min_value = Some(33);
        ring.max_value = Some(43);
        assert_eq!(level_message(&ring, 0.0), vec![0xB0, 48, 33]);
        assert_eq!(level_message(&ring, 0.5), vec![0xB0, 48, 38]);
        assert_eq!(level_message(&ring, 1.0), vec![0xB0, 48, 43]);

        let led = cc(48, ControlKind::Button);
        assert_eq!(indicator_message(&led, true), vec![0xB0, 48, 127]);
        assert_eq!(indicator_message(&led, false), vec![0xB0, 48, 0]);

        let meter = Control {
            kind: ControlKind::Output,
            command: Command::ChannelAftertouch,
            control: 3,
            ..Default::default()
        };
        assert_eq!(meter_message(&meter, 1.0), vec![0xD0, 0x3D]);
    }

    #[test]
    fn curves() {
        assert!((curve(0.5, 1.0) - 0.5).abs() < 1e-6);
        assert!((curve(0.5, 2.0) - 0.25).abs() < 1e-6);
        for e in [1.0, 1.5, 2.0, 3.0] {
            for p in [0.0, 0.1, 0.5, 0.9, 1.0] {
                assert!((inverse_curve(curve(p, e), e) - p).abs() < 1e-4);
            }
        }
    }

    #[test]
    fn pickup() {
        // Fader at 20%, volume at 60%: moving to 30% does nothing...
        assert!(!picks_up(Some(0.2), 0.3, 0.6));
        // ...until it reaches the volume, or jumps across it.
        assert!(picks_up(Some(0.3), 0.59, 0.6));
        assert!(picks_up(Some(0.5), 0.7, 0.6));
        assert!(picks_up(Some(0.9), 0.4, 0.6));
        // First message after connecting: only if it's already close.
        assert!(!picks_up(None, 0.2, 0.6));
        assert!(picks_up(None, 0.61, 0.6));
        // Once picked up, small moves keep following.
        assert!(picks_up(Some(0.6), 0.65, 0.6));
    }
}
