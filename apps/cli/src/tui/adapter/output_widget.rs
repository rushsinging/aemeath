//! Retired output widget adapter.
//!
//! Output document projection is owned by `OutputDocumentRenderer::render_model_document(...)` and
//! applied centrally by `App::refresh_output_document_from_model()`. This module intentionally
//! contains no widget writeback helpers.

#[cfg(test)]
mod tests {
    use crate::tui::adapter::output_view_widget::sync_output_scroll_view_state;
    use crate::tui::render::output::document_renderer::OutputDocumentRenderer;
    use crate::tui::render::output_area::OutputArea;
    use crate::tui::view_model::output::{
        BlockNode, OutputBlockKind, OutputViewModel, TextBlockView,
    };
    use crate::tui::view_model::style::SemanticStyle;
    use crate::tui::view_state::output::OutputViewState;

    /// 构造一个 SystemNotice 叶子 BlockNode（生产路径走 render_tree(roots)）。
    fn leaf(id: &str, text: &str) -> BlockNode {
        let kind = OutputBlockKind::SystemNotice(TextBlockView {
            key: id.into(),
            text: text.into(),
            style: SemanticStyle::Muted,
        });
        BlockNode {
            block_id: id.into(),
            block_version: kind.cache_version(),
            kind,
            children: Vec::new(),
        }
    }

    fn vm(lines: usize) -> OutputViewModel {
        let roots: Vec<BlockNode> = (0..lines)
            .map(|i| leaf(&format!("b-{i}"), &format!("line {i}")))
            .collect();
        OutputViewModel {
            roots,
            version: 1,
            follow_tail_hint: true,
        }
    }

    #[test]
    fn test_output_area_render_document_uses_known_term_width_when_layout_width_unready() {
        let mut renderer = OutputDocumentRenderer::default();
        let kind = OutputBlockKind::AssistantMessage(TextBlockView {
            key: "a".into(),
            text: "整理一轮，不改代码。".into(),
            style: SemanticStyle::Normal,
        });
        let view_model = OutputViewModel {
            roots: vec![BlockNode {
                block_id: "a".into(),
                block_version: kind.cache_version(),
                kind,
                children: Vec::new(),
            }],
            version: 1,
            follow_tail_hint: true,
        };

        let document = renderer.render_model_document(&view_model, 1, 80);

        // 每个 root block 前有 1 空行（视觉分隔），故 assistant block = 空行 + 内容 = 2 行。
        assert_eq!(document.total_lines(), 2);
        assert!(document
            .iter_lines()
            .any(|l| l.plain == "整理一轮，不改代码。"));
    }

    #[test]
    fn test_output_area_render_document_clamps_zero_fallback_width_to_one() {
        let mut renderer = OutputDocumentRenderer::default();
        let document = renderer.render_model_document(&vm(1), 1, 0);

        assert_eq!(document.total_lines(), 2);
    }

    #[test]
    fn test_output_area_render_document_uses_layout_width_when_ready() {
        let mut renderer = OutputDocumentRenderer::default();
        let _ = renderer.render_model_document(&vm(1), 40, 80);
        let _ = renderer.render_model_document(&vm(1), 40, 80);

        assert_eq!(
            renderer.render_count(),
            1,
            "相同 view model 与有效 layout width 应命中 renderer cache"
        );
    }

    /// 钳制真相已迁至 `output_view_widget::sync_output_scroll_view_state`（操作 view_state）。
    /// 这两个回归用例走 document projection + 滚动同步组合，验证 stale offset 钳零 / 有效
    /// offset 保留的整链行为不变。
    #[test]
    fn test_render_then_apply_scroll_clamps_stale_offset() {
        let mut output_area = OutputArea::new();
        output_area.last_visible_height = 2;
        let mut renderer = OutputDocumentRenderer::default();
        let mut view = OutputViewState {
            scroll_offset: 100,
            auto_scroll: false,
            ..Default::default()
        };

        let document = renderer.render_model_document(&vm(1), 80, output_area.term_width);
        output_area.replace_document(document);
        // 初始化 last_document_total_lines，避免首帧触发补偿
        view.last_document_total_lines = output_area.document().total_lines();
        sync_output_scroll_view_state(&mut view, &output_area);

        assert_eq!(view.scroll_offset, 0);
        assert!(view.auto_scroll);
    }

    #[test]
    fn test_render_then_apply_scroll_preserves_valid_offset() {
        let mut output_area = OutputArea::new();
        output_area.last_visible_height = 20;
        let mut renderer = OutputDocumentRenderer::default();
        let mut view = OutputViewState {
            scroll_offset: 5,
            auto_scroll: false,
            ..Default::default()
        };

        let document = renderer.render_model_document(&vm(100), 80, output_area.term_width);
        output_area.replace_document(document);
        // 初始化 last_document_total_lines，避免首帧触发补偿
        view.last_document_total_lines = output_area.document().total_lines();
        sync_output_scroll_view_state(&mut view, &output_area);

        assert_eq!(view.scroll_offset, 5);
        assert!(!view.auto_scroll);
    }
}
