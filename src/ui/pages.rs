//! Buttons, Profiles, Settings and Log pages.

use eframe::egui::{self, RichText, Stroke, vec2};
use rust_i18n::t;

use super::controller::port_combo;
use super::mixer::target_glyph;
use super::theme::{self, icon, *};
use super::widgets;
use super::{App, PickPurpose, Tab};
use crate::audio::AudioState;
use crate::config::{self, Settings};
use crate::engine::{Event, Notice, UiCmd};
use crate::profile::{AppMatch, AutoAction, AutoRule, ButtonAction, MediaKey, Profile, Target};

impl App {
    pub(super) fn buttons(&mut self, ui: &mut egui::Ui) {
        let (profile, audio, profiles, activity) = {
            let sh = self.shared.lock().unwrap();
            let profiles: Vec<(String, String)> =
                sh.store.profiles.iter().map(|(id, p)| (id.clone(), p.name.clone())).collect();
            (sh.active().cloned(), sh.audio.clone(), profiles, sh.activity.clone())
        };
        let Some(mut p) = profile else {
            self.no_profile(ui);
            return;
        };
        let before = p.buttons.clone();
        widgets::page_header(
            ui,
            &t!("Buttons"),
            Some(&t!("Buttons that aren't part of a channel strip can run these actions.")),
            |_| {},
        );
        let spare = p.spare_buttons();
        let mut press = None;
        let mut learn: Option<String> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if spare.is_empty() {
                widgets::card(ui, icon::INFO, &t!("No spare buttons"), |ui| {
                    ui.label(t!(
                        "Every button belongs to a group. Add a button control under Controller to use it here."
                    ));
                });
            }
            // Grouped like the hardware, cards in equal-width columns.
            let avail = ui.available_width() - 14.0;
            let cols = ((avail / 440.0).floor() as usize).clamp(1, 3);
            let width = (avail - 12.0 * (cols as f32 - 1.0)) / cols as f32;
            let scenes: Vec<String> = p.scenes.keys().cloned().collect();
            let mut sections: Vec<(&str, Vec<String>)> = Vec::new();
            for cid in &spare {
                let section = button_section(&p.control_name(cid));
                match sections.iter_mut().find(|(s, _)| *s == section) {
                    Some((_, ids)) => ids.push(cid.clone()),
                    None => sections.push((section, vec![cid.clone()])),
                }
            }
            sections.sort_by_key(|(s, _)| ["Transport", "Track", "Marker", "Other"].iter().position(|x| x == s));
            for (section, ids) in sections {
                if section != "Other" || spare.len() > ids.len() {
                    let heading = match section {
                        "Transport" => t!("Transport"),
                        "Track" => t!("Track"),
                        "Marker" => t!("Marker"),
                        _ => t!("Other"),
                    };
                    ui.label(RichText::new(heading.to_uppercase()).size(11.5).color(pal().text_dim).strong());
                    ui.add_space(2.0);
                }
                egui::Grid::new(("button-cards", section)).num_columns(cols).spacing([12.0, 12.0]).show(ui, |ui| {
                    for (i, cid) in ids.iter().enumerate() {
                        let name = p.control_name(cid);
                        let lit = activity.get(cid).is_some_and(|(t, v)| *v > 0 && t.elapsed().as_millis() < 250);
                        if lit {
                            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
                        }
                        ui.vertical(|ui| {
                            ui.set_width(width);
                            ui.set_max_width(width);
                            let glyph = if lit { icon::RECORD } else { button_glyph(&name) };
                            let corner = |ui: &mut egui::Ui| {
                                if widgets::icon_only(
                                    ui,
                                    icon::CROSSHAIR,
                                    &t!("Learn: press the button on your controller to set which one this is"),
                                )
                                .clicked()
                                {
                                    learn = Some(cid.clone());
                                }
                                if widgets::icon_only(ui, icon::PLAY, &t!("Try it")).clicked() {
                                    press = Some(cid.clone());
                                }
                            };
                            widgets::card_with(ui, glyph, &name, corner, |ui| {
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
                                action_editor(ui, cid, action, &audio, &profiles, &scenes);
                            });
                        });
                        if (i + 1) % cols == 0 {
                            ui.end_row();
                        }
                    }
                });
                ui.add_space(14.0);
            }
        });
        p.buttons.retain(|_, a| *a != ButtonAction::None);
        if p.buttons != before {
            let buttons = p.buttons;
            self.edit_profile(|ap| ap.buttons = buttons);
        }
        if let Some(cid) = press {
            self.send(UiCmd::Press(cid));
        }
        if let Some(cid) = learn {
            self.learn(&cid);
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
                        sh.store.touch(id);
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
            .on_hover_text(t!("Double-click to rename"));
        if r.double_clicked() {
            self.profile_rename = Some((id.to_string(), name.to_string()));
        }
    }

    /// Focus rules of the active profile: switch bank or scene while an app
    /// is in front.
    fn auto_card(&mut self, ui: &mut egui::Ui) {
        let (rules, banks, scenes, apps) = {
            let sh = self.shared.lock().unwrap();
            let Some(p) = sh.active() else { return };
            let mut apps: Vec<(String, String)> =
                sh.audio.apps.iter().map(|a| (a.display_name(), a.pattern())).collect();
            apps.sort();
            apps.dedup();
            (p.auto.clone(), p.bank_count(), p.scenes.keys().cloned().collect::<Vec<_>>(), apps)
        };
        widgets::card(ui, icon::APP_WINDOW, &t!("When an app has focus"), |ui| {
            widgets::hint(
                ui,
                &t!("Switch to a bank or recall a scene while an app is in front, and back when you leave it."),
            );
            let mut remove = None;
            let mut change = None;
            for (i, r) in rules.iter().enumerate() {
                ui.horizontal(|ui| {
                    let name = if r.label.is_empty() { &r.app.pattern } else { &r.label };
                    let layout = egui::Layout::left_to_right(egui::Align::Center);
                    ui.allocate_ui_with_layout(vec2(150.0, 22.0), layout, |ui| {
                        ui.add(egui::Label::new(RichText::new(name).strong()).truncate())
                            .on_hover_text(t!("Matches \"%{pattern}\"", pattern = r.app.pattern));
                    });
                    ui.label(RichText::new(icon::ARROW_RIGHT).color(pal().text_dim));
                    let text = match &r.action {
                        AutoAction::Bank { bank } => t!("Bank %{bank}", bank = bank + 1),
                        AutoAction::Scene { name } => t!("Scene \"%{name}\"", name = name),
                    };
                    widgets::combo(("auto", i)).selected_text(text).width(170.0).show_ui(ui, |ui| {
                        for b in 0..banks {
                            let a = AutoAction::Bank { bank: b };
                            if ui.selectable_label(r.action == a, t!("Bank %{bank}", bank = b + 1)).clicked() {
                                change = Some((i, a));
                            }
                        }
                        for sc in &scenes {
                            let a = AutoAction::Scene { name: sc.clone() };
                            if ui.selectable_label(r.action == a, t!("Scene \"%{name}\"", name = sc)).clicked() {
                                change = Some((i, a));
                            }
                        }
                    });
                    if widgets::icon_only(ui, icon::TRASH, &t!("Remove rule")).clicked() {
                        remove = Some(i);
                    }
                });
            }
            let default = if banks > 1 {
                Some(AutoAction::Bank { bank: 1 })
            } else {
                scenes.first().map(|name| AutoAction::Scene { name: name.clone() })
            };
            match default {
                None => {
                    widgets::hint(ui, &t!("Add a bank or save a scene first (on the Mixer page)."));
                }
                Some(default) => {
                    let r = ui.button(format!("{}  {}", icon::PLUS, t!("Add rule")));
                    egui::Popup::menu(&r).show(|ui| {
                        ui.label(RichText::new(t!("Apps playing now")).font(semibold(13.0)));
                        if apps.is_empty() {
                            widgets::hint(ui, &t!("None. Start the app, then add the rule."));
                        }
                        for (label, pattern) in &apps {
                            if ui.button(label).clicked() {
                                let rule = AutoRule {
                                    app: AppMatch { pattern: pattern.clone(), regex: false },
                                    label: label.clone(),
                                    action: default.clone(),
                                };
                                self.edit_profile(|p| p.auto.push(rule));
                                ui.close();
                            }
                        }
                    });
                }
            };
            if let Some((i, a)) = change {
                self.edit_profile(|p| p.auto[i].action = a);
            }
            if let Some(i) = remove {
                self.edit_profile(|p| {
                    p.auto.remove(i);
                });
            }
        });
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
        widgets::page_header(
            ui,
            &t!("Profiles"),
            Some(&t!("One profile per controller. The matching one is picked when a device is plugged in.")),
            |_| {},
        );
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
                    .inner_margin(egui::Margin::symmetric(14, 12))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            profile_thumb(ui, p, connected);
                            ui.add_space(4.0);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.horizontal(|ui| {
                                    self.profile_name(ui, id, &p.name);
                                    if is_active {
                                        widgets::pill(ui, Some(pal().accent), &t!("In use"));
                                    }
                                });
                                let (led, status) = if connected {
                                    (RUN, t!("connected"))
                                } else {
                                    (pal().text_faint, t!("not connected"))
                                };
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(10.0), egui::Sense::hover());
                                    ui.painter().circle_filled(r.center(), 4.0, led);
                                    let port = if p.input.is_empty() {
                                        t!("no port set").to_string()
                                    } else {
                                        p.input.clone()
                                    };
                                    widgets::hint(
                                        ui,
                                        &t!(
                                            "%{port} · %{status} · %{controls} controls · %{strips} strips",
                                            port = port,
                                            status = status,
                                            controls = p.controls.len(),
                                            strips = p.groups.len()
                                        ),
                                    );
                                });
                                ui.horizontal(|ui| {
                                    if !is_active
                                        && ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new(format!("{}  {}", icon::CHECK, t!("Use"))).strong(),
                                                )
                                                .fill(pal().accent_bg),
                                            )
                                            .clicked()
                                    {
                                        action = Some(("activate", id.clone()));
                                    }
                                    if widgets::icon_button(ui, icon::COPY, &t!("Duplicate")).clicked() {
                                        action = Some(("duplicate", id.clone()));
                                    }
                                    if widgets::icon_button(ui, icon::EXPORT, &t!("Export"))
                                        .on_hover_text(t!("Save it as a file (the MIDI Mixer spec.json format)"))
                                        .clicked()
                                    {
                                        action = Some(("export", id.clone()));
                                    }
                                    if widgets::icon_button(ui, icon::TRASH, &t!("Delete")).clicked() {
                                        action = Some(("delete", id.clone()));
                                    }
                                });
                            });
                        });
                    });
                ui.add_space(6.0);
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if widgets::icon_button(ui, icon::PLUS, &t!("New empty profile")).clicked() {
                    action = Some(("new", String::new()));
                }
                if widgets::icon_button(ui, icon::PLUS, &t!("Korg nanoKONTROL2 default")).clicked() {
                    action = Some(("nano", String::new()));
                }
            });
            ui.add_space(12.0);
            widgets::card(ui, icon::DOWNLOAD_SIMPLE, &t!("Import"), |ui| {
                widgets::hint(ui, &t!("Profiles from MIDI Mixer for Windows (spec.json) and from this app."));
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.import_path)
                            .desired_width(420.0)
                            .hint_text("/path/to/spec.json"),
                    );
                    if widgets::icon_button(ui, icon::FOLDER_OPEN, &t!("Browse…")).clicked() {
                        self.pick_file(PickPurpose::Import);
                    }
                    if widgets::icon_button(ui, icon::DOWNLOAD_SIMPLE, &t!("Import")).clicked() {
                        action = Some(("import", self.import_path.trim().to_string()));
                    }
                });
            });
            ui.add_space(12.0);
            self.auto_card(ui);
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
                    p.name = t!("%{name} (copy)", name = p.name).to_string();
                    let id = crate::profile::new_id();
                    let name = p.name.clone();
                    sh.store.profiles.insert(id.clone(), p);
                    sh.store.touch(&id);
                    drop(sh);
                    // Straight into renaming the copy.
                    self.profile_rename = Some((id, name));
                    self.flash(t!("Duplicated; give the copy a name"));
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
                    title: t!("Delete \"%{name}\"?", name = name).to_string(),
                    body: t!(
                        "Its controls, assignments, banks and scenes are deleted too. Export it first to keep a copy."
                    )
                    .into(),
                    button: t!("Delete profile").into(),
                    action: super::ConfirmAction::DeleteProfile(arg),
                    alt: None,
                });
            }
            "new" | "nano" => {
                let p = if what == "nano" {
                    Profile::nanokontrol2()
                } else {
                    Profile { name: t!("New profile").into(), ..Default::default() }
                };
                let id = crate::profile::new_id();
                sh.store.profiles.insert(id.clone(), p);
                sh.store.touch(&id);
                drop(sh);
                self.send(UiCmd::ActivateProfile(id));
                if what == "new" {
                    self.tab = Tab::Controller;
                }
            }
            "import" => {
                drop(sh);
                // Ask before keeping buttons that would run commands.
                let risky = Profile::load(std::path::Path::new(&arg)).map(|p| p.risky_buttons()).unwrap_or_default();
                if risky.is_empty() {
                    self.import_profile(&arg, false);
                } else {
                    self.confirm = Some(super::Confirm {
                        title: t!("This profile runs commands").into(),
                        body: format!(
                            "{}\n\n{}",
                            risky.join("\n"),
                            t!("Only keep these if you trust where the profile came from.")
                        ),
                        button: t!("Keep them").into(),
                        action: super::ConfirmAction::Import { path: arg.clone(), trusted: true },
                        alt: Some((
                            t!("Import without them").into(),
                            super::ConfirmAction::Import { path: arg, trusted: false },
                        )),
                    });
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
        widgets::page_header(ui, &t!("Settings"), None, |_| {});
        let wide = ui.available_width() > 1000.0;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if wide {
                ui.columns(2, |cols| {
                    self.general_card(&mut cols[0], &mut s);
                    cols[0].add_space(10.0);
                    appearance_card(&mut cols[0], &mut s);
                    cols[0].add_space(10.0);
                    volume_card(&mut cols[0], &mut s);
                    cols[0].add_space(10.0);
                    advanced_card(&mut cols[0], &mut s, &ports_out);
                    self.nano_card(&mut cols[1]);
                    cols[1].add_space(10.0);
                    obs_card(&mut cols[1], &mut s, &self.events);
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
                self.nano_card(ui);
                ui.add_space(10.0);
                obs_card(ui, &mut s, &self.events);
                ui.add_space(10.0);
                advanced_card(ui, &mut s, &ports_out);
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
        widgets::card(ui, icon::GEAR_SIX, &t!("General"), |ui| {
            let mut autostart = config::autostart_enabled();
            if widgets::toggle(ui, &mut autostart, &t!("Start when I log in")).changed() {
                let events = self.events.clone();
                config::set_autostart_in_background(autostart, move |e| {
                    let _ =
                        events.send(Event::Notice(Notice::Problem(t!("Autostart: %{error}", error = e).to_string())));
                });
            }
            widgets::toggle(ui, &mut s.start_minimized, &t!("Start minimised to the tray"));
            widgets::toggle(ui, &mut s.close_quits, &t!("Closing the window quits (instead of hiding to the tray)"));
            widgets::toggle(
                ui,
                &mut s.auto_switch,
                &t!("Switch to the matching profile when a controller is plugged in"),
            );
            widgets::toggle(ui, &mut s.osd, &t!("Show a volume pop-up when a fader moves"));
        });
    }

    /// LED mode setup as steps, and a test that lights every LED in turn.
    fn nano_card(&mut self, ui: &mut egui::Ui) {
        widgets::card(ui, icon::LIGHTBULB, &t!("nanoKONTROL2 LEDs"), |ui| {
            widgets::hint(ui, &t!("The computer can only light the buttons in \"External\" LED mode. Set it once:"));
            for (i, step) in [
                t!("Open the Korg KONTROL Editor (it runs under Wine)."),
                t!("Under Common, set LED Mode to External."),
                t!("Write the scene to the device."),
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
                    ui.label(step.clone());
                });
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let running = self.led_test.is_some();
                let text = if running { t!("Testing…") } else { t!("Test LEDs") };
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
                widgets::hint(ui, &t!("Every button should light up once. If none do, LED Mode is still Internal."));
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
        widgets::card(ui, icon::INFO, &t!("About"), |ui| {
            ui.horizontal(|ui| {
                if let Some(logo) = &self.logo {
                    ui.add(egui::Image::new(logo).fit_to_exact_size(vec2(40.0, 40.0)));
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(RichText::new(format!("Slidewire {}", env!("CARGO_PKG_VERSION"))).font(semibold(15.0)));
                    widgets::hint(ui, &t!("Free software under the GPL 3.0 or later."));
                });
            });
            ui.add_space(SP_S);
            ui.horizontal_wrapped(|ui| {
                ui.hyperlink_to(format!("{}  {}", icon::CODE, t!("Source code")), REPO);
                ui.hyperlink_to(format!("{}  {}", icon::BUG, t!("Report a problem")), format!("{REPO}/issues"));
            });
            ui.add_space(SP_S);
            ui.horizontal_wrapped(|ui| {
                let dir = self.shared.lock().unwrap().store.dir.clone();
                if ui
                    .button(format!("{}  {}", icon::FOLDER_OPEN, t!("Open settings folder")))
                    .on_hover_text(dir.display().to_string())
                    .clicked()
                    && let Err(e) = crate::actions::spawn_detached(std::process::Command::new("xdg-open").arg(&dir))
                {
                    self.problem(t!("Couldn't open the folder: %{error}", error = e));
                }
                if ui.button(format!("{}  {}", icon::HAND_WAVING, t!("Show the welcome again"))).clicked() {
                    self.welcome = Some(super::welcome::Welcome::new());
                    self.welcome_other = None;
                }
            });
            ui.add_space(SP_S);
            ui.collapsing(t!("Third-party notices"), |ui| {
                widgets::hint(
                    ui,
                    &t!(
                        "Inter and JetBrains Mono fonts: SIL Open Font License 1.1. Phosphor icons: MIT License. Rust crates: see each crate's licence."
                    ),
                );
            });
        });
    }

    pub(super) fn log(&mut self, ui: &mut egui::Ui) {
        let lines: Vec<crate::engine::LogEntry> = self.shared.lock().unwrap().log.iter().cloned().collect();
        let mut copied = false;
        widgets::page_header(ui, &t!("Log"), Some("What happened, newest at the bottom."), |ui| {
            if widgets::icon_button(ui, icon::TRASH, &t!("Clear")).clicked() {
                self.shared.lock().unwrap().log.clear();
            }
            if widgets::icon_button(ui, icon::COPY, &t!("Copy")).clicked() {
                let text: String = lines.iter().map(|l| format!("{}  {}\n", l.time, l.text)).collect();
                ui.ctx().copy_text(text);
                copied = true;
            }
            ui.checkbox(&mut self.log_warnings_only, t!("Problems only"));
            widgets::text_field(ui, &mut self.log_filter, &t!("Filter"), Some(icon::MAGNIFYING_GLASS), 220.0);
        });
        if copied {
            self.flash(t!("Log copied"));
        }
        let filter = self.log_filter.to_lowercase();
        let shown: Vec<&crate::engine::LogEntry> = lines
            .iter()
            .filter(|l| {
                (!self.log_warnings_only || l.warn) && (filter.is_empty() || l.text.to_lowercase().contains(&filter))
            })
            .collect();
        // Dark: a terminal-like display. Light: a plain card, easier to read.
        let dark = pal().dark;
        let (bg, text, time, warn) = if dark {
            (LCD_BG, LCD_TEXT, theme::mix(LCD_TEXT, LCD_BG, 0.45), ASSIGN)
        } else {
            (pal().card, pal().text, pal().text_dim, pal().warn_text)
        };
        egui::Frame::new()
            .fill(bg)
            .corner_radius(8)
            .stroke(Stroke::new(1.0, if dark { egui::Color32::from_black_alpha(200) } else { pal().border }))
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(ui.available_height());
                egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if shown.is_empty() {
                        let msg = if lines.is_empty() { t!("Nothing logged yet.") } else { t!("Nothing matches.") };
                        ui.label(RichText::new(msg).font(mono(12.5)).color(time));
                    }
                    for (i, l) in shown.iter().enumerate() {
                        // Every other row slightly shaded, so long lines stay easy to follow.
                        let stripe =
                            if i % 2 == 1 { pal().sheen(if dark { 5 } else { 0 }) } else { egui::Color32::TRANSPARENT };
                        let stripe = if !dark && i % 2 == 1 { pal().inset } else { stripe };
                        egui::Frame::new()
                            .fill(stripe)
                            .corner_radius(4)
                            .inner_margin(egui::Margin::symmetric(6, 3))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(RichText::new(&l.time).font(mono(12.0)).color(time));
                                    if l.warn {
                                        widgets::chip(ui, &t!("WARN"), warn);
                                    }
                                    ui.label(RichText::new(&l.text).font(mono(12.5)).color(if l.warn {
                                        warn
                                    } else {
                                        text
                                    }));
                                });
                            });
                    }
                });
            });
    }
}

/// A small drawing of a profile's controller: one fader per strip (up to
/// 8), lit when it's connected.
fn profile_thumb(ui: &mut egui::Ui, p: &Profile, connected: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(96.0, 60.0), egui::Sense::hover());
    let painter = ui.painter();
    widgets::bevel_panel(painter, rect, 6, theme::hw::BODY_TOP, theme::hw::BODY_BOTTOM);
    let strips = p.groups.values().filter(|g| !g.faders.is_empty() && !g.name.to_lowercase().starts_with("knob"));
    let n = strips.count().clamp(1, 8);
    let area = rect.shrink2(vec2(10.0, 9.0));
    let step = area.width() / n as f32;
    for i in 0..n {
        let x = area.left() + step * (i as f32 + 0.5);
        painter.line_segment([egui::pos2(x, area.top()), egui::pos2(x, area.bottom())], Stroke::new(2.0, SLOT));
        // Caps at different heights, like a mix in progress.
        let y = area.top() + area.height() * (0.25 + 0.5 * (((i * 7) % 5) as f32 / 4.0));
        let cap = egui::Rect::from_center_size(egui::pos2(x, y), vec2((step * 0.6).min(9.0), 4.0));
        painter.rect_filled(cap, 1.0, if connected { pal().accent } else { theme::hw::TEXT_DIM });
    }
}

/// Where a spare button goes on the Buttons page, by its name.
fn button_section(name: &str) -> &'static str {
    let n = name.to_lowercase();
    if ["play", "stop", "rec", "rewind", "forward", "ffwd", "rew"].iter().any(|k| n.contains(k)) {
        "Transport"
    } else if n.contains("track") || n.contains("cycle") {
        "Track"
    } else if n.contains("marker") {
        "Marker"
    } else {
        "Other"
    }
}

/// An icon that looks like the button on the device.
fn button_glyph(name: &str) -> &'static str {
    let n = name.to_lowercase();
    if n.contains("rewind") || n.contains("rew") {
        icon::REWIND
    } else if n.contains("forward") || n.contains("ffwd") {
        icon::FAST_FORWARD
    } else if n.contains("stop") {
        icon::STOP
    } else if n.contains("play") {
        icon::PLAY
    } else if n.contains("rec") {
        icon::RECORD
    } else if n.contains("cycle") {
        icon::REPEAT
    } else if n.contains("set") {
        icon::MAP_PIN
    } else if n.contains('<') || n.contains("prev") || n.contains("left") {
        icon::CARET_LEFT
    } else if n.contains('>') || n.contains("next") || n.contains("right") {
        icon::CARET_RIGHT
    } else {
        icon::CURSOR_CLICK
    }
}

/// Pick a device by name from `options` (name, label); empty = `default`.
fn device_combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    label: &str,
    value: &mut String,
    options: &[(String, String)],
    default: &str,
) {
    ui.horizontal(|ui| {
        ui.label(label);
        let text = options.iter().find(|(n, _)| n == value).map_or(default, |(_, l)| l.as_str());
        widgets::combo(id).selected_text(text).width(220.0).show_ui(ui, |ui| {
            ui.selectable_value(value, String::new(), default);
            for (name, l) in options {
                ui.selectable_value(value, name.clone(), l);
            }
        });
    });
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
        ButtonAction::Media { key: MediaKey::PlayPause, player: String::new() },
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
        ButtonAction::Obs { op: crate::obs::ObsOp::ToggleRecord },
        ButtonAction::Monitor { source: String::new(), sink: String::new() },
        ButtonAction::EasyEffects { preset: String::new() },
    ];
    widgets::combo(("action", cid)).selected_text(action.label()).width(240.0).show_ui(ui, |ui| {
        for k in kinds {
            let same = std::mem::discriminant(&k) == std::mem::discriminant(action);
            if ui.selectable_label(same, k.label()).clicked() && !same {
                *action = k;
            }
        }
    });
    match action {
        ButtonAction::None => {}
        ButtonAction::Media { key, player } => {
            ui.horizontal(|ui| {
                for (k, l) in [
                    (MediaKey::PlayPause, format!("{}  {}", icon::PLAY_PAUSE, t!("Play/Pause"))),
                    (MediaKey::Stop, format!("{}  {}", icon::STOP, t!("Stop"))),
                    (MediaKey::Previous, format!("{}  {}", icon::SKIP_BACK, t!("Previous"))),
                    (MediaKey::Next, format!("{}  {}", icon::SKIP_FORWARD, t!("Next"))),
                ] {
                    ui.selectable_value(key, k, l);
                }
            });
            ui.horizontal(|ui| {
                ui.label(t!("Player"));
                ui.add(egui::TextEdit::singleline(player).hint_text(t!("whichever is playing")).desired_width(160.0));
                let r = widgets::icon_framed(ui, icon::CARET_DOWN, &t!("Running players"));
                egui::Popup::menu(&r).show(|ui| {
                    let players = crate::dbus::mpris_players();
                    if players.is_empty() {
                        widgets::hint(ui, &t!("No media players running."));
                    }
                    for p in players {
                        if ui.button(&p).clicked() {
                            *player = p;
                            ui.close();
                        }
                    }
                });
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
            widgets::hint(ui, &t!("Pick one device, or several to cycle through them:"));
            device_checklist(ui, names, &audio.sinks);
        }
        ButtonAction::DefaultInput { names } => {
            widgets::hint(ui, &t!("Pick one device, or several to cycle through them:"));
            device_checklist(ui, names, &audio.sources);
        }
        ButtonAction::Command { command } => {
            ui.add(egui::TextEdit::singleline(command).desired_width(320.0).hint_text(t!("e.g. spectacle -r")));
        }
        ButtonAction::Keys { keys } => {
            ui.add(egui::TextEdit::singleline(keys).desired_width(220.0).hint_text("ctrl+shift+m"));
            if keys_ready() {
                widgets::hint(ui, &format!("{}  {}", icon::CHECK, t!("ydotool is ready.")));
            } else {
                ui.label(
                    RichText::new(format!(
                        "{}  {}",
                        icon::WARNING,
                        t!("Needs ydotool with ydotoold running (sudo pacman -S ydotool; systemctl --user enable --now ydotool).")
                    ))
                    .size(12.5)
                    .color(pal().warn_text),
                );
            }
        }
        ButtonAction::Bank { step } => {
            ui.horizontal(|ui| {
                ui.selectable_value(step, -1, format!("{}  {}", icon::CARET_LEFT, t!("Previous")));
                ui.selectable_value(step, 1, format!("{}  {}", icon::CARET_RIGHT, t!("Next")));
            });
            widgets::hint(ui, &t!("Wraps around. The button's LED is lit on banks 2 and up."));
        }
        ButtonAction::Scene { name } => {
            if scenes.is_empty() {
                widgets::hint(ui, &t!("No scenes yet. Save one with the Scenes menu on the Mixer page."));
            } else {
                let text = if name.is_empty() { t!("Pick…").to_string() } else { name.clone() };
                widgets::combo(("scene-action", cid)).selected_text(text).show_ui(ui, |ui| {
                    for s in scenes {
                        ui.selectable_value(name, s.clone(), s);
                    }
                });
            }
        }
        ButtonAction::MuteInputs => {
            widgets::hint(ui, &t!("Mutes or unmutes every microphone. The LED is lit while they're muted."));
        }
        ButtonAction::PushToTalk => {
            widgets::hint(ui, &t!("The default microphone is live only while you hold the button."));
        }
        ButtonAction::PushToMute => {
            widgets::hint(ui, &t!("The default microphone is muted while you hold the button."));
        }
        ButtonAction::Monitor { source, sink } => {
            let sources: Vec<(String, String)> = audio
                .sources
                .iter()
                .map(|d| (d.name.clone(), d.label.clone()))
                .chain(
                    audio
                        .sinks
                        .iter()
                        .filter(|d| crate::routing::is_bus(&d.name))
                        .map(|d| (d.monitor_source.clone(), t!("%{bus} (bus)", bus = d.label).to_string())),
                )
                .collect();
            let sinks: Vec<(String, String)> = audio
                .sinks
                .iter()
                .filter(|d| !crate::routing::is_bus(&d.name))
                .map(|d| (d.name.clone(), d.label.clone()))
                .collect();
            device_combo(ui, ("monitor-src", cid), &t!("Hear"), source, &sources, &t!("Default input"));
            device_combo(ui, ("monitor-sink", cid), &t!("On"), sink, &sinks, &t!("Default output"));
            widgets::hint(
                ui,
                &t!("Press to hear it live (wear headphones), press again to stop. The LED shows it's on."),
            );
        }
        ButtonAction::EasyEffects { preset } => {
            let presets = crate::routing::easyeffects_presets();
            if presets.is_empty() {
                widgets::hint(ui, &t!("No EasyEffects presets found. Save one in EasyEffects first."));
                ui.add(egui::TextEdit::singleline(preset).hint_text(t!("Preset name")).desired_width(220.0));
            } else {
                let text = if preset.is_empty() { t!("Pick…").to_string() } else { preset.clone() };
                widgets::combo(("ee-preset", cid)).selected_text(text).width(220.0).show_ui(ui, |ui| {
                    for (name, input) in presets {
                        let label = format!("{}  {name}", if input { icon::MICROPHONE } else { icon::SPEAKER_HIGH });
                        ui.selectable_value(preset, name.clone(), label);
                    }
                });
            }
            widgets::hint(
                ui,
                &t!("Two buttons with two presets (say, with and without noise suppression) make a switch."),
            );
        }
        ButtonAction::Obs { op } => {
            use crate::obs::ObsOp;
            ui.horizontal(|ui| {
                for (o, l) in [
                    (ObsOp::Scene { name: String::new() }, t!("Switch scene")),
                    (ObsOp::ToggleMute { input: String::new() }, t!("Mute a source")),
                    (ObsOp::ToggleRecord, t!("Record")),
                    (ObsOp::ToggleStream, t!("Stream")),
                ] {
                    let same = std::mem::discriminant(&o) == std::mem::discriminant(op);
                    if ui.selectable_label(same, l).clicked() && !same {
                        *op = o;
                    }
                }
            });
            match op {
                ObsOp::Scene { name } => {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(name)
                                .hint_text(t!("Scene name, exactly as in OBS"))
                                .desired_width(240.0),
                        );
                        let r = widgets::icon_framed(ui, icon::CARET_DOWN, &t!("Scenes in OBS"));
                        if r.clicked() {
                            fetch_obs_scenes(ui.ctx());
                        }
                        egui::Popup::menu(&r).show(|ui| match &*OBS_SCENES.lock().unwrap() {
                            None => {
                                widgets::hint(ui, &t!("Asking OBS…"));
                            }
                            Some(Err(e)) => {
                                widgets::hint(ui, e);
                            }
                            Some(Ok(list)) => {
                                for scene in list {
                                    if ui.button(scene).clicked() {
                                        *name = scene.clone();
                                        ui.close();
                                    }
                                }
                            }
                        });
                    });
                }
                ObsOp::ToggleMute { input } => {
                    ui.add(
                        egui::TextEdit::singleline(input)
                            .hint_text(t!("Source name, e.g. Mic/Aux"))
                            .desired_width(240.0),
                    );
                }
                ObsOp::ToggleRecord => {
                    widgets::hint(ui, &t!("Starts recording, or stops it."));
                }
                ObsOp::ToggleStream => {
                    widgets::hint(ui, &t!("Starts streaming, or stops it."));
                }
            }
            widgets::hint(ui, &t!("Set OBS's address and password under Settings."));
        }
        ButtonAction::Profile { id } => {
            let current = profiles
                .iter()
                .find(|(pid, _)| pid == id)
                .map(|(_, n)| n.clone())
                .unwrap_or_else(|| t!("Pick…").into());
            widgets::combo(("profile-action", cid)).selected_text(current).show_ui(ui, |ui| {
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
            widgets::hint(ui, &t!("%{device} (not connected)", device = n));
        }
    }
}

const REPO: &str = "https://git.briggen.dev/NilsBriggen/SlideWire";

fn appearance_card(ui: &mut egui::Ui, s: &mut Settings) {
    use crate::config::{Accent, ThemeMode};
    widgets::card(ui, icon::PALETTE, &t!("Appearance"), |ui| {
        ui.horizontal(|ui| {
            ui.label(t!("Theme"));
            ui.add_space(SP_S);
            widgets::segmented(
                ui,
                &mut s.theme,
                &[
                    (ThemeMode::System, &*t!("System")),
                    (ThemeMode::Dark, &*t!("Dark")),
                    (ThemeMode::Light, &*t!("Light")),
                ],
            );
        });
        ui.horizontal(|ui| {
            ui.label(t!("Accent"));
            ui.add_space(SP_S);
            let system = crate::appearance::system().accent;
            for a in Accent::ALL {
                if a == Accent::System && system.is_none() {
                    continue;
                }
                let c = theme::accent_color(a, system);
                if widgets::swatch(ui, c, s.accent == a, &a.label()).clicked() {
                    s.accent = a;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(t!("Size"));
            ui.add_space(SP_S);
            // Applied when the slider is let go: zooming while dragging
            // would move the slider under the mouse.
            let id = egui::Id::new("ui-scale-drag");
            let mut v = ui.data(|d| d.get_temp::<f32>(id)).unwrap_or(s.ui_scale);
            let r = ui.add(
                egui::Slider::new(&mut v, 0.75..=2.0)
                    .step_by(0.05)
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
            );
            if r.dragged() {
                ui.data_mut(|d| d.insert_temp(id, v));
            } else {
                ui.data_mut(|d| d.remove::<f32>(id));
                if (v - s.ui_scale).abs() > 0.001 {
                    s.ui_scale = v;
                }
            }
        });
        widgets::hint(ui, &t!("Ctrl + and Ctrl − change it too; Ctrl 0 resets."));
        widgets::toggle(ui, &mut s.reduce_motion, &t!("Reduce motion"));
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
    widgets::card(ui, icon::SPEAKER_HIGH, &t!("Volume"), |ui| {
        ui.label(t!("Fine control at low volume"));
        ui.horizontal(|ui| {
            ui.spacing_mut().slider_width = (ui.available_width() - 200.0).clamp(120.0, 320.0);
            ui.add(
                egui::Slider::new(&mut s.curve, 0.5..=3.0)
                    .step_by(0.1)
                    .custom_formatter(|v, _| if (v - 1.0).abs() < 0.05 { t!("off").into() } else { format!("{v:.1}") }),
            );
            ui.add_space(SP_S);
            curve_preview(ui, s.curve);
        });
        widgets::hint(
            ui,
            &t!("Off: faders follow the system volume slider. Higher: more of the fader's travel for quiet volumes."),
        );
        widgets::toggle(ui, &mut s.allow_boost, &t!("Allow boosting above 100% (up to 150%)"));
        widgets::toggle(ui, &mut s.pickup, &t!("Pick up: a fader only takes over once it reaches the volume"));
        widgets::hint(
            ui,
            &t!("Stops jumps when a volume was changed elsewhere. The mixer shows where your fader is in amber."),
        );
    });
}

/// OBS's scene list, fetched in the background for the scene picker.
static OBS_SCENES: std::sync::Mutex<Option<Result<Vec<String>, String>>> = std::sync::Mutex::new(None);

fn fetch_obs_scenes(ctx: &egui::Context) {
    *OBS_SCENES.lock().unwrap() = None;
    let ctx = ctx.clone();
    let (url, password) = crate::dbus::SHARED
        .get()
        .map(|sh| {
            let s = &sh.lock().unwrap().store.settings;
            (s.obs_url.clone(), s.obs_password.clone())
        })
        .unwrap_or_default();
    std::thread::spawn(move || {
        let result = crate::obs::scenes(&url, &password).map_err(|e| format!("{e:#}"));
        *OBS_SCENES.lock().unwrap() = Some(result);
        ctx.request_repaint();
    });
}

/// obs-websocket connection for OBS button actions.
fn obs_card(ui: &mut egui::Ui, s: &mut Settings, events: &crossbeam_channel::Sender<crate::engine::Event>) {
    widgets::card(ui, icon::VIDEO_CAMERA, &t!("OBS Studio"), |ui| {
        widgets::hint(ui, &t!("For OBS buttons. In OBS: Tools → WebSocket Server Settings."));
        egui::Grid::new("obs").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label(t!("Address"));
            ui.add(egui::TextEdit::singleline(&mut s.obs_url).desired_width(220.0));
            ui.end_row();
            ui.label(t!("Password"));
            ui.add(egui::TextEdit::singleline(&mut s.obs_password).password(true).desired_width(220.0));
            ui.end_row();
        });
        ui.horizontal(|ui| {
            if ui.button(format!("{}  {}", icon::PLUGS_CONNECTED, t!("Test connection"))).clicked() {
                let (url, password, events) = (s.obs_url.clone(), s.obs_password.clone(), events.clone());
                std::thread::spawn(move || {
                    let n = match crate::obs::check(&url, &password) {
                        Ok(v) => {
                            crate::engine::Notice::Info(t!("Connected to OBS %{version}", version = v).to_string())
                        }
                        Err(e) => crate::engine::Notice::Problem(e.to_string()),
                    };
                    let _ = events.send(crate::engine::Event::Notice(n));
                });
            }
        });
    });
}

/// Options most people never need.
fn advanced_card(ui: &mut egui::Ui, s: &mut Settings, ports_out: &[String]) {
    widgets::card(ui, icon::WRENCH, &t!("Advanced"), |ui| {
        widgets::toggle(ui, &mut s.led_heartbeat, &t!("LED heartbeat: resend LED states every 5 s"));
        widgets::hint(ui, &t!("For devices that forget their LEDs."));
        ui.horizontal(|ui| {
            ui.label(t!("Encoder clicks from 0 to 100%"));
            ui.add(egui::DragValue::new(&mut s.default_steps).range(5..=400));
        });
        widgets::hint(ui, &t!("For endless knobs whose profile doesn't say."));
        ui.horizontal(|ui| {
            ui.label(t!("Forward all MIDI input to"));
            port_combo(ui, "passthru", &mut s.passthru, ports_out);
        });
    });
}
