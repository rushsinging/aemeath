use super::{UiPreferences, UiPreferencesChange, UiPreferencesIntent};
use crate::tui::render::output::spacing::{MarkdownSpacingMode, MarkdownSpacingPolicy};

#[test]
fn default_preferences_use_normal_spacing() {
    assert_eq!(
        UiPreferences::default().markdown_spacing().mode(),
        MarkdownSpacingMode::Normal
    );
}

#[test]
fn applying_changed_spacing_updates_once_and_equal_policy_is_noop() {
    let mut preferences = UiPreferences::default();
    let compact = MarkdownSpacingPolicy::compact();

    assert_eq!(
        preferences.apply(UiPreferencesIntent::MarkdownSpacingChanged(compact)),
        UiPreferencesChange::MarkdownSpacingChanged
    );
    assert_eq!(preferences.markdown_spacing(), compact);
    assert_eq!(
        preferences.apply(UiPreferencesIntent::MarkdownSpacingChanged(compact)),
        UiPreferencesChange::Unchanged
    );
}
