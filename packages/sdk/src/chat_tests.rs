use super::*;

#[test]
fn queue_display_text_derives_command_text_for_every_control_event() {
    // 排队回显的文本必须与用户输入的命令一致（#1816）。
    let cases = [
        (ChatInputEvent::Compact, "/compact"),
        (ChatInputEvent::Reset, "/clear"),
        (ChatInputEvent::ReflectNow, "/reflect-now"),
        (ChatInputEvent::ListModels, "/model"),
        (
            ChatInputEvent::SwitchModel {
                selection: "anthropic/claude".to_string(),
            },
            "/model anthropic/claude",
        ),
        (
            ChatInputEvent::SetThinking {
                desired: Some(true),
            },
            "/think on",
        ),
        (
            ChatInputEvent::SetThinking {
                desired: Some(false),
            },
            "/think off",
        ),
        (ChatInputEvent::SetThinking { desired: None }, "/think"),
        (ChatInputEvent::InitProject { force: true }, "/init --force"),
        (ChatInputEvent::InitProject { force: false }, "/init"),
        (
            ChatInputEvent::ManageSession {
                args: "list".to_string(),
            },
            "/session list",
        ),
        (
            ChatInputEvent::ManageMemory {
                args: String::new(),
            },
            "/memory",
        ),
        (
            ChatInputEvent::ResumeSession {
                id: "s-1".to_string(),
            },
            "/resume s-1",
        ),
        (
            ChatInputEvent::QueryReflectionHistory { limit: 3 },
            "/reflect 3",
        ),
        (
            ChatInputEvent::ControlCommand {
                raw: "/custom raw".to_string(),
            },
            "/custom raw",
        ),
    ];
    for (event, expected) in cases {
        assert_eq!(
            event.queue_display_text().as_deref(),
            Some(expected),
            "{event:?} 的排队展示文本必须与命令一致"
        );
    }
}

#[test]
fn queue_display_text_is_absent_for_non_command_events() {
    // 用户消息、技能请求与撤回指令不进入命令队列（#1816）。
    assert_eq!(
        ChatInputEvent::user_message("hi", Vec::new()).queue_display_text(),
        None
    );
    assert_eq!(ChatInputEvent::WithdrawAll.queue_display_text(), None);
    assert_eq!(
        ChatInputEvent::SkillRequest(SkillRequest {
            input_id: crate::InputId::new_v7(),
            skill: "skill".to_string(),
            arguments: String::new(),
            raw_input: "/skill".to_string(),
        })
        .queue_display_text(),
        None
    );
}

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
