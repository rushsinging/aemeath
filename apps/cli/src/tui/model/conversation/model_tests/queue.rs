#[test]
fn test_queue_submission_pushes_queued_user_message_block() {
    // 正常路径：排队提交经 ConversationModel 进入 QueuedUserMessage 块（取代旧
    // OutputArea::queued_messages 命令式显示路径）。
    let mut model = ConversationModel::default();
    let changes = model.apply(QueueSubmission {
        input_id: "queue-1".to_string(),
        text: "排队的消息".to_string(),
    });

    assert!(changes
        .iter()
        .any(|c| matches!(c, ConversationChange::QueuedSubmissionAdded { .. })));
    assert!(model.timeline.items().iter().any(|item| matches!(
        item,
        OutputTimelineItem::QueuedUserMessage { text, .. } if text == "排队的消息"
    )));
    assert_eq!(model.queued_submissions.len(), 1);
}

#[test]
fn test_clear_queued_by_id_removes_only_matching_entry() {
    // 入队 3 条占位（A/B/C），按 B 的 input_id 精确清除后，
    // queued_submissions / blocks / timeline 三处各只剩 A 和 C。
    let mut model = ConversationModel::default();
    let id_a = "input-a".to_string();
    let id_b = "input-b".to_string();
    let id_c = "input-c".to_string();

    model.apply(QueueSubmission {
        input_id: id_a.clone(),
        text: "A".to_string(),
    });
    model.apply(QueueSubmission {
        input_id: id_b.clone(),
        text: "B".to_string(),
    });
    model.apply(QueueSubmission {
        input_id: id_c.clone(),
        text: "C".to_string(),
    });

    let changes = model.apply(ClearQueuedSubmissionById {
        input_id: id_b.clone(),
    });

    // 只移除了 1 条
    assert!(changes.iter().any(|c| matches!(
        c,
        ConversationChange::QueuedSubmissionsCleared { count } if *count == 1
    )));

    // queued_submissions：剩 A、C，无 B
    assert_eq!(model.queued_submissions.len(), 2);
    assert!(model.queued_submissions.iter().any(|q| q.input_id == id_a));
    assert!(model.queued_submissions.iter().any(|q| q.input_id == id_c));
    assert!(!model.queued_submissions.iter().any(|q| q.input_id == id_b));

    // timeline：剩 A、C 的 QueuedUserMessage，无 B
    let queued_timeline: Vec<_> = model
        .timeline
        .items()
        .iter()
        .filter_map(|it| match it {
            OutputTimelineItem::QueuedUserMessage { input_id, text, .. } => {
                Some((input_id.clone(), text.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(queued_timeline.len(), 2);
    assert!(queued_timeline.iter().any(|(iid, _)| iid == &id_a));
    assert!(queued_timeline.iter().any(|(iid, _)| iid == &id_c));
    assert!(!queued_timeline.iter().any(|(iid, _)| iid == &id_b));
}


/// #1816：命令队列全量快照整列替换，命令不进输出时间线。
#[test]
fn test_sync_queued_commands_replaces_snapshot_without_timeline_entries() {
    let mut model = ConversationModel::default();
    let changes = model.apply(SyncQueuedCommands {
        queued: vec![
            (UiQueuedInputId::from("01920000-0000-7000-8000-000000000001"), "/compact".to_string()),
            (
                UiQueuedInputId::from("01920000-0000-7000-8000-000000000002"),
                "/model anthropic/claude".to_string(),
            ),
        ],
    });

    assert!(changes
        .iter()
        .any(|change| matches!(change, ConversationChange::QueuedCommandsSynced { count: 2 })));
    assert_eq!(model.queued_commands.len(), 2);
    assert_eq!(model.queued_commands[0].text, "/compact");
    assert!(!model.timeline.items().iter().any(|item| matches!(
        item,
        OutputTimelineItem::QueuedUserMessage { text, .. } if text == "/compact"
    )));

    // 快照是全量语义：空快照清空命令占位
    model.apply(SyncQueuedCommands { queued: vec![] });
    assert!(model.queued_commands.is_empty());
}

/// #1816：撤回消息（ClearAllQueuedSubmissions）不误清命令占位。
#[test]
fn test_clear_all_queued_submissions_keeps_queued_commands() {
    let mut model = ConversationModel::default();
    model.apply(SyncQueuedCommands {
        queued: vec![(
            UiQueuedInputId::from("01920000-0000-7000-8000-000000000001"),
            "/compact".to_string(),
        )],
    });
    model.apply(QueueSubmission {
        input_id: "input-a".to_string(),
        text: "排队的消息".to_string(),
    });

    model.apply(ClearAllQueuedSubmissions);

    assert!(model.queued_submissions.is_empty());
    assert_eq!(model.queued_commands.len(), 1, "命令占位有独立生命周期");
}

/// #1818 回归（无 TUI 逻辑变更）：runtime 同批折叠后快照只含 1 条，
/// TUI 的全量替换路径据此把 2 组排队行收敛为 1 组。
#[test]
fn test_queued_snapshot_with_single_merged_message_renders_one_queue_group() {
    let mut model = ConversationModel::default();
    // 先乐观占位两条（提交瞬间的本地回显）。
    model.apply(QueueSubmission {
        input_id: "input-a".to_string(),
        text: "第一段".to_string(),
    });
    model.apply(QueueSubmission {
        input_id: "input-b".to_string(),
        text: "第二段".to_string(),
    });
    assert_eq!(model.queued_submissions.len(), 2);

    // runtime 权威快照：同批已折叠为一条。
    model.apply(SyncQueuedSubmissions {
        queued: vec![TuiChatMessage {
            role: "user".to_string(),
            content: vec![TuiContentBlock::text("第一段\n\n第二段")],
            input_id: Some("input-a".to_string()),
            source: TuiMessageSource::User,
            hook_notice: None,
            skill_request: None,
        }],
    });

    assert_eq!(
        model.queued_submissions.len(),
        1,
        "全量替换后只应剩 runtime 快照里的那一条"
    );
    assert_eq!(model.queued_submissions[0].text, "第一段\n\n第二段");
}
