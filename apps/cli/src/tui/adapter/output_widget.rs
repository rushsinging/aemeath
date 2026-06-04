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
    // 滚动钳制（offset 反喂 + clamp）统一由
    // `adapter::output_view_widget::sync_output_scroll_view_state` 在渲染前管线处理，
    // 真相归 view_state，此处不再直改滚动态。
}

fn effective_render_width(output_area: &OutputArea, width: u16) -> u16 {
    if width > 1 {
        return width;
    }
    u16::try_from(output_area.term_width.max(1)).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn test_render_document_from_view_model_uses_known_term_width_when_layout_width_unready() {
        let mut output_area = OutputArea::new();
        output_area.handle_resize(80, 20);
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

        render_document_from_view_model(&mut output_area, &view_model, 1);

        // 每个 root block 前有 1 空行（视觉分隔），故 assistant block = 空行 + 内容 = 2 行。
        assert_eq!(output_area.document().total_lines(), 2);
        assert!(output_area
            .document()
            .iter_lines()
            .any(|l| l.plain == "整理一轮，不改代码。"));
    }

    /// 钳制真相已迁至 `output_view_widget::sync_output_scroll_view_state`（操作 view_state）。
    /// 这两个回归用例改为走渲染（render_document_from_view_model）+ 滚动写回（adapter）组合，
    /// 验证 stale offset 钳零 / 有效 offset 保留的整链行为不变。
    #[test]
    fn test_render_then_apply_scroll_clamps_stale_offset() {
        use crate::tui::adapter::output_view_widget::sync_output_scroll_view_state;
        use crate::tui::view_state::output::OutputViewState;

        let mut output_area = OutputArea::new();
        output_area.last_visible_height = 2;
        let mut view = OutputViewState {
            scroll_offset: 100,
            auto_scroll: false,
            ..Default::default()
        };

        render_document_from_view_model(&mut output_area, &vm(1), 80);
        // 初始化 last_document_total_lines，避免首帧触发补偿
        view.last_document_total_lines = output_area.document().total_lines();
        sync_output_scroll_view_state(&mut view, &output_area);

        assert_eq!(view.scroll_offset, 0);
        assert!(view.auto_scroll);
    }

    #[test]
    fn test_render_then_apply_scroll_preserves_valid_offset() {
        use crate::tui::adapter::output_view_widget::sync_output_scroll_view_state;
        use crate::tui::view_state::output::OutputViewState;

        let mut output_area = OutputArea::new();
        output_area.last_visible_height = 20;
        let mut view = OutputViewState {
            scroll_offset: 5,
            auto_scroll: false,
            ..Default::default()
        };

        render_document_from_view_model(&mut output_area, &vm(100), 80);
        // 初始化 last_document_total_lines，避免首帧触发补偿
        view.last_document_total_lines = output_area.document().total_lines();
        sync_output_scroll_view_state(&mut view, &output_area);

        assert_eq!(view.scroll_offset, 5);
        assert!(!view.auto_scroll);
    }
}
