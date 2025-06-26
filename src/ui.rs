use eframe::egui::{self, Color32, Stroke, Style, Theme, style::Selection};

const MAIN_COLOR_LIGHT: (u8, u8, u8) = (159, 185, 194);
const MAIN_COLOR_DARK: (u8, u8, u8) = (133, 152, 158);

pub fn setup_custom_style(ctx: &egui::Context) {
    if ctx.style().visuals.dark_mode {
        ctx.style_mut_of(Theme::Dark, custom_colors)
    } else {
        ctx.style_mut_of(Theme::Light, custom_colors)
    }
}

fn custom_colors(style: &mut Style) {
    if !style.visuals.dark_mode {
        style.visuals.selection = Selection {
            bg_fill: Color32::from_rgb(MAIN_COLOR_LIGHT.0, MAIN_COLOR_LIGHT.1, MAIN_COLOR_LIGHT.2),
            stroke: Stroke::new(2.0, Color32::BLACK),
        };
        style.visuals.widgets.hovered.weak_bg_fill =
            Color32::from_rgb(MAIN_COLOR_LIGHT.0, MAIN_COLOR_LIGHT.1, MAIN_COLOR_LIGHT.2);
    } else {
        style.visuals.selection = Selection {
            bg_fill: Color32::from_rgb(MAIN_COLOR_DARK.0, MAIN_COLOR_DARK.1, MAIN_COLOR_DARK.2),
            stroke: Stroke::new(2.0, Color32::BLACK),
        };
        style.visuals.widgets.hovered.weak_bg_fill =
            Color32::from_rgb(MAIN_COLOR_DARK.0, MAIN_COLOR_DARK.1, MAIN_COLOR_DARK.2);
        style.visuals.widgets.inactive.weak_bg_fill = style.visuals.faint_bg_color;
    }
}
