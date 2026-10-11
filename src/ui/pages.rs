//! Buttons, Profiles, Settings and Log pages.

use eframe::egui::{self, RichText, Stroke, vec2};

use super::controller::port_combo;
use super::mixer::target_glyph;
use super::theme::{self, icon, *};
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
                            let scenes: Vec<String> = p.scenes.keys().cloned().collect();
                            let action = p.buttons.entry(cid.clone()).or_default();
                            action_editor(ui, cid, action, &audio, &profiles, &scenes);
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

    /// The profile's name; double-click to rename it in place.
    fn profile_name(&mut self, ui: &mut egui::Ui, id: &str, name: &str) {
        if let Some((rid, text)) = &mut self.profile_rename
            && rid == id
        {
            let edit_id = ui.id().with(("rename-profile", id));
            let r = ui.add(egui::TextEdit::singleline(text).id(edit_id).font(semibold(15.0)).desired_width(260.0));
            // Focus once when editing starts; after that, leaving the field ends it.
            let started = ui.data_mut(|d| std::mem::replace(d.get_temp_mut_or(edit_id, true), false));
            if started {
                r.request_focus();
            }
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            let done = r.lost_focus() || (r.has_focus() && enter);
            let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
            if done || cancel {
                let new = text.trim().to_string();
                if !cancel && !new.is_empty() && new != name {
                    let mut sh = self.shared.lock().unwrap();
                    if let Some(p) = sh.store.profiles.get_mut(id) {
                        p.name = new;
                        let _ = sh.store.save_profile(id);
                    }
                    drop(sh);
                    self.send(UiCmd::ProfileEdited);
                }
                self.profile_rename = None;
                ui.data_mut(|d| d.remove::<bool>(edit_id));
            }
            return;
        }
        let r = ui
            .add(egui::Label::new(RichText::new(name).font(semibold(15.0))).sense(egui::Sense::click()))
            .on_hover_text("Double-click to rename");
        if r.double_clicked() {
            self.profile_rename = Some((id.to_string(), name.to_string()));
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
                    .fill(pal().card)
                    .stroke(Stroke::new(
                        if is_active { 1.5 } else { 1.0 },
                        if is_active { pal().accent } else { pal().border },
                    ))
                    .corner_radius(10)
                    .inner_margin(egui::Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let led = if connected { RUN } else { pal().text_faint };
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
                                    self.profile_name(ui, id, &p.name);
                                    if is_active {
                                        widgets::chip(ui, "ACTIVE", pal().accent);
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
                drop(sh);
                self.pick_file(PickPurpose::Export(arg));
            }
            "delete" => {
                let name = sh.store.profiles.get(&arg).map(|p| p.name.clone()).unwrap_or_default();
                drop(sh);
                self.confirm = Some(super::Confirm {
                    title: format!("Delete \"{name}\"?"),
                    body:
                        "Its controls, assignments, banks and scenes are deleted too. Export it first to keep a copy."
                            .into(),
                    button: "Delete profile".into(),
                    action: super::ConfirmAction::DeleteProfile(arg),
                });
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
        let before = s.clone();
        ui.label(RichText::new("Settings").font(semibold(17.0)));
        ui.add_space(8.0);
        let wide = ui.available_width() > 1000.0;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if wide {
                ui.columns(2, |cols| {
                    self.general_card(&mut cols[0], &mut s);
                    cols[0].add_space(10.0);
                    appearance_card(&mut cols[0], &mut s);
                    cols[0].add_space(10.0);
                    volume_card(&mut cols[0], &mut s);
                    midi_card(&mut cols[1], &mut s, &ports_out);
                    cols[1].add_space(10.0);
                    self.nano_card(&mut cols[1]);
                    cols[1].add_space(10.0);
                    self.about_card(&mut cols[1]);
                });
            } else {
                ui.set_max_width(760.0);
                self.general_card(ui, &mut s);
                ui.add_space(10.0);
                appearance_card(ui, &mut s);
                ui.add_space(10.0);
                volume_card(ui, &mut s);
                ui.add_space(10.0);
                midi_card(ui, &mut s, &ports_out);
                ui.add_space(10.0);
                self.nano_card(ui);
                ui.add_space(10.0);
                self.about_card(ui);
            }
        });
        if s != before {
            self.shared.lock().unwrap().store.settings = s;
            self.send(UiCmd::ProfileEdited);
        }
    }

    fn general_card(&mut self, ui: &mut egui::Ui, s: &mut Settings) {
        widgets::card(ui, icon::GEAR_SIX, "General", |ui| {
            let mut autostart = config::autostart_enabled();
            if widgets::toggle(ui, &mut autostart, "Start when I log in").changed()
                && let Err(e) = config::set_autostart(autostart)
            {
                self.flash(format!("Autostart: {e:#}"));
            }
            widgets::toggle(ui, &mut s.start_minimized, "Start minimised to the tray");
            widgets::toggle(ui, &mut s.auto_switch, "Switch to the matching profile when a controller is plugged in");
            widgets::toggle(ui, &mut s.osd, "Show a volume pop-up when a fader moves");
        });
    }

    /// LED mode setup as steps, and a test that lights every LED in turn.
    fn nano_card(&mut self, ui: &mut egui::Ui) {
        widgets::card(ui, icon::LIGHTBULB, "nanoKONTROL2 LEDs", |ui| {
            widgets::hint(ui, "The computer can only light the buttons in \"External\" LED mode. Set it once:");
            for (i, step) in [
                "Open the Korg KONTROL Editor (it runs under Wine).",
                "Under Common, set LED Mode to External.",
                "Write the scene to the device.",
            ]
            .iter()
            .enumerate()
            {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(20.0, 20.0), egui::Sense::hover());
                    ui.painter().circle_filled(r.center(), 9.0, pal().accent_bg);
                    ui.painter().text(
                        r.center(),
                        egui::Align2::CENTER_CENTER,
                        (i + 1).to_string(),
                        semibold(11.5),
                        pal().text_strong,
                    );
                    ui.label(*step);
                });
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let running = self.led_test.is_some();
                let text = if running { "Testing…" } else { "Test LEDs" };
                if ui.add_enabled(!running, egui::Button::new(format!("{}  {text}", icon::LIGHTBULB))).clicked() {
                    let ids: Vec<String> = {
                        let sh = self.shared.lock().unwrap();
                        sh.active()
                            .map(|p| {
                                p.controls
                                    .iter()
                                    .filter(|(_, c)| c.kind != crate::profile::ControlKind::Fader)
                                    .map(|(id, _)| id.clone())
                                    .collect()
                            })
                            .unwrap_or_default()
                    };
                    self.led_test = Some((std::time::Instant::now(), ids, 0));
                }
                widgets::hint(ui, "Every button should light up once. If none do, LED Mode is still Internal.");
            });
        });
        // Light the next LED every 120 ms.
        if let Some((start, ids, next)) = &mut self.led_test {
            let due = (start.elapsed().as_millis() / 120) as usize;
            while *next <= due && *next < ids.len() {
                let _ = self.events.send(crate::engine::Event::Ui(UiCmd::SendTest(ids[*next].clone())));
                *next += 1;
            }
            if *next >= ids.len() {
                self.led_test = None;
            } else {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(120));
            }
        }
    }

    fn about_card(&mut self, ui: &mut egui::Ui) {
        widgets::card(ui, icon::INFO, "About", |ui| {
            ui.label(format!("Slidewire {}", env!("CARGO_PKG_VERSION")));
            let dir = self.shared.lock().unwrap().store.dir.clone();
            ui.horizontal(|ui| {
                ui.label(RichText::new(dir.display().to_string()).font(mono(12.0)).color(pal().text_dim));
                if ui.small_button(format!("{}  Open", icon::FOLDER_OPEN)).clicked()
                    && let Err(e) = std::process::Command::new("xdg-open").arg(&dir).spawn()
                {
                    self.flash(format!("Couldn't open the folder: {e}"));
                }
            });
            widgets::hint(ui, "Settings and profiles are plain JSON files in this folder.");
        });
    }

    pub(super) fn log(&mut self, ui: &mut egui::Ui) {
        let lines: Vec<crate::engine::LogEntry> = self.shared.lock().unwrap().log.iter().cloned().collect();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Log").font(semibold(17.0)));
            ui.add_space(SP_S);
            ui.add(
                egui::TextEdit::singleline(&mut self.log_filter)
                    .hint_text(format!("{}  Filter", icon::MAGNIFYING_GLASS))
                    .desired_width(220.0),
            );
            ui.checkbox(&mut self.log_warnings_only, "Problems only");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::icon_button(ui, icon::TRASH, "Clear").clicked() {
                    self.shared.lock().unwrap().log.clear();
                }
                if widgets::icon_button(ui, icon::COPY, "Copy").clicked() {
                    let text: String = lines.iter().map(|l| format!("{}  {}\n", l.time, l.text)).collect();
                    ui.ctx().copy_text(text);
                    self.flash("Log copied");
                }
            });
        });
        ui.add_space(8.0);
        let filter = self.log_filter.to_lowercase();
        let shown: Vec<&crate::engine::LogEntry> = lines
            .iter()
            .filter(|l| {
                (!self.log_warnings_only || l.warn) && (filter.is_empty() || l.text.to_lowercase().contains(&filter))
            })
            .collect();
        egui::Frame::new()
            .fill(LCD_BG)
            .corner_radius(8)
            .stroke(Stroke::new(1.0, egui::Color32::from_black_alpha(200)))
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(ui.available_height());
                egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    if shown.is_empty() {
                        let text = if lines.is_empty() { "Nothing logged yet." } else { "Nothing matches." };
                        ui.label(RichText::new(text).font(mono(12.5)).color(LCD_DIM));
                    }
                    for l in shown {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(&l.time).font(mono(12.0)).color(LCD_DIM));
                            let color = if l.warn { ASSIGN } else { LCD_TEXT };
                            ui.label(RichText::new(&l.text).font(mono(12.5)).color(color));
                        });
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
    scenes: &[String],
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
        ButtonAction::Bank { step: 1 },
        ButtonAction::Scene { name: scenes.first().cloned().unwrap_or_default() },
        ButtonAction::MuteInputs,
        ButtonAction::PushToTalk,
        ButtonAction::PushToMute,
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
            if keys_ready() {
                widgets::hint(ui, &format!("{}  ydotool is ready.", icon::CHECK));
            } else {
                ui.label(
                    RichText::new(format!(
                        "{}  Needs ydotool with ydotoold running (sudo pacman -S ydotool; systemctl --user enable --now ydotool).",
                        icon::WARNING
                    ))
                    .size(12.5)
                    .color(pal().warn_text),
                );
            }
        }
        ButtonAction::Bank { step } => {
            ui.horizontal(|ui| {
                ui.selectable_value(step, -1, format!("{}  Previous", icon::CARET_LEFT));
                ui.selectable_value(step, 1, format!("{}  Next", icon::CARET_RIGHT));
            });
            widgets::hint(ui, "Wraps around. The button's LED is lit on banks 2 and up.");
        }
        ButtonAction::Scene { name } => {
            if scenes.is_empty() {
                widgets::hint(ui, "No scenes yet. Save one with the Scenes menu on the Mixer page.");
            } else {
                let text = if name.is_empty() { "Pick…".to_string() } else { name.clone() };
                egui::ComboBox::from_id_salt(("scene-action", cid)).selected_text(text).show_ui(ui, |ui| {
                    for s in scenes {
                        ui.selectable_value(name, s.clone(), s);
                    }
                });
            }
        }
        ButtonAction::MuteInputs => {
            widgets::hint(ui, "Mutes or unmutes every microphone. The LED is lit while they're muted.");
        }
        ButtonAction::PushToTalk => {
            widgets::hint(ui, "The default microphone is live only while you hold the button.");
        }
        ButtonAction::PushToMute => {
            widgets::hint(ui, "The default microphone is muted while you hold the button.");
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

fn appearance_card(ui: &mut egui::Ui, s: &mut Settings) {
    use crate::config::{Accent, ThemeMode};
    widgets::card(ui, icon::PALETTE, "Appearance", |ui| {
        ui.horizontal(|ui| {
            ui.label("Theme");
            ui.add_space(SP_S);
            widgets::segmented(
                ui,
                &mut s.theme,
                &[(ThemeMode::System, "System"), (ThemeMode::Dark, "Dark"), (ThemeMode::Light, "Light")],
            );
        });
        ui.horizontal(|ui| {
            ui.label("Accent");
            ui.add_space(SP_S);
            let system = crate::appearance::system().accent;
            for a in Accent::ALL {
                if a == Accent::System && system.is_none() {
                    continue;
                }
                let c = theme::accent_color(a, system);
                if widgets::swatch(ui, c, s.accent == a, a.label()).clicked() {
                    s.accent = a;
                }
            }
        });
        widgets::toggle(ui, &mut s.reduce_motion, "Reduce motion");
    });
}

/// Is ydotool installed and its daemon running? Checked once.
fn keys_ready() -> bool {
    static READY: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *READY.get_or_init(|| {
        let installed =
            std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("ydotool").is_file()));
        let socket = std::env::var_os("YDOTOOL_SOCKET").map(std::path::PathBuf::from).unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var("XDG_RUNTIME_DIR").unwrap_or_default()).join(".ydotool_socket")
        });
        installed && (socket.exists() || std::path::Path::new("/tmp/.ydotool_socket").exists())
    })
}

/// Volume against fader position, with the straight 1.0 curve for reference.
fn curve_preview(ui: &mut egui::Ui, exponent: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(84.0, 52.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 4.0, LCD_BG);
    let inner = rect.shrink(5.0);
    let at = |x: f32, y: f32| egui::pos2(inner.left() + x * inner.width(), inner.bottom() - y * inner.height());
    p.line_segment([at(0.0, 0.0), at(1.0, 1.0)], Stroke::new(1.0, LCD_DIM));
    let pts: Vec<egui::Pos2> =
        (0..=32).map(|i| i as f32 / 32.0).map(|x| at(x, crate::mapping::curve(x, exponent))).collect();
    p.add(egui::Shape::line(pts, Stroke::new(1.5, LCD_TEXT)));
}

fn volume_card(ui: &mut egui::Ui, s: &mut Settings) {
    widgets::card(ui, icon::SPEAKER_HIGH, "Volume", |ui| {
        ui.horizontal(|ui| {
            ui.label("Curve");
            ui.spacing_mut().slider_width = (ui.available_width() - 200.0).clamp(120.0, 320.0);
            ui.add(egui::Slider::new(&mut s.curve, 0.5..=3.0).step_by(0.1).fixed_decimals(1));
            ui.add_space(SP_S);
            curve_preview(ui, s.curve);
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
        widgets::toggle(ui, &mut s.pickup, "Pick up: a fader only takes over once it reaches the volume");
        widgets::hint(
            ui,
            "Stops jumps when a volume was changed elsewhere. The mixer shows where your fader is in amber.",
        );
    });
}

fn midi_card(ui: &mut egui::Ui, s: &mut Settings, ports_out: &[String]) {
    widgets::card(ui, icon::PLUGS_CONNECTED, "MIDI", |ui| {
        widgets::toggle(ui, &mut s.led_heartbeat, "LED heartbeat: resend LED states every 5 s");
        widgets::hint(ui, "For devices that forget their LEDs.");
        ui.horizontal(|ui| {
            ui.label("Forward all MIDI input to");
            port_combo(ui, "passthru", &mut s.passthru, ports_out);
        });
    });
}
