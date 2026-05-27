use super::App;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use tokio::sync::{oneshot, watch};

struct BlockingReflectionClient {
    started_tx: Mutex<Option<oneshot::Sender<()>>>,
    finish_rx: Mutex<Option<oneshot::Receiver<()>>>,
}

#[async_trait]
impl sdk::AgentClient for BlockingReflectionClient {
    fn session_snapshot(&self) -> sdk::SessionSnapshot {
        sdk::SessionSnapshot {
            id: "test-session".to_string(),
            message_count: 0,
            total_tokens: 0,
        }
    }

    fn cost(&self) -> sdk::CostInfo {
        sdk::CostInfo::default()
    }

    fn task_list(&self) -> Vec<sdk::TaskSummary> {
        Vec::new()
    }

    async fn task_status(&self) -> Result<sdk::TaskStatusView, sdk::SdkError> {
        Ok(sdk::TaskStatusView::default())
    }

    fn project(&self) -> sdk::ProjectContext {
        sdk::ProjectContext::default()
    }

    fn changes(&self) -> watch::Receiver<sdk::ChangeSet> {
        let (_tx, rx) = watch::channel(sdk::ChangeSet::empty());
        rx
    }

    async fn chat(&self, _input: sdk::ChatRequest) -> Result<sdk::ChatStream, sdk::SdkError> {
        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Ok(sdk::ChatStream::new(rx))
    }

    async fn save_current_session(&self) -> Result<(), sdk::SdkError> {
        Ok(())
    }

    fn cancel(&self) {}

    async fn load_session(&self, _id: &str) -> Result<sdk::SessionSnapshot, sdk::SdkError> {
        Ok(self.session_snapshot())
    }

    async fn list_sessions(&self) -> Result<Vec<sdk::SessionSummary>, sdk::SdkError> {
        Ok(Vec::new())
    }

    async fn delete_session(&self, _id: &str) -> Result<(), sdk::SdkError> {
        Ok(())
    }

    async fn list_models(&self) -> Result<Vec<sdk::ModelSummary>, sdk::SdkError> {
        Ok(Vec::new())
    }

    async fn compact(&self) -> Result<(), sdk::SdkError> {
        Ok(())
    }

    async fn read_clipboard_image(&self) -> Result<sdk::ClipboardImageView, sdk::SdkError> {
        Err(sdk::SdkError::Internal("not implemented".to_string()))
    }

    async fn process_image_file(
        &self,
        _path: String,
    ) -> Result<sdk::ClipboardImageView, sdk::SdkError> {
        Err(sdk::SdkError::Internal("not implemented".to_string()))
    }

    async fn run_reflection(
        &self,
        _messages: Vec<sdk::ChatMessage>,
    ) -> Result<sdk::ReflectionOutputView, sdk::SdkError> {
        if let Some(started_tx) = self.started_tx.lock().unwrap().take() {
            let _ = started_tx.send(());
        }
        let finish_rx = self.finish_rx.lock().unwrap().take().unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), finish_rx)
            .await
            .expect("reflection test finish signal should arrive before timeout")
            .expect("reflection test finish sender should not be dropped");
        Ok(sdk::ReflectionOutputView {
            content: "reflection done".to_string(),
            input_tokens: 3,
            output_tokens: 5,
            suggested_memories: vec![sdk::ReflectionMemorySuggestionView {
                content: "记住 /reflect 后台执行".to_string(),
                layer: "project".to_string(),
            }],
            outdated_memories: Vec::new(),
        })
    }

    async fn apply_reflection(
        &self,
        _output: sdk::ReflectionOutputView,
    ) -> Result<String, sdk::SdkError> {
        Ok("applied".to_string())
    }
}

fn app_with_blocking_reflection_client() -> (App, oneshot::Receiver<()>, oneshot::Sender<()>) {
    let (started_tx, started_rx) = oneshot::channel();
    let (finish_tx, finish_rx) = oneshot::channel();
    let client = Arc::new(BlockingReflectionClient {
        started_tx: Mutex::new(Some(started_tx)),
        finish_rx: Mutex::new(Some(finish_rx)),
    });
    let mut app = App::new(
        "test-session".to_string(),
        std::env::temp_dir(),
        "test-model".to_string(),
    );
    app.agent_client = Some(client);
    app.session.memory_config.enabled = true;
    app.session.memory_config.reflection.enabled = true;
    (app, started_rx, finish_tx)
}

#[tokio::test]
async fn test_spawn_llm_reflection_returns_before_llm_finishes() {
    let (mut app, started_rx, finish_tx) = app_with_blocking_reflection_client();

    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let elapsed = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        app.handle_reflect_command_with_events("", Some(tx)),
    )
    .await;

    assert!(elapsed.is_ok(), "/reflect 不应同步等待 LLM 完成");
    assert!(app.chat.is_processing, "/reflect 后应进入后台处理中状态");
    assert!(app.output_area.spinner.is_some());
    tokio::time::timeout(std::time::Duration::from_secs(1), started_rx)
        .await
        .expect("后台 reflection LLM 应在 1 秒内启动")
        .expect("后台 reflection LLM 应已启动");

    let _ = finish_tx.send(());

    let mut got_done = false;
    loop {
        let event = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("后台 reflection 完成后应在 1 秒内返回 UI event");
        let Some(event) = event else { break };
        match event {
            super::event::UiEvent::ReflectionDone { output } => {
                assert_eq!(output.suggested_memories.len(), 1);
                got_done = true;
                break;
            }
            super::event::UiEvent::Error(msg) => panic!("unexpected reflection error: {msg}"),
            _ => {}
        }
    }
    assert!(got_done, "后台 reflection 完成后应通过 UI event 返回结果");
}

#[tokio::test]
async fn test_auto_reflection_triggers_on_configured_interval() {
    let (mut app, mut started_rx, finish_tx) = app_with_blocking_reflection_client();
    app.session.memory_config.reflection.interval_turns = 2;

    let (tx, _rx) = tokio::sync::mpsc::channel(8);
    app.maybe_auto_reflect(&tx);
    assert_eq!(app.chat.turn_count, 1);
    assert!(started_rx.try_recv().is_err(), "第一轮不应触发 reflection");

    app.maybe_auto_reflect(&tx);
    assert_eq!(app.chat.turn_count, 2);
    tokio::time::timeout(std::time::Duration::from_secs(1), started_rx)
        .await
        .expect("第二轮应在 1 秒内触发后台 reflection")
        .expect("第二轮应触发后台 reflection");
    assert!(!app.chat.is_processing, "自动 reflection 不应阻塞 UI 输入");
    assert!(
        app.output_area.spinner.is_none(),
        "自动 reflection 不应启动 spinner"
    );

    let _ = finish_tx.send(());
}

#[tokio::test]
async fn test_auto_reflection_boundary_disabled_does_not_trigger() {
    let (mut app, mut started_rx, finish_tx) = app_with_blocking_reflection_client();
    app.session.memory_config.reflection.enabled = false;
    app.session.memory_config.reflection.interval_turns = 1;

    let (tx, _rx) = tokio::sync::mpsc::channel(8);
    app.maybe_auto_reflect(&tx);

    assert_eq!(app.chat.turn_count, 1);
    assert!(started_rx.try_recv().is_err(), "禁用时不应触发 reflection");
    let _ = finish_tx.send(());
}

#[tokio::test]
async fn test_auto_reflection_boundary_memory_disabled_does_not_trigger() {
    let (mut app, mut started_rx, finish_tx) = app_with_blocking_reflection_client();
    app.session.memory_config.enabled = false;
    app.session.memory_config.reflection.interval_turns = 1;

    let (tx, _rx) = tokio::sync::mpsc::channel(8);
    app.maybe_auto_reflect(&tx);

    assert_eq!(app.chat.turn_count, 1);
    assert!(
        started_rx.try_recv().is_err(),
        "memory 禁用时不应触发 reflection"
    );
    let _ = finish_tx.send(());
}

#[tokio::test]
async fn test_auto_reflection_boundary_pending_reflection_does_not_trigger() {
    let (mut app, mut started_rx, finish_tx) = app_with_blocking_reflection_client();
    app.session.memory_config.reflection.interval_turns = 1;
    app.chat.pending_reflection = Some(sdk::ReflectionOutputView {
        content: String::new(),
        input_tokens: 0,
        output_tokens: 0,
        suggested_memories: Vec::new(),
        outdated_memories: Vec::new(),
    });

    let (tx, _rx) = tokio::sync::mpsc::channel(8);
    app.maybe_auto_reflect(&tx);

    assert_eq!(app.chat.turn_count, 1);
    assert!(
        started_rx.try_recv().is_err(),
        "已有 pending reflection 时不应重复触发"
    );
    let _ = finish_tx.send(());
}

#[tokio::test]
async fn test_auto_reflection_error_zero_interval_does_not_trigger() {
    let (mut app, mut started_rx, finish_tx) = app_with_blocking_reflection_client();
    app.session.memory_config.reflection.interval_turns = 0;

    let (tx, _rx) = tokio::sync::mpsc::channel(8);
    app.maybe_auto_reflect(&tx);

    assert_eq!(app.chat.turn_count, 1);
    assert!(
        started_rx.try_recv().is_err(),
        "间隔为 0 时不应触发 reflection"
    );
    let _ = finish_tx.send(());
}
