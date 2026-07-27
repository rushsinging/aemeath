use super::{
    ElementSpacingOverride, MarkdownSpacingMode, MarkdownSpacingOverrides, SpacingLines, UiConfig,
};

#[test]
fn default_ui_uses_normal_markdown_spacing_without_overrides() {
    let config = UiConfig::default();

    assert_eq!(config.markdown_spacing, MarkdownSpacingMode::Normal);
    assert_eq!(
        config.markdown_spacing_overrides,
        MarkdownSpacingOverrides::default()
    );
}

#[test]
fn markdown_spacing_accepts_all_typed_overrides() {
    let config: UiConfig = serde_json::from_str(
        r#"{
            "markdown_spacing": "compact",
            "markdown_spacing_overrides": {
                "paragraph": { "before": 0, "after": 1 },
                "heading": { "before": 2, "after": 3 },
                "list": { "before": 1, "after": 0 },
                "code_block": { "before": 2, "after": 2 },
                "table": { "before": 1, "after": 1 },
                "blockquote": { "before": 0, "after": 0 }
            }
        }"#,
    )
    .unwrap();

    assert_eq!(config.markdown_spacing, MarkdownSpacingMode::Compact);
    assert_eq!(
        config.markdown_spacing_overrides.heading,
        Some(ElementSpacingOverride {
            before: Some(SpacingLines::new(2).unwrap()),
            after: Some(SpacingLines::new(3).unwrap()),
        })
    );
    assert!(config.markdown_spacing_overrides.paragraph.is_some());
    assert!(config.markdown_spacing_overrides.list.is_some());
    assert!(config.markdown_spacing_overrides.code_block.is_some());
    assert!(config.markdown_spacing_overrides.table.is_some());
    assert!(config.markdown_spacing_overrides.blockquote.is_some());
}

#[test]
fn markdown_spacing_rejects_unknown_mode_element_edge_and_out_of_range_lines() {
    for invalid in [
        r#"{"markdown_spacing":"dense"}"#,
        r#"{"markdown_spacing_overrides":{"image":{"before":1}}}"#,
        r#"{"markdown_spacing_overrides":{"heading":{"around":1}}}"#,
        r#"{"markdown_spacing_overrides":{"heading":{"before":9}}}"#,
    ] {
        assert!(
            serde_json::from_str::<UiConfig>(invalid).is_err(),
            "invalid config unexpectedly parsed: {invalid}"
        );
    }
}
