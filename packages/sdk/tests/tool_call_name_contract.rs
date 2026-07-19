#[test]
fn tool_call_event_preserves_canonical_name() {
    let event = sdk::ChatEvent::ToolCallStart {
        context: sdk::ChatEventContext::new(
            sdk::ids::ChatId::new("chat-1"),
            sdk::ids::ChatTurnId::new("turn-1"),
        ),
        id: sdk::ids::ToolCallId::new("tool-1"),
        provider_id: Some("provider-1".to_string()),
        name: "Grep".to_string(),
        index: 0,
    };

    match event {
        sdk::ChatEvent::ToolCallStart { name, .. } => assert_eq!(name, "Grep"),
        other => panic!("unexpected event: {other:?}"),
    }
}
