use super::{
    TuiChatMessage, TuiContentBlock, TuiMessageSource, TuiStopHookFeedback, TuiToolResultImage,
};

#[test]
fn message_value_preserves_identity_content_and_stop_hook_metadata() {
    let message = TuiChatMessage {
        role: "user".to_string(),
        content: vec![
            TuiContentBlock::Text {
                text: "before ".to_string(),
            },
            TuiContentBlock::Image {
                media_type: "image/png".to_string(),
                base64: "image-data".to_string(),
                placeholder: Some("[Image #1]".to_string()),
            },
        ],
        input_id: Some("input-1".to_string()),
        source: TuiMessageSource::StopHook,
        stop_hook: Some(TuiStopHookFeedback {
            summary: "blocked".to_string(),
            command: "check.sh".to_string(),
            exit_code: Some(2),
            reason: "policy".to_string(),
            stdout_preview: "stdout".to_string(),
            stderr_preview: "stderr".to_string(),
            stdout_truncated: false,
            stderr_truncated: true,
            output_file: Some("/tmp/hook.log".to_string()),
        }),
    };

    assert_eq!(message.input_id.as_deref(), Some("input-1"));
    assert_eq!(message.text_content(), "before [Image #1]");
    assert_eq!(message.source, TuiMessageSource::StopHook);
    assert_eq!(message.stop_hook.unwrap().exit_code, Some(2));
}

#[test]
fn tool_result_image_is_tui_owned_value() {
    let image = TuiToolResultImage {
        base64: "image-data".to_string(),
        media_type: "image/png".to_string(),
    };

    assert_eq!(image.media_type, "image/png");
}

#[test]
fn production_runtime_view_has_no_sdk_or_runtime_resources() {
    let source = include_str!("runtime_view.rs");

    for forbidden in [
        "sdk::",
        "oneshot::Sender",
        "mpsc::Sender",
        "AgentClient",
        "PendingInteraction",
        "Registry",
    ] {
        assert!(
            !source.contains(forbidden),
            "Tui runtime view must not depend on {forbidden}"
        );
    }
}
