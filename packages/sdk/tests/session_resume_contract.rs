use sdk::{ChatMessage, ResumedSessionStep};

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
