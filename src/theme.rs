//! Look and feel: Inter typeface, flat neutral surfaces, one restrained accent, small corner radii.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Theme,
    ThemePreference, Visuals,
};

pub struct Palette {
    pub bg: Color32,
    pub surface: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    pub error: Color32,
}

const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xF6, 0xF6, 0xF3),
    surface: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    border: Color32::from_rgb(0xCD, 0xCD, 0xC6),
    text: Color32::from_rgb(0x1B, 0x1B, 0x19),
    muted: Color32::from_rgb(0x70, 0x70, 0x69),
    accent: Color32::from_rgb(0x1F, 0x4F, 0xB8),
    on_accent: Color32::WHITE,
    error: Color32::from_rgb(0xB4, 0x2B, 0x20),
};

const DARK: Palette = Palette {
    bg: Color32::from_rgb(0x15, 0x15, 0x14),
    surface: Color32::from_rgb(0x1E, 0x1E, 0x1C),
    border: Color32::from_rgb(0x37, 0x37, 0x34),
    text: Color32::from_rgb(0xEC, 0xEC, 0xE7),
    muted: Color32::from_rgb(0x9A, 0x9A, 0x93),
    accent: Color32::from_rgb(0x7F, 0xA6, 0xF5),
    on_accent: Color32::from_rgb(0x10, 0x14, 0x1F),
    error: Color32::from_rgb(0xF0, 0x7A, 0x6E),
};

pub fn palette(ctx: &egui::Context) -> &'static Palette {
    match ctx.theme() {
        Theme::Light => &LIGHT,
        Theme::Dark => &DARK,
    }
}

pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

pub fn install(ctx: &egui::Context, preference: &str) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert("Inter".into(), Arc::new(FontData::from_static(include_bytes!("../assets/Inter-Regular.ttf"))));
    fonts
        .font_data
        .insert("Inter-SemiBold".into(), Arc::new(FontData::from_static(include_bytes!("../assets/Inter-SemiBold.ttf"))));
    let fallback = fonts.families[&FontFamily::Proportional].clone();
    fonts.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "Inter".into());
    let mut bold = vec!["Inter-SemiBold".to_string()];
    bold.extend(fallback);
    fonts.families.insert(semibold(), bold);
    ctx.set_fonts(fonts);

    ctx.set_visuals_of(Theme::Light, visuals(&LIGHT, false));
    ctx.set_visuals_of(Theme::Dark, visuals(&DARK, true));
    ctx.all_styles_mut(|s| {
        s.text_styles = [
            (TextStyle::Small, FontId::new(12.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Heading, FontId::new(20.0, semibold())),
            (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
        ]
        .into();
        s.spacing.item_spacing = egui::vec2(10.0, 8.0);
        s.spacing.button_padding = egui::vec2(14.0, 6.0);
        s.spacing.interact_size.y = 32.0;
        s.spacing.combo_width = 0.0;
        s.spacing.window_margin = egui::Margin::same(16);
    });
    set_preference(ctx, preference);
}

pub fn set_preference(ctx: &egui::Context, preference: &str) {
    ctx.set_theme(match preference {
        "light" => ThemePreference::Light,
        "dark" => ThemePreference::Dark,
        _ => ThemePreference::System,
    });
}

fn visuals(p: &Palette, dark: bool) -> Visuals {
    let mut v = if dark { Visuals::dark() } else { Visuals::light() };
    let radius = CornerRadius::same(3);
    v.panel_fill = p.bg;
    v.window_fill = p.surface;
    v.extreme_bg_color = p.surface;
    v.text_edit_bg_color = Some(p.surface);
    v.faint_bg_color = p.surface;
    v.code_bg_color = p.surface;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(4);
    v.menu_corner_radius = CornerRadius::same(4);
    v.window_shadow = egui::Shadow::NONE;
    v.popup_shadow = egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(if dark { 120 } else { 36 }) };
    v.hyperlink_color = p.accent;
    v.error_fg_color = p.error;
    v.selection.bg_fill = p.accent.gamma_multiply(0.30);
    v.selection.stroke = Stroke::new(1.0, p.text);
    v.weak_text_color = Some(p.muted);
    v.striped = false;

    let w = &mut v.widgets;
    for (state, fill, stroke_color) in [
        (&mut w.noninteractive, p.bg, p.border),
        (&mut w.inactive, p.surface, p.border),
        (&mut w.hovered, p.surface, p.muted),
        (&mut w.active, p.surface, p.accent),
        (&mut w.open, p.surface, p.muted),
    ] {
        state.corner_radius = radius;
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0, stroke_color);
        state.fg_stroke = Stroke::new(1.0, p.text);
        state.expansion = 0.0;
    }
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    v
}
