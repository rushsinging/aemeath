use super::OutputViewAssembler;
use crate::tui::model::conversation::interaction::UiRunId;
use crate::tui::view_model::OutputBlockKind;
use crate::tui::view_state::RunActivityState;
use std::time::{Duration, Instant};

#[test]
fn model_silence_placeholder_is_transient_and_stable_within_interval() {
    let conversation = crate::tui::model::conversation::model::ConversationModel::default();
    let run_id = UiRunId::from("main-1");
    let now = Instant::now();
    let mut activity = RunActivityState::default();
    activity.sync_main_run(Some(&run_id), true, 0, 0, now);
    let revision = conversation.revision();
    let timeline_len = conversation.timeline.items().len();

    let before = OutputViewAssembler::assemble_from_conversation_with_activity(
        &conversation,
        &activity,
        now + Duration::from_millis(9_999),
        revision,
        None,
    );
    assert!(before.roots.is_empty());

    let at_boundary = OutputViewAssembler::assemble_from_conversation_with_activity(
        &conversation,
        &activity,
        now + Duration::from_secs(10),
        revision,
        None,
    );
    let block = at_boundary
        .roots
        .last()
        .expect("placeholder at ten seconds");
    let block_id = block.block_id.clone();
    assert!(matches!(
        &block.kind,
        OutputBlockKind::ThinkingMessage(view) if view.text == "Thinking."
    ));

    activity.advance_frame();
    let next_frame = OutputViewAssembler::assemble_from_conversation_with_activity(
        &conversation,
        &activity,
        now + Duration::from_secs(11),
        revision,
        None,
    );
    assert_eq!(next_frame.roots.last().unwrap().block_id, block_id);
    assert!(matches!(
        &next_frame.roots.last().unwrap().kind,
        OutputBlockKind::ThinkingMessage(view) if view.text == "Thinking.."
    ));
    assert_eq!(conversation.revision(), revision);
    assert_eq!(conversation.timeline.items().len(), timeline_len);
}

#[test]
fn real_activity_removes_placeholder_until_next_silence_boundary() {
    let conversation = crate::tui::model::conversation::model::ConversationModel::default();
    let run_id = UiRunId::from("main-1");
    let now = Instant::now();
    let mut activity = RunActivityState::default();
    activity.sync_main_run(Some(&run_id), true, 0, 0, now);
    assert!(activity.observe_main_model_activity(&run_id, now + Duration::from_secs(10)));

    let after_activity = OutputViewAssembler::assemble_from_conversation_with_activity(
        &conversation,
        &activity,
        now + Duration::from_secs(19),
        conversation.revision(),
        None,
    );
    assert!(after_activity.roots.is_empty());

    let silent_again = OutputViewAssembler::assemble_from_conversation_with_activity(
        &conversation,
        &activity,
        now + Duration::from_secs(20),
        conversation.revision(),
        None,
    );
    assert_eq!(silent_again.roots.len(), 1);
}
