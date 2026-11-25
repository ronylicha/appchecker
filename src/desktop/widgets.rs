use super::theme::{DesktopView, Tokens};
use eframe::egui::{Button, Color32, Response, RichText, Ui, Vec2, WidgetText};

pub fn primary_button(text: impl Into<WidgetText>, tokens: Tokens) -> Button<'static> {
    Button::new(text.into().color(tokens.button_text).strong())
        .fill(tokens.accent)
        .corner_radius(8.0)
        .min_size(Vec2::new(112.0, 38.0))
}

pub fn primary_action(
    enabled: bool,
    text: impl Into<WidgetText>,
    tokens: Tokens,
) -> Button<'static> {
    if enabled {
        primary_button(text, tokens)
    } else {
        secondary_button(text, tokens)
    }
}

pub fn danger_button(text: impl Into<WidgetText>, tokens: Tokens) -> Button<'static> {
    Button::new(text.into().color(tokens.button_text).strong())
        .fill(tokens.danger_fill)
        .corner_radius(8.0)
        .min_size(Vec2::new(112.0, 38.0))
}

pub fn secondary_button(text: impl Into<WidgetText>, tokens: Tokens) -> Button<'static> {
    Button::new(text.into().color(tokens.text))
        .fill(tokens.surface)
        .stroke((1.0, tokens.border_strong))
        .corner_radius(8.0)
        .min_size(Vec2::new(104.0, 38.0))
}

pub fn navigation_button(view: DesktopView, selected: bool, tokens: Tokens) -> Button<'static> {
    Button::new(
        RichText::new(view.label())
            .color(if selected {
                tokens.accent_text
            } else {
                tokens.text
            })
            .strong(),
    )
    .fill(if selected {
        tokens.selection
    } else {
        Color32::TRANSPARENT
    })
    .stroke((if selected { 1.0 } else { 0.0 }, tokens.accent))
    .corner_radius(8.0)
    .min_size(Vec2::new(176.0, 42.0))
}

pub fn page_title(ui: &mut Ui, text: &str, tokens: Tokens) {
    ui.label(RichText::new(text).size(28.0).strong().color(tokens.text));
}

pub fn section_heading(ui: &mut Ui, text: &str, tokens: Tokens) {
    ui.label(RichText::new(text).size(20.0).strong().color(tokens.text));
}

pub fn card_title(ui: &mut Ui, text: &str, tokens: Tokens) {
    ui.label(RichText::new(text).size(17.0).strong().color(tokens.text));
}

pub fn muted_label(ui: &mut Ui, text: &str, tokens: Tokens) {
    ui.label(RichText::new(text).color(tokens.muted_text));
}

pub fn pill_badge(ui: &mut Ui, text: &str, color: Color32, tokens: Tokens) -> Response {
    egui::Frame::new()
        .fill(tokens.surface_hover)
        .stroke((1.0, tokens.border))
        .corner_radius(10.0)
        .inner_margin(egui::Margin::symmetric(8, 3))
        .show(ui, |ui| ui.label(RichText::new(text).color(color).strong()))
        .inner
}

use eframe::egui;
