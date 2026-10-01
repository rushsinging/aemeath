use super::*;
use crate::tui::view_model::output::ToolSemanticStatus;
use crate::tui::view_model::output::{AgentActivityKindView, AgentActivityLineView};
use crate::tui::view_model::style::SemanticStyle;
use unicode_width::UnicodeWidthStr;

fn tool(status: ToolSemanticStatus) -> ToolCallBlockView {
    ToolCallBlockView {
        key: "t1".into(),
        chat_id: None,
        run_id: None,
        tool_call_id: Some("t1".into()),
        title: "Grep".into(),
        icon: "●".into(),
        semantic_status: status,
        style: SemanticStyle::Running,
        args_preview: Some("/foo/".into()),
        streaming_preview: None,
        result_summary: None,
        result_payload: None,
        workspace_root: None,
        collapsible: false,
        collapsed: false,
        agent_meta: None,
    }
}

#[test]
fn test_tool_call_running_applies_text_color_to_title() {
    // marker（●）现由 gutter 注入；header 文本统一使用 TEXT 色（与 assistant message 一致），
    // 任务状态由 gutter 颜色表示。颜色通过 RenderedLine 的 line base style 传递。
    let block = render_tool_call(
        "t1",
        &tool(ToolSemanticStatus::Running),
        &RenderCtx::for_width(80),
    );
    // header 行的 line base style 应为 TEXT
    assert_eq!(block.lines[0].style.fg, Some(theme::TEXT));
    assert!(block.lines[0].plain.contains("Search"));
    // header 行不再自写 marker 字形（gutter.rs 覆盖 marker）。
    assert!(
        !block.lines[0].plain.starts_with('●'),
        "header 不应自写 ● marker"
    );
}

#[test]
fn test_tool_call_success_uses_text_title_color() {
    let mut view = tool(ToolSemanticStatus::Success);
    view.style = SemanticStyle::Success;
    view.icon = "✓".into();
    let block = render_tool_call("t1", &view, &RenderCtx::for_width(80));
    // header 行的 line base style 应为 TEXT
    assert_eq!(block.lines[0].style.fg, Some(theme::TEXT));
    assert!(block.lines[0].plain.contains("Search"));
    assert!(
        !block.lines[0].plain.starts_with('✓'),
        "header 不应自写 ✓ marker"
    );
}

#[test]
fn test_tool_call_renders_args_detail_from_summary() {
    // summary 提供工具入参 JSON，经 format_tool_call 产出 header + detail，
    // 验证参数预览作为 detail 行渲染（取代旧 OutputArea 命令式 push）。
    let mut view = tool(ToolSemanticStatus::Running);
    view.title = "Grep".into();
    view.args_preview = Some(r#"{"pattern":"test","path":"src"}"#.into());

    let block = render_tool_call("t1", &view, &RenderCtx::for_width(80));

    // header 含工具名
    assert!(block.lines[0].plain.contains("Search"));
    // Grep header 现在包含 pattern 和 path
    assert!(block.lines[0].plain.contains("test"));
}

#[test]
fn test_tool_call_renders_args_detail_from_args_preview_before_summary() {
    let mut view = tool(ToolSemanticStatus::Running);
    view.title = "Grep".into();
    view.args_preview = Some(r#"{"pattern":"test","path":"src"}"#.into());

    let block = render_tool_call("t1", &view, &RenderCtx::for_width(80));

    assert!(block.lines[0].plain.contains("Search"));
    assert!(
        block.lines[0].plain.contains("test"),
        "ToolArgumentsDelta 后应不等 ToolResult/最终 summary 就显示 header"
    );
}

#[test]
fn test_tool_call_renders_header_only_no_result_lines() {
    // 结果已升为独立子块（ToolResult），tool_call 仅渲染 header（+ args detail）。
    // 即使 result_summary 有值，也不应出现在本块内。
    let mut view = tool(ToolSemanticStatus::Success);
    view.title = "Bash".into();
    view.result_summary = Some("done: 3 matches".into());
    view.args_preview = None;

    let block = render_tool_call("t1", &view, &RenderCtx::for_width(80));

    assert_eq!(block.lines.len(), 1, "无 summary 时只应有 header 行");
    assert!(
        block
            .lines
            .iter()
            .all(|line| !line.plain.contains("done: 3 matches")),
        "结果文本不应出现在 tool_call 块内（已升为子块）"
    );
}

// ── issue #361 回归：tool_call 三部分（header / detail / activity）应消费
// ctx.text_width 做 wrap，窄终端下不溢出。修前 _ctx 被忽略，长内容超出被截断。

#[test]
fn test_tool_call_wraps_long_header_to_text_width() {
    // 未注册工具的长 display name（如 MCP 工具 mcp__github__create_issue）走
    // format_tool_call fallback：header = "● {display_name}"，strip_leading_bullet
    // 去掉 "● "，header 实际为 display_name 本身。窄终端应 wrap 到 ctx.text_width。
    let mut view = tool(ToolSemanticStatus::Running);
    view.title = "mcp__github__create_issue_a_very_long_tool_name".into();
    view.args_preview = Some("{}".into()); // 触发 fallback header 路径

    let block = render_tool_call("t1", &view, &RenderCtx::for_width(20));

    assert!(!block.lines.is_empty(), "block 至少应有 header 行");
    for (i, line) in block.lines.iter().enumerate() {
        assert!(
            line.plain.width() <= 20,
            "header 行 #{i} 宽度 {} 超 20: {:?}",
            line.plain.width(),
            line.plain
        );
    }
}

#[test]
fn test_tool_call_wraps_long_detail_lines_to_text_width() {
    // 未注册工具的长 args JSON 经 fallback detail（truncate_json ≤100 字符）渲染为
    // detail 行。窄终端应 wrap 而非整行溢出。
    let mut view = tool(ToolSemanticStatus::Running);
    view.title = "UnknownLongToolName".into();
    let long_value = "x".repeat(120);
    view.args_preview = Some(format!(r#"{{"key":"{long_value}"}}"#));

    let block = render_tool_call("t1", &view, &RenderCtx::for_width(40));

    assert!(
        block.lines.len() >= 2,
        "应有 header + detail 行，实际: {:?}",
        block
            .lines
            .iter()
            .map(|l| l.plain.as_str())
            .collect::<Vec<_>>()
    );
    for (i, line) in block.lines.iter().enumerate() {
        assert!(
            line.plain.width() <= 40,
            "行 #{i} 宽度 {} 超 40: {:?}",
            line.plain.width(),
            line.plain
        );
    }
}

#[test]
fn test_tool_call_does_not_render_streaming_preview_inline() {
    // #1547：streaming preview 由 gutter 管理的 ToolResult 子块渲染，
    // ToolCall 自身只渲染 header/detail，不含预览内容或 ⎿ marker。
    let mut view = tool(ToolSemanticStatus::Running);
    view.title = "Bash".into();
    view.args_preview = Some(r#"{"command":"ls"}"#.into());
    view.streaming_preview = Some(vec![AgentActivityLineView {
        kind: AgentActivityKindView::Message,
        content: "子任务正在执行一个非常长的操作描述文本用于测试窄终端下 activity 行的换行行为"
            .into(),
    }]);

    let block = render_tool_call("t1", &view, &RenderCtx::for_width(30));

    for (i, line) in block.lines.iter().enumerate() {
        assert!(
            !line.plain.contains('⎿'),
            "ToolCall 行 #{i} 不应内联渲染 ⎿ marker: {:?}",
            line.plain
        );
        assert!(
            !line.plain.contains("子任务正在执行"),
            "ToolCall 行 #{i} 不应内联渲染 streaming_preview 内容: {:?}",
            line.plain
        );
    }
}

#[test]
fn test_tool_call_does_not_render_inline_streaming_marker() {
    // #1547：running activity 已升为独立 ToolResult 子块，ToolCall 仅保留 header/detail。
    let mut view = tool(ToolSemanticStatus::Running);
    view.title = "Bash".into();
    view.args_preview = Some(r#"{\"command\":\"seq 1 6\"}"#.into());
    view.streaming_preview = Some(vec![
        AgentActivityLineView {
            kind: AgentActivityKindView::ToolCall,
            content: "2".into(),
        },
        AgentActivityLineView {
            kind: AgentActivityKindView::Message,
            content: "still running".into(),
        },
    ]);

    let block = render_tool_call("t1", &view, &RenderCtx::for_width(80));
    let rendered: Vec<_> = block.lines.iter().map(|line| line.plain.as_str()).collect();

    assert!(rendered.iter().all(|line| !line.contains("→ 2")));
    assert!(rendered.iter().all(|line| !line.contains("still running")));
    assert!(rendered.iter().all(|line| !line.contains('⎿')));
}

#[test]
fn test_merge_agent_meta_none_returns_original() {
    // 无 agent_meta 时 JSON 原样返回（main agent 或非 Agent tool）
    let raw = r#"{"prompt":"do something","description":"task"}"#;
    assert_eq!(merge_agent_meta(raw, None), raw);
}
#[test]
fn test_merge_agent_meta_fills_role_and_model() {
    // case 2（input 只有 role 无 model）：agent_meta 补上 runtime resolve 的 model
    let raw = r#"{"prompt":"x","description":"task","role":"coder"}"#;
    let meta = AgentMetaView {
        role: Some("coder".into()),
        model: "Zhipu/glm-5.2".into(),
    };
    let merged = merge_agent_meta(raw, Some(&meta));
    let v: serde_json::Value = serde_json::from_str(&merged).unwrap();
    assert_eq!(v["role"], "coder");
    assert_eq!(v["model"], "Zhipu/glm-5.2");
}

#[test]
fn test_merge_agent_meta_overrides_input_model() {
    // input 指定 model 但 runtime resolve 出不同值时，agent_meta 优先
    let raw = r#"{"prompt":"x","description":"task","model":"Old/model"}"#;
    let meta = AgentMetaView {
        role: None,
        model: "New/model".into(),
    };
    let merged = merge_agent_meta(raw, Some(&meta));
    let v: serde_json::Value = serde_json::from_str(&merged).unwrap();
    assert_eq!(v["model"], "New/model");
}

#[test]
fn test_merge_agent_meta_empty_model_not_overwritten() {
    // agent_meta.model 为空时不覆盖 input 已有的 model
    let raw = r#"{"prompt":"x","description":"task","model":"Keep/me"}"#;
    let meta = AgentMetaView {
        role: Some("bot".into()),
        model: "".into(),
    };
    let merged = merge_agent_meta(raw, Some(&meta));
    let v: serde_json::Value = serde_json::from_str(&merged).unwrap();
    assert_eq!(v["model"], "Keep/me");
    assert_eq!(v["role"], "bot");
}

#[test]
fn test_merge_agent_meta_invalid_json_falls_back() {
    // input JSON 无效时，agent_meta 仍可构造最小 JSON
    let raw = "not valid json";
    let meta = AgentMetaView {
        role: Some("coder".into()),
        model: "Zhipu/glm-5.2".into(),
    };
    let merged = merge_agent_meta(raw, Some(&meta));
    let v: serde_json::Value = serde_json::from_str(&merged).unwrap();
    assert_eq!(v["role"], "coder");
    assert_eq!(v["model"], "Zhipu/glm-5.2");
}

// ── issue #839：args_preview 为 None 时 fallback 路径应走 format_tool_call ──

#[test]
fn test_tool_call_none_args_preview_uses_format_tool_call_fallback() {
    // args_preview 为 None（PendingArgs 阶段或空字符串被过滤）时，
    // 不应只显示裸 display name，应走 format_tool_call 统一路径。
    let mut view = tool(ToolSemanticStatus::Running);
    view.title = "TaskUpdate".into();
    view.args_preview = None;

    let block = render_tool_call("t1", &view, &RenderCtx::for_width(80));

    assert!(
        block.lines[0].plain.contains("Update Task"),
        "fallback header 应包含 display name: {}",
        block.lines[0].plain
    );
}
