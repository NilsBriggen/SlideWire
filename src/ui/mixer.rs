//! Mixer page: console strips plus the draggable sources panel.

use std::collections::HashSet;

use eframe::egui::{self, RichText, Sense, Stroke, StrokeKind, Vec2, vec2};

use super::App;
use super::theme::{self, icon, *};
use super::widgets;
use crate::audio::{AudioState, Kind, MeterKey};
use crate::engine::{StripView, UiCmd};
use crate::mapping;
use crate::profile::{AppMatch, Target, Widget};

const STRIP_MIN_W: f32 = 92.0;
const STRIP_MAX_W: f32 = 168.0;
const GAP: f32 = 10.0;
const MASTER_W: f32 = 104.0;
const FADER_H: f32 = 400.0;
const KNOB_H: f32 = 236.0;
/// Height of the knob section in a channel strip.
const CHANNEL_KNOB_H: f32 = 140.0;

/// A hardware channel: the knob and the fader with the same number.
struct Channel<'a> {
    no: u32,
    knob: &'a StripView,
    fader: &'a StripView,
}

fn channel_no(name: &str) -> Option<u32> {
    name.rsplit(' ').next()?.parse().ok()
}

/// Pair knobs and faders by the number in their names ("Knob 3" + "Fader 3").
/// Strips without a partner are returned separately.
fn pair_channels(strips: &[StripView]) -> (Vec<Channel<'_>>, Vec<&StripView>) {
    let mut channels = Vec::new();
    let mut used = HashSet::new();
    for f in strips.iter().filter(|s| !s.knob) {
        let Some(no) = channel_no(&f.name) else { continue };
        if let Some(k) = strips.iter().find(|s| s.knob && !used.contains(&s.id) && channel_no(&s.name) == Some(no)) {
            used.insert(k.id.clone());
            used.insert(f.id.clone());
            channels.push(Channel { no, knob: k, fader: f });
        }
    }
    channels.sort_by_key(|c| c.no);
    let rest = strips.iter().filter(|s| !used.contains(&s.id)).collect();
    (channels, rest)
}

/// Strip width that fills `avail` with `n` strips.
fn strip_width(avail: f32, n: usize) -> f32 {
    let n = n.max(1) as f32;
    ((avail - GAP * (n - 1.0)) / n).clamp(STRIP_MIN_W, STRIP_MAX_W)
}

/// PulseAudio volume (cubic scale) in dB.
fn db_text(vol: f32) -> String {
    if vol <= 0.0005 { "-inf dB".into() } else { format!("{:.1} dB", 60.0 * vol.log10()) }
}

impl App {
    pub(super) fn mixer(&mut self, ui: &mut egui::Ui) {
        let (strips, audio) = {
            let sh = self.shared.lock().unwrap();
            (sh.strips.clone(), sh.audio.clone())
        };
        let (bank, bank_count, scenes) = {
            let sh = self.shared.lock().unwrap();
            sh.active().map(|p| (p.bank, p.bank_count(), p.scenes.keys().cloned().collect::<Vec<_>>())).unwrap_or((
                0,
                1,
                Vec::new(),
            ))
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new("Console").font(semibold(17.0)));
            ui.add_space(6.0);
            self.bank_tabs(ui, bank, bank_count);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if self.show_sources { "Hide sources" } else { "Show sources" };
                if widgets::icon_button(ui, icon::SQUARES_FOUR, label).clicked() {
                    self.show_sources = !self.show_sources;
                }
                self.scenes_menu(ui, &scenes);
            });
        });
        ui.add_space(6.0);
        if strips.is_empty() {
            widgets::card(ui, icon::INFO, "No active profile", |ui| {
                ui.label("Create or import a controller profile under Profiles.");
            });
            return;
        }
        let dragging = egui::DragAndDrop::has_payload_of_type::<Target>(ui.ctx());
        let (channels, rest) = pair_channels(&strips);
        // Measured outside the scroll area, which would report its scrollbars' space too.
        let avail = ui.available_width();
        let avail_h = ui.available_height();
        egui::ScrollArea::both().id_salt("console").auto_shrink([false, false]).show(ui, |ui| {
            if !channels.is_empty() {
                // The master strip only when it fits next to full-size channels.
                let n = channels.len() as f32;
                let show_master = avail >= n * STRIP_MIN_W + (n - 1.0) * GAP + 2.0 * GAP + MASTER_W;
                let w = strip_width(avail - if show_master { MASTER_W + GAP * 2.0 } else { 0.0 }, channels.len());
                let h = (avail_h - if rest.is_empty() { 4.0 } else { 280.0 }).max(520.0);
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = GAP;
                    for c in &channels {
                        self.channel_strip(ui, c, &audio, vec2(w, h), dragging);
                    }
                    if show_master {
                        ui.add_space(GAP);
                        self.master_strip(ui, &audio, vec2(MASTER_W, h));
                    }
                });
                if !rest.is_empty() {
                    ui.add_space(14.0);
                }
            }
            // Other controllers: one row per kind of strip.
            let (knobs, faders): (Vec<&StripView>, Vec<&StripView>) = rest.into_iter().partition(|s| s.knob);
            for (row, label, height) in [(&knobs, "Rotary", KNOB_H), (&faders, "Faders", FADER_H)] {
                if row.is_empty() {
                    continue;
                }
                ui.label(RichText::new(label.to_uppercase()).size(11.0).color(pal().text_faint));
                let w = strip_width(avail, row.len());
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = GAP;
                    for s in row.iter() {
                        let (rect, _) = ui.allocate_exact_size(vec2(w, height), Sense::hover());
                        widgets::bevel_panel(
                            &ui.painter_at(rect.expand(12.0)),
                            rect,
                            8,
                            pal().strip_top,
                            pal().strip_bottom,
                        );
                        let title = s.label.clone().unwrap_or_else(|| s.name.clone());
                        self.section(ui, s, rect, &audio, false, Some(&title), dragging);
                    }
                });
                ui.add_space(14.0);
            }
        });
    }

    /// "Bank 1 | Bank 2 | +": each bank has its own assignments.
    fn bank_tabs(&mut self, ui: &mut egui::Ui, bank: usize, count: usize) {
        ui.spacing_mut().item_spacing.x = 4.0;
        for i in 0..count {
            let r = ui
                .selectable_label(i == bank, format!("Bank {}", i + 1))
                .on_hover_text("Each bank has its own assignments. Right-click to clear or remove.");
            if r.clicked() && i != bank {
                self.send(UiCmd::SetBank(i));
            }
            r.context_menu(|ui| {
                let label = if i == 0 { "Clear bank 1" } else { "Remove this bank" };
                if ui.button(format!("{}  {label}", icon::TRASH)).clicked() {
                    self.confirm = Some(super::Confirm {
                        title: format!("{label}?"),
                        body: format!("Everything assigned in bank {} is removed.", i + 1),
                        button: label.into(),
                        action: super::ConfirmAction::RemoveBank(i),
                        alt: None,
                    });
                    ui.close();
                }
            });
        }
        if ui.small_button(icon::PLUS).on_hover_text("Add a bank").clicked() {
            self.edit_profile(|p| {
                p.banks.push(Default::default());
                p.bank = p.bank_count() - 1;
            });
        }
        if count == 1 {
            widgets::hint(ui, "Drag apps and devices from the right onto a strip, or use the S button.");
        }
    }

    /// Recall, save and delete scenes (volume and mute snapshots).
    fn scenes_menu(&mut self, ui: &mut egui::Ui, scenes: &[String]) {
        let button = widgets::icon_button(ui, icon::BOOKMARKS_SIMPLE, "Scenes");
        // Stays open while typing a name (a plain menu closes on any click inside).
        egui::Popup::menu(&button).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_width(260.0);
            if scenes.is_empty() {
                widgets::hint(ui, "Save the current volumes and mutes to recall them later.");
            }
            let mut delete = None;
            for name in scenes {
                ui.horizontal(|ui| {
                    if ui.button(format!("{}  {name}", icon::PLAY)).on_hover_text("Recall").clicked() {
                        self.send(UiCmd::RecallScene(name.clone()));
                        ui.close();
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button(icon::TRASH).on_hover_text("Delete").clicked() {
                            delete = Some(name.clone());
                        }
                        if ui
                            .small_button(icon::FLOPPY_DISK)
                            .on_hover_text("Overwrite with the current volumes")
                            .clicked()
                        {
                            self.send(UiCmd::SaveScene(name.clone()));
                            self.flash(format!("Saved scene \"{name}\""));
                        }
                    });
                });
            }
            if let Some(name) = delete {
                self.edit_profile(|p| {
                    p.scenes.shift_remove(&name);
                });
                self.toast(icon::TRASH, format!("Deleted scene \"{name}\""), None, Some(super::ToastUndo::Edit));
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.scene_name).hint_text("New scene name").desired_width(150.0),
                );
                let name = self.scene_name.trim().to_string();
                if ui.add_enabled(!name.is_empty(), egui::Button::new("Save")).clicked() {
                    self.send(UiCmd::SaveScene(name.clone()));
                    self.flash(format!("Saved scene \"{name}\""));
                    self.scene_name.clear();
                    ui.close();
                }
            });
            widgets::hint(ui, "Buttons can recall scenes too (Buttons page).");
        });
    }

    /// Knob on top, fader below, like the channel on the hardware.
    fn channel_strip(&mut self, ui: &mut egui::Ui, c: &Channel, audio: &AudioState, size: Vec2, dragging: bool) {
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let painter = ui.painter_at(rect.expand(12.0));
        widgets::bevel_panel(&painter, rect, 8, pal().strip_top, pal().strip_bottom);
        let head = egui::Rect::from_min_size(rect.min, vec2(rect.width(), 28.0));
        let label = c.fader.label.as_ref().or(c.knob.label.as_ref());
        let color = c.fader.color.or(c.knob.color);
        if let Some(i) = color {
            // A coloured band across the top, like tape on a real console.
            let band = egui::Rect::from_min_size(rect.min + vec2(10.0, 4.0), vec2(rect.width() - 20.0, 3.0));
            widgets::glow(&painter, band, 1.5, theme::strip_color(Some(i)).gamma_multiply(0.5), 6.0);
            painter.rect_filled(band, 1.5, theme::strip_color(Some(i)));
        }
        strip_header(&painter, head.shrink2(vec2(8.0, 0.0)), c.no, label.map(String::as_str));
        if self.selected_channel == Some(c.no) {
            // Picked with the number keys: arrows and M act on it.
            painter.rect_stroke(rect.expand(2.0), 10.0, Stroke::new(2.0, pal().accent), StrokeKind::Outside);
        }
        let split = head.bottom() + CHANNEL_KNOB_H;
        let knob_zone =
            egui::Rect::from_min_max(egui::pos2(rect.left(), head.bottom()), egui::pos2(rect.right(), split));
        let fader_zone = egui::Rect::from_min_max(egui::pos2(rect.left(), split), rect.max);
        // Engraved divider.
        let (l, r) = (rect.left() + 10.0, rect.right() - 10.0);
        painter.line_segment([egui::pos2(l, split), egui::pos2(r, split)], Stroke::new(1.0, pal().shadow(140)));
        painter
            .line_segment([egui::pos2(l, split + 1.0), egui::pos2(r, split + 1.0)], Stroke::new(1.0, pal().sheen(10)));
        self.section(ui, c.knob, knob_zone, audio, true, None, dragging);
        self.section(ui, c.fader, fader_zone, audio, false, None, dragging);
    }

    /// The default output, always at the right end of the console.
    fn master_strip(&mut self, ui: &mut egui::Ui, audio: &AudioState, size: Vec2) {
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let painter = ui.painter_at(rect.expand(12.0));
        widgets::bevel_panel(&painter, rect, 8, pal().strip_top, pal().strip_bottom);
        let head = egui::Rect::from_min_size(rect.min, vec2(rect.width(), 28.0));
        painter.text(head.center(), egui::Align2::CENTER_CENTER, "Master", semibold(14.0), pal().text);
        let Some(sink) = audio.sinks.iter().find(|d| d.name == audio.default_sink) else {
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, "No output", mono(11.0), pal().text_faint);
            return;
        };
        let (curve, boost, peak) = {
            let sh = self.shared.lock().unwrap();
            let s = &sh.store.settings;
            let peak = sh.source_peaks.get(&MeterKey::Sink(sink.name.clone())).copied().unwrap_or(0.0);
            (s.curve, if s.allow_boost { 1.5 } else { 1.0 }, peak)
        };
        let inner = egui::Rect::from_min_max(head.left_bottom() + vec2(8.0, 6.0), rect.max - vec2(8.0, 10.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
        let ui = &mut child;
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.vertical_centered(|ui| {
            // Pick the output from the LCD.
            let img = self.icons.image(&[sink.icon.clone(), "audio-speakers".into()]);
            let lcd = widgets::lcd(ui, vec2(inner.width(), 30.0), img, &sink.label, sink.mute, None)
                .interact(Sense::click())
                .on_hover_text(format!("{}\nClick to choose the default output", sink.description))
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            egui::Popup::menu(&lcd).show(|ui| {
                ui.label(RichText::new("Default output").font(semibold(13.0)));
                for d in &audio.sinks {
                    let on = d.name == sink.name;
                    let text = format!("{}  {}", if on { icon::CHECK } else { "    " }, d.label);
                    if ui.selectable_label(on, text).on_hover_text(&d.description).clicked() && !on {
                        self.send(UiCmd::SetDefault(Kind::Sink, d.name.clone()));
                        ui.close();
                    }
                }
            });
            let led = vec2(36.0, 22.0);
            if widgets::led_button(ui, "M", sink.mute, MUTE, led).on_hover_text("Mute the output").clicked() {
                self.send(UiCmd::SetStreamMute(Kind::Sink, sink.index, !sink.mute));
            }
            let h = ui.available_height() - 46.0;
            let mut pos = mapping::inverse_curve(sink.volume / boost, curve);
            let changed = ui
                .horizontal(|ui| {
                    ui.add_space((inner.width() - 52.0 - 10.0 - 8.0) / 2.0);
                    let color = if sink.mute { pal().accent.gamma_multiply(0.35) } else { pal().accent };
                    let marks = mapping::db_marks(&mapping::Range { min: 0.0, top: boost, curve });
                    let r = widgets::fader(ui, &mut pos, vec2(52.0, h), color, None, &marks);
                    widgets::segment_meter(ui, peak, 0.0, vec2(10.0, h - 20.0));
                    r.changed()
                })
                .inner;
            if changed {
                self.send(UiCmd::SetStreamVolume(Kind::Sink, sink.index, mapping::curve(pos, curve) * boost));
            }
            let text = if sink.mute { "MUTE".to_string() } else { format!("{:>3.0}%", sink.volume * 100.0) };
            let color = if sink.mute { pal().mute_text } else { pal().readout };
            ui.label(RichText::new(text).font(mono(13.0)).color(color));
            if !sink.mute {
                ui.add_space(-6.0);
                ui.label(RichText::new(db_text(sink.volume)).font(mono(10.5)).color(pal().text_faint));
            }
        });
    }

    /// One strip's controls inside `zone`: a drop target with display, buttons,
    /// knob or fader, meter and readout. `compact` is the knob part of a
    /// channel strip.
    #[allow(clippy::too_many_arguments)]
    fn section(
        &mut self,
        ui: &mut egui::Ui,
        s: &StripView,
        zone: egui::Rect,
        audio: &AudioState,
        compact: bool,
        title: Option<&str>,
        dragging: bool,
    ) {
        let response = ui.interact(zone, ui.id().with(("zone", &s.id)), Sense::click());
        let painter = ui.painter_at(zone.expand(12.0));
        let empty = s.targets.is_empty();
        let hovering = response.dnd_hover_payload::<Target>().is_some();
        let hover_t = theme::anim(ui.ctx(), response.id.with("drop"), if hovering { 1.0 } else { 0.0 }, 0.12);
        if hover_t > 0.0 {
            widgets::glow(&painter, zone.shrink(2.0), 8.0, pal().accent.gamma_multiply(0.35 * hover_t), 14.0);
            painter.rect_stroke(
                zone.shrink(2.0),
                8.0,
                Stroke::new(2.0, pal().accent.gamma_multiply(hover_t)),
                StrokeKind::Inside,
            );
        } else if dragging {
            // Free drop targets breathe while something is being dragged.
            let t = ui.input(|i| i.time) as f32;
            let breathe = if s.targets.is_empty() { 0.3 + 0.2 * (t * 4.0).sin() } else { 0.25 };
            painter.rect_stroke(
                zone.shrink(3.0),
                7.0,
                Stroke::new(1.0, pal().accent.gamma_multiply(breathe)),
                StrokeKind::Inside,
            );
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
        }
        if let Some(t) = response.dnd_release_payload::<Target>() {
            self.add_target(&s.id, (*t).clone());
        }

        if s.muted {
            painter.rect_filled(zone.shrink(2.0), 8.0, MUTE.gamma_multiply(0.07));
        }
        let inner = if compact { zone.shrink2(vec2(8.0, 6.0)) } else { zone.shrink2(vec2(8.0, 10.0)) };
        // A detached child, so drawing inside the zone doesn't move the parent's cursor.
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
        {
            let ui = &mut child;
            ui.spacing_mut().item_spacing.y = if compact { 6.0 } else { 8.0 };
            ui.vertical_centered(|ui| {
                if let Some(title) = title {
                    ui.add(egui::Label::new(RichText::new(title).font(semibold(13.5))).truncate());
                }
                let text = if !s.resolved.is_empty() {
                    join_counted(&s.resolved)
                } else if !s.targets.is_empty() {
                    s.targets.iter().map(|t| t.label()).collect::<Vec<_>>().join(" + ")
                } else if dragging {
                    "drop here".into()
                } else {
                    "empty".into()
                };
                let img = s
                    .icon
                    .as_ref()
                    .and_then(|i| self.icons.image(std::slice::from_ref(i)))
                    .or_else(|| self.icons.image(&s.icon_hint))
                    .or_else(|| s.targets.first().and_then(|t| self.icons.image(&[target_icon_name(t).to_string()])));
                // Faders show what they control as big icons; the LCD then has text only.
                let img = if compact { img } else { self.strip_icons(ui, s, inner.width(), img) };
                let dim = s.muted || s.resolved.is_empty();
                let tint = if s.muted { Some(MUTE) } else { s.color.map(|i| theme::strip_color(Some(i))) };
                let size = vec2(inner.width(), if compact { 24.0 } else { 30.0 });
                let r = widgets::lcd(ui, size, img, &text, dim, tint);
                if !empty {
                    r.on_hover_text(&text);
                }

                let hold = self.peak_hold(ui, s);
                let color = theme::strip_color(s.color);
                let color = if s.muted { color.gamma_multiply(0.35) } else { color };
                let mut pos = s.position;
                if compact {
                    let d = 58.0;
                    let changed = ui
                        .horizontal(|ui| {
                            ui.add_space((inner.width() - d - 6.0 - 8.0) / 2.0);
                            let r = widgets::knob(ui, &mut pos, d, color, s.pickup);
                            widgets::segment_meter(ui, s.peak, hold, vec2(6.0, d));
                            r.changed()
                        })
                        .inner;
                    if changed {
                        self.send(UiCmd::SetGroupPosition(s.id.clone(), pos));
                    }
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let narrow = inner.width() < 96.0;
                        let small = vec2(if narrow { 18.0 } else { 22.0 }, 18.0);
                        let text_w = if narrow { 34.0 } else { 52.0 };
                        ui.add_space(((inner.width() - 2.0 * small.x - 8.0 - text_w) / 2.0).max(0.0));
                        self.strip_buttons(ui, s, audio, small, false);
                        ui.add_space(4.0);
                        readout(ui, s, if narrow { 10.5 } else { 12.0 });
                    });
                } else {
                    // Three buttons; narrower on narrow strips.
                    let led = vec2(((inner.width() - 12.0) / 3.0).min(28.0), 22.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.add_space((inner.width() - led.x * 3.0 - 12.0) / 2.0);
                        self.strip_buttons(ui, s, audio, led, true);
                    });
                    // Same height on every strip (room for the dB line), so faders line up.
                    let h = ui.available_height() - 46.0;
                    let changed = ui
                        .horizontal(|ui| {
                            ui.add_space((inner.width() - 52.0 - 10.0 - 8.0) / 2.0);
                            let marks = if s.volume.is_some() { mapping::db_marks(&s.range) } else { Vec::new() };
                            let r = widgets::fader(ui, &mut pos, vec2(52.0, h), color, s.pickup, &marks);
                            widgets::segment_meter(ui, s.peak, hold, vec2(10.0, h - 20.0));
                            r.changed()
                        })
                        .inner;
                    if changed {
                        self.send(UiCmd::SetGroupPosition(s.id.clone(), pos));
                    }
                    readout(ui, s, 13.0);
                    if s.ducked && !s.muted {
                        ui.add_space(-6.0);
                        ui.label(
                            RichText::new(format!("{} ducked", icon::ARROW_DOWN))
                                .font(mono(10.5))
                                .color(pal().warn_text),
                        )
                        .on_hover_text("Turned down while a strip set to duck others is playing");
                    } else if let Some(v) = s.volume.filter(|_| !s.muted) {
                        ui.add_space(-6.0);
                        ui.label(RichText::new(db_text(v)).font(mono(10.5)).color(pal().text_faint));
                    }
                }
            });
        }
        // Unassigned strips recede until something is dragged onto them.
        if empty && !dragging {
            painter.rect_filled(zone.shrink(1.0), 8.0, pal().veil(120));
        }
        egui::Popup::context_menu(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| self.assign_menu(ui, s, audio));
    }

    /// Up to three big icons of what the strip controls, "+N" for more. Takes
    /// the same height on every strip so the faders line up. Returns the
    /// LCD's icon: none when the big icons already show it.
    fn strip_icons<'a>(
        &mut self,
        ui: &mut egui::Ui,
        s: &StripView,
        width: f32,
        lcd_icon: Option<egui::Image<'a>>,
    ) -> Option<egui::Image<'a>> {
        const SIZE: f32 = 34.0;
        let mut images: Vec<egui::Image> = match &s.icon {
            Some(i) => self.icons.image(std::slice::from_ref(i)).into_iter().collect(),
            None => {
                let mut seen = HashSet::new();
                s.icons.iter().filter(|h| seen.insert(*h)).filter_map(|h| self.icons.image(h)).collect()
            }
        };
        if images.is_empty()
            && !s.targets.is_empty()
            && let Some(img) = lcd_icon.clone()
        {
            images.push(img);
        }
        let (rect, _) = ui.allocate_exact_size(vec2(width, SIZE), Sense::hover());
        if images.is_empty() {
            if s.targets.is_empty() {
                let c = pal().text_faint.gamma_multiply(0.6);
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, icon::PLUS_CIRCLE, mono(22.0), c);
            }
            return lcd_icon;
        }
        let extra = images.len().saturating_sub(3);
        let shown = images.len().min(3);
        // Overlap a little when there are several.
        let step = SIZE - 8.0;
        let total = SIZE + step * (shown - 1) as f32 + if extra > 0 { 26.0 } else { 0.0 };
        let mut x = rect.center().x - total / 2.0;
        let tint = if s.muted { egui::Color32::from_white_alpha(110) } else { egui::Color32::WHITE };
        for img in images.into_iter().take(3) {
            let r = egui::Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::splat(SIZE));
            img.tint(tint).paint_at(ui, r);
            x += step;
        }
        if extra > 0 {
            let p = egui::pos2(x + SIZE - step + 13.0, rect.center().y);
            ui.painter().text(p, egui::Align2::CENTER_CENTER, format!("+{extra}"), semibold(12.0), pal().text_dim);
        }
        None
    }

    /// S (assign menu), M (mute) and, with `run`, R buttons.
    fn strip_buttons(&mut self, ui: &mut egui::Ui, s: &StripView, audio: &AudioState, size: Vec2, run: bool) {
        let s_resp = widgets::led_button(ui, "S", s.assigned, ASSIGN, size).on_hover_text("Assign");
        if widgets::led_button(ui, "M", s.muted, MUTE, size).on_hover_text("Mute").clicked() {
            self.send(UiCmd::ToggleGroupMute(s.id.clone()));
        }
        if run
            && widgets::led_button(ui, "R", s.running, RUN, size)
                .on_hover_text("Run: make the device default / start the app")
                .clicked()
        {
            self.send(UiCmd::PressRun(s.id.clone()));
        }
        egui::Popup::menu(&s_resp)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| self.assign_menu(ui, s, audio));
    }

    /// Peak hold for the meter: decays after 1.5 s.
    fn peak_hold(&mut self, ui: &egui::Ui, s: &StripView) -> f32 {
        let now = ui.input(|i| i.time);
        let e = self.meter_hold.entry(s.id.clone()).or_insert((0.0, now));
        if s.peak >= e.0 || now - e.1 > 1.5 {
            *e = (s.peak, now);
        }
        if e.0 > s.peak {
            // Let the held line drop once its time is up.
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64((e.1 + 1.55 - now).max(0.0)));
        }
        e.0
    }

    fn assign_menu(&mut self, ui: &mut egui::Ui, s: &StripView, audio: &AudioState) {
        // Fixed width: right-aligned rows would otherwise stretch the popup.
        ui.set_width(300.0);
        let title = s.label.as_deref().unwrap_or(&s.name);
        ui.label(RichText::new(format!("{}  {title}", icon::TARGET)).font(semibold(14.0)));
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
                if let Some(a) = p.current_mut().get_mut(&gid) {
                    a.targets.remove(i);
                }
            });
            let what = s.targets[i].label();
            let strip = s.label.as_deref().unwrap_or(&s.name);
            self.toast(icon::TRASH, format!("Removed {what} from {strip}"), None, Some(super::ToastUndo::Edit));
        }
        ui.separator();
        self.target_menu(ui, &s.id, audio);
        ui.separator();
        self.identity_menu(ui, s, audio);
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

    /// Name, colour and icon of the strip, and clearing it.
    fn identity_menu(&mut self, ui: &mut egui::Ui, s: &StripView, audio: &AudioState) {
        let gid = s.id.clone();
        if self.rename.as_ref().is_none_or(|(id, _)| *id != gid) {
            self.rename = Some((gid.clone(), s.label.clone().unwrap_or_default()));
        }
        ui.horizontal(|ui| {
            ui.label("Name");
            let (_, text) = self.rename.as_mut().unwrap();
            let r = ui.add(egui::TextEdit::singleline(text).hint_text(&s.name).desired_width(200.0));
            if r.changed() {
                let label = Some(text.trim().to_string()).filter(|t| !t.is_empty());
                let gid = gid.clone();
                self.edit_profile(|p| p.current_mut().entry(gid).or_default().label = label);
            }
        });
        ui.horizontal(|ui| {
            ui.label("Colour");
            ui.spacing_mut().item_spacing.x = 0.0;
            let mut pick = None;
            if widgets::swatch(ui, pal().accent, s.color.is_none(), "Accent colour").clicked() {
                pick = Some(None);
            }
            for (i, (c, name)) in STRIP_COLORS.iter().enumerate() {
                if widgets::swatch(ui, *c, s.color == Some(i as u8), name).clicked() {
                    pick = Some(Some(i as u8));
                }
            }
            if let Some(color) = pick {
                let gid = gid.clone();
                self.edit_profile(|p| p.current_mut().entry(gid).or_default().color = color);
            }
        });
        ui.horizontal(|ui| {
            ui.label("Icon");
            let current = s.icon.as_ref().and_then(|i| self.icons.image(std::slice::from_ref(i)));
            if let Some(img) = current {
                ui.add(img.fit_to_exact_size(Vec2::splat(20.0)));
            } else {
                widgets::hint(ui, "Automatic");
            }
            ui.menu_button("Change", |ui| {
                if ui.button("Automatic (from what's assigned)").clicked() {
                    let gid = gid.clone();
                    self.edit_profile(|p| p.current_mut().entry(gid).or_default().icon = None);
                    ui.close();
                }
                ui.separator();
                // Icons of running apps first, then some generic ones.
                let mut names: Vec<String> = Vec::new();
                for a in audio.apps.iter().chain(&audio.app_inputs) {
                    for n in [&a.icon, &a.binary] {
                        if !n.is_empty() && self.icons.image(std::slice::from_ref(n)).is_some() {
                            names.push(n.clone());
                            break;
                        }
                    }
                }
                names.extend(ICON_CHOICES.iter().map(|n| n.to_string()));
                let mut seen = HashSet::new();
                names.retain(|n| seen.insert(n.clone()));
                egui::Grid::new("icon-grid").spacing(vec2(4.0, 4.0)).show(ui, |ui| {
                    let mut col = 0;
                    for n in names {
                        let Some(img) = self.icons.image(std::slice::from_ref(&n)) else { continue };
                        let b = egui::Button::image(img.fit_to_exact_size(Vec2::splat(24.0)))
                            .selected(s.icon.as_deref() == Some(n.as_str()));
                        if ui.add(b).on_hover_text(&n).clicked() {
                            let gid = gid.clone();
                            self.edit_profile(|p| p.current_mut().entry(gid).or_default().icon = Some(n));
                            ui.close();
                        }
                        col += 1;
                        if col % 6 == 0 {
                            ui.end_row();
                        }
                    }
                });
            });
        });
        // Ducking: this strip turns the other apps down while it plays.
        let duck = {
            let sh = self.shared.lock().unwrap();
            sh.active().and_then(|p| p.current().get(&gid)).and_then(|a| a.duck)
        };
        let mut on = duck.is_some();
        if widgets::toggle(ui, &mut on, "Turn other apps down while this plays").changed() {
            let gid = gid.clone();
            self.edit_profile(|p| p.current_mut().entry(gid).or_default().duck = on.then(Default::default));
        }
        if let Some(mut d) = duck {
            let before = d;
            ui.horizontal(|ui| {
                ui.label("By");
                ui.add(egui::Slider::new(&mut d.amount_db, -30.0..=-3.0).step_by(1.0).suffix(" dB"));
            });
            ui.horizontal(|ui| {
                ui.label("When louder than");
                ui.add(
                    egui::Slider::new(&mut d.threshold, 0.005..=0.3)
                        .logarithmic(true)
                        .custom_formatter(|v, _| format!("{:.0} dB", 20.0 * v.log10())),
                );
            });
            if d != before {
                let gid = gid.clone();
                self.edit_profile(|p| p.current_mut().entry(gid).or_default().duck = Some(d));
            }
        }
        let has_any = !s.targets.is_empty() || s.label.is_some() || s.color.is_some() || s.icon.is_some();
        if ui.add_enabled(has_any, egui::Button::new(format!("{}  Clear strip", icon::ERASER))).clicked() {
            let gid = gid.clone();
            self.edit_profile(|p| {
                if let Some(a) = p.current_mut().get_mut(&gid) {
                    a.targets.clear();
                    a.label = None;
                    a.color = None;
                    a.icon = None;
                }
            });
            self.rename = None;
            let strip = s.label.as_deref().unwrap_or(&s.name).to_string();
            self.toast(icon::ERASER, format!("Cleared {strip}"), None, Some(super::ToastUndo::Edit));
            ui.close();
        }
        ui.separator();
    }

    pub(super) fn add_target(&self, gid: &str, t: Target) {
        let gid = gid.to_string();
        self.edit_profile(|p| {
            let a = p.current_mut().entry(gid).or_default();
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
                let pattern = a.pattern();
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
                let pattern = a.pattern();
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
        // The fader's range and curve: e.g. a mic that never goes below 40%.
        let (before, global_curve) = {
            let sh = self.shared.lock().unwrap();
            let a = sh.active().and_then(|p| p.current().get(gid)).cloned().unwrap_or_default();
            ((a.min_volume, a.max_volume, a.curve), sh.store.settings.curve)
        };
        let (mut min, mut max, mut curve) = before;
        let pct = |v: f64, _: std::ops::RangeInclusive<usize>| format!("{:.0}%", v * 100.0);
        egui::Grid::new(("range", gid)).num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Bottom");
            ui.add(egui::Slider::new(&mut min, 0.0..=0.95).custom_formatter(pct));
            ui.end_row();
            ui.label("Top");
            ui.add(egui::Slider::new(&mut max, 0.05..=1.0).custom_formatter(pct));
            ui.end_row();
            ui.label("Curve");
            ui.horizontal(|ui| {
                let mut own = curve.is_some();
                if ui.checkbox(&mut own, "own").on_hover_text("Use a curve just for this strip").changed() {
                    curve = own.then_some(global_curve);
                }
                match &mut curve {
                    Some(c) => {
                        ui.add(egui::Slider::new(c, 0.5..=3.0).step_by(0.1).fixed_decimals(1));
                    }
                    None => {
                        widgets::hint(ui, &format!("{global_curve:.1} (Settings)"));
                    }
                }
            });
            ui.end_row();
        });
        if min > max {
            min = max;
        }
        if (min, max, curve) != before {
            let gid = gid.to_string();
            self.edit_profile(|p| {
                let a = p.current_mut().entry(gid).or_default();
                (a.min_volume, a.max_volume, a.curve) = (min, max, curve);
            });
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
        let apps = group_streams(&audio.apps);
        let inputs = group_streams(&audio.app_inputs);
        let total = apps.len() + inputs.len() + audio.sinks.len() + audio.sources.len();
        if total > 8 || !self.source_filter.is_empty() || self.want_filter {
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.source_filter)
                    .hint_text(format!("{}  Filter", icon::MAGNIFYING_GLASS))
                    .desired_width(f32::INFINITY),
            );
            if std::mem::take(&mut self.want_filter) {
                r.request_focus();
            }
            ui.add_space(4.0);
        }
        let filter = self.source_filter.to_lowercase();
        let shown = |name: &str, sub: &str| {
            filter.is_empty() || name.to_lowercase().contains(&filter) || sub.to_lowercase().contains(&filter)
        };
        egui::ScrollArea::vertical().id_salt("sources").auto_shrink([false, false]).show(ui, |ui| {
            widgets::collapsible_card(ui, icon::MUSIC_NOTES, "Applications", apps.len(), |ui| {
                if apps.is_empty() {
                    widgets::hint(ui, "Nothing is playing.");
                }
                for g in &apps {
                    let a = g[0];
                    let name = a.display_name();
                    let sub = stream_sub(g);
                    if !shown(&name, sub.as_deref().unwrap_or_default()) {
                        continue;
                    }
                    let target =
                        Target::App { m: AppMatch { pattern: a.pattern(), regex: false }, label: name.clone() };
                    let hints = vec![a.icon.clone(), a.binary.clone(), a.app_name.clone()];
                    let peak = g
                        .iter()
                        .map(|a| peaks.get(&MeterKey::SinkInput(a.index)).copied().unwrap_or(0.0))
                        .fold(0.0, f32::max);
                    let indices: Vec<u32> = g.iter().map(|a| a.index).collect();
                    self.source_row(
                        ui,
                        Kind::SinkInput,
                        &indices,
                        &name,
                        sub,
                        &hints,
                        a.volume,
                        g.iter().all(|a| a.mute),
                        None,
                        peak,
                        max,
                        target,
                        Some((a.device, &audio.sinks)),
                    );
                }
            });
            ui.add_space(8.0);
            if !inputs.is_empty() {
                widgets::collapsible_card(ui, icon::MICROPHONE, "Recording", inputs.len(), |ui| {
                    for g in &inputs {
                        let a = g[0];
                        let name = a.display_name();
                        let sub = stream_sub(g).filter(|_| g.len() > 1);
                        if !shown(&name, sub.as_deref().unwrap_or_default()) {
                            continue;
                        }
                        let target = Target::AppInput {
                            m: AppMatch { pattern: a.pattern(), regex: false },
                            label: name.clone(),
                        };
                        let hints = vec![a.icon.clone(), a.binary.clone(), a.app_name.clone()];
                        let indices: Vec<u32> = g.iter().map(|a| a.index).collect();
                        self.source_row(
                            ui,
                            Kind::SourceOutput,
                            &indices,
                            &name,
                            sub,
                            &hints,
                            a.volume,
                            g.iter().all(|a| a.mute),
                            None,
                            0.0,
                            max,
                            target,
                            Some((a.device, &audio.sources)),
                        );
                    }
                });
                ui.add_space(8.0);
            }
            for (kind, title, ic, devices, default) in [
                (Kind::Sink, "Outputs", icon::SPEAKER_HIGH, &audio.sinks, &audio.default_sink),
                (Kind::Source, "Inputs", icon::MICROPHONE_STAGE, &audio.sources, &audio.default_source),
            ] {
                widgets::collapsible_card(ui, ic, title, devices.len(), |ui| {
                    for d in devices {
                        if !shown(&d.label, &d.detail) {
                            continue;
                        }
                        let (target, generic, meter) = if kind == Kind::Sink {
                            (
                                Target::Output { name: d.name.clone(), label: d.description.clone() },
                                "audio-speakers",
                                MeterKey::Sink(d.name.clone()),
                            )
                        } else {
                            (
                                Target::Input { name: d.name.clone(), label: d.description.clone() },
                                "audio-input-microphone",
                                MeterKey::Source(d.name.clone()),
                            )
                        };
                        let hints = vec![d.icon.clone(), generic.into()];
                        let peak = peaks.get(&meter).copied().unwrap_or(0.0);
                        self.source_row(
                            ui,
                            kind,
                            &[d.index],
                            &d.label,
                            Some(d.detail.clone()).filter(|s| !s.is_empty()),
                            &hints,
                            d.volume,
                            d.mute,
                            Some((d.name == *default, d.name.clone())),
                            peak,
                            max,
                            target,
                            None,
                        );
                    }
                });
                ui.add_space(8.0);
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn source_row(
        &mut self,
        ui: &mut egui::Ui,
        kind: Kind,
        indices: &[u32],
        name: &str,
        sub: Option<String>,
        hints: &[String],
        volume: f32,
        mute: bool,
        default: Option<(bool, String)>,
        peak: f32,
        max: f32,
        target: Target,
        route: Option<(u32, &[crate::audio::Device])>,
    ) {
        let img = self.icons.image(hints);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            // Drag handle: icon + name.
            let id = ui.id().with(("src", kind as u8, indices.first().copied()));
            ui.dnd_drag_source(id, target, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(icon::DOTS_SIX_VERTICAL).color(pal().text_faint));
                    match &img {
                        Some(i) => {
                            ui.add(i.clone().fit_to_exact_size(Vec2::splat(22.0)));
                        }
                        None => {
                            ui.label(RichText::new(icon::WAVEFORM).size(18.0).color(pal().text_dim));
                        }
                    }
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.set_width(146.0);
                        ui.add(egui::Label::new(RichText::new(name).size(13.0)).truncate());
                        if let Some(sub) = &sub {
                            ui.add(egui::Label::new(RichText::new(sub).size(11.0).color(pal().text_dim)).truncate());
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
                    for &i in indices {
                        self.send(UiCmd::SetStreamVolume(kind, i, v));
                    }
                }
                r.on_hover_text(format!("{:.0}%", volume * 100.0));
                widgets::bar_meter(ui, peak, vec2(ui.spacing().slider_width, 3.0));
            });
            if let Some((is_default, dev_name)) = &default {
                let star = if *is_default {
                    RichText::new(icon::STAR).color(pal().warn_text)
                } else {
                    RichText::new(icon::STAR).color(pal().text_faint)
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
                RichText::new(icon::SPEAKER_SLASH).color(pal().mute_text)
            } else {
                RichText::new(icon::SPEAKER_HIGH).color(pal().text_dim)
            };
            if ui.add(egui::Button::new(m).frame(false)).on_hover_text(if mute { "Unmute" } else { "Mute" }).clicked() {
                for &i in indices {
                    self.send(UiCmd::SetStreamMute(kind, i, !mute));
                }
            }
            // Which device an app plays on (or records from).
            if let Some((current, devices)) = route {
                let hover = if kind == Kind::SinkInput { "Play on…" } else { "Record from…" };
                let button = ui
                    .add(egui::Button::new(RichText::new(icon::CARET_DOWN).color(pal().text_dim)).frame(false))
                    .on_hover_text(hover);
                egui::Popup::menu(&button).show(|ui| {
                    ui.label(RichText::new(hover).font(semibold(13.0)));
                    for d in devices {
                        let on = d.index == current;
                        let text = format!("{}  {}", if on { icon::CHECK } else { "    " }, d.label);
                        if ui.selectable_label(on, text).on_hover_text(&d.description).clicked() && !on {
                            for &i in indices {
                                self.send(UiCmd::MoveStream(kind, i, d.name.clone()));
                            }
                            ui.close();
                        }
                    }
                });
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

/// "47%", "MUTE", or the volume in amber with an arrow while the hardware
/// control waits to pick it up.
/// "Firefox ×2 + Spotify": names joined, repeats counted.
fn join_counted(names: &[String]) -> String {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for n in names {
        match counts.iter_mut().find(|(m, _)| m == n) {
            Some((_, c)) => *c += 1,
            None => counts.push((n, 1)),
        }
    }
    counts.iter().map(|(n, c)| if *c > 1 { format!("{n} ×{c}") } else { n.to_string() }).collect::<Vec<_>>().join(" + ")
}

/// Streams of the same app (same match pattern) together, in first-seen order.
fn group_streams(streams: &[crate::audio::AppStream]) -> Vec<Vec<&crate::audio::AppStream>> {
    let mut groups: Vec<Vec<&crate::audio::AppStream>> = Vec::new();
    for a in streams {
        match groups.iter_mut().find(|g| g[0].pattern() == a.pattern()) {
            Some(g) => g.push(a),
            None => groups.push(vec![a]),
        }
    }
    groups
}

/// Second line for an app row: what it plays, or how many streams it has.
fn stream_sub(group: &[&crate::audio::AppStream]) -> Option<String> {
    let a = group[0];
    if group.len() > 1 {
        return Some(format!("{} streams", group.len()));
    }
    (!a.media_name.is_empty() && a.media_name != a.display_name()).then(|| a.media_name.clone())
}

/// Generic icons offered for strips, besides the running apps' own.
const ICON_CHOICES: &[&str] = &[
    "audio-headphones",
    "audio-speakers",
    "audio-card",
    "audio-input-microphone",
    "multimedia-player",
    "applications-multimedia",
    "applications-games",
    "internet-web-browser",
    "internet-chat",
    "video-display",
    "media-optical-audio",
    "preferences-desktop-notification",
];

/// Channel number, plus the strip's name when it has one.
fn strip_header(painter: &egui::Painter, rect: egui::Rect, no: u32, label: Option<&str>) {
    let Some(label) = label else {
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, no.to_string(), semibold(15.0), pal().text_dim);
        return;
    };
    let num = painter.layout_no_wrap(no.to_string(), semibold(12.0), pal().text_faint);
    let mut job = egui::text::LayoutJob::simple_singleline(label.to_string(), semibold(14.0), pal().text);
    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - num.size().x - 6.0);
    let name = painter.layout_job(job);
    let total = num.size().x + 6.0 + name.size().x;
    let x = rect.center().x - total / 2.0;
    painter.galley(egui::pos2(x, rect.center().y - num.size().y / 2.0), num.clone(), pal().text_faint);
    painter.galley(egui::pos2(x + num.size().x + 6.0, rect.center().y - name.size().y / 2.0), name, pal().text);
}

fn readout(ui: &mut egui::Ui, s: &StripView, size: f32) {
    match s.pickup {
        Some(hw) if !s.muted => {
            let arrow = if hw < s.position { icon::ARROW_UP } else { icon::ARROW_DOWN };
            ui.label(
                RichText::new(format!("{arrow}{:>3.0}%", s.position * 100.0)).font(mono(size)).color(pal().warn_text),
            )
            .on_hover_text(format!(
                "Your controller is at {:.0}%. Move it {} to the volume to take over.",
                hw * 100.0,
                if hw < s.position { "up" } else { "down" }
            ));
        }
        _ => {
            let text = if s.muted { "MUTE".to_string() } else { format!("{:>3.0}%", s.position * 100.0) };
            ui.label(RichText::new(text).font(mono(size)).color(if s.muted { pal().mute_text } else { pal().readout }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counted_names() {
        let n = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(join_counted(&n(&["Firefox", "Spotify", "Firefox"])), "Firefox ×2 + Spotify");
        assert_eq!(join_counted(&n(&["Discord"])), "Discord");
    }

    #[test]
    fn streams_grouped_by_app() {
        let s = |i, bin: &str| crate::audio::AppStream { index: i, binary: bin.into(), ..Default::default() };
        let streams = [s(1, "game"), s(2, "firefox"), s(3, "game")];
        let g = group_streams(&streams);
        assert_eq!(
            g.iter().map(|g| g.iter().map(|a| a.index).collect::<Vec<_>>()).collect::<Vec<_>>(),
            [vec![1, 3], vec![2]]
        );
        assert_eq!(stream_sub(&g[0]).as_deref(), Some("2 streams"));
    }
}
