use super::*;
use std::path::PathBuf;

fn make_app() -> App {
    App::new(
        "sess-notice".to_string(),
        PathBuf::from("/tmp"),
        "test-model".to_string(),
    )
}

#[test]
fn test_append_system_notice_pushes_system_block() {
    let mut app = make_app();
    app.append_system_notice("你好");
    let has_system = app
        .model
        .conversation
        .timeline
        .items()
        .iter()
        .any(|item| matches!(item, crate::tui::model::output_timeline::OutputTimelineItem::System { text, .. } if text == "你好"));
    assert!(
        has_system,
        "系统消息应作为 System block 进入 ConversationModel"
    );
}

#[test]
fn test_append_error_notice_pushes_error_block() {
    let mut app = make_app();
    app.append_error_notice("出错了");
    let has_error =
        app.model.conversation.timeline.items().iter().any(
            |item| matches!(item, crate::tui::model::output_timeline::OutputTimelineItem::Error { text, .. } if text == "出错了"),
        );
    assert!(
        has_error,
        "错误消息应作为 Error block 进入 ConversationModel"
    );
}

#[test]
fn test_append_system_notice_renders_into_document() {
    let mut app = make_app();
    // 边界：banner 由 init() 写入 legacy lines，document 此时为空。
    // 派发系统消息后，document 必须经 ViewModel 派生出非空 block。
    app.append_system_notice("渲染检查");
    app.flush_dirty_view_models();
    let plain = app
        .output_area
        .document()
        .iter_lines()
        .map(|line| line.plain.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        plain.contains("渲染检查"),
        "系统消息应经 document 渲染出现在输出区，实际: {plain:?}"
    );
}

#[test]
fn test_append_user_echo_pushes_user_block_without_new_chat() {
    let mut app = make_app();
    app.model
        .conversation
        .apply(crate::tui::model::conversation::intent::StartChat {
            submission: "原始提问".to_string(),
        });
    let chats_before = app.model.conversation.chats.len();

    app.append_user_echo("我的答复");

    // 正常路径：回显作为 UserMessage 块进入模型，但不新开 chat。
    assert_eq!(
        app.model.conversation.chats.len(),
        chats_before,
        "回显不应新建 chat"
    );
    let has_user = app.model.conversation.timeline.items().iter().any(
        |item| matches!(item, crate::tui::model::output_timeline::OutputTimelineItem::UserMessage { text, .. } if text == "我的答复"),
    );
    assert!(
        has_user,
        "回显应作为 UserMessage block 进入 ConversationModel"
    );
}

#[test]
fn test_append_user_echo_renders_gt_prefix_into_document() {
    let mut app = make_app();
    app.append_user_echo("回显检查");
    app.flush_dirty_view_models();
    // `> ` marker 现由 gutter 注入到行首 span（plain 仅含内容）；断言渲染文档中
    // 存在「行首 gutter span == `> ` 且内容为回显文本」的行，验证回显仍带 `> ` 前缀。
    let has_echo = app.output_area.document().iter_lines().any(|line| {
        line.plain == "回显检查"
            && line
                .spans
                .first()
                .is_some_and(|s| s.content.as_ref() == "> ")
    });
    assert!(
        has_echo,
        "用户回显应以 gutter 注入的 \"> \" 前缀 span 渲染（plain 为内容原文）"
    );
}

#[test]
fn test_append_user_echo_empty_text_still_creates_block() {
    let mut app = make_app();
    let before = app.model.conversation.timeline.items().len();
    app.append_user_echo("");
    assert_eq!(
        app.model.conversation.timeline.items().len(),
        before + 1,
        "空回显文本仍应创建一个 UserMessage block"
    );
}

#[test]
fn test_append_error_notice_empty_text_still_creates_block() {
    let mut app = make_app();
    let before = app.model.conversation.timeline.items().len();
    app.append_error_notice("");
    assert_eq!(
        app.model.conversation.timeline.items().len(),
        before + 1,
        "空错误文本仍应创建一个 Error block"
    );
}

#[test]
fn test_enqueue_submission_echo_renders_queued_block_into_model() {
    // 正常路径：入队即时显示——派发后 QueuedUserMessage 块进入模型。
    // 渲染不再经 document block，改为 live-status projection。
    let mut app = make_app();
    app.enqueue_submission_echo(sdk::InputId::new_v7(), "排队中的输入");

    let has_queued = app.model.conversation.timeline.items().iter().any(|item| {
        matches!(item, crate::tui::model::output_timeline::OutputTimelineItem::QueuedUserMessage { text, .. } if text == "排队中的输入")
    });
    assert!(
        has_queued,
        "入队应作为 QueuedUserMessage block 进入 ConversationModel"
    );

    // queued_submission 不再出现在 document 中（已移至 live-status projection）。
    let plain = app
        .output_area
        .document()
        .iter_lines()
        .map(|line| line.plain.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !plain.contains(">"),
        "排队提交不应出现在 document 渲染中，实际: {plain:?}"
    );
}

#[test]
fn test_enqueue_submission_echo_refreshes_live_status_projection() {
    let mut app = make_app();
    app.enqueue_submission_echo(sdk::InputId::new_v7(), "排队中的输入");

    assert_eq!(
        app.live_status_view_model().queued_lines,
        vec!["> 排队中的输入"],
        "入队后应可从 live-status projection 派生排队输入"
    );
}

#[test]
fn test_enqueue_submission_echo_uses_display_text_for_copied_text() {
    let mut app = make_app();
    app.enqueue_submission_echo(sdk::InputId::new_v7(), "[Copied Text 1]");

    let has_queued = app.model.conversation.timeline.items().iter().any(|item| {
        matches!(item, crate::tui::model::output_timeline::OutputTimelineItem::QueuedUserMessage { text, .. } if text == "[Copied Text 1]")
    });
    assert!(has_queued, "排队区应显示折叠占位符");
    assert_eq!(
        app.live_status_view_model().queued_lines,
        vec!["> [Copied Text 1]"]
    );
}

#[test]
fn test_assistant_after_system_notice_uses_assistant_color() {
    // #74 回归端到端测试：System block（Muted 暗色）后追加 AssistantText block，
    // 验证 document 渲染中 assistant 行使用 ASSISTANT 色而非继承 System 的 Muted 色。

    use crate::tui::render::theme;

    let mut app = make_app();
    // 模拟 reflection 输出（System block）
    app.append_system_notice("reflection 输出内容");
    // 模拟后续 LLM 回复（Assistant block）
    app.model.conversation.apply(AssistantText {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("turn-1"),
        text: "后续回复".to_string(),
    });
    app.refresh_output_document_from_model();

    // 在 document 中找到包含"后续回复"的行
    let assistant_line = app
        .output_area
        .document()
        .iter_lines()
        .find(|line| line.plain.contains("后续回复"))
        .expect("应渲染 assistant 文本");

    let fg = assistant_line
        .spans
        .iter()
        .find(|s| s.content.as_ref().contains("后续回复"))
        .map(|s| s.style.fg)
        .expect("应找到 assistant span");

    assert_eq!(
        fg,
        Some(theme::ASSISTANT),
        "System block 后的 Assistant block 应使用 ASSISTANT 色 ({:?})，而非 Muted ({:?})",
        theme::ASSISTANT,
        theme::TEXT_MUTED
    );
}

#[test]
fn test_streaming_assistant_interrupted_by_system_uses_assistant_color() {
    // #74 场景：streaming assistant text 被 System notice 中断后，
    // 后续 streaming text 仍应使用 ASSISTANT 色。

    use crate::tui::render::theme;

    let mut app = make_app();
    // 模拟用户提问
    app.model.conversation.apply(StartChat {
        submission: "hello".to_string(),
    });
    // 模拟 LLM streaming
    app.model.conversation.apply(AssistantText {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("turn-1"),
        text: "你好".to_string(),
    });
    app.refresh_output_document_from_model();
    // 模拟 System notice 中断（如自动 reflection）
    app.append_system_notice("[reflection: ...]");
    app.flush_dirty_view_models();
    // 模拟 LLM streaming 继续
    app.model.conversation.apply(AssistantText {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("turn-1"),
        text: "世界".to_string(),
    });
    app.refresh_output_document_from_model();

    // 验证"你好"和"世界"都在 document 中且使用 ASSISTANT 色
    for needle in &["你好", "世界"] {
        let line = app
            .output_area
            .document()
            .iter_lines()
            .find(|line| line.plain.contains(needle))
            .unwrap_or_else(|| panic!("应渲染文本: {needle}"));
        let fg = line
            .spans
            .iter()
            .find(|s| s.content.as_ref().contains(needle))
            .map(|s| s.style.fg)
            .unwrap_or_else(|| panic!("应找到 span: {needle}"));
        assert_eq!(
            fg,
            Some(theme::ASSISTANT),
            "\"{needle}\" 应使用 ASSISTANT 色，实际: {fg:?}"
        );
    }
}

#[test]
fn test_spinner_tick_idle_does_not_mark_output_dirty() {
    use crate::tui::app::event::UiEvent;
    use crate::tui::effect::session::processing::SpawnContextRefs;
    use crate::tui::update::msg::TuiMsg;
    let mut app = make_app();
    app.model.conversation.spinner.phase = None; // idle / 已完成
    app.view_state.dirty.clear_output();
    let (ui_tx, _ui_rx) = tokio::sync::mpsc::channel::<UiEvent>(8);
    let spawn_refs = SpawnContextRefs { agent_client: None };
    app.update(TuiMsg::SpinnerTick, &ui_tx, &spawn_refs);
    assert!(
        !app.view_state.dirty.output,
        "idle 时 SpinnerTick 不应标脏 output（否则空闲态每 90ms 全量重建整会话）"
    );
}

#[test]
fn test_spinner_tick_active_marks_output_dirty() {
    use crate::tui::app::event::UiEvent;
    use crate::tui::effect::session::processing::SpawnContextRefs;
    use crate::tui::update::msg::TuiMsg;
    let mut app = make_app();
    app.model.conversation.spinner.chat_active = true;
    app.model.conversation.spinner.phase =
        Some(crate::tui::model::conversation::spinner::SpinnerPhase::Thinking); // 处理中，需要 gutter 动画
    app.view_state.dirty.clear_output();
    let (ui_tx, _ui_rx) = tokio::sync::mpsc::channel::<UiEvent>(8);
    let spawn_refs = SpawnContextRefs { agent_client: None };
    app.update(TuiMsg::SpinnerTick, &ui_tx, &spawn_refs);
    assert!(
        app.view_state.dirty.output,
        "active 时 SpinnerTick 应标脏 output，以驱动运行中 tool 的 gutter 动画"
    );
}

#[test]
fn test_refresh_skips_assemble_when_revision_unchanged() {
    let mut app = make_app();
    app.append_system_notice("一条消息"); // 产生 change，revision 前进
    app.refresh_output_document_from_model();
    let after_first = app.assemble_count;
    // conversation 未变，再次 refresh：应命中 memo，不重新 assemble。
    app.refresh_output_document_from_model();
    assert_eq!(
        app.assemble_count, after_first,
        "conversation 未变时 refresh 应复用 view_model，不重新 assemble"
    );
}

#[test]
fn test_refresh_assembles_again_after_conversation_mutates() {
    let mut app = make_app();
    app.append_system_notice("第一条");
    app.refresh_output_document_from_model();
    let after_first = app.assemble_count;
    app.append_system_notice("第二条"); // conversation 变化 → revision 前进
    app.refresh_output_document_from_model();
    assert_eq!(
        app.assemble_count,
        after_first + 1,
        "conversation 变化后 refresh 应以新 revision 重新 assemble"
    );
}

#[test]
fn test_refresh_assembles_again_when_workspace_root_changes() {
    // Fix 1 回归：/worktree enter 改变 workspace_root 时，revision 不变，
    // 但 memo 的 key 应包含 workspace_root，使工具路径显示刷新。
    let mut app = make_app();
    app.append_system_notice("一条消息");
    app.refresh_output_document_from_model();
    let after_first = app.assemble_count;

    // conversation 不变（revision 不推进），只改 workspace_root。
    app.model.conversation.workspace.workspace_root = Some("/new/root".to_string());
    app.refresh_output_document_from_model();

    assert_eq!(
        app.assemble_count,
        after_first + 1,
        "workspace_root 变化时应触发重新 assemble，即使 revision 未变"
    );
}
