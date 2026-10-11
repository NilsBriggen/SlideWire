//! Buttons, Profiles, Settings and Log pages.

use eframe::egui::{self, RichText, Stroke};

use super::controller::port_combo;
use super::mixer::target_glyph;
use super::theme::{icon, *};
use super::widgets;
use super::{App, PickPurpose, Tab};
use crate::audio::AudioState;
use crate::config::{self, Settings};
use crate::engine::UiCmd;
use crate::profile::{ButtonAction, MediaKey, Profile, Target};

impl App {
    pub(super) fn buttons(&mut self, ui: &mut egui::Ui) {
        let (profile, audio, profiles, activity) = {
            let sh = self.shared.lock().unwrap();
            let profiles: Vec<(String, String)> =
                sh.store.profiles.iter().map(|(id, p)| (id.clone(), p.name.clone())).collect();
            (sh.active().cloned(), sh.audio.clone(), profiles, sh.activity.clone())
        };
        let Some(mut p) = profile else {
            widgets::card(ui, icon::INFO, "No active profile", |ui| {
                ui.label("Create or import a profile under Profiles.");
            });
            return;
        };
        let before = p.buttons.clone();
        ui.label(RichText::new("Buttons").font(semibold(17.0)));
        widgets::hint(ui, "Buttons that are not part of a group can trigger these actions.");
        ui.add_space(8.0);
        let spare = p.spare_buttons();
        let mut press = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if spare.is_empty() {
                widgets::card(ui, icon::INFO, "No spare buttons", |ui| {
                    ui.label("Every button belongs to a group. Add a button control under Controller to use it here.");
                });
            }
            // Cards in equal-width columns.
            let avail = ui.available_width() - 14.0;
            let cols = ((avail / 440.0).floor() as usize).clamp(1, 3);
            let width = (avail - 12.0 * (cols as f32 - 1.0)) / cols as f32;
            egui::Grid::new("button-cards").num_columns(cols).spacing([12.0, 12.0]).show(ui, |ui| {
                for (i, cid) in spare.iter().enumerate() {
                    let name = p.control_name(cid);
                    let lit = activity.get(cid).is_some_and(|(t, v)| *v > 0 && t.elapsed().as_millis() < 250);
                    if lit {
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
                    }
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        ui.set_max_width(width);
                        let glyph = if lit { icon::RECORD } else { icon::CURSOR_CLICK };
                        widgets::card(ui, glyph, &name, |ui| {
                            if lit {
                                let r = ui.min_rect();
                                ui.painter().rect_stroke(
                                    r.expand(14.0),
                                    10.0,
                                    Stroke::new(1.5, ASSIGN),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            let action = p.buttons.entry(cid.clone()).or_default();
                            action_editor(ui, cid, action, &audio, &profiles);
                            ui.add_space(4.0);
                            if widgets::icon_button(ui, icon::PLAY, "Try it").clicked() {
                                press = Some(cid.clone());
                            }
                        });
                    });
                    if (i + 1) % cols == 0 {
                        ui.end_row();
                    }
                }
            });
        });
        p.buttons.retain(|_, a| *a != ButtonAction::None);
        if p.buttons != before {
            let buttons = p.buttons;
            self.edit_profile(|ap| ap.buttons = buttons);
        }
        if let Some(cid) = press {
            self.send(UiCmd::Press(cid));
        }
    }

    pub(super) fn profiles(&mut self, ui: &mut egui::Ui) {
        let (list, active, ports_in): (Vec<(String, Profile)>, Option<String>, Vec<String>) = {
            let sh = self.shared.lock().unwrap();
            (
                sh.store.profiles.iter().map(|(id, p)| (id.clone(), p.clone())).collect(),
                sh.active_id(),
                sh.ports_in.clone(),
            )
        };
        let mut action: Option<(&str, String)> = None;
        ui.label(RichText::new("Profiles").font(semibold(17.0)));
        widgets::hint(
            ui,
            "One profile per controller. The matching profile is picked automatically when a device is plugged in.",
        );
        ui.add_space(8.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (id, p) in &list {
                let is_active = active.as_deref() == Some(id.as_str());
                let connected = crate::midi::find_port(&ports_in, &p.input).is_some();
                egui::Frame::new()
                    .fill(CARD)
                    .stroke(Stroke::new(
                        if is_active { 1.5 } else { 1.0 },
                        if is_active { CYAN } else { egui::Color32::from_rgb(0x2c, 0x2f, 0x39) },
                    ))
                    .corner_radius(10)
                    .inner_margin(egui::Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let led = if connected { RUN } else { TEXT_FAINT };
                            let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(12.0), egui::Sense::hover());
                            if connected {
                                widgets::glow(
                                    ui.painter(),
                                    egui::Rect::from_center_size(r.center(), egui::Vec2::splat(7.0)),
                                    3.0,
                                    led.gamma_multiply(0.6),
                                    8.0,
                                );
                            }
                            ui.painter().circle_filled(r.center(), 5.0, led);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&p.name).font(semibold(15.0)));
                                    if is_active {
                                        widgets::chip(ui, "ACTIVE", CYAN);
                                    }
                                });
                                let status = if connected { "connected" } else { "not connected" };
                                widgets::hint(
                                    ui,
                                    &format!(
                                        "{} · {status} · {} controls · {} groups",
                                        if p.input.is_empty() { "no port" } else { &p.input },
                                        p.controls.len(),
                                        p.groups.len()
                                    ),
                                );
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button(icon::TRASH).on_hover_text("Delete").clicked() {
                                    action = Some(("delete", id.clone()));
                                }
                                if ui
                                    .button(icon::EXPORT)
                                    .on_hover_text("Export in the MIDI Mixer spec.json format")
                                    .clicked()
                                {
                                    action = Some(("export", id.clone()));
                                }
                                if ui.button(icon::COPY).on_hover_text("Duplicate").clicked() {
                                    action = Some(("duplicate", id.clone()));
                                }
                                if !is_active && widgets::icon_button(ui, icon::CHECK, "Use").clicked() {
                                    action = Some(("activate", id.clone()));
                                }
                            });
                        });
                    });
                ui.add_space(6.0);
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if widgets::icon_button(ui, icon::PLUS, "New empty profile").clicked() {
                    action = Some(("new", String::new()));
                }
                if widgets::icon_button(ui, icon::PLUS, "Korg nanoKONTROL2 default").clicked() {
                    action = Some(("nano", String::new()));
                }
            });
            ui.add_space(12.0);
            widgets::card(ui, icon::DOWNLOAD_SIMPLE, "Import", |ui| {
                widgets::hint(ui, "Profiles from MIDI Mixer for Windows (spec.json) and from this app.");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.import_path)
                            .desired_width(420.0)
                            .hint_text("/path/to/spec.json"),
                    );
                    if widgets::icon_button(ui, icon::FOLDER_OPEN, "Browse…").clicked() {
                        self.pick_file(PickPurpose::Import);
                    }
                    if widgets::icon_button(ui, icon::DOWNLOAD_SIMPLE, "Import").clicked() {
                        action = Some(("import", self.import_path.trim().to_string()));
                    }
                });
            });
        });

        let Some((what, arg)) = action else { return };
        let mut sh = self.shared.lock().unwrap();
        match what {
            "activate" => {
                drop(sh);
                self.send(UiCmd::ActivateProfile(arg));
            }
            "duplicate" => {
                if let Some(mut p) = sh.store.profiles.get(&arg).cloned() {
                    p.name = format!("{} (copy)", p.name);
                    let id = crate::profile::new_id();
                    sh.store.profiles.insert(id.clone(), p);
                    let _ = sh.store.save_profile(&id);
                }
            }
            "export" => {
                sh.store.settings.active_profile = Some(arg.clone());
                drop(sh);
                self.send(UiCmd::ActivateProfile(arg));
                self.pick_file(PickPurpose::Export);
            }
            "delete" => {
                sh.store.delete_profile(&arg);
                let _ = sh.store.save_settings();
                drop(sh);
                self.send(UiCmd::ProfileEdited);
            }
            "new" | "nano" => {
                let p = if what == "nano" {
                    Profile::nanokontrol2()
                } else {
                    Profile { name: "New profile".into(), ..Default::default() }
                };
                let id = crate::profile::new_id();
                sh.store.profiles.insert(id.clone(), p);
                let _ = sh.store.save_profile(&id);
                drop(sh);
                self.send(UiCmd::ActivateProfile(id));
                if what == "new" {
                    self.tab = Tab::Controller;
                }
            }
            "import" => {
                let res = sh.store.import(std::path::Path::new(&arg));
                drop(sh);
                match res {
                    Ok(id) => {
                        self.flash("Profile imported");
                        self.send(UiCmd::ActivateProfile(id));
                        self.import_path.clear();
                    }
                    Err(e) => self.flash(format!("Import failed: {e:#}")),
                }
            }
            _ => {}
        }
    }

    pub(super) fn settings(&mut self, ui: &mut egui::Ui) {
        let (mut s, ports_out): (Settings, Vec<String>) = {
            let sh = self.shared.lock().unwrap();
            (sh.store.settings.clone(), sh.ports_out.clone())
        };
        let before = serde_json::to_string(&s).unwrap_or_default();
        ui.label(RichText::new("Settings").font(semibold(17.0)));
        ui.add_space(8.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.set_max_width(760.0);
            widgets::card(ui, icon::GEAR_SIX, "General", |ui| {
                let mut autostart = config::autostart_enabled();
                if widgets::toggle(ui, &mut autostart, "Start when I log in").changed()
                    && let Err(e) = config::set_autostart(autostart)
                {
                    self.flash(format!("Autostart: {e:#}"));
                }
                widgets::toggle(ui, &mut s.start_minimized, "Start minimised to the tray");
                widgets::toggle(
                    ui,
                    &mut s.auto_switch,
                    "Switch to the matching profile when a controller is plugged in",
                );
                widgets::toggle(ui, &mut s.osd, "Show the Plasma volume pop-up when a fader moves");
            });
            ui.add_space(10.0);
            widgets::card(ui, icon::SPEAKER_HIGH, "Volume", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Curve");
                    ui.spacing_mut().slider_width = 260.0;
                    ui.add(egui::Slider::new(&mut s.curve, 0.5..=3.0).step_by(0.1));
                });
                widgets::hint(
                    ui,
                    "1.0 = same scale as the system volume slider. Higher values give finer control at low volume.",
                );
                ui.horizontal(|ui| {
                    ui.label("Encoder clicks from 0 to 100%");
                    ui.add(egui::DragValue::new(&mut s.default_steps).range(5..=400));
                });
                widgets::toggle(ui, &mut s.allow_boost, "Allow boosting above 100% (up to 150%)");
            });
            ui.add_space(10.0);
            widgets::card(ui, icon::PLUGS_CONNECTED, "MIDI", |ui| {
                widgets::toggle(
                    ui,
                    &mut s.led_heartbeat,
                    "LED heartbeat: resend LED states every 5 s, for devices that forget them",
                );
                ui.horizontal(|ui| {
                    ui.label("Forward all MIDI input to");
                    port_combo(ui, "passthru", &mut s.passthru, &ports_out);
                });
            });
            ui.add_space(10.0);
            widgets::card(ui, icon::INFO, "Korg nanoKONTROL2", |ui| {
                ui.label(
                    "The nanoKONTROL2 only lets the computer control its button LEDs in \"External\" LED mode. \
                     Set LED Mode to External once with the Korg KONTROL Editor (it runs under Wine) and write \
                     it to the device; otherwise the buttons still work but their LEDs only light while pressed.",
                );
            });
        });
        if serde_json::to_string(&s).unwrap_or_default() != before {
            self.shared.lock().unwrap().store.settings = s;
            self.send(UiCmd::ProfileEdited);
        }
    }

    pub(super) fn log(&mut self, ui: &mut egui::Ui) {
        let lines: Vec<String> = self.shared.lock().unwrap().log.iter().cloned().collect();
        ui.label(RichText::new("Log").font(semibold(17.0)));
        ui.add_space(8.0);
        egui::Frame::new()
            .fill(LCD_BG)
            .corner_radius(8)
            .stroke(Stroke::new(1.0, egui::Color32::from_black_alpha(200)))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(ui.available_height());
                egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                    if lines.is_empty() {
                        ui.label(RichText::new("Nothing logged yet.").font(mono(12.5)).color(LCD_DIM));
                    }
                    for l in lines {
                        ui.label(RichText::new(l).font(mono(12.5)).color(LCD_TEXT));
                    }
                });
            });
    }
}

fn action_editor(
    ui: &mut egui::Ui,
    cid: &str,
    action: &mut ButtonAction,
    audio: &AudioState,
    profiles: &[(String, String)],
) {
    let kinds = [
        ButtonAction::None,
        ButtonAction::Media { key: MediaKey::PlayPause },
        ButtonAction::Mute { targets: vec![Target::DefaultOutput] },
        ButtonAction::DefaultOutput { names: vec![] },
        ButtonAction::DefaultInput { names: vec![] },
        ButtonAction::Command { command: String::new() },
        ButtonAction::Keys { keys: String::new() },
        ButtonAction::Profile { id: String::new() },
    ];
    egui::ComboBox::from_id_salt(("action", cid)).selected_text(action.label()).width(240.0).show_ui(ui, |ui| {
        for k in kinds {
            let same = std::mem::discriminant(&k) == std::mem::discriminant(action);
            if ui.selectable_label(same, k.label()).clicked() && !same {
                *action = k;
            }
        }
    });
    match action {
        ButtonAction::None => {}
        ButtonAction::Media { key } => {
            ui.horizontal(|ui| {
                for (k, l) in [
                    (MediaKey::PlayPause, format!("{}  Play/Pause", icon::PLAY_PAUSE)),
                    (MediaKey::Stop, format!("{}  Stop", icon::STOP)),
                    (MediaKey::Previous, format!("{}  Previous", icon::SKIP_BACK)),
                    (MediaKey::Next, format!("{}  Next", icon::SKIP_FORWARD)),
                ] {
                    ui.selectable_value(key, k, l);
                }
            });
        }
        ButtonAction::Mute { targets } => {
            ui.horizontal_wrapped(|ui| {
                let mut remove = None;
                for (i, t) in targets.iter().enumerate() {
                    if ui.button(format!("{}  {}  {}", target_glyph(t), t.label(), icon::X)).clicked() {
                        remove = Some(i);
                    }
                }
                if let Some(i) = remove {
                    targets.remove(i);
                }
                ui.menu_button(icon::PLUS, |ui| {
                    for t in [Target::DefaultOutput, Target::DefaultInput, Target::Focus] {
                        if ui.button(format!("{}  {}", target_glyph(&t), t.label())).clicked() {
                            targets.push(t);
                            ui.close();
                        }
                    }
                    for d in &audio.sinks {
                        if ui.button(format!("{}  {}", icon::SPEAKER_HIGH, d.description)).clicked() {
                            targets.push(Target::Output { name: d.name.clone(), label: d.description.clone() });
                            ui.close();
                        }
                    }
                    for d in &audio.sources {
                        if ui.button(format!("{}  {}", icon::MICROPHONE_STAGE, d.description)).clicked() {
                            targets.push(Target::Input { name: d.name.clone(), label: d.description.clone() });
                            ui.close();
                        }
                    }
                });
            });
        }
        ButtonAction::DefaultOutput { names } => {
            widgets::hint(ui, "Pick one device, or several to cycle through them:");
            device_checklist(ui, names, &audio.sinks);
        }
        ButtonAction::DefaultInput { names } => {
            widgets::hint(ui, "Pick one device, or several to cycle through them:");
            device_checklist(ui, names, &audio.sources);
        }
        ButtonAction::Command { command } => {
            ui.add(egui::TextEdit::singleline(command).desired_width(320.0).hint_text("e.g. spectacle -r"));
        }
        ButtonAction::Keys { keys } => {
            ui.add(egui::TextEdit::singleline(keys).desired_width(220.0).hint_text("ctrl+shift+m"));
            widgets::hint(ui, "Needs ydotool with ydotoold running.");
        }
        ButtonAction::Profile { id } => {
            let current =
                profiles.iter().find(|(pid, _)| pid == id).map(|(_, n)| n.clone()).unwrap_or_else(|| "Pick…".into());
            egui::ComboBox::from_id_salt(("profile-action", cid)).selected_text(current).show_ui(ui, |ui| {
                for (pid, name) in profiles {
                    ui.selectable_value(id, pid.clone(), name);
                }
            });
        }
    }
}

fn device_checklist(ui: &mut egui::Ui, names: &mut Vec<String>, devices: &[crate::audio::Device]) {
    for d in devices {
        let mut on = names.contains(&d.name);
        if ui.checkbox(&mut on, &d.description).changed() {
            if on {
                names.push(d.name.clone());
            } else {
                names.retain(|n| *n != d.name);
            }
        }
    }
    // Keep devices that are currently unplugged.
    for n in names.clone() {
        if !devices.iter().any(|d| d.name == n) {
            widgets::hint(ui, &format!("{n} (not connected)"));
        }
    }
}
