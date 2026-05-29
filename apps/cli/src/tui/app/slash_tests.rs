use super::App;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use tokio::sync::{oneshot, watch};

pub(super) struct BlockingReflectionClient {
    pub(super) started_tx: Mutex<Option<oneshot::Sender<()>>>,
    pub(super) finish_rx: Mutex<Option<oneshot::Receiver<()>>>,
}

#[async_trait]
impl sdk::AgentClient for BlockingReflectionClient {
    fn session_snapshot(&self) -> sdk::SessionSnapshot {
        sdk::SessionSnapshot {
            id: "test-session".to_string(),
            message_count: 0,
            total_tokens: 0,
            messages: vec![],
            created_at: None,
            trimmed: 0,
            repaired: 0,
            workspace: None,
            tasks: None,
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

    async fn execute_command(
        &self,
        _name: &str,
        _args: &str,
        _ctx: sdk::CommandContext,
    ) -> Result<sdk::CommandResult, sdk::SdkError> {
        Ok(sdk::CommandResult::Success("ok".to_string()))
    }

    async fn estimate_context(
        &self,
        _messages: &[sdk::ChatMessage],
        _system_prompt: &str,
    ) -> Result<sdk::ContextEstimate, sdk::SdkError> {
        Ok(sdk::ContextEstimate {
            estimated_tokens: 0,
            system_tokens: 0,
            context_size: 0,
            usage_percentage: 0.0,
        })
    }

    async fn switch_model(
        &self,
        _params: sdk::ModelSwitchParams,
    ) -> Result<sdk::ModelSwitchResult, sdk::SdkError> {
        Ok(sdk::ModelSwitchResult {
            display_name: "test/model".to_string(),
            context_window: 0,
            reasoning_active: None,
        })
    }

    async fn set_thinking(&self, _desired: Option<bool>) -> Result<bool, sdk::SdkError> {
        Ok(true)
    }

    async fn compact_messages(
        &self,
        messages: Vec<sdk::ChatMessage>,
        _system_prompt: &str,
        _context_size: usize,
    ) -> Result<(Vec<sdk::ChatMessage>, bool), sdk::SdkError> {
        Ok((messages, false))
    }

    async fn notify_hook(&self, _message: &str, _kind: &str) -> Result<(), sdk::SdkError> {
        Ok(())
    }

    async fn list_reminders(&self) -> Result<Vec<sdk::ReminderView>, sdk::SdkError> {
        Ok(Vec::new())
    }

    async fn add_reminder(&self, _content: &str) -> Result<String, sdk::SdkError> {
        Ok("test-id".to_string())
    }

    async fn complete_reminder(&self, _id: &str) -> Result<(), sdk::SdkError> {
        Ok(())
    }

    async fn get_thinking(&self) -> Result<bool, sdk::SdkError> {
        Ok(false)
    }

    async fn restore_tasks(&self, _snapshot: serde_json::Value) -> Result<(), sdk::SdkError> {
        Ok(())
    }
}

pub(super) fn app_with_blocking_reflection_client(
) -> (App, oneshot::Receiver<()>, oneshot::Sender<()>) {
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
    use crate::tui::effect::effect::Effect;
    let (mut app, started_rx, finish_tx) = app_with_blocking_reflection_client();

    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    // /reflect 同步返回 RunReflection Effect（不在此处 spawn），UI 状态立即更新。
    let effects = app.handle_reflect_command("");
    assert!(
        matches!(
            effects.first(),
            Some(Effect::RunReflection { foreground: true })
        ),
        "/reflect 应返回前台 RunReflection Effect"
    );
    assert!(app.chat.is_processing, "/reflect 后应进入后台处理中状态");
    // spinner 业务真相在 Model；widget 镜像经 refresh_live_status_from_model 单向派生。
    assert!(
        app.model.runtime.spinner.active,
        "/reflect 后 Model spinner 应 active"
    );
    app.refresh_live_status_from_model();
    assert!(app.output_area.spinner.is_some());

    // 由 executor 执行 Effect：后台 spawn 调用 LLM，不阻塞调用方。
    for effect in effects {
        app.execute_effect(effect, &tx).await;
    }
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
    let effect1 = app.maybe_auto_reflect();
    assert!(effect1.is_none(), "第一轮不应返回 reflection Effect");
    assert_eq!(app.chat.turn_count, 1);
    assert!(started_rx.try_recv().is_err(), "第一轮不应触发 reflection");

    let effect2 = app.maybe_auto_reflect();
    assert!(effect2.is_some(), "第二轮应返回后台 reflection Effect");
    if let Some(effect) = effect2 {
        app.execute_effect(effect, &tx).await;
    }
    assert_eq!(app.chat.turn_count, 2);
    tokio::time::timeout(std::time::Duration::from_secs(1), started_rx)
        .await
        .expect("第二轮应在 1 秒内触发后台 reflection")
        .expect("第二轮应触发后台 reflection");
    assert!(!app.chat.is_processing, "自动 reflection 不应阻塞 UI 输入");
    assert!(
        !app.model.runtime.spinner.active,
        "自动 reflection 不应启动 spinner（Model 真相）"
    );
    app.refresh_live_status_from_model();
    assert!(
        app.output_area.spinner.is_none(),
        "自动 reflection 不应启动 spinner（widget 镜像）"
    );

    let _ = finish_tx.send(());
}

#[tokio::test]
async fn test_auto_reflection_boundary_disabled_does_not_trigger() {
    let (mut app, mut started_rx, finish_tx) = app_with_blocking_reflection_client();
    app.session.memory_config.reflection.enabled = false;
    app.session.memory_config.reflection.interval_turns = 1;

    let (_tx, _rx) = tokio::sync::mpsc::channel::<super::event::UiEvent>(8);
    let effect = app.maybe_auto_reflect();
    assert!(effect.is_none(), "禁用时不应返回 reflection Effect");

    assert_eq!(app.chat.turn_count, 1);
    assert!(started_rx.try_recv().is_err(), "禁用时不应触发 reflection");
    let _ = finish_tx.send(());
}

#[tokio::test]
async fn test_auto_reflection_boundary_memory_disabled_does_not_trigger() {
    let (mut app, mut started_rx, finish_tx) = app_with_blocking_reflection_client();
    app.session.memory_config.enabled = false;
    app.session.memory_config.reflection.interval_turns = 1;

    let (_tx, _rx) = tokio::sync::mpsc::channel::<super::event::UiEvent>(8);
    let effect = app.maybe_auto_reflect();
    assert!(effect.is_none(), "memory 禁用时不应返回 reflection Effect");

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

    let (_tx, _rx) = tokio::sync::mpsc::channel::<super::event::UiEvent>(8);
    let effect = app.maybe_auto_reflect();
    assert!(
        effect.is_none(),
        "已有 pending reflection 时不应返回 Effect"
    );

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

    let (_tx, _rx) = tokio::sync::mpsc::channel::<super::event::UiEvent>(8);
    let effect = app.maybe_auto_reflect();
    assert!(effect.is_none(), "间隔为 0 时不应返回 reflection Effect");

    assert_eq!(app.chat.turn_count, 1);
    assert!(
        started_rx.try_recv().is_err(),
        "间隔为 0 时不应触发 reflection"
    );
    let _ = finish_tx.send(());
}
