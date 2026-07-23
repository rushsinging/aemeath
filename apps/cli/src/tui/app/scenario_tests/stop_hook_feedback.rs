use crate::tui::adapter::agent_event::map_agent_event;
use crate::tui::adapter::runtime_view::{
    TuiChatMessage, TuiContentBlock, TuiMessageSource, TuiStopHookFeedback,
};
use crate::tui::app::event::UiEvent;
use crate::tui::model::conversation::intent::ConversationIntent;

#[test]
fn stop_hook_feedback_scenario_projects_one_structured_notice() {
    let messages = vec![
        TuiChatMessage::user_text("finish work"),
        TuiChatMessage {
            role: "user".to_string(),
            content: vec![TuiContentBlock::text(
                "<system-reminder>Stop hook prevented stopping.</system-reminder>",
            )],
            input_id: None,
            source: TuiMessageSource::StopHook,
            stop_hook: Some(TuiStopHookFeedback {
                summary: "Stop hook prevented stopping.".to_string(),
                command: ".agents/hooks/check-agent-stop.sh".to_string(),
                exit_code: Some(2),
                reason: "temporary failure".to_string(),
                stdout_preview: "first failure".to_string(),
                stderr_preview: "BLOCKED".to_string(),
                stdout_truncated: false,
                stderr_truncated: false,
                output_file: None,
            }),
        },
    ];

    let mapping = map_agent_event(&UiEvent::StopHookBlocked { messages });

    assert!(matches!(
        mapping.conversation.as_slice(),
        [ConversationIntent::AppendHookNotice(notice)]
            if notice.content.body == "Stop hook prevented stopping."
                && notice.content.details.as_deref().is_some_and(|details|
                    details.contains("Command: .agents/hooks/check-agent-stop.sh")
                        && details.contains("Exit code: 2")
                )
    ));
}
