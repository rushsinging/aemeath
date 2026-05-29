//! 输出文档渲染器：遍历 ViewModel.blocks，经 block 级缓存产出 RenderedDocument。

use crate::tui::render::output::block_cache::{BlockCache, CacheKey};
use crate::tui::render::output::rendered::{RenderedBlock, RenderedDocument};
use crate::tui::render::output_area::types::MAX_LINES;
use crate::tui::view_model::output::{BlockNode, OutputViewModel};

#[derive(Default)]
pub struct OutputDocumentRenderer {
    cache: BlockCache,
    #[cfg(test)]
    render_count: std::cell::Cell<usize>,
}

impl OutputDocumentRenderer {
    /// 递归走 `view_model.roots`（DFS：父块先于子块），经 block 级缓存展平为线性文档。
    /// gutter（depth 缩进 + marker）在组合期注入。
    pub fn render_tree(&mut self, view_model: &OutputViewModel, width: u16) -> RenderedDocument {
        let mut blocks = Vec::new();
        let mut live_ids = Vec::new();
        for root in &view_model.roots {
            self.render_node(root, width, 0, &mut blocks, &mut live_ids);
        }
        self.cache.retain(&live_ids);
        RenderedDocument {
            blocks: trim_blocks_to_max_lines(blocks, MAX_LINES),
        }
    }

    fn render_node(
        &mut self,
        node: &BlockNode,
        width: u16,
        depth: usize,
        out: &mut Vec<RenderedBlock>,
        live_ids: &mut Vec<String>,
    ) {
        let key = CacheKey {
            version: node.block_version,
            width,
        };
        let rendered = self.cache.get_or_render(&node.block_id, key, |ctx| {
            #[cfg(test)]
            self.render_count.set(self.render_count.get() + 1);
            node.kind.component().render_self(&node.block_id, ctx)
        });
        live_ids.push(node.block_id.clone());
        // gutter（depth 缩进 + marker）在缓存外注入：缓存只存无 gutter 内容，
        // gutter 随 depth/status 变化，故组合期叠加（rendered 已 owned，无借用冲突）。
        let gutted =
            crate::tui::render::output::gutter::apply_gutter(&node.kind, depth, rendered.lines);
        out.push(RenderedBlock {
            block_id: rendered.block_id,
            lines: gutted,
        });
        for child in &node.children {
            self.render_node(child, width, depth + 1, out, live_ids);
        }
    }

    #[cfg(test)]
    pub fn render_count(&self) -> usize {
        self.render_count.get()
    }
}

fn trim_blocks_to_max_lines(blocks: Vec<RenderedBlock>, max_lines: usize) -> Vec<RenderedBlock> {
    if max_lines == 0 {
        return Vec::new();
    }

    let mut kept = Vec::new();
    let mut used = 0usize;
    for block in blocks.into_iter().rev() {
        let line_count = block.lines.len();
        if used > 0 && used.saturating_add(line_count) > max_lines {
            break;
        }
        used = used.saturating_add(line_count);
        kept.push(block);
    }
    kept.reverse();
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::view_model::output::{
        BlockNode, OutputBlockKind, OutputViewModel, TextBlockView,
    };
    use crate::tui::view_model::style::SemanticStyle;

    fn node(id: &str, text: &str, children: Vec<BlockNode>) -> BlockNode {
        let kind = OutputBlockKind::SystemNotice(TextBlockView {
            key: id.into(),
            text: text.into(),
            style: SemanticStyle::Muted,
        });
        BlockNode {
            block_id: id.into(),
            block_version: kind.cache_version(),
            kind,
            children,
        }
    }

    fn vm_with_roots(roots: Vec<BlockNode>) -> OutputViewModel {
        OutputViewModel {
            roots,
            version: 1,
            follow_tail_hint: true,
        }
    }

    #[test]
    fn test_renderer_emits_one_block_per_root() {
        let mut renderer = OutputDocumentRenderer::default();
        let vm = vm_with_roots(vec![node("s", "ok", vec![])]);
        let doc = renderer.render_tree(&vm, 80);

        assert_eq!(doc.blocks.len(), 1);
        assert_eq!(doc.blocks[0].block_id, "s");
    }

    #[test]
    fn test_renderer_caches_unchanged_block() {
        let mut renderer = OutputDocumentRenderer::default();
        let vm = vm_with_roots(vec![node("s", "ok", vec![])]);
        let _ = renderer.render_tree(&vm, 80);
        let _ = renderer.render_tree(&vm, 80);

        assert_eq!(
            renderer.render_count(),
            1,
            "同 version+width 第二次应命中缓存"
        );
    }

    #[test]
    fn test_render_tree_dfs_flattens_parent_then_children() {
        let vm = vm_with_roots(vec![node("p", "parent", vec![node("c", "child", vec![])])]);
        let mut renderer = OutputDocumentRenderer::default();
        let doc = renderer.render_tree(&vm, 80);

        assert_eq!(doc.blocks.len(), 2);
        assert_eq!(doc.blocks[0].block_id, "p");
        assert_eq!(doc.blocks[1].block_id, "c");
    }

    #[test]
    fn test_render_tree_tool_result_fence_does_not_leak_to_sibling_root() {
        // #65 结构回归：ToolResult 子块含完整 ```fenced``` 代码块，其后兄弟
        // AssistantMessage root 的首行不应残留 CODE 色——每个 block 经独立组件渲染，
        // fence 状态机随 block 销毁，结构上隔离泄漏（不依赖行内顺序补偿）。
        use crate::tui::render::theme;
        use crate::tui::view_model::output::{
            ToolCallBlockView, ToolResultBlockView, ToolSemanticStatus,
        };

        let tool_kind = OutputBlockKind::ToolCall(ToolCallBlockView {
            key: "tool".into(),
            chat_id: None,
            turn_id: None,
            tool_call_id: Some("tool".into()),
            title: "Bash".into(),
            icon: "✓".into(),
            semantic_status: ToolSemanticStatus::Success,
            style: SemanticStyle::Success,
            args_preview: None,
            summary: None,
            activity_summary: None,
            result_summary: Some("```\ncode\n```".into()),
            collapsible: false,
            collapsed: false,
        });
        let result_kind = OutputBlockKind::ToolResult(ToolResultBlockView {
            key: "tool-result".into(),
            tool_title: "Bash".into(),
            summary: None,
            result_text: "```\ncode\n```".into(),
            is_error: false,
        });
        let tool_node = BlockNode {
            block_id: "tool".into(),
            block_version: tool_kind.cache_version(),
            kind: tool_kind,
            children: vec![BlockNode {
                block_id: "tool-result".into(),
                block_version: result_kind.cache_version(),
                kind: result_kind,
                children: Vec::new(),
            }],
        };
        let assistant_kind = OutputBlockKind::AssistantMessage(TextBlockView {
            key: "a".into(),
            text: "plain assistant line".into(),
            style: SemanticStyle::Normal,
        });
        let assistant_node = BlockNode {
            block_id: "a".into(),
            block_version: assistant_kind.cache_version(),
            kind: assistant_kind,
            children: Vec::new(),
        };

        let vm = vm_with_roots(vec![tool_node, assistant_node]);
        let mut renderer = OutputDocumentRenderer::default();
        let doc = renderer.render_tree(&vm, 80);

        let assistant_block = doc
            .blocks
            .iter()
            .find(|b| b.block_id == "a")
            .expect("assistant block 存在");
        assert!(
            assistant_block.lines[0]
                .spans
                .iter()
                .all(|s| s.style.fg != Some(theme::CODE)),
            "兄弟 AssistantMessage 首行不应残留工具结果 fence 的 CODE 色（#65）"
        );
    }

    #[test]
    fn test_document_drops_oldest_block_when_over_max_lines() {
        use crate::tui::render::output::rendered::{RenderedBlock, RenderedLine};
        use ratatui::text::Span;

        let blocks = vec![
            RenderedBlock {
                block_id: "old".into(),
                lines: vec![RenderedLine::new(vec![Span::raw("old")]); 2],
            },
            RenderedBlock {
                block_id: "new".into(),
                lines: vec![RenderedLine::new(vec![Span::raw("new")]); 2],
            },
        ];
        let trimmed = trim_blocks_to_max_lines(blocks, 3);

        assert_eq!(trimmed.len(), 1);
        assert_eq!(trimmed[0].block_id, "new");
        assert_eq!(trimmed[0].lines.len(), 2);
    }
}
