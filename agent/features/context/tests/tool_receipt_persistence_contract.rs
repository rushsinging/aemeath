use context::domain::{
    CleanupConfirmation, SessionId, ToolCallIdentity, ToolCallReceipt, ToolCallState,
    ToolOutcomeKind, ToolReceiptMutation, ToolTerminalReceipt,
};
use sdk::{RunId, RunStepId};

fn identity() -> ToolCallIdentity {
    ToolCallIdentity {
        session_id: SessionId::new("session"),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        runtime_call_id: "call-1".to_string(),
        provider_call_id: Some("provider-1".to_string()),
        tool_name: "Glob".to_string(),
        call_index: 0,
        agent: false,
    }
}

#[test]
fn tool_receipt_wire_round_trips_running_and_terminal_details() {
    let running = ToolCallReceipt {
        identity: identity(),
        input_preview: "{\"pattern\":\"**/archify.mjs\"}".to_string(),
        state: ToolCallState::Running,
    };
    let encoded = serde_json::to_vec(&running).unwrap();
    assert_eq!(
        serde_json::from_slice::<ToolCallReceipt>(&encoded).unwrap(),
        running
    );

    let terminal = running
        .advance(ToolReceiptMutation::terminal(
            identity(),
            ToolTerminalReceipt::new(
                ToolOutcomeKind::CancellationUnconfirmed,
                "cleanup not confirmed",
                CleanupConfirmation::Unconfirmed,
            )
            .with_possible_side_effect("filesystem traversal may still be active")
            .with_unfinished_call("call-1"),
        ))
        .unwrap()
        .receipt;
    let encoded = serde_json::to_vec(&terminal).unwrap();
    assert_eq!(
        serde_json::from_slice::<ToolCallReceipt>(&encoded).unwrap(),
        terminal
    );
}
