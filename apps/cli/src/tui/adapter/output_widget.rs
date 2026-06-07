//! Retired output widget adapter.
//!
//! Output document projection is owned by `OutputDocumentRenderer::render_model_document(...)` and
//! applied centrally by `App::refresh_output_document_from_model()`. This module intentionally
//! contains no widget writeback helpers.

#[cfg(test)]
mod tests {
    use crate::tui::render::output::document_renderer::OutputDocumentRenderer;
    use crate::tui::view_model::output::{
        BlockNode, OutputBlockKind, OutputViewModel, TextBlockView,
    };
    use crate::tui::view_model::style::SemanticStyle;

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

        let document = renderer.render_model_document(&view_model, 1, 80, 0);
        // 每个 root block 前有 1 空行（视觉分隔），故 assistant block = 空行 + 内容 = 2 行。
        assert_eq!(document.total_lines(), 2);
        assert!(document
            .iter_lines()
            .any(|l| l.plain == "整理一轮，不改代码。"));
    }

    #[test]
    fn test_output_area_render_document_clamps_zero_fallback_width_to_one() {
        let mut renderer = OutputDocumentRenderer::default();
        let document = renderer.render_model_document(&vm(1), 1, 0, 0);
        assert_eq!(document.total_lines(), 2);
    }

    #[test]
    fn test_output_area_render_document_uses_layout_width_when_ready() {
        let mut renderer = OutputDocumentRenderer::default();
        let _ = renderer.render_model_document(&vm(1), 40, 80, 0);
        let _ = renderer.render_model_document(&vm(1), 40, 80, 1);
        assert_eq!(
            renderer.render_count(),
            1,
            "相同 view model 与有效 layout width 应命中 renderer cache"
        );
    }
}
