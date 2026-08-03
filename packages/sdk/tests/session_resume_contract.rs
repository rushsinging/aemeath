use sdk::{ChatMessage, ResumedSessionStep};
use std::sync::Arc;

#[test]
fn display_history_window_round_trip_preserves_requested_steps() {
    let window = sdk::DisplayHistoryWindow {
        session_id: "session-window".to_string(),
        generation_revision: 21,
        steps: vec![ResumedSessionStep {
            run_id: "run-window".to_string(),
            step_id: "step-window".to_string(),
            messages: vec![ChatMessage::user_text("window body")],
            finalize_cause: Some(sdk::ResumedStepFinalizeCause::Completed),
            duration_ms: Some(77),
        }],
    };

    let encoded = serde_json::to_value(&window).expect("serialize display history window");
    let decoded: sdk::DisplayHistoryWindow =
        serde_json::from_value(encoded).expect("deserialize display history window");

    assert_eq!(decoded.session_id, "session-window");
    assert_eq!(decoded.generation_revision, 21);
    assert_eq!(decoded.steps[0].messages[0].text_content(), "window body");
    assert_eq!(decoded.steps[0].duration_ms, Some(77));
}

#[test]
fn display_history_index_round_trip_contains_no_message_bodies() {
    let index = sdk::DisplayHistoryIndex {
        session_id: "session-index".to_string(),
        generation_revision: 17,
        steps: vec![sdk::DisplayHistoryStepReference {
            run_id: "run-1".to_string(),
            step_id: "step-1".to_string(),
            member_name: "step-run-step.json".to_string(),
            estimated_lines: 23,
            user_input_history: vec!["historical input".to_string()],
            finalize_cause: Some(sdk::ResumedStepFinalizeCause::Completed),
            duration_ms: Some(42),
        }],
    };

    let encoded = serde_json::to_value(&index).expect("serialize history index");
    let decoded: sdk::DisplayHistoryIndex =
        serde_json::from_value(encoded.clone()).expect("deserialize history index");

    assert_eq!(decoded.session_id, "session-index");
    assert_eq!(decoded.generation_revision, 17);
    assert_eq!(decoded.steps[0].estimated_lines, 23);
    assert_eq!(decoded.steps[0].duration_ms, Some(42));
    let json = encoded.to_string();
    assert!(!json.contains("messages"));
    assert!(!json.contains("message_segments"));
    assert!(!json.contains("tool_receipts"));
}

#[test]
fn local_resume_backing_clone_reuses_shared_step_messages() {
    let shared_messages: Arc<[share::message::Message]> =
        vec![share::message::Message::user("large history")].into();
    let step = sdk::LocalResumedSessionStep {
        run_id: "run-shared".to_string(),
        step_id: "step-shared".to_string(),
        message_segments: vec![Arc::clone(&shared_messages)],
        finalize_cause: Some(sdk::ResumedStepFinalizeCause::Completed),
        duration_ms: Some(42),
    };
    let backing = sdk::LocalSessionResumeBacking {
        steps: vec![step],
        display_history: None,
        session_id: "session-shared".to_string(),
        created_at: 42,
        compacted: true,
    };

    let cloned = backing.clone();

    assert!(Arc::ptr_eq(
        &shared_messages,
        &cloned.steps[0].message_segments[0]
    ));
    assert_eq!(cloned.steps[0].messages().count(), 1);
    assert!(cloned.compacted);
}

#[test]
fn resumed_session_step_round_trip_preserves_run_step_boundaries() {
    let step = ResumedSessionStep {
        run_id: "run-1".to_string(),
        step_id: "step-1".to_string(),
        messages: vec![ChatMessage::user_text("hello")],
        finalize_cause: Some(sdk::ResumedStepFinalizeCause::UserCancelledStep),
        duration_ms: Some(7_325_000),
    };

    let encoded = serde_json::to_value(&step).expect("serialize resume step");
    let decoded: ResumedSessionStep =
        serde_json::from_value(encoded).expect("deserialize resume step");

    assert_eq!(decoded.run_id, "run-1");
    assert_eq!(decoded.step_id, "step-1");
    assert_eq!(decoded.messages[0].text_content(), "hello");
    assert_eq!(
        decoded.finalize_cause,
        Some(sdk::ResumedStepFinalizeCause::UserCancelledStep)
    );
    assert_eq!(decoded.duration_ms, Some(7_325_000));
}

#[test]
fn resumed_session_step_round_trip_preserves_reconstructed_tool_pair() {
    let step = ResumedSessionStep {
        run_id: "run-terminated".to_string(),
        step_id: "step-running-tool".to_string(),
        messages: vec![
            ChatMessage {
                role: "assistant".to_string(),
                content: vec![sdk::ContentBlock::ToolUse {
                    id: "provider-call-1".to_string(),
                    name: "Bash".to_string(),
                    input: serde_json::json!({"command": "sleep 180"}),
                }],
                metadata: None,
                input_id: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: vec![sdk::ContentBlock::ToolResult {
                    tool_use_id: "provider-call-1".to_string(),
                    content: serde_json::json!({"outcome": "CancellationUnconfirmed"}),
                    is_error: true,
                    text: Some("cleanup could not be confirmed".to_string()),
                }],
                metadata: None,
                input_id: None,
            },
        ],
        finalize_cause: Some(sdk::ResumedStepFinalizeCause::RunTerminated),
        duration_ms: None,
    };

    let encoded = serde_json::to_value(&step).expect("serialize reconstructed resume step");
    let decoded: ResumedSessionStep =
        serde_json::from_value(encoded).expect("deserialize reconstructed resume step");

    assert_eq!(
        decoded.finalize_cause,
        Some(sdk::ResumedStepFinalizeCause::RunTerminated)
    );
    assert!(matches!(
        &decoded.messages[0].content[0],
        sdk::ContentBlock::ToolUse { id, name, .. }
            if id == "provider-call-1" && name == "Bash"
    ));
    assert!(matches!(
        &decoded.messages[1].content[0],
        sdk::ContentBlock::ToolResult { tool_use_id, is_error: true, .. }
            if tool_use_id == "provider-call-1"
    ));
}
