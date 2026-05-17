use super::App;
use aemeath_core::message::{ContentBlock, Message, Role};
use aemeath_llm::provider::{LlmProvider, StreamHandler};
use aemeath_llm::types::{StopReason, StreamResponse, SystemBlock, Usage};
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
    ) -> Result<StreamResponse, aemeath_llm::LlmError> {
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

#[tokio::test]
async fn test_spawn_llm_reflection_returns_before_llm_finishes() {
    let (started_tx, started_rx) = oneshot::channel();
    let (finish_tx, finish_rx) = oneshot::channel();
    let provider = Arc::new(BlockingReflectionProvider {
        started_tx: Mutex::new(Some(started_tx)),
        finish_rx: Mutex::new(Some(finish_rx)),
    });
    let client = Arc::new(aemeath_llm::client::LlmClient::from_provider(provider));
    let mut app = App::new(
        "test-session".to_string(),
        std::env::temp_dir(),
        "test-model".to_string(),
    );
    app.client = Some(client);

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
