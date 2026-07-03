use super::App;
use crate::tui::effect::effect::Effect;
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{oneshot, watch};

pub(super) struct BlockingReflectionClient {
    pub(super) started_tx: Mutex<Option<oneshot::Sender<()>>>,
    pub(super) finish_rx: Mutex<Option<oneshot::Receiver<()>>>,
    pub(super) clear_tasks_calls: AtomicUsize,
    pub(super) apply_reflection_calls: AtomicUsize,
    pub(super) apply_reflection_should_fail: std::sync::atomic::AtomicBool,
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
                category: "decision".to_string(),
                tags: Vec::new(),
            }],
            outdated_memories: Vec::new(),
            auto_applied: false,
        })
    }

    async fn apply_reflection(
        &self,
        _output: sdk::ReflectionOutputView,
    ) -> Result<String, sdk::SdkError> {
        self.apply_reflection_calls.fetch_add(1, Ordering::SeqCst);
        if self
            .apply_reflection_should_fail
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            Err(sdk::SdkError::Internal("apply failed".to_string()))
        } else {
            Ok("applied".to_string())
        }
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

    async fn clear_tasks(&self) -> Result<(), sdk::SdkError> {
        self.clear_tasks_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

pub(super) fn app_with_blocking_reflection_client(
) -> (App, oneshot::Receiver<()>, oneshot::Sender<()>) {
    let (app, started_rx, finish_tx, _) = app_with_blocking_reflection_client_handle();
    (app, started_rx, finish_tx)
}

pub(super) fn app_with_blocking_reflection_client_handle() -> (
    App,
    oneshot::Receiver<()>,
    oneshot::Sender<()>,
    Arc<BlockingReflectionClient>,
) {
    let (started_tx, started_rx) = oneshot::channel();
    let (finish_tx, finish_rx) = oneshot::channel();
    let client = Arc::new(BlockingReflectionClient {
        started_tx: Mutex::new(Some(started_tx)),
        finish_rx: Mutex::new(Some(finish_rx)),
        clear_tasks_calls: AtomicUsize::new(0),
        apply_reflection_calls: AtomicUsize::new(0),
        apply_reflection_should_fail: std::sync::atomic::AtomicBool::new(false),
    });
    let mut app = App::new(
        "test-session".to_string(),
        std::env::temp_dir(),
        "test-model".to_string(),
    );
    app.agent_client = Some(client.clone());
    app.session.memory_config.enabled = true;
    app.session.memory_config.reflection.enabled = true;
    (app, started_rx, finish_tx, client)
}

fn reflection_output(content: &str, auto_applied: bool) -> sdk::ReflectionOutputView {
    sdk::ReflectionOutputView {
        content: content.to_string(),
        input_tokens: 3,
        output_tokens: 5,
        suggested_memories: vec![sdk::ReflectionMemorySuggestionView {
            content: "记住 /reflect 后台执行".to_string(),
            layer: "project".to_string(),
            category: "decision".to_string(),
            tags: Vec::new(),
        }],
        outdated_memories: Vec::new(),
        auto_applied,
    }
}

fn system_texts(app: &App) -> Vec<&str> {
    app.model
        .conversation
        .timeline
        .items()
        .iter()
        .filter_map(|item| match item {
            crate::tui::model::output_timeline::OutputTimelineItem::System { text, .. } => {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect()
}

fn error_texts(app: &App) -> Vec<&str> {
    app.model
        .conversation
        .timeline
        .items()
        .iter()
        .filter_map(|item| match item {
            crate::tui::model::output_timeline::OutputTimelineItem::Error { text, .. } => {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect()
}

fn apply_ui_event(app: &mut App, event: super::event::UiEvent) -> Vec<Effect> {
    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    let spawn_refs =
        crate::tui::effect::session::processing::SpawnContextRefs { agent_client: None };
    app.update(crate::tui::update::msg::TuiMsg::Ui(event), &tx, &spawn_refs)
        .effects
}

#[tokio::test]
async fn test_reflection_done_auto_complete_display_and_pending() {
    let (mut app, _started_rx, finish_tx) = app_with_blocking_reflection_client();
    let output = reflection_output("完整 reflection 内容", false);

    let effects = apply_ui_event(&mut app, super::event::UiEvent::ReflectionDone { output });

    assert!(effects.is_empty());
    assert_eq!(
        app.chat
            .pending_reflection
            .as_ref()
            .map(|o| o.content.as_str()),
        Some("完整 reflection 内容")
    );
    let texts = system_texts(&app);
    assert!(texts
        .iter()
        .any(|text| text.contains("完整 reflection 内容")));
    assert!(texts
        .iter()
        .any(|text| text.contains("可运行 /reflect apply 应用这些 memory 建议")));
    let _ = finish_tx.send(());
}

#[tokio::test]
async fn test_reflection_done_auto_applied_does_not_save_pending() {
    let (mut app, _started_rx, finish_tx) = app_with_blocking_reflection_client();
    let output = reflection_output("已自动应用的完整内容", true);

    let effects = apply_ui_event(&mut app, super::event::UiEvent::ReflectionDone { output });

    assert!(effects.is_empty(), "auto_applied=true 不应再次 apply");
    assert!(app.chat.pending_reflection.is_none());
    let texts = system_texts(&app);
    assert!(texts
        .iter()
        .any(|text| text.contains("已自动应用的完整内容")));
    assert!(texts.iter().any(|text| text.contains("已自动应用")));
    let _ = finish_tx.send(());
}

#[tokio::test]
async fn test_handle_reflect_command_with_pending_warns_refresh() {
    let (mut app, _started_rx, finish_tx) = app_with_blocking_reflection_client();
    app.chat.pending_reflection = Some(reflection_output("旧建议", false));

    let effects = app.handle_reflect_command("");

    assert!(matches!(
        effects.first(),
        Some(Effect::RunReflection { foreground: true })
    ));
    assert!(system_texts(&app)
        .iter()
        .any(|text| text.contains("已有未应用建议，本次将刷新")));
    let _ = finish_tx.send(());
}

#[tokio::test]
async fn test_apply_reflection_success_clears_pending_via_ui_event() {
    let (mut app, _started_rx, finish_tx, client) = app_with_blocking_reflection_client_handle();
    app.chat.pending_reflection = Some(reflection_output("待应用", false));
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);

    let effects = app.handle_reflect_command("apply");
    assert!(app.chat.pending_reflection.is_none());
    assert_eq!(
        app.chat.applying_reflection.as_ref().unwrap().content,
        "待应用"
    );
    for effect in effects {
        app.execute_effect(effect, &tx).await;
    }
    let event = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("apply 应回传 UI event")
        .expect("apply event should exist");
    let effects = apply_ui_event(&mut app, event);

    assert!(effects.is_empty());
    assert!(app.chat.pending_reflection.is_none());
    assert!(app.chat.applying_reflection.is_none());
    assert_eq!(client.apply_reflection_calls.load(Ordering::SeqCst), 1);
    assert!(system_texts(&app)
        .iter()
        .any(|text| text.contains("reflection apply 成功")));
    let _ = finish_tx.send(());
}

#[tokio::test]
async fn test_apply_reflection_failure_keeps_pending_via_ui_event() {
    let (mut app, _started_rx, finish_tx, client) = app_with_blocking_reflection_client_handle();
    client
        .apply_reflection_should_fail
        .store(true, std::sync::atomic::Ordering::SeqCst);
    app.chat.pending_reflection = Some(reflection_output("待重试", false));
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);

    let effects = app.handle_reflect_command("apply");
    assert!(app.chat.pending_reflection.is_none());
    assert_eq!(
        app.chat.applying_reflection.as_ref().unwrap().content,
        "待重试"
    );
    for effect in effects {
        app.execute_effect(effect, &tx).await;
    }
    let event = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("apply 失败应回传 UI event")
        .expect("apply event should exist");
    let effects = apply_ui_event(&mut app, event);

    assert!(effects.is_empty());
    assert_eq!(
        app.chat
            .pending_reflection
            .as_ref()
            .map(|o| o.content.as_str()),
        Some("待重试")
    );
    assert_eq!(client.apply_reflection_calls.load(Ordering::SeqCst), 1);
    assert!(app.chat.applying_reflection.is_none());
    assert!(error_texts(&app)
        .iter()
        .any(|text| text.contains("Reflection apply 失败")));
    let _ = finish_tx.send(());
}

#[tokio::test]
async fn test_apply_reflection_duplicate_apply_is_rejected_while_in_flight() {
    let (mut app, _started_rx, finish_tx, client) = app_with_blocking_reflection_client_handle();
    app.chat.pending_reflection = Some(reflection_output("待应用一次", false));

    let effects = app.handle_reflect_command("apply");
    assert_eq!(effects.len(), 1);
    assert!(app.chat.pending_reflection.is_none());
    assert_eq!(
        app.chat
            .applying_reflection
            .as_ref()
            .map(|output| output.content.as_str()),
        Some("待应用一次")
    );

    let duplicate_effects = app.handle_reflect_command("apply");

    assert!(duplicate_effects.is_empty());
    assert!(system_texts(&app)
        .iter()
        .any(|text| text.contains("Reflection apply 正在进行中")));
    assert_eq!(client.apply_reflection_calls.load(Ordering::SeqCst), 0);
    let _ = finish_tx.send(());
}

#[tokio::test]
async fn test_apply_reflection_success_keeps_new_pending_created_while_in_flight() {
    let (mut app, _started_rx, finish_tx, _client) = app_with_blocking_reflection_client_handle();
    let old_output = reflection_output("旧待应用", false);
    app.chat.pending_reflection = Some(old_output.clone());

    let effects = app.handle_reflect_command("apply");
    assert_eq!(effects.len(), 1);
    assert!(app.chat.pending_reflection.is_none());
    assert_eq!(
        app.chat
            .applying_reflection
            .as_ref()
            .map(|output| output.content.as_str()),
        Some("旧待应用")
    );

    apply_ui_event(
        &mut app,
        super::event::UiEvent::ReflectionDone {
            output: reflection_output("新建议", false),
        },
    );
    assert_eq!(
        app.chat
            .pending_reflection
            .as_ref()
            .map(|output| output.content.as_str()),
        Some("新建议")
    );

    let effects = apply_ui_event(
        &mut app,
        super::event::UiEvent::ReflectionApplyDone {
            output: old_output,
            result: Ok("applied".to_string()),
        },
    );

    assert!(effects.is_empty());
    assert!(app.chat.applying_reflection.is_none());
    assert_eq!(
        app.chat
            .pending_reflection
            .as_ref()
            .map(|output| output.content.as_str()),
        Some("新建议")
    );
    let _ = finish_tx.send(());
}

#[tokio::test]
async fn test_clear_command_clears_task_store_and_task_window() {
    let (mut app, _started_rx, finish_tx, client) = app_with_blocking_reflection_client_handle();
    app.model
        .conversation
        .apply(crate::tui::model::conversation::intent::UpdateTaskLines(
            vec!["━━ Tasks: 1/1 ━━".to_string(), "□ #1 existing".to_string()],
        ));
    app.refresh_live_status_from_model();
    assert!(!app.live_status_view_model().task_lines.is_empty());

    app.handle_slash_command_with_events("/clear", None).await;
    app.refresh_live_status_from_model();

    assert_eq!(client.clear_tasks_calls.load(Ordering::SeqCst), 1);
    assert!(app.model.conversation.runtime.task_status.lines.is_empty());
    assert!(app.live_status_view_model().task_lines.is_empty());
    let _ = finish_tx.send(());
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
    // spinner 业务真相在 Model；渲染直接消费 LiveStatusViewModel。
    // #536: /reflect 经 run_loop 兜底设 Thinking（spinner_phase → chat_active=true）。
    assert!(
        app.model.conversation.runtime.spinner.chat_active,
        "/reflect 后 Model spinner 应 active"
    );
    app.refresh_live_status_from_model();
    assert!(app.live_status_view_model().spinner.is_some());

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
        !app.model.conversation.runtime.spinner.chat_active,
        "自动 reflection 不应启动 spinner（Model 真相）"
    );
    app.refresh_live_status_from_model();
    assert!(
        app.live_status_view_model().spinner.is_none(),
        "自动 reflection 不应启动 spinner（LiveStatusViewModel）"
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
        auto_applied: false,
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
