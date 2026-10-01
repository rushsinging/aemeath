use super::*;

/// `CancelCurrentRun` executor 契约：Esc/Ctrl-C 在无可寻址 RunStep 时（Manual
/// Reflection Run、Main Run step 间隙）经 `AgentClient::cancel_current_run` 让
/// Runtime 控制面裁决当前活跃 Run，TUI 不自行推断 identity。
mod cancel_current_run {
    use super::*;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};

    struct StubAgentClient {
        outcome: sdk::CancelCurrentRunOutcome,
        calls: Mutex<Vec<sdk::ControlDeadline>>,
    }

    impl StubAgentClient {
        fn new(outcome: sdk::CancelCurrentRunOutcome) -> Self {
            Self {
                outcome,
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl sdk::AgentClient for StubAgentClient {
        fn cancel_current_run(
            &self,
            deadline: sdk::ControlDeadline,
        ) -> sdk::CancelCurrentRunOutcome {
            self.calls.lock().expect("calls").push(deadline);
            self.outcome
        }

        async fn chat(&self, _input: sdk::ChatRequest) -> Result<sdk::ChatStream, sdk::SdkError> {
            unreachable!("cancel stub 不提供 chat")
        }
    }

    fn app_with_client(outcome: sdk::CancelCurrentRunOutcome) -> (App, Arc<StubAgentClient>) {
        let mut app = App::new(
            "s".to_string(),
            std::path::PathBuf::from("/tmp"),
            "m".to_string(),
        );
        let client = Arc::new(StubAgentClient::new(outcome));
        app.agent_client = Some(client.clone());
        (app, client)
    }

    #[test]
    fn accepted_outcome_enters_cancelling_and_announces() {
        let (mut app, client) = app_with_client(sdk::CancelCurrentRunOutcome::Accepted);

        app.cancel_current_run_effect();

        assert_eq!(client.calls.lock().expect("calls").len(), 1);
        assert!(
            app.chat.is_cancelling,
            "Accepted 必须进入 cancelling 展示态"
        );
        assert!(
            app.model
                .conversation
                .runtime
                .status_notice
                .text
                .contains("Cancelling"),
            "Accepted 必须提示取消受理，实际: {}",
            app.model.conversation.runtime.status_notice.text
        );
    }

    #[test]
    fn no_active_run_does_not_enter_cancelling() {
        let (mut app, client) = app_with_client(sdk::CancelCurrentRunOutcome::NoActiveRun);

        app.cancel_current_run_effect();

        assert_eq!(client.calls.lock().expect("calls").len(), 1);
        assert!(
            !app.chat.is_cancelling,
            "NoActiveRun 不得伪造 cancelling 展示态"
        );
        assert!(
            app.model
                .conversation
                .runtime
                .status_notice
                .text
                .contains("No active response"),
            "NoActiveRun 必须提示无可取消目标，实际: {}",
            app.model.conversation.runtime.status_notice.text
        );
    }

    #[test]
    fn run_terminating_enters_cancelling_with_terminating_notice() {
        let (mut app, client) = app_with_client(sdk::CancelCurrentRunOutcome::RunTerminating);

        app.cancel_current_run_effect();

        assert_eq!(client.calls.lock().expect("calls").len(), 1);
        assert!(app.chat.is_cancelling);
        assert!(
            app.model
                .conversation
                .runtime
                .status_notice
                .text
                .contains("terminating"),
            "RunTerminating 必须提示终止中，实际: {}",
            app.model.conversation.runtime.status_notice.text
        );
    }
}

#[test]
fn effect_runtime_ignores_noop_effect() {
    let app = App::new(
        "s".to_string(),
        std::path::PathBuf::from("/tmp"),
        "m".to_string(),
    );
    assert!(!app.layout.should_exit);
}

#[test]
fn effect_runtime_quit_effect_sets_exit_flag() {
    let mut app = App::new(
        "s".to_string(),
        std::path::PathBuf::from("/tmp"),
        "m".to_string(),
    );
    app.layout.request_exit();
    assert!(app.layout.should_exit);
}

#[test]
fn effect_runtime_accepts_pending_image() {
    let mut app = App::new(
        "s".to_string(),
        std::path::PathBuf::from("/tmp"),
        "m".to_string(),
    );
    // accept_pending_clipboard_image 已移除（spawn_guarded 化），
    // 图片经 UiEvent::ClipboardImage → InsertImage intent 注入。
    app.handle_input_intent(crate::tui::model::input::intent::InputIntent::InsertImage(
        sdk::ClipboardImageView {
            base64: "abc".to_string(),
            media_type: "image/png".to_string(),
            final_size: 3,
            display_path: None,
            width: None,
            height: None,
        },
    ));
    assert_eq!(app.model.input.document.image_spans.len(), 1);
}

/// SendTerminalNotification 接线契约：executor 必须把 Effect 分派到注入 writer 的
/// 写出函数。NEVER 直接 execute_effect 执行该 Effect——它向真实 stdout 写 OSC 序列，
/// 会让每次 `cargo test` 触发真实桌面通知（测试副作用）。字节内容契约由
/// `terminal_notification_tests::write_terminal_notification_writes_exact_bytes_to_writer` 覆盖。
#[test]
fn executor_routes_terminal_notification_to_injected_writer() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui/effect/executor.rs"),
    )
    .expect("read executor source");

    assert!(
        source.contains("Effect::SendTerminalNotification { title, body }"),
        "executor 必须存在 SendTerminalNotification 分派分支"
    );
    assert!(
        source.contains("write_terminal_notification"),
        "executor 分支必须委托 write_terminal_notification（注入 writer 的唯一写出入口）"
    );
}
