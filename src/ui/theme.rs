//! Colours, fonts and the egui style for the "hardware console" look.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Vec2,
};

pub use egui_phosphor::regular as icon;

pub const PANEL: Color32 = Color32::from_rgb(0x15, 0x16, 0x1b);
pub const PANEL_DEEP: Color32 = Color32::from_rgb(0x0f, 0x10, 0x14);
pub const STRIP_TOP: Color32 = Color32::from_rgb(0x2a, 0x2c, 0x35);
pub const STRIP_BOTTOM: Color32 = Color32::from_rgb(0x1c, 0x1d, 0x23);
pub const CARD: Color32 = Color32::from_rgb(0x1e, 0x20, 0x27);
pub const SLOT: Color32 = Color32::from_rgb(0x08, 0x09, 0x0c);

pub const TEXT: Color32 = Color32::from_rgb(0xdc, 0xde, 0xe6);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8a, 0x8e, 0x9c);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x5a, 0x5e, 0x6b);

pub const LCD_BG: Color32 = Color32::from_rgb(0x0a, 0x13, 0x10);
pub const LCD_TEXT: Color32 = Color32::from_rgb(0x7f, 0xff, 0xd4);
pub const LCD_DIM: Color32 = Color32::from_rgb(0x2c, 0x5a, 0x4c);

/// LED colours, matching the button roles on the hardware.
pub const MUTE: Color32 = Color32::from_rgb(0xff, 0x3b, 0x3b);
pub const ASSIGN: Color32 = Color32::from_rgb(0xff, 0xb0, 0x00);
pub const RUN: Color32 = Color32::from_rgb(0x39, 0xd3, 0x53);
pub const CYAN: Color32 = Color32::from_rgb(0x4c, 0xc9, 0xf0);

pub const METER_LOW: Color32 = Color32::from_rgb(0x39, 0xd3, 0x53);
pub const METER_MID: Color32 = Color32::from_rgb(0xe9, 0xc4, 0x6a);
pub const METER_HIGH: Color32 = Color32::from_rgb(0xff, 0x3b, 0x3b);

pub const RADIUS: u8 = 6;

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
    // Icons inside semibold text, too.
    if let Some(f) = fonts.families.get_mut(&FontFamily::Name("semibold".into())) {
        f.insert(1, "phosphor".into());
    }
    ctx.set_fonts(fonts);

    ctx.set_visuals(egui::Visuals::dark());
    ctx.global_style_mut(|s| {
        s.text_styles = [
            (TextStyle::Small, FontId::proportional(11.5)),
            (TextStyle::Body, FontId::proportional(14.0)),
            (TextStyle::Button, FontId::proportional(14.0)),
            (TextStyle::Heading, semibold(19.0)),
            (TextStyle::Monospace, mono(13.0)),
        ]
        .into();
        s.spacing.item_spacing = Vec2::new(8.0, 7.0);
        s.spacing.button_padding = Vec2::new(10.0, 5.0);
        s.spacing.interact_size.y = 26.0;
        s.spacing.slider_rail_height = 4.0;
        s.spacing.menu_margin = egui::Margin::same(8);

        let v = &mut s.visuals;
        v.panel_fill = PANEL;
        v.window_fill = CARD;
        v.extreme_bg_color = PANEL_DEEP;
        v.faint_bg_color = Color32::from_rgb(0x1a, 0x1b, 0x21);
        v.override_text_color = Some(TEXT);
        v.hyperlink_color = CYAN;
        v.selection.bg_fill = Color32::from_rgb(0x1f, 0x5f, 0x78);
        v.selection.stroke = Stroke::new(1.0, CYAN);
        v.window_corner_radius = CornerRadius::same(10);
        v.menu_corner_radius = CornerRadius::same(8);
        v.window_stroke = Stroke::new(1.0, Color32::from_rgb(0x34, 0x37, 0x42));
        v.window_shadow =
            egui::epaint::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(140) };
        v.popup_shadow = v.window_shadow;
        v.slider_trailing_fill = true;

        let w = &mut v.widgets;
        let r = CornerRadius::same(RADIUS);
        w.noninteractive.bg_fill = CARD;
        w.noninteractive.weak_bg_fill = CARD;
        w.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x2c, 0x2f, 0x39));
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        w.noninteractive.corner_radius = r;
        w.inactive.bg_fill = Color32::from_rgb(0x2b, 0x2e, 0x38);
        w.inactive.weak_bg_fill = Color32::from_rgb(0x2b, 0x2e, 0x38);
        w.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x46, 0x4a, 0x57));
        w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        w.inactive.corner_radius = r;
        w.hovered.bg_fill = Color32::from_rgb(0x36, 0x3a, 0x47);
        w.hovered.weak_bg_fill = Color32::from_rgb(0x36, 0x3a, 0x47);
        w.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x4a, 0x4f, 0x5f));
        w.hovered.fg_stroke = Stroke::new(1.5, Color32::WHITE);
        w.hovered.corner_radius = r;
        w.active.bg_fill = Color32::from_rgb(0x1f, 0x5f, 0x78);
        w.active.weak_bg_fill = Color32::from_rgb(0x1f, 0x5f, 0x78);
        w.active.bg_stroke = Stroke::new(1.0, CYAN);
        w.active.fg_stroke = Stroke::new(1.5, Color32::WHITE);
        w.active.corner_radius = r;
        w.open = w.hovered;
    });
}
