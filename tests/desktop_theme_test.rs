use appchecker::desktop::theme::{DesktopView, Theme, contrast_ratio};

#[test]
fn text_and_controls_meet_accessible_contrast() {
    for theme in [Theme::Light, Theme::Dark] {
        let tokens = theme.tokens();
        assert!(contrast_ratio(tokens.text, tokens.background) >= 4.5);
        assert!(contrast_ratio(tokens.muted_text, tokens.background) >= 4.5);
        assert!(contrast_ratio(tokens.button_text, tokens.accent) >= 4.5);
        assert!(contrast_ratio(tokens.error, tokens.background) >= 4.5);
        assert!(contrast_ratio(tokens.success, tokens.background) >= 4.5);
    }
}

#[test]
fn interactive_states_and_selected_cards_remain_legible() {
    for theme in [Theme::Light, Theme::Dark] {
        let tokens = theme.tokens();
        assert!(contrast_ratio(tokens.accent_text, tokens.background) >= 4.5);
        assert!(contrast_ratio(tokens.accent_text, tokens.surface) >= 4.5);
        assert!(contrast_ratio(tokens.border_strong, tokens.surface) >= 3.0);
        assert!(contrast_ratio(tokens.text, tokens.selection) >= 4.5);
        assert!(contrast_ratio(tokens.muted_text, tokens.selection) >= 4.5);
        assert!(contrast_ratio(tokens.button_text, tokens.danger_fill) >= 4.5);
        assert!(contrast_ratio(tokens.text, tokens.surface_hover) >= 4.5);
    }
}

#[test]
fn navigation_exposes_all_user_jobs() {
    let views = DesktopView::ALL;
    assert_eq!(views.len(), 4);
    assert_eq!(views[0].label(), "Projects");
    assert_eq!(views[1].label(), "Updates");
    assert_eq!(views[2].label(), "AI remediation");
    assert_eq!(views[3].label(), "Activity");
}
