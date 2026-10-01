use super::*;

#[test]
fn skill_request_preserves_identity_and_raw_arguments() {
    let input_id = crate::InputId::new_v7();
    let event = ChatInputEvent::SkillRequest(SkillRequest {
        input_id: input_id.clone(),
        skill: "release".to_string(),
        arguments: "v1.2.3 --dry-run".to_string(),
        raw_input: "/release v1.2.3 --dry-run".to_string(),
    });
    match event {
        ChatInputEvent::SkillRequest(request) => {
            assert_eq!(request.input_id, input_id);
            assert_eq!(request.skill, "release");
            assert_eq!(request.arguments, "v1.2.3 --dry-run");
            assert_eq!(request.raw_input, "/release v1.2.3 --dry-run");
        }
        other => panic!("expected SkillRequest, got {other:?}"),
    }
}

#[derive(Default)]
struct ClosedInputPort;

impl crate::ChatInputEventPort for ClosedInputPort {
    fn drain_input_events<'a>(&'a self) -> crate::InputEventFuture<'a> {
        Box::pin(async { Vec::new() })
    }

    fn recv_next<'a>(&'a self) -> crate::InputEventOptFuture<'a> {
        Box::pin(async { None })
    }
}

#[test]
fn chat_request_requires_single_typed_ingress() {
    let request = ChatRequest {
        ingress: std::sync::Arc::new(ClosedInputPort),
    };

    let _ingress = request.ingress;
}

#[test]
fn test_chat_input_event_classify_text_user_message() {
    let img = crate::ChatInputImage {
        id: "[Image #1]".to_string(),
        base64: "AAAA".to_string(),
        media_type: "image/png".to_string(),
    };
    let event = ChatInputEvent::classify_text("继续分析", vec![img.clone()]);
    match event {
        ChatInputEvent::UserMessage { text, images, .. } => {
            assert_eq!(text, "继续分析");
            assert_eq!(images, vec![img]);
        }
        other => panic!("expected UserMessage, got {other:?}"),
    }
}

#[test]
fn test_chat_input_event_classify_text_control_command() {
    let img = crate::ChatInputImage {
        id: "[Image #1]".to_string(),
        base64: "x".to_string(),
        media_type: "image/png".to_string(),
    };
    let event = ChatInputEvent::classify_text("  /clear", vec![img]);
    assert!(matches!(
        event,
        ChatInputEvent::ControlCommand { ref raw } if raw == "  /clear"
    ));
}

#[test]
fn test_user_message_generates_v7_input_id() {
    match ChatInputEvent::user_message("x", vec![]) {
        ChatInputEvent::UserMessage { id, .. } => {
            assert_eq!(id.as_uuid().get_version_num(), 7);
        }
        other => panic!("expected UserMessage, got {other:?}"),
    }
}
