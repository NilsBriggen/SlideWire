//! Controller page: live diagram, controls and groups.

use eframe::egui::{self, RichText, Stroke};

use super::App;
use super::diagram::{self, LiveState};
use super::theme::{icon, *};
use super::widgets;
use crate::engine::{LearnRequest, UiCmd};
use crate::profile::{Command, Control, ControlKind, Delta, Group, Role};

impl App {
    pub(super) fn controller(&mut self, ui: &mut egui::Ui) {
        let (profile, ports_in, ports_out, learn, activity, leds) = {
            let sh = self.shared.lock().unwrap();
            (
                sh.active().cloned(),
                sh.ports_in.clone(),
                sh.ports_out.clone(),
                sh.learn.clone(),
                sh.activity.clone(),
                sh.led_state.clone(),
            )
        };
        let Some(mut p) = profile else {
            widgets::card(ui, icon::INFO, "No active profile", |ui| {
                ui.label("Create or import a profile under Profiles.");
            });
            return;
        };
        let before = serde_json::to_string(&p).unwrap_or_default();
        let mut learn_req: Option<Option<LearnRequest>> = None;
        let mut test: Option<String> = None;

        egui::ScrollArea::vertical().id_salt("controller").auto_shrink([false, false]).show(ui, |ui| {
            // Live diagram.
            let live = LiveState {
                activity: &activity,
                leds: &leds,
                learning: learn.as_ref().map(|l| l.control_id.as_str()),
                selected: self.expanded_control.as_deref(),
            };
            if let Some(id) = diagram::draw(ui, &p, &live) {
                self.expanded_control = if self.expanded_control.as_deref() == Some(&id) { None } else { Some(id) };
            }
            widgets::hint(ui, "Touch a control on your device to see it light up. Click one to edit it below.");
            ui.add_space(10.0);

            widgets::card(ui, icon::PLUGS_CONNECTED, "Device", |ui| {
                egui::Grid::new("device").num_columns(2).spacing([14.0, 8.0]).show(ui, |ui| {
                    ui.label("Profile name");
                    ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(280.0));
                    ui.end_row();
                    ui.label("MIDI input");
                    port_combo(ui, "in-port", &mut p.input, &ports_in);
                    ui.end_row();
                    ui.label("MIDI output (LEDs)");
                    port_combo(ui, "out-port", &mut p.output, &ports_out);
                    ui.end_row();
                });
            });
            ui.add_space(10.0);

            widgets::card(ui, icon::SLIDERS_HORIZONTAL, &format!("Controls ({})", p.controls.len()), |ui| {
                ui.horizontal(|ui| {
                    if widgets::icon_button(ui, icon::PLUS, "Add control").clicked() {
                        let id = crate::profile::new_id();
                        p.controls.insert(id.clone(), Control { name: format!("Control {}", p.controls.len() + 1), ..Default::default() });
                        learn_req = Some(Some(LearnRequest { control_id: id.clone() }));
                        self.expanded_control = Some(id);
                    }
                    widgets::hint(ui, "New controls start listening: move the control on your device.");
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(icon::MAGNIFYING_GLASS).color(TEXT_DIM));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.control_search)
                            .hint_text("Filter controls")
                            .desired_width(220.0),
                    );
                });
                ui.add_space(4.0);
                let search = self.control_search.trim().to_lowercase();
                let mut remove = None;
                for (title, ids) in control_sections(&p) {
                    let shown: Vec<String> = ids
                        .into_iter()
                        .filter(|id| {
                            search.is_empty()
                                || p.controls.get(id).is_some_and(|c| {
                                    c.name.to_lowercase().contains(&search)
                                        || message_summary(c).to_lowercase().contains(&search)
                                })
                        })
                        .collect();
                    if shown.is_empty() {
                        continue;
                    }
                    // Open while filtering, or when a control inside is being edited or learned.
                    let force_open = !search.is_empty()
                        || shown.iter().any(|id| {
                            self.expanded_control.as_deref() == Some(id.as_str())
                                || learn.as_ref().is_some_and(|l| &l.control_id == id)
                        });
                    let header = egui::CollapsingHeader::new(
                        RichText::new(format!("{title}  ·  {}", shown.len())).font(semibold(13.5)),
                    )
                    .id_salt(("section", title.as_str()));
                    let header = if force_open { header.open(Some(true)) } else { header };
                    header.show(ui, |ui| {
                        for id in &shown {
                            let Some(c) = p.controls.get_mut(id) else { continue };
                            let open = self.expanded_control.as_deref() == Some(id.as_str());
                            let learning = learn.as_ref().is_some_and(|l| &l.control_id == id);
                            let touched = activity.get(id).is_some_and(|(t, _)| t.elapsed().as_millis() < 400);
                            let stroke = if learning {
                                Stroke::new(1.5, ASSIGN)
                            } else if touched {
                                Stroke::new(1.5, CYAN)
                            } else if open {
                                Stroke::new(1.0, egui::Color32::from_rgb(0x4a, 0x4f, 0x5f))
                            } else {
                                Stroke::new(1.0, egui::Color32::from_rgb(0x2c, 0x2f, 0x39))
                            };
                            egui::Frame::new()
                                .fill(egui::Color32::from_rgb(0x19, 0x1a, 0x20))
                                .stroke(stroke)
                                .corner_radius(6)
                                .inner_margin(egui::Margin::symmetric(10, 6))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let (glyph, color) = match c.kind {
                                            ControlKind::Fader if c.delta.is_some() => (icon::ARROWS_CLOCKWISE, CYAN),
                                            ControlKind::Fader => (icon::FADERS, CYAN),
                                            ControlKind::Button => (icon::CURSOR_CLICK, ASSIGN),
                                            ControlKind::Output => (icon::LIGHTNING, RUN),
                                        };
                                        ui.label(RichText::new(glyph).size(16.0).color(color));
                                        ui.add(egui::TextEdit::singleline(&mut c.name).desired_width(150.0));
                                        widgets::chip(ui, &message_summary(c), TEXT_DIM);
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if ui.button(icon::TRASH).on_hover_text("Delete").clicked() {
                                                remove = Some(id.clone());
                                            }
                                            if ui.selectable_label(open, icon::GEAR_SIX).on_hover_text("Edit").clicked() {
                                                self.expanded_control = if open { None } else { Some(id.clone()) };
                                            }
                                            if c.kind != ControlKind::Fader
                                                && ui.button("Test").on_hover_text("Light it on the device").clicked()
                                            {
                                                test = Some(id.clone());
                                            }
                                            let learn_text = if learning {
                                                let t = ui.input(|i| i.time) as f32;
                                                ui.ctx().request_repaint();
                                                RichText::new(format!("{}  Listening…", icon::RECORD))
                                                    .color(ASSIGN.gamma_multiply(0.6 + 0.4 * (t * 6.0).sin().abs()))
                                            } else {
                                                RichText::new(format!("{}  Learn", icon::CROSSHAIR))
                                            };
                                            if ui.button(learn_text).clicked() {
                                                learn_req = Some(if learning { None } else { Some(LearnRequest { control_id: id.clone() }) });
                                            }
                                        });
                                    });
                                    if open {
                                        ui.add_space(4.0);
                                        edit_control(ui, id, c);
                                    }
                                });
                        }
                    });
                }
                if let Some(id) = remove {
                    p.remove_control(&id);
                }
            });
            ui.add_space(10.0);

            widgets::card(ui, icon::SQUARES_FOUR, &format!("Groups ({})", p.groups.len()), |ui| {
                ui.horizontal(|ui| {
                    if widgets::icon_button(ui, icon::PLUS, "Add group").clicked() {
                        p.groups.insert(crate::profile::new_id(), Group { name: format!("Group {}", p.groups.len() + 1), ..Default::default() });
                    }
                    widgets::hint(ui, "A group is one channel strip: faders set the volume, buttons mute / assign / run, outputs show the state.");
                });
                let controls = p.controls.clone();
                let mut remove_group = None;
                for (gid, g) in p.groups.iter_mut() {
                    egui::CollapsingHeader::new(RichText::new(&g.name).font(semibold(14.0))).id_salt(gid).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Name");
                            ui.text_edit_singleline(&mut g.name);
                            if widgets::icon_button(ui, icon::TRASH, "Delete group").clicked() {
                                remove_group = Some(gid.clone());
                            }
                        });
                        egui::Grid::new(("roles", gid.as_str())).num_columns(2).spacing([14.0, 6.0]).show(ui, |ui| {
                            for role in Role::ALL {
                                ui.label(RichText::new(role.label()).color(role_color(role)));
                                ui.horizontal_wrapped(|ui| role_editor(ui, gid, g, role, &controls));
                                ui.end_row();
                            }
                        });
                    });
                }
                if let Some(gid) = remove_group {
                    p.groups.shift_remove(&gid);
                    p.assignments.shift_remove(&gid);
                }
            });
        });

        if let Some(l) = learn_req {
            self.shared.lock().unwrap().learn = l;
        }
        if serde_json::to_string(&p).unwrap_or_default() != before {
            self.edit_profile(|ap| *ap = p);
        }
        if let Some(id) = test {
            self.send(UiCmd::SendTest(id));
        }
    }
}

/// Controls in collapsible sections: one per channel (groups whose names end
/// in the same number, like "Fader 1" and "Knob 1"), then everything else.
fn control_sections(p: &crate::profile::Profile) -> Vec<(String, Vec<String>)> {
    let mut sections: Vec<(String, Vec<String>)> = Vec::new();
    let mut placed = std::collections::HashSet::new();
    for g in p.groups.values() {
        let title = match g.name.rsplit(' ').next().and_then(|n| n.parse::<u32>().ok()) {
            Some(n) => format!("Channel {n}"),
            None => g.name.clone(),
        };
        let ids: Vec<String> = Role::ALL
            .iter()
            .flat_map(|r| g.role(*r).iter().cloned())
            .filter(|id| p.controls.contains_key(id) && placed.insert(id.clone()))
            .collect();
        match sections.iter_mut().find(|(t, _)| *t == title) {
            Some((_, list)) => list.extend(ids),
            None => sections.push((title, ids)),
        }
    }
    sections.sort_by_key(|(t, _)| {
        t.strip_prefix("Channel ").and_then(|n| n.parse::<u32>().ok()).map_or((1, 0), |n| (0, n))
    });
    let other: Vec<String> = p.controls.keys().filter(|id| !placed.contains(*id)).cloned().collect();
    if !other.is_empty() {
        sections.push(("Other controls".into(), other));
    }
    sections
}

fn message_summary(c: &Control) -> String {
    let base = if c.command.has_control() {
        format!("{} {} · ch {}", c.command.label(), c.control, c.channel + 1)
    } else {
        format!("{} · ch {}", c.command.label(), c.channel + 1)
    };
    if c.delta.is_some() { format!("{base} · relative") } else { base }
}

fn role_color(r: Role) -> egui::Color32 {
    match r {
        Role::Faders | Role::Indicators | Role::Meters => CYAN,
        Role::Mute | Role::MuteIndicators => MUTE,
        Role::Assign | Role::AssignIndicators => ASSIGN,
        Role::Run | Role::RunIndicators => RUN,
    }
}

fn edit_control(ui: &mut egui::Ui, id: &str, c: &mut Control) {
    egui::Grid::new(("edit", id)).num_columns(2).spacing([14.0, 6.0]).show(ui, |ui| {
        ui.label("Type");
        egui::ComboBox::from_id_salt(("kind", id)).selected_text(c.kind.label()).width(140.0).show_ui(ui, |ui| {
            for k in ControlKind::ALL {
                ui.selectable_value(&mut c.kind, k, k.label());
            }
        });
        ui.end_row();
        ui.label("Message");
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt(("cmd", id)).selected_text(c.command.label()).width(110.0).show_ui(ui, |ui| {
                for k in Command::ALL {
                    ui.selectable_value(&mut c.command, k, k.label());
                }
            });
            ui.label("channel");
            let mut ch = c.channel as u32 + 1;
            if ui.add(egui::DragValue::new(&mut ch).range(1..=16)).changed() {
                c.channel = (ch - 1) as u8;
            }
            if c.command.has_control() {
                ui.label("number");
                ui.add(egui::DragValue::new(&mut c.control).range(0..=127));
            }
        });
        ui.end_row();
        let default_max = c.command.default_max();
        let mut min = c.min();
        let mut max = c.max();
        ui.label("Range");
        ui.horizontal(|ui| {
            let a = ui.add(egui::DragValue::new(&mut min).range(0..=default_max)).changed();
            ui.label("to");
            let b = ui.add(egui::DragValue::new(&mut max).range(0..=default_max)).changed();
            if a || b {
                c.min_value = (min != 0).then_some(min);
                c.max_value = (max != default_max).then_some(max);
            }
        });
        ui.end_row();
        match c.kind {
            ControlKind::Fader => {
                ui.label("Encoder");
                ui.vertical(|ui| {
                    let mut relative = c.delta.is_some();
                    if ui.checkbox(&mut relative, "Endless encoder (sends relative steps)").changed() {
                        c.delta = relative.then(|| Delta::new(1, 65));
                    }
                    if let Some(d) = &mut c.delta {
                        ui.horizontal(|ui| {
                            ui.label("one click up =");
                            ui.add(egui::DragValue::new(&mut d.up).range(0..=127));
                            ui.label("down =");
                            ui.add(egui::DragValue::new(&mut d.down).range(0..=127));
                        });
                        let mut steps = c.steps.unwrap_or(0);
                        ui.horizontal(|ui| {
                            ui.label("clicks from 0 to 100% (0 = default)");
                            if ui.add(egui::DragValue::new(&mut steps).range(0..=1000)).changed() {
                                c.steps = (steps > 0).then_some(steps);
                            }
                        });
                    }
                });
                ui.end_row();
            }
            ControlKind::Button => {
                ui.label("Button");
                ui.vertical(|ui| {
                    let mut has_push = c.push_value.is_some();
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut has_push, "Only react to value").changed() {
                            c.push_value = has_push.then_some(127);
                        }
                        if let Some(v) = &mut c.push_value {
                            ui.add(egui::DragValue::new(v).range(1..=127));
                        }
                    });
                    ui.checkbox(&mut c.velocity_sensitive, "Any velocity counts as a press");
                    ui.checkbox(&mut c.automatic_indicator, "Has its own LED (same address)");
                });
                ui.end_row();
            }
            ControlKind::Output => {}
        }
    });
}

pub(super) fn port_combo(ui: &mut egui::Ui, id: &str, value: &mut String, ports: &[String]) {
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(value).desired_width(220.0).hint_text("port name (part of it is enough)"));
        egui::ComboBox::from_id_salt(id).selected_text("Pick").width(64.0).show_ui(ui, |ui| {
            if ui.selectable_label(value.is_empty(), "(none)").clicked() {
                value.clear();
            }
            for p in ports.iter().filter(|p| !p.starts_with(crate::midi::CLIENT) && !p.contains("Midi Through")) {
                // The client name is stable across reconnects, unlike port numbers.
                let short = p.split(':').next().unwrap_or(p).to_string();
                if ui.selectable_label(false, p).clicked() {
                    *value = short;
                }
            }
        });
    });
}

fn role_editor(
    ui: &mut egui::Ui,
    gid: &str,
    g: &mut Group,
    role: Role,
    controls: &indexmap::IndexMap<String, Control>,
) {
    let mut remove = None;
    let color = role_color(role);
    for (i, cid) in g.role(role).iter().enumerate() {
        let name = controls.get(cid).map(|c| c.name.as_str()).unwrap_or("?");
        let r = egui::Frame::new()
            .fill(color.gamma_multiply(0.12))
            .stroke(Stroke::new(1.0, color.gamma_multiply(0.4)))
            .corner_radius(12)
            .inner_margin(egui::Margin::symmetric(8, 2))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(RichText::new(name).size(12.5));
                    ui.add(egui::Button::new(RichText::new(icon::X).size(11.0)).frame(false)).clicked()
                })
                .inner
            });
        if r.inner {
            remove = Some(i);
        }
    }
    if let Some(i) = remove {
        g.role_mut(role).remove(i);
    }
    let wants = role.wants();
    let mut add = None;
    egui::ComboBox::from_id_salt(("add-role", gid, role.label())).selected_text(icon::PLUS).width(36.0).show_ui(
        ui,
        |ui| {
            let mut any = false;
            for (cid, c) in controls {
                // Indicators may also use a button's own LED.
                let fits = c.kind == wants || (wants == ControlKind::Output && c.kind == ControlKind::Button);
                if fits && !g.role(role).contains(cid) {
                    any = true;
                    if ui.selectable_label(false, &c.name).clicked() {
                        add = Some(cid.clone());
                    }
                }
            }
            if !any {
                ui.label("No matching controls");
            }
        },
    );
    if let Some(cid) = add {
        g.role_mut(role).push(cid);
    }
}
