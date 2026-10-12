//! PulseAudio / PipeWire-pulse backend.
//!
//! A dedicated thread owns the PulseAudio mainloop and context. It keeps an
//! up-to-date [`AudioState`] (refreshed on server events), publishes snapshots
//! through a callback, applies [`AudioCmd`]s and runs peak-detect streams for
//! the meters that are currently wanted.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use libpulse_binding as pa;
use pa::callbacks::ListResult;
use pa::channelmap::{Map as ChannelMap, Position};
use pa::context::subscribe::{Facility, InterestMaskSet};
use pa::context::{Context, FlagSet as CtxFlags, State as CtxState};
use pa::mainloop::api::MainloopInnerType;
use pa::mainloop::standard::{IterateResult, Mainloop};
use pa::proplist::Proplist;
use pa::stream::{FlagSet as StreamFlags, PeekResult, Stream};
use pa::volume::{ChannelVolumes, Volume};

pub const APP_NAME: &str = "Slidewire";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Sink,
    Source,
    SinkInput,
    SourceOutput,
}

#[derive(Debug, Clone, Default)]
pub struct Device {
    pub index: u32,
    pub name: String,
    pub description: String,
    /// Short name ("FiiO K11") and the rest ("Digital Stereo (IEC958)").
    pub label: String,
    pub detail: String,
    pub icon: String,
    /// 1.0 = 100%.
    pub volume: f32,
    pub mute: bool,
    pub layout: Layout,
    pub monitor_source: String,
}

/// The channels of a device or stream: each one's volume relative to the
/// loudest (1.0), and its side (-1 left, 1 right, 0 neither). Volume
/// changes keep these ratios, so a balance set elsewhere survives a fader.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Layout {
    pub levels: Vec<f32>,
    pub sides: Vec<i8>,
}

impl Layout {
    fn new(v: &ChannelVolumes, map: &ChannelMap) -> Self {
        // At 0% the ratios are gone; start again from centred.
        let max = v.max().0 as f32;
        let levels = v.get().iter().map(|c| if max > 0.0 { (c.0 as f32 / max).clamp(0.0, 1.0) } else { 1.0 }).collect();
        let sides = (0..v.len() as usize)
            .map(|i| match map.get().get(i) {
                Some(
                    Position::FrontLeft
                    | Position::RearLeft
                    | Position::SideLeft
                    | Position::FrontLeftOfCenter
                    | Position::TopFrontLeft
                    | Position::TopRearLeft,
                ) => -1,
                Some(
                    Position::FrontRight
                    | Position::RearRight
                    | Position::SideRight
                    | Position::FrontRightOfCenter
                    | Position::TopFrontRight
                    | Position::TopRearRight,
                ) => 1,
                _ => 0,
            })
            .collect();
        Layout { levels, sides }
    }

    pub fn channels(&self) -> usize {
        self.levels.len()
    }

    fn side_level(&self, side: i8) -> Option<f32> {
        self.levels.iter().zip(&self.sides).filter(|(_, s)| **s == side).map(|(l, _)| *l).reduce(f32::max)
    }

    /// -1 (all left) to 1 (all right), the way pavucontrol shows it.
    pub fn balance(&self) -> f32 {
        match (self.side_level(-1), self.side_level(1)) {
            (Some(l), Some(r)) if l.max(r) > 0.0 => (r - l) / l.max(r),
            _ => 0.0,
        }
    }

    /// The same channels with this balance.
    pub fn with_balance(&self, balance: f32) -> Layout {
        let b = balance.clamp(-1.0, 1.0);
        let levels = self
            .sides
            .iter()
            .zip(&self.levels)
            .map(|(side, level)| match side {
                -1 => (1.0 - b).min(1.0),
                1 => (1.0 + b).min(1.0),
                _ => *level,
            })
            .collect();
        Layout { levels, sides: self.sides.clone() }
    }

    /// Raw channel volumes for `frac` (1.0 = 100%) on the loudest channel.
    fn volumes(&self, frac: f32) -> ChannelVolumes {
        let top = frac.clamp(0.0, 1.5) as f64 * Volume::NORMAL.0 as f64;
        let mut cv = ChannelVolumes::default();
        cv.set_len(self.channels() as u8);
        for (v, level) in cv.get_mut().iter_mut().zip(&self.levels) {
            *v = Volume((top * *level as f64).round() as u32);
        }
        cv
    }
}

#[derive(Debug, Clone, Default)]
pub struct AppStream {
    pub index: u32,
    pub app_name: String,
    pub binary: String,
    pub pid: Option<u32>,
    pub media_name: String,
    pub icon: String,
    pub role: String,
    pub volume: f32,
    pub mute: bool,
    pub layout: Layout,
    /// Sink (for playback) or source (for recording) index.
    pub device: u32,
}

impl AppStream {
    pub fn display_name(&self) -> String {
        // Electron/Chromium apps and ALSA clients report a generic name; the
        // binary says more (e.g. "vesktop" instead of "Chromium").
        let generic = self.app_name.trim().is_empty()
            || self.app_name == "Chromium"
            || self.app_name.starts_with("ALSA plug-in")
            || self.app_name == "WEBRTC VoiceEngine";
        if !generic {
            self.app_name.clone()
        } else if !self.binary.is_empty() {
            capitalize(&self.binary)
        } else if !self.app_name.trim().is_empty() {
            self.app_name.clone()
        } else if !self.media_name.trim().is_empty() {
            self.media_name.clone()
        } else {
            format!("Unknown app #{}", self.index)
        }
    }

    /// What an assignment matches on: the binary when known, since it's
    /// stable, else the app or stream name. Never empty, because an empty
    /// pattern would match every app.
    pub fn pattern(&self) -> String {
        [&self.binary, &self.app_name, &self.media_name]
            .into_iter()
            .find(|s| !s.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| self.display_name())
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[derive(Debug, Clone, Default)]
pub struct AudioState {
    pub connected: bool,
    pub sinks: Vec<Device>,
    pub sources: Vec<Device>,
    pub apps: Vec<AppStream>,
    pub app_inputs: Vec<AppStream>,
    pub default_sink: String,
    pub default_source: String,
    /// Loaded server modules (we load null sinks and loopbacks for buses
    /// and monitoring).
    pub modules: Vec<Module>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Module {
    pub index: u32,
    pub name: String,
    pub argument: String,
}

impl AudioState {
    pub fn sink_by_index(&self, i: u32) -> Option<&Device> {
        self.sinks.iter().find(|d| d.index == i)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MeterKey {
    Sink(String),
    Source(String),
    SinkInput(u32),
}

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub enum AudioCmd {
    SetVolume(Kind, u32, f32),
    /// Left/right balance, -1 to 1, keeping the volume.
    SetBalance(Kind, u32, f32),
    SetMute(Kind, u32, bool),
    SetDefault(Kind, String),
    /// Move an app's stream to another device (by name).
    Move(Kind, u32, String),
    SetMeters(HashSet<MeterKey>),
    /// Answer once everything sent before has gone out to the server.
    Flush(Sender<()>),
    /// Load a module (name, arguments).
    LoadModule(String, String),
    UnloadModule(u32),
}

pub type Peaks = Arc<Mutex<HashMap<MeterKey, f32>>>;

/// Interrupts the audio thread's wait in the PulseAudio mainloop, so a
/// command is handled right away while the thread otherwise sleeps.
struct Waker(*mut libpulse_sys::pa_mainloop);
// SAFETY: pa_mainloop_wakeup is meant to be called from other threads, and
// the pointer is only used while the audio thread keeps the mainloop alive
// (it clears the waker under the lock before dropping it).
unsafe impl Send for Waker {}

type WakerSlot = Arc<Mutex<Option<Waker>>>;

/// Clears the waker when the mainloop goes away.
struct WakerGuard(WakerSlot);

impl Drop for WakerGuard {
    fn drop(&mut self) {
        *self.0.lock().unwrap() = None;
    }
}

#[derive(Clone)]
pub struct AudioHandle {
    tx: Sender<AudioCmd>,
    pub peaks: Peaks,
    waker: WakerSlot,
}

impl AudioHandle {
    pub fn send(&self, cmd: AudioCmd) {
        let _ = self.tx.send(cmd);
        if let Some(w) = &*self.waker.lock().unwrap() {
            // SAFETY: see `Waker`.
            unsafe { libpulse_sys::pa_mainloop_wakeup(w.0) };
        }
    }
    pub fn peak(&self, key: &MeterKey) -> f32 {
        self.peaks.lock().unwrap().get(key).copied().unwrap_or(0.0)
    }

    /// Wait (up to `timeout`) until the commands sent so far reached the server.
    pub fn flush(&self, timeout: Duration) {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.send(AudioCmd::Flush(tx));
        let _ = rx.recv_timeout(timeout);
    }
}

/// Start the audio thread. `on_change` is called (on the audio thread) with a
/// fresh snapshot whenever the server state changed. While disconnected it
/// gets an empty state (`connected: false`).
pub fn spawn(on_change: impl Fn(AudioState) + Send + 'static) -> AudioHandle {
    let (tx, rx) = crossbeam_channel::unbounded();
    let peaks: Peaks = Arc::default();
    let waker: WakerSlot = Arc::default();
    let (p, w) = (peaks.clone(), waker.clone());
    std::thread::Builder::new()
        .name("audio".into())
        .spawn(move || {
            loop {
                if let Err(e) = run(&rx, &p, &w, &on_change) {
                    eprintln!("audio: {e}; reconnecting in 2s");
                }
                on_change(AudioState::default());
                std::thread::sleep(Duration::from_secs(2));
                // Commands queued meanwhile name streams of the old server;
                // the same numbers may belong to other streams after a restart.
                while let Ok(cmd) = rx.try_recv() {
                    if let AudioCmd::Flush(done) = cmd {
                        let _ = done.send(());
                    }
                }
            }
        })
        .expect("spawn audio thread");
    AudioHandle { tx, peaks, waker }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Fac {
    Sink,
    Source,
    SinkInput,
    SourceOutput,
    Server,
    Module,
}

#[derive(Default)]
struct Shared {
    state: AudioState,
    dirty: HashSet<Fac>,
    in_flight: HashSet<Fac>,
    changed: bool,
    // Lists being collected by in-flight queries.
    new_sinks: Vec<Device>,
    new_sources: Vec<Device>,
    new_apps: Vec<AppStream>,
    new_app_inputs: Vec<AppStream>,
    new_modules: Vec<Module>,
}

fn run(
    rx: &Receiver<AudioCmd>,
    peaks: &Peaks,
    waker: &WakerSlot,
    on_change: &dyn Fn(AudioState),
) -> anyhow::Result<()> {
    let mut mainloop = Mainloop::new().ok_or_else(|| anyhow::anyhow!("mainloop"))?;
    *waker.lock().unwrap() = Some(Waker(mainloop._inner.get_ptr()));
    let _guard = WakerGuard(waker.clone());
    let mut props = Proplist::new().unwrap();
    let _ = props.set_str("application.name", APP_NAME);
    let _ = props.set_str("application.id", crate::APP_ID);
    let _ = props.set_str("application.icon_name", crate::APP_ID);
    let ctx = Rc::new(RefCell::new(
        Context::new_with_proplist(&mainloop, APP_NAME, &props).ok_or_else(|| anyhow::anyhow!("context"))?,
    ));
    ctx.borrow_mut().connect(None, CtxFlags::NOFAIL, None)?;

    loop {
        if let IterateResult::Err(e) = mainloop.iterate(true) {
            anyhow::bail!("iterate: {e}");
        }
        match ctx.borrow().get_state() {
            CtxState::Ready => break,
            CtxState::Failed | CtxState::Terminated => anyhow::bail!("connection failed"),
            _ => {}
        }
    }

    let shared = Rc::new(RefCell::new(Shared::default()));
    {
        let mut s = shared.borrow_mut();
        s.state.connected = true;
        s.dirty.extend([Fac::Sink, Fac::Source, Fac::SinkInput, Fac::SourceOutput, Fac::Server, Fac::Module]);
    }
    {
        let sh = shared.clone();
        let mut c = ctx.borrow_mut();
        c.set_subscribe_callback(Some(Box::new(move |fac, _op, _idx| {
            let f = match fac {
                Some(Facility::Sink) => Fac::Sink,
                Some(Facility::Source) => Fac::Source,
                Some(Facility::SinkInput) => Fac::SinkInput,
                Some(Facility::SourceOutput) => Fac::SourceOutput,
                Some(Facility::Server) => Fac::Server,
                Some(Facility::Module) => Fac::Module,
                _ => return,
            };
            sh.borrow_mut().dirty.insert(f);
        })));
        c.subscribe(
            InterestMaskSet::SINK
                | InterestMaskSet::SOURCE
                | InterestMaskSet::SINK_INPUT
                | InterestMaskSet::SOURCE_OUTPUT
                | InterestMaskSet::SERVER
                | InterestMaskSet::MODULE,
            |_| {},
        );
    }

    let mut meters: HashMap<MeterKey, Rc<RefCell<Stream>>> = HashMap::new();
    let mut wanted_meters: HashSet<MeterKey> = HashSet::new();
    let mut flushes: Vec<Sender<()>> = Vec::new();

    loop {
        // Sleep until the server has something for us or a command arrives
        // (AudioHandle::send wakes the mainloop). A flush waiting for an
        // answer must not sleep.
        let mut block = flushes.is_empty();
        loop {
            match mainloop.iterate(block) {
                IterateResult::Success(0) if !block => break,
                IterateResult::Success(_) => {}
                IterateResult::Err(e) => anyhow::bail!("iterate: {e}"),
                IterateResult::Quit(_) => anyhow::bail!("quit"),
            }
            block = false;
        }
        match ctx.borrow().get_state() {
            CtxState::Failed | CtxState::Terminated => anyhow::bail!("connection lost"),
            _ => {}
        }
        for done in flushes.drain(..) {
            let _ = done.send(());
        }

        refresh(&ctx, &shared);

        let snapshot = {
            let mut s = shared.borrow_mut();
            if s.changed && s.in_flight.is_empty() {
                s.changed = false;
                Some(s.state.clone())
            } else {
                None
            }
        };
        if let Some(st) = snapshot {
            sync_meters(&ctx, &st, &wanted_meters, &mut meters, peaks);
            on_change(st);
        }

        loop {
            let cmd = match rx.try_recv() {
                Ok(c) => c,
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => return Ok(()),
            };
            match cmd {
                AudioCmd::SetMeters(keys) => {
                    wanted_meters = keys;
                    let st = shared.borrow().state.clone();
                    sync_meters(&ctx, &st, &wanted_meters, &mut meters, peaks);
                }
                AudioCmd::Flush(done) => flushes.push(done),
                other => apply(&ctx, &shared, other),
            }
        }
    }
}

/// `label` holds the card's name ("FiiO K11") when the device has one; the
/// rest of the description becomes `detail`.
fn split_label(mut d: Device) -> Device {
    let nick = std::mem::take(&mut d.label);
    match d.description.strip_prefix(nick.as_str()) {
        Some(rest) if !nick.is_empty() => {
            d.label = nick.clone();
            d.detail = rest.trim().to_string();
        }
        _ => d.label = d.description.clone(),
    }
    d
}

/// Some apps (games under Wine, mostly) send names that were decoded as
/// Latin-1 somewhere on the way: "HELLDIVERSï¿½ 2". Undo that, and drop
/// replacement characters that carry no information.
fn repair_text(s: &str) -> String {
    let mut out = s.to_string();
    if s.chars().all(|c| (c as u32) < 0x100) && s.chars().any(|c| (c as u32) >= 0x80) {
        let bytes: Vec<u8> = s.chars().map(|c| c as u8).collect();
        if let Ok(fixed) = String::from_utf8(bytes) {
            out = fixed;
        }
    }
    if out.contains('\u{FFFD}') {
        out = out.replace('\u{FFFD}', "").split_whitespace().collect::<Vec<_>>().join(" ");
    }
    out
}

fn props_str(p: &Proplist, key: &str) -> String {
    p.get_str(key).unwrap_or_default()
}

fn vol_frac(v: &ChannelVolumes) -> f32 {
    v.max().0 as f32 / Volume::NORMAL.0 as f32
}

fn refresh(ctx: &Rc<RefCell<Context>>, shared: &Rc<RefCell<Shared>>) {
    let todo: Vec<Fac> = {
        let mut s = shared.borrow_mut();
        let todo: Vec<Fac> = s.dirty.iter().copied().filter(|f| !s.in_flight.contains(f)).collect();
        for f in &todo {
            s.dirty.remove(f);
            s.in_flight.insert(*f);
        }
        todo
    };
    let intro = ctx.borrow().introspect();
    for f in todo {
        let sh = shared.clone();
        match f {
            Fac::Sink => {
                intro.get_sink_info_list(move |r| {
                    let mut s = sh.borrow_mut();
                    match r {
                        ListResult::Item(i) => {
                            let d = Device {
                                index: i.index,
                                name: i.name.as_deref().unwrap_or_default().to_string(),
                                description: i.description.as_deref().unwrap_or_default().to_string(),
                                label: props_str(&i.proplist, "device.description"),
                                detail: String::new(),
                                icon: props_str(&i.proplist, "device.icon_name"),
                                volume: vol_frac(&i.volume),
                                mute: i.mute,
                                layout: Layout::new(&i.volume, &i.channel_map),
                                monitor_source: i.monitor_source_name.as_deref().unwrap_or_default().to_string(),
                            };
                            s.new_sinks.push(split_label(d));
                        }
                        ListResult::End | ListResult::Error => {
                            s.state.sinks = std::mem::take(&mut s.new_sinks);
                            s.in_flight.remove(&Fac::Sink);
                            s.changed = true;
                        }
                    }
                });
            }
            Fac::Source => {
                intro.get_source_info_list(move |r| {
                    let mut s = sh.borrow_mut();
                    match r {
                        ListResult::Item(i) => {
                            // Skip sink monitors; they are reachable through the sink.
                            if i.monitor_of_sink.is_some() {
                                return;
                            }
                            let d = Device {
                                index: i.index,
                                name: i.name.as_deref().unwrap_or_default().to_string(),
                                description: i.description.as_deref().unwrap_or_default().to_string(),
                                label: props_str(&i.proplist, "device.description"),
                                detail: String::new(),
                                icon: props_str(&i.proplist, "device.icon_name"),
                                volume: vol_frac(&i.volume),
                                mute: i.mute,
                                layout: Layout::new(&i.volume, &i.channel_map),
                                monitor_source: String::new(),
                            };
                            s.new_sources.push(split_label(d));
                        }
                        ListResult::End | ListResult::Error => {
                            s.state.sources = std::mem::take(&mut s.new_sources);
                            s.in_flight.remove(&Fac::Source);
                            s.changed = true;
                        }
                    }
                });
            }
            Fac::SinkInput => {
                intro.get_sink_input_info_list(move |r| {
                    let mut s = sh.borrow_mut();
                    match r {
                        ListResult::Item(i) => {
                            let a = app_stream(
                                i.index,
                                &i.proplist,
                                &i.volume,
                                &i.channel_map,
                                i.mute,
                                i.sink,
                                i.name.as_deref(),
                            );
                            if a.app_name != APP_NAME && !is_loopback(&a, &i.proplist) {
                                s.new_apps.push(a);
                            }
                        }
                        ListResult::End | ListResult::Error => {
                            s.state.apps = std::mem::take(&mut s.new_apps);
                            s.in_flight.remove(&Fac::SinkInput);
                            s.changed = true;
                        }
                    }
                });
            }
            Fac::SourceOutput => {
                intro.get_source_output_info_list(move |r| {
                    let mut s = sh.borrow_mut();
                    match r {
                        ListResult::Item(i) => {
                            let a = app_stream(
                                i.index,
                                &i.proplist,
                                &i.volume,
                                &i.channel_map,
                                i.mute,
                                i.source,
                                i.name.as_deref(),
                            );
                            // Hide our own and other mixers' peak-detect streams.
                            let peak = a.media_name.to_lowercase().contains("peak");
                            if a.app_name != APP_NAME && !peak && !is_loopback(&a, &i.proplist) {
                                s.new_app_inputs.push(a);
                            }
                        }
                        ListResult::End | ListResult::Error => {
                            s.state.app_inputs = std::mem::take(&mut s.new_app_inputs);
                            s.in_flight.remove(&Fac::SourceOutput);
                            s.changed = true;
                        }
                    }
                });
            }
            Fac::Server => {
                intro.get_server_info(move |i| {
                    let mut s = sh.borrow_mut();
                    s.state.default_sink = i.default_sink_name.as_deref().unwrap_or_default().to_string();
                    s.state.default_source = i.default_source_name.as_deref().unwrap_or_default().to_string();
                    s.in_flight.remove(&Fac::Server);
                    s.changed = true;
                });
            }
            Fac::Module => {
                intro.get_module_info_list(move |r| {
                    let mut s = sh.borrow_mut();
                    match r {
                        ListResult::Item(i) => {
                            let m = Module {
                                index: i.index,
                                name: i.name.as_deref().unwrap_or_default().to_string(),
                                argument: i.argument.as_deref().unwrap_or_default().to_string(),
                            };
                            s.new_modules.push(m);
                        }
                        ListResult::End | ListResult::Error => {
                            s.state.modules = std::mem::take(&mut s.new_modules);
                            s.in_flight.remove(&Fac::Module);
                            s.changed = true;
                        }
                    }
                });
            }
        }
    }
}

/// Our bus and monitor loopbacks: plumbing, not apps.
fn is_loopback(a: &AppStream, p: &Proplist) -> bool {
    let ours = |n: &str| n == crate::routing::BUS_STREAM || n == crate::routing::MONITOR_STREAM;
    ours(&a.media_name) || ours(&props_str(p, "media.name"))
}

fn app_stream(
    index: u32,
    p: &Proplist,
    v: &ChannelVolumes,
    map: &ChannelMap,
    mute: bool,
    device: u32,
    name: Option<&str>,
) -> AppStream {
    AppStream {
        index,
        app_name: repair_text(&props_str(p, "application.name")),
        binary: props_str(p, "application.process.binary"),
        pid: props_str(p, "application.process.id").parse().ok(),
        media_name: repair_text(&name.map(str::to_string).unwrap_or_else(|| props_str(p, "media.name"))),
        icon: props_str(p, "application.icon_name"),
        role: props_str(p, "media.role"),
        volume: vol_frac(v),
        mute,
        layout: Layout::new(v, map),
        device,
    }
}

/// A device's or stream's channel layout and volume.
fn layout_of(st: &AudioState, kind: Kind, index: u32) -> Option<(Layout, f32)> {
    let found = match kind {
        Kind::Sink => st.sinks.iter().find(|d| d.index == index).map(|d| (d.layout.clone(), d.volume)),
        Kind::Source => st.sources.iter().find(|d| d.index == index).map(|d| (d.layout.clone(), d.volume)),
        Kind::SinkInput => st.apps.iter().find(|d| d.index == index).map(|d| (d.layout.clone(), d.volume)),
        Kind::SourceOutput => st.app_inputs.iter().find(|d| d.index == index).map(|d| (d.layout.clone(), d.volume)),
    };
    found.filter(|(l, _)| l.channels() > 0)
}

fn store_layout(st: &mut AudioState, kind: Kind, index: u32, layout: Layout) {
    let slot = match kind {
        Kind::Sink => st.sinks.iter_mut().find(|d| d.index == index).map(|d| &mut d.layout),
        Kind::Source => st.sources.iter_mut().find(|d| d.index == index).map(|d| &mut d.layout),
        Kind::SinkInput => st.apps.iter_mut().find(|d| d.index == index).map(|d| &mut d.layout),
        Kind::SourceOutput => st.app_inputs.iter_mut().find(|d| d.index == index).map(|d| &mut d.layout),
    };
    if let Some(slot) = slot {
        *slot = layout;
    }
}

fn set_volume(intro: &mut pa::context::introspect::Introspector, kind: Kind, index: u32, cv: &ChannelVolumes) {
    match kind {
        Kind::Sink => drop(intro.set_sink_volume_by_index(index, cv, None)),
        Kind::Source => drop(intro.set_source_volume_by_index(index, cv, None)),
        Kind::SinkInput => drop(intro.set_sink_input_volume(index, cv, None)),
        Kind::SourceOutput => drop(intro.set_source_output_volume(index, cv, None)),
    }
}

fn apply(ctx: &Rc<RefCell<Context>>, shared: &Rc<RefCell<Shared>>, cmd: AudioCmd) {
    let mut c = ctx.borrow_mut();
    let mut intro = c.introspect();
    match cmd {
        AudioCmd::SetVolume(kind, index, frac) => {
            let Some((layout, _)) = layout_of(&shared.borrow().state, kind, index) else { return };
            set_volume(&mut intro, kind, index, &layout.volumes(frac));
        }
        AudioCmd::SetBalance(kind, index, balance) => {
            let mut s = shared.borrow_mut();
            let Some((layout, volume)) = layout_of(&s.state, kind, index) else { return };
            let layout = layout.with_balance(balance);
            set_volume(&mut intro, kind, index, &layout.volumes(volume));
            // Until the server reports back, volume changes use the new balance.
            store_layout(&mut s.state, kind, index, layout);
        }
        AudioCmd::SetMute(kind, index, m) => match kind {
            Kind::Sink => drop(intro.set_sink_mute_by_index(index, m, None)),
            Kind::Source => drop(intro.set_source_mute_by_index(index, m, None)),
            Kind::SinkInput => drop(intro.set_sink_input_mute(index, m, None)),
            Kind::SourceOutput => drop(intro.set_source_output_mute(index, m, None)),
        },
        AudioCmd::SetDefault(kind, name) => match kind {
            Kind::Sink => drop(c.set_default_sink(&name, |_| {})),
            Kind::Source => drop(c.set_default_source(&name, |_| {})),
            _ => {}
        },
        AudioCmd::Move(kind, index, device) => match kind {
            Kind::SinkInput => drop(intro.move_sink_input_by_name(index, &device, None)),
            Kind::SourceOutput => drop(intro.move_source_output_by_name(index, &device, None)),
            _ => {}
        },
        AudioCmd::LoadModule(name, args) => drop(intro.load_module(&name, &args, |_| {})),
        AudioCmd::UnloadModule(index) => drop(intro.unload_module(index, |_| {})),
        AudioCmd::SetMeters(_) | AudioCmd::Flush(_) => {}
    }
}

/// Create or drop peak-detect streams so that exactly the wanted meters run.
fn sync_meters(
    ctx: &Rc<RefCell<Context>>,
    st: &AudioState,
    wanted: &HashSet<MeterKey>,
    meters: &mut HashMap<MeterKey, Rc<RefCell<Stream>>>,
    peaks: &Peaks,
) {
    // Resolve each wanted meter to (source to record from, optional sink input).
    let resolve = |k: &MeterKey| -> Option<(String, Option<u32>)> {
        match k {
            MeterKey::Sink(name) => st.sinks.iter().find(|d| &d.name == name).map(|d| (d.monitor_source.clone(), None)),
            MeterKey::Source(name) => st.sources.iter().find(|d| &d.name == name).map(|d| (d.name.clone(), None)),
            MeterKey::SinkInput(i) => {
                let a = st.apps.iter().find(|a| a.index == *i)?;
                let sink = st.sink_by_index(a.device)?;
                Some((sink.monitor_source.clone(), Some(*i)))
            }
        }
    };

    let stale: Vec<MeterKey> = meters
        .iter()
        .filter(|(k, s)| {
            !wanted.contains(*k)
                || resolve(k).is_none()
                || matches!(s.borrow().get_state(), pa::stream::State::Failed | pa::stream::State::Terminated)
        })
        .map(|(k, _)| k.clone())
        .collect();
    for k in stale {
        if let Some(s) = meters.remove(&k) {
            let mut s = s.borrow_mut();
            s.set_read_callback(None);
            let _ = s.disconnect();
        }
        peaks.lock().unwrap().remove(&k);
    }

    for k in wanted {
        if meters.contains_key(k) {
            continue;
        }
        let Some((source, sink_input)) = resolve(k) else { continue };
        if source.is_empty() {
            continue;
        }
        let spec = pa::sample::Spec { format: pa::sample::Format::F32le, channels: 1, rate: 25 };
        let Some(stream) = Stream::new(&mut ctx.borrow_mut(), "Peak detect", &spec, None) else { continue };
        let stream = Rc::new(RefCell::new(stream));
        if let Some(i) = sink_input
            && stream.borrow_mut().set_monitor_stream(i).is_err()
        {
            continue;
        }
        let attr = pa::def::BufferAttr {
            maxlength: u32::MAX,
            tlength: u32::MAX,
            prebuf: u32::MAX,
            minreq: u32::MAX,
            fragsize: 4,
        };
        let flags = StreamFlags::PEAK_DETECT
            | StreamFlags::ADJUST_LATENCY
            | StreamFlags::DONT_MOVE
            | StreamFlags::DONT_INHIBIT_AUTO_SUSPEND;
        if stream.borrow_mut().connect_record(Some(&source), Some(&attr), flags).is_err() {
            continue;
        }
        let weak = Rc::downgrade(&stream);
        let pk = peaks.clone();
        let key = k.clone();
        stream.borrow_mut().set_read_callback(Some(Box::new(move |_len| {
            let Some(s) = weak.upgrade() else { return };
            let Ok(mut s) = s.try_borrow_mut() else { return };
            let mut level: Option<f32> = None;
            loop {
                match s.peek() {
                    Ok(PeekResult::Empty) | Err(_) => break,
                    Ok(PeekResult::Hole(_)) => {
                        let _ = s.discard();
                    }
                    Ok(PeekResult::Data(d)) => {
                        if d.len() >= 4 {
                            let b = &d[d.len() - 4..];
                            let v = f32::from_le_bytes([b[0], b[1], b[2], b[3]]).abs();
                            level = Some(level.map_or(v, |l: f32| l.max(v)));
                        }
                        let _ = s.discard();
                    }
                }
            }
            if let Some(v) = level {
                pk.lock().unwrap().insert(key.clone(), v.clamp(0.0, 1.0));
            }
        })));
        meters.insert(k.clone(), stream);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repairs_mojibake() {
        assert_eq!(repair_text("HELLDIVERSï¿½ 2"), "HELLDIVERS 2");
        assert_eq!(repair_text("CafÃ© Radio"), "Café Radio");
        assert_eq!(repair_text("Café Radio"), "Café Radio");
        assert_eq!(repair_text("Firefox"), "Firefox");
        assert_eq!(repair_text("ÄÖÜ"), "ÄÖÜ");
    }

    #[test]
    fn app_names_never_empty() {
        let a = AppStream { index: 7, app_name: " ".into(), ..Default::default() };
        assert_eq!(a.display_name(), "Unknown app #7");
        assert!(!a.pattern().trim().is_empty());
        let a = AppStream { app_name: "Chromium".into(), binary: "vesktop".into(), ..Default::default() };
        assert_eq!((a.display_name().as_str(), a.pattern().as_str()), ("Vesktop", "vesktop"));
        let a = AppStream { media_name: "audio stream #1".into(), ..Default::default() };
        assert_eq!((a.display_name().as_str(), a.pattern().as_str()), ("audio stream #1", "audio stream #1"));
    }

    #[test]
    fn volume_keeps_the_balance() {
        let stereo = Layout { levels: vec![1.0, 0.5], sides: vec![-1, 1] };
        let cv = stereo.volumes(0.8);
        let raw: Vec<u32> = cv.get().iter().map(|v| v.0).collect();
        let n = Volume::NORMAL.0 as f32;
        assert_eq!(raw, [(0.8 * n).round() as u32, (0.4 * n).round() as u32]);
        assert!((stereo.balance() + 0.5).abs() < 1e-6);
        let centred = stereo.with_balance(0.0);
        assert_eq!(centred.levels, [1.0, 1.0]);
        let right = stereo.with_balance(0.25);
        assert_eq!(right.levels, [0.75, 1.0]);
        assert!((right.balance() - 0.25).abs() < 1e-6);
        // Mono has no sides: nothing to balance.
        let mono = Layout { levels: vec![1.0], sides: vec![0] };
        assert_eq!(mono.with_balance(-1.0).levels, [1.0]);
        assert_eq!(mono.balance(), 0.0);
    }

    #[test]
    fn device_labels() {
        let d = split_label(Device {
            description: "FiiO K11 Digital Stereo (IEC958)".into(),
            label: "FiiO K11".into(),
            ..Default::default()
        });
        assert_eq!((d.label.as_str(), d.detail.as_str()), ("FiiO K11", "Digital Stereo (IEC958)"));
        let d = split_label(Device { description: "Virtual Sink".into(), ..Default::default() });
        assert_eq!((d.label.as_str(), d.detail.as_str()), ("Virtual Sink", ""));
    }
}
