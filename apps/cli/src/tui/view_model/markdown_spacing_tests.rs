use super::{ElementSpacing, MarkdownSpacingMode, MarkdownSpacingPolicy};

#[test]
fn sdk_config_view_maps_all_spacing_fields_without_shared_config_types() {
    let view = sdk::ConfigView {
        markdown_spacing: sdk::MarkdownSpacingModeView::Compact,
        markdown_spacing_overrides: sdk::MarkdownSpacingOverridesView {
            paragraph: Some(sdk::ElementSpacingView {
                before: Some(0),
                after: Some(1),
            }),
            heading: Some(sdk::ElementSpacingView {
                before: Some(2),
                after: Some(3),
            }),
            list: Some(sdk::ElementSpacingView {
                before: Some(1),
                after: Some(0),
            }),
            code_block: Some(sdk::ElementSpacingView {
                before: Some(2),
                after: Some(2),
            }),
            table: Some(sdk::ElementSpacingView {
                before: Some(1),
                after: Some(1),
            }),
            blockquote: Some(sdk::ElementSpacingView {
                before: Some(0),
                after: Some(0),
            }),
        },
        ..Default::default()
    };

    let policy = MarkdownSpacingPolicy::from(&view);
    let overrides = policy.overrides();

    assert_eq!(policy.mode(), MarkdownSpacingMode::Compact);
    assert_eq!(
        overrides.heading,
        Some(ElementSpacing {
            before: Some(2),
            after: Some(3),
        })
    );
    assert!(overrides.paragraph.is_some());
    assert!(overrides.list.is_some());
    assert!(overrides.code_block.is_some());
    assert!(overrides.table.is_some());
    assert!(overrides.blockquote.is_some());
}
