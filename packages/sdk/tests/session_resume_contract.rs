use sdk::{ChatMessage, ResumedSessionStep};

#[test]
fn resumed_session_step_round_trip_preserves_run_step_boundaries() {
    let step = ResumedSessionStep {
        run_id: "run-1".to_string(),
        step_id: "step-1".to_string(),
        messages: vec![ChatMessage::user_text("hello")],
    };

    let encoded = serde_json::to_value(&step).expect("serialize resume step");
    let decoded: ResumedSessionStep =
        serde_json::from_value(encoded).expect("deserialize resume step");

    assert_eq!(decoded.run_id, "run-1");
    assert_eq!(decoded.step_id, "step-1");
    assert_eq!(decoded.messages[0].text_content(), "hello");
}
