//! Live drawing of the controller: controls light up when touched and show
//! the LED state the engine last sent.

use std::collections::HashMap;
use std::f32::consts::PI;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};
use rust_i18n::t;

use super::theme::{icon, *};
use super::widgets;
use crate::profile::{Command, Control, ControlKind, Profile};

pub struct LiveState<'a> {
    pub activity: &'a HashMap<String, (Instant, u16)>,
    pub leds: &'a HashMap<String, f32>,
    pub learning: Option<&'a str>,
    pub selected: Option<&'a str>,
}

impl LiveState<'_> {
    fn touched(&self, id: &str) -> f32 {
        // 1.0 right after a message, fading out over 600 ms.
        self.activity.get(id).map_or(0.0, |(t, _)| {
            let e = t.elapsed().as_secs_f32();
            (1.0 - e / 0.6).max(0.0)
        })
    }
    fn value(&self, c: &Control, id: &str) -> Option<f32> {
        let (_, v) = self.activity.get(id)?;
        let (min, max) = (c.min() as f32, c.max() as f32);
        (max > min).then(|| ((*v as f32 - min) / (max - min)).clamp(0.0, 1.0))
    }
    fn pressed(&self, id: &str) -> bool {
        self.activity.get(id).is_some_and(|(t, v)| *v > 0 && t.elapsed() < Duration::from_millis(200))
    }
    fn lit(&self, id: &str) -> bool {
        self.leds.get(id).is_some_and(|v| *v > 0.5) || self.pressed(id)
    }
}

pub fn is_nanokontrol2(p: &Profile) -> bool {
    let n = format!("{} {}", p.name, p.input).to_lowercase();
    n.contains("nanokontrol2")
}

/// Draw the controller; returns the id of a clicked control.
pub fn draw(ui: &mut Ui, p: &Profile, live: &LiveState) -> Option<String> {
    let any_recent = live.activity.values().any(|(t, _)| t.elapsed() < Duration::from_millis(700));
    if any_recent || live.learning.is_some() {
        ui.ctx().request_repaint_after(Duration::from_millis(33));
    }
    if is_nanokontrol2(p) { nanokontrol2(ui, p, live) } else { generic(ui, p, live) }
}

// ------------------------------------------------------------ primitives

pub(super) fn touch_glow(painter: &egui::Painter, rect: Rect, radius: f32, amount: f32) {
    if amount > 0.0 {
        widgets::glow(painter, rect, radius, pal().accent.gamma_multiply(0.55 * amount), 14.0);
        painter.rect_stroke(
            rect.expand(1.5),
            radius,
            Stroke::new(1.5, pal().accent.gamma_multiply(amount)),
            StrokeKind::Outside,
        );
    }
}

fn learn_pulse(ui: &Ui, painter: &egui::Painter, rect: Rect, radius: f32) {
    let t = ui.input(|i| i.time) as f32;
    let a = 0.5 + 0.5 * (t * 6.0).sin();
    painter.rect_stroke(
        rect.expand(3.0),
        radius + 2.0,
        Stroke::new(2.0, ASSIGN.gamma_multiply(a)),
        StrokeKind::Outside,
    );
}

fn selected_ring(painter: &egui::Painter, rect: Rect, radius: f32) {
    painter.rect_stroke(
        rect.expand(4.0),
        radius + 3.0,
        Stroke::new(1.0, Color32::from_white_alpha(140)),
        StrokeKind::Outside,
    );
}

fn draw_knob(painter: &egui::Painter, center: Pos2, r: f32, value: Option<f32>) {
    let start = 0.75 * PI;
    let sweep = 1.5 * PI;
    let pts = |to: f32| -> Vec<Pos2> {
        (0..=32)
            .map(|i| {
                let a = start + (to - start) * i as f32 / 32.0;
                center + (r + 4.0) * vec2(a.cos(), a.sin())
            })
            .collect()
    };
    painter.add(Shape::line(pts(start + sweep), Stroke::new(2.0, SLOT)));
    if let Some(v) = value {
        painter.add(Shape::line(pts(start + sweep * v), Stroke::new(2.0, pal().accent)));
    }
    painter.circle_filled(center + vec2(0.0, 2.0), r, Color32::from_black_alpha(150));
    painter.circle_filled(center, r, Color32::from_rgb(0x1d, 0x1e, 0x23));
    painter.circle_filled(center - vec2(0.0, r * 0.1), r * 0.82, Color32::from_rgb(0x2a, 0x2c, 0x33));
    let a = start + sweep * value.unwrap_or(0.5);
    painter.line_segment(
        [center + r * 0.2 * vec2(a.cos(), a.sin()), center + r * 0.8 * vec2(a.cos(), a.sin())],
        Stroke::new(2.0, hw::TEXT),
    );
}

fn draw_fader(painter: &egui::Painter, rect: Rect, value: Option<f32>) {
    let track = Rect::from_center_size(rect.center(), vec2(5.0, rect.height() - 14.0));
    painter.rect_filled(track, 2.0, SLOT);
    let v = value.unwrap_or(0.0);
    let y = track.bottom() - track.height() * v;
    if value.is_some() {
        painter.rect_filled(
            Rect::from_min_max(pos2(track.left() + 1.0, y), pos2(track.right() - 1.0, track.bottom())),
            1.0,
            pal().accent,
        );
    }
    let cap = Rect::from_center_size(pos2(track.center().x, y), vec2(rect.width().min(28.0), 13.0));
    painter.rect_filled(cap.translate(vec2(0.0, 2.0)), 2.0, Color32::from_black_alpha(150));
    painter.rect_filled(cap, 2.0, Color32::from_rgb(0x2d, 0x2f, 0x36));
    painter.line_segment(
        [pos2(cap.left() + 2.0, cap.center().y), pos2(cap.right() - 2.0, cap.center().y)],
        Stroke::new(1.0, hw::TEXT_DIM),
    );
}

fn draw_button(painter: &egui::Painter, rect: Rect, label: &str, lit: bool, color: Color32) {
    if lit {
        widgets::glow(painter, rect, 3.0, color.gamma_multiply(0.5), 10.0);
        painter.rect_filled(rect, 3.0, color);
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(12.0),
            Color32::from_rgb(0x15, 0x12, 0x10),
        );
    } else {
        painter.rect_filled(rect.translate(vec2(0.0, 1.5)), 3.0, Color32::from_black_alpha(140));
        painter.rect_filled(rect, 3.0, Color32::from_rgb(0x2a, 0x2c, 0x33));
        painter.rect_stroke(rect, 3.0, Stroke::new(1.0, Color32::from_rgb(0x3c, 0x3f, 0x4a)), StrokeKind::Inside);
        painter.text(rect.center(), Align2::CENTER_CENTER, label, egui::FontId::proportional(12.0), hw::TEXT_DIM);
    }
}

/// Make `rect` clickable for control `id`, adding touch/learn/selection rings.
fn control_area(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    id: &str,
    radius: f32,
    live: &LiveState,
    name: &str,
) -> bool {
    touch_glow(painter, rect, radius, live.touched(id));
    if live.learning == Some(id) {
        learn_pulse(ui, painter, rect, radius);
    }
    if live.selected == Some(id) {
        selected_ring(painter, rect, radius);
    }
    ui.interact(rect, ui.id().with(("diagram", id)), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(name)
        .clicked()
}

// ------------------------------------------------------------ nanoKONTROL2

fn nanokontrol2(ui: &mut Ui, p: &Profile, live: &LiveState) -> Option<String> {
    // Find controls by their factory CC numbers, whatever they are named.
    let by_cc: HashMap<u8, (&String, &Control)> = p
        .controls
        .iter()
        .filter(|(_, c)| c.command == Command::Cc && c.kind != ControlKind::Output)
        .map(|(id, c)| (c.control, (id, c)))
        .collect();

    let w = ui.available_width().clamp(720.0, 1400.0);
    let h = (w * 0.25).clamp(250.0, 330.0);
    let (outer, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let painter = ui.painter_at(outer.expand(16.0));
    widgets::bevel_panel(&painter, outer, 12, Color32::from_rgb(0x2b, 0x2d, 0x34), Color32::from_rgb(0x1a, 0x1b, 0x20));
    painter.text(outer.left_top() + vec2(18.0, 12.0), Align2::LEFT_TOP, "nanoKONTROL2", semibold(13.0), hw::TEXT_DIM);
    painter.text(outer.right_top() + vec2(-18.0, 12.0), Align2::RIGHT_TOP, "KORG", semibold(13.0), hw::TEXT_FAINT);

    let mut clicked: Option<String> = None;
    let button = |ui: &mut Ui, clicked: &mut Option<String>, rect: Rect, cc: u8, label: &str, color: Color32| {
        if let Some((id, c)) = by_cc.get(&cc) {
            draw_button(&painter, rect, label, live.lit(id), color);
            if control_area(ui, &painter, rect, id, 4.0, live, &c.name) {
                *clicked = Some((*id).clone());
            }
        } else {
            draw_button(&painter, rect, label, false, color);
        }
    };
    let caption = |pos: Pos2, text: &str| {
        painter.text(pos, Align2::LEFT_BOTTOM, text, egui::FontId::proportional(9.5), hw::TEXT_FAINT);
    };

    let body = Rect::from_min_max(outer.min + vec2(18.0, 44.0), outer.max - vec2(18.0, 16.0));
    let left = Rect::from_min_size(body.min, vec2(body.width() * 0.25, body.height()));
    let right = Rect::from_min_max(pos2(left.right() + 22.0, body.top()), body.max);
    painter.line_segment(
        [pos2(left.right() + 11.0, body.top()), pos2(left.right() + 11.0, body.bottom())],
        Stroke::new(1.0, Color32::from_black_alpha(130)),
    );

    // Transport section: TRACK, CYCLE + MARKER, then the five transport keys.
    let gap = 8.0;
    let bw = (left.width() - 4.0 * gap) / 5.0;
    let bh = (bw * 0.62).clamp(20.0, 30.0);
    let at = |col: f32, y: f32| Rect::from_min_size(pos2(left.left() + col * (bw + gap), y), vec2(bw, bh));
    let y0 = left.top() + 14.0;
    let y1 = left.top() + left.height() * 0.40;
    let y2 = left.bottom() - bh - 6.0;
    caption(pos2(left.left(), y0 - 3.0), "TRACK");
    button(ui, &mut clicked, at(0.0, y0), 58, icon::CARET_LEFT, hw::TEXT);
    button(ui, &mut clicked, at(1.0, y0), 59, icon::CARET_RIGHT, hw::TEXT);
    caption(pos2(left.left(), y1 - 3.0), "CYCLE");
    button(ui, &mut clicked, at(0.0, y1), 46, icon::REPEAT, ASSIGN);
    caption(pos2(left.left() + 2.0 * (bw + gap), y1 - 3.0), "MARKER  SET");
    button(ui, &mut clicked, at(2.0, y1), 60, "SET", hw::TEXT);
    button(ui, &mut clicked, at(3.0, y1), 61, icon::CARET_LEFT, hw::TEXT);
    button(ui, &mut clicked, at(4.0, y1), 62, icon::CARET_RIGHT, hw::TEXT);
    let big = |col: f32| Rect::from_min_size(pos2(left.left() + col * (bw + gap), y2 - 6.0), vec2(bw, bh + 6.0));
    for (i, (cc, label, color)) in [
        (43u8, icon::REWIND, hw::TEXT),
        (44, icon::FAST_FORWARD, hw::TEXT),
        (42, icon::STOP, hw::TEXT),
        (41, icon::PLAY, RUN),
        (45, icon::RECORD, MUTE),
    ]
    .iter()
    .enumerate()
    {
        button(ui, &mut clicked, big(i as f32), *cc, label, *color);
    }

    // Eight channel strips: knob on top, S/M/R on the left, fader on the right.
    let col_w = right.width() / 8.0;
    let kr = (col_w * 0.24).min(right.height() * 0.13);
    for i in 0..8u8 {
        let col = Rect::from_min_size(pos2(right.left() + col_w * i as f32, right.top()), vec2(col_w, right.height()));
        let kc = pos2(col.left() + col_w * 0.5, col.top() + kr + 4.0);
        if let Some((id, c)) = by_cc.get(&(16 + i)) {
            draw_knob(&painter, kc, kr, live.value(c, id));
            let r = Rect::from_center_size(kc, egui::Vec2::splat(kr * 2.0 + 10.0));
            if control_area(ui, &painter, r, id, kr + 5.0, live, &c.name) {
                clicked = Some((*id).clone());
            }
        }
        let lower_top = kc.y + kr + 20.0;
        let lower_bottom = col.bottom() - 14.0;
        let bsize = vec2((col_w * 0.30).clamp(20.0, 32.0), ((lower_bottom - lower_top) / 4.0).clamp(16.0, 24.0));
        let step = (lower_bottom - lower_top - bsize.y) / 2.0;
        for (j, (cc, label, color)) in
            [(32 + i, "S", ASSIGN), (48 + i, "M", MUTE), (64 + i, "R", RUN)].iter().enumerate()
        {
            let rect = Rect::from_min_size(pos2(col.left() + col_w * 0.12, lower_top + j as f32 * step), bsize);
            button(ui, &mut clicked, rect, *cc, label, *color);
        }
        let fr = Rect::from_min_max(
            pos2(col.left() + col_w * 0.52, lower_top - 6.0),
            pos2(col.left() + col_w * 0.88, lower_bottom + 6.0),
        );
        if let Some((id, c)) = by_cc.get(&i) {
            draw_fader(&painter, fr, live.value(c, id));
            if control_area(ui, &painter, fr, id, 4.0, live, &c.name) {
                clicked = Some((*id).clone());
            }
        }
        painter.text(
            pos2(col.center().x, outer.bottom() - 6.0),
            Align2::CENTER_BOTTOM,
            format!("{}", i + 1),
            egui::FontId::proportional(10.0),
            hw::TEXT_FAINT,
        );
    }
    clicked
}

// ------------------------------------------------------------ generic

fn generic(ui: &mut Ui, p: &Profile, live: &LiveState) -> Option<String> {
    let mut clicked = None;
    if p.controls.is_empty() {
        widgets::hint(ui, &t!("No controls yet. Add one below and move it on your device."));
        return None;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
        for (id, c) in &p.controls {
            let (rect, _) = ui.allocate_exact_size(vec2(84.0, 96.0), Sense::hover());
            let painter = ui.painter_at(rect.expand(12.0));
            widgets::bevel_panel(&painter, rect, 6, hw::BODY_TOP, hw::BODY_BOTTOM);
            let area = rect.shrink2(vec2(10.0, 22.0)).translate(vec2(0.0, -6.0));
            match c.kind {
                ControlKind::Fader if c.delta.is_some() || c.name.to_lowercase().contains("knob") => {
                    draw_knob(&painter, area.center(), area.height().min(area.width()) * 0.38, live.value(c, id));
                }
                ControlKind::Fader => draw_fader(&painter, area, live.value(c, id)),
                ControlKind::Button => draw_button(
                    &painter,
                    Rect::from_center_size(area.center(), vec2(34.0, 22.0)),
                    "",
                    live.lit(id),
                    ASSIGN,
                ),
                ControlKind::Output => {
                    let v = live.leds.get(id.as_str()).copied().unwrap_or(0.0);
                    painter.circle_filled(
                        area.center(),
                        8.0,
                        if v > 0.0 { pal().accent.gamma_multiply(0.3 + 0.7 * v) } else { SLOT },
                    );
                }
            }
            painter.text(
                pos2(rect.center().x, rect.bottom() - 6.0),
                Align2::CENTER_BOTTOM,
                truncate(&c.name, 12),
                egui::FontId::proportional(11.0),
                hw::TEXT_DIM,
            );
            if control_area(ui, &painter, rect, id, 6.0, live, &c.name) {
                clicked = Some(id.clone());
            }
        }
    });
    clicked
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { s.chars().take(n - 1).collect::<String>() + "…" }
}
