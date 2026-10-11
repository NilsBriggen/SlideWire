//! The engine: one thread that turns MIDI input into audio changes and audio
//! state into LED feedback.
//!
//! Everything the GUI shows lives in [`Shared`] behind a mutex; the GUI edits
//! profiles there directly and pokes the engine with [`Event::Ui`].

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use midir::{MidiInputConnection, MidiOutputConnection};
use regex::Regex;

use crate::audio::{AppStream, AudioCmd, AudioHandle, AudioState, Kind, MeterKey};
use crate::config::Store;
use crate::dbus::Osd;
use crate::mapping::{self, Input, Msg};
use crate::midi::{self, PortLister};
use crate::profile::{AppMatch, ButtonAction, ControlKind, Profile, Target};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FocusInfo {
    pub pid: Option<u32>,
    pub caption: String,
    pub class: String,
}

#[derive(Debug, Clone)]
pub enum UiCmd {
    /// Slider position 0..=1 for a group.
    SetGroupPosition(String, f32),
    ToggleGroupMute(String),
    /// The group's Run action (set default device / launch app).
    PressRun(String),
    SetStreamVolume(Kind, u32, f32),
    SetStreamMute(Kind, u32, bool),
    SetDefault(Kind, String),
    /// Profile or settings edited: save, reconnect if needed, refresh LEDs.
    ProfileEdited,
    ActivateProfile(String),
    SendTest(String),
    /// Run the action of a group role / spare button as if pressed.
    Press(String),
    MuteAll,
}

pub enum Event {
    Midi(Vec<u8>),
    Audio(AudioState),
    Focus(FocusInfo),
    Ui(UiCmd),
    ShowWindow,
}

/// What the GUI draws for each group.
#[derive(Debug, Clone, Default)]
pub struct StripView {
    pub id: String,
    pub name: String,
    pub targets: Vec<Target>,
    /// Labels of what the targets currently resolve to.
    pub resolved: Vec<String>,
    pub position: f32,
    pub muted: bool,
    pub peak: f32,
    pub assigned: bool,
    /// Run LED: target is the default device / app is running.
    pub running: bool,
    /// Draw as a rotary knob rather than a fader.
    pub knob: bool,
    /// Icon names to try for the first resolved target.
    pub icon_hint: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LearnRequest {
    pub control_id: String,
}

pub struct Shared {
    pub store: Store,
    pub audio: AudioState,
    pub focus: FocusInfo,
    pub ports_in: Vec<String>,
    pub ports_out: Vec<String>,
    pub connected_in: Option<String>,
    pub connected_out: Option<String>,
    pub last_msg: Option<(String, Instant)>,
    pub learn: Option<LearnRequest>,
    pub strips: Vec<StripView>,
    pub log: VecDeque<String>,
    pub ui_visible: bool,
    pub egui: Option<eframe::egui::Context>,
    /// Control id -> when it last sent a message and the raw value.
    pub activity: HashMap<String, (Instant, u16)>,
    /// Output/indicator control id -> what we last showed on it (0..=1).
    pub led_state: HashMap<String, f32>,
    /// When the last MIDI message arrived (for the activity LED).
    pub last_midi: Option<Instant>,
    /// Peak levels of every metered stream/device (for the sources panel).
    pub source_peaks: HashMap<MeterKey, f32>,
}

impl Shared {
    pub fn active_id(&self) -> Option<String> {
        self.store.settings.active_profile.clone().filter(|id| self.store.profiles.contains_key(id))
    }
    pub fn active(&self) -> Option<&Profile> {
        self.active_id().and_then(|id| self.store.profiles.get(&id))
    }
    pub fn log(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        eprintln!("{msg}");
        self.log.push_back(msg);
        while self.log.len() > 200 {
            self.log.pop_front();
        }
    }
    pub fn repaint(&self) {
        if let Some(c) = &self.egui {
            c.request_repaint();
        }
    }
}

pub type SharedRef = Arc<Mutex<Shared>>;

/// A concrete thing a target currently resolves to.
#[derive(Debug, Clone)]
struct Resolved {
    kind: Kind,
    index: u32,
    label: String,
    icon: String,
    hints: Vec<String>,
    volume: f32,
    mute: bool,
    meter: Option<MeterKey>,
}

pub struct Engine {
    shared: SharedRef,
    rx: Receiver<Event>,
    tx: Sender<Event>,
    audio: AudioHandle,
    osd: Osd,
    lister: PortLister,
    input: Option<MidiInputConnection<()>>,
    output: Option<MidiOutputConnection>,
    passthru: Option<(String, MidiOutputConnection)>,
    /// Last bytes sent per output address, to avoid flooding the device.
    sent: HashMap<(u8, u8), Vec<u8>>,
    /// Slider positions for groups (for relative encoders and groups whose
    /// targets are not running).
    positions: HashMap<String, f32>,
    regexes: HashMap<String, Option<Regex>>,
    /// Focus PID -> stream PID -> related?
    focus_cache: HashMap<u32, bool>,
    focus_binary: String,
    meters_wanted: HashSet<MeterKey>,
    /// Smoothed group peaks for meter output.
    peaks: HashMap<String, f32>,
    show_window: Sender<()>,
}

impl Engine {
    pub fn new(
        shared: SharedRef,
        rx: Receiver<Event>,
        tx: Sender<Event>,
        audio: AudioHandle,
        show_window: Sender<()>,
    ) -> Engine {
        Engine {
            shared,
            rx,
            tx,
            audio,
            osd: Osd::spawn(),
            lister: PortLister::new(),
            input: None,
            output: None,
            passthru: None,
            sent: HashMap::new(),
            positions: HashMap::new(),
            regexes: HashMap::new(),
            focus_cache: HashMap::new(),
            focus_binary: String::new(),
            meters_wanted: HashSet::new(),
            peaks: HashMap::new(),
            show_window,
        }
    }

    pub fn run(mut self) {
        let mut last_scan = Instant::now() - Duration::from_secs(10);
        let mut last_heartbeat = Instant::now();
        let mut last_meter = Instant::now();
        loop {
            match self.rx.recv_timeout(Duration::from_millis(40)) {
                Ok(ev) => {
                    self.handle(ev);
                    // Drain bursts (fader sweeps) before doing feedback work.
                    while let Ok(ev) = self.rx.try_recv() {
                        self.handle(ev);
                    }
                    self.feedback();
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(_) => return,
            }
            let now = Instant::now();
            if now - last_scan > Duration::from_secs(2) {
                last_scan = now;
                self.scan_ports();
            }
            let heartbeat = self.shared.lock().unwrap().store.settings.led_heartbeat;
            if heartbeat && now - last_heartbeat > Duration::from_secs(5) {
                last_heartbeat = now;
                self.sent.clear();
                self.feedback();
            }
            if now - last_meter > Duration::from_millis(50) {
                last_meter = now;
                self.meters();
            }
        }
    }

    fn handle(&mut self, ev: Event) {
        match ev {
            Event::Midi(bytes) => self.on_midi(&bytes),
            Event::Audio(st) => {
                let mut sh = self.shared.lock().unwrap();
                sh.audio = st;
            }
            Event::Focus(f) => {
                self.focus_cache.clear();
                self.focus_binary = f.pid.map(process_binary).unwrap_or_default();
                self.shared.lock().unwrap().focus = f;
            }
            Event::ShowWindow => {
                let _ = self.show_window.send(());
            }
            Event::Ui(cmd) => self.on_ui(cmd),
        }
    }

    // ---------------------------------------------------------------- MIDI

    fn scan_ports(&mut self) {
        let ins = self.lister.inputs();
        let outs = self.lister.outputs();
        let mut sh = self.shared.lock().unwrap();
        let changed = sh.ports_in != ins || sh.ports_out != outs;
        sh.ports_in = ins.clone();
        sh.ports_out = outs.clone();

        // Auto-switch to a profile whose device just showed up.
        if changed && sh.store.settings.auto_switch {
            let active_connected = sh.active().is_some_and(|p| midi::find_port(&ins, &p.input).is_some());
            if !active_connected {
                let candidate = sh
                    .store
                    .profiles
                    .iter()
                    .find(|(_, p)| midi::find_port(&ins, &p.input).is_some())
                    .map(|(id, _)| id.clone());
                if let Some(id) = candidate {
                    let name = sh.store.profiles[&id].name.clone();
                    sh.store.settings.active_profile = Some(id);
                    let _ = sh.store.save_settings();
                    sh.log(format!("Switched to profile \"{name}\" (device connected)"));
                }
            }
        }

        let (want_in, want_out) = sh
            .active()
            .map(|p| {
                (
                    midi::find_port(&ins, &p.input).cloned(),
                    midi::find_port(&outs, if p.output.is_empty() { &p.input } else { &p.output }).cloned(),
                )
            })
            .unwrap_or((None, None));
        let want_pass = midi::find_port(&outs, &sh.store.settings.passthru).cloned();

        if want_in != sh.connected_in || (want_in.is_some() && self.input.is_none()) {
            self.input = None;
            sh.connected_in = None;
            if let Some(port) = &want_in {
                let tx = self.tx.clone();
                match midi::connect_input(port, move |b| {
                    let _ = tx.send(Event::Midi(b.to_vec()));
                }) {
                    Ok(c) => {
                        self.input = Some(c);
                        sh.connected_in = Some(port.clone());
                        sh.log(format!("Connected input {port}"));
                    }
                    Err(e) => sh.log(format!("{e:#}")),
                }
            }
        }
        if want_out != sh.connected_out || (want_out.is_some() && self.output.is_none()) {
            self.output = None;
            sh.connected_out = None;
            self.sent.clear();
            if let Some(port) = &want_out {
                match midi::connect_output(port) {
                    Ok(c) => {
                        self.output = Some(c);
                        sh.connected_out = Some(port.clone());
                        sh.log(format!("Connected output {port}"));
                    }
                    Err(e) => sh.log(format!("{e:#}")),
                }
            }
        }
        if want_pass.as_ref() != self.passthru.as_ref().map(|p| &p.0) {
            self.passthru = want_pass.and_then(|p| midi::connect_output(&p).ok().map(|c| (p, c)));
        }
        drop(sh);
        if changed {
            self.shared.lock().unwrap().repaint();
            self.feedback();
        }
    }

    fn send(&mut self, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        let key = (bytes[0], if bytes.len() == 3 && bytes[0] & 0xF0 != 0xE0 { bytes[1] } else { 0 });
        if self.sent.get(&key) == Some(&bytes) {
            return;
        }
        if let Some(out) = &mut self.output
            && out.send(&bytes).is_ok()
        {
            self.sent.insert(key, bytes);
        }
    }

    fn on_midi(&mut self, bytes: &[u8]) {
        if let Some((_, out)) = &mut self.passthru {
            let _ = out.send(bytes);
        }
        let Some(msg) = Msg::parse(bytes) else { return };
        let mut sh = self.shared.lock().unwrap();
        let now = Instant::now();
        sh.last_msg = Some((msg.describe(), now));
        sh.last_midi = Some(now);
        if let Some(p) = sh.active().cloned() {
            for (cid, c) in &p.controls {
                if c.kind != ControlKind::Output && mapping::matches(c, &msg) {
                    sh.activity.insert(cid.clone(), (now, msg.value()));
                }
            }
        }

        // Learn mode takes the message.
        if let Some(req) = sh.learn.clone() {
            let Some(id) = sh.active_id() else { return };
            if let Some(c) = sh.store.profiles.get_mut(&id).and_then(|p| p.controls.get_mut(&req.control_id)) {
                // Ignore button releases so pressing a button learns the press.
                if msg.command == crate::profile::Command::NoteOff {
                    return;
                }
                let learned = mapping::learn(&msg, c.kind);
                c.channel = learned.channel;
                c.command = learned.command;
                c.control = learned.control;
                if c.max_value.is_some_and(|m| m > c.command.default_max()) {
                    c.max_value = None;
                }
                sh.learn = None;
                let _ = sh.store.save_profile(&id);
                sh.log(format!("Learned {}", msg.describe()));
            }
            sh.repaint();
            return;
        }

        let Some(profile) = sh.active().cloned() else {
            sh.repaint();
            return;
        };
        drop(sh);

        for (cid, control) in &profile.controls {
            if control.kind == ControlKind::Output || !mapping::matches(control, &msg) {
                continue;
            }
            let Some(input) = mapping::interpret(control, &msg) else { continue };
            // The device may have changed its own LED; resend ours next time.
            if control.automatic_indicator || control.kind == ControlKind::Button {
                self.sent.remove(&(control.command.status() | control.channel, control.control));
            }
            match input {
                Input::Absolute(pos) => {
                    for (gid, g) in &profile.groups {
                        if g.faders.contains(cid) {
                            self.set_group_position(&profile, gid, pos, true);
                        }
                    }
                }
                Input::Relative(clicks) => {
                    let steps = control
                        .step_count()
                        .unwrap_or_else(|| self.shared.lock().unwrap().store.settings.default_steps)
                        .max(1);
                    for (gid, g) in &profile.groups {
                        if g.faders.contains(cid) {
                            let cur = self.group_position(&profile, gid);
                            let pos = (cur + clicks as f32 / steps as f32).clamp(0.0, 1.0);
                            self.set_group_position(&profile, gid, pos, true);
                        }
                    }
                }
                Input::Press => self.press(&profile, cid),
                Input::Release => {}
            }
        }
        self.shared.lock().unwrap().repaint();
    }

    /// A button was pressed (from MIDI or the GUI).
    fn press(&mut self, profile: &Profile, cid: &str) {
        let mut handled = false;
        for (gid, g) in &profile.groups {
            if g.mute.iter().any(|c| c == cid) {
                self.toggle_group_mute(profile, gid);
                handled = true;
            }
            if g.assign.iter().any(|c| c == cid) {
                self.assign(gid);
                handled = true;
            }
            if g.run.iter().any(|c| c == cid) {
                self.run_group(profile, gid);
                handled = true;
            }
        }
        if !handled && let Some(action) = profile.buttons.get(cid).cloned() {
            self.button_action(profile, &action);
        }
    }

    fn button_action(&mut self, profile: &Profile, action: &ButtonAction) {
        match action {
            ButtonAction::None => {}
            ButtonAction::Mute { targets } => {
                let res = self.resolve(profile, targets);
                let mute = !res.iter().all(|r| r.mute);
                for r in res {
                    self.audio.send(AudioCmd::SetMute(r.kind, r.index, mute));
                }
            }
            ButtonAction::Media { key } => {
                let key = *key;
                let shared = self.shared.clone();
                std::thread::spawn(move || {
                    if let Err(e) = crate::dbus::media(key) {
                        shared.lock().unwrap().log(format!("Media key: {e:#}"));
                    }
                });
            }
            ButtonAction::Command { command } => {
                if let Err(e) = crate::actions::run_command(command) {
                    self.shared.lock().unwrap().log(format!("Command: {e:#}"));
                }
            }
            ButtonAction::Keys { keys } => {
                let keys = keys.clone();
                let shared = self.shared.clone();
                std::thread::spawn(move || {
                    if let Err(e) = crate::actions::press_keys(&keys) {
                        shared.lock().unwrap().log(format!("Keys: {e:#}"));
                    }
                });
            }
            ButtonAction::DefaultOutput { names } => self.cycle_default(Kind::Sink, names),
            ButtonAction::DefaultInput { names } => self.cycle_default(Kind::Source, names),
            ButtonAction::Profile { id } => self.activate_profile(id),
        }
    }

    fn cycle_default(&mut self, kind: Kind, names: &[String]) {
        if names.is_empty() {
            return;
        }
        let current = {
            let sh = self.shared.lock().unwrap();
            if kind == Kind::Sink { sh.audio.default_sink.clone() } else { sh.audio.default_source.clone() }
        };
        let next = match names.iter().position(|n| *n == current) {
            Some(i) => &names[(i + 1) % names.len()],
            None => &names[0],
        };
        self.audio.send(AudioCmd::SetDefault(kind, next.clone()));
        self.osd_device(kind, next);
    }

    fn osd_device(&self, kind: Kind, name: &str) {
        let sh = self.shared.lock().unwrap();
        if !sh.store.settings.osd {
            return;
        }
        let list = if kind == Kind::Sink { &sh.audio.sinks } else { &sh.audio.sources };
        if let Some(d) = list.iter().find(|d| d.name == name) {
            let icon = if kind == Kind::Sink { "audio-speakers" } else { "audio-input-microphone" };
            self.osd.show(icon, (d.volume * 100.0).round() as i32, &d.description);
        }
    }

    fn activate_profile(&mut self, id: &str) {
        let mut sh = self.shared.lock().unwrap();
        if !sh.store.profiles.contains_key(id) {
            return;
        }
        sh.store.settings.active_profile = Some(id.to_string());
        let _ = sh.store.save_settings();
        drop(sh);
        self.positions.clear();
        self.sent.clear();
        self.scan_ports();
    }

    // ---------------------------------------------------------- UI commands

    fn on_ui(&mut self, cmd: UiCmd) {
        let profile = self.shared.lock().unwrap().active().cloned();
        match cmd {
            UiCmd::SetGroupPosition(gid, pos) => {
                if let Some(p) = &profile {
                    self.set_group_position(p, &gid, pos, false);
                }
            }
            UiCmd::ToggleGroupMute(gid) => {
                if let Some(p) = &profile {
                    self.toggle_group_mute(p, &gid);
                }
            }
            UiCmd::PressRun(gid) => {
                if let Some(p) = &profile {
                    self.run_group(p, &gid);
                }
            }
            UiCmd::SetStreamVolume(kind, idx, v) => {
                self.audio.send(AudioCmd::SetVolume(kind, idx, v));
                self.optimistic(kind, idx, Some(v), None);
            }
            UiCmd::SetStreamMute(kind, idx, m) => {
                self.audio.send(AudioCmd::SetMute(kind, idx, m));
                self.optimistic(kind, idx, None, Some(m));
            }
            UiCmd::SetDefault(kind, name) => self.audio.send(AudioCmd::SetDefault(kind, name)),
            UiCmd::ProfileEdited => {
                let sh = self.shared.lock().unwrap();
                if let Some(id) = sh.active_id() {
                    let _ = sh.store.save_profile(&id);
                }
                let _ = sh.store.save_settings();
                drop(sh);
                self.sent.clear();
                self.regexes.clear();
                self.scan_ports();
            }
            UiCmd::ActivateProfile(id) => self.activate_profile(&id),
            UiCmd::SendTest(cid) => {
                if let Some(c) = profile.as_ref().and_then(|p| p.controls.get(&cid)).cloned() {
                    // Flash: full, then back to whatever it should show.
                    if let Some(out) = &mut self.output {
                        let _ = out.send(&mapping::level_message(&c, 1.0));
                    }
                    self.sent.clear();
                    let tx = self.tx.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(600));
                        let _ = tx.send(Event::Ui(UiCmd::ProfileEdited));
                    });
                }
            }
            UiCmd::Press(cid) => {
                if let Some(p) = &profile {
                    self.press(p, &cid);
                }
            }
            UiCmd::MuteAll => {
                let sh = self.shared.lock().unwrap();
                let cmds: Vec<AudioCmd> =
                    sh.audio.sinks.iter().map(|d| AudioCmd::SetMute(Kind::Sink, d.index, true)).collect();
                drop(sh);
                for c in cmds {
                    self.audio.send(c);
                }
            }
        }
    }

    // ------------------------------------------------------- group behaviour

    fn group_settings(&self, profile: &Profile, gid: &str) -> (Vec<Target>, f32) {
        profile.assignments.get(gid).map(|a| (a.targets.clone(), a.max_volume)).unwrap_or((Vec::new(), 1.0))
    }

    fn vol_range(&self) -> (f32, f32) {
        let s = &self.shared.lock().unwrap().store.settings;
        (s.curve, if s.allow_boost { 1.5 } else { 1.0 })
    }

    /// Current slider position of a group.
    fn group_position(&mut self, profile: &Profile, gid: &str) -> f32 {
        let (targets, max) = self.group_settings(profile, gid);
        let res = self.resolve(profile, &targets);
        let (curve, boost) = self.vol_range();
        match res.first() {
            Some(r) => mapping::inverse_curve(r.volume / (max * boost), curve),
            None => self.positions.get(gid).copied().unwrap_or(0.0),
        }
    }

    fn set_group_position(&mut self, profile: &Profile, gid: &str, pos: f32, osd: bool) {
        self.positions.insert(gid.to_string(), pos);
        let (targets, max) = self.group_settings(profile, gid);
        let (curve, boost) = self.vol_range();
        let vol = mapping::curve(pos, curve) * max * boost;
        let res = self.resolve(profile, &targets);
        for r in &res {
            self.audio.send(AudioCmd::SetVolume(r.kind, r.index, vol));
            self.optimistic(r.kind, r.index, Some(vol), None);
        }
        if osd {
            let sh = self.shared.lock().unwrap();
            if sh.store.settings.osd {
                let text = match res.first() {
                    Some(r) if res.len() == 1 => r.label.clone(),
                    Some(r) => format!("{} +{}", r.label, res.len() - 1),
                    None => {
                        format!("{} (nothing playing)", profile.groups.get(gid).map(|g| g.name.as_str()).unwrap_or(""))
                    }
                };
                let icon = res
                    .first()
                    .map(|r| r.icon.clone())
                    .filter(|i| !i.is_empty())
                    .unwrap_or_else(|| "audio-volume-high".into());
                self.osd.show(&icon, (vol * 100.0).round() as i32, &text);
            }
        }
    }

    fn toggle_group_mute(&mut self, profile: &Profile, gid: &str) {
        let (targets, _) = self.group_settings(profile, gid);
        let res = self.resolve(profile, &targets);
        if res.is_empty() {
            return;
        }
        let mute = !res.iter().all(|r| r.mute);
        for r in &res {
            self.audio.send(AudioCmd::SetMute(r.kind, r.index, mute));
            self.optimistic(r.kind, r.index, None, Some(mute));
        }
        let sh = self.shared.lock().unwrap();
        if sh.store.settings.osd {
            let icon = if mute { "audio-volume-muted" } else { "audio-volume-high" };
            let label = if res.len() == 1 { res[0].label.clone() } else { profile.groups[gid].name.clone() };
            let pct = if mute { 0 } else { (res[0].volume * 100.0).round() as i32 };
            self.osd.show(icon, pct, &format!("{label}{}", if mute { " — muted" } else { "" }));
        }
    }

    /// Assign button: unassign if assigned, else assign the focused app.
    fn assign(&mut self, gid: &str) {
        let mut sh = self.shared.lock().unwrap();
        let Some(id) = sh.active_id() else { return };
        let assigned = sh.store.profiles[&id].assignments.get(gid).is_some_and(|a| !a.targets.is_empty());
        let target = if assigned {
            None
        } else {
            let focus = sh.focus.clone();
            let Some(pid) = focus.pid else {
                sh.log("Assign: no focused window known");
                return;
            };
            let stream = sh.audio.apps.iter().find(|a| self.related_to_focus_uncached(pid, a)).cloned();
            let (pattern, label) = match stream {
                Some(a) => {
                    let pat = if !a.binary.is_empty() { a.binary.clone() } else { a.app_name.clone() };
                    (pat, a.display_name())
                }
                None if !self.focus_binary.is_empty() => (self.focus_binary.clone(), focus.class.clone()),
                None => return,
            };
            Some(Target::App { m: AppMatch { pattern, regex: false }, label })
        };
        let p = sh.store.profiles.get_mut(&id).unwrap();
        let entry = p.assignments.entry(gid.to_string()).or_default();
        match target {
            Some(t) => entry.targets = vec![t],
            None => entry.targets.clear(),
        }
        let _ = sh.store.save_profile(&id);
        sh.repaint();
    }

    /// Run button: make devices the default, launch apps that aren't running.
    fn run_group(&mut self, profile: &Profile, gid: &str) {
        let (targets, _) = self.group_settings(profile, gid);
        for t in targets {
            match &t {
                Target::Output { name, .. } => {
                    self.audio.send(AudioCmd::SetDefault(Kind::Sink, name.clone()));
                    self.osd_device(Kind::Sink, name);
                }
                Target::Input { name, .. } => {
                    self.audio.send(AudioCmd::SetDefault(Kind::Source, name.clone()));
                    self.osd_device(Kind::Source, name);
                }
                Target::App { m, .. }
                    if !m.regex
                        && !m.pattern.contains('*')
                        && self.resolve(profile, std::slice::from_ref(&t)).is_empty() =>
                {
                    let _ = crate::actions::run_command(&format!("{} &", shell_quote(&m.pattern)));
                }
                _ => {}
            }
        }
    }

    // ------------------------------------------------------------ resolving

    fn app_matches(&mut self, m: &AppMatch, a: &AppStream) -> bool {
        let key = if m.regex { format!("re:{}", m.pattern) } else { format!("glob:{}", m.pattern) };
        let re = self
            .regexes
            .entry(key)
            .or_insert_with(|| {
                let src = if m.regex {
                    format!("(?i){}", m.pattern)
                } else {
                    let parts: Vec<String> = m.pattern.split('*').map(regex::escape).collect();
                    format!("(?i)^{}$", parts.join(".*"))
                };
                Regex::new(&src).ok()
            })
            .clone();
        let Some(re) = re else { return false };
        re.is_match(&a.binary) || re.is_match(&a.app_name)
    }

    fn related_to_focus_uncached(&self, focus_pid: u32, a: &AppStream) -> bool {
        if let Some(pid) = a.pid
            && (pid == focus_pid || is_ancestor(focus_pid, pid))
        {
            return true;
        }
        !self.focus_binary.is_empty()
            && (a.binary.eq_ignore_ascii_case(&self.focus_binary)
                || a.binary.trim_end_matches("-bin").eq_ignore_ascii_case(self.focus_binary.trim_end_matches("-bin")))
    }

    fn related_to_focus(&mut self, focus_pid: u32, a: &AppStream) -> bool {
        let key = a.index;
        if let Some(v) = self.focus_cache.get(&key) {
            return *v;
        }
        let v = self.related_to_focus_uncached(focus_pid, a);
        self.focus_cache.insert(key, v);
        v
    }

    fn resolve(&mut self, profile: &Profile, targets: &[Target]) -> Vec<Resolved> {
        let (audio, focus) = {
            let sh = self.shared.lock().unwrap();
            (sh.audio.clone(), sh.focus.clone())
        };
        let stream = |a: &AppStream, kind: Kind| Resolved {
            kind,
            index: a.index,
            label: a.display_name(),
            icon: a.icon.clone(),
            hints: [a.icon.clone(), a.binary.clone(), a.app_name.clone()]
                .into_iter()
                .filter(|h| !h.is_empty())
                .collect(),
            volume: a.volume,
            mute: a.mute,
            meter: (kind == Kind::SinkInput).then_some(MeterKey::SinkInput(a.index)),
        };
        let device = |d: &crate::audio::Device, kind: Kind| Resolved {
            kind,
            index: d.index,
            label: d.description.clone(),
            icon: if d.icon.is_empty() {
                if kind == Kind::Sink { "audio-speakers".into() } else { "audio-input-microphone".into() }
            } else {
                d.icon.clone()
            },
            hints: vec![
                d.icon.clone(),
                if kind == Kind::Sink { "audio-speakers".into() } else { "audio-input-microphone".into() },
            ],
            volume: d.volume,
            mute: d.mute,
            meter: Some(if kind == Kind::Sink {
                MeterKey::Sink(d.name.clone())
            } else {
                MeterKey::Source(d.name.clone())
            }),
        };
        let mut out: Vec<Resolved> = Vec::new();
        let mut seen: HashSet<(Kind, u32)> = HashSet::new();
        let mut push = |r: Resolved, out: &mut Vec<Resolved>| {
            if seen.insert((r.kind, r.index)) {
                out.push(r);
            }
        };
        for t in targets {
            match t {
                Target::Focus => {
                    if let Some(pid) = focus.pid {
                        for a in &audio.apps {
                            if self.related_to_focus(pid, a) {
                                push(stream(a, Kind::SinkInput), &mut out);
                            }
                        }
                    }
                }
                Target::App { m, .. } => {
                    for a in &audio.apps {
                        if self.app_matches(m, a) {
                            push(stream(a, Kind::SinkInput), &mut out);
                        }
                    }
                }
                Target::AppInput { m, .. } => {
                    for a in &audio.app_inputs {
                        if self.app_matches(m, a) {
                            push(stream(a, Kind::SourceOutput), &mut out);
                        }
                    }
                }
                Target::EventSounds => {
                    for a in audio.apps.iter().filter(|a| a.role == "event") {
                        push(stream(a, Kind::SinkInput), &mut out);
                    }
                }
                Target::Unassigned => {
                    let claimed: Vec<AppMatch> = profile
                        .assignments
                        .values()
                        .flat_map(|a| a.targets.iter())
                        .filter_map(|t| if let Target::App { m, .. } = t { Some(m.clone()) } else { None })
                        .collect();
                    for a in &audio.apps {
                        if a.role != "event" && !claimed.iter().any(|m| self.app_matches(m, a)) {
                            push(stream(a, Kind::SinkInput), &mut out);
                        }
                    }
                }
                Target::Output { name, .. } => {
                    if let Some(d) = audio.sinks.iter().find(|d| &d.name == name) {
                        push(device(d, Kind::Sink), &mut out);
                    }
                }
                Target::Input { name, .. } => {
                    if let Some(d) = audio.sources.iter().find(|d| &d.name == name) {
                        push(device(d, Kind::Source), &mut out);
                    }
                }
                Target::DefaultOutput => {
                    if let Some(d) = audio.sinks.iter().find(|d| d.name == audio.default_sink) {
                        push(device(d, Kind::Sink), &mut out);
                    }
                }
                Target::DefaultInput => {
                    if let Some(d) = audio.sources.iter().find(|d| d.name == audio.default_source) {
                        push(device(d, Kind::Source), &mut out);
                    }
                }
            }
        }
        out
    }

    /// Apply a change to our copy of the audio state right away, so LEDs and
    /// relative encoders don't wait for the server round trip.
    fn optimistic(&mut self, kind: Kind, index: u32, vol: Option<f32>, mute: Option<bool>) {
        let mut sh = self.shared.lock().unwrap();
        let a = &mut sh.audio;
        let apply_dev = |d: &mut crate::audio::Device| {
            if let Some(v) = vol {
                d.volume = v;
            }
            if let Some(m) = mute {
                d.mute = m;
            }
        };
        let apply_stream = |d: &mut AppStream| {
            if let Some(v) = vol {
                d.volume = v;
            }
            if let Some(m) = mute {
                d.mute = m;
            }
        };
        match kind {
            Kind::Sink => a.sinks.iter_mut().filter(|d| d.index == index).for_each(apply_dev),
            Kind::Source => a.sources.iter_mut().filter(|d| d.index == index).for_each(apply_dev),
            Kind::SinkInput => a.apps.iter_mut().filter(|d| d.index == index).for_each(apply_stream),
            Kind::SourceOutput => a.app_inputs.iter_mut().filter(|d| d.index == index).for_each(apply_stream),
        }
    }

    // ------------------------------------------------------------- feedback

    /// Recompute strip views and send LED / motor fader state.
    fn feedback(&mut self) {
        let Some(profile) = self.shared.lock().unwrap().active().cloned() else {
            self.shared.lock().unwrap().strips.clear();
            return;
        };
        let audio = self.shared.lock().unwrap().audio.clone();
        let (curve, boost) = self.vol_range();
        let mut strips = Vec::new();
        let mut msgs: Vec<Vec<u8>> = Vec::new();
        let mut meters = HashSet::new();
        let mut leds: HashMap<String, f32> = HashMap::new();
        let ui_visible = self.shared.lock().unwrap().ui_visible;

        for (gid, g) in &profile.groups {
            let (targets, max) = self.group_settings(&profile, gid);
            let res = self.resolve(&profile, &targets);
            let position = match res.first() {
                Some(r) => mapping::inverse_curve(r.volume / (max * boost), curve),
                None => self.positions.get(gid).copied().unwrap_or(0.0),
            };
            let muted = !res.is_empty() && res.iter().all(|r| r.mute);
            let assigned = !targets.is_empty();
            let running = targets.iter().any(|t| match t {
                Target::Output { name, .. } => audio.default_sink == *name,
                Target::Input { name, .. } => audio.default_source == *name,
                _ => false,
            }) || (!res.is_empty()
                && !targets.iter().any(|t| matches!(t, Target::Output { .. } | Target::Input { .. })));

            for (role_ids, value) in [
                (&g.indicators, None),
                (&g.mute_indicators, Some(muted)),
                (&g.assign_indicators, Some(assigned)),
                (&g.run_indicators, Some(running)),
            ] {
                for cid in role_ids {
                    if let Some(c) = profile.controls.get(cid) {
                        msgs.push(match value {
                            None => mapping::level_message(c, position),
                            Some(on) => mapping::indicator_message(c, on),
                        });
                        leds.insert(cid.clone(), value.map_or(position, |on| if on { 1.0 } else { 0.0 }));
                    }
                }
            }
            // Motor faders: echo the position to faders that also accept output.
            for cid in &g.faders {
                if let Some(c) = profile.controls.get(cid)
                    && c.command == crate::profile::Command::Pitch
                    && c.delta.is_none()
                {
                    msgs.push(mapping::level_message(c, position));
                }
            }
            if !g.meters.is_empty() {
                meters.extend(res.iter().filter_map(|r| r.meter.clone()));
            } else if ui_visible {
                meters.extend(res.iter().filter_map(|r| r.meter.clone()).filter(|k| !matches!(k, MeterKey::Source(_))));
            }
            strips.push(StripView {
                id: gid.clone(),
                name: g.name.clone(),
                targets: targets.clone(),
                resolved: res.iter().map(|r| r.label.clone()).collect(),
                position,
                muted,
                peak: self.peaks.get(gid).copied().unwrap_or(0.0),
                assigned,
                running,
                knob: profile
                    .assignments
                    .get(gid)
                    .and_then(|a| a.widget)
                    .map(|w| w == crate::profile::Widget::Knob)
                    .unwrap_or_else(|| {
                        g.name.to_lowercase().starts_with("knob")
                            || g.faders.first().and_then(|c| profile.controls.get(c)).is_some_and(|c| c.delta.is_some())
                    }),
                icon_hint: res.first().map(|r| r.hints.clone()).unwrap_or_default(),
            });
        }
        if ui_visible {
            // The sources panel shows playback meters. Inputs are not metered
            // unless a profile asks for it: that would open the microphone.
            meters.extend(audio.apps.iter().map(|a| MeterKey::SinkInput(a.index)));
            meters.extend(audio.sinks.iter().map(|d| MeterKey::Sink(d.name.clone())));
        }
        for m in msgs {
            self.send(m);
        }
        if meters != self.meters_wanted {
            self.meters_wanted = meters.clone();
            self.audio.send(AudioCmd::SetMeters(meters));
        }
        let mut sh = self.shared.lock().unwrap();
        sh.strips = strips;
        sh.led_state = leds;
        sh.repaint();
    }

    /// Update peak meters (MIDI output and GUI).
    fn meters(&mut self) {
        if self.meters_wanted.is_empty() {
            return;
        }
        let Some(profile) = self.shared.lock().unwrap().active().cloned() else { return };
        let mut msgs = Vec::new();
        let mut any = false;
        for (gid, g) in &profile.groups {
            let (targets, _) = self.group_settings(&profile, gid);
            let res = self.resolve(&profile, &targets);
            let peak = res.iter().filter_map(|r| r.meter.as_ref()).map(|k| self.audio.peak(k)).fold(0.0f32, f32::max);
            let prev = self.peaks.get(gid).copied().unwrap_or(0.0);
            // Fast attack, slow release.
            let v = if peak >= prev { peak } else { (prev - 0.04).max(peak) };
            if (v - prev).abs() > 0.001 {
                any = true;
            }
            self.peaks.insert(gid.clone(), v);
            for cid in &g.meters {
                if let Some(c) = profile.controls.get(cid) {
                    msgs.push(mapping::meter_message(c, v));
                }
            }
        }
        for m in msgs {
            // Meters must be refreshed even when unchanged (Mackie meters decay
            // on the device), so bypass the dedup cache.
            if let Some(out) = &mut self.output {
                let _ = out.send(&m);
            }
        }
        let source_peaks: HashMap<MeterKey, f32> =
            self.meters_wanted.iter().map(|k| (k.clone(), self.audio.peak(k))).collect();
        any |= source_peaks != self.shared.lock().unwrap().source_peaks;
        self.shared.lock().unwrap().source_peaks = source_peaks;
        if any {
            let mut sh = self.shared.lock().unwrap();
            for s in sh.strips.iter_mut() {
                s.peak = self.peaks.get(&s.id).copied().unwrap_or(0.0);
            }
            if sh.ui_visible {
                sh.repaint();
            }
        }
    }
}

fn parent_pid(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command name may contain spaces; fields resume after the last ')'.
    let rest = &stat[stat.rfind(')')? + 2..];
    rest.split_whitespace().nth(1)?.parse().ok()
}

/// Is `ancestor` a (grand)parent of `pid`?
fn is_ancestor(ancestor: u32, mut pid: u32) -> bool {
    for _ in 0..16 {
        match parent_pid(pid) {
            Some(p) if p == ancestor => return true,
            Some(p) if p > 1 => pid = p,
            _ => return false,
        }
    }
    false
}

fn process_binary(pid: u32) -> String {
    std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()))
        .or_else(|| std::fs::read_to_string(format!("/proc/{pid}/comm")).ok().map(|s| s.trim().to_string()))
        .unwrap_or_default()
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
