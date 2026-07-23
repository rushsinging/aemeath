use super::*;
use crate::tui::model::conversation::block::AskUserSlot;
use crate::tui::model::conversation::intent::*;

fn make_slot(id: &str, question: &str, options: &[&str]) -> AskUserSlot {
    let llm_count = options.len();
    let mut all = options
        .iter()
        .map(|s| sdk::OptionItem::title_only(s.to_string()))
        .collect::<Vec<_>>();
    if !all.is_empty() {
        all.push(sdk::OptionItem::title_only("Type something...".to_string()));
    }
    AskUserSlot {
        id: id.to_string(),
        question: question.to_string(),
        options: all,
        llm_option_count: llm_count,
        multi_select: false,
        default: None,
        answer: None,
    }
}

fn show_batch(model: &mut ConversationModel, slots: Vec<AskUserSlot>) {
    model.apply(ShowAskUserBatch { slots });
}

fn timeline_item(model: &ConversationModel) -> &OutputTimelineItem {
    model
        .timeline
        .items()
        .iter()
        .find(|i| matches!(i, OutputTimelineItem::AskUserBatch { .. }))
        .expect("ask user batch timeline item")
}

#[test]
fn test_show_ask_user_batch_initializes_answering_phase() {
    let mut model = ConversationModel::default();
    show_batch(&mut model, vec![make_slot("q1", "问题1", &["A", "B"])]);
    if let OutputTimelineItem::AskUserBatch {
        phase,
        active_index,
        ..
    } = timeline_item(&model)
    {
        assert_eq!(*phase, AskUserPhase::Answering);
        assert_eq!(*active_index, 0);
    }
}

#[test]
fn test_answer_current_advances_to_next_question() {
    let mut model = ConversationModel::default();
    show_batch(
        &mut model,
        vec![
            make_slot("q1", "问题1", &["A"]),
            make_slot("q2", "问题2", &["B"]),
        ],
    );
    model.apply(AnswerCurrentAskUser {
        answer: "A".to_string(),
    });
    if let OutputTimelineItem::AskUserBatch {
        active_index,
        phase,
        slots,
        ..
    } = timeline_item(&model)
    {
        assert_eq!(*active_index, 1);
        assert_eq!(*phase, AskUserPhase::Answering);
        assert_eq!(slots[0].answer.as_deref(), Some("A"));
    }
}

#[test]
fn test_answer_last_question_enters_confirming_phase() {
    let mut model = ConversationModel::default();
    show_batch(
        &mut model,
        vec![
            make_slot("q1", "问题1", &["A"]),
            make_slot("q2", "问题2", &["B"]),
        ],
    );
    model.apply(AnswerCurrentAskUser {
        answer: "A".to_string(),
    });
    model.apply(AnswerCurrentAskUser {
        answer: "B".to_string(),
    });
    if let OutputTimelineItem::AskUserBatch {
        phase,
        confirm_cursor,
        slots,
        ..
    } = timeline_item(&model)
    {
        assert_eq!(*phase, AskUserPhase::Confirming);
        assert_eq!(*confirm_cursor, 2); // 默认在「提交」
        assert_eq!(slots[0].answer.as_deref(), Some("A"));
        assert_eq!(slots[1].answer.as_deref(), Some("B"));
    }
}

#[test]
fn test_confirm_sets_confirmed_flag() {
    let mut model = ConversationModel::default();
    show_batch(&mut model, vec![make_slot("q1", "问题1", &["A"])]);
    model.apply(AnswerCurrentAskUser {
        answer: "A".to_string(),
    });
    model.apply(ConfirmAskUserBatch);
    if let OutputTimelineItem::AskUserBatch { confirmed, .. } = timeline_item(&model) {
        assert!(*confirmed);
    }
}

#[test]
fn test_single_question_batch_answer_confirmed_immediately() {
    let mut model = ConversationModel::default();
    show_batch(&mut model, vec![make_slot("q1", "问题1", &["A"])]);
    model.apply(AnswerCurrentAskUser {
        answer: "A".to_string(),
    });
    if let OutputTimelineItem::AskUserBatch {
        confirmed, phase, ..
    } = timeline_item(&model)
    {
        assert!(*confirmed);
        assert_eq!(*phase, AskUserPhase::Answering); // phase 不变，直接 confirmed
    }
}

#[test]
fn test_single_question_batch_answer_no_options_confirmed_immediately() {
    let mut model = ConversationModel::default();
    show_batch(&mut model, vec![make_slot("q1", "问题1", &[])]);
    model.apply(AnswerCurrentAskUser {
        answer: "自由输入".to_string(),
    });
    if let OutputTimelineItem::AskUserBatch { confirmed, .. } = timeline_item(&model) {
        assert!(*confirmed);
    }
}

#[test]
fn test_navigate_ask_user_to_resets_cursor_and_selected() {
    let mut model = ConversationModel::default();
    show_batch(
        &mut model,
        vec![
            make_slot("q1", "问题1", &["A", "B"]),
            make_slot("q2", "问题2", &["C"]),
        ],
    );
    // 先答完两题进入确认页
    model.apply(AnswerCurrentAskUser {
        answer: "A".to_string(),
    });
    model.apply(AnswerCurrentAskUser {
        answer: "C".to_string(),
    });
    // 导航回第 0 题重新作答
    model.apply(NavigateAskUserTo { index: 0 });
    if let OutputTimelineItem::AskUserBatch {
        active_index,
        phase,
        cursor,
        chat_input_active,
        ..
    } = timeline_item(&model)
    {
        assert_eq!(*active_index, 0);
        assert_eq!(*phase, AskUserPhase::Answering);
        assert_eq!(*cursor, 0);
        assert!(!*chat_input_active);
    }
}

#[test]
fn test_set_cursor_without_batch_is_noop() {
    let mut model = ConversationModel::default();
    let changes = model.apply(SetAskUserCursor { cursor: 0 });
    assert!(changes.is_empty());
}

#[test]
fn test_dismiss_ask_user_batch_removes_block() {
    let mut model = ConversationModel::default();
    show_batch(&mut model, vec![make_slot("q1", "问题1", &["A"])]);
    let changes = model.apply(DismissAskUserBatch);
    assert!(changes
        .iter()
        .any(|c| matches!(c, ConversationChange::AskUserDismissed)));
    assert!(!model.timeline.items().iter().any(|b| matches!(
        b,
        crate::tui::model::output_timeline::OutputTimelineItem::AskUserBatch { .. }
    )));
}

// ── chat_input cursor 回归测试 ──

fn enable_chat_input(model: &mut ConversationModel) {
    model.apply(SetAskUserChatInput { active: true });
}

#[test]
fn test_chat_input_cursor_insert_and_backspace_at_cursor() {
    let mut model = ConversationModel::default();
    show_batch(&mut model, vec![make_slot("q1", "问题1", &[])]);
    enable_chat_input(&mut model);

    // 输入 "abc"
    model.apply(AppendAskUserChatChar { ch: 'a' });
    model.apply(AppendAskUserChatChar { ch: 'b' });
    model.apply(AppendAskUserChatChar { ch: 'c' });
    if let OutputTimelineItem::AskUserBatch {
        chat_input_text,
        chat_input_cursor,
        ..
    } = timeline_item(&model)
    {
        assert_eq!(*chat_input_text, "abc");
        assert_eq!(*chat_input_cursor, 3);
    }

    // 左移到 1，再插入 X 应该是 aXbc
    model.apply(MoveAskUserChatCursor { delta: -2 });
    model.apply(AppendAskUserChatChar { ch: 'X' });
    if let OutputTimelineItem::AskUserBatch {
        chat_input_text,
        chat_input_cursor,
        ..
    } = timeline_item(&model)
    {
        assert_eq!(*chat_input_text, "aXbc");
        assert_eq!(*chat_input_cursor, 2);
    }

    // 在 cursor=2 位置 backspace 删除 X
    model.apply(DeleteAskUserChatChar);
    if let OutputTimelineItem::AskUserBatch {
        chat_input_text,
        chat_input_cursor,
        ..
    } = timeline_item(&model)
    {
        assert_eq!(*chat_input_text, "abc");
        assert_eq!(*chat_input_cursor, 1);
    }
}

#[test]
fn test_chat_input_cursor_move_home_end_word_delete() {
    let mut model = ConversationModel::default();
    show_batch(&mut model, vec![make_slot("q1", "问题1", &[])]);
    enable_chat_input(&mut model);

    // 输入 "hello world"
    for ch in "hello world".chars() {
        model.apply(AppendAskUserChatChar { ch });
    }
    // Home (cursor -> 0)
    model.apply(MoveAskUserChatCursorEnd { to_end: false });
    if let OutputTimelineItem::AskUserBatch {
        chat_input_cursor, ..
    } = timeline_item(&model)
    {
        assert_eq!(*chat_input_cursor, 0);
    }
    // Right 2 次
    model.apply(MoveAskUserChatCursor { delta: 1 });
    model.apply(MoveAskUserChatCursor { delta: 1 });
    if let OutputTimelineItem::AskUserBatch {
        chat_input_cursor, ..
    } = timeline_item(&model)
    {
        assert_eq!(*chat_input_cursor, 2);
    }
    // End (cursor -> 11)
    model.apply(MoveAskUserChatCursorEnd { to_end: true });
    if let OutputTimelineItem::AskUserBatch {
        chat_input_cursor, ..
    } = timeline_item(&model)
    {
        assert_eq!(*chat_input_cursor, "hello world".len());
    }
    // Ctrl+W 删除 "world"
    model.apply(DeleteAskUserChatWord);
    if let OutputTimelineItem::AskUserBatch {
        chat_input_text,
        chat_input_cursor,
        ..
    } = timeline_item(&model)
    {
        assert_eq!(*chat_input_text, "hello ");
        assert_eq!(*chat_input_cursor, "hello ".len());
    }
}

#[test]
fn test_chat_input_cursor_unicode_char_boundary() {
    let mut model = ConversationModel::default();
    show_batch(&mut model, vec![make_slot("q1", "问题1", &[])]);
    enable_chat_input(&mut model);
    // 输入中文 "你好"
    model.apply(AppendAskUserChatChar { ch: '你' });
    model.apply(AppendAskUserChatChar { ch: '好' });
    // 左移一个 char (cursor 从 6 -> 3)
    model.apply(MoveAskUserChatCursor { delta: -1 });
    if let OutputTimelineItem::AskUserBatch {
        chat_input_cursor, ..
    } = timeline_item(&model)
    {
        assert_eq!(*chat_input_cursor, 3); // '你' 占 3 字节
    }
    // 再右移一个 char (cursor 从 3 -> 6)
    model.apply(MoveAskUserChatCursor { delta: 1 });
    if let OutputTimelineItem::AskUserBatch {
        chat_input_cursor, ..
    } = timeline_item(&model)
    {
        assert_eq!(*chat_input_cursor, 6);
    }
}

#[test]
fn show_no_option_batch_activates_chat_input() {
    let mut model = ConversationModel::default();
    show_batch(&mut model, vec![make_slot("q1", "问题1", &[])]);

    assert!(
        model
            .ask_user_snapshot()
            .expect("active Ask batch")
            .chat_input_active
    );
}

#[test]
fn show_batch_uses_first_slot_id_for_stable_identity() {
    let mut model = ConversationModel::default();
    show_batch(
        &mut model,
        vec![
            make_slot("first-tool-call", "问题1", &["A"]),
            make_slot("second-tool-call", "问题2", &["B"]),
        ],
    );

    let OutputTimelineItem::AskUserBatch { id, .. } = timeline_item(&model) else {
        panic!("Ask batch expected");
    };
    assert_eq!(id, "ask-user-first-tool-call");
}

#[test]
fn show_new_batch_preserves_confirmed_batches_and_replaces_only_active_batch() {
    let mut model = ConversationModel::default();
    model.restore_answered_ask_user_batch(vec![make_slot("done", "已完成", &[])]);
    show_batch(&mut model, vec![make_slot("active-1", "待答1", &["A"])]);
    show_batch(&mut model, vec![make_slot("active-2", "待答2", &["B"])]);

    let asks = model
        .timeline
        .items()
        .iter()
        .filter_map(|item| match item {
            OutputTimelineItem::AskUserBatch { id, confirmed, .. } => Some((id, confirmed)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(asks.len(), 2);
    assert_eq!(asks[0].0, "ask-user-done");
    assert!(*asks[0].1);
    assert_eq!(asks[1].0, "ask-user-active-2");
    assert!(!*asks[1].1);
}

#[test]
fn ask_user_snapshot_targets_active_unconfirmed_batch() {
    let mut model = ConversationModel::default();
    model.restore_answered_ask_user_batch(vec![make_slot("done", "已完成", &[])]);
    show_batch(&mut model, vec![make_slot("active", "待答", &["A"])]);

    assert!(
        !model
            .ask_user_snapshot()
            .expect("active Ask batch")
            .confirmed
    );
}

#[test]
fn answering_next_no_option_slot_activates_chat_input() {
    let mut model = ConversationModel::default();
    show_batch(
        &mut model,
        vec![
            make_slot("q1", "问题1", &["A"]),
            make_slot("q2", "问题2", &[]),
        ],
    );
    model.apply(AnswerCurrentAskUser {
        answer: "A".to_string(),
    });

    assert!(
        model
            .ask_user_snapshot()
            .expect("active Ask batch")
            .chat_input_active
    );
}

#[test]
fn navigate_back_to_no_option_slot_activates_chat_input() {
    let mut model = ConversationModel::default();
    show_batch(
        &mut model,
        vec![
            make_slot("q1", "问题1", &[]),
            make_slot("q2", "问题2", &["B"]),
        ],
    );
    model.apply(AnswerCurrentAskUser {
        answer: "自由输入".to_string(),
    });
    model.apply(NavigateAskUserTo { index: 0 });

    assert!(
        model
            .ask_user_snapshot()
            .expect("active Ask batch")
            .chat_input_active
    );
}

#[test]
fn restore_answered_batch_appends_confirmed_history() {
    let mut model = ConversationModel::default();
    model.restore_answered_ask_user_batch(vec![make_slot("first", "问题1", &[])]);
    model.restore_answered_ask_user_batch(vec![make_slot("second", "问题2", &[])]);

    let ids = model
        .timeline
        .items()
        .iter()
        .filter_map(|item| match item {
            OutputTimelineItem::AskUserBatch { id, confirmed, .. } if *confirmed => Some(id),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["ask-user-first", "ask-user-second"]);
}
