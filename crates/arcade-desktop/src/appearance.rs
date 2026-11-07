//! Desktop color themes and shared readability settings.
use eframe::egui::{self, Color32, FontId, Stroke};

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    #[default]
    Teal,
    FieryRed,
    MidnightBlue,
}

impl Theme {
    pub const ALL: [Self; 3] = [Self::Teal, Self::FieryRed, Self::MidnightBlue];

    pub fn name(self) -> &'static str {
        match self {
            Self::Teal => "Classic teal",
            Self::FieryRed => "Fiery red",
            Self::MidnightBlue => "Midnight blue",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Teal => "teal",
            Self::FieryRed => "fiery-red",
            Self::MidnightBlue => "midnight-blue",
        }
    }

    pub fn from_id(id: &str) -> Self {
        Self::ALL.into_iter().find(|theme| theme.id() == id).unwrap_or_default()
    }

    pub fn palette(self) -> Palette {
        let (accent, selection) = match self {
            Self::Teal => ([72, 213, 188], [35, 76, 72]),
            Self::FieryRed => ([255, 102, 68], [113, 43, 29]),
            Self::MidnightBlue => ([117, 174, 255], [37, 61, 100]),
        };
        let color = |rgb: [u8; 3]| Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
        Palette {
            accent: color(accent),
            selection: color(selection),
            // All themes retain Classic teal's background surfaces.
            panel: Color32::from_gray(27),
            raised: Color32::from_gray(29),
            sidebar: Color32::from_gray(25),
            muted: Color32::from_gray(180),
        }
    }
}

pub struct Palette {
    pub accent: Color32,
    pub panel: Color32,
    pub raised: Color32,
    pub sidebar: Color32,
    pub selection: Color32,
    pub muted: Color32,
}

pub fn install(ctx: &egui::Context, theme: Theme) {
    let palette = theme.palette();
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
    visuals.panel_fill = palette.panel;
    visuals.window_fill = palette.raised;
    visuals.extreme_bg_color = palette.sidebar;
    visuals.faint_bg_color = palette.raised;
    visuals.hyperlink_color = palette.accent;
    visuals.window_corner_radius = 5.into();
    visuals.selection.bg_fill = palette.selection;
    visuals.selection.stroke = Stroke::new(1.0, palette.accent);
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
    ] {
        widget.corner_radius = 4.into();
    }
    // Give controls their own surface so they remain visible before hovering.
    visuals.widgets.inactive.bg_fill = Color32::from_gray(65);
    visuals.widgets.inactive.weak_bg_fill = Color32::from_gray(65);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_gray(120));
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::from_gray(235));
    visuals.widgets.hovered.bg_fill = Color32::from_gray(85);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_gray(85);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, palette.accent);
    visuals.widgets.active.bg_fill = palette.selection;
    visuals.widgets.active.weak_bg_fill = palette.selection;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, palette.accent);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.selection);
    style.visuals = visuals;
    ctx.set_style_of(egui::Theme::Dark, style);
}
