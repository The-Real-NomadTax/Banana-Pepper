//! Desktop color themes and shared readability settings.
use eframe::egui::{self, Color32, FontId, Stroke};

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    #[default]
    Teal,
    FieryRed,
    MidnightBlue,
    Emerald,
    Lime,
    Amber,
    Gold,
    Tangerine,
    Rose,
    Magenta,
    Violet,
    Lavender,
    Cyan,
    Ice,
    Ocean,
    Slate,
    Copper,
    Mint,
}

impl Theme {
    pub const ALL: [Self; 18] = [
        Self::Teal, Self::FieryRed, Self::MidnightBlue,
        Self::Emerald,
        Self::Lime,
        Self::Amber,
        Self::Gold,
        Self::Tangerine,
        Self::Rose,
        Self::Magenta,
        Self::Violet,
        Self::Lavender,
        Self::Cyan,
        Self::Ice,
        Self::Ocean,
        Self::Slate,
        Self::Copper,
        Self::Mint,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Teal => "Classic teal",
            Self::FieryRed => "Fiery red",
            Self::MidnightBlue => "Midnight blue",
            Self::Emerald => "Emerald",
            Self::Lime => "Electric lime",
            Self::Amber => "Arcade amber",
            Self::Gold => "Golden hour",
            Self::Tangerine => "Tangerine",
            Self::Rose => "Rose",
            Self::Magenta => "Neon magenta",
            Self::Violet => "Ultraviolet",
            Self::Lavender => "Lavender",
            Self::Cyan => "Electric cyan",
            Self::Ice => "Arctic ice",
            Self::Ocean => "Ocean",
            Self::Slate => "Silver slate",
            Self::Copper => "Copper",
            Self::Mint => "Mint",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Teal => "teal",
            Self::FieryRed => "fiery-red",
            Self::MidnightBlue => "midnight-blue",
            Self::Emerald => "emerald",
            Self::Lime => "lime",
            Self::Amber => "amber",
            Self::Gold => "gold",
            Self::Tangerine => "tangerine",
            Self::Rose => "rose",
            Self::Magenta => "magenta",
            Self::Violet => "violet",
            Self::Lavender => "lavender",
            Self::Cyan => "cyan",
            Self::Ice => "ice",
            Self::Ocean => "ocean",
            Self::Slate => "slate",
            Self::Copper => "copper",
            Self::Mint => "mint",
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
            Self::Emerald => ([77, 220, 143], [30, 81, 54]),
            Self::Lime => ([190, 230, 90], [63, 78, 31]),
            Self::Amber => ([255, 184, 65], [94, 65, 24]),
            Self::Gold => ([239, 208, 112], [83, 70, 32]),
            Self::Tangerine => ([255, 153, 78], [96, 53, 28]),
            Self::Rose => ([255, 137, 165], [93, 43, 59]),
            Self::Magenta => ([244, 111, 220], [85, 35, 79]),
            Self::Violet => ([185, 139, 255], [63, 43, 96]),
            Self::Lavender => ([201, 183, 255], [64, 55, 86]),
            Self::Cyan => ([77, 222, 246], [26, 74, 83]),
            Self::Ice => ([168, 225, 244], [47, 72, 82]),
            Self::Ocean => ([72, 176, 223], [25, 63, 87]),
            Self::Slate => ([191, 203, 220], [56, 63, 74]),
            Self::Copper => ([225, 166, 126], [82, 57, 42]),
            Self::Mint => ([157, 236, 201], [43, 77, 63]),
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
