//! The mixer / settings window (egui), styled as a hardware console.

mod controller;
mod diagram;
mod icons;
mod mixer;
mod pages;
mod theme;
mod widgets;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender};
use eframe::egui::{self, Color32, RichText, Sense, Stroke, Vec2, pos2, vec2};

use crate::WindowCmd;
use crate::engine::{Event, SharedRef, UiCmd};
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
    /// Export the profile with this id.
    Export(String),
}

/// Something destructive waiting for a yes.
pub(super) struct Confirm {
    pub title: String,
    pub body: String,
    pub button: String,
    pub action: ConfirmAction,
}

pub(super) enum ConfirmAction {
    DeleteProfile(String),
    RemoveBank(usize),
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
    status: Option<(String, Instant)>,
    expanded_control: Option<String>,
    control_search: String,
    scene_name: String,
    source_filter: String,
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
        theme::install(&cc.egui_ctx);
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
            status: None,
            expanded_control: None,
            control_search: String::new(),
            scene_name: String::new(),
            rename: None,
            source_filter: String::new(),
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
        self.status = Some((msg.into(), Instant::now()));
    }

    /// Run `f` on the active profile, then let the engine save and refresh.
    fn edit_profile(&self, f: impl FnOnce(&mut Profile)) {
        let mut sh = self.shared.lock().unwrap();
        if let Some(id) = sh.active_id()
            && let Some(p) = sh.store.profiles.get_mut(&id)
        {
            f(p);
        }
        drop(sh);
        self.send(UiCmd::ProfileEdited);
    }

    fn pick_file(&self, purpose: PickPurpose) {
        let slot = self.picked.clone();
        let ctx = self.shared.lock().unwrap().egui.clone();
        std::thread::spawn(move || {
            let path = match &purpose {
                PickPurpose::Import => crate::portal::pick_json("Import a profile", None),
                PickPurpose::Export(_) => crate::portal::pick_json("Export the profile", Some("profile.json")),
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
                PickPurpose::Export(id) => {
                    let res = self.shared.lock().unwrap().store.profiles.get(&id).map(|p| p.to_spec_json());
                    match res {
                        Some(Ok(json)) => match std::fs::write(&path, json) {
                            Ok(()) => self.flash(format!("Exported to {}", path.display())),
                            Err(e) => self.flash(format!("Export failed: {e}")),
                        },
                        _ => self.flash("Nothing to export"),
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
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            if let Some(logo) = &self.logo {
                ui.add(egui::Image::new(logo).fit_to_exact_size(Vec2::splat(30.0)));
            }
            ui.label(RichText::new("Slidewire").font(semibold(18.0)));
            ui.add_space(14.0);

            let mut underline = None;
            for (t, ic, label) in [
                (Tab::Mixer, icon::FADERS, "Mixer"),
                (Tab::Controller, icon::SLIDERS_HORIZONTAL, "Controller"),
                (Tab::Buttons, icon::CURSOR_CLICK, "Buttons"),
                (Tab::Profiles, icon::FOLDER_SIMPLE, "Profiles"),
                (Tab::Settings, icon::GEAR_SIX, "Settings"),
                (Tab::Log, icon::LIST_BULLETS, "Log"),
            ] {
                let r = tab_button(ui, ic, label, self.tab == t);
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
                    (Some(port), Some(p)) => {
                        (if blink { ASSIGN } else { RUN }, format!("{p} · {}", crate::midi::display_name(port)))
                    }
                    (None, Some(p)) => (MUTE, format!("{p} · not connected")),
                    _ => (pal().text_faint, "No profile".into()),
                };
                let pill = widgets::pill(ui, Some(led), &text)
                    .interact(Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("Connection check");
                if pill.clicked() {
                    self.show_doctor = !self.show_doctor;
                    self.doctor = None;
                }
                if !pulse {
                    widgets::pill(ui, Some(MUTE), "Audio server offline");
                }
                if !focus.class.is_empty() {
                    widgets::pill(ui, None, &format!("{}  {}", icon::CROSSHAIR, focus.class))
                        .on_hover_text("Focused app");
                }
                if let Some((s, at)) = &self.status
                    && at.elapsed() < Duration::from_secs(6)
                {
                    ui.label(RichText::new(s).color(pal().accent));
                    ui.ctx().request_repaint_after(Duration::from_secs(1));
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
        egui::Window::new(format!("{}  Connection check", icon::STETHOSCOPE))
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
                widgets::hint(ui, "Updates every 2 seconds. Same checks from a terminal: slidewire --doctor");
            });
        ctx.request_repaint_after(Duration::from_secs(1));
        self.show_doctor = open;
    }
}

/// Page switcher button: icon + label, tinted when selected.
fn tab_button(ui: &mut egui::Ui, ic: &str, label: &str, selected: bool) -> egui::Response {
    let text = format!("{ic}  {label}");
    let galley = ui.painter().layout_no_wrap(text, egui::FontId::proportional(14.0), pal().text);
    let size = galley.size() + vec2(20.0, 14.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hot = response.hovered();
    if selected {
        ui.painter().rect_filled(rect, 6.0, theme::mix(pal().panel_deep, pal().accent, 0.12));
    } else if hot {
        ui.painter().rect_filled(rect, 6.0, pal().control);
    }
    let color = if selected {
        pal().accent
    } else if hot {
        pal().text_strong
    } else {
        pal().text_dim
    };
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
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
                    answer = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(false);
                }
            });
        });
        if modal.should_close() && answer.is_none() {
            answer = Some(false);
        }
        match answer {
            Some(true) => {
                let action = self.confirm.take().unwrap().action;
                match action {
                    ConfirmAction::DeleteProfile(id) => {
                        let mut sh = self.shared.lock().unwrap();
                        sh.store.delete_profile(&id);
                        let _ = sh.store.save_settings();
                        drop(sh);
                        self.send(UiCmd::ProfileEdited);
                    }
                    ConfirmAction::RemoveBank(i) => self.edit_profile(|p| p.remove_bank(i)),
                }
            }
            Some(false) => self.confirm = None,
            None => {}
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

        egui::Panel::top("top")
            .frame(
                egui::Frame::new()
                    .fill(pal().panel_deep)
                    .inner_margin(egui::Margin::symmetric(14, 9))
                    .stroke(Stroke::new(1.0, pal().border)),
            )
            .show(ui, |ui| self.top_bar(ui));

        if self.tab == Tab::Mixer && self.show_sources {
            egui::Panel::right("sources")
                .resizable(true)
                .default_size(340.0)
                .size_range(280.0..=520.0)
                .frame(egui::Frame::new().fill(pal().panel_deep).inner_margin(egui::Margin::same(12)))
                .show(ui, |ui| self.sources(ui));
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(pal().panel).inner_margin(egui::Margin::same(16)))
            .show(ui, |ui| match self.tab {
                Tab::Mixer => self.mixer(ui),
                Tab::Controller => self.controller(ui),
                Tab::Buttons => self.buttons(ui),
                Tab::Profiles => self.profiles(ui),
                Tab::Settings => self.settings(ui),
                Tab::Log => self.log(ui),
            });
        self.doctor_window(&ctx);
        self.confirm_dialog(&ctx);

        // Everything that moves asks for its own frames (engine events, meter
        // peaks, blinks); this slow floor only catches anything that didn't.
        ctx.request_repaint_after(Duration::from_secs(2));
    }
}
