use super::render_tool_group;
use crate::tui::render::output::rendered::RenderCtx;
use crate::tui::view_model::output::{ToolGroupBlockView, ToolGroupKind};
use crate::tui::view_model::style::SemanticStyle;

#[test]
fn renders_group_title_without_member_count_or_status_summary() {
    let view = ToolGroupBlockView {
        key: "group-1".into(),
        kind: ToolGroupKind::Explore,
        title: ToolGroupKind::Explore.title().into(),
        style: SemanticStyle::Muted,
    };

    let rendered = render_tool_group("group-1", &view, &RenderCtx::for_width(80));

    assert_eq!(rendered.lines.len(), 1);
    assert_eq!(rendered.lines[0].plain, "── Explore ──");
    assert!(!rendered.lines[0].plain.contains('2'));
    assert!(!rendered.lines[0].plain.contains("Running"));
}
