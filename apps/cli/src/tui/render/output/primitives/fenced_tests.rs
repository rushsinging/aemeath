use super::*;
use crate::tui::render::output::spacing::{
    ElementSpacing, MarkdownSpacingMode, MarkdownSpacingOverrides,
};

fn render(text: &str, policy: MarkdownSpacingPolicy) -> Vec<RenderedLine> {
    render_fenced_markdown(text, Style::default().fg(theme::TEXT), 80, &policy)
}

#[test]
fn normal_preserves_single_source_gap_and_compact_removes_it() {
    let normal = render("one\n\n\ntwo", MarkdownSpacingPolicy::normal());
    let compact = render("one\n\n\ntwo", MarkdownSpacingPolicy::compact());

    assert_eq!(
        normal.iter().filter(|line| line.plain.is_empty()).count(),
        1
    );
    assert_eq!(
        compact.iter().filter(|line| line.plain.is_empty()).count(),
        0
    );
}

#[test]
fn overrides_take_precedence_and_adjacent_edges_use_max() {
    let policy = MarkdownSpacingPolicy::new_for_test(
        MarkdownSpacingMode::Compact,
        MarkdownSpacingOverrides {
            heading: Some(ElementSpacing {
                before: Some(1),
                after: Some(2),
            }),
            paragraph: Some(ElementSpacing {
                before: Some(1),
                after: Some(0),
            }),
            ..Default::default()
        },
    );
    let lines = render("# title\nparagraph", policy);

    assert!(lines[0].plain.is_empty());
    assert_eq!(
        lines.iter().filter(|line| line.plain.is_empty()).count(),
        3,
        "leading 1 + max(heading.after=2, paragraph.before=1)"
    );
}

#[test]
fn fence_internal_blank_lines_survive_compact_and_style_does_not_leak() {
    let lines = render(
        "before\n\n```\ncode\n\nmore\n```\n\nafter",
        MarkdownSpacingPolicy::compact(),
    );

    assert_eq!(lines.iter().filter(|line| line.plain.is_empty()).count(), 1);
    let after = lines.last().unwrap();
    assert_eq!(after.plain, "after");
    assert_ne!(after.spans[0].style.fg, Some(theme::CODE));
}

#[test]
fn table_and_diff_fence_keep_existing_rendering() {
    let table_lines = render(
        "| a | b |\n|---|---|\n| 1 | 2 |",
        MarkdownSpacingPolicy::normal(),
    );
    assert!(table_lines.iter().any(|line| line.plain.contains('│')));

    let diff_lines = render(
        "```diff\n@@ -1 +1 @@\n-old\n+new\n```",
        MarkdownSpacingPolicy::normal(),
    );
    assert!(diff_lines.iter().any(|line| {
        line.spans
            .iter()
            .any(|span| span.style.fg == Some(theme::DIFF_ADD_FG))
    }));
}

#[test]
fn text_fence_hides_markers_and_renders_markdown() {
    let lines = render(
        "```text\n**bold**\n- item\n```",
        MarkdownSpacingPolicy::normal(),
    );
    let plains = lines
        .iter()
        .map(|line| line.plain.as_str())
        .collect::<Vec<_>>();

    assert_eq!(plains, vec!["bold", "• item"]);
}
