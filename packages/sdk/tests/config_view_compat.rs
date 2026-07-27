use sdk::{
    ConfigReloadedEvent, ConfigView, ElementSpacingView, MarkdownSpacingModeView,
    MarkdownSpacingOverridesView,
};

#[test]
fn old_config_view_json_defaults_markdown_spacing() {
    let view: ConfigView = serde_json::from_str(
        r#"{
            "model_name":"model",
            "provider":null,
            "has_api_key":false,
            "permission_mode":"ask",
            "markdown":true,
            "verbose":false,
            "context_size":128000,
            "logging_level":"info"
        }"#,
    )
    .unwrap();

    assert_eq!(view.markdown_spacing, MarkdownSpacingModeView::Normal);
    assert_eq!(
        view.markdown_spacing_overrides,
        MarkdownSpacingOverridesView::default()
    );
}

#[test]
fn config_view_round_trip_preserves_spacing_policy() {
    let view = ConfigView {
        markdown_spacing: MarkdownSpacingModeView::Compact,
        markdown_spacing_overrides: MarkdownSpacingOverridesView {
            paragraph: Some(ElementSpacingView {
                before: Some(0),
                after: Some(1),
            }),
            heading: Some(ElementSpacingView {
                before: Some(2),
                after: Some(3),
            }),
            list: Some(ElementSpacingView {
                before: Some(1),
                after: Some(0),
            }),
            code_block: Some(ElementSpacingView {
                before: Some(2),
                after: Some(2),
            }),
            table: Some(ElementSpacingView {
                before: Some(1),
                after: Some(1),
            }),
            blockquote: Some(ElementSpacingView {
                before: Some(0),
                after: Some(0),
            }),
        },
        ..ConfigView::default()
    };

    let json = serde_json::to_string(&view).unwrap();
    assert_eq!(serde_json::from_str::<ConfigView>(&json).unwrap(), view);
}

#[test]
fn old_reload_event_json_defaults_config_view() {
    let event: ConfigReloadedEvent =
        serde_json::from_str(r#"{"changed_keys":["ui.markdown_spacing"],"scopes":["immediate"]}"#)
            .unwrap();

    assert_eq!(event.view, ConfigView::default());
}
