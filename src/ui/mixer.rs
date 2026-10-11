//! Mixer page: console strips plus the draggable sources panel.

use std::collections::HashSet;

use eframe::egui::{self, RichText, Sense, Stroke, StrokeKind, Vec2, vec2};

use super::App;
use super::theme::{icon, *};
use super::widgets;
use crate::audio::{AudioState, Kind, MeterKey};
use crate::engine::{StripView, UiCmd};
use crate::profile::{AppMatch, Target, Widget};

const STRIP_W: f32 = 112.0;
const FADER_H: f32 = 400.0;
const KNOB_H: f32 = 236.0;

impl App {
    pub(super) fn mixer(&mut self, ui: &mut egui::Ui) {
        let (strips, audio) = {
            let sh = self.shared.lock().unwrap();
            (sh.strips.clone(), sh.audio.clone())
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new("Console").font(semibold(17.0)));
            widgets::hint(ui, "Drag apps and devices from the right onto a strip, or use the S button.");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if self.show_sources { "Hide sources" } else { "Show sources" };
                if widgets::icon_button(ui, icon::SQUARES_FOUR, label).clicked() {
                    self.show_sources = !self.show_sources;
                }
            });
        });
        ui.add_space(6.0);
        if strips.is_empty() {
            widgets::card(ui, icon::INFO, "No active profile", |ui| {
                ui.label("Create or import a controller profile under Profiles.");
            });
            return;
        }
        let (knobs, faders): (Vec<&StripView>, Vec<&StripView>) = strips.iter().partition(|s| s.knob);
        egui::ScrollArea::both().id_salt("console").auto_shrink([false, false]).show(ui, |ui| {
            for (row, label, height) in [(&knobs, "Rotary", KNOB_H), (&faders, "Faders", FADER_H)] {
                if row.is_empty() {
                    continue;
                }
                ui.label(RichText::new(label.to_uppercase()).size(11.0).color(TEXT_FAINT));
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    for s in row.iter() {
                        self.strip(ui, s, &audio, height);
                    }
                });
                ui.add_space(14.0);
            }
        });
    }

    fn strip(&mut self, ui: &mut egui::Ui, s: &StripView, audio: &AudioState, height: f32) {
        let (rect, response) = ui.allocate_exact_size(vec2(STRIP_W, height), Sense::hover());
        let painter = ui.painter_at(rect.expand(12.0));
        widgets::bevel_panel(&painter, rect, 8, STRIP_TOP, STRIP_BOTTOM);

        // Drop target for dragged sources.
        let hovering = response.dnd_hover_payload::<Target>().is_some();
        if hovering {
            widgets::glow(&painter, rect, 8.0, CYAN.gamma_multiply(0.35), 14.0);
            painter.rect_stroke(rect, 8.0, Stroke::new(2.0, CYAN), StrokeKind::Inside);
        }
        if let Some(t) = response.dnd_release_payload::<Target>() {
            self.add_target(&s.id, (*t).clone());
        }

        let inner = rect.shrink2(vec2(8.0, 10.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.vertical_centered(|ui| {
                ui.add(egui::Label::new(RichText::new(&s.name).font(semibold(13.5))).truncate());
                let text = if !s.resolved.is_empty() {
                    s.resolved.join(" + ")
                } else if !s.targets.is_empty() {
                    s.targets.iter().map(|t| t.label()).collect::<Vec<_>>().join(" + ")
                } else {
                    "— empty —".into()
                };
                let img = self
                    .icons
                    .image(&s.icon_hint)
                    .or_else(|| s.targets.first().and_then(|t| self.icons.image(&[target_icon_name(t).to_string()])));
                let dim = s.muted || s.resolved.is_empty();
                let r = widgets::lcd(ui, vec2(inner.width(), 30.0), img, &text, dim);
                if !s.targets.is_empty() {
                    r.on_hover_text(&text);
                }

                let led = vec2(28.0, 22.0);
                let (mut mute, mut run) = (false, false);
                let mut s_resp = None;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.add_space((inner.width() - led.x * 3.0 - 12.0) / 2.0);
                    s_resp = Some(widgets::led_button(ui, "S", s.assigned, ASSIGN, led).on_hover_text("Assign"));
                    mute = widgets::led_button(ui, "M", s.muted, MUTE, led).on_hover_text("Mute").clicked();
                    run = widgets::led_button(ui, "R", s.running, RUN, led)
                        .on_hover_text("Run: make the device default / start the app")
                        .clicked();
                });
                if mute {
                    self.send(UiCmd::ToggleGroupMute(s.id.clone()));
                }
                if run {
                    self.send(UiCmd::PressRun(s.id.clone()));
                }
                if let Some(r) = &s_resp {
                    egui::Popup::menu(r)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| self.assign_menu(ui, s, audio));
                }

                // Peak hold: decays after 1.5 s.
                let now = ui.input(|i| i.time);
                let hold = {
                    let e = self.meter_hold.entry(s.id.clone()).or_insert((0.0, now));
                    if s.peak >= e.0 || now - e.1 > 1.5 {
                        *e = (s.peak, now);
                    }
                    e.0
                };

                let color = if s.muted { CYAN.gamma_multiply(0.35) } else { CYAN };
                let mut pos = s.position;
                let changed = if s.knob {
                    ui.horizontal(|ui| {
                        ui.add_space((inner.width() - 74.0 - 8.0 - 8.0) / 2.0);
                        let r = widgets::knob(ui, &mut pos, 74.0, color);
                        widgets::segment_meter(ui, s.peak, hold, vec2(8.0, 74.0));
                        r.changed()
                    })
                    .inner
                } else {
                    let h = ui.available_height() - 34.0;
                    ui.horizontal(|ui| {
                        ui.add_space((inner.width() - 52.0 - 10.0 - 8.0) / 2.0);
                        let r = widgets::fader(ui, &mut pos, vec2(52.0, h), color);
                        widgets::segment_meter(ui, s.peak, hold, vec2(10.0, h - 20.0));
                        r.changed()
                    })
                    .inner
                };
                if changed {
                    self.send(UiCmd::SetGroupPosition(s.id.clone(), pos));
                }
                let readout = if s.muted { "MUTE".to_string() } else { format!("{:>3.0}%", s.position * 100.0) };
                ui.label(RichText::new(readout).font(mono(13.0)).color(if s.muted { MUTE } else { LCD_TEXT }));
            });
        });

        response.context_menu(|ui| self.assign_menu(ui, s, audio));
    }

    fn assign_menu(&mut self, ui: &mut egui::Ui, s: &StripView, audio: &AudioState) {
        ui.set_min_width(260.0);
        ui.label(RichText::new(format!("{}  {}", icon::TARGET, s.name)).font(semibold(14.0)));
        if s.targets.is_empty() {
            widgets::hint(ui, "Nothing assigned yet.");
        }
        let mut remove = None;
        for (i, t) in s.targets.iter().enumerate() {
            ui.horizontal(|ui| {
                if let Some(img) = self.icons.image(&[target_icon_name(t).to_string()]) {
                    ui.add(img.fit_to_exact_size(Vec2::splat(18.0)));
                }
                ui.label(t.label());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(icon::X).on_hover_text("Remove").clicked() {
                        remove = Some(i);
                    }
                });
            });
        }
        if let Some(i) = remove {
            let gid = s.id.clone();
            self.edit_profile(|p| {
                if let Some(a) = p.assignments.get_mut(&gid) {
                    a.targets.remove(i);
                }
            });
        }
        ui.separator();
        self.target_menu(ui, &s.id, audio);
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Show as");
            let gid = s.id.clone();
            if ui.selectable_label(!s.knob, "Fader").clicked() {
                self.edit_profile(|p| p.assignments.entry(gid.clone()).or_default().widget = Some(Widget::Fader));
            }
            if ui.selectable_label(s.knob, "Knob").clicked() {
                self.edit_profile(|p| p.assignments.entry(gid).or_default().widget = Some(Widget::Knob));
            }
        });
    }

    pub(super) fn add_target(&self, gid: &str, t: Target) {
        let gid = gid.to_string();
        self.edit_profile(|p| {
            let a = p.assignments.entry(gid).or_default();
            if !a.targets.contains(&t) {
                a.targets.push(t);
            }
        });
    }

    fn target_menu(&mut self, ui: &mut egui::Ui, gid: &str, audio: &AudioState) {
        for t in [Target::Focus, Target::DefaultOutput, Target::DefaultInput, Target::Unassigned, Target::EventSounds] {
            if ui.button(format!("{}  {}", target_glyph(&t), t.label())).clicked() {
                self.add_target(gid, t);
                ui.close();
            }
        }
        ui.menu_button(format!("{}  Application", icon::MUSIC_NOTES), |ui| {
            let mut seen = HashSet::new();
            for a in &audio.apps {
                let pattern = if a.binary.is_empty() { a.app_name.clone() } else { a.binary.clone() };
                if pattern.is_empty() || !seen.insert(pattern.clone()) {
                    continue;
                }
                if ui.button(a.display_name()).clicked() {
                    self.add_target(
                        gid,
                        Target::App { m: AppMatch { pattern, regex: false }, label: a.display_name() },
                    );
                    ui.close();
                }
            }
            if seen.is_empty() {
                widgets::hint(ui, "No apps are playing audio");
            }
        });
        ui.menu_button(format!("{}  Application recording", icon::MICROPHONE), |ui| {
            let mut seen = HashSet::new();
            for a in &audio.app_inputs {
                let pattern = if a.binary.is_empty() { a.app_name.clone() } else { a.binary.clone() };
                if pattern.is_empty() || !seen.insert(pattern.clone()) {
                    continue;
                }
                if ui.button(a.display_name()).clicked() {
                    self.add_target(
                        gid,
                        Target::AppInput { m: AppMatch { pattern, regex: false }, label: a.display_name() },
                    );
                    ui.close();
                }
            }
            if seen.is_empty() {
                widgets::hint(ui, "No apps are recording");
            }
        });
        ui.menu_button(format!("{}  Output device", icon::SPEAKER_HIGH), |ui| {
            for d in &audio.sinks {
                if ui.button(&d.description).clicked() {
                    self.add_target(gid, Target::Output { name: d.name.clone(), label: d.description.clone() });
                    ui.close();
                }
            }
        });
        ui.menu_button(format!("{}  Input device", icon::MICROPHONE_STAGE), |ui| {
            for d in &audio.sources {
                if ui.button(&d.description).clicked() {
                    self.add_target(gid, Target::Input { name: d.name.clone(), label: d.description.clone() });
                    ui.close();
                }
            }
        });
        ui.add_space(4.0);
        widgets::hint(ui, "Match apps by name (* wildcard):");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.custom_pattern).desired_width(140.0).hint_text("firefox*"));
            ui.checkbox(&mut self.custom_regex, "regex");
            if ui.button(icon::PLUS).clicked() && !self.custom_pattern.trim().is_empty() {
                let pattern = self.custom_pattern.trim().to_string();
                self.add_target(
                    gid,
                    Target::App { m: AppMatch { pattern: pattern.clone(), regex: self.custom_regex }, label: pattern },
                );
                self.custom_pattern.clear();
                ui.close();
            }
        });
        let current_max = {
            let sh = self.shared.lock().unwrap();
            sh.active().and_then(|p| p.assignments.get(gid)).map(|a| a.max_volume).unwrap_or(1.0)
        };
        let mut max = current_max;
        ui.horizontal(|ui| {
            ui.label("Volume limit");
            ui.add(egui::Slider::new(&mut max, 0.05..=1.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
        });
        if (max - current_max).abs() > f32::EPSILON {
            let gid = gid.to_string();
            self.edit_profile(|p| p.assignments.entry(gid).or_default().max_volume = max);
        }
    }

    // ------------------------------------------------------------ sources

    pub(super) fn sources(&mut self, ui: &mut egui::Ui) {
        let (audio, boost, peaks) = {
            let sh = self.shared.lock().unwrap();
            (sh.audio.clone(), sh.store.settings.allow_boost, sh.source_peaks.clone())
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new("Sources").font(semibold(17.0)));
        });
        widgets::hint(ui, &format!("{}  Drag a row by its handle onto a strip to assign it.", icon::DOTS_SIX_VERTICAL));
        ui.add_space(6.0);
        let max = if boost { 1.5 } else { 1.0 };
        egui::ScrollArea::vertical().id_salt("sources").auto_shrink([false, false]).show(ui, |ui| {
            widgets::card(ui, icon::MUSIC_NOTES, "Applications", |ui| {
                if audio.apps.is_empty() {
                    widgets::hint(ui, "Nothing is playing.");
                }
                for a in &audio.apps {
                    let pattern = if a.binary.is_empty() { a.app_name.clone() } else { a.binary.clone() };
                    let target = Target::App { m: AppMatch { pattern, regex: false }, label: a.display_name() };
                    let hints = vec![a.icon.clone(), a.binary.clone(), a.app_name.clone()];
                    let sub =
                        (!a.media_name.is_empty() && a.media_name != a.display_name()).then(|| a.media_name.clone());
                    let peak = peaks.get(&MeterKey::SinkInput(a.index)).copied().unwrap_or(0.0);
                    self.source_row(
                        ui,
                        Kind::SinkInput,
                        a.index,
                        &a.display_name(),
                        sub,
                        &hints,
                        a.volume,
                        a.mute,
                        None,
                        peak,
                        max,
                        target,
                    );
                }
            });
            ui.add_space(8.0);
            if !audio.app_inputs.is_empty() {
                widgets::card(ui, icon::MICROPHONE, "Recording", |ui| {
                    for a in &audio.app_inputs {
                        let pattern = if a.binary.is_empty() { a.app_name.clone() } else { a.binary.clone() };
                        let target =
                            Target::AppInput { m: AppMatch { pattern, regex: false }, label: a.display_name() };
                        let hints = vec![a.icon.clone(), a.binary.clone(), a.app_name.clone()];
                        self.source_row(
                            ui,
                            Kind::SourceOutput,
                            a.index,
                            &a.display_name(),
                            None,
                            &hints,
                            a.volume,
                            a.mute,
                            None,
                            0.0,
                            max,
                            target,
                        );
                    }
                });
                ui.add_space(8.0);
            }
            widgets::card(ui, icon::SPEAKER_HIGH, "Outputs", |ui| {
                for d in &audio.sinks {
                    let target = Target::Output { name: d.name.clone(), label: d.description.clone() };
                    let hints = vec![d.icon.clone(), "audio-speakers".into()];
                    let peak = peaks.get(&MeterKey::Sink(d.name.clone())).copied().unwrap_or(0.0);
                    let default = Some((d.name == audio.default_sink, d.name.clone()));
                    self.source_row(
                        ui,
                        Kind::Sink,
                        d.index,
                        &d.label,
                        Some(d.detail.clone()).filter(|s| !s.is_empty()),
                        &hints,
                        d.volume,
                        d.mute,
                        default,
                        peak,
                        max,
                        target,
                    );
                }
            });
            ui.add_space(8.0);
            widgets::card(ui, icon::MICROPHONE_STAGE, "Inputs", |ui| {
                for d in &audio.sources {
                    let target = Target::Input { name: d.name.clone(), label: d.description.clone() };
                    let hints = vec![d.icon.clone(), "audio-input-microphone".into()];
                    let peak = peaks.get(&MeterKey::Source(d.name.clone())).copied().unwrap_or(0.0);
                    let default = Some((d.name == audio.default_source, d.name.clone()));
                    self.source_row(
                        ui,
                        Kind::Source,
                        d.index,
                        &d.label,
                        Some(d.detail.clone()).filter(|s| !s.is_empty()),
                        &hints,
                        d.volume,
                        d.mute,
                        default,
                        peak,
                        max,
                        target,
                    );
                }
            });
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn source_row(
        &mut self,
        ui: &mut egui::Ui,
        kind: Kind,
        index: u32,
        name: &str,
        sub: Option<String>,
        hints: &[String],
        volume: f32,
        mute: bool,
        default: Option<(bool, String)>,
        peak: f32,
        max: f32,
        target: Target,
    ) {
        let img = self.icons.image(hints);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            // Drag handle: icon + name.
            let id = ui.id().with(("src", kind as u8, index));
            ui.dnd_drag_source(id, target, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(icon::DOTS_SIX_VERTICAL).color(TEXT_FAINT));
                    match &img {
                        Some(i) => {
                            ui.add(i.clone().fit_to_exact_size(Vec2::splat(22.0)));
                        }
                        None => {
                            ui.label(RichText::new(icon::WAVEFORM).size(18.0).color(TEXT_DIM));
                        }
                    }
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.set_width(146.0);
                        ui.add(egui::Label::new(RichText::new(name).size(13.0)).truncate());
                        if let Some(sub) = &sub {
                            ui.add(egui::Label::new(RichText::new(sub).size(11.0).color(TEXT_DIM)).truncate());
                        }
                    });
                });
            })
            .response
            .on_hover_text(match &sub {
                Some(sub) => format!("{name}\n{sub}"),
                None => name.to_string(),
            })
            .on_hover_cursor(egui::CursorIcon::Grab);

            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.spacing_mut().slider_width = (ui.available_width() - 60.0).max(60.0);
                let mut v = volume;
                let r = ui.add(egui::Slider::new(&mut v, 0.0..=max.max(volume)).show_value(false).trailing_fill(true));
                if r.changed() {
                    self.send(UiCmd::SetStreamVolume(kind, index, v));
                }
                r.on_hover_text(format!("{:.0}%", volume * 100.0));
                widgets::bar_meter(ui, peak, vec2(ui.spacing().slider_width, 3.0));
            });
            if let Some((is_default, dev_name)) = &default {
                let star = if *is_default {
                    RichText::new(icon::STAR).color(ASSIGN)
                } else {
                    RichText::new(icon::STAR).color(TEXT_FAINT)
                };
                if ui
                    .add(egui::Button::new(star).frame(false))
                    .on_hover_text(if *is_default { "Default device" } else { "Make default" })
                    .clicked()
                    && !is_default
                {
                    self.send(UiCmd::SetDefault(kind, dev_name.clone()));
                }
            }
            let m = if mute {
                RichText::new(icon::SPEAKER_SLASH).color(MUTE)
            } else {
                RichText::new(icon::SPEAKER_HIGH).color(TEXT_DIM)
            };
            if ui.add(egui::Button::new(m).frame(false)).on_hover_text(if mute { "Unmute" } else { "Mute" }).clicked() {
                self.send(UiCmd::SetStreamMute(kind, index, !mute));
            }
        });
        ui.add_space(2.0);
    }
}

/// Icon theme name representing a target type.
pub(super) fn target_icon_name(t: &Target) -> &'static str {
    match t {
        Target::Focus => "window",
        Target::App { .. } | Target::Unassigned => "applications-multimedia",
        Target::AppInput { .. } => "audio-input-microphone",
        Target::EventSounds => "preferences-desktop-notification-bell",
        Target::Output { .. } | Target::DefaultOutput => "audio-speakers",
        Target::Input { .. } | Target::DefaultInput => "audio-input-microphone",
    }
}

/// Phosphor glyph for a target type.
pub(super) fn target_glyph(t: &Target) -> &'static str {
    match t {
        Target::Focus => icon::CROSSHAIR,
        Target::App { .. } => icon::MUSIC_NOTES,
        Target::AppInput { .. } => icon::MICROPHONE,
        Target::Unassigned => icon::SQUARES_FOUR,
        Target::EventSounds => icon::BELL,
        Target::Output { .. } | Target::DefaultOutput => icon::SPEAKER_HIGH,
        Target::Input { .. } | Target::DefaultInput => icon::MICROPHONE_STAGE,
    }
}
