use super::App;
use ::runtime::api::core::message::{ContentBlock, Message, Role};
use ::runtime::api::provider::types::{StopReason, StreamResponse, SystemBlock, Usage};
use ::runtime::api::provider::{LlmProvider, StreamHandler};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

struct BlockingReflectionProvider {
    started_tx: Mutex<Option<oneshot::Sender<()>>>,
    finish_rx: Mutex<Option<oneshot::Receiver<()>>>,
}

#[async_trait]
impl LlmProvider for BlockingReflectionProvider {
    async fn stream_message(
        &self,
        _system: &[SystemBlock],
        _messages: &[Message],
        _tool_schemas: &[serde_json::Value],
        handler: &mut dyn StreamHandler,
        _cancel: &CancellationToken,
    ) -> Result<StreamResponse, ::runtime::api::provider::LlmError> {
        if let Some(started_tx) = self.started_tx.lock().unwrap().take() {
            let _ = started_tx.send(());
        }
        let finish_rx = self.finish_rx.lock().unwrap().take().unwrap();
        let _ = finish_rx.await;
        let json = r#"{"deviations":[],"suggested_memories":[{"layer":"project","category":"fact","content":"记住 /reflect 后台执行","tags":["reflect"]}],"outdated_memories":[],"user_alert":null}"#;
        handler.on_text(json);
        Ok(StreamResponse {
            assistant_message: Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text {
                    text: json.to_string(),
                }],
            },
            usage: Usage {
                input_tokens: 3,
                output_tokens: 5,
            },
            stop_reason: StopReason::EndTurn,
        })
    }

    fn model_name(&self) -> &str {
        "blocking-reflection-model"
    }

    fn provider_name(&self) -> &str {
        "blocking-reflection-provider"
    }

    fn set_reasoning(&self, _enabled: bool) {}

    fn is_reasoning(&self) -> bool {
        false
    }
}

fn app_with_blocking_reflection_provider() -> (App, oneshot::Receiver<()>, oneshot::Sender<()>) {
    let (started_tx, started_rx) = oneshot::channel();
    let (finish_tx, finish_rx) = oneshot::channel();
    let provider = Arc::new(BlockingReflectionProvider {
        started_tx: Mutex::new(Some(started_tx)),
        finish_rx: Mutex::new(Some(finish_rx)),
    });
    let client = Arc::new(::runtime::api::provider::client::LlmClient::from_provider(
        provider,
    ));
    let mut app = App::new(
        "test-session".to_string(),
        std::env::temp_dir(),
        "test-model".to_string(),
    );
    app.client = Some(client);
    (app, started_rx, finish_tx)
}

#[tokio::test]
async fn test_spawn_llm_reflection_returns_before_llm_finishes() {
    let (mut app, started_rx, finish_tx) = app_with_blocking_reflection_provider();

    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let elapsed = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        app.handle_reflect_command_with_events("", Some(tx)),
    )
    .await;

    assert!(elapsed.is_ok(), "/reflect 不应同步等待 LLM 完成");
    assert!(app.is_processing, "/reflect 后应进入后台处理中状态");
    assert!(app.output_area.spinner.is_some());
    assert!(started_rx.await.is_ok(), "后台 reflection LLM 应已启动");

    let _ = finish_tx.send(());

    let mut got_done = false;
    while let Some(event) = rx.recv().await {
        match event {
            super::UiEvent::ReflectionDone { output } => {
                assert_eq!(output.suggested_memories.len(), 1);
                got_done = true;
                break;
            }
            super::UiEvent::Error(msg) => panic!("unexpected reflection error: {msg}"),
            _ => {}
        }
    }
    assert!(got_done, "后台 reflection 完成后应通过 UI event 返回结果");
}

#[tokio::test]
async fn test_auto_reflection_triggers_on_configured_interval() {
    let (mut app, mut started_rx, finish_tx) = app_with_blocking_reflection_provider();
    app.memory_config.reflection.interval_turns = 2;

    let (tx, _rx) = tokio::sync::mpsc::channel(8);
    app.maybe_auto_reflect(&tx);
    assert_eq!(app.turn_count, 1);
    assert!(started_rx.try_recv().is_err(), "第一轮不应触发 reflection");

    app.maybe_auto_reflect(&tx);
    assert_eq!(app.turn_count, 2);
    assert!(started_rx.await.is_ok(), "第二轮应触发后台 reflection");
    assert!(!app.is_processing, "自动 reflection 不应阻塞 UI 输入");
    assert!(
        app.output_area.spinner.is_none(),
        "自动 reflection 不应启动 spinner"
    );

    let _ = finish_tx.send(());
}

#[tokio::test]
async fn test_auto_reflection_boundary_disabled_does_not_trigger() {
    let (mut app, mut started_rx, finish_tx) = app_with_blocking_reflection_provider();
    app.memory_config.reflection.enabled = false;
    app.memory_config.reflection.interval_turns = 1;

    let (tx, _rx) = tokio::sync::mpsc::channel(8);
    app.maybe_auto_reflect(&tx);

    assert_eq!(app.turn_count, 1);
    assert!(started_rx.try_recv().is_err(), "禁用时不应触发 reflection");
    let _ = finish_tx.send(());
}

#[tokio::test]
async fn test_auto_reflection_boundary_memory_disabled_does_not_trigger() {
    let (mut app, mut started_rx, finish_tx) = app_with_blocking_reflection_provider();
    app.memory_config.enabled = false;
    app.memory_config.reflection.interval_turns = 1;

    let (tx, _rx) = tokio::sync::mpsc::channel(8);
    app.maybe_auto_reflect(&tx);

    assert_eq!(app.turn_count, 1);
    assert!(
        started_rx.try_recv().is_err(),
        "memory 禁用时不应触发 reflection"
    );
    let _ = finish_tx.send(());
}

#[tokio::test]
async fn test_auto_reflection_boundary_pending_reflection_does_not_trigger() {
    let (mut app, mut started_rx, finish_tx) = app_with_blocking_reflection_provider();
    app.memory_config.reflection.interval_turns = 1;
    app.pending_reflection = Some(::runtime::api::core::reflection::ReflectionOutput {
        deviations: Vec::new(),
        suggested_memories: Vec::new(),
        outdated_memories: Vec::new(),
        user_alert: None,
    });

    let (tx, _rx) = tokio::sync::mpsc::channel(8);
    app.maybe_auto_reflect(&tx);

    assert_eq!(app.turn_count, 1);
    assert!(
        started_rx.try_recv().is_err(),
        "已有 pending reflection 时不应重复触发"
    );
    let _ = finish_tx.send(());
}

#[tokio::test]
async fn test_auto_reflection_error_zero_interval_does_not_trigger() {
    let (mut app, mut started_rx, finish_tx) = app_with_blocking_reflection_provider();
    app.memory_config.reflection.interval_turns = 0;

    let (tx, _rx) = tokio::sync::mpsc::channel(8);
    app.maybe_auto_reflect(&tx);

    assert_eq!(app.turn_count, 1);
    assert!(
        started_rx.try_recv().is_err(),
        "间隔为 0 时不应触发 reflection"
    );
    let _ = finish_tx.send(());
}
