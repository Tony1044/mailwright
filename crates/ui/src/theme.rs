//! Design tokens (light and dark), interface fonts and the egui style, in the Craft family's
//! design: Inter for the interface, a neutral ribbon, one app colour.

use std::sync::Arc;

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, Visuals};
use serde::{Deserialize, Serialize};

/// MailCraft's app colour, and a darker ink of it for text on light backgrounds.
pub const APP_COLOR: Color32 = Color32::from_rgb(0x0F, 0x6C, 0xBD);
pub const APP_INK: Color32 = Color32::from_rgb(0x0C, 0x5A, 0x9E);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    Light,
    Dark,
    #[default]
    System,
}

impl Appearance {
    pub const ALL: [Appearance; 3] = [Appearance::System, Appearance::Light, Appearance::Dark];
    pub fn label(self) -> &'static str {
        match self {
            Appearance::Light => "Light",
            Appearance::Dark => "Dark",
            Appearance::System => "Use system setting",
        }
    }
    pub fn preference(self) -> egui::ThemePreference {
        match self {
            Appearance::Light => egui::ThemePreference::Light,
            Appearance::Dark => egui::ThemePreference::Dark,
            Appearance::System => egui::ThemePreference::System,
        }
    }
}

/// Every colour the interface uses.
#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub dark: bool,
    pub title_bar: Color32,
    pub ribbon: Color32,
    pub tab_strip: Color32,
    pub canvas: Color32,
    pub panel: Color32,
    pub list: Color32,
    pub reading: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_disabled: Color32,
    pub icon: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub checked: Color32,
    /// A selected list row (with and without focus).
    pub selected: Color32,
    pub selected_unfocused: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub on_accent: Color32,
    pub input: Color32,
    pub input_border: Color32,
    pub group_label: Color32,
    pub status_bar: Color32,
    pub menu: Color32,
    pub header_bg: Color32,
    pub info_bar: Color32,
    pub info_bar_text: Color32,
    pub error_bar: Color32,
    pub flag: Color32,
    pub high: Color32,
    pub green: Color32,
    pub today: Color32,
    pub grid_line: Color32,
    pub grid_half: Color32,
    pub work_hours: Color32,
    pub off_hours: Color32,
    pub now_line: Color32,
}

impl Tokens {
    pub fn light() -> Tokens {
        Tokens {
            dark: false,
            title_bar: Color32::from_rgb(0xF0, 0xF0, 0xF0),
            ribbon: Color32::from_rgb(0xF9, 0xF9, 0xF9),
            tab_strip: Color32::from_rgb(0xF0, 0xF0, 0xF0),
            canvas: Color32::from_rgb(0xE6, 0xE6, 0xE6),
            panel: Color32::from_rgb(0xFA, 0xFA, 0xFA),
            list: Color32::WHITE,
            reading: Color32::WHITE,
            border: Color32::from_rgb(0xE1, 0xE1, 0xE1),
            border_strong: Color32::from_rgb(0xC8, 0xC8, 0xC8),
            text: Color32::from_rgb(0x24, 0x24, 0x24),
            text_dim: Color32::from_rgb(0x61, 0x61, 0x61),
            text_disabled: Color32::from_rgb(0xA8, 0xA8, 0xA8),
            icon: Color32::from_rgb(0x42, 0x42, 0x42),
            hover: Color32::from_rgb(0xE6, 0xE6, 0xE6),
            pressed: Color32::from_rgb(0xD6, 0xD6, 0xD6),
            checked: Color32::from_rgb(0xD6, 0xE7, 0xF7),
            selected: Color32::from_rgb(0xCC, 0xE3, 0xF5),
            selected_unfocused: Color32::from_rgb(0xE5, 0xE5, 0xE5),
            accent: APP_COLOR,
            accent_text: APP_INK,
            on_accent: Color32::WHITE,
            input: Color32::WHITE,
            input_border: Color32::from_rgb(0x8A, 0x8A, 0x8A),
            group_label: Color32::from_rgb(0x70, 0x70, 0x70),
            status_bar: Color32::from_rgb(0xF0, 0xF0, 0xF0),
            menu: Color32::WHITE,
            header_bg: Color32::from_rgb(0xF5, 0xF5, 0xF5),
            info_bar: Color32::from_rgb(0xFF, 0xF4, 0xCE),
            info_bar_text: Color32::from_rgb(0x4A, 0x3A, 0x00),
            error_bar: Color32::from_rgb(0xFD, 0xE7, 0xE9),
            flag: Color32::from_rgb(0xC5, 0x0F, 0x1F),
            high: Color32::from_rgb(0xC5, 0x0F, 0x1F),
            green: Color32::from_rgb(0x10, 0x7C, 0x10),
            today: APP_COLOR,
            grid_line: Color32::from_rgb(0xE1, 0xE1, 0xE1),
            grid_half: Color32::from_rgb(0xF0, 0xF0, 0xF0),
            work_hours: Color32::WHITE,
            off_hours: Color32::from_rgb(0xF6, 0xF6, 0xF6),
            now_line: Color32::from_rgb(0xC5, 0x0F, 0x1F),
        }
    }

    pub fn dark() -> Tokens {
        Tokens {
            dark: true,
            title_bar: Color32::from_rgb(0x1F, 0x1F, 0x1F),
            ribbon: Color32::from_rgb(0x29, 0x29, 0x29),
            tab_strip: Color32::from_rgb(0x1F, 0x1F, 0x1F),
            canvas: Color32::from_rgb(0x14, 0x14, 0x14),
            panel: Color32::from_rgb(0x26, 0x26, 0x26),
            list: Color32::from_rgb(0x20, 0x20, 0x20),
            reading: Color32::from_rgb(0x1C, 0x1C, 0x1C),
            border: Color32::from_rgb(0x3A, 0x3A, 0x3A),
            border_strong: Color32::from_rgb(0x55, 0x55, 0x55),
            text: Color32::from_rgb(0xEE, 0xEE, 0xEE),
            text_dim: Color32::from_rgb(0xC0, 0xC0, 0xC0),
            text_disabled: Color32::from_rgb(0x6A, 0x6A, 0x6A),
            icon: Color32::from_rgb(0xDD, 0xDD, 0xDD),
            hover: Color32::from_rgb(0x3D, 0x3D, 0x3D),
            pressed: Color32::from_rgb(0x4A, 0x4A, 0x4A),
            checked: Color32::from_rgb(0x1F, 0x3A, 0x55),
            selected: Color32::from_rgb(0x1F, 0x3F, 0x5F),
            selected_unfocused: Color32::from_rgb(0x38, 0x38, 0x38),
            accent: Color32::from_rgb(0x47, 0x9E, 0xF5),
            accent_text: Color32::from_rgb(0x6C, 0xB4, 0xF7),
            on_accent: Color32::from_rgb(0x08, 0x18, 0x2C),
            input: Color32::from_rgb(0x1E, 0x1E, 0x1E),
            input_border: Color32::from_rgb(0x7A, 0x7A, 0x7A),
            group_label: Color32::from_rgb(0xA0, 0xA0, 0xA0),
            status_bar: Color32::from_rgb(0x1F, 0x1F, 0x1F),
            menu: Color32::from_rgb(0x2B, 0x2B, 0x2B),
            header_bg: Color32::from_rgb(0x2A, 0x2A, 0x2A),
            info_bar: Color32::from_rgb(0x43, 0x3A, 0x13),
            info_bar_text: Color32::from_rgb(0xF5, 0xE6, 0xB0),
            error_bar: Color32::from_rgb(0x44, 0x22, 0x26),
            flag: Color32::from_rgb(0xF0, 0x6B, 0x75),
            high: Color32::from_rgb(0xF0, 0x6B, 0x75),
            green: Color32::from_rgb(0x6C, 0xCB, 0x5F),
            today: Color32::from_rgb(0x47, 0x9E, 0xF5),
            grid_line: Color32::from_rgb(0x38, 0x38, 0x38),
            grid_half: Color32::from_rgb(0x2C, 0x2C, 0x2C),
            work_hours: Color32::from_rgb(0x20, 0x20, 0x20),
            off_hours: Color32::from_rgb(0x1A, 0x1A, 0x1A),
            now_line: Color32::from_rgb(0xF0, 0x6B, 0x75),
        }
    }

    pub fn get(ctx: &egui::Context) -> Tokens {
        if ctx.global_style().visuals.dark_mode { Tokens::dark() } else { Tokens::light() }
    }
}

pub fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let mut add = |name: &str, bytes: &'static [u8]| {
        fonts.font_data.insert(name.into(), Arc::new(FontData::from_static(bytes)));
    };
    add("Inter", include_bytes!("../../../assets/fonts/Inter-Regular.ttf"));
    add("InterMedium", include_bytes!("../../../assets/fonts/Inter-Medium.ttf"));
    add("InterSemiBold", include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf"));
    add("SourceSans", include_bytes!("../../../assets/fonts/SourceSans3-Regular.ttf"));
    add("SourceSerif", include_bytes!("../../../assets/fonts/SourceSerif4-Regular.ttf"));
    add("Mono", include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf"));
    if let Some(prop) = fonts.families.get_mut(&FontFamily::Proportional) {
        prop.insert(0, "Inter".into());
        prop.push("SourceSans".into());
    }
    let fallbacks: Vec<String> = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    if let Some(m) = fonts.families.get_mut(&FontFamily::Monospace) {
        m.insert(0, "Mono".into());
    }
    let with_fallbacks = |first: &str| {
        let mut v = vec![first.to_string()];
        v.extend(fallbacks.iter().filter(|f| *f != "Inter").cloned());
        v
    };
    fonts.families.insert(FontFamily::Name("medium".into()), with_fallbacks("InterMedium"));
    fonts.families.insert(FontFamily::Name("semibold".into()), with_fallbacks("InterSemiBold"));
    fonts.families.insert(FontFamily::Name("serif".into()), with_fallbacks("SourceSerif"));
    fonts
}

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}
pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("medium".into()))
}
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

pub fn apply(ctx: &egui::Context, appearance: Appearance) {
    ctx.set_fonts(fonts());
    ctx.set_visuals_of(egui::Theme::Light, visuals(&Tokens::light()));
    ctx.set_visuals_of(egui::Theme::Dark, visuals(&Tokens::dark()));
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(8.0, 3.0);
        s.spacing.interact_size = egui::vec2(24.0, 22.0);
        s.spacing.menu_margin = egui::Margin::same(6);
        s.text_styles.insert(egui::TextStyle::Body, regular(12.5));
        s.text_styles.insert(egui::TextStyle::Button, regular(12.5));
        s.text_styles.insert(egui::TextStyle::Small, regular(10.5));
        s.text_styles.insert(egui::TextStyle::Heading, semibold(17.0));
        s.text_styles.insert(egui::TextStyle::Monospace, FontId::monospace(12.0));
        s.interaction.tooltip_delay = 0.45;
    });
    set_appearance(ctx, appearance);
}

pub fn set_appearance(ctx: &egui::Context, appearance: Appearance) {
    ctx.options_mut(|o| {
        o.fallback_theme = egui::Theme::Light;
        o.theme_preference = appearance.preference();
    });
}

fn visuals(t: &Tokens) -> Visuals {
    let mut v = if t.dark { Visuals::dark() } else { Visuals::light() };
    v.panel_fill = t.ribbon;
    v.window_fill = t.menu;
    v.extreme_bg_color = t.input;
    v.faint_bg_color = t.panel;
    v.window_stroke = Stroke::new(1.0, t.border_strong);
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = CornerRadius::same(6);
    v.selection.bg_fill = t.selected;
    v.selection.stroke = Stroke::new(1.0, t.accent);
    v.hyperlink_color = t.accent_text;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.inactive.bg_fill = t.input;
    v.widgets.inactive.weak_bg_fill = t.input;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, t.input_border);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.inactive.corner_radius = CornerRadius::same(4);
    v.widgets.hovered.bg_fill = t.hover;
    v.widgets.hovered.weak_bg_fill = t.hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, t.border_strong);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.hovered.corner_radius = CornerRadius::same(4);
    v.widgets.active.bg_fill = t.pressed;
    v.widgets.active.weak_bg_fill = t.pressed;
    v.widgets.active.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.active.corner_radius = CornerRadius::same(4);
    v.widgets.open.bg_fill = t.pressed;
    v.widgets.open.weak_bg_fill = t.pressed;
    v.popup_shadow = egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(if t.dark { 120 } else { 40 }) };
    v.window_shadow = egui::Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(if t.dark { 140 } else { 50 }) };
    v
}

/// A colour for a person, from their address (avatars).
pub fn person_color(key: &str) -> Color32 {
    const PALETTE: [Color32; 10] = [
        Color32::from_rgb(0x0F, 0x6C, 0xBD),
        Color32::from_rgb(0x88, 0x17, 0x98),
        Color32::from_rgb(0x0B, 0x6A, 0x0B),
        Color32::from_rgb(0xC2, 0x39, 0xB3),
        Color32::from_rgb(0xCA, 0x50, 0x10),
        Color32::from_rgb(0x03, 0x83, 0x87),
        Color32::from_rgb(0x8E, 0x56, 0x2E),
        Color32::from_rgb(0x5C, 0x2E, 0x91),
        Color32::from_rgb(0xA4, 0x26, 0x2C),
        Color32::from_rgb(0x48, 0x60, 0x6E),
    ];
    let h = key.to_ascii_lowercase().bytes().fold(5381u32, |h, b| h.wrapping_mul(33) ^ b as u32);
    PALETTE[(h % PALETTE.len() as u32) as usize]
}

/// Calendar category and calendar colours.
pub fn category_color(name: &str) -> Color32 {
    match name.to_ascii_lowercase().as_str() {
        "red category" | "red" => Color32::from_rgb(0xE7, 0x48, 0x56),
        "orange category" | "orange" => Color32::from_rgb(0xF7, 0x63, 0x0C),
        "yellow category" | "yellow" => Color32::from_rgb(0xFF, 0xB9, 0x00),
        "green category" | "green" => Color32::from_rgb(0x16, 0xC6, 0x0C),
        "blue category" | "blue" => Color32::from_rgb(0x00, 0x78, 0xD4),
        "purple category" | "purple" => Color32::from_rgb(0x88, 0x17, 0x98),
        _ => person_color(name),
    }
}

/// Initials for an avatar.
pub fn initials(name: &str) -> String {
    let name = if name.contains('@') { name.split('@').next().unwrap_or(name) } else { name };
    let words: Vec<&str> = name.split(|c: char| c.is_whitespace() || c == '.' || c == '_' || c == '@').filter(|w| w.chars().next().is_some_and(char::is_alphanumeric)).collect();
    let mut s = String::new();
    if let Some(f) = words.first().and_then(|w| w.chars().next()) {
        s.extend(f.to_uppercase());
    }
    if words.len() > 1 {
        if let Some(l) = words.last().and_then(|w| w.chars().next()) {
            s.extend(l.to_uppercase());
        }
    }
    s
}

#[cfg(test)]
mod tests {
    #[test]
    fn initials_from_names_and_addresses() {
        assert_eq!(super::initials("Ada Lindqvist"), "AL");
        assert_eq!(super::initials("raj.patel@harbourview.example"), "RP");
        assert_eq!(super::initials("IT Service Desk"), "ID");
        assert_eq!(super::initials(""), "");
    }
}
