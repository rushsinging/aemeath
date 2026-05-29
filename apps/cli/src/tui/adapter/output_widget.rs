use crate::tui::render::output_area::OutputArea;
use crate::tui::view_model::OutputViewModel;

pub(crate) fn render_document_from_view_model(
    output_area: &mut OutputArea,
    view_model: &OutputViewModel,
    width: u16,
) {
    let render_width = effective_render_width(output_area, width);
    let document = output_area
        .document_renderer
        .render_tree(view_model, render_width);
    output_area.set_document(document);
    clamp_scroll_state(output_area);
}

fn effective_render_width(output_area: &OutputArea, width: u16) -> u16 {
    if width > 1 {
        return width;
    }
    u16::try_from(output_area.term_width.max(1)).unwrap_or(u16::MAX)
}

fn clamp_scroll_state(output_area: &mut OutputArea) {
    let max_offset = output_area
        .document()
        .total_lines()
        .saturating_sub(output_area.last_visible_height);
    output_area.scroll_offset = output_area.scroll_offset.min(max_offset);
    if output_area.scroll_offset == 0 {
        output_area.auto_scroll = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::view_model::output::{
        BlockNode, OutputBlockKind, OutputBlockView, OutputViewModel, TextBlockView,
    };
    use crate::tui::view_model::style::SemanticStyle;

    /// 构造 roots 为 blocks 的叶子镜像（与 assembler Task 2.2 产物一致），
    /// 生产路径现走 render_tree(roots)，测试夹具须同步填充 roots。
    fn leaf_roots(blocks: &[OutputBlockView]) -> Vec<BlockNode> {
        blocks
            .iter()
            .map(|b| BlockNode {
                block_id: b.block_id.clone(),
                block_version: b.block_version,
                kind: b.kind.clone(),
                children: Vec::new(),
            })
            .collect()
    }

    fn vm(lines: usize) -> OutputViewModel {
        let blocks: Vec<OutputBlockView> = (0..lines)
            .map(|i| OutputBlockView {
                block_id: format!("b-{i}"),
                block_version: 1,
                kind: OutputBlockKind::SystemNotice(TextBlockView {
                    key: format!("b-{i}"),
                    text: format!("line {i}"),
                    style: SemanticStyle::Muted,
                }),
            })
            .collect();
        let roots = leaf_roots(&blocks);
        OutputViewModel {
            blocks,
            roots,
            version: 1,
            follow_tail_hint: true,
        }
    }

    #[test]
    fn test_render_document_from_view_model_uses_known_term_width_when_layout_width_unready() {
        let mut output_area = OutputArea::new();
        output_area.handle_resize(80, 20);
        let blocks = vec![OutputBlockView {
            block_id: "a".into(),
            block_version: 1,
            kind: OutputBlockKind::AssistantMessage(TextBlockView {
                key: "a".into(),
                text: "整理一轮，不改代码。".into(),
                style: SemanticStyle::Normal,
            }),
        }];
        let roots = leaf_roots(&blocks);
        let view_model = OutputViewModel {
            blocks,
            roots,
            version: 1,
            follow_tail_hint: true,
        };

        render_document_from_view_model(&mut output_area, &view_model, 1);

        assert_eq!(output_area.document().total_lines(), 1);
        assert_eq!(
            output_area.document().iter_lines().next().unwrap().plain,
            "整理一轮，不改代码。"
        );
    }

    #[test]
    fn test_render_document_from_view_model_clamps_stale_scroll_offset() {
        let mut output_area = OutputArea::new();
        output_area.last_visible_height = 2;
        output_area.auto_scroll = false;
        output_area.scroll_offset = 100;

        render_document_from_view_model(&mut output_area, &vm(1), 80);

        assert_eq!(output_area.scroll_offset, 0);
        assert!(output_area.auto_scroll);
    }

    #[test]
    fn test_render_document_from_view_model_preserves_valid_scroll_offset() {
        let mut output_area = OutputArea::new();
        output_area.last_visible_height = 20;
        output_area.auto_scroll = false;
        output_area.scroll_offset = 5;

        render_document_from_view_model(&mut output_area, &vm(100), 80);

        assert_eq!(output_area.scroll_offset, 5);
        assert!(!output_area.auto_scroll);
    }
}
