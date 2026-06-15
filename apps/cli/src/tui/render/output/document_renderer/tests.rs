use super::*;
use crate::tui::view_model::output::{BlockNode, OutputBlockKind, OutputViewModel, TextBlockView};
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
