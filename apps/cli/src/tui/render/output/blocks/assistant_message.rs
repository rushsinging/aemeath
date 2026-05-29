use crate::tui::render::output::primitives::fenced::render_fenced_markdown;
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::TextBlockView;
use ratatui::style::Style;

pub fn render_assistant_message(
    block_id: &str,
    view: &TextBlockView,
    ctx: &RenderCtx,
) -> RenderedBlock {
    let base = Style::default().fg(theme::ASSISTANT);
    // fence/markdown/table 解析统一走 primitives::fenced（DRY，与工具结果共用）。
    let mut lines = render_fenced_markdown(&view.text, base, ctx.width);

    if lines.is_empty() {
        lines.push(RenderedLine::default());
    }
    RenderedBlock {
        block_id: block_id.to_string(),
        lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::view_model::style::SemanticStyle;
    use ratatui::style::Modifier;

    fn render(text: &str) -> RenderedBlock {
        let view = TextBlockView {
            key: "a".into(),
            text: text.into(),
            style: SemanticStyle::Normal,
        };
        render_assistant_message("a", &view, &RenderCtx { width: 80 })
    }

    #[test]
    fn test_assistant_renders_markdown_bold() {
        let block = render("see **this**");

        assert!(block.lines.iter().any(|line| line
            .spans
            .iter()
            .any(|span| span.content.as_ref() == "this"
                && span.style.add_modifier.contains(Modifier::BOLD))));
        assert!(block
            .lines
            .iter()
            .any(|line| line.plain.contains("see this")));
    }

    #[test]
    fn test_assistant_cjk_text_does_not_wrap_per_character_at_normal_width() {
        let block = render("整理一轮，不改代码。");

        assert_eq!(block.lines.len(), 1);
        assert_eq!(block.lines[0].plain, "整理一轮，不改代码。");
    }

    #[test]
    fn test_assistant_base_color_is_assistant_theme() {
        let block = render("plain text");

        assert_eq!(block.lines[0].spans[0].style.fg, Some(theme::ASSISTANT));
    }

    #[test]
    fn test_assistant_fence_does_not_leak_style_after_close() {
        let block = render("```\ncode\n```\nafter");
        let after = block.lines.last().unwrap();

        assert_eq!(after.plain, "after");
        assert_ne!(after.spans[0].style.fg, Some(theme::CODE));
    }

    #[test]
    fn test_assistant_diff_fence_renders_indent_signs_and_semantic_color() {
        // #61：LLM markdown 中 ```diff 代码块应走 unified diff 渲染——
        // INDENT 缩进（不贴最左）+ 加减语义色，而非通用代码块单色着色。
        let block = render("```diff\n@@ -1 +1 @@\n-let a = 1;\n+let a = 2;\n```");

        // 删除行带 DIFF_REMOVE_FG。
        let removed = block.lines.iter().find(|l| l.plain.contains("1;")).unwrap();
        assert!(
            removed.plain.starts_with("  "),
            "diff 行应保留 INDENT 缩进, got: {:?}",
            removed.plain
        );
        assert!(
            removed
                .spans
                .iter()
                .any(|s| s.style.fg == Some(theme::DIFF_REMOVE_FG)),
            "删除行应带删除语义色"
        );
        // 新增行带 DIFF_ADD_FG。
        let added = block.lines.iter().find(|l| l.plain.contains("2;")).unwrap();
        assert!(
            added
                .spans
                .iter()
                .any(|s| s.style.fg == Some(theme::DIFF_ADD_FG)),
            "新增行应带新增语义色"
        );
    }

    #[test]
    fn test_assistant_diff_fence_added_line_syntax_highlight() {
        // 注：assistant_message 对 ```diff 不传 ext（无文件信息），故新增行仅语义色，
        // 不做语法高亮——本测试锁定该行为（语法高亮归 Edit 工具路径）。
        let block = render("```diff\n+fn main() {}\n```");
        let added = block
            .lines
            .iter()
            .find(|l| l.plain.contains("fn main"))
            .unwrap();

        assert!(added.plain.starts_with("  +"));
        assert!(added
            .spans
            .iter()
            .any(|s| s.style.fg == Some(theme::DIFF_ADD_FG)));
    }

    #[test]
    fn test_assistant_diff_fence_line_keeps_fg_under_selection_overlay() {
        // #61：diff 行经 apply_selection_overlay 选中后应保留前景色（高亮不丢）。
        use crate::tui::render::output::selection_overlay::{apply_selection_overlay, SelRange};

        let block = render("```diff\n+let a = 2;\n```");
        let added = block.lines.iter().find(|l| l.plain.contains("2;")).unwrap();
        let add_fg = added
            .spans
            .iter()
            .find(|s| s.style.fg == Some(theme::DIFF_ADD_FG))
            .map(|s| s.style.fg)
            .expect("新增行存在带新增色的 span");

        let overlaid = apply_selection_overlay(
            added,
            Some(SelRange {
                start: 0,
                end: added.plain.chars().count(),
            }),
        );

        // 选中区段保留原 fg（只叠加 bg），且原新增色仍在。
        assert!(
            overlaid
                .iter()
                .all(|s| s.style.bg == Some(theme::SELECTION_BG)),
            "全选后每段应带选区背景"
        );
        assert!(
            overlaid.iter().any(|s| s.style.fg == add_fg),
            "选中后应保留新增前景色（修 #61）"
        );
    }
}
