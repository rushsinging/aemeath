//! 输出文档渲染器：遍历 ViewModel.blocks，经 block 级缓存产出 RenderedDocument。

use crate::tui::render::output::block_cache::{BlockCache, CacheKey};
use crate::tui::render::output::rendered::{RenderedBlock, RenderedDocument, RenderedLine};
use crate::tui::render::output_area::types::MAX_LINES;
use crate::tui::render::theme;
use crate::tui::view_model::output::{BlockNode, OutputViewModel};
use ratatui::style::Style;

#[derive(Default)]
pub struct OutputDocumentRenderer {
    cache: BlockCache,
    #[cfg(test)]
    render_count: std::cell::Cell<usize>,
}

impl OutputDocumentRenderer {
    pub fn render_model_document(
        &mut self,
        view_model: &OutputViewModel,
        width: u16,
        fallback_width: usize,
        animation_frame: u64,
    ) -> RenderedDocument {
        let render_width = if width > 1 {
            width
        } else {
            u16::try_from(fallback_width.max(1)).unwrap_or(u16::MAX)
        };
        self.render_tree_with_animation_frame(view_model, render_width, animation_frame)
    }

    /// 递归走 `view_model.roots`（DFS：父块先于子块），经 block 级缓存展平为线性文档。
    /// gutter（depth 缩进 + marker）在组合期注入。
    pub fn render_tree(&mut self, view_model: &OutputViewModel, width: u16) -> RenderedDocument {
        self.render_tree_with_animation_frame(view_model, width, 0)
    }

    /// 带动画帧的 render_tree；动画只进入缓存外 gutter，不参与 block 内容缓存。
    pub fn render_tree_with_animation_frame(
        &mut self,
        view_model: &OutputViewModel,
        width: u16,
        animation_frame: u64,
    ) -> RenderedDocument {
        // 按 root 分组渲染：每个 root 子树（父块 + 全部后代）落入独立 group，
        // 以便 MAX_LINES 裁剪以整棵子树为单位，NEVER 切断 parent/child 关系。
        let mut groups: Vec<Vec<RenderedBlock>> = Vec::new();
        for root in &view_model.roots {
            let mut group = Vec::new();
            self.render_node(root, width, 0, animation_frame, &mut group);
            groups.push(group);
        }
        let blocks = trim_root_groups_to_max_lines(groups, MAX_LINES);
        let live_ids = collect_rendered_block_ids(&blocks);
        self.cache.retain(&live_ids);
        RenderedDocument { blocks }
    }

    fn render_node(
        &mut self,
        node: &BlockNode,
        width: u16,
        depth: usize,
        animation_frame: u64,
        out: &mut Vec<RenderedBlock>,
    ) {
        let key = CacheKey {
            version: node.block_version,
            width,
        };
        let mut rendered = self.cache.get_or_render(&node.block_id, key, |ctx| {
            #[cfg(test)]
            self.render_count.set(self.render_count.get() + 1);
            node.kind.component().render_self(&node.block_id, ctx)
        });
        if matches!(
            node.kind,
            crate::tui::view_model::output::OutputBlockKind::UserMessage(_)
        ) {
            rendered = rendered.with_line_fill_style(Style::default().bg(theme::USER_BG));
        }
        // gutter（depth 缩进 + marker）在缓存外注入：缓存只存无 gutter 内容，        // gutter 随 depth/status 变化，故组合期叠加（rendered 已 owned，无借用冲突）。
        let mut gutted = crate::tui::render::output::gutter::apply_gutter_with_frame(
            &node.kind,
            depth,
            rendered.lines,
            animation_frame,
        );
        if matches!(
            node.kind,
            crate::tui::view_model::output::OutputBlockKind::UserMessage(_)
        ) {
            wrap_user_message_card_lines(&mut gutted);
        }
        // 每个 root block（depth 0）前加一个空行，分隔相邻对话块（视觉呼吸）；
        // 子块（depth>0，如 tool result）紧贴父块、不额外空行。
        if depth == 0 {
            gutted.insert(0, RenderedLine::default());
        }
        out.push(RenderedBlock {
            block_id: rendered.block_id,
            lines: gutted,
        });
        for child in &node.children {
            self.render_node(child, width, depth + 1, animation_frame, out);
        }
    }

    #[cfg(test)]
    pub fn render_count(&self) -> usize {
        self.render_count.get()
    }
}

fn collect_rendered_block_ids(blocks: &[RenderedBlock]) -> Vec<String> {
    blocks.iter().map(|block| block.block_id.clone()).collect()
}

fn wrap_user_message_card_lines(lines: &mut Vec<RenderedLine>) {
    let gutter_cols = lines.first().map(|line| line.gutter_cols).unwrap_or(0);
    let spacer = user_message_card_spacer_line(gutter_cols);
    lines.insert(0, spacer.clone());
    lines.push(spacer);
}

fn user_message_card_spacer_line(gutter_cols: usize) -> RenderedLine {
    let mut line = RenderedLine::empty().with_fill_style(Style::default().bg(theme::USER_BG));
    line.gutter_cols = gutter_cols;
    line
}

/// 按 root 子树整组裁剪：从尾部（最新）向前累计每个 group 的总行数，
/// 仅当加入该 group 不超过 `max_lines` 时保留整组；NEVER 拆分 group
/// （即父块与其后代要么整体保留、要么整体丢弃）。
/// 边界语义与旧的 per-block 裁剪一致：最新一组即便单独超限也始终保留
/// （首组跳过超限判断），避免输出为空。
fn trim_root_groups_to_max_lines(
    groups: Vec<Vec<RenderedBlock>>,
    max_lines: usize,
) -> Vec<RenderedBlock> {
    if max_lines == 0 {
        return Vec::new();
    }

    let mut kept: Vec<Vec<RenderedBlock>> = Vec::new();
    let mut used = 0usize;
    for group in groups.into_iter().rev() {
        let group_lines: usize = group.iter().map(|b| b.lines.len()).sum();
        if used > 0 && used.saturating_add(group_lines) > max_lines {
            break;
        }
        used = used.saturating_add(group_lines);
        kept.push(group);
    }
    kept.reverse();
    kept.into_iter().flatten().collect()
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
            activity_summary: None,
            result_summary: Some("```\ncode\n```".into()),
            collapsible: false,
            collapsed: false,
        });
        let result_kind = OutputBlockKind::ToolResult(ToolResultBlockView {
            key: "tool-result".into(),
            tool_title: "Bash".into(),
            args_preview: None,
            result_text: "```\ncode\n```".into(),
            style: SemanticStyle::Success,
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
    fn test_renderer_adds_user_message_card_spacers() {
        let kind = OutputBlockKind::UserMessage(TextBlockView {
            key: "u".into(),
            text: "hello".into(),
            style: SemanticStyle::Normal,
        });
        let user = BlockNode {
            block_id: "u".into(),
            block_version: kind.cache_version(),
            kind,
            children: Vec::new(),
        };
        let vm = vm_with_roots(vec![user]);
        let mut renderer = OutputDocumentRenderer::default();
        let doc = renderer.render_tree(&vm, 80);
        let lines = &doc.blocks[0].lines;

        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].plain, "", "root 分隔空行保持无样式");
        assert_eq!(lines[1].plain, "", "用户消息上方应有背景空行");
        assert_eq!(lines[2].plain, "hello");
        assert_eq!(lines[3].plain, "", "用户消息下方应有背景空行");
        assert_eq!(lines[0].fill_style.and_then(|style| style.bg), None);
        assert_eq!(
            lines[1].fill_style.and_then(|style| style.bg),
            Some(theme::USER_BG)
        );
        assert_eq!(
            lines[2].fill_style.and_then(|style| style.bg),
            Some(theme::USER_BG)
        );
        assert_eq!(
            lines[3].fill_style.and_then(|style| style.bg),
            Some(theme::USER_BG)
        );
        assert!(lines[1].spans.is_empty());
        assert_eq!(lines[2].spans[1].style.bg, Some(theme::USER_BG));
        assert!(lines[3].spans.is_empty());
        assert_eq!(lines[2].spans[1].style.fg, Some(theme::USER));
    }

    #[test]
    fn test_user_message_blank_lines_receive_fill_style_without_filler_text() {
        let kind = OutputBlockKind::UserMessage(TextBlockView {
            key: "u".into(),
            text: "a\n\nb".into(),
            style: SemanticStyle::Normal,
        });
        let user = BlockNode {
            block_id: "u".into(),
            block_version: kind.cache_version(),
            kind,
            children: Vec::new(),
        };
        let vm = vm_with_roots(vec![user]);
        let mut renderer = OutputDocumentRenderer::default();
        let doc = renderer.render_tree(&vm, 80);
        let lines = &doc.blocks[0].lines;

        assert_eq!(lines.len(), 6);
        assert_eq!(lines[0].plain, "", "root 分隔空行不属于用户消息卡片");
        assert_eq!(lines[1].plain, "", "用户消息上方 spacer");
        assert_eq!(lines[2].plain, "a");
        assert_eq!(lines[3].plain, "", "用户消息内部空行");
        assert_eq!(lines[4].plain, "b");
        assert_eq!(lines[5].plain, "", "用户消息下方 spacer");
        assert!(lines[1..].iter().all(|line| line
            .fill_style
            .is_some_and(|style| style.bg == Some(theme::USER_BG))));
        assert!(lines.iter().all(|line| !line.plain.ends_with(' ')));
        assert!(lines[1].spans.is_empty());
        assert!(
            lines[3].spans.len() <= 1,
            "内部空行只允许 gutter chrome，不允许文本 filler"
        );
        assert!(lines[5].spans.is_empty());
    }

    fn rb(id: &str, lines: usize) -> RenderedBlock {
        use crate::tui::render::output::rendered::RenderedLine;
        use ratatui::text::Span;
        RenderedBlock {
            block_id: id.into(),
            lines: vec![RenderedLine::new(vec![Span::raw(id.to_string())]); lines],
        }
    }

    #[test]
    fn test_trim_root_groups_drops_oldest_group_when_over_max_lines() {
        // 每个 root 子树自成一组（单块）；总 4 行，max=3，最新组（2 行）保留，最旧组丢弃。
        let groups = vec![vec![rb("old", 2)], vec![rb("new", 2)]];
        let trimmed = trim_root_groups_to_max_lines(groups, 3);

        assert_eq!(trimmed.len(), 1);
        assert_eq!(trimmed[0].block_id, "new");
        assert_eq!(trimmed[0].lines.len(), 2);
    }

    #[test]
    fn test_trim_root_groups_never_splits_subtree() {
        // 两个 root 子树，各 = parent(1) + child(1) = 2 行，共 4 行；max=3 只容得下最新整组。
        let old_group = vec![rb("p-old", 1), rb("c-old", 1)];
        let new_group = vec![rb("p-new", 1), rb("c-new", 1)];
        let trimmed = trim_root_groups_to_max_lines(vec![old_group, new_group], 3);

        let ids: Vec<&str> = trimmed.iter().map(|b| b.block_id.as_str()).collect();
        // 最新子树的父与子都在；最旧子树的父与子都不在——子树从不被拆开。
        assert_eq!(
            ids,
            vec!["p-new", "c-new"],
            "裁剪必须以整棵 root 子树为单位，NEVER 拆分父/子块"
        );
    }

    #[test]
    fn test_trim_root_groups_keeps_newest_even_if_over_max() {
        // 边界：最新组单独超限也始终保留（与旧 per-block 裁剪一致），避免输出为空。
        let groups = vec![vec![rb("old", 5)], vec![rb("new", 10)]];
        let trimmed = trim_root_groups_to_max_lines(groups, 3);

        assert_eq!(trimmed.len(), 1);
        assert_eq!(trimmed[0].block_id, "new");
    }

    #[test]
    fn test_render_tree_retains_only_trimmed_live_blocks() {
        let mut renderer = OutputDocumentRenderer::default();
        let roots = (0..6)
            .map(|idx| node(&format!("root-{idx}"), &"x\n".repeat(2_000), vec![]))
            .collect();
        let vm = vm_with_roots(roots);

        let doc = renderer.render_tree(&vm, 80);

        assert!(
            doc.total_lines() <= MAX_LINES,
            "渲染文档应被裁剪到 MAX_LINES 以内"
        );
        assert!(
            !renderer.cache.contains("root-0"),
            "已被 MAX_LINES 裁剪掉的旧 block 不应继续留在缓存中"
        );
        assert!(
            renderer.cache.contains("root-5"),
            "最新保留 block 应继续留在缓存中"
        );
    }

    #[test]
    fn test_trim_root_groups_zero_max_returns_empty() {
        let trimmed = trim_root_groups_to_max_lines(vec![vec![rb("a", 1)]], 0);
        assert!(trimmed.is_empty());
    }

    #[test]
    fn test_child_version_change_only_rerenders_child() {
        let mut renderer = OutputDocumentRenderer::default();
        let vm = vm_with_roots(vec![node("p", "parent", vec![node("c", "child", vec![])])]);
        let _ = renderer.render_tree(&vm, 80);
        assert_eq!(renderer.render_count(), 2, "首次渲染 parent + child = 2 次");

        // 仅改子块 version，父块 version/width 不变 → 父命中缓存，仅子块重渲。
        let mut child = node("c", "child", vec![]);
        child.block_version += 1;
        let vm2 = vm_with_roots(vec![node("p", "parent", vec![child])]);
        let _ = renderer.render_tree(&vm2, 80);

        assert_eq!(
            renderer.render_count(),
            3,
            "仅子块 version 变 → 父命中缓存，只重渲子块（+1）"
        );
    }

    #[test]
    fn test_retain_keeps_all_tree_block_ids() {
        let mut renderer = OutputDocumentRenderer::default();
        let vm = vm_with_roots(vec![node("p", "parent", vec![node("c", "child", vec![])])]);
        let _ = renderer.render_tree(&vm, 80);
        assert!(renderer.cache.contains("p"), "渲染后父块在缓存中");
        assert!(
            renderer.cache.contains("c"),
            "渲染后子块也在缓存中（全树 retain）"
        );

        // 再渲染只剩父块的树：子块从 ViewModel 消失 → retain 应清除其缓存条目。
        let vm2 = vm_with_roots(vec![node("p", "parent", vec![])]);
        let _ = renderer.render_tree(&vm2, 80);

        assert!(renderer.cache.contains("p"), "父块仍存活");
        assert!(
            !renderer.cache.contains("c"),
            "子块已从树中移除 → retain 清除缓存防泄漏"
        );
    }
}
