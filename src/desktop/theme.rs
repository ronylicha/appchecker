use eframe::egui::{Color32, FontFamily, FontId, Stroke, Style, TextStyle, Vec2, Visuals};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy)]
pub struct Tokens {
    pub background: Color32,
    pub surface: Color32,
    pub text: Color32,
    pub muted_text: Color32,
    pub accent: Color32,
    pub button_text: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub error: Color32,
    pub border: Color32,
    pub selection: Color32,
    pub accent_text: Color32,
    pub accent_hover: Color32,
    pub border_strong: Color32,
    pub danger_fill: Color32,
    pub surface_hover: Color32,
}

impl Theme {
    pub fn tokens(self) -> Tokens {
        match self {
            Self::Light => Tokens {
                background: Color32::from_rgb(246, 248, 251),
                surface: Color32::WHITE,
                text: Color32::from_rgb(23, 32, 51),
                muted_text: Color32::from_rgb(74, 88, 108),
                accent: Color32::from_rgb(36, 95, 214),
                button_text: Color32::WHITE,
                success: Color32::from_rgb(20, 125, 100),
                warning: Color32::from_rgb(145, 92, 0),
                error: Color32::from_rgb(180, 35, 54),
                border: Color32::from_rgb(208, 216, 226),
                selection: Color32::from_rgb(224, 235, 255),
                accent_text: Color32::from_rgb(29, 78, 216),
                accent_hover: Color32::from_rgb(29, 78, 216),
                border_strong: Color32::from_rgb(100, 116, 139),
                danger_fill: Color32::from_rgb(180, 35, 54),
                surface_hover: Color32::from_rgb(237, 242, 247),
            },
            Self::Dark => Tokens {
                background: Color32::from_rgb(17, 24, 39),
                surface: Color32::from_rgb(31, 41, 55),
                text: Color32::from_rgb(243, 244, 246),
                muted_text: Color32::from_rgb(203, 213, 225),
                accent: Color32::from_rgb(29, 78, 216),
                button_text: Color32::WHITE,
                success: Color32::from_rgb(52, 211, 153),
                warning: Color32::from_rgb(251, 191, 36),
                error: Color32::from_rgb(253, 164, 175),
                border: Color32::from_rgb(71, 85, 105),
                selection: Color32::from_rgb(44, 65, 105),
                accent_text: Color32::from_rgb(147, 197, 253),
                accent_hover: Color32::from_rgb(37, 99, 235),
                border_strong: Color32::from_rgb(148, 163, 184),
                danger_fill: Color32::from_rgb(180, 35, 54),
                surface_hover: Color32::from_rgb(55, 65, 81),
            },
        }
    }

    pub fn visuals(self) -> Visuals {
        let tokens = self.tokens();
        let mut visuals = match self {
            Self::Light => Visuals::light(),
            Self::Dark => Visuals::dark(),
        };
        visuals.panel_fill = tokens.background;
        visuals.window_fill = tokens.surface;
        visuals.faint_bg_color = tokens.surface;
        visuals.extreme_bg_color = tokens.background;
        visuals.selection.bg_fill = tokens.selection;
        visuals.selection.stroke = Stroke::new(2.0, tokens.accent_text);
        visuals.hyperlink_color = tokens.accent_text;
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, tokens.text);
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, tokens.border);
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, tokens.text);
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, tokens.border_strong);
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, tokens.text);
        visuals.widgets.active.bg_fill = tokens.selection;
        visuals.widgets.active.bg_stroke = Stroke::new(2.0, tokens.accent_text);
        visuals.widgets.open.fg_stroke = Stroke::new(1.0, tokens.text);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, tokens.text);
        visuals.widgets.hovered.bg_fill = tokens.surface_hover;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, tokens.accent_text);
        visuals
    }

    pub fn text_style(self) -> Style {
        let text_styles = [
            (
                TextStyle::Heading,
                FontId::new(24.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(16.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(16.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(13.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(14.0, FontFamily::Monospace),
            ),
        ]
        .into();
        Style {
            visuals: self.visuals(),
            text_styles,
            spacing: eframe::egui::Spacing {
                button_padding: Vec2::new(14.0, 8.0),
                item_spacing: Vec2::new(10.0, 8.0),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

pub fn contrast_ratio(foreground: Color32, background: Color32) -> f32 {
    let (lighter, darker) = {
        let foreground_luminance = luminance(foreground);
        let background_luminance = luminance(background);
        if foreground_luminance > background_luminance {
            (foreground_luminance, background_luminance)
        } else {
            (background_luminance, foreground_luminance)
        }
    };
    (lighter + 0.05) / (darker + 0.05)
}

fn luminance(color: Color32) -> f32 {
    fn channel(value: u8) -> f32 {
        let value = value as f32 / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopView {
    Projects,
    Updates,
    Remediation,
    Activity,
}

impl DesktopView {
    pub const ALL: [Self; 4] = [
        Self::Projects,
        Self::Updates,
        Self::Remediation,
        Self::Activity,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Projects => "Projects",
            Self::Updates => "Updates",
            Self::Remediation => "AI remediation",
            Self::Activity => "Activity",
        }
    }
}
