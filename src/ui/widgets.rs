//! Painted console widgets: faders, knobs, LED buttons, meters, LCDs, cards.

use std::f32::consts::PI;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, Mesh, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, Vec2,
    epaint::RectShape, pos2, vec2,
};

use super::theme::{self, *};

/// Fill `rect` with a vertical gradient.
pub fn gradient_rect(painter: &egui::Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 2, 3);
    painter.add(Shape::mesh(mesh));
}

/// A soft coloured halo around `rect`.
pub fn glow(painter: &egui::Painter, rect: Rect, radius: f32, color: Color32, blur: f32) {
    painter.add(
        RectShape::filled(rect.expand(blur * 0.25), CornerRadius::same(radius as u8), color).with_blur_width(blur),
    );
}

/// Raised panel: body plus 1px bevel highlight on top and shadow at the bottom.
pub fn bevel_panel(painter: &egui::Painter, rect: Rect, radius: u8, top: Color32, bottom: Color32) {
    // Drop shadow.
    painter.add(
        RectShape::filled(rect.translate(vec2(0.0, 3.0)), CornerRadius::same(radius), Color32::from_black_alpha(110))
            .with_blur_width(10.0),
    );
    painter.rect_filled(rect, radius, bottom);
    // Gradient body inset by the corner radius so corners stay rounded.
    let inner = rect.shrink2(vec2(0.0, radius as f32));
    gradient_rect(painter, inner, top, bottom);
    painter.rect_filled(
        Rect::from_min_max(rect.min, pos2(rect.max.x, inner.min.y + 0.5)),
        CornerRadius { nw: radius, ne: radius, sw: 0, se: 0 },
        top,
    );
    painter.rect_stroke(rect, radius, Stroke::new(1.0, Color32::from_rgb(0x33, 0x36, 0x41)), StrokeKind::Inside);
    painter.line_segment(
        [pos2(rect.left() + radius as f32, rect.top() + 1.0), pos2(rect.right() - radius as f32, rect.top() + 1.0)],
        Stroke::new(1.0, Color32::from_white_alpha(22)),
    );
}

fn scroll_delta(ui: &Ui, response: &Response) -> f32 {
    if response.hovered() {
        let d = ui.input(|i| i.smooth_scroll_delta.y);
        if d != 0.0 {
            return d;
        }
    }
    0.0
}

/// Long-throw fader. `pos` is 0..=1. Drag, scroll, double-click for 100%.
pub fn fader(ui: &mut Ui, pos: &mut f32, size: Vec2, color: Color32) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let cap = vec2(size.x.min(38.0), 20.0);
    let track = Rect::from_center_size(rect.center(), vec2(6.0, rect.height() - cap.y));
    let travel = track.height();
    let before = *pos;

    if response.dragged() {
        *pos -= response.drag_delta().y / travel;
    }
    let scroll = scroll_delta(ui, &response);
    if scroll != 0.0 {
        *pos += scroll.signum() * 0.02;
    }
    if response.double_clicked() {
        *pos = 1.0;
    }
    *pos = pos.clamp(0.0, 1.0);
    if (*pos - before).abs() > f32::EPSILON {
        response.mark_changed();
    }

    let painter = ui.painter_at(rect.expand(12.0));
    // Tick marks on both sides.
    for i in 0..=10 {
        let y = track.bottom() - travel * i as f32 / 10.0;
        let major = i % 5 == 0;
        let len = if major { 8.0 } else { 4.0 };
        let c = if major { TEXT_FAINT } else { Color32::from_rgb(0x3a, 0x3d, 0x48) };
        painter.line_segment([pos2(track.left() - 6.0 - len, y), pos2(track.left() - 6.0, y)], Stroke::new(1.0, c));
        painter.line_segment([pos2(track.right() + 6.0, y), pos2(track.right() + 6.0 + len, y)], Stroke::new(1.0, c));
    }
    // Slot.
    painter.rect_filled(track, 3.0, SLOT);
    painter.line_segment([track.left_bottom(), track.right_bottom()], Stroke::new(1.0, Color32::from_white_alpha(16)));
    // Level fill.
    let y = track.bottom() - travel * *pos;
    let fill = Rect::from_min_max(pos2(track.center().x - 1.5, y), pos2(track.center().x + 1.5, track.bottom()));
    if *pos > 0.0 {
        glow(&painter, fill, 2.0, color.gamma_multiply(0.5), 8.0);
        painter.rect_filled(fill, 1.5, color);
    }
    // Cap.
    let cap_rect = Rect::from_center_size(pos2(track.center().x, y), cap);
    painter.add(
        RectShape::filled(cap_rect.translate(vec2(0.0, 4.0)), CornerRadius::same(3), Color32::from_black_alpha(160))
            .with_blur_width(8.0),
    );
    let hot = response.hovered() || response.dragged();
    let (top, bottom) = if hot {
        (Color32::from_rgb(0xf0, 0xf2, 0xf6), Color32::from_rgb(0x9a, 0x9e, 0xa8))
    } else {
        (Color32::from_rgb(0xd6, 0xd9, 0xe0), Color32::from_rgb(0x7e, 0x82, 0x8c))
    };
    painter.rect_filled(cap_rect, 3.0, bottom);
    gradient_rect(&painter, cap_rect.shrink2(vec2(0.0, 2.0)), top, bottom);
    painter.rect_stroke(cap_rect, 3.0, Stroke::new(1.0, Color32::from_rgb(0x3c, 0x3f, 0x47)), StrokeKind::Outside);
    painter.line_segment(
        [pos2(cap_rect.left() + 3.0, cap_rect.center().y), pos2(cap_rect.right() - 3.0, cap_rect.center().y)],
        Stroke::new(2.0, Color32::from_rgb(0x26, 0x28, 0x2e)),
    );
    painter.line_segment(
        [
            pos2(cap_rect.left() + 3.0, cap_rect.center().y + 1.5),
            pos2(cap_rect.right() - 3.0, cap_rect.center().y + 1.5),
        ],
        Stroke::new(1.0, Color32::from_white_alpha(70)),
    );
    response.on_hover_cursor(egui::CursorIcon::ResizeVertical)
}

fn arc_points(center: Pos2, radius: f32, from: f32, to: f32) -> Vec<Pos2> {
    let n = ((to - from).abs() / (PI / 48.0)).ceil().max(2.0) as usize;
    (0..=n)
        .map(|i| {
            let a = from + (to - from) * i as f32 / n as f32;
            center + radius * vec2(a.cos(), a.sin())
        })
        .collect()
}

/// Rotary knob with a 270° value ring. Drag vertically or scroll.
pub fn knob(ui: &mut Ui, pos: &mut f32, diameter: f32, color: Color32) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::click_and_drag());
    let before = *pos;
    if response.dragged() {
        *pos -= response.drag_delta().y / 160.0;
    }
    let scroll = scroll_delta(ui, &response);
    if scroll != 0.0 {
        *pos += scroll.signum() * 0.02;
    }
    if response.double_clicked() {
        *pos = 1.0;
    }
    *pos = pos.clamp(0.0, 1.0);
    if (*pos - before).abs() > f32::EPSILON {
        response.mark_changed();
    }

    let painter = ui.painter_at(rect.expand(6.0));
    let c = rect.center();
    let r = diameter / 2.0;
    // Angles in screen space (y down): start at 135° (bottom-left), sweep 270°.
    let start = 0.75 * PI;
    let end = start + 1.5 * PI;
    let value = start + (end - start) * *pos;
    let ring_r = r - 3.0;
    painter.add(Shape::line(arc_points(c, ring_r, start, end), Stroke::new(4.0, SLOT)));
    if *pos > 0.001 {
        painter.add(Shape::line(arc_points(c, ring_r, start, value), Stroke::new(7.0, color.gamma_multiply(0.25))));
        painter.add(Shape::line(arc_points(c, ring_r, start, value), Stroke::new(3.0, color)));
    }
    // Body.
    let body_r = r - 9.0;
    painter.circle_filled(c + vec2(0.0, 3.0), body_r, Color32::from_black_alpha(140));
    painter.circle_filled(c, body_r, Color32::from_rgb(0x2e, 0x31, 0x3a));
    painter.circle_filled(c - vec2(0.0, body_r * 0.12), body_r * 0.86, Color32::from_rgb(0x3a, 0x3e, 0x49));
    painter.circle_stroke(c, body_r, Stroke::new(1.0, Color32::from_rgb(0x15, 0x16, 0x1b)));
    let hot = response.hovered() || response.dragged();
    let tip = c + (body_r - 3.0) * vec2(value.cos(), value.sin());
    let base = c + (body_r * 0.25) * vec2(value.cos(), value.sin());
    painter.line_segment([base, tip], Stroke::new(2.5, if hot { Color32::WHITE } else { TEXT }));
    response.on_hover_cursor(egui::CursorIcon::ResizeVertical)
}

/// Square "M/S/R" button with an LED that glows in `color` when `on`.
pub fn led_button(ui: &mut Ui, label: &str, on: bool, color: Color32, size: Vec2) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter_at(rect.expand(10.0));
    let hot = response.hovered();
    if on {
        glow(&painter, rect, 5.0, color.gamma_multiply(0.45), 12.0);
        painter.rect_filled(rect, 5.0, color);
        gradient_rect(
            &painter,
            rect.shrink2(vec2(1.0, 4.0)),
            Color32::from_white_alpha(40),
            Color32::from_black_alpha(10),
        );
        painter.text(rect.center(), Align2::CENTER_CENTER, label, semibold(13.0), Color32::from_rgb(0x14, 0x12, 0x10));
    } else {
        painter.add(
            RectShape::filled(rect.translate(vec2(0.0, 2.0)), CornerRadius::same(5), Color32::from_black_alpha(120))
                .with_blur_width(4.0),
        );
        let fill = if hot { Color32::from_rgb(0x33, 0x36, 0x41) } else { Color32::from_rgb(0x27, 0x29, 0x31) };
        painter.rect_filled(rect, 5.0, fill);
        painter.rect_stroke(rect, 5.0, Stroke::new(1.0, Color32::from_rgb(0x3a, 0x3d, 0x48)), StrokeKind::Inside);
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            label,
            semibold(13.0),
            color.gamma_multiply(if hot { 0.9 } else { 0.6 }),
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Vertical LED-segment meter. `level` and `hold` are 0..=1.
pub fn segment_meter(ui: &mut Ui, level: f32, hold: f32, size: Vec2) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, SLOT);
    let n = 24;
    let inner = rect.shrink(2.0);
    let seg_h = inner.height() / n as f32;
    let hold_seg = (hold * n as f32).ceil() as usize;
    for i in 0..n {
        let frac = (i as f32 + 1.0) / n as f32;
        let color = if frac > 0.9 {
            METER_HIGH
        } else if frac > 0.7 {
            METER_MID
        } else {
            METER_LOW
        };
        let y1 = inner.bottom() - seg_h * i as f32;
        let seg = Rect::from_min_max(pos2(inner.left(), y1 - seg_h + 1.0), pos2(inner.right(), y1));
        let lit = frac <= level + 0.5 / n as f32 || (hold_seg > 0 && i + 1 == hold_seg);
        painter.rect_filled(seg, 1.0, if lit { color } else { color.gamma_multiply(0.09) });
    }
    response
}

/// Horizontal mini meter for list rows.
pub fn bar_meter(ui: &mut Ui, level: f32, size: Vec2) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, SLOT);
    let w = rect.width() * level.clamp(0.0, 1.0);
    if w > 0.5 {
        let color = if level > 0.9 {
            METER_HIGH
        } else if level > 0.7 {
            METER_MID
        } else {
            METER_LOW
        };
        painter.rect_filled(Rect::from_min_size(rect.min, vec2(w, rect.height())), 2.0, color);
    }
    response
}

/// Scribble-strip style display with optional icon; long text scrolls.
pub fn lcd(ui: &mut Ui, size: Vec2, icon: Option<egui::Image<'_>>, text: &str, dim: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, LCD_BG);
    painter.rect_stroke(rect, 4.0, Stroke::new(1.0, Color32::from_black_alpha(200)), StrokeKind::Inside);
    gradient_rect(
        &painter,
        Rect::from_min_size(rect.min + vec2(2.0, 2.0), vec2(rect.width() - 4.0, rect.height() * 0.45)),
        Color32::from_white_alpha(7),
        Color32::TRANSPARENT,
    );
    let mut text_rect = rect.shrink2(vec2(6.0, 2.0));
    if let Some(img) = icon {
        let s = (rect.height() - 8.0).min(22.0);
        let r = Rect::from_center_size(pos2(rect.left() + 5.0 + s / 2.0, rect.center().y), Vec2::splat(s));
        img.tint(if dim { Color32::from_white_alpha(90) } else { Color32::WHITE }).paint_at(ui, r);
        text_rect.min.x = r.right() + 5.0;
    }
    let color = if dim { LCD_DIM } else { LCD_TEXT };
    let font = mono(12.0);
    let galley = painter.layout_no_wrap(text.to_string(), font, color);
    let clip = painter.with_clip_rect(text_rect);
    let y = text_rect.center().y - galley.size().y / 2.0;
    if galley.size().x <= text_rect.width() {
        let x = text_rect.center().x - galley.size().x / 2.0;
        clip.galley(pos2(x, y), galley, color);
    } else {
        // Marquee.
        let gap = 30.0;
        let period = galley.size().x + gap;
        let t = ui.input(|i| i.time) as f32;
        let offset = (t * 28.0) % period;
        clip.galley(pos2(text_rect.left() - offset, y), galley.clone(), color);
        clip.galley(pos2(text_rect.left() - offset + period, y), galley, color);
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
    }
    response
}

/// Rounded panel with a header line.
pub fn card<R>(ui: &mut Ui, icon: &str, title: &str, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, Color32::from_rgb(0x2c, 0x2f, 0x39)))
        .corner_radius(10)
        .inner_margin(egui::Margin::same(14))
        .shadow(egui::epaint::Shadow { offset: [0, 3], blur: 10, spread: 0, color: Color32::from_black_alpha(90) })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !title.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(icon).size(17.0).color(CYAN));
                    ui.label(egui::RichText::new(title).font(semibold(15.0)));
                });
                ui.add_space(4.0);
            }
            add_contents(ui)
        })
        .inner
}

/// Small rounded status chip with an LED dot.
pub fn pill(ui: &mut Ui, led: Option<Color32>, text: &str) -> Response {
    egui::Frame::new()
        .fill(Color32::from_rgb(0x22, 0x24, 0x2c))
        .stroke(Stroke::new(1.0, Color32::from_rgb(0x33, 0x36, 0x41)))
        .corner_radius(20)
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if let Some(c) = led {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                    if c != TEXT_FAINT {
                        glow(
                            ui.painter(),
                            Rect::from_center_size(r.center(), Vec2::splat(6.0)),
                            3.0,
                            c.gamma_multiply(0.6),
                            8.0,
                        );
                    }
                    ui.painter().circle_filled(r.center(), 4.0, c);
                }
                ui.label(egui::RichText::new(text).size(12.5));
            });
        })
        .response
}

/// A text chip, e.g. `CC 16 · ch 1`.
pub fn chip(ui: &mut Ui, text: &str, color: Color32) -> Response {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.14))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.45)))
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| ui.label(egui::RichText::new(text).font(theme::mono(11.5)).color(color)))
        .response
}

/// Button with a Phosphor icon and label.
pub fn icon_button(ui: &mut Ui, icon: &str, label: &str) -> Response {
    let text = if label.is_empty() { icon.to_string() } else { format!("{icon}  {label}") };
    ui.button(egui::RichText::new(text))
}

/// Hint text in a faint colour.
pub fn hint(ui: &mut Ui, text: &str) -> Response {
    ui.label(egui::RichText::new(text).size(12.5).color(TEXT_DIM))
}

/// Pill-shaped on/off switch with a label; glows cyan when on.
pub fn toggle(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(label.to_string(), egui::FontId::proportional(14.0), TEXT);
    let sw = vec2(36.0, 20.0);
    let size = vec2(sw.x + 10.0 + galley.size().x, sw.y.max(galley.size().y) + 4.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let t = ui.ctx().animate_bool(response.id, *on);
    let track = Rect::from_min_size(pos2(rect.left(), rect.center().y - sw.y / 2.0), sw);
    let painter = ui.painter();
    if t > 0.0 {
        glow(painter, track, 10.0, CYAN.gamma_multiply(0.35 * t), 10.0);
    }
    let off = Color32::from_rgb(0x2b, 0x2e, 0x38);
    painter.rect_filled(track, 10.0, lerp_color(off, CYAN.gamma_multiply(0.85), t));
    painter.rect_stroke(track, 10.0, Stroke::new(1.0, Color32::from_rgb(0x3c, 0x40, 0x4c)), StrokeKind::Inside);
    let x = egui::lerp(track.left() + 10.0..=track.right() - 10.0, t);
    painter.circle_filled(pos2(x, track.center().y + 1.0), 7.5, Color32::from_black_alpha(90));
    painter.circle_filled(pos2(x, track.center().y), 7.0, if response.hovered() { Color32::WHITE } else { TEXT });
    let color = if response.hovered() { Color32::WHITE } else { TEXT };
    painter.galley(pos2(track.right() + 10.0, rect.center().y - galley.size().y / 2.0), galley, color);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}
