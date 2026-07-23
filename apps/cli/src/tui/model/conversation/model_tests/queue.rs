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

