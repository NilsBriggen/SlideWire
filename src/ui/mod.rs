//! The mixer / settings window (egui), styled as a hardware console.

mod controller;
mod diagram;
mod history;
mod icons;
mod mixer;
mod pages;
mod theme;
mod welcome;
mod widgets;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use eframe::egui::{self, Color32, RichText, Sense, Stroke, Vec2, pos2, vec2};
use rust_i18n::t;

use crate::WindowCmd;
use crate::engine::{Event, Notice, SharedRef, UiCmd};
use crate::profile::Profile;
use theme::{icon, *};

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Mixer,
    Controller,
    Buttons,
    Profiles,
    Settings,
    Log,
}

#[derive(Clone, PartialEq)]
enum PickPurpose {
    Import,
    /// Export the profile with this id: everything, or (false) just the
    /// controller in MIDI Mixer's format.
    Export(String, bool),
}

/// A short message in the bottom-right corner.
pub(super) struct Toast {
    icon: &'static str,
    text: String,
    /// None: the accent colour.
    color: Option<Color32>,
    born: Instant,
    undo: Option<ToastUndo>,
    /// Problems stay until dismissed, and can be copied.
    sticky: bool,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum ToastUndo {
    /// Undo the last profile edit.
    Edit,
    /// Undo the last scene recall.
    Scene,
}

/// Something destructive waiting for a yes.
pub(super) struct Confirm {
    pub title: String,
    pub body: String,
    pub button: String,
    pub action: ConfirmAction,
    /// A second, safe choice next to the destructive one.
    pub alt: Option<(String, ConfirmAction)>,
}

pub(super) enum ConfirmAction {
    DeleteProfile(String),
    RemoveBank(usize),
    /// Save the strips over an existing scene.
    SaveScene(String),
    /// Import a profile; `trusted` keeps its command and key buttons.
    Import {
        path: String,
        trusted: bool,
    },
}

pub struct App {
    shared: SharedRef,
    events: Sender<Event>,
    window: Receiver<WindowCmd>,
    tab: Tab,
    icons: icons::Icons,
    logo: Option<egui::TextureHandle>,
    custom_pattern: String,
    custom_regex: bool,
    import_path: String,
    picked: Arc<Mutex<Option<(PickPurpose, PathBuf)>>>,
    toasts: Vec<Toast>,
    welcome: Option<welcome::Welcome>,
    /// In the welcome: a picked controller that isn't a nanoKONTROL2.
    welcome_other: Option<String>,
    history: RefCell<history::History>,
    show_shortcuts: bool,
    /// Ctrl+F asked for the sources filter.
    want_filter: bool,
    /// Channel strip picked with the number keys.
    selected_channel: Option<u32>,
    expanded_control: Option<String>,
    control_search: String,
    scene_name: String,
    /// Strips whose hardware was just touched, and how recently (0..=1).
    touched: HashMap<String, f32>,
    /// The scene being renamed: (old name, new name).
    scene_rename: Option<(String, String)>,
    source_filter: String,
    /// Sources panel: hide what's already on a strip.
    unassigned_only: bool,
    /// Width of the names in the sources panel.
    source_name_w: f32,
    /// Name typed for a new bus.
    new_bus: String,
    /// The zoom factor last applied from settings.
    zoom: f32,
    /// The language setting last applied.
    language: Option<String>,
    /// Width of the Scenes and Sources buttons in the console header.
    header_buttons_w: f32,
    /// The page on screen and since when (it fades in).
    shown_tab: Tab,
    tab_since: Instant,
    log_filter: String,
    log_warnings_only: bool,
    confirm: Option<Confirm>,
    /// Profile being renamed: (id, text).
    profile_rename: Option<(String, String)>,
    /// LED test in progress: started, control ids, next index.
    led_test: Option<(Instant, Vec<String>, usize)>,
    /// Strip being renamed in its menu: (group id, text).
    rename: Option<(String, String)>,
    /// Peak-hold per strip: (value, time it was set).
    meter_hold: HashMap<String, (f32, f64)>,
    show_sources: bool,
    show_doctor: bool,
    doctor: Option<(Instant, Vec<crate::doctor::Check>)>,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext,
        shared: SharedRef,
        events: Sender<Event>,
        window: Receiver<WindowCmd>,
    ) -> App {
        theme::install(&cc.egui_ctx, crate::i18n::cjk());
        let ctx = cc.egui_ctx.clone();
        crate::appearance::watch(move || ctx.request_repaint());
        egui_extras::install_image_loaders(&cc.egui_ctx);
        {
            let mut sh = shared.lock().unwrap();
            sh.egui = Some(cc.egui_ctx.clone());
            sh.ui_visible = true;
        }
        let _ = events.send(Event::Ui(UiCmd::ProfileEdited));
        let logo = egui::ColorImage::from_rgba_unmultiplied([64, 64], &crate::icon::rgba(64));
        let first_run = shared.lock().unwrap().store.first_run;
        App {
            shared,
            events,
            window,
            // SLIDEWIRE_TAB picks the starting page (used by the screenshot script).
            tab: match crate::config::env("TAB").as_deref() {
                Some("controller") => Tab::Controller,
                Some("buttons") => Tab::Buttons,
                Some("profiles") => Tab::Profiles,
                Some("settings") => Tab::Settings,
                Some("log") => Tab::Log,
                _ => Tab::Mixer,
            },
            icons: icons::Icons::new(),
            logo: Some(cc.egui_ctx.load_texture("logo", logo, egui::TextureOptions::LINEAR)),
            custom_pattern: String::new(),
            custom_regex: false,
            import_path: String::new(),
            picked: Arc::default(),
            toasts: Vec::new(),
            welcome: (first_run || crate::config::env("TAB").as_deref() == Some("welcome")).then(welcome::Welcome::new),
            history: RefCell::default(),
            show_shortcuts: false,
            want_filter: false,
            selected_channel: None,
            expanded_control: None,
            control_search: String::new(),
            scene_name: String::new(),
            touched: HashMap::new(),
            scene_rename: None,
            rename: None,
            source_filter: String::new(),
            unassigned_only: false,
            source_name_w: 146.0,
            new_bus: String::new(),
            welcome_other: None,
            zoom: 0.0,
            language: None,
            header_buttons_w: 290.0,
            shown_tab: Tab::Mixer,
            tab_since: Instant::now(),
            log_filter: String::new(),
            log_warnings_only: false,
            led_test: None,
            confirm: None,
            profile_rename: None,
            meter_hold: HashMap::new(),
            show_sources: true,
            show_doctor: crate::config::env("TAB").as_deref() == Some("doctor"),
            doctor: None,
        }
    }

    fn send(&self, cmd: UiCmd) {
        let _ = self.events.send(Event::Ui(cmd));
    }

    fn flash(&mut self, msg: impl Into<String>) {
        self.toast(icon::INFO, msg, None, None);
    }

    /// Something went wrong: an amber toast that stays until dismissed.
    pub(super) fn problem(&mut self, msg: impl Into<String>) {
        self.push_toast(Toast {
            icon: icon::WARNING,
            text: msg.into(),
            color: Some(ASSIGN),
            born: Instant::now(),
            undo: None,
            sticky: true,
        });
    }

    fn toast(&mut self, icon: &'static str, text: impl Into<String>, color: Option<Color32>, undo: Option<ToastUndo>) {
        self.push_toast(Toast { icon, text: text.into(), color, born: Instant::now(), undo, sticky: false });
    }

    fn push_toast(&mut self, toast: Toast) {
        // The same message again just restarts its timer.
        self.toasts.retain(|t| t.text != toast.text);
        self.toasts.push(toast);
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
    }

    /// Run `f` on the active profile, then let the engine save and refresh.
    /// The edit can be undone.
    fn edit_profile(&self, f: impl FnOnce(&mut Profile)) {
        let mut sh = self.shared.lock().unwrap();
        if let Some(id) = sh.active_id()
            && let Some(p) = sh.store.profiles.get_mut(&id)
        {
            let before = p.clone();
            f(p);
            let json = |p: &Profile| serde_json::to_string(p).unwrap_or_default();
            if json(&before) != json(p) {
                self.history.borrow_mut().record(&id, &before, Instant::now());
            }
        }
        drop(sh);
        self.send(UiCmd::ProfileEdited);
    }

    /// Save the strips as a scene. Saving over one can be undone.
    pub(super) fn save_scene(&mut self, name: &str) {
        let replacing = {
            let sh = self.shared.lock().unwrap();
            let id = sh.active_id();
            let p = sh.active();
            match (id, p) {
                (Some(id), Some(p)) if p.scenes.contains_key(name) => {
                    self.history.borrow_mut().record(&id, p, Instant::now());
                    true
                }
                _ => false,
            }
        };
        self.send(UiCmd::SaveScene(name.to_string()));
        if replacing {
            self.toast(icon::FLOPPY_DISK, t!("Saved over scene \"%{name}\"", name = name), None, Some(ToastUndo::Edit));
        } else {
            self.flash(t!("Saved scene \"%{name}\"", name = name));
        }
    }

    /// Start learning control `cid`: the next message from the controller
    /// becomes its MIDI address.
    pub(super) fn learn(&self, cid: &str) {
        let mut sh = self.shared.lock().unwrap();
        sh.learn = Some(crate::engine::LearnRequest { control_id: cid.to_string() });
        sh.repaint();
    }

    /// While learning: a bar on every page saying what to touch.
    fn learn_bar(&mut self, ui: &mut egui::Ui) {
        let (learn, name) = {
            let sh = self.shared.lock().unwrap();
            let name = sh.learn.as_ref().and_then(|l| sh.active().map(|p| p.control_name(&l.control_id)));
            (sh.learn.clone(), name)
        };
        let Some(_) = learn else { return };
        let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
        let mut stop = cancel;
        egui::Panel::top("learn")
            .frame(egui::Frame::new().fill(ASSIGN.gamma_multiply(0.18)).inner_margin(egui::Margin::symmetric(16, 8)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let t = ui.input(|i| i.time) as f32;
                    let pulse = 0.6 + 0.4 * (t * 4.0).sin().abs();
                    ui.label(RichText::new(icon::CROSSHAIR).color(ASSIGN.gamma_multiply(pulse)).size(16.0));
                    ui.label(
                        RichText::new(match name.as_deref() {
                            Some(control) => {
                                t!("Move or press the control for %{control} on your controller…", control = control)
                            }
                            None => t!("Move or press the control for this on your controller…"),
                        })
                        .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(t!("Cancel")).clicked() {
                            stop = true;
                        }
                        widgets::hint(ui, &t!("Esc cancels"));
                    });
                });
                ui.ctx().request_repaint_after(Duration::from_millis(50));
            });
        if stop {
            self.shared.lock().unwrap().learn = None;
        }
    }

    fn page(&mut self, ui: &mut egui::Ui) {
        match self.tab {
            Tab::Mixer => self.mixer(ui),
            Tab::Controller => self.controller(ui),
            Tab::Buttons => self.buttons(ui),
            Tab::Profiles => self.profiles(ui),
            Tab::Settings => self.settings(ui),
            Tab::Log => self.log(ui),
        }
    }

    /// Switch language when the setting changes; CJK languages need a font.
    fn sync_language(&mut self, ctx: &egui::Context, language: &str) {
        if self.language.as_deref() == Some(language) {
            return;
        }
        let first = self.language.is_none();
        self.language = Some(language.to_string());
        crate::i18n::apply(language);
        let cjk = crate::i18n::cjk();
        if cjk.is_some() || !first {
            // Keep a CJK font that's already loaded (the picker loads one).
            theme::install(ctx, cjk.or(theme::loaded_cjk().as_deref()));
        }
        ctx.request_repaint();
    }

    /// The size setting and Ctrl+/Ctrl- zoom are the same thing: apply the
    /// setting when it changes, and save zooming done with the keyboard.
    fn sync_zoom(&mut self, ctx: &egui::Context, scale: f32) {
        let scale = scale.clamp(0.5, 3.0);
        if (scale - self.zoom).abs() > 0.001 {
            ctx.set_zoom_factor(scale);
            self.zoom = scale;
        } else if (ctx.zoom_factor() - self.zoom).abs() > 0.001 {
            let z = ctx.zoom_factor();
            self.zoom = z;
            self.edit_settings(|s| s.ui_scale = z);
        }
    }

    /// Remember the window's size (and position, where known) for next time.
    fn remember_geometry(&self, ctx: &egui::Context) {
        let (inner, outer, maximized, minimized) = ctx.input(|i| {
            let v = i.viewport();
            (v.inner_rect, v.outer_rect, v.maximized.unwrap_or(false), v.minimized.unwrap_or(false))
        });
        let Some(inner) = inner else { return };
        if minimized || inner.width() < 100.0 {
            return;
        }
        // egui's points shrink with the UI size; the window is sized in the desktop's.
        let z = ctx.zoom_factor();
        let (inner, outer) = (inner * z, outer.map(|r| r * z));
        let mut sh = self.shared.lock().unwrap();
        let old = sh.store.settings.window;
        // Maximized: keep the size it had before, to go back to.
        let (width, height) = match old {
            Some(o) if maximized => (o.width, o.height),
            _ => (inner.width().round(), inner.height().round()),
        };
        let pos = outer.filter(|_| !maximized).map(|r| (r.min.x.round(), r.min.y.round()));
        let geometry = crate::config::WindowGeometry {
            width,
            height,
            x: pos.map(|p| p.0).or(old.and_then(|o| o.x)),
            y: pos.map(|p| p.1).or(old.and_then(|o| o.y)),
            maximized,
        };
        if old != Some(geometry) {
            sh.store.settings.window = Some(geometry);
            sh.store.touch_settings();
        }
    }

    /// Empty state for pages that need a profile.
    pub(super) fn no_profile(&mut self, ui: &mut egui::Ui) {
        widgets::card(ui, icon::PIANO_KEYS, &t!("No controller profile yet"), |ui| {
            ui.label(t!("A profile says which controls your controller has and what they do."));
            ui.add_space(SP_S);
            ui.horizontal_wrapped(|ui| {
                if ui.button(format!("{}  Korg nanoKONTROL2", icon::PLUS)).clicked() {
                    let id = crate::profile::new_id();
                    let mut sh = self.shared.lock().unwrap();
                    sh.store.profiles.insert(id.clone(), Profile::nanokontrol2());
                    sh.store.touch(&id);
                    drop(sh);
                    self.send(UiCmd::ActivateProfile(id));
                }
                if ui
                    .button(format!("{}  {}", icon::DOWNLOAD_SIMPLE, t!("Import a profile or MIDI Mixer preset")))
                    .clicked()
                {
                    self.tab = Tab::Profiles;
                    self.pick_file(PickPurpose::Import);
                }
                if ui.button(format!("{}  {}", icon::CROSSHAIR, t!("Map my own controller"))).clicked() {
                    let id = crate::profile::new_id();
                    let p = Profile { name: t!("My controller").to_string(), ..Default::default() };
                    let mut sh = self.shared.lock().unwrap();
                    sh.store.profiles.insert(id.clone(), p);
                    sh.store.touch(&id);
                    drop(sh);
                    self.send(UiCmd::ActivateProfile(id));
                    self.tab = Tab::Controller;
                }
            });
        });
    }

    /// Change settings from outside the Settings page (buses, routes).
    fn edit_settings(&self, f: impl FnOnce(&mut crate::config::Settings)) {
        f(&mut self.shared.lock().unwrap().store.settings);
        self.send(UiCmd::ProfileEdited);
    }

    fn undo(&mut self, redo: bool) {
        let shared = self.shared.clone();
        let current = |id: &str| shared.lock().unwrap().store.profiles.get(id).cloned();
        let step = if redo { self.history.borrow_mut().redo(current) } else { self.history.borrow_mut().undo(current) };
        let Some((id, p)) = step else {
            self.flash(if redo { t!("Nothing to redo") } else { t!("Nothing to undo") });
            return;
        };
        let mut sh = self.shared.lock().unwrap();
        sh.store.profiles.insert(id.clone(), p);
        sh.store.touch(&id);
        drop(sh);
        self.send(UiCmd::ProfileEdited);
        self.toast(icon::ARROW_COUNTER_CLOCKWISE, if redo { t!("Redone") } else { t!("Undone") }, None, None);
    }

    /// Turn the engine's notices into toasts.
    fn take_notices(&mut self) {
        let notices = std::mem::take(&mut self.shared.lock().unwrap().notices);
        for n in notices {
            match n {
                Notice::Connected(d) => {
                    self.toast(icon::PLUGS_CONNECTED, t!("%{device} connected", device = d), Some(RUN), None)
                }
                Notice::Disconnected(d) => {
                    self.toast(icon::PLUGS, t!("%{device} disconnected", device = d), Some(ASSIGN), None)
                }
                Notice::ProfileSwitched(p) => {
                    self.toast(icon::FOLDER_SIMPLE, t!("Switched to \"%{profile}\"", profile = p), None, None)
                }
                Notice::Bank(b) => self.toast(icon::STACK, t!("Bank %{bank}", bank = b + 1), None, None),
                Notice::Info(t) => self.toast(icon::INFO, t, None, None),
                Notice::Problem(t) => self.problem(t),
                Notice::SceneRecalled(name) => self.toast(
                    icon::BOOKMARKS_SIMPLE,
                    t!("Scene \"%{scene}\" recalled", scene = name),
                    None,
                    Some(ToastUndo::Scene),
                ),
            }
        }
    }

    fn draw_toasts(&mut self, ctx: &egui::Context) {
        let life = |t: &Toast| Duration::from_secs(if t.undo.is_some() { 7 } else { 4 });
        self.toasts.retain(|t| t.sticky || t.born.elapsed() < life(t));
        if self.toasts.is_empty() {
            return;
        }
        let mut undo = None;
        let mut close = None;
        let mut copy = None;
        // Repaint only for fading, and when the next toast is due to go.
        let mut next_frame = Duration::MAX;
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(egui::Align2::RIGHT_BOTTOM, vec2(-16.0, -16.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = SP_S;
                for (i, t) in self.toasts.iter().enumerate() {
                    let age = t.born.elapsed().as_secs_f32();
                    let left = if t.sticky { f32::INFINITY } else { life(t).as_secs_f32() - age };
                    // Fade in quickly, fade out over the last 0.3 s.
                    let fade = (age / 0.15).min(1.0).min(left / 0.3).clamp(0.0, 1.0);
                    let fade = if theme::reduce_motion() { 1.0 } else { fade };
                    if age < 0.15 || left < 0.3 {
                        next_frame = next_frame.min(Duration::from_millis(16));
                    } else if left.is_finite() {
                        next_frame = next_frame.min(Duration::from_secs_f32(left - 0.3));
                    }
                    ui.scope(|ui| {
                        ui.multiply_opacity(fade);
                        egui::Frame::popup(ui.style()).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
                            ui.set_max_width(380.0);
                            ui.horizontal(|ui| {
                                let color = t.color.unwrap_or(pal().accent);
                                ui.label(RichText::new(t.icon).size(18.0).color(color));
                                // Leave room for the buttons; long messages wrap.
                                let buttons = if t.undo.is_some() || t.sticky { 90.0 } else { 34.0 };
                                let room = (ui.available_width() - buttons).max(120.0);
                                ui.scope(|ui| {
                                    ui.set_max_width(room);
                                    ui.add(egui::Label::new(&t.text).wrap());
                                });
                                if let Some(u) = t.undo
                                    && ui.button(RichText::new(t!("Undo")).color(pal().accent)).clicked()
                                {
                                    undo = Some((i, u));
                                }
                                if t.sticky && widgets::icon_framed(ui, icon::COPY, &t!("Copy the message")).clicked() {
                                    copy = Some(t.text.clone());
                                }
                                if widgets::icon_framed(ui, icon::X, &t!("Dismiss")).clicked() {
                                    close = Some(i);
                                }
                            });
                        });
                    });
                }
            });
        if let Some(text) = copy {
            ctx.copy_text(text);
        }
        if let Some((i, u)) = undo {
            self.toasts.remove(i);
            match u {
                ToastUndo::Edit => self.undo(false),
                ToastUndo::Scene => self.send(UiCmd::UndoScene),
            }
        } else if let Some(i) = close {
            self.toasts.remove(i);
        }
        if next_frame < Duration::MAX {
            ctx.request_repaint_after(next_frame);
        }
    }

    /// Keyboard shortcuts that work anywhere in the window.
    fn shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let typing = ctx.egui_wants_keyboard_input();
        let pressed = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)));
        if !typing {
            if pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || pressed(Modifiers::COMMAND, Key::Y) {
                self.undo(true);
            } else if pressed(Modifiers::COMMAND, Key::Z) {
                self.undo(false);
            }
        }
        for (k, tab) in [
            (Key::Num1, Tab::Mixer),
            (Key::Num2, Tab::Controller),
            (Key::Num3, Tab::Buttons),
            (Key::Num4, Tab::Profiles),
            (Key::Num5, Tab::Settings),
            (Key::Num6, Tab::Log),
        ] {
            if pressed(Modifiers::COMMAND, k) {
                self.tab = tab;
            }
        }
        if pressed(Modifiers::COMMAND, Key::F) {
            self.tab = Tab::Mixer;
            self.show_sources = true;
            self.want_filter = true;
        }
        if pressed(Modifiers::NONE, Key::F1) {
            self.show_shortcuts = !self.show_shortcuts;
        }
        if pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::M) {
            self.send(UiCmd::ToggleMicMute);
        }
        if typing || self.tab != Tab::Mixer {
            return;
        }
        for (k, step) in [(Key::PageUp, -1), (Key::PageDown, 1)] {
            if pressed(Modifiers::NONE, k) {
                let bank = self.shared.lock().unwrap().active().map(|p| (p.bank, p.bank_count()));
                if let Some((bank, count)) = bank.filter(|(_, n)| *n > 1) {
                    self.send(UiCmd::SetBank((bank as i32 + step).rem_euclid(count as i32) as usize));
                }
            }
        }
        // Mixer: number keys pick a channel, arrows move its fader, M mutes it.
        let digits =
            [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];
        for (n, k) in digits.iter().enumerate() {
            if pressed(Modifiers::NONE, *k) {
                let n = n as u32 + 1;
                self.selected_channel = if self.selected_channel == Some(n) { None } else { Some(n) };
            }
        }
        if pressed(Modifiers::NONE, Key::Escape) {
            self.selected_channel = None;
        }
        let Some(n) = self.selected_channel else { return };
        let strip = {
            let sh = self.shared.lock().unwrap();
            sh.strips.iter().find(|s| !s.knob && s.name.rsplit(' ').next() == Some(&n.to_string())).cloned()
        };
        let Some(s) = strip else { return };
        let step = if ctx.input(|i| i.modifiers.shift) { 0.01 } else { 0.05 };
        for (k, d) in [(Key::ArrowUp, step), (Key::ArrowDown, -step)] {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, k) || i.consume_key(Modifiers::SHIFT, k)) {
                self.send(UiCmd::SetGroupPosition(s.id.clone(), (s.position + d).clamp(0.0, 1.0)));
            }
        }
        if pressed(Modifiers::NONE, Key::M) {
            self.send(UiCmd::ToggleGroupMute(s.id.clone()));
        }
    }

    fn shortcuts_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_shortcuts;
        egui::Window::new(t!("Keyboard shortcuts"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .frame(egui::Frame::window(&ctx.global_style()).inner_margin(18))
            .show(ctx, |ui| {
                egui::Grid::new("keys").num_columns(2).spacing([24.0, 8.0]).show(ui, |ui| {
                    for (keys, what) in [
                        ("Ctrl+1 … Ctrl+6", t!("Switch pages")),
                        ("1 … 9", t!("Pick a channel strip (Mixer)")),
                        ("↑ / ↓", t!("Move its fader 5% (Shift: 1%)")),
                        ("M", t!("Mute it")),
                        ("Esc", t!("Let go of the strip")),
                        ("PgUp / PgDn", t!("Previous or next bank (Mixer)")),
                        ("Ctrl+Shift+M", t!("Mute or unmute the microphone")),
                        ("Ctrl+F", t!("Filter the sources")),
                        ("Ctrl+Z", t!("Undo a change to assignments or the profile")),
                        ("Ctrl+Shift+Z", t!("Redo")),
                        ("F1", t!("This list")),
                    ] {
                        ui.label(RichText::new(keys).font(mono(12.5)).color(pal().accent));
                        ui.label(what);
                        ui.end_row();
                    }
                });
            });
        self.show_shortcuts = open;
    }

    fn pick_file(&self, purpose: PickPurpose) {
        let slot = self.picked.clone();
        let ctx = self.shared.lock().unwrap().egui.clone();
        std::thread::spawn(move || {
            let path = match &purpose {
                PickPurpose::Import => crate::portal::pick_json(&t!("Import a profile"), None),
                PickPurpose::Export(_, full) => crate::portal::pick_json(
                    &t!("Export the profile"),
                    Some(if *full { "slidewire-profile.json" } else { "spec.json" }),
                ),
            };
            if let Some(path) = path {
                *slot.lock().unwrap() = Some((purpose, path));
                if let Some(c) = ctx {
                    c.request_repaint();
                }
            }
        });
    }

    fn handle_window_cmds(&mut self, ctx: &egui::Context) {
        if let Ok(cmd) = self.window.try_recv() {
            match cmd {
                WindowCmd::Quit => {
                    crate::QUIT.store(true, std::sync::atomic::Ordering::SeqCst);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                WindowCmd::Show => ctx.send_viewport_cmd(egui::ViewportCommand::Focus),
            }
        }
        let picked = self.picked.lock().unwrap().take();
        if let Some((purpose, path)) = picked {
            match purpose {
                PickPurpose::Import => self.import_path = path.display().to_string(),
                PickPurpose::Export(id, full) => {
                    let res = self
                        .shared
                        .lock()
                        .unwrap()
                        .store
                        .profiles
                        .get(&id)
                        .map(|p| if full { p.to_full_json() } else { p.to_spec_json() });
                    match res {
                        Some(Ok(json)) => match std::fs::write(&path, json) {
                            Ok(()) => self.flash(t!("Exported to %{path}", path = path.display())),
                            Err(e) => self.problem(t!("Export failed: %{error}", error = e)),
                        },
                        _ => self.flash(t!("Nothing to export")),
                    }
                }
            }
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let (profile, connected, focus, last_midi, pulse) = {
            let sh = self.shared.lock().unwrap();
            (
                sh.active().map(|p| p.name.clone()),
                sh.connected_in.clone(),
                sh.focus.clone(),
                sh.last_midi,
                sh.audio.connected,
            )
        };
        // Narrow windows: tabs show only their icon, and less goes on the right.
        let width = ui.available_width();
        let icons_only = width < 980.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = if icons_only { 4.0 } else { 10.0 };
            if let Some(logo) = &self.logo {
                ui.add(egui::Image::new(logo).fit_to_exact_size(Vec2::splat(30.0)));
            }
            if width > 760.0 {
                ui.label(RichText::new("Slidewire").font(semibold(18.0)));
            }
            ui.add_space(if icons_only { 6.0 } else { 14.0 });

            let mut underline = None;
            for (t, ic, label) in [
                (Tab::Mixer, icon::FADERS, t!("Mixer")),
                (Tab::Controller, icon::SLIDERS_HORIZONTAL, t!("Controller")),
                (Tab::Buttons, icon::CURSOR_CLICK, t!("Buttons")),
                (Tab::Profiles, icon::FOLDER_SIMPLE, t!("Profiles")),
                (Tab::Settings, icon::GEAR_SIX, t!("Settings")),
                (Tab::Log, icon::LIST_BULLETS, t!("Log")),
            ] {
                let r = tab_button(ui, ic, &label, self.tab == t, icons_only);
                if r.clicked() {
                    self.tab = t;
                }
                if self.tab == t {
                    underline = Some(r.rect);
                }
            }
            // The underline slides to the selected tab.
            if let Some(rect) = underline {
                let id = egui::Id::new("tab-underline");
                let l = theme::anim(ui.ctx(), id.with("l"), rect.left() + 8.0, 0.16);
                let r = theme::anim(ui.ctx(), id.with("r"), rect.right() - 8.0, 0.16);
                let bar = egui::Rect::from_min_max(pos2(l, rect.bottom() - 2.0), pos2(r, rect.bottom()));
                widgets::glow(ui.painter(), bar, 1.0, pal().accent.gamma_multiply(0.5), 6.0);
                ui.painter().rect_filled(bar, 1.0, pal().accent);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // MIDI activity blinks the device LED amber.
                let blink = last_midi.is_some_and(|t| t.elapsed() < Duration::from_millis(120));
                if blink {
                    ui.ctx().request_repaint_after(Duration::from_millis(130));
                }
                let (led, text) = match (&connected, &profile) {
                    (Some(port), Some(_)) if icons_only => {
                        (if blink { ASSIGN } else { RUN }, crate::midi::display_name(port))
                    }
                    (Some(port), Some(p)) => {
                        (if blink { ASSIGN } else { RUN }, format!("{p} · {}", crate::midi::display_name(port)))
                    }
                    (None, Some(_)) if icons_only => (MUTE, t!("Not connected").to_string()),
                    (None, Some(p)) => (MUTE, t!("%{profile} · not connected", profile = p).to_string()),
                    _ => (pal().text_faint, t!("No profile").to_string()),
                };
                let pill = widgets::pill(ui, Some(led), &text)
                    .interact(Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(t!("Connection check"));
                if pill.clicked() {
                    self.show_doctor = !self.show_doctor;
                    self.doctor = None;
                }
                if !pulse {
                    widgets::pill(ui, Some(MUTE), &t!("Audio server offline"));
                }
                if !focus.class.is_empty() && width > 1200.0 {
                    widgets::pill(ui, None, &format!("{}  {}", icon::CROSSHAIR, focus.class))
                        .on_hover_text(t!("Focused app"));
                }
            });
        });
    }
}

impl App {
    /// The connection check, opened from the status pill.
    fn doctor_window(&mut self, ctx: &egui::Context) {
        if !self.show_doctor {
            return;
        }
        if self.doctor.as_ref().is_none_or(|(at, _)| at.elapsed() > Duration::from_secs(2)) {
            let sh = self.shared.lock().unwrap();
            let pattern = sh.active().map(|p| p.input.clone()).unwrap_or_default();
            let mut checks = crate::doctor::diagnose(&pattern, &sh.ports_in);
            checks.push(crate::doctor::connection(sh.connected_in.as_deref(), sh.midi_error.as_deref()));
            self.doctor = Some((Instant::now(), checks));
        }
        let checks = self.doctor.as_ref().map(|(_, c)| c.clone()).unwrap_or_default();
        let mut open = true;
        egui::Window::new(format!("{}  {}", icon::STETHOSCOPE, t!("Connection check")))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .anchor(egui::Align2::RIGHT_TOP, vec2(-16.0, 64.0))
            .frame(egui::Frame::window(&ctx.global_style()).inner_margin(16))
            .show(ctx, |ui| {
                ui.set_width(460.0);
                for c in &checks {
                    let (ic, color) = match c.status {
                        crate::doctor::Status::Ok => (icon::CHECK_CIRCLE, RUN),
                        crate::doctor::Status::Warn => (icon::WARNING, ASSIGN),
                        crate::doctor::Status::Fail => (icon::X_CIRCLE, MUTE),
                    };
                    ui.horizontal_top(|ui| {
                        ui.label(RichText::new(ic).size(18.0).color(color));
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&c.title).font(semibold(14.0)));
                            ui.add(egui::Label::new(RichText::new(&c.detail).color(pal().text_dim).size(12.5)).wrap());
                        });
                    });
                    ui.add_space(4.0);
                }
                widgets::hint(ui, &t!("Updates every 2 seconds. Same checks from a terminal: slidewire --doctor"));
            });
        ctx.request_repaint_after(Duration::from_secs(1));
        self.show_doctor = open;
    }
}

/// Page switcher button: icon + label, tinted when selected.
/// A page tab; `icon_only` on narrow windows (the label becomes a tooltip).
fn tab_button(ui: &mut egui::Ui, ic: &str, label: &str, selected: bool, icon_only: bool) -> egui::Response {
    let text = if icon_only { ic.to_string() } else { format!("{ic}  {label}") };
    let galley =
        ui.painter().layout_no_wrap(text, egui::FontId::proportional(if icon_only { 17.0 } else { 14.0 }), pal().text);
    let size = galley.size() + vec2(20.0, 14.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let name = label.to_string();
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, &name));
    let hot = response.hovered();
    if selected {
        ui.painter().rect_filled(rect, 6.0, theme::mix(pal().panel_deep, pal().accent, 0.12));
    } else if hot {
        ui.painter().rect_filled(rect, 6.0, pal().control);
    }
    if response.has_focus() {
        ui.painter().rect_stroke(rect, 6.0, Stroke::new(2.0, pal().accent), egui::StrokeKind::Inside);
    }
    let color = if selected {
        pal().accent
    } else if hot {
        pal().text_strong
    } else {
        pal().text_dim
    };
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if icon_only { response.on_hover_text(label) } else { response }
}

impl App {
    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(c) = &self.confirm else { return };
        let mut answer = None;
        let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(20).corner_radius(12);
        let modal = egui::Modal::new(egui::Id::new("confirm")).frame(frame).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.label(RichText::new(&c.title).font(semibold(16.0)));
            ui.add_space(4.0);
            ui.label(&c.body);
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let danger =
                    egui::Button::new(RichText::new(&c.button).color(Color32::WHITE)).fill(MUTE.gamma_multiply(0.85));
                if ui.add(danger).clicked() {
                    answer = Some(1);
                }
                if let Some((label, _)) = &c.alt
                    && ui.add(egui::Button::new(RichText::new(label).strong()).fill(pal().accent_bg)).clicked()
                {
                    answer = Some(2);
                }
                if ui.button(t!("Cancel")).clicked() {
                    answer = Some(0);
                }
            });
        });
        if modal.should_close() && answer.is_none() {
            answer = Some(0);
        }
        let Some(answer) = answer else { return };
        let c = self.confirm.take().unwrap();
        let action = match answer {
            1 => c.action,
            2 => match c.alt {
                Some((_, a)) => a,
                None => return,
            },
            _ => return,
        };
        match action {
            ConfirmAction::DeleteProfile(id) => {
                let mut sh = self.shared.lock().unwrap();
                sh.store.delete_profile(&id);
                sh.store.touch_settings();
                drop(sh);
                self.send(UiCmd::ProfileEdited);
            }
            ConfirmAction::RemoveBank(i) => self.edit_profile(|p| p.remove_bank(i)),
            ConfirmAction::SaveScene(name) => self.save_scene(&name),
            ConfirmAction::Import { path, trusted } => self.import_profile(&path, trusted),
        }
    }

    /// Add a profile from a file and switch to it.
    pub(super) fn import_profile(&mut self, path: &str, trusted: bool) {
        let res = self.shared.lock().unwrap().store.import(std::path::Path::new(path), trusted);
        match res {
            Ok(id) => {
                self.flash(t!("Profile imported"));
                self.send(UiCmd::ActivateProfile(id));
                self.import_path.clear();
            }
            Err(e) => self.problem(t!("Import failed: %{error}", error = format!("{e:#}"))),
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if let Ok(mut sh) = self.shared.lock() {
            sh.ui_visible = false;
            sh.egui = None;
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_window_cmds(&ctx);
        let settings = self.shared.lock().unwrap().store.settings.clone();
        theme::sync(&ctx, &settings);
        self.sync_zoom(&ctx, settings.ui_scale);
        self.sync_language(&ctx, &settings.language);
        self.remember_geometry(&ctx);
        self.shortcuts(&ctx);

        egui::Panel::top("top")
            .frame(
                egui::Frame::new()
                    .fill(pal().panel_deep)
                    .inner_margin(egui::Margin::symmetric(14, 9))
                    .stroke(Stroke::new(1.0, pal().border)),
            )
            .show(ui, |ui| self.top_bar(ui));

        self.learn_bar(ui);

        if self.tab == Tab::Mixer && self.show_sources {
            egui::Panel::right("sources")
                .resizable(true)
                .default_size(340.0)
                .size_range(280.0..=520.0)
                .frame(egui::Frame::new().fill(pal().panel_deep).inner_margin(egui::Margin::same(12)))
                .show(ui, |ui| self.sources(ui));
        }

        // A new page fades in quickly.
        if self.tab != self.shown_tab {
            self.shown_tab = self.tab;
            self.tab_since = Instant::now();
        }
        let fade = if theme::reduce_motion() { 1.0 } else { (self.tab_since.elapsed().as_secs_f32() / 0.12).min(1.0) };
        if fade < 1.0 {
            ctx.request_repaint();
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(pal().panel).inner_margin(egui::Margin::same(16)))
            .show(ui, |ui| {
                ui.multiply_opacity(egui::emath::easing::cubic_out(fade));
                self.page(ui)
            });
        self.doctor_window(&ctx);
        self.confirm_dialog(&ctx);
        self.shortcuts_window(&ctx);
        self.welcome_window(&ctx);
        self.take_notices();
        self.draw_toasts(&ctx);

        // Everything that moves asks for its own frames (engine events, meter
        // peaks, blinks); this slow floor only catches anything that didn't.
        ctx.request_repaint_after(Duration::from_secs(2));
    }
}
