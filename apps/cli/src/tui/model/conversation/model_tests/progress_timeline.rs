/// timeline 镜像验证：完整回合（user / assistant / tool-call / tool-result）后
/// timeline 应包含 UserMessage、AssistantText、ToolCall、ToolResult，
/// 且 AgentProgress **不进 timeline**（进度通过 tool_calls[].activities 内联渲染）。
#[test]
fn test_timeline_mirrors_blocks_no_agent_progress() {
    let mut model = ConversationModel::default();
    let chat_id = super::ids::ChatId::new("chat-a42");
    let turn_id = super::ids::ChatTurnId::new("turn-a42");
    let tool_id = super::ids::ToolCallId::new("tool-a42");

    // 1. 用户消息
    model.apply(StartChat {
        submission: "run task".to_string(),
    });

    // 2. Assistant text
    model.apply(AssistantText {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        text: "starting agent".to_string(),
    });
    model.apply(CompleteBlock {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
    });

    // 3. Tool call start
    model.apply(ToolCallStart {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        id: tool_id.clone(),
        provider_id: None,
        name: "Agent".to_string(),
        index: 0,
    });

    // 4. Agent progress — 不进 timeline，只写入 tool_calls[].activities
    model.apply(RecordAgentProgress {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        tool_id: tool_id.clone(),
        message: "analysing codebase".to_string(),
    });

    // 5. Tool result
    model.apply(ToolResult {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
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
        .flat_map(|ch| ch.turns.iter())
        .find(|t| t.id == turn_id);
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
    let turn_id = super::ids::ChatTurnId::new("turn-bash-stream");
    let tool_id = super::ids::ToolCallId::new("tool-bash-stream");

    model.ensure_runtime_turn(chat_id.clone(), turn_id.clone());
    model.apply(ToolCallStart {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        id: tool_id.clone(),
        provider_id: None,
        name: "Bash".to_string(),
        index: 0,
    });

    model.apply(RecordAgentProgress {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        tool_id: tool_id.clone(),
        message: "a\nb\nc\nd\ne\nf".to_string(),
    });

    let activities = tool_call(&model, &chat_id, &turn_id, &tool_id)
        .map(|call| call.activities.clone())
        .unwrap_or_default();
    assert_eq!(activities, vec!["b", "c", "d", "e", "f"]);
}

#[test]
fn test_agent_progress_preview_limits_activity_lines() {
    let mut model = ConversationModel::default();
    let chat_id = super::ids::ChatId::new("chat-agent-stream");
    let turn_id = super::ids::ChatTurnId::new("turn-agent-stream");
    let tool_id = super::ids::ToolCallId::new("tool-agent-stream");

    model.ensure_runtime_turn(chat_id.clone(), turn_id.clone());
    model.apply(ToolCallStart {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        id: tool_id.clone(),
        provider_id: None,
        name: "Agent".to_string(),
        index: 0,
    });

    model.apply(RecordAgentProgress {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        tool_id: tool_id.clone(),
        message: "one\ntwo\nthree\nfour\nfive\nsix".to_string(),
    });

    let activities = tool_call(&model, &chat_id, &turn_id, &tool_id)
        .map(|call| call.activities.clone())
        .unwrap_or_default();
    assert_eq!(activities, vec!["two", "three", "four", "five", "six"]);
}

