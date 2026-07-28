use super::*;
use crate::tui::render::display::safe_text::str_display_width;
use crate::tui::render::output::rendered::RenderedLine;
use crate::tui::render::output::spacing::MarkdownSpacingPolicy;
use crate::tui::view_model::output::{
    BlockNode, ModelStreamPlaceholderBlockView, OutputBlockKind, OutputViewModel, TextBlockView,
    ToolCallBlockView, ToolResultBlockView, ToolSemanticStatus,
};
use crate::tui::view_model::style::SemanticStyle;

fn visible_line_width(line: &RenderedLine) -> usize {
    line.spans
        .iter()
        .map(|span| str_display_width(span.content.as_ref()))
        .sum()
}

fn assistant_node(id: &str, text: &str) -> BlockNode {
    let kind = OutputBlockKind::AssistantMessage(TextBlockView {
        key: id.into(),
        text: text.into(),
        style: SemanticStyle::Normal,
    });
    BlockNode {
        block_id: id.into(),
        block_version: kind.cache_version(),
        kind,
        children: Vec::new(),
    }
}

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

fn placeholder_node() -> BlockNode {
    let kind = OutputBlockKind::ModelStreamPlaceholder(ModelStreamPlaceholderBlockView {
        key: "model-stream-placeholder".into(),
        elapsed_secs: 10,
        phase: "waiting_first_model_delta".into(),
    });
    BlockNode {
        block_id: "model-stream-placeholder".into(),
        block_version: kind.cache_version(),
        kind,
        children: Vec::new(),
    }
}

#[test]
fn model_stream_placeholder_document_is_static_across_animation_frames() {
    let vm = vm_with_roots(vec![placeholder_node()]);
    let mut renderer = OutputDocumentRenderer::default();

    let doc0 =
        renderer.render_tree_with_animation_frame(&vm, 80, 0, MarkdownSpacingPolicy::normal());
    let render_count = renderer.render_count();
    let gutted_render_count = renderer.gutted_render_count();
    let doc1 =
        renderer.render_tree_with_animation_frame(&vm, 80, 4, MarkdownSpacingPolicy::normal());

    assert_eq!(doc0, doc1, "动画帧不得固化进历史文档");
    assert_eq!(renderer.render_count(), render_count);
    assert_eq!(renderer.gutted_render_count(), gutted_render_count);
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
        activity_lines: Vec::new(),
        result_summary: Some("```\ncode\n```".into()),
        result_payload: None,
        workspace_root: None,
        collapsible: false,
        collapsed: false,
        agent_meta: None,
    });
    let result_kind = OutputBlockKind::ToolResult(ToolResultBlockView {
        key: "tool-result".into(),
        tool_title: "Bash".into(),
        args_preview: None,
        result_text: "```\ncode\n```".into(),
        data: None,
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

#[test]
fn render_window_drops_oldest_group_when_over_line_limit() {
    let selected = select_root_window(
        &[2, 2],
        OutputRenderWindow {
            line_limit: 3,
            tail_offset: 0,
        },
    );

    assert_eq!(selected.root_range, 1..2);
    assert_eq!(selected.source_total_lines, 4);
    assert_eq!(selected.folded_earlier_lines, 2);
}

#[test]
fn render_window_never_splits_subtree() {
    let selected = select_root_window(
        &[2, 5, 2],
        OutputRenderWindow {
            line_limit: 3,
            tail_offset: 2,
        },
    );

    assert_eq!(selected.root_range, 1..2);
    assert_eq!(selected.folded_earlier_lines, 2);
}

#[test]
fn render_window_keeps_single_root_even_if_over_line_limit() {
    let selected = select_root_window(
        &[5, 10],
        OutputRenderWindow {
            line_limit: 3,
            tail_offset: 0,
        },
    );

    assert_eq!(selected.root_range, 1..2);
}

#[test]
fn render_window_tail_offset_selects_older_roots() {
    let selected = select_root_window(
        &[2, 2, 2, 2],
        OutputRenderWindow {
            line_limit: 4,
            tail_offset: 2,
        },
    );

    assert_eq!(selected.root_range, 1..3);
    assert_eq!(selected.folded_earlier_lines, 2);
}

#[test]
fn render_window_at_oldest_history_has_no_folded_earlier_lines() {
    let selected = select_root_window(
        &[2, 2, 2, 2],
        OutputRenderWindow {
            line_limit: 4,
            tail_offset: 4,
        },
    );

    assert_eq!(selected.root_range, 0..2);
    assert_eq!(selected.folded_earlier_lines, 0);
}

#[test]
fn render_window_only_materializes_requested_blocks_but_keeps_recent_cache_entries() {
    let mut renderer = OutputDocumentRenderer::default();
    let roots = (0..6)
        .map(|idx| node(&format!("root-{idx}"), &"x\n".repeat(2_000), vec![]))
        .collect();
    let vm = vm_with_roots(roots);

    let rendered = renderer.render_tree_with_window(
        &vm,
        80,
        0,
        MarkdownSpacingPolicy::normal(),
        OutputRenderWindow {
            line_limit: 3_000,
            tail_offset: 0,
        },
    );

    assert!(
        rendered.document.total_lines() <= 3_001,
        "渲染文档只包含请求窗口和至多一行折叠提示"
    );
    assert_eq!(rendered.source_total_lines, 12_012);
    assert!(rendered.folded_earlier_lines > 0);
    assert!(
        renderer.cache.contains("root-0"),
        "测量过的窗口外 block 在容量内应保留，供历史窗口往返复用"
    );
    assert!(
        renderer.cache.contains("root-5"),
        "请求窗口内最新 block 应继续留在 rendered cache"
    );
}

#[test]
fn render_window_keeps_recent_blocks_cached_across_window_round_trip() {
    let roots = (0..4)
        .map(|idx| node(&format!("root-{idx}"), &format!("line-{idx}"), vec![]))
        .collect();
    let vm = vm_with_roots(roots);
    let mut renderer = OutputDocumentRenderer::with_render_cache_capacity(4);
    let newest = OutputRenderWindow {
        line_limit: 2,
        tail_offset: 0,
    };
    let older = OutputRenderWindow {
        line_limit: 2,
        tail_offset: 2,
    };

    renderer.render_tree_with_window(&vm, 80, 0, MarkdownSpacingPolicy::normal(), newest);
    renderer.render_tree_with_window(&vm, 80, 0, MarkdownSpacingPolicy::normal(), older);
    let before_return = renderer.render_count();
    let gutted_before_return = renderer.gutted_render_count();
    renderer.render_tree_with_window(&vm, 80, 0, MarkdownSpacingPolicy::normal(), newest);

    assert_eq!(
        renderer.render_count(),
        before_return,
        "容量内窗口往返不应重新执行 block 内容渲染"
    );
    assert_eq!(
        renderer.gutted_render_count(),
        gutted_before_return,
        "容量内窗口往返不应重新组合 gutter"
    );
}

#[test]
fn rendered_caches_never_exceed_configured_capacity_across_windows() {
    let roots = (0..8)
        .map(|idx| node(&format!("root-{idx}"), &format!("line-{idx}"), vec![]))
        .collect();
    let vm = vm_with_roots(roots);
    let mut renderer = OutputDocumentRenderer::with_render_cache_capacity(3);

    for tail_offset in [0, 2, 4, 6, 0] {
        renderer.render_tree_with_window(
            &vm,
            80,
            0,
            MarkdownSpacingPolicy::normal(),
            OutputRenderWindow {
                line_limit: 2,
                tail_offset,
            },
        );
        let retained = renderer.retained_cache_capacity();
        assert!(retained.block_entries <= 3);
        assert!(retained.gutted_entries <= 3);
    }
}

#[test]
fn render_window_zero_limit_returns_empty_window() {
    let selected = select_root_window(
        &[1],
        OutputRenderWindow {
            line_limit: 0,
            tail_offset: 0,
        },
    );

    assert_eq!(selected.root_range, 1..1);
    assert_eq!(selected.source_total_lines, 1);
    assert_eq!(selected.folded_earlier_lines, 1);
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

// ─── #329 回归测试：行尾字符碰到右边界时被 LineTruncator 截断丢失 ───
//
// 根因：document 预 wrap 宽度（=`App::output_document_width()` = content_area.width）
// 未扣除组合期注入的 gutter 宽度（`gutter::gutter_width(depth)`）。
// 真实渲染 `Paragraph::new(display_lines)` 未调用 `.wrap()`，默认走 LineTruncator，
// 超宽 line 的尾部字符会被吞掉。
//
// 修复后契约：`effective_block_width(outer_width, depth) = outer_width - gutter_width(depth)`，
// block 内部 wrap 宽度即 `RenderCtx.text_width`，因此 gutter + content 可见总宽 ≤ outer_width。

#[test]
fn test_render_tree_depth_zero_full_width_assistant_does_not_exceed_outer_width() {
    // 复现 #329：outer_width=77 (= content_area.width 80 - 3 scrollbar reserve)，
    // assistant block 文本刚好填满 outer_width。修复前 line 总宽 = 77 (wrap) + 2 (gutter) = 79 > 77。
    let outer_width: u16 = 77;
    let text = "x".repeat(outer_width as usize);
    let mut renderer = OutputDocumentRenderer::default();
    let vm = vm_with_roots(vec![assistant_node("a", &text)]);
    let doc = renderer.render_tree(&vm, outer_width);

    // render_node 在 depth=0 时前置 root 分隔空行（index 0），index 1 是 content line。
    let content_line = &doc.blocks[0].lines[1];
    let visible = visible_line_width(content_line);

    assert!(
        visible <= outer_width as usize,
        "depth=0：gutter + content 可见总宽 {} 应 ≤ outer_width {}（#329）",
        visible,
        outer_width
    );
}

#[test]
fn test_render_tree_depth_one_full_width_assistant_does_not_exceed_outer_width() {
    // depth=1（如 ToolResult 子块）gutter=4 列：未修复时 line 总宽 = 77 + 4 = 81 > 77。
    // 用 AssistantMessage 作为 ToolCall 子节点（OutputBlockKind 任意 enum 都能挂为子节点）。
    let outer_width: u16 = 77;
    let text = "x".repeat(outer_width as usize);
    use crate::tui::view_model::output::{ToolCallBlockView, ToolSemanticStatus};
    let tool_kind = OutputBlockKind::ToolCall(ToolCallBlockView {
        key: "tool".into(),
        chat_id: None,
        turn_id: None,
        tool_call_id: Some("tool".into()),
        title: "Bash".into(),
        icon: "✓".into(),
        semantic_status: ToolSemanticStatus::Success,
        style: SemanticStyle::Normal,
        args_preview: None,
        activity_lines: Vec::new(),
        result_summary: None,
        result_payload: None,
        workspace_root: None,
        collapsible: false,
        collapsed: false,
        agent_meta: None,
    });
    let tool_node = BlockNode {
        block_id: "tool".into(),
        block_version: tool_kind.cache_version(),
        kind: tool_kind,
        children: vec![assistant_node("child", &text)],
    };
    let mut renderer = OutputDocumentRenderer::default();
    let vm = vm_with_roots(vec![tool_node]);
    let doc = renderer.render_tree(&vm, outer_width);

    // tool_node 是 root（depth=0，block index 0），其子 child 是 depth=1（block index 1）。
    let child_block = doc
        .blocks
        .iter()
        .find(|b| b.block_id == "child")
        .expect("child block 存在");
    // depth=1 块没有 root 分隔空行（render_node 只在 depth==0 时插入）；
    // 跳过 tool 自身产生的任何空行，取第一个非空 content line。
    let content_line = child_block
        .lines
        .iter()
        .find(|line| !line.plain.is_empty())
        .expect("child block 至少有一行 content");
    let visible = visible_line_width(content_line);

    assert!(
        visible <= outer_width as usize,
        "depth=1：gutter(4) + content 可见总宽 {} 应 ≤ outer_width {}（#329）",
        visible,
        outer_width
    );
}

#[test]
fn spacing_policy_change_invalidates_content_and_gutted_caches() {
    let kind = OutputBlockKind::AssistantMessage(TextBlockView {
        key: "assistant".into(),
        text: "one\n\ntwo".into(),
        style: SemanticStyle::Normal,
    });
    let vm = OutputViewModel {
        version: 1,
        follow_tail_hint: false,
        roots: vec![BlockNode {
            block_id: "assistant".into(),
            block_version: kind.cache_version(),
            kind,
            children: vec![],
        }],
    };
    let mut renderer = OutputDocumentRenderer::default();

    let normal = renderer.render_model_document(&vm, 80, 80, 0, MarkdownSpacingPolicy::normal());
    let after_normal_content = renderer.render_count();
    let after_normal_gutted = renderer.gutted_render_count();
    let compact = renderer.render_model_document(&vm, 80, 80, 0, MarkdownSpacingPolicy::compact());

    assert!(compact.total_lines() < normal.total_lines());
    assert_eq!(renderer.render_count(), after_normal_content + 1);
    assert_eq!(renderer.gutted_render_count(), after_normal_gutted + 1);

    let after_compact_content = renderer.render_count();
    let after_compact_gutted = renderer.gutted_render_count();
    let _ = renderer.render_model_document(&vm, 80, 80, 0, MarkdownSpacingPolicy::compact());
    assert_eq!(renderer.render_count(), after_compact_content);
    assert_eq!(renderer.gutted_render_count(), after_compact_gutted);
}

#[test]
fn test_gutted_cache_reuses_static_block_across_frames() {
    let kind = OutputBlockKind::AssistantMessage(TextBlockView {
        key: "a".to_string(),
        text: "静态文本".to_string(),
        style: SemanticStyle::Normal,
    });
    let node = BlockNode {
        block_id: "a".to_string(),
        block_version: kind.cache_version(),
        kind,
        children: Vec::new(),
    };
    let vm = OutputViewModel {
        roots: vec![node],
        version: 1,
        follow_tail_hint: true,
    };
    let mut r = OutputDocumentRenderer::default();
    let _ = r.render_model_document(
        &vm,
        80,
        80,
        0,
        crate::tui::render::output::spacing::MarkdownSpacingPolicy::normal(),
    );
    let after_first = r.gutted_render_count();
    // 同一 vm、frame 推进：静态 block 应命中 gutted 缓存，不重算。
    let _ = r.render_model_document(
        &vm,
        80,
        80,
        1,
        crate::tui::render::output::spacing::MarkdownSpacingPolicy::normal(),
    );
    assert_eq!(
        r.gutted_render_count(),
        after_first,
        "静态 block 跨 frame 应复用 gutted 缓存"
    );
}

fn static_edit_root(id: &str, lines: usize) -> BlockNode {
    let old = (0..lines)
        .map(|index| format!("fn item_{index}() {{ println!(\"old {index}\"); }}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut new_lines = old.lines().map(str::to_string).collect::<Vec<_>>();
    new_lines[lines / 2] = format!("fn item_{}() {{ println!(\"new\"); }}", lines / 2);
    let result_text = format!("replaced 1 occurrence(s) in src/{id}.rs");
    let result_kind = OutputBlockKind::ToolResult(ToolResultBlockView {
        key: format!("{id}-result"),
        tool_title: "Edit".into(),
        args_preview: Some(format!(r#"{{"file_path":"src/{id}.rs"}}"#)),
        result_text: result_text.clone(),
        data: Some(serde_json::json!({
            "old": old,
            "new": new_lines.join("\n"),
            "start_line": 1
        })),
        style: SemanticStyle::Success,
    });
    let tool_kind = OutputBlockKind::ToolCall(ToolCallBlockView {
        key: id.into(),
        chat_id: None,
        turn_id: None,
        tool_call_id: Some(id.into()),
        title: "Edit".into(),
        icon: "✓".into(),
        semantic_status: ToolSemanticStatus::Success,
        style: SemanticStyle::Success,
        args_preview: Some(format!(r#"{{"file_path":"src/{id}.rs"}}"#)),
        activity_lines: Vec::new(),
        result_summary: Some(result_text),
        result_payload: None,
        workspace_root: None,
        collapsible: false,
        collapsed: false,
        agent_meta: None,
    });
    BlockNode {
        block_id: id.into(),
        block_version: tool_kind.cache_version(),
        kind: tool_kind,
        children: vec![BlockNode {
            block_id: format!("{id}-result"),
            block_version: result_kind.cache_version(),
            kind: result_kind,
            children: Vec::new(),
        }],
    }
}

#[test]
fn history_over_max_lines_does_not_rehighlight_evicted_static_edits_on_next_frame() {
    let vm = vm_with_roots(
        (0..6)
            .map(|index| static_edit_root(&format!("edit-{index}"), 2_000))
            .collect(),
    );
    let mut renderer = OutputDocumentRenderer::default();

    let (_, cold) = crate::tui::render::performance::capture(|| {
        renderer.render_model_document(&vm, 100, 100, 0, MarkdownSpacingPolicy::normal())
    });
    let (_, warm) = crate::tui::render::performance::capture(|| {
        renderer.render_model_document(&vm, 100, 100, 1, MarkdownSpacingPolicy::normal())
    });

    assert_eq!(cold.edit_diff_calls, 6);
    assert_eq!(warm.edit_diff_calls, 0);
    assert_eq!(warm.diff_build_calls, 0);
    assert_eq!(warm.syntax_highlight_calls, 0);
}

#[test]
fn unrelated_new_root_does_not_rehighlight_windowed_static_edits() {
    let mut vm = vm_with_roots(
        (0..6)
            .map(|index| static_edit_root(&format!("edit-{index}"), 2_000))
            .collect(),
    );
    let mut renderer = OutputDocumentRenderer::default();
    let _ = renderer.render_model_document(&vm, 100, 100, 0, MarkdownSpacingPolicy::normal());

    vm.version += 1;
    vm.roots.push(node("unrelated", "无关的新消息", vec![]));
    let (_, revised) = crate::tui::render::performance::capture(|| {
        renderer.render_model_document(&vm, 100, 100, 1, MarkdownSpacingPolicy::normal())
    });

    assert_eq!(revised.edit_diff_calls, 0);
    assert_eq!(revised.diff_build_calls, 0);
    assert_eq!(revised.syntax_highlight_calls, 0);
}

#[test]
fn static_edit_diff_reuses_render_and_highlight_across_spinner_frames() {
    let old = (0..80)
        .map(|index| format!("fn item_{index}() {{ println!(\"old {index}\"); }}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut new_lines = old.lines().map(str::to_string).collect::<Vec<_>>();
    new_lines[40] = "fn item_40() { println!(\"new 40\"); }".to_string();
    let new = new_lines.join("\n");
    let result_text = "replaced 1 occurrence(s) in src/lib.rs".to_string();
    let result_kind = OutputBlockKind::ToolResult(ToolResultBlockView {
        key: "edit-result".into(),
        tool_title: "Edit".into(),
        args_preview: Some(r#"{"file_path":"src/lib.rs"}"#.into()),
        result_text: result_text.clone(),
        data: Some(serde_json::json!({ "old": old, "new": new, "start_line": 1 })),
        style: SemanticStyle::Success,
    });
    let tool_kind = OutputBlockKind::ToolCall(ToolCallBlockView {
        key: "edit".into(),
        chat_id: None,
        turn_id: None,
        tool_call_id: Some("edit".into()),
        title: "Edit".into(),
        icon: "✓".into(),
        semantic_status: ToolSemanticStatus::Success,
        style: SemanticStyle::Success,
        args_preview: Some(r#"{"file_path":"src/lib.rs"}"#.into()),
        activity_lines: Vec::new(),
        result_summary: Some(result_text),
        result_payload: None,
        workspace_root: None,
        collapsible: false,
        collapsed: false,
        agent_meta: None,
    });
    let vm = vm_with_roots(vec![BlockNode {
        block_id: "edit".into(),
        block_version: tool_kind.cache_version(),
        kind: tool_kind,
        children: vec![BlockNode {
            block_id: "edit-result".into(),
            block_version: result_kind.cache_version(),
            kind: result_kind,
            children: Vec::new(),
        }],
    }]);
    let mut renderer = OutputDocumentRenderer::default();

    let (_, cold) = crate::tui::render::performance::capture(|| {
        renderer.render_model_document(&vm, 100, 100, 0, MarkdownSpacingPolicy::normal())
    });
    let (_, warm) = crate::tui::render::performance::capture(|| {
        renderer.render_model_document(&vm, 100, 100, 1, MarkdownSpacingPolicy::normal())
    });

    assert_eq!(cold.edit_diff_calls, 1);
    assert_eq!(cold.diff_build_calls, 1);
    assert!(cold.syntax_highlight_calls > 0);
    assert_eq!(cold.block_cache_misses, 2);
    assert_eq!(cold.gutted_cache_misses, 2);
    assert_eq!(warm.edit_diff_calls, 0);
    assert_eq!(warm.diff_build_calls, 0);
    assert_eq!(warm.syntax_highlight_calls, 0);
    assert_eq!(
        warm.block_cache_hits, 0,
        "gutted cache 命中应短路内层 block cache"
    );
    assert_eq!(warm.block_cache_misses, 0);
    assert_eq!(warm.gutted_cache_hits, 2);
    assert_eq!(warm.gutted_cache_misses, 0);
}

#[test]
fn retained_cache_capacity_tracks_current_and_peak_entries() {
    let mut renderer = OutputDocumentRenderer::default();
    let large = vm_with_roots(vec![
        assistant_node("a", "alpha"),
        assistant_node("b", "beta"),
    ]);
    let small = vm_with_roots(vec![assistant_node("b", "beta")]);

    renderer.render_tree(&large, 80);
    let large_capacity = renderer.retained_cache_capacity();
    assert_eq!(large_capacity.block_entries, 2);
    assert_eq!(large_capacity.gutted_entries, 2);
    assert_eq!(large_capacity.root_layout_entries, 2);
    assert_eq!(large_capacity.peak_block_entries, 2);
    assert_eq!(large_capacity.peak_gutted_entries, 2);
    assert_eq!(large_capacity.peak_root_layout_entries, 2);

    renderer.render_tree(&small, 80);
    let retained = renderer.retained_cache_capacity();
    assert_eq!(retained.block_entries, 1);
    assert_eq!(retained.gutted_entries, 1);
    assert_eq!(retained.root_layout_entries, 1);
    assert_eq!(retained.peak_block_entries, 2);
    assert_eq!(retained.peak_gutted_entries, 2);
    assert_eq!(retained.peak_root_layout_entries, 2);

    renderer.render_tree(&vm_with_roots(Vec::new()), 80);
    let empty = renderer.retained_cache_capacity();
    assert_eq!(empty.block_entries, 0);
    assert_eq!(empty.gutted_entries, 0);
    assert_eq!(empty.root_layout_entries, 0);
    assert_eq!(empty.peak_block_entries, 2);
    assert_eq!(empty.peak_gutted_entries, 2);
    assert_eq!(empty.peak_root_layout_entries, 2);
}

#[test]
fn resize_and_spinner_frame_do_not_grow_retained_cache_entries() {
    let mut renderer = OutputDocumentRenderer::default();
    let vm = vm_with_roots(vec![assistant_node("stable", "content")]);

    renderer.render_tree_with_animation_frame(&vm, 80, 0, MarkdownSpacingPolicy::normal());
    renderer.render_tree_with_animation_frame(&vm, 100, 1, MarkdownSpacingPolicy::normal());

    let retained = renderer.retained_cache_capacity();
    assert_eq!(retained.block_entries, 1);
    assert_eq!(retained.gutted_entries, 1);
    assert_eq!(retained.root_layout_entries, 1);
    assert_eq!(retained.peak_block_entries, 1);
    assert_eq!(retained.peak_gutted_entries, 1);
    assert_eq!(retained.peak_root_layout_entries, 1);
}

#[test]
fn test_render_tree_various_widths_keep_every_line_within_outer_width() {
    // 横扫多种 outer_width / text 长度组合，确保 wrap 边界 + gutter 后不超。
    // 修复前：text 长度 = outer_width 时必失败；修复后全过。
    for outer_width in [10u16, 20, 40, 77, 120] {
        for text_len in [outer_width, outer_width + 5, outer_width * 2] {
            let text = "x".repeat(text_len as usize);
            let mut renderer = OutputDocumentRenderer::default();
            let vm = vm_with_roots(vec![assistant_node("a", &text)]);
            let doc = renderer.render_tree(&vm, outer_width);

            for (idx, line) in doc.blocks[0].lines.iter().enumerate() {
                let visible = visible_line_width(line);
                assert!(
                    visible <= outer_width as usize,
                    "outer_width={} text_len={} line[{}] 可见宽 {} > {}（#329）",
                    outer_width,
                    text_len,
                    idx,
                    visible,
                    outer_width
                );
            }
        }
    }
}
