#[test]
fn test_output_assembler_maps_tool_status_to_icon() {
    let mut conversation = ConversationModel::default();
    add_completed_tool_after_thinking(&mut conversation, "Read", "ok");

    let vm = assemble_output_view(&conversation, None);
    let tool = vm
        .roots
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::ToolCall(tool) => Some(tool),
            _ => None,
        })
        .expect("tool block");

    assert_eq!(tool.icon, "✓");
    assert_eq!(tool.semantic_status, ToolSemanticStatus::Success);
}

#[test]
fn test_output_assembler_keeps_tool_result_inside_tool_after_thinking() {
    let mut conversation = ConversationModel::default();
    add_completed_tool_after_thinking(
        &mut conversation,
        "Grep",
        "/tmp/docs/bug/active.md:18:match",
    );

    let vm = assemble_output_view(&conversation, None);
    let diagnostic_results = vm
        .roots
        .iter()
        .filter(|block| matches!(&block.kind, OutputBlockKind::DiagnosticNotice(_)))
        .count();
    let tool = vm
        .roots
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::ToolCall(tool) => Some(tool),
            _ => None,
        })
        .expect("tool block");

    assert_eq!(diagnostic_results, 0);
    assert_eq!(tool.title, "Grep");
    // result 子块携带实际 output（供渲染层截断成预览），不再是纯摘要。
    assert_eq!(
        tool.result_summary.as_deref(),
        Some("/tmp/docs/bug/active.md:18:match")
    );
}

#[test]
fn test_output_assembler_embedded_result_carries_output_for_preview() {
    // result 子块的 result_text = 实际工具 output（供渲染层 format_result_lines 按
    // result_max_lines 截断成前 N 行预览）；完整内容不刷屏由渲染层截断保证，
    // assembler 不再退化为纯 "✓ Read completed" 摘要，且结果不泄漏为 root DiagnosticNotice。
    let mut conversation = ConversationModel::default();
    let full_output = "line1\nline2\nline3\nline4\nline5\nline6";
    add_completed_tool_after_thinking(&mut conversation, "Read", full_output);

    let vm = assemble_output_view(&conversation, None);
    let diagnostic_results = vm
        .roots
        .iter()
        .filter(|block| matches!(&block.kind, OutputBlockKind::DiagnosticNotice(_)))
        .count();
    let tool_node = vm
        .roots
        .iter()
        .find(|block| matches!(&block.kind, OutputBlockKind::ToolCall(_)))
        .expect("tool block");
    let OutputBlockKind::ToolCall(tool) = &tool_node.kind else {
        panic!("expected tool call");
    };

    assert_eq!(
        diagnostic_results, 0,
        "结果不应泄漏为 root DiagnosticNotice"
    );
    assert_eq!(tool.result_summary.as_deref(), Some(full_output));
    assert_eq!(tool_node.children.len(), 1);
    let OutputBlockKind::ToolResult(result) = &tool_node.children[0].kind else {
        panic!("expected tool result child");
    };
    assert_eq!(result.result_text, full_output);
}

#[test]
fn test_output_assembler_keeps_assistant_text_outside_read_result() {
    let mut conversation = ConversationModel::default();
    conversation.ensure_runtime_turn(
        crate::tui::model::conversation::ids::ChatId::new("session-1"),
        crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
    );
    conversation.apply(AppendUserMessage { text: "查看 active bug".to_string() });
    add_completed_tool(
        &mut conversation,
        "tool-read",
        "Read",
        r#"{"file_path":"docs/bug/active.md"}"#,
        "## 活跃 Bug（21 个）\n\n # │ 标题 │ 优先级 │ 状态\n|---|------|--------|------|",
        false,
    );
    conversation.apply(AssistantText {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        run_id: crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        text: "我看到 active bug 列表，下面是分析。".to_string(),
    });

    let vm = assemble_output_view(&conversation, None);
    let tool_node = vm
        .roots
        .iter()
        .find(|block| matches!(&block.kind, OutputBlockKind::ToolCall(_)))
        .expect("tool block");
    let assistant = vm
        .roots
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::AssistantMessage(text) => Some(text),
            _ => None,
        })
        .expect("assistant text block");

    let OutputBlockKind::ToolCall(tool) = &tool_node.kind else {
        panic!("expected tool call");
    };
    let read_output =
        "## 活跃 Bug（21 个）\n\n # │ 标题 │ 优先级 │ 状态\n|---|------|--------|------|";
    assert_eq!(tool.result_summary.as_deref(), Some(read_output));
    let OutputBlockKind::ToolResult(result) = &tool_node.children[0].kind else {
        panic!("expected tool result child");
    };
    assert_eq!(result.result_text, read_output);
    // #87 核心仍成立：assistant 正文保持独立 block，不混入 ToolResult 子块。
    assert_eq!(assistant.text, "我看到 active bug 列表，下面是分析。");
}

#[test]
fn test_output_assembler_late_bound_tool_result_stays_inside_tool_block() {
    let mut conversation = ConversationModel::default();
    conversation.ensure_runtime_turn(
        crate::tui::model::conversation::ids::ChatId::new("session-1"),
        crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
    );
    conversation.apply(AppendUserMessage { text: "edit docs".to_string() });
    conversation.apply(ToolCallStart {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        run_id: crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        id: ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Edit".to_string(),
        index: 0,
    });
    conversation.apply(ToolResult {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        run_id: crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        provider_id: "provider-1".to_string(),
        id: ToolCallId::new("tool-1"),
        tool_name: "Edit".to_string(),
        output: "replaced 1 occurrence(s) in docs/bug/active.md\n---DIFF---\nold\n---DIFF---\nnew"
            .to_string(),
        content: serde_json::json!({ "text": "replaced 1 occurrence(s) in docs/bug/active.md\n---DIFF---\nold\n---DIFF---\nnew" }),
        is_error: false,
        image_count: 0,
    });
    conversation.apply(ToolCallUpdate {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        run_id: crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        provider_id: Some("provider-1".to_string()),
        id: ToolCallId::new("tool-1"),
        name: "Edit".to_string(),
        index: 0,
        arguments: None,
        status: ToolCallStatus::Ready,
    });

    let vm = assemble_output_view(&conversation, None);
    let diagnostics = vm
        .roots
        .iter()
        .filter(|block| matches!(&block.kind, OutputBlockKind::DiagnosticNotice(_)))
        .count();
    let tool_root = vm
        .roots
        .iter()
        .find(|block| matches!(&block.kind, OutputBlockKind::ToolCall(_)))
        .expect("tool block");

    assert_eq!(diagnostics, 0, "已绑定工具结果不应泄漏成块外诊断文本");

    // 嵌入式 Edit ToolResult 子块应渲染为加减色 diff：result_summary 携带实际 output
    // （含 ---DIFF--- 标记，#64），render_tool_result → render_edit_diff 消费标记，
    // 输出 old/new diff 行（refs #90）。
    let result_child = tool_root
        .children
        .iter()
        .find(|child| matches!(&child.kind, OutputBlockKind::ToolResult(_)))
        .expect("ToolResult 子块存在");
    let rendered = result_child
        .kind
        .component()
        .render_self(&result_child.block_id, &RenderCtx::for_width(80 ));
    let plains: Vec<&str> = rendered.lines.iter().map(|l| l.plain.as_str()).collect();

    assert!(
        !plains.iter().any(|p| p.contains("---DIFF---")),
        "diff 渲染后不应残留原始 ---DIFF--- 标记, got: {plains:?}"
    );
    assert!(
        !plains.iter().any(|p| p.contains("Edit completed")),
        "Edit 结果应渲染为 diff 而非 ✓ Edit completed 摘要, got: {plains:?}"
    );
    assert!(
        plains.iter().any(|p| p.contains("- ") && p.contains("old")),
        "应含删除行（- old）, got: {plains:?}"
    );
    assert!(
        plains.iter().any(|p| p.contains("+ ") && p.contains("new")),
        "应含新增行（+ new）, got: {plains:?}"
    );
}

#[test]
fn test_output_assembler_uses_error_summary_for_failed_tool_result() {
    let mut conversation = ConversationModel::default();
    add_failed_tool_after_thinking(&mut conversation, "Read", "permission denied");

    let vm = assemble_output_view(&conversation, None);
    let tool = vm
        .roots
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::ToolCall(tool) => Some(tool),
            _ => None,
        })
        .expect("tool block");

    // 失败工具的 result 子块也携带实际错误 output（渲染层以 Error 色截断展示），
    // 不再退化为纯 "✗ Read failed" 摘要。
    assert_eq!(tool.result_summary.as_deref(), Some("permission denied"));
}

#[test]
fn test_output_assembler_attaches_tool_result_as_child_of_tool_call() {
    // 工具结果升为子块（#60）：完成的 ToolCall root 应带一个 ToolResult 子节点，
    // 子节点 key 为 `<toolid>-result`，且子节点本身为叶子。
    let mut conversation = ConversationModel::default();
    add_completed_tool_after_thinking(&mut conversation, "Read", "ok");

    let vm = assemble_output_view(&conversation, None);

    let tool_node = vm
        .roots
        .iter()
        .find(|n| matches!(&n.kind, OutputBlockKind::ToolCall(_)))
        .expect("tool call root");
    assert_eq!(tool_node.children.len(), 1, "完成工具应附带 1 个结果子块");
    let result = &tool_node.children[0];
    assert!(
        matches!(&result.kind, OutputBlockKind::ToolResult(_)),
        "子块应为 ToolResult 变体"
    );
    let expected_tool_id = ToolCallId::new("tool-1");
    assert_eq!(
        result.block_id,
        format!("{}-result", expected_tool_id.as_ref())
    );
    assert!(result.children.is_empty(), "ToolResult 为叶子");
    // ToolResult 不应作为顶层 root 出现（必须是 tool_call 的子）。
    assert!(
        !vm.roots
            .iter()
            .any(|n| matches!(&n.kind, OutputBlockKind::ToolResult(_))),
        "ToolResult 不应是顶层 root"
    );
}

#[test]
fn test_output_assembler_tool_arguments_delta_updates_header_before_result() {
    let mut conversation = ConversationModel::default();
    conversation.ensure_runtime_turn(
        crate::tui::model::conversation::ids::ChatId::new("session-1"),
        crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
    );
    conversation.apply(AppendUserMessage { text: "read file".to_string() });
    conversation.apply(ToolCallStart {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        run_id: crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        id: ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });
    conversation.apply(ToolCallUpdate {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        run_id: crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        id: ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
        arguments: Some(r#"{"file_path":"src/lib.rs"}"#.to_string()),
        status: ToolCallStatus::Ready,
    });

    let vm = assemble_output_view(&conversation, None);
    let tool = vm
        .roots
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::ToolCall(tool) => Some(tool),
            _ => None,
        })
        .expect("tool block");

    assert_eq!(tool.title, "Read");

    assert_eq!(
        tool.args_preview.as_deref(),
        Some(r#"{"file_path":"src/lib.rs"}"#)
    );
    assert!(tool.result_summary.is_none(), "ToolResult 尚未到达");
    let rendered = OutputBlockKind::ToolCall(tool.clone())
        .component()
        .render_self("tool-1", &RenderCtx::for_width(80 ));
    assert!(
        rendered
            .lines
            .iter()
            .any(|line| line.plain.contains("src/lib.rs")),
        "summary 尚未到达、result 尚未到达时，应使用 args_preview 提前渲染 header/detail"
    );
}

#[test]
fn test_output_assembler_write_arguments_delta_updates_realtime_bytes_header() {
    let mut conversation = ConversationModel::default();
    conversation.ensure_runtime_turn(
        crate::tui::model::conversation::ids::ChatId::new("session-1"),
        crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
    );
    conversation.apply(AppendUserMessage { text: "write file".to_string() });
    conversation.apply(ToolCallStart {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        run_id: crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        id: ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Write".to_string(),
        index: 0,
    });
    conversation.apply(ToolCallUpdate {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        run_id: crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        id: ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Write".to_string(),
        index: 0,
        arguments: Some(
            r#"{"file_path":"out.rs","content":"hello world","content_bytes":11}"#.to_string(),
        ),
        status: ToolCallStatus::Ready,
    });

    let vm = assemble_output_view(&conversation, None);
    let tool = vm
        .roots
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::ToolCall(tool) => Some(tool),
            _ => None,
        })
        .expect("tool block");
    let rendered = OutputBlockKind::ToolCall(tool.clone())
        .component()
        .render_self("tool-1", &RenderCtx::for_width(80 ));

    assert!(
        rendered
            .lines
            .iter()
            .any(|line| line.plain.contains("11 bytes")),
        "Write running header 应显示 realtime content_bytes，实际: {:?}",
        rendered
            .lines
            .iter()
            .map(|line| line.plain.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_output_assembler_pending_tool_has_no_result_child() {
    // 边界：未产出结果（仅 ToolCallStart，无 ToolResult）的工具不附结果子块。
    let mut conversation = ConversationModel::default();
    conversation.ensure_runtime_turn(
        crate::tui::model::conversation::ids::ChatId::new("session-1"),
        crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
    );
    conversation.apply(AppendUserMessage { text: "search".to_string() });
    conversation.apply(ToolCallStart {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        run_id: crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        id: ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });
    conversation.apply(ToolCallUpdate {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        run_id: crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        provider_id: Some("provider-1".to_string()),
        id: ToolCallId::new("tool-1"),
        name: "Read".to_string(),
        index: 0,
        arguments: None,
        status: ToolCallStatus::Ready,
    });

    let vm = assemble_output_view(&conversation, None);

    let tool_node = vm
        .roots
        .iter()
        .find(|n| matches!(&n.kind, OutputBlockKind::ToolCall(_)))
        .expect("tool call root");
    assert!(
        tool_node.children.is_empty(),
        "无结果的工具不应附带结果子块"
    );
}

#[test]
fn test_output_assembler_hides_streaming_preview_when_tool_completed() {
    // 回归：Agent 工具完成后，streaming_preview 应为 None，让位给权威最终 ToolResult 子块。
    let mut conversation = ConversationModel::default();
    conversation.ensure_runtime_turn(
        crate::tui::model::conversation::ids::ChatId::new("session-1"),
        crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
    );
    conversation.apply(AppendUserMessage { text: "run sub-agent".to_string() });
    conversation.apply(ToolCallStart {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        run_id: crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        id: ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Agent".to_string(),
        index: 0,
    });
    conversation.apply(ToolCallUpdate {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        run_id: crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        provider_id: Some("provider-1".to_string()),
        id: ToolCallId::new("tool-1"),
        name: "Agent".to_string(),
        index: 0,
        arguments: Some(r#"{"description":"sub-task","prompt":"do stuff"}"#.to_string()),
        status: ToolCallStatus::Ready,
    });
    // 子代理运行中发送 progress（写入 activities）
    conversation.record_agent_activities(
        crate::tui::model::conversation::ids::ChatId::new("session-1"),
        crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        ToolCallId::new("tool-1"),
        vec![crate::tui::model::conversation::agent_activity::AgentActivityLine::message("子代理最终输出文本".to_string())],
    );
    // 工具完成
    conversation.apply(ToolResult {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        run_id: crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        provider_id: "provider-1".to_string(),
        id: ToolCallId::new("tool-1"),
        tool_name: "Agent".to_string(),
        output: "子代理最终输出文本".to_string(),
        content: serde_json::json!({ "text": "子代理最终输出文本" }),
        is_error: false,
        image_count: 0,
    });

    let vm = assemble_output_view(&conversation, None);
    let tool = vm
        .roots
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::ToolCall(tool) => Some(tool),
            _ => None,
        })
        .expect("tool block");

    assert!(
        tool.streaming_preview.is_none(),
        "工具完成后 streaming_preview 应为 None（结果已在权威 ToolResult 子块），实际: {:?}",
        tool.streaming_preview
    );
    assert_eq!(
        tool.result_summary.as_deref(),
        Some("子代理最终输出文本"),
        "结果应在 ToolResult 子块中展示"
    );
}

#[test]
fn sub_run_tool_call_uses_current_workspace_root_during_view_assembly() {
    let mut conversation = ConversationModel::default();
    let chat_id = crate::tui::model::conversation::ids::ChatId::new("session-1");
    let run_id = crate::tui::model::conversation::ids::ChatRunId::new("turn-1");
    let parent_tool_id = ToolCallId::new("tool-1");
    conversation.ensure_runtime_turn(chat_id.clone(), run_id.clone());
    conversation.apply(ToolCallStart {
        chat_id: chat_id.clone(),
        run_id: run_id.clone(),
        id: parent_tool_id.clone(),
        provider_id: None,
        name: "Agent".to_string(),
        index: 0,
    });
    conversation.apply(RecordSubRunActivity {
        agent_id: "researcher".to_string(),
        sub_run_id: "sub-run".to_string(),
        parent_run_id: run_id.to_string(),
        spawned_by_tool_call_id: parent_tool_id,
        sequence: 1,
        sequence_index: 0,
        kind: TuiSubRunActivityKind::ToolCall {
            id: "read-call".to_string(),
            name: "Read".to_string(),
            input: serde_json::json!({"file_path": "/repo/src/domain.rs"}),
        },
    });

    let view = assemble_output_view(&conversation, Some(std::path::Path::new("/repo")));
    let tool = view
        .roots
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::ToolCall(tool) => Some(tool),
            _ => None,
        })
        .expect("parent Agent tool block");

    assert_eq!(
        tool.streaming_preview,
        Some(vec![AgentActivityLineView {
            kind: AgentActivityKindView::ToolCall,
            content: crate::tui::view_model::output::AgentActivityContentView::ToolCall {
                name: "Read".to_string(),
                input: serde_json::json!({"file_path": "/repo/src/domain.rs"}),
            },
        }])
    );
}

#[test]
fn test_output_assembler_shows_streaming_preview_while_tool_running() {
    // 运行中（未完成）的工具应将 activities 合并为 streaming_preview 文本。
    let mut conversation = ConversationModel::default();
    conversation.ensure_runtime_turn(
        crate::tui::model::conversation::ids::ChatId::new("session-1"),
        crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
    );
    conversation.apply(AppendUserMessage { text: "run sub-agent".to_string() });
    conversation.apply(ToolCallStart {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        run_id: crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        id: ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Agent".to_string(),
        index: 0,
    });
    conversation.apply(ToolCallUpdate {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        run_id: crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        provider_id: Some("provider-1".to_string()),
        id: ToolCallId::new("tool-1"),
        name: "Agent".to_string(),
        index: 0,
        arguments: Some(r#"{"description":"sub-task","prompt":"do stuff"}"#.to_string()),
        status: ToolCallStatus::Ready,
    });
    conversation.record_agent_activities(
        crate::tui::model::conversation::ids::ChatId::new("session-1"),
        crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        ToolCallId::new("tool-1"),
        vec![crate::tui::model::conversation::agent_activity::AgentActivityLine::message("Agent turn 1/200, messages: 2, est_tokens: 500".to_string())],
    );

    let vm = assemble_output_view(&conversation, None);
    let tool = vm
        .roots
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::ToolCall(tool) => Some(tool),
            _ => None,
        })
        .expect("tool block");

    assert_eq!(
        tool.streaming_preview,
        Some(vec![AgentActivityLineView {
            kind: AgentActivityKindView::Message,
            content: "Agent turn 1/200, messages: 2, est_tokens: 500".into(),
        }])
    );
}

#[test]
fn test_output_assembler_streaming_preview_is_tool_result_child() {
    // #1547：运行中工具的 streaming preview 必须是 ToolCall 的 ToolResult 子节点
    // （block_id = `<tool-id>-streaming-result`），由 gutter 统一管理 marker/缩进，
    // 而非留在 ToolCall 内部的 activity 行被 renderer 手工拼接。
    let mut conversation = ConversationModel::default();
    conversation.ensure_runtime_turn(
        crate::tui::model::conversation::ids::ChatId::new("session-1"),
        crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
    );
    conversation.apply(AppendUserMessage { text: "run sub-agent".to_string() });
    conversation.apply(ToolCallStart {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        run_id: crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        id: ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Agent".to_string(),
        index: 0,
    });
    conversation.apply(ToolCallUpdate {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        run_id: crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        provider_id: Some("provider-1".to_string()),
        id: ToolCallId::new("tool-1"),
        name: "Agent".to_string(),
        index: 0,
        arguments: Some(r#"{"description":"sub-task","prompt":"do stuff"}"#.to_string()),
        status: ToolCallStatus::Ready,
    });
    conversation.record_agent_activities(
        crate::tui::model::conversation::ids::ChatId::new("session-1"),
        crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        ToolCallId::new("tool-1"),
        vec![
            AgentActivityLine::tool_call(
                "Read",
                serde_json::json!({"file_path": "src/domain.rs"}),
            ),
            AgentActivityLine::message("Read as ordinary prose"),
        ],
    );

    let vm = assemble_output_view(&conversation, None);
    let tool_node = vm
        .roots
        .iter()
        .find(|n| matches!(&n.kind, OutputBlockKind::ToolCall(_)))
        .expect("tool call root");

    assert_eq!(
        tool_node.children.len(),
        1,
        "运行中工具应有一个 streaming ToolResult 子节点"
    );
    let result = &tool_node.children[0];
    assert!(
        matches!(&result.kind, OutputBlockKind::ToolResult(_)),
        "子块应为 ToolResult 变体"
    );
    assert_eq!(
        result.block_id, "tool-1-streaming-result",
        "streaming 子节点 block_id 应为 `<tool-id>-streaming-result`"
    );
    let OutputBlockKind::ToolResult(result_view) = &result.kind else {
        panic!("expected tool result");
    };
    assert_eq!(
        result_view.activity_lines,
        Some(vec![
            AgentActivityLineView {
                kind: AgentActivityKindView::ToolCall,
                content: crate::tui::view_model::output::AgentActivityContentView::ToolCall {
                    name: "Read".to_string(),
                    input: serde_json::json!({"file_path": "src/domain.rs"}),
                },
            },
            AgentActivityLineView {
                kind: AgentActivityKindView::Message,
                content: "Read as ordinary prose".into(),
            },
        ])
    );
    assert_eq!(result_view.result_text, "Read as ordinary prose");
}

#[test]
fn test_output_assembler_completed_tool_has_single_authoritative_result_child() {
    // #1547：工具完成后仅有唯一权威最终 ToolResult 子节点（`<tool-id>-result`），
    // 不应同时存在 streaming-result 子节点（二者互斥）。
    let mut conversation = ConversationModel::default();
    conversation.ensure_runtime_turn(
        crate::tui::model::conversation::ids::ChatId::new("session-1"),
        crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
    );
    conversation.apply(AppendUserMessage { text: "run sub-agent".to_string() });
    conversation.apply(ToolCallStart {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        run_id: crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        id: ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Agent".to_string(),
        index: 0,
    });
    conversation.apply(ToolCallUpdate {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        run_id: crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        provider_id: Some("provider-1".to_string()),
        id: ToolCallId::new("tool-1"),
        name: "Agent".to_string(),
        index: 0,
        arguments: Some(r#"{"description":"sub-task","prompt":"do stuff"}"#.to_string()),
        status: ToolCallStatus::Ready,
    });
    // 运行中发送 streaming progress
    conversation.record_agent_activities(
        crate::tui::model::conversation::ids::ChatId::new("session-1"),
        crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        ToolCallId::new("tool-1"),
        vec![crate::tui::model::conversation::agent_activity::AgentActivityLine::message("intermediate preview".to_string())],
    );
    // 工具完成
    conversation.apply(ToolResult {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        run_id: crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        provider_id: "provider-1".to_string(),
        id: ToolCallId::new("tool-1"),
        tool_name: "Agent".to_string(),
        output: "final output".to_string(),
        content: serde_json::json!({ "text": "final output" }),
        is_error: false,
        image_count: 0,
    });

    let vm = assemble_output_view(&conversation, None);
    let tool_node = vm
        .roots
        .iter()
        .find(|n| matches!(&n.kind, OutputBlockKind::ToolCall(_)))
        .expect("tool call root");

    assert_eq!(
        tool_node.children.len(),
        1,
        "完成工具应仅有 1 个权威最终 ToolResult 子节点"
    );
    let result = &tool_node.children[0];
    assert!(
        matches!(&result.kind, OutputBlockKind::ToolResult(_)),
        "子块应为 ToolResult 变体"
    );
    assert_eq!(
        result.block_id, "tool-1-result",
        "完成工具子节点 block_id 应为 `<tool-id>-result`，不应残留 streaming-result"
    );
    // 不应同时存在 streaming-result
    assert!(
        !tool_node
            .children
            .iter()
            .any(|c| c.block_id.ends_with("-streaming-result")),
        "完成工具不应同时存在 streaming-result 子节点"
    );
}

#[test]
fn test_output_assembler_pending_tool_without_streaming_has_no_child() {
    // 边界：未产出结果、无 streaming preview 的工具不附任何子块。
    let mut conversation = ConversationModel::default();
    conversation.ensure_runtime_turn(
        crate::tui::model::conversation::ids::ChatId::new("session-1"),
        crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
    );
    conversation.apply(AppendUserMessage { text: "search".to_string() });
    conversation.apply(ToolCallStart {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        run_id: crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        id: ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });
    conversation.apply(ToolCallUpdate {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        run_id: crate::tui::model::conversation::ids::ChatRunId::new("turn-1"),
        provider_id: Some("provider-1".to_string()),
        id: ToolCallId::new("tool-1"),
        name: "Read".to_string(),
        index: 0,
        arguments: Some(r#"{"file_path":"x.rs"}"#.to_string()),
        status: ToolCallStatus::Ready,
    });

    let vm = assemble_output_view(&conversation, None);
    let tool_node = vm
        .roots
        .iter()
        .find(|n| matches!(&n.kind, OutputBlockKind::ToolCall(_)))
        .expect("tool call root");

    assert!(
        tool_node.children.is_empty(),
        "无结果且无 streaming preview 的工具不应附带子块，实际: {} children",
        tool_node.children.len()
    );
}
