//! Small readability adjustments to the original egui workbench.
use eframe::egui::{self, Color32, FontId, Stroke};

pub const ACCENT: Color32 = Color32::from_rgb(72, 213, 188);
pub const MUTED: Color32 = Color32::from_rgb(166, 166, 166);

pub fn install(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style
        .text_styles
        .insert(egui::TextStyle::Heading, FontId::proportional(21.0));
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Small, FontId::proportional(12.0));
    style.spacing.item_spacing = egui::vec2(9.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.interact_size.y = 28.0;
    style.spacing.window_margin = egui::Margin::same(16);
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(Color32::from_gray(215));
    visuals.panel_fill = Color32::from_gray(27);
    visuals.window_fill = Color32::from_gray(32);
    visuals.window_corner_radius = 5.into();
    visuals.selection.bg_fill = Color32::from_rgb(35, 76, 72);
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
    ] {
        widget.corner_radius = 4.into();
    }
    style.visuals = visuals;
    ctx.set_style_of(egui::Theme::Dark, style);
}
