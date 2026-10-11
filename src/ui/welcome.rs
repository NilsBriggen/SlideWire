//! First-run welcome: connect the controller, check it talks to us, then
//! assign something. Shown once, when there were no settings yet.

use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, vec2};

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
        let mut next = false;
        let mut close = false;
        let frame = egui::Frame::popup(&ctx.global_style()).inner_margin(24).corner_radius(14);
        egui::Modal::new(egui::Id::new("welcome")).frame(frame).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.horizontal(|ui| {
                if let Some(logo) = &self.logo {
                    ui.add(egui::Image::new(logo).fit_to_exact_size(vec2(36.0, 36.0)));
                }
                ui.vertical(|ui| {
                    ui.label(RichText::new("Welcome to Slidewire").font(semibold(20.0)));
                    widgets::hint(ui, "Three quick steps and your controller runs your volumes.");
                });
            });
            ui.add_space(SP_S * 2.0);
            match step {
                0 => {
                    step_title(ui, 1, "Connect your controller");
                    if let Some(port) = &connected {
                        ok_line(ui, &format!("{} is connected.", crate::midi::display_name(port)));
                    } else if ports.is_empty() {
                        widgets::hint(ui, "No MIDI devices found. Plug yours in; it shows up here right away.");
                        if ui.button(format!("{}  Why isn't it showing up?", icon::STETHOSCOPE)).clicked() {
                            self.show_doctor = true;
                        }
                    } else {
                        widgets::hint(ui, "Pick it from the list:");
                        for port in ports.iter().filter(|p| !p.contains("Through")) {
                            let name = crate::midi::display_name(port);
                            let pattern = name.trim_end_matches(" (raw)").to_string();
                            let on = !profile_input.is_empty() && pattern.contains(&profile_input);
                            if ui.selectable_label(on, format!("{}  {name}", icon::PIANO_KEYS)).clicked() {
                                self.edit_profile(|p| {
                                    p.input = pattern.clone();
                                    p.output = pattern.clone();
                                });
                            }
                        }
                    }
                }
                1 => {
                    step_title(ui, 2, "Move a fader or knob");
                    match (touched, last_msg.filter(|(_, t)| *t > since)) {
                        (Some(name), _) => ok_line(ui, &format!("Got it: {name}")),
                        (None, Some((msg, _))) => {
                            ok_line(ui, &format!("Got a message ({msg})"));
                            widgets::hint(ui, "It isn't mapped in this profile yet; map it on the Controller page.");
                        }
                        (None, None) => {
                            let t = ui.input(|i| i.time) as f32;
                            let c = pal().text_dim.gamma_multiply(0.6 + 0.4 * (t * 3.0).sin().abs());
                            ui.label(
                                RichText::new(format!("{}  Waiting for your controller…", icon::WAVEFORM)).color(c),
                            );
                            ui.ctx().request_repaint_after(Duration::from_millis(50));
                        }
                    }
                }
                _ => {
                    step_title(ui, 3, "Assign your first app");
                    ui.label("Drag an app from Sources (on the right) onto a strip, or right-click a strip.");
                    ui.add_space(4.0);
                    for (k, what) in [
                        ("S", "lists what a strip controls"),
                        ("M", "mutes it"),
                        ("R", "makes a device the default, or starts the app"),
                    ] {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(k).font(mono(13.0)).color(pal().accent));
                            ui.label(what);
                        });
                    }
                    widgets::hint(ui, "Press F1 any time for keyboard shortcuts.");
                }
            }
            ui.add_space(SP_S * 2.0);
            ui.horizontal(|ui| {
                // Progress dots.
                for i in 0..STEPS {
                    let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
                    let c = if i == step { pal().accent } else { pal().border_strong };
                    ui.painter().circle_filled(r.center(), 4.0, c);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let last = step + 1 == STEPS;
                    let label = if last { "Start mixing" } else { "Next" };
                    if ui.add(egui::Button::new(RichText::new(label).strong()).fill(pal().accent_bg)).clicked() {
                        if last {
                            close = true;
                        } else {
                            next = true;
                        }
                    }
                    if !last && ui.link("Skip").clicked() {
                        close = true;
                    }
                });
            });
        });
        if close {
            self.welcome = None;
        } else if next && let Some(w) = &mut self.welcome {
            w.step += 1;
            w.since = Instant::now();
        }
    }
}

fn step_title(ui: &mut egui::Ui, n: usize, text: &str) {
    ui.label(RichText::new(format!("{n}. {text}")).font(semibold(15.0)));
    ui.add_space(2.0);
}

fn ok_line(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(format!("{}  {text}", icon::CHECK_CIRCLE)).color(pal().readout));
}
