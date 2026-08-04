use crate::tui::adapter::tui_runtime_event::{
    TuiChildRunActivityKind, TuiChildRunTerminalOutcome,
};

#[test]
fn child_run_activities_attach_by_parent_tool_identity_and_deduplicate() {
    let mut model = ConversationModel::default();
    let chat_id = super::ids::ChatId::new("parent-chat");
    let run_id = super::ids::ChatRunId::new("parent-run");
    let first_tool_id = super::ids::ToolCallId::new("agent-first");
    let second_tool_id = super::ids::ToolCallId::new("agent-second");

    model.ensure_runtime_turn(chat_id.clone(), run_id.clone());
    for (index, tool_id) in [first_tool_id.clone(), second_tool_id.clone()]
        .into_iter()
        .enumerate()
    {
        model.apply(ToolCallStart {
            chat_id: chat_id.clone(),
            run_id: run_id.clone(),
            id: tool_id,
            provider_id: None,
            name: "Agent".to_string(),
            index,
        });
    }

    let first_text = RecordChildRunActivity {
        agent_id: "researcher".to_string(),
        child_run_id: "child-first".to_string(),
        parent_run_id: run_id.to_string(),
        spawned_by_tool_call_id: first_tool_id.clone(),
        sequence: 1,
        kind: TuiChildRunActivityKind::Text {
            text: "first child text".to_string(),
        },
    };
    model.apply(first_text.clone());
    model.apply(first_text);
    model.apply(RecordChildRunActivity {
        agent_id: "reviewer".to_string(),
        child_run_id: "child-second".to_string(),
        parent_run_id: run_id.to_string(),
        spawned_by_tool_call_id: second_tool_id.clone(),
        sequence: 1,
        kind: TuiChildRunActivityKind::Thinking {
            text: "second child thinking".to_string(),
        },
    });
    model.apply(RecordChildRunActivity {
        agent_id: "researcher".to_string(),
        child_run_id: "child-first".to_string(),
        parent_run_id: run_id.to_string(),
        spawned_by_tool_call_id: first_tool_id.clone(),
        sequence: 2,
        kind: TuiChildRunActivityKind::ToolOutput {
            tool_name: "grep".to_string(),
            text: "grep output".to_string(),
        },
    });

    assert_eq!(
        tool_call(&model, &chat_id, &run_id, &first_tool_id)
            .expect("first parent Agent ToolCall")
            .activities,
        vec!["first child textgrep output"]
    );
    assert_eq!(
        tool_call(&model, &chat_id, &run_id, &second_tool_id)
            .expect("second parent Agent ToolCall")
            .activities,
        vec!["second child thinking"]
    );
    assert_eq!(model.child_run_activities.len(), 3);
}

#[test]
fn child_run_activity_rejects_unknown_parent_and_out_of_order_sequence() {
    let mut model = ConversationModel::default();
    let chat_id = super::ids::ChatId::new("parent-chat");
    let run_id = super::ids::ChatRunId::new("parent-run");
    let tool_id = super::ids::ToolCallId::new("agent-tool");
    model.ensure_runtime_turn(chat_id.clone(), run_id.clone());
    model.apply(ToolCallStart {
        chat_id: chat_id.clone(),
        run_id: run_id.clone(),
        id: tool_id.clone(),
        provider_id: None,
        name: "Agent".to_string(),
        index: 0,
    });

    model.apply(RecordChildRunActivity {
        agent_id: "researcher".to_string(),
        child_run_id: "child".to_string(),
        parent_run_id: run_id.to_string(),
        spawned_by_tool_call_id: tool_id.clone(),
        sequence: 2,
        kind: TuiChildRunActivityKind::Terminal {
            outcome: TuiChildRunTerminalOutcome::Completed,
        },
    });
    model.apply(RecordChildRunActivity {
        agent_id: "researcher".to_string(),
        child_run_id: "child".to_string(),
        parent_run_id: run_id.to_string(),
        spawned_by_tool_call_id: tool_id.clone(),
        sequence: 1,
        kind: TuiChildRunActivityKind::Text {
            text: "late text".to_string(),
        },
    });
    model.apply(RecordChildRunActivity {
        agent_id: "unknown".to_string(),
        child_run_id: "unknown-child".to_string(),
        parent_run_id: run_id.to_string(),
        spawned_by_tool_call_id: super::ids::ToolCallId::new("missing-agent-tool"),
        sequence: 1,
        kind: TuiChildRunActivityKind::Text {
            text: "must not attach".to_string(),
        },
    });

    assert_eq!(
        tool_call(&model, &chat_id, &run_id, &tool_id)
            .expect("parent Agent ToolCall")
            .activities,
        vec!["Sub-agent terminal: Completed"]
    );
    assert_eq!(model.child_run_activities.len(), 1);
}

#[test]
fn concurrent_agent_progress_attaches_to_matching_parent_tool_blocks() {
    let mut model = ConversationModel::default();
    let chat_id = super::ids::ChatId::new("parent-chat");
    let run_id = super::ids::ChatRunId::new("parent-turn");
    let first_tool_id = super::ids::ToolCallId::new("agent-first");
    let second_tool_id = super::ids::ToolCallId::new("agent-second");

    model.ensure_runtime_turn(chat_id.clone(), run_id.clone());
    for (index, tool_id) in [first_tool_id.clone(), second_tool_id.clone()]
        .into_iter()
        .enumerate()
    {
        model.apply(ToolCallStart {
            chat_id: chat_id.clone(),
            run_id: run_id.clone(),
            id: tool_id,
            provider_id: None,
            name: "Agent".to_string(),
            index,
        });
    }

    model.apply(RecordAgentProgress {
        chat_id: chat_id.clone(),
        run_id: run_id.clone(),
        tool_id: first_tool_id.clone(),
        message: "first child activity".to_string(),
    });
    model.apply(RecordAgentProgress {
        chat_id: chat_id.clone(),
        run_id: run_id.clone(),
        tool_id: second_tool_id.clone(),
        message: "second child activity".to_string(),
    });

    assert_eq!(
        tool_call(&model, &chat_id, &run_id, &first_tool_id)
            .expect("first parent Agent ToolCall")
            .activities,
        vec!["first child activity"]
    );
    assert_eq!(
        tool_call(&model, &chat_id, &run_id, &second_tool_id)
            .expect("second parent Agent ToolCall")
            .activities,
        vec!["second child activity"]
    );
    assert!(model
        .timeline
        .items()
        .iter()
        .all(|item| !matches!(item, OutputTimelineItem::AgentProgress { .. })));
}

/// timeline 镜像验证：完整回合（user / assistant / tool-call / tool-result）后
/// timeline 应包含 UserMessage、AssistantText、ToolCall、ToolResult，
/// 且 AgentProgress **不进 timeline**（进度通过 tool_calls[].activities 内联渲染）。
#[test]
fn test_timeline_mirrors_blocks_no_agent_progress() {
    let mut model = ConversationModel::default();
    let chat_id = super::ids::ChatId::new("chat-a42");
    let run_id = super::ids::ChatRunId::new("turn-a42");
    let tool_id = super::ids::ToolCallId::new("tool-a42");

    // 1. 用户消息
    model.apply(StartChat {
        submission: "run task".to_string(),
    });

    // 2. Assistant text
    model.apply(AssistantText {
        chat_id: chat_id.clone(),
        run_id: run_id.clone(),
        text: "starting agent".to_string(),
    });
    model.apply(CompleteBlock {
        chat_id: chat_id.clone(),
        run_id: run_id.clone(),
    });

    // 3. Tool call start
    model.apply(ToolCallStart {
        chat_id: chat_id.clone(),
        run_id: run_id.clone(),
        id: tool_id.clone(),
        provider_id: None,
        name: "Agent".to_string(),
        index: 0,
    });

    // 4. Agent progress — 不进 timeline，只写入 tool_calls[].activities
    model.apply(RecordAgentProgress {
        chat_id: chat_id.clone(),
        run_id: run_id.clone(),
        tool_id: tool_id.clone(),
        message: "analysing codebase".to_string(),
    });

    // 5. Tool result
    model.apply(ToolResult {
        chat_id: chat_id.clone(),
        run_id: run_id.clone(),
        id: tool_id.clone(),
        provider_id: "provider-a42".to_string(),
        tool_name: "Agent".to_string(),
        output: "done".to_string(),
        content: serde_json::json!({ "text": "done" }),
        is_error: false,
        image_count: 0,
    });

    // 断言 AgentProgress 不在 timeline（防双显示）
    let has_agent_progress = model
        .timeline
        .items()
        .iter()
        .any(|item| matches!(item, OutputTimelineItem::AgentProgress { .. }));
    assert!(
        !has_agent_progress,
        "timeline.items() MUST NOT contain AgentProgress (it is inline-rendered via \
         tool_calls[].activities); items = {:?}",
        model
            .timeline
            .items()
            .iter()
            .map(|i| i.id().into_owned())
            .collect::<Vec<_>>()
    );

    // 进度消息写入对应 tool_call.activities（内联渲染路径）
    let turn = model
        .chats
        .iter()
        .flat_map(|ch| ch.runs.iter())
        .find(|t| t.id == run_id);
    let activities = turn
        .and_then(|t| {
            t.tool_calls.iter().find(|c| {
                c.id.as_ref()
                    .is_some_and(|id| id.as_ref() == tool_id.to_string())
            })
        })
        .map(|c| c.activities.clone())
        .unwrap_or_default();
    assert!(
        activities.iter().any(|a| a.contains("analysing codebase")),
        "tool_call.activities should contain the progress message; activities = {activities:?}"
    );

    // 全 timeline 条目的 id 不重复（种类完整、无重）
    let ids: Vec<_> = model
        .timeline
        .items()
        .iter()
        .map(|i| i.id().into_owned())
        .collect();
    let unique_count = ids.iter().collect::<std::collections::HashSet<_>>().len();
    assert_eq!(
        ids.len(),
        unique_count,
        "timeline ids should be unique; ids = {ids:?}"
    );
}

#[test]
fn test_bash_streaming_preview_tails_complete_lines() {
    let mut model = ConversationModel::default();
    let chat_id = super::ids::ChatId::new("chat-bash-stream");
    let run_id = super::ids::ChatRunId::new("turn-bash-stream");
    let tool_id = super::ids::ToolCallId::new("tool-bash-stream");

    model.ensure_runtime_turn(chat_id.clone(), run_id.clone());
    model.apply(ToolCallStart {
        chat_id: chat_id.clone(),
        run_id: run_id.clone(),
        id: tool_id.clone(),
        provider_id: None,
        name: "Bash".to_string(),
        index: 0,
    });

    model.apply(RecordAgentProgress {
        chat_id: chat_id.clone(),
        run_id: run_id.clone(),
        tool_id: tool_id.clone(),
        message: "a\nb\nc\nd\ne\nf".to_string(),
    });

    let activities = tool_call(&model, &chat_id, &run_id, &tool_id)
        .map(|call| call.activities.clone())
        .unwrap_or_default();
    assert_eq!(activities, vec!["b", "c", "d", "e", "f"]);
}

#[test]
fn test_agent_progress_preview_limits_activity_lines() {
    let mut model = ConversationModel::default();
    let chat_id = super::ids::ChatId::new("chat-agent-stream");
    let run_id = super::ids::ChatRunId::new("turn-agent-stream");
    let tool_id = super::ids::ToolCallId::new("tool-agent-stream");

    model.ensure_runtime_turn(chat_id.clone(), run_id.clone());
    model.apply(ToolCallStart {
        chat_id: chat_id.clone(),
        run_id: run_id.clone(),
        id: tool_id.clone(),
        provider_id: None,
        name: "Agent".to_string(),
        index: 0,
    });

    model.apply(RecordAgentProgress {
        chat_id: chat_id.clone(),
        run_id: run_id.clone(),
        tool_id: tool_id.clone(),
        message: "one\ntwo\nthree\nfour\nfive\nsix".to_string(),
    });

    let activities = tool_call(&model, &chat_id, &run_id, &tool_id)
        .map(|call| call.activities.clone())
        .unwrap_or_default();
    assert_eq!(activities, vec!["two", "three", "four", "five", "six"]);
}

