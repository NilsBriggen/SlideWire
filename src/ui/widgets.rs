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
    // Edges follow the body colour, so light chrome and dark hardware both work.
    let light = theme::luminance(top) > 0.5;
    let (shadow, edge, sheen) = if light {
        (Color32::from_black_alpha(40), theme::mix(top, Color32::BLACK, 0.16), Color32::from_white_alpha(220))
    } else {
        (Color32::from_black_alpha(110), theme::mix(top, Color32::WHITE, 0.05), Color32::from_white_alpha(22))
    };
    // Drop shadow.
    painter.add(
        RectShape::filled(rect.translate(vec2(0.0, 3.0)), CornerRadius::same(radius), shadow).with_blur_width(10.0),
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
    painter.rect_stroke(rect, radius, Stroke::new(1.0, edge), StrokeKind::Inside);
    painter.line_segment(
        [pos2(rect.left() + radius as f32, rect.top() + 1.0), pos2(rect.right() - radius as f32, rect.top() + 1.0)],
        Stroke::new(1.0, sheen),
    );
}

/// Keyboard control of a focused fader or knob: arrows by 2%, Page Up/Down
/// by 10%, Home/End to the ends. Returns the new position.
fn keyboard_adjust(ui: &Ui, response: &Response, pos: f32) -> f32 {
    if !response.has_focus() {
        return pos;
    }
    // Arrows adjust the value here instead of moving focus.
    ui.memory_mut(|m| {
        m.set_focus_lock_filter(
            response.id,
            egui::EventFilter { horizontal_arrows: true, vertical_arrows: true, ..Default::default() },
        )
    });
    ui.input(|i| {
        use egui::Key;
        let n = |k| i.num_presses(k) as f32;
        let mut p = pos + 0.02 * (n(Key::ArrowUp) + n(Key::ArrowRight) - n(Key::ArrowDown) - n(Key::ArrowLeft));
        p += 0.1 * (n(Key::PageUp) - n(Key::PageDown));
        if i.key_pressed(Key::Home) {
            p = 0.0;
        }
        if i.key_pressed(Key::End) {
            p = 1.0;
        }
        p
    })
}

/// A ring around a widget that has keyboard focus.
fn focus_ring(ui: &Ui, response: &Response, rect: Rect, radius: f32) {
    if response.has_focus() {
        ui.painter().rect_stroke(rect.expand(3.0), radius, Stroke::new(2.0, pal().accent), StrokeKind::Outside);
    }
}

/// Accessible name and value of a fader or knob.
fn slider_info(ui: &Ui, response: &Response, pos: f32, label: &str) {
    let label = label.to_string();
    response.widget_info(|| egui::WidgetInfo::slider(ui.is_enabled(), (pos * 100.0).round() as f64, &label));
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

/// Long-throw fader. `pos` is 0..=1. Drag, scroll, double-click for 100%,
/// or focus it and use the keyboard. `ghost` marks where the hardware fader
/// is while it waits to pick up. `marks` are dB labels for the left side:
/// (position, dB). `label` names it for screen readers.
pub fn fader(
    ui: &mut Ui,
    pos: &mut f32,
    size: Vec2,
    color: Color32,
    ghost: Option<f32>,
    marks: &[(f32, i32)],
    label: &str,
) -> Response {
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
    *pos = keyboard_adjust(ui, &response, *pos).clamp(0.0, 1.0);
    if (*pos - before).abs() > f32::EPSILON {
        response.mark_changed();
    }
    slider_info(ui, &response, *pos, label);
    // Glide to values that change elsewhere (hardware, other apps); follow the mouse exactly.
    let glide = if response.dragged() { 0.0 } else { 0.08 };
    let shown = theme::anim(ui.ctx(), response.id.with("shown"), *pos, glide);
    focus_ring(ui, &response, track.expand2(vec2(cap.x / 2.0, cap.y / 2.0)), 6.0);

    let painter = ui.painter_at(rect.expand(16.0));
    // Even ticks on the right; dB marks (or the same ticks) on the left.
    for i in 0..=10 {
        let y = track.bottom() - travel * i as f32 / 10.0;
        let major = i % 5 == 0;
        let len = if major { 8.0 } else { 4.0 };
        let c = if major { pal().text_faint } else { pal().tick };
        if marks.is_empty() {
            painter.line_segment([pos2(track.left() - 6.0 - len, y), pos2(track.left() - 6.0, y)], Stroke::new(1.0, c));
        }
        painter.line_segment([pos2(track.right() + 6.0, y), pos2(track.right() + 6.0 + len, y)], Stroke::new(1.0, c));
    }
    if !marks.is_empty() {
        let font = mono(9.5);
        let mut last_y = f32::INFINITY;
        let all = marks.iter().map(|&(p, db)| (p, db.to_string())).chain([(0.0, "-∞".to_string())]);
        for (p, text) in all {
            let y = track.bottom() - travel * p;
            // Skip marks that would overlap the one above.
            if (last_y - y).abs() < 11.0 {
                continue;
            }
            last_y = y;
            let x = track.left() - 6.0;
            painter.line_segment([pos2(x - 4.0, y), pos2(x, y)], Stroke::new(1.0, pal().text_faint));
            painter.text(pos2(x - 6.0, y), Align2::RIGHT_CENTER, text, font.clone(), pal().text_faint);
        }
    }
    // Slot.
    painter.rect_filled(track, 3.0, SLOT);
    painter.line_segment([track.left_bottom(), track.right_bottom()], Stroke::new(1.0, pal().sheen(16)));
    // Level fill.
    let y = track.bottom() - travel * shown;
    let fill = Rect::from_min_max(pos2(track.center().x - 1.5, y), pos2(track.center().x + 1.5, track.bottom()));
    if shown > 0.0 {
        glow(&painter, fill, 2.0, color.gamma_multiply(0.5), 8.0);
        painter.rect_filled(fill, 1.5, color);
    }
    if let Some(g) = ghost {
        let gy = track.bottom() - travel * g.clamp(0.0, 1.0);
        let ghost_rect = Rect::from_center_size(pos2(track.center().x, gy), cap);
        painter.rect_stroke(ghost_rect, 3.0, Stroke::new(1.5, ASSIGN.gamma_multiply(0.8)), StrokeKind::Inside);
        // Dotted line from the hardware position to the volume.
        let (a, b) = (gy.min(y) + cap.y / 2.0, gy.max(y) - cap.y / 2.0);
        let mut yy = a;
        while yy < b {
            painter.circle_filled(pos2(track.right() + 3.0, yy), 1.0, ASSIGN.gamma_multiply(0.7));
            yy += 5.0;
        }
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

/// Rotary knob with a 270° value ring. Drag vertically, scroll, or use the
/// keyboard. `ghost` marks where the hardware knob is while it waits to pick up.
pub fn knob(ui: &mut Ui, pos: &mut f32, diameter: f32, color: Color32, ghost: Option<f32>, label: &str) -> Response {
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
    *pos = keyboard_adjust(ui, &response, *pos).clamp(0.0, 1.0);
    if (*pos - before).abs() > f32::EPSILON {
        response.mark_changed();
    }
    slider_info(ui, &response, *pos, label);
    // Glide to values that change elsewhere (hardware, other apps); follow the mouse exactly.
    let glide = if response.dragged() { 0.0 } else { 0.08 };
    let shown = theme::anim(ui.ctx(), response.id.with("shown"), *pos, glide);
    focus_ring(ui, &response, rect, diameter / 2.0);

    let painter = ui.painter_at(rect.expand(6.0));
    let c = rect.center();
    let r = diameter / 2.0;
    // Angles in screen space (y down): start at 135° (bottom-left), sweep 270°.
    let start = 0.75 * PI;
    let end = start + 1.5 * PI;
    let value = start + (end - start) * shown;
    let ring_r = r - 3.0;
    painter.add(Shape::line(arc_points(c, ring_r, start, end), Stroke::new(4.0, SLOT)));
    if shown > 0.001 {
        painter.add(Shape::line(arc_points(c, ring_r, start, value), Stroke::new(7.0, color.gamma_multiply(0.25))));
        painter.add(Shape::line(arc_points(c, ring_r, start, value), Stroke::new(3.0, color)));
    }
    if let Some(g) = ghost {
        let a = start + (end - start) * g.clamp(0.0, 1.0);
        let p = c + ring_r * vec2(a.cos(), a.sin());
        painter.circle_filled(p, 4.0, ASSIGN);
        painter.circle_stroke(p, 4.0, Stroke::new(1.0, Color32::from_black_alpha(160)));
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
    painter.line_segment([base, tip], Stroke::new(2.5, if hot { Color32::WHITE } else { hw::TEXT }));
    response.on_hover_cursor(egui::CursorIcon::ResizeVertical)
}

/// Square "M/S/R" button with an LED that glows in `color` when `on`.
/// `name` is what it does, for screen readers ("Mute Fader 1").
pub fn led_button(ui: &mut Ui, label: &str, on: bool, color: Color32, size: Vec2, name: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let name = name.to_string();
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, &name));
    focus_ring(ui, &response, rect, 6.0);
    let painter = ui.painter_at(rect.expand(10.0));
    let hot = response.hovered();
    let lit = theme::anim(ui.ctx(), response.id.with("lit"), if on { 1.0 } else { 0.0 }, 0.12);
    // Unlit cap, then the lit cap fading in over it.
    painter.add(
        RectShape::filled(rect.translate(vec2(0.0, 2.0)), CornerRadius::same(5), Color32::from_black_alpha(120))
            .with_blur_width(4.0),
    );
    let fill = if hot { Color32::from_rgb(0x33, 0x36, 0x41) } else { Color32::from_rgb(0x27, 0x29, 0x31) };
    painter.rect_filled(rect, 5.0, fill);
    painter.rect_stroke(rect, 5.0, Stroke::new(1.0, Color32::from_rgb(0x3a, 0x3d, 0x48)), StrokeKind::Inside);
    if lit > 0.0 {
        glow(&painter, rect, 5.0, color.gamma_multiply(0.45 * lit), 12.0);
        painter.rect_filled(rect, 5.0, color.gamma_multiply(lit));
        gradient_rect(
            &painter,
            rect.shrink2(vec2(1.0, 4.0)),
            Color32::from_white_alpha((40.0 * lit) as u8),
            Color32::from_black_alpha((10.0 * lit) as u8),
        );
    }
    let unlit_text = color.gamma_multiply(if hot { 0.9 } else { 0.6 });
    let text = theme::mix(unlit_text, Color32::from_rgb(0x14, 0x12, 0x10), lit);
    painter.text(rect.center(), Align2::CENTER_CENTER, label, semibold(13.0), text);
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

/// Scribble-strip style display with optional icon; long text is cut with
/// "…" and scrolls while hovered.
pub fn lcd(
    ui: &mut Ui,
    size: Vec2,
    icon: Option<egui::Image<'_>>,
    text: &str,
    dim: bool,
    tint: Option<Color32>,
) -> Response {
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
    let lit = tint.map_or(LCD_TEXT, |t| theme::mix(t, Color32::WHITE, 0.25));
    let color = if dim { theme::mix(lit, LCD_BG, 0.6) } else { lit };
    let font = mono(12.0);
    let galley = painter.layout_no_wrap(text.to_string(), font.clone(), color);
    let clip = painter.with_clip_rect(text_rect);
    let y = text_rect.center().y - galley.size().y / 2.0;
    if galley.size().x <= text_rect.width() {
        let x = text_rect.center().x - galley.size().x / 2.0;
        clip.galley(pos2(x, y), galley, color);
    } else if !response.hovered() {
        let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), font, color);
        job.wrap = egui::text::TextWrapping::truncate_at_width(text_rect.width());
        let cut = painter.layout_job(job);
        clip.galley(pos2(text_rect.left(), y), cut, color);
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
        .fill(pal().card)
        .stroke(Stroke::new(1.0, pal().border))
        .corner_radius(10)
        .inner_margin(egui::Margin::same(14))
        .shadow(egui::epaint::Shadow { offset: [0, 3], blur: 10, spread: 0, color: pal().shadow(90) })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !title.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(icon).size(17.0).color(pal().accent));
                    ui.label(egui::RichText::new(title).font(semibold(15.0)));
                });
                ui.add_space(4.0);
            }
            add_contents(ui)
        })
        .inner
}

/// A [`card`] whose body folds away when its header is clicked. `count`
/// shows next to the title; the open state is remembered.
pub fn collapsible_card(ui: &mut Ui, icon: &str, title: &str, count: usize, add_contents: impl FnOnce(&mut Ui)) {
    let id = ui.make_persistent_id(("card-open", title));
    let mut open = ui.data_mut(|d| *d.get_persisted_mut_or(id, true));
    card(ui, "", "", |ui| {
        let header = ui
            .horizontal(|ui| {
                ui.label(egui::RichText::new(icon).size(17.0).color(pal().accent));
                ui.label(egui::RichText::new(title).font(semibold(15.0)));
                ui.label(egui::RichText::new(count.to_string()).size(12.0).color(pal().text_faint));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let t = theme::anim(ui.ctx(), id.with("caret"), if open { 1.0 } else { 0.0 }, 0.12);
                    let caret = if t > 0.5 { theme::icon::CARET_UP } else { theme::icon::CARET_DOWN };
                    ui.label(egui::RichText::new(caret).color(pal().text_dim));
                });
            })
            .response;
        let click =
            ui.interact(header.rect, id.with("hdr"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        if click.clicked() {
            open = !open;
            ui.data_mut(|d| d.insert_persisted(id, open));
        }
        if open {
            ui.add_space(4.0);
            add_contents(ui);
        }
    });
}

/// Small rounded status chip with an LED dot.
pub fn pill(ui: &mut Ui, led: Option<Color32>, text: &str) -> Response {
    egui::Frame::new()
        .fill(pal().inset)
        .stroke(Stroke::new(1.0, pal().border_strong))
        .corner_radius(20)
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if let Some(c) = led {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                    if c != pal().text_faint {
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

/// A button showing only an icon; `label` is its tooltip and its name for
/// screen readers.
pub fn icon_only(ui: &mut Ui, icon: &str, label: &str) -> Response {
    let r = ui.add(egui::Button::new(egui::RichText::new(icon).color(pal().text_dim)).frame(false));
    let name = label.to_string();
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
    r.on_hover_text(label)
}

/// Like [`icon_only`], with a button frame.
pub fn icon_framed(ui: &mut Ui, icon: &str, label: &str) -> Response {
    let r = ui.button(icon);
    let name = label.to_string();
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
    r.on_hover_text(label)
}

/// Hint text in a faint colour.
pub fn hint(ui: &mut Ui, text: &str) -> Response {
    ui.label(egui::RichText::new(text).size(12.5).color(pal().text_dim))
}

/// A row of mutually exclusive options in one rounded track.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> Response {
    let p = pal();
    let font = egui::FontId::proportional(13.5);
    let galleys: Vec<_> =
        options.iter().map(|(_, l)| ui.painter().layout_no_wrap(l.to_string(), font.clone(), p.text)).collect();
    let seg_w = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max) + 24.0;
    let size = vec2(seg_w * options.len() as f32 + 4.0, 28.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 8.0, p.control);
    painter.rect_stroke(rect, 8.0, Stroke::new(1.0, p.border_strong), StrokeKind::Inside);
    let selected = options.iter().position(|(v, _)| v == value).unwrap_or(0);
    // The highlight slides between segments.
    let x = ui.ctx().animate_value_with_time(response.id.with("seg"), selected as f32, 0.12);
    let hl = Rect::from_min_size(pos2(rect.left() + 2.0 + x * seg_w, rect.top() + 2.0), vec2(seg_w, size.y - 4.0));
    painter.rect_filled(hl, 6.0, p.accent_bg);
    painter.rect_stroke(hl, 6.0, Stroke::new(1.0, p.accent.gamma_multiply(0.6)), StrokeKind::Inside);
    for (i, ((v, _), g)) in options.iter().zip(galleys).enumerate() {
        let seg = Rect::from_min_size(pos2(rect.left() + 2.0 + i as f32 * seg_w, rect.top()), vec2(seg_w, size.y));
        let r = ui.interact(seg, response.id.with(i), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let label = options[i].1.to_string();
        r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, i == selected, &label));
        focus_ring(ui, &r, seg.shrink(2.0), 6.0);
        if r.clicked() && *value != *v {
            *value = *v;
            response.mark_changed();
        }
        let color = if i == selected {
            p.text_strong
        } else if r.hovered() {
            p.text
        } else {
            p.text_dim
        };
        ui.painter().galley(seg.center() - g.size() / 2.0, g, color);
    }
    response
}

/// Round colour swatch; ringed when selected.
pub fn swatch(ui: &mut Ui, color: Color32, selected: bool, tooltip: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
    let name = tooltip.to_string();
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, &name));
    focus_ring(ui, &response, rect.shrink(2.0), 14.0);
    let hot = ui.ctx().animate_bool(response.id, response.hovered() || selected);
    let painter = ui.painter();
    let c = rect.center();
    if selected {
        painter.circle_stroke(c, 13.0, Stroke::new(2.0, color));
    }
    painter.circle_filled(c, 8.5 + 1.5 * hot, color);
    painter.circle_stroke(c, 8.5 + 1.5 * hot, Stroke::new(1.0, pal().shadow(90)));
    response.on_hover_text(tooltip).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Pill-shaped on/off switch with a label; glows cyan when on.
pub fn toggle(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(label.to_string(), egui::FontId::proportional(14.0), pal().text);
    let sw = vec2(36.0, 20.0);
    let size = vec2(sw.x + 10.0 + galley.size().x, sw.y.max(galley.size().y) + 4.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let (state, name) = (*on, label.to_string());
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, state, &name));
    let t = ui.ctx().animate_bool(response.id, *on);
    let track = Rect::from_min_size(pos2(rect.left(), rect.center().y - sw.y / 2.0), sw);
    focus_ring(ui, &response, track, 12.0);
    let painter = ui.painter();
    if t > 0.0 {
        glow(painter, track, 10.0, pal().accent.gamma_multiply(0.35 * t), 10.0);
    }
    let p = pal();
    painter.rect_filled(track, 10.0, theme::mix(p.control, p.accent, t * 0.85));
    painter.rect_stroke(track, 10.0, Stroke::new(1.0, p.control_stroke), StrokeKind::Inside);
    let x = egui::lerp(track.left() + 10.0..=track.right() - 10.0, t);
    painter.circle_filled(pos2(x, track.center().y + 1.0), 7.5, Color32::from_black_alpha(90));
    let thumb = if response.hovered() { Color32::WHITE } else { Color32::from_rgb(0xec, 0xee, 0xf2) };
    painter.circle_filled(pos2(x, track.center().y), 7.0, thumb);
    let color = if response.hovered() { p.text_strong } else { p.text };
    painter.galley(pos2(track.right() + 10.0, rect.center().y - galley.size().y / 2.0), galley, color);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
