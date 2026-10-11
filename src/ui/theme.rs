//! Colours, fonts and the egui style for the "hardware console" look.
//!
//! Chrome (panels, cards, text, borders, the accent) comes from the active
//! [`Palette`], which has a dark and a light variant. Hardware (LCDs, fader
//! slots, LEDs, knobs and caps) looks the same in both, like a real console.

use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Vec2,
};

use crate::config::{Accent, Settings, ThemeMode};

pub use egui_phosphor::regular as icon;

pub const SLOT: Color32 = Color32::from_rgb(0x08, 0x09, 0x0c);

pub const LCD_BG: Color32 = Color32::from_rgb(0x0a, 0x13, 0x10);
pub const LCD_TEXT: Color32 = Color32::from_rgb(0x7f, 0xff, 0xd4);
pub const LCD_DIM: Color32 = Color32::from_rgb(0x2c, 0x5a, 0x4c);

/// LED colours, matching the button roles on the hardware.
pub const MUTE: Color32 = Color32::from_rgb(0xff, 0x3b, 0x3b);
pub const ASSIGN: Color32 = Color32::from_rgb(0xff, 0xb0, 0x00);
pub const RUN: Color32 = Color32::from_rgb(0x39, 0xd3, 0x53);

pub const METER_LOW: Color32 = Color32::from_rgb(0x39, 0xd3, 0x53);
pub const METER_MID: Color32 = Color32::from_rgb(0xe9, 0xc4, 0x6a);
pub const METER_HIGH: Color32 = Color32::from_rgb(0xff, 0x3b, 0x3b);

pub const RADIUS: u8 = 6;

/// Colours a strip can be given; glows and LCD text use them.
pub const STRIP_COLORS: [(Color32, &str); 8] = [
    (Color32::from_rgb(0x4c, 0xc9, 0xf0), "Cyan"),
    (Color32::from_rgb(0x5b, 0x8d, 0xff), "Blue"),
    (Color32::from_rgb(0xa7, 0x8b, 0xfa), "Violet"),
    (Color32::from_rgb(0xf4, 0x72, 0xb6), "Pink"),
    (Color32::from_rgb(0xff, 0x5c, 0x5c), "Red"),
    (Color32::from_rgb(0xff, 0x96, 0x3b), "Orange"),
    (Color32::from_rgb(0xfa, 0xd0, 0x2c), "Yellow"),
    (Color32::from_rgb(0x4a, 0xde, 0x80), "Green"),
];

/// A strip's colour: its own, or the accent.
pub fn strip_color(index: Option<u8>) -> Color32 {
    index.and_then(|i| STRIP_COLORS.get(i as usize)).map_or_else(|| pal().accent, |c| c.0)
}

/// Spacing: the base unit, and the gap under page headers.
pub const SP_S: f32 = 8.0;
pub const SP_M: f32 = 14.0;

/// Colours for things drawn as hardware on any theme (the controller diagram).
pub mod hw {
    use eframe::egui::Color32;
    pub const TEXT: Color32 = Color32::from_rgb(0xdc, 0xde, 0xe6);
    pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8a, 0x8e, 0x9c);
    pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x5a, 0x5e, 0x6b);
    pub const BODY_TOP: Color32 = Color32::from_rgb(0x2a, 0x2c, 0x35);
    pub const BODY_BOTTOM: Color32 = Color32::from_rgb(0x1c, 0x1d, 0x23);
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Palette {
    pub dark: bool,
    pub panel: Color32,
    pub panel_deep: Color32,
    pub faint: Color32,
    pub strip_top: Color32,
    pub strip_bottom: Color32,
    pub card: Color32,
    /// A panel inside a card.
    pub inset: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub control: Color32,
    pub control_hover: Color32,
    pub control_stroke: Color32,
    pub control_hover_stroke: Color32,
    pub text: Color32,
    /// Hovered or emphasised text.
    pub text_strong: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub tick: Color32,
    pub accent: Color32,
    /// Fill for selected things, a quiet version of the accent.
    pub accent_bg: Color32,
    /// Value readouts printed on the chassis.
    pub readout: Color32,
    /// Text versions of the LED colours, readable on the chassis.
    pub mute_text: Color32,
    pub warn_text: Color32,
}

const DARK: Palette = Palette {
    dark: true,
    panel: Color32::from_rgb(0x15, 0x16, 0x1b),
    panel_deep: Color32::from_rgb(0x0f, 0x10, 0x14),
    faint: Color32::from_rgb(0x1a, 0x1b, 0x21),
    strip_top: Color32::from_rgb(0x2a, 0x2c, 0x35),
    strip_bottom: Color32::from_rgb(0x1c, 0x1d, 0x23),
    card: Color32::from_rgb(0x1e, 0x20, 0x27),
    inset: Color32::from_rgb(0x22, 0x24, 0x2c),
    border: Color32::from_rgb(0x2c, 0x2f, 0x39),
    border_strong: Color32::from_rgb(0x34, 0x37, 0x42),
    control: Color32::from_rgb(0x2b, 0x2e, 0x38),
    control_hover: Color32::from_rgb(0x36, 0x3a, 0x47),
    control_stroke: Color32::from_rgb(0x46, 0x4a, 0x57),
    control_hover_stroke: Color32::from_rgb(0x4a, 0x4f, 0x5f),
    text: Color32::from_rgb(0xdc, 0xde, 0xe6),
    text_strong: Color32::WHITE,
    text_dim: Color32::from_rgb(0x94, 0x98, 0xa6),
    text_faint: Color32::from_rgb(0x76, 0x7a, 0x88),
    tick: Color32::from_rgb(0x3a, 0x3d, 0x48),
    accent: Color32::from_rgb(0x4c, 0xc9, 0xf0),
    accent_bg: Color32::from_rgb(0x1f, 0x5f, 0x78),
    readout: LCD_TEXT,
    mute_text: Color32::from_rgb(0xff, 0x6b, 0x6b),
    warn_text: ASSIGN,
};

const LIGHT: Palette = Palette {
    dark: false,
    panel: Color32::from_rgb(0xec, 0xee, 0xf2),
    panel_deep: Color32::from_rgb(0xdf, 0xe2, 0xe8),
    faint: Color32::from_rgb(0xe5, 0xe8, 0xed),
    strip_top: Color32::from_rgb(0xfa, 0xfb, 0xfc),
    strip_bottom: Color32::from_rgb(0xe2, 0xe5, 0xeb),
    card: Color32::from_rgb(0xfd, 0xfd, 0xfe),
    inset: Color32::from_rgb(0xf2, 0xf4, 0xf7),
    border: Color32::from_rgb(0xd5, 0xd9, 0xe1),
    border_strong: Color32::from_rgb(0xc4, 0xc9, 0xd3),
    control: Color32::from_rgb(0xe6, 0xe9, 0xee),
    control_hover: Color32::from_rgb(0xda, 0xde, 0xe6),
    control_stroke: Color32::from_rgb(0xbd, 0xc2, 0xcd),
    control_hover_stroke: Color32::from_rgb(0xa5, 0xab, 0xb8),
    text: Color32::from_rgb(0x1b, 0x1e, 0x25),
    text_strong: Color32::BLACK,
    text_dim: Color32::from_rgb(0x55, 0x5b, 0x6a),
    text_faint: Color32::from_rgb(0x76, 0x7c, 0x8c),
    tick: Color32::from_rgb(0xb4, 0xb9, 0xc4),
    accent: Color32::from_rgb(0x0a, 0x7e, 0xa8),
    accent_bg: Color32::from_rgb(0xcf, 0xea, 0xf4),
    readout: Color32::from_rgb(0x0b, 0x6b, 0x55),
    mute_text: Color32::from_rgb(0xc0, 0x1c, 0x1c),
    warn_text: Color32::from_rgb(0x8f, 0x55, 0x00),
};

impl Palette {
    pub fn new(dark: bool, accent: Color32) -> Palette {
        let mut p = if dark { DARK } else { LIGHT };
        p.accent = fit_contrast(accent, p.card, dark, 4.5);
        p.accent_bg = mix(p.card, p.accent, if dark { 0.32 } else { 0.18 });
        p
    }

    /// A drop shadow; lighter on the light theme.
    pub fn shadow(&self, alpha: u8) -> Color32 {
        Color32::from_black_alpha(if self.dark { alpha } else { alpha / 3 })
    }

    /// The light edge of a bevel.
    pub fn sheen(&self, alpha: u8) -> Color32 {
        Color32::from_white_alpha(if self.dark { alpha } else { alpha.saturating_mul(8).max(150) })
    }

    /// `c` laid over the panel at `alpha`, for dimming overlays.
    pub fn veil(&self, alpha: u8) -> Color32 {
        // Light: much thinner, or the dark hardware (displays, buttons) turns grey.
        let alpha = if self.dark { alpha } else { (alpha as f32 * 0.4) as u8 };
        Color32::from_rgba_unmultiplied(self.panel.r(), self.panel.g(), self.panel.b(), alpha)
    }
}

/// Accent presets: the colour as it looks on the dark theme. [`Palette::new`]
/// darkens it as needed for the light one.
pub fn accent_color(a: Accent, system: Option<[u8; 3]>) -> Color32 {
    match a {
        Accent::Cyan => Color32::from_rgb(0x4c, 0xc9, 0xf0),
        Accent::Violet => Color32::from_rgb(0xa7, 0x8b, 0xfa),
        Accent::Green => Color32::from_rgb(0x4a, 0xde, 0x80),
        Accent::Amber => Color32::from_rgb(0xfb, 0xbf, 0x24),
        Accent::Pink => Color32::from_rgb(0xf4, 0x72, 0xb6),
        Accent::System => match system {
            Some([r, g, b]) => Color32::from_rgb(r, g, b),
            None => accent_color(Accent::Cyan, None),
        },
    }
}

static PAL: RwLock<Palette> = RwLock::new(DARK);
static MOTION: AtomicBool = AtomicBool::new(true);

/// `animate_value_with_time`, or the target right away when the user
/// turned animations off.
pub fn anim(ctx: &egui::Context, id: egui::Id, target: f32, secs: f32) -> f32 {
    let secs = if MOTION.load(Ordering::Relaxed) { secs } else { 0.0 };
    ctx.animate_value_with_time(id, target, secs)
}

/// The user turned animations off.
pub fn reduce_motion() -> bool {
    !MOTION.load(Ordering::Relaxed)
}

/// The active palette.
pub fn pal() -> Palette {
    *PAL.read().unwrap()
}

/// Apply the palette the settings ask for, if it changed. Cheap enough to
/// call every frame.
pub fn sync(ctx: &egui::Context, settings: &Settings) {
    let system = crate::appearance::system();
    let dark = match settings.theme {
        ThemeMode::Dark => true,
        ThemeMode::Light => false,
        ThemeMode::System => system.dark.unwrap_or(true),
    };
    let want = Palette::new(dark, accent_color(settings.accent, system.accent));
    if want != pal() {
        *PAL.write().unwrap() = want;
        apply_visuals(ctx, &want);
    }
    MOTION.store(!settings.reduce_motion, Ordering::Relaxed);
    let motion = if settings.reduce_motion { 0.0 } else { 0.12 };
    if ctx.global_style().animation_time != motion {
        ctx.global_style_mut(|s| s.animation_time = motion);
    }
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// WCAG relative luminance.
pub fn luminance(c: Color32) -> f32 {
    let ch = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
}

pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Lighten (on dark) or darken (on light) `c` until it reads on `bg`.
fn fit_contrast(c: Color32, bg: Color32, dark: bool, min: f32) -> Color32 {
    let toward = if dark { Color32::WHITE } else { Color32::BLACK };
    (0..=20).map(|i| mix(c, toward, i as f32 / 20.0)).find(|&c| contrast(c, bg) >= min).unwrap_or(toward)
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("inter".into(), FontData::from_static(include_bytes!("../../assets/fonts/Inter-Regular.ttf")).into());
    fonts.font_data.insert(
        "inter-semibold".into(),
        FontData::from_static(include_bytes!("../../assets/fonts/Inter-SemiBold.ttf")).into(),
    );
    fonts.font_data.insert(
        "jetbrains".into(),
        FontData::from_static(include_bytes!("../../assets/fonts/JetBrainsMono-Medium.ttf")).into(),
    );
    // Our fonts first; egui's bundled fonts stay as fallbacks for symbols.
    fonts.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "inter".into());
    fonts.families.get_mut(&FontFamily::Monospace).unwrap().insert(0, "jetbrains".into());
    let mut semi = fonts.families[&FontFamily::Proportional].clone();
    semi[0] = "inter-semibold".into();
    fonts.families.insert(FontFamily::Name("semibold".into()), semi);
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    // Icons inside semibold and monospace text, too.
    if let Some(f) = fonts.families.get_mut(&FontFamily::Name("semibold".into())) {
        f.insert(1, "phosphor".into());
    }
    if let Some(f) = fonts.families.get_mut(&FontFamily::Monospace) {
        f.insert(1, "phosphor".into());
    }
    ctx.set_fonts(fonts);
    apply_visuals(ctx, &pal());
}

fn apply_visuals(ctx: &egui::Context, p: &Palette) {
    ctx.set_visuals(if p.dark { egui::Visuals::dark() } else { egui::Visuals::light() });
    ctx.global_style_mut(|s| {
        s.text_styles = [
            (TextStyle::Small, FontId::proportional(11.5)),
            (TextStyle::Body, FontId::proportional(14.0)),
            (TextStyle::Button, FontId::proportional(14.0)),
            (TextStyle::Heading, semibold(19.0)),
            (TextStyle::Monospace, mono(13.0)),
        ]
        .into();
        s.spacing.item_spacing = Vec2::new(SP_S, 7.0);
        s.spacing.button_padding = Vec2::new(10.0, 5.0);
        s.spacing.interact_size.y = 26.0;
        s.spacing.slider_rail_height = 4.0;
        s.spacing.menu_margin = egui::Margin::same(SP_S as i8);
        // Scrollbars get their own space instead of covering content.
        s.spacing.scroll = egui::style::ScrollStyle::solid();

        let v = &mut s.visuals;
        v.panel_fill = p.panel;
        v.window_fill = p.card;
        v.extreme_bg_color = p.panel_deep;
        v.text_edit_bg_color = Some(p.inset);
        v.faint_bg_color = p.faint;
        v.override_text_color = Some(p.text);
        v.hyperlink_color = p.accent;
        v.selection.bg_fill = p.accent_bg;
        v.selection.stroke = Stroke::new(1.0, p.accent);
        v.window_corner_radius = CornerRadius::same(10);
        v.menu_corner_radius = CornerRadius::same(8);
        v.window_stroke = Stroke::new(1.0, p.border_strong);
        v.window_shadow = egui::epaint::Shadow { offset: [0, 6], blur: 18, spread: 0, color: p.shadow(140) };
        v.popup_shadow = v.window_shadow;
        v.slider_trailing_fill = true;

        let w = &mut v.widgets;
        let r = CornerRadius::same(RADIUS);
        w.noninteractive.bg_fill = p.card;
        w.noninteractive.weak_bg_fill = p.card;
        w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
        w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
        w.noninteractive.corner_radius = r;
        w.inactive.bg_fill = p.control;
        w.inactive.weak_bg_fill = p.control;
        w.inactive.bg_stroke = Stroke::new(1.0, p.control_stroke);
        w.inactive.fg_stroke = Stroke::new(1.0, p.text);
        w.inactive.corner_radius = r;
        w.hovered.bg_fill = p.control_hover;
        w.hovered.weak_bg_fill = p.control_hover;
        w.hovered.bg_stroke = Stroke::new(1.0, p.control_hover_stroke);
        w.hovered.fg_stroke = Stroke::new(1.5, p.text_strong);
        w.hovered.corner_radius = r;
        w.active.bg_fill = p.accent_bg;
        w.active.weak_bg_fill = p.accent_bg;
        w.active.bg_stroke = Stroke::new(1.0, p.accent);
        w.active.fg_stroke = Stroke::new(1.5, p.text_strong);
        w.active.corner_radius = r;
        w.open = w.hovered;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readable_text_in_both_themes() {
        for dark in [true, false] {
            for a in Accent::ALL {
                let p = Palette::new(dark, accent_color(a, Some([0x3d, 0xae, 0xe9])));
                for bg in [p.card, p.panel, p.strip_top, p.strip_bottom] {
                    assert!(contrast(p.text, bg) >= 7.0, "text, dark={dark}");
                    assert!(contrast(p.text_dim, bg) >= 4.5, "text_dim, dark={dark}");
                    // Scale labels and placeholders: the 3:1 of non-text UI.
                    assert!(contrast(p.text_faint, bg) >= 3.0, "text_faint on {bg:?}, dark={dark}");
                    for c in [p.readout, p.mute_text, p.warn_text] {
                        assert!(contrast(c, bg) >= 4.5, "{c:?} on {bg:?}, dark={dark}");
                    }
                }
                assert!(contrast(p.accent, p.card) >= 4.5, "accent {a:?}, dark={dark}");
                assert!(contrast(p.text, p.accent_bg) >= 4.5, "text on accent_bg {a:?}, dark={dark}");
            }
        }
    }
}
