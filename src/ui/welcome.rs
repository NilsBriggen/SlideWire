//! First-run welcome: connect the controller, check it talks to us, then
//! assign something. Shown once, when there were no settings yet.

use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, vec2};
use rust_i18n::t;

use super::theme::{icon, *};
use super::{App, widgets};

pub(super) struct Welcome {
    step: usize,
    /// When the current step started (step 2 waits for MIDI after this).
    since: Instant,
}

impl Welcome {
    pub fn new() -> Welcome {
        Welcome { step: 0, since: Instant::now() }
    }
}

const STEPS: usize = 3;

impl App {
    pub(super) fn welcome_window(&mut self, ctx: &egui::Context) {
        let Some(w) = &self.welcome else { return };
        let (step, since) = (w.step, w.since);
        if step + 1 == STEPS {
            self.welcome_coach(ctx);
            return;
        }
        let (connected, ports, last_msg, profile_input, touched) = {
            let sh = self.shared.lock().unwrap();
            // The control that moved most recently, by name.
            let touched = sh
                .activity
                .iter()
                .filter(|(_, (t, _))| *t > since)
                .max_by_key(|(_, (t, _))| *t)
                .and_then(|(id, _)| sh.active()?.controls.get(id).map(|c| c.name.clone()));
            (
                sh.connected_in.clone(),
                sh.ports_in.clone(),
                sh.last_msg.clone(),
                sh.active().map(|p| p.input.clone()).unwrap_or_default(),
                touched,
            )
        };
        let mut go: Option<isize> = None;
        let mut close = false;
        let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(24).corner_radius(14);
        egui::Modal::new(egui::Id::new("welcome")).frame(frame).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.horizontal(|ui| {
                if let Some(logo) = &self.logo {
                    ui.add(egui::Image::new(logo).fit_to_exact_size(vec2(36.0, 36.0)));
                }
                ui.vertical(|ui| {
                    ui.label(RichText::new(t!("Welcome to Slidewire")).font(semibold(20.0)));
                    widgets::hint(ui, &t!("Three quick steps and your controller runs your volumes."));
                });
            });
            ui.add_space(SP_S * 2.0);
            match step {
                0 => {
                    step_title(ui, 1, &t!("Connect your controller"));
                    let nano = |port: &str| port.to_lowercase().contains("nanokontrol2");
                    if let Some(port) = &connected {
                        ok_line(ui, &t!("%{device} is connected.", device = crate::midi::display_name(port)));
                    } else if ports.is_empty() {
                        widgets::hint(ui, &t!("No MIDI devices found. Plug yours in; it shows up here right away."));
                        if ui.button(format!("{}  {}", icon::STETHOSCOPE, t!("Why isn't it showing up?"))).clicked() {
                            self.show_doctor = true;
                        }
                    } else {
                        widgets::hint(ui, &t!("Pick it from the list:"));
                        for port in ports.iter().filter(|p| !p.contains("Through")) {
                            let name = crate::midi::display_name(port);
                            let pattern = name.trim_end_matches(" (raw)").to_string();
                            let on = !profile_input.is_empty() && pattern.contains(&profile_input);
                            if ui.selectable_label(on, format!("{}  {name}", icon::PIANO_KEYS)).clicked() {
                                if nano(port) {
                                    self.edit_profile(|p| {
                                        p.input = pattern.clone();
                                        p.output = pattern.clone();
                                    });
                                } else {
                                    self.welcome_other = Some(pattern.clone());
                                }
                            }
                        }
                    }
                    // Not a nanoKONTROL2: its layout has to come from somewhere.
                    if let Some(other) = self.welcome_other.clone() {
                        ui.add_space(SP_S);
                        egui::Frame::new().fill(pal().inset).corner_radius(8).inner_margin(10).show(ui, |ui| {
                            ui.label(RichText::new(t!("%{device} isn't a nanoKONTROL2", device = other)).strong());
                            widgets::hint(
                                ui,
                                &t!("MIDI Mixer for Windows has presets for many controllers; import one, or map your controls by moving them."),
                            );
                            ui.horizontal(|ui| {
                                if ui.button(format!("{}  {}", icon::DOWNLOAD_SIMPLE, t!("Import a preset"))).clicked() {
                                    self.tab = super::Tab::Profiles;
                                    close = true;
                                }
                                if ui.button(format!("{}  {}", icon::CROSSHAIR, t!("Map my controller"))).clicked() {
                                    let id = crate::profile::new_id();
                                    let p = crate::profile::Profile {
                                        name: other.clone(),
                                        input: other.clone(),
                                        output: other.clone(),
                                        ..Default::default()
                                    };
                                    let mut sh = self.shared.lock().unwrap();
                                    sh.store.profiles.insert(id.clone(), p);
                                    sh.store.touch(&id);
                                    drop(sh);
                                    self.send(super::UiCmd::ActivateProfile(id));
                                    self.tab = super::Tab::Controller;
                                    close = true;
                                }
                            });
                        });
                    }
                }
                _ => {
                    step_title(ui, 2, &t!("Move a fader or knob"));
                    match (touched, last_msg.filter(|(_, t)| *t > since)) {
                        (Some(name), _) => ok_line(ui, &t!("Got it: %{control}", control = name)),
                        (None, Some((msg, _))) => {
                            ok_line(ui, &t!("Got a message (%{message})", message = msg));
                            widgets::hint(ui, &t!("It isn't mapped in this profile yet; map it on the Controller page."));
                        }
                        (None, None) => {
                            let t = ui.input(|i| i.time) as f32;
                            let c = pal().text_dim.gamma_multiply(0.6 + 0.4 * (t * 3.0).sin().abs());
                            ui.label(
                                RichText::new(format!("{}  {}", icon::WAVEFORM, t!("Waiting for your controller…"))).color(c),
                            );
                            ui.ctx().request_repaint_after(Duration::from_millis(50));
                        }
                    }
                }
            }
            ui.add_space(SP_S * 2.0);
            ui.horizontal(|ui| {
                progress_dots(ui, step);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(RichText::new(t!("Next")).strong()).fill(pal().accent_bg)).clicked() {
                        go = Some(1);
                    }
                    if step > 0 && ui.button(t!("Back")).clicked() {
                        go = Some(-1);
                    }
                    if ui.link(t!("Skip")).clicked() {
                        close = true;
                    }
                });
            });
        });
        if close {
            self.welcome = None;
        } else if let Some(d) = go
            && let Some(w) = &mut self.welcome
        {
            w.step = w.step.saturating_add_signed(d);
            w.since = Instant::now();
            if w.step + 1 == STEPS {
                // The last step points at the mixer, so it must be visible.
                self.tab = super::Tab::Mixer;
                self.show_sources = true;
            }
        }
    }

    /// Last step: a small card that leaves the mixer usable, so the first
    /// app can really be dragged onto a strip. Done once something is assigned.
    fn welcome_coach(&mut self, ctx: &egui::Context) {
        let assigned = {
            let sh = self.shared.lock().unwrap();
            sh.strips.iter().find(|s| s.assigned).map(|s| {
                let what = s.resolved.first().cloned().or_else(|| s.targets.first().map(|t| t.label()));
                (s.label.clone().unwrap_or_else(|| s.name.clone()), what.unwrap_or_default())
            })
        };
        let mut close = false;
        let mut back = false;
        egui::Window::new("welcome-coach")
            .title_bar(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_BOTTOM, vec2(0.0, -24.0))
            .frame(egui::Frame::popup(&ctx.global_style()).inner_margin(18).corner_radius(14))
            .show(ctx, |ui| {
                ui.set_width(420.0);
                step_title(ui, 3, &t!("Assign your first app"));
                match &assigned {
                    Some((strip, what)) => {
                        ok_line(ui, &t!("%{strip} now controls %{target}.", strip = strip, target = what));
                        widgets::hint(ui, &t!("Move that fader on your controller and listen."));
                    }
                    None => {
                        ui.label(t!(
                            "Drag an app from Sources (on the right) onto a strip, or click a strip's S button."
                        ));
                    }
                }
                ui.add_space(4.0);
                for (k, what) in [
                    ("S", t!("lists what a strip controls")),
                    ("M", t!("mutes it")),
                    ("R", t!("makes a device the default, or starts the app")),
                ] {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(k).font(mono(13.0)).color(pal().accent));
                        ui.label(what);
                    });
                }
                widgets::hint(ui, &t!("Press F1 any time for keyboard shortcuts."));
                ui.add_space(SP_S);
                ui.horizontal(|ui| {
                    progress_dots(ui, STEPS - 1);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = if assigned.is_some() { t!("Start mixing") } else { t!("Later") };
                        if ui.add(egui::Button::new(RichText::new(label).strong()).fill(pal().accent_bg)).clicked() {
                            close = true;
                        }
                        if ui.button(t!("Back")).clicked() {
                            back = true;
                        }
                    });
                });
            });
        if close {
            self.welcome = None;
        } else if back && let Some(w) = &mut self.welcome {
            w.step -= 1;
            w.since = Instant::now();
        }
    }
}

fn progress_dots(ui: &mut egui::Ui, step: usize) {
    for i in 0..STEPS {
        let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
        let c = if i == step { pal().accent } else { pal().border_strong };
        ui.painter().circle_filled(r.center(), 4.0, c);
    }
}

fn step_title(ui: &mut egui::Ui, n: usize, text: &str) {
    ui.label(RichText::new(format!("{n}. {text}")).font(semibold(15.0)));
    ui.add_space(2.0);
}

fn ok_line(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(format!("{}  {text}", icon::CHECK_CIRCLE)).color(pal().readout));
}
