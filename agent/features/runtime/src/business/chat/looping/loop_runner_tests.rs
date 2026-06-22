//! Tests for `loop_runner`, extracted into a dedicated module to keep the
//! runner file focused on the production code path.

use super::loop_helpers::is_user_cancelled_provider_error;
use super::*;
use ::tools::api::ToolRegistry;
use async_trait::async_trait;
use hook::api::HookRunner;
use provider::api::{LlmProvider, StreamHandler};
use provider::api::{StopReason, StreamResponse, SystemBlock, Usage};
use share::config::hooks::{HookEntry, HookEvent, HooksConfig};
use share::message::{Message, MessageSource, Role};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[test]
fn provider_cancelled_error_maps_to_cancelled_outcome() {
    let error = provider::api::LlmError::Cancelled;
    assert!(is_user_cancelled_provider_error(&error));
}

#[derive(Clone)]
struct SequenceQueueDrainPort {
    responses: Arc<Mutex<VecDeque<Option<Vec<String>>>>>,
}

impl SequenceQueueDrainPort {
    fn new(responses: Vec<Option<Vec<String>>>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(VecDeque::from(responses))),
        }
    }
}

impl QueueDrainPort for SequenceQueueDrainPort {
    fn drain_queued_input<'a>(&'a self) -> crate::business::chat::looping::QueueFuture<'a> {
        Box::pin(async move { self.responses.lock().unwrap().pop_front().flatten() })
    }
}

#[derive(Clone, Default)]
struct EmptyInputEvents;

impl InputEventDrainPort for EmptyInputEvents {
    fn drain_input_events<'a>(&'a self) -> crate::business::chat::looping::InputEventFuture<'a> {
        Box::pin(async { Vec::new() })
    }

    fn recv_next_input<'a>(&'a self) -> crate::business::chat::looping::InputEventOptFuture<'a> {
        Box::pin(async { None })
    }
}

#[derive(Clone, Default)]
struct RecordingSink {
    events: Arc<Mutex<Vec<String>>>,
    messages_syncs: Arc<Mutex<Vec<Vec<Message>>>>,
    done_durations: Arc<Mutex<Vec<std::time::Duration>>>,
}

impl ChatEventSink for RecordingSink {
    fn send_event<'a>(
        &'a self,
        event: RuntimeStreamEvent,
    ) -> crate::business::chat::looping::EventFuture<'a> {
        Box::pin(async move {
            self.record(event);
        })
    }

    fn try_send_event(&self, event: RuntimeStreamEvent) {
        self.record(event);
    }
}

impl RecordingSink {
    fn record(&self, event: RuntimeStreamEvent) {
        let name = match event {
            RuntimeStreamEvent::MessagesSync(messages) => {
                self.messages_syncs.lock().unwrap().push(messages.clone());
                format!(
                    "MessagesSync:{}",
                    messages
                        .last()
                        .map(|message| message.text_content())
                        .unwrap_or_default()
                )
            }
            RuntimeStreamEvent::DoneWithDuration { duration, .. } => {
                self.done_durations.lock().unwrap().push(duration);
                "DoneWithDuration".to_string()
            }
            RuntimeStreamEvent::HookEvent(event) => {
                format!("HookEvent:{}:{:?}", event.hook_name, event.status)
            }
            RuntimeStreamEvent::TurnChanged(turn) => format!("TurnChanged:{turn}"),
            RuntimeStreamEvent::Usage { .. } => "Usage".to_string(),
            RuntimeStreamEvent::Text { text, .. } => format!("Text:{text}"),
            RuntimeStreamEvent::Done { .. } => "Done".to_string(),
            RuntimeStreamEvent::SystemMessage(message) => format!("SystemMessage:{message}"),
            RuntimeStreamEvent::Error(message) => format!("Error:{message}"),
            RuntimeStreamEvent::Cancelled { .. } => "Cancelled".to_string(),
            RuntimeStreamEvent::Thinking { .. } => "Thinking".to_string(),
            RuntimeStreamEvent::BlockComplete { .. } => "BlockComplete".to_string(),
            RuntimeStreamEvent::ToolCallStart { .. } => "ToolCallStart".to_string(),
            RuntimeStreamEvent::ToolCallUpdate { .. } => "ToolCallUpdate".to_string(),
            RuntimeStreamEvent::ToolResult { .. } => "ToolResult".to_string(),
            RuntimeStreamEvent::LiveTps(_) => "LiveTps".to_string(),
            RuntimeStreamEvent::AskUserBatch { .. } => "AskUserBatch".to_string(),
            RuntimeStreamEvent::AgentProgress { .. } => "AgentProgress".to_string(),
            RuntimeStreamEvent::WorkingDirectoryChanged { .. } => {
                "WorkingDirectoryChanged".to_string()
            }
            RuntimeStreamEvent::TasksChanged => "TasksChanged".to_string(),
            RuntimeStreamEvent::ConfigReloaded { .. } => "ConfigReloaded".to_string(),
            RuntimeStreamEvent::UserMessagesAdded { .. } => "UserMessagesAdded".to_string(),
            RuntimeStreamEvent::SessionReset => "SessionReset".to_string(),
            RuntimeStreamEvent::UserMessagesWithdrawn { .. } => "UserMessagesWithdrawn".to_string(),
        };
        self.events.lock().unwrap().push(name);
    }

    fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }

    fn synced_messages(&self) -> Vec<Vec<Message>> {
        self.messages_syncs.lock().unwrap().clone()
    }

    fn done_durations(&self) -> Vec<std::time::Duration> {
        self.done_durations.lock().unwrap().clone()
    }
}

struct TwoTurnProvider;

#[async_trait]
impl LlmProvider for TwoTurnProvider {
    async fn stream_message(
        &self,
        _system: &[SystemBlock],
        messages: &[Message],
        _tool_schemas: &[serde_json::Value],
        handler: &mut dyn StreamHandler,
        _cancel: &CancellationToken,
    ) -> Result<StreamResponse, provider::LlmError> {
        let text = if messages
            .iter()
            .any(|message| message.text_content() == "stop-hook input")
        {
            "handled queued input"
        } else {
            "initial final response"
        };
        handler.on_text(text);
        Ok(StreamResponse {
            assistant_message: Message {
                role: share::message::Role::Assistant,
                content: vec![share::message::ContentBlock::Text {
                    text: text.to_string(),
                }],
                metadata: None,
            },
            usage: Usage {
                input_tokens: 1,
                output_tokens: 1,
                cached_tokens: None,
                cache_creation_tokens: None,
                reasoning_tokens: None,
            },
            stop_reason: StopReason::EndTurn,
        })
    }

    fn model_name(&self) -> &str {
        "test-model"
    }

    fn provider_name(&self) -> &str {
        "test-provider"
    }

    fn set_reasoning(&self, _enabled: bool) {}

    fn is_reasoning(&self) -> bool {
        false
    }
}

struct SequenceProvider {
    responses: Arc<Mutex<VecDeque<String>>>,
}

impl SequenceProvider {
    fn new(responses: Vec<&str>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(
                responses.into_iter().map(str::to_string).collect(),
            )),
        }
    }
}

#[async_trait]
impl LlmProvider for SequenceProvider {
    async fn stream_message(
        &self,
        _system: &[SystemBlock],
        _messages: &[Message],
        _tool_schemas: &[serde_json::Value],
        handler: &mut dyn StreamHandler,
        _cancel: &CancellationToken,
    ) -> Result<StreamResponse, provider::LlmError> {
        let text = self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| "fallback final response".to_string());
        handler.on_text(&text);
        Ok(StreamResponse {
            assistant_message: Message {
                role: share::message::Role::Assistant,
                content: vec![share::message::ContentBlock::Text { text }],
                metadata: None,
            },
            usage: Usage {
                input_tokens: 1,
                output_tokens: 1,
                cached_tokens: None,
                cache_creation_tokens: None,
                reasoning_tokens: None,
            },
            stop_reason: StopReason::EndTurn,
        })
    }

    fn model_name(&self) -> &str {
        "test-model"
    }

    fn provider_name(&self) -> &str {
        "test-provider"
    }

    fn set_reasoning(&self, _enabled: bool) {}

    fn is_reasoning(&self) -> bool {
        false
    }
}

fn test_hook_runner() -> HookRunner {
    let mut events = HashMap::new();
    events.insert(
        HookEvent::Stop,
        vec![HookEntry {
            matcher: String::new(),
            command: "true".to_string(),
            timeout: 5,
        }],
    );
    HookRunner::new(HooksConfig { events })
}

fn blocking_then_success_hook_runner(flag_path: &std::path::Path) -> HookRunner {
    // 用 nanos 时间戳生成唯一 flag 路径，避免与并行 cargo test 共享
    // target/stop-hook-once.flag 时的 race condition。
    let flag_path_str = flag_path.to_string_lossy().to_string();
    let mut events = HashMap::new();
    events.insert(
        HookEvent::Stop,
        vec![HookEntry {
            matcher: String::new(),
            command: format!(
                "python3 -c 'import pathlib, sys; \
                 p=pathlib.Path(\"{flag_path}\"); \
                 sys.exit(0 if p.exists() else (p.parent.mkdir(parents=True, exist_ok=True), \
                 p.write_text(\"blocked\"), print(\"fix before stopping\"), 2)[3])'",
                flag_path = flag_path_str,
            ),
            timeout: 5,
        }],
    );
    HookRunner::new(HooksConfig { events })
}

#[tokio::test]
async fn test_process_chat_loop_stop_hook_blocked_continues_until_success() {
    // 每次测试生成独立 flag 路径，避免 cargo test 并行 race。
    let flag_path = std::env::temp_dir().join(format!(
        "aemeath_stop_hook_once_{}.flag",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_file(&flag_path);
    let sink = RecordingSink::default();

    process_chat_loop(ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![None, None, None, None]),
        input_events: EmptyInputEvents,
        client: Arc::new(provider::api::LlmClient::from_provider(Arc::new(
            SequenceProvider::new(vec!["first attempted final", "after hook feedback"]),
        ))),
        registry: Arc::new(ToolRegistry::new()),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        messages: vec![Message::user("hello")],
        context_size: 200_000,
        cwd: std::env::current_dir().unwrap(),
        workspace: project::api::WorkspaceService::new(std::env::current_dir().unwrap()),
        session_id: "test-stop-hook-blocked".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(share::tool::SessionReminders::new())),
        agent_runner: None,
        allow_all: false,
        cancel: Arc::new(Mutex::new(CancellationToken::new())),
        task_store: Arc::new(storage::api::TaskStore::new()),
        max_tool_concurrency: 1,
        max_agent_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: blocking_then_success_hook_runner(&flag_path),
        memory_config: share::config::MemoryConfig::default(),
        frozen_chats: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        active_summary: std::sync::Arc::new(std::sync::Mutex::new(None)),
        language: "en".to_string(),
    })
    .await;
    let _ = std::fs::remove_file(&flag_path);

    let events = sink.events();
    let feedback_sync = events
        .iter()
        .position(|event| {
            event.starts_with("MessagesSync:")
                && event.contains("You MUST first satisfy the Stop hook requirement")
        })
        .expect("blocked Stop hook feedback should be synced into messages");
    let hook_notice = events
        .iter()
        .position(|event| event == "HookEvent:Stop:Blocked")
        .expect("blocked Stop hook should emit typed hook event");
    let second_text = events
        .iter()
        .position(|event| event == "Text:after hook feedback")
        .expect("blocked Stop hook should continue to another LLM turn");
    let done = events
        .iter()
        .position(|event| event == "DoneWithDuration")
        .expect("loop should finish after Stop hook succeeds");

    assert!(hook_notice < feedback_sync);
    assert!(feedback_sync < second_text);
    assert!(second_text < done);
    assert_eq!(
        events
            .iter()
            .filter(|event| event.as_str() == "DoneWithDuration")
            .count(),
        1
    );
}

#[tokio::test]
async fn test_stop_hook_feedback_message_is_marked_system_generated() {
    let flag_path = std::env::temp_dir().join(format!(
        "aemeath_stop_hook_metadata_{}.flag",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_file(&flag_path);
    let sink = RecordingSink::default();

    process_chat_loop(ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![None, None, None, None]),
        input_events: EmptyInputEvents,
        client: Arc::new(provider::api::LlmClient::from_provider(Arc::new(
            SequenceProvider::new(vec!["first attempted final", "after hook feedback"]),
        ))),
        registry: Arc::new(ToolRegistry::new()),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        messages: vec![Message::user("hello")],
        context_size: 200_000,
        cwd: std::env::current_dir().unwrap(),
        workspace: project::api::WorkspaceService::new(std::env::current_dir().unwrap()),
        session_id: "test-stop-hook-metadata".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(share::tool::SessionReminders::new())),
        agent_runner: None,
        allow_all: false,
        cancel: Arc::new(Mutex::new(CancellationToken::new())),
        task_store: Arc::new(storage::api::TaskStore::new()),
        max_tool_concurrency: 1,
        max_agent_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: blocking_then_success_hook_runner(&flag_path),
        memory_config: share::config::MemoryConfig::default(),
        frozen_chats: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        active_summary: std::sync::Arc::new(std::sync::Mutex::new(None)),
        language: "en".to_string(),
    })
    .await;
    let _ = std::fs::remove_file(&flag_path);

    let feedback = sink
        .synced_messages()
        .into_iter()
        .flatten()
        .find(|message| {
            message
                .text_content()
                .contains("You MUST first satisfy the Stop hook requirement")
        })
        .expect("blocked Stop hook feedback should be synced into messages");

    assert_eq!(feedback.role, Role::User);
    assert_eq!(feedback.source(), MessageSource::SystemGenerated);
}

#[tokio::test]
async fn test_process_chat_loop_uses_workspace_workspace_root_for_stop_hook_env() {
    let sink = RecordingSink::default();
    let path_base = tempfile::tempdir().unwrap();
    let workspace_root = tempfile::tempdir().unwrap();
    let marker = path_base.path().join("stop-hook-env.txt");
    let marker_path = marker.display().to_string();
    let workspace_dto = crate::business::session::PersistedWorkspaceContext {
        path_base: path_base.path().display().to_string(),
        workspace_root: workspace_root.path().display().to_string(),
        context_stack: vec![crate::business::session::PersistedWorkspaceFrame {
            path_base: path_base.path().display().to_string(),
            workspace_root: path_base.path().display().to_string(),
        }],
    };
    let workspace = project::api::WorkspaceService::new(path_base.path().to_path_buf());
    project::api::WorkspacePersist::restore(workspace.as_ref(), &workspace_dto)
        .expect("restore workspace dto");
    let mut events = HashMap::new();
    events.insert(
        HookEvent::Stop,
        vec![HookEntry {
            matcher: String::new(),
            command: format!(
                "printf '%s|%s|%s' \"$AEMEATH_PROJECT_DIR\" \"$CLAUDE_PROJECT_DIR\" \"$PWD\" > \"{}\"",
                marker_path
            ),
            timeout: 5,
        }],
    );

    process_chat_loop(ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![None, None]),
        input_events: EmptyInputEvents,
        client: Arc::new(provider::api::LlmClient::from_provider(Arc::new(
            SequenceProvider::new(vec!["final response"]),
        ))),
        registry: Arc::new(ToolRegistry::new()),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        messages: vec![Message::user("hello")],
        context_size: 200_000,
        cwd: path_base.path().to_path_buf(),
        workspace,
        session_id: "test-worktree-stop-hook-env".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(share::tool::SessionReminders::new())),
        agent_runner: None,
        allow_all: false,
        cancel: Arc::new(Mutex::new(CancellationToken::new())),
        task_store: Arc::new(storage::api::TaskStore::new()),
        max_tool_concurrency: 1,
        max_agent_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: HookRunner::new(HooksConfig { events }),
        memory_config: share::config::MemoryConfig::default(),
        frozen_chats: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        active_summary: std::sync::Arc::new(std::sync::Mutex::new(None)),
        language: "en".to_string(),
    })
    .await;

    assert!(sink
        .events()
        .iter()
        .any(|event| event == "HookEvent:Stop:Succeeded"));
    let output = std::fs::read_to_string(marker).unwrap();
    let parts: Vec<&str> = output.split('|').collect();
    assert_eq!(parts.len(), 3);
    let expected = workspace_root.path().canonicalize().unwrap();
    for part in parts {
        assert_eq!(std::fs::canonicalize(part).unwrap(), expected);
    }
}

#[tokio::test]
async fn test_process_chat_loop_drains_input_after_stop_hook_before_done() {
    let sink = RecordingSink::default();
    let queue = SequenceQueueDrainPort::new(vec![
        None,
        Some(vec!["stop-hook input".to_string()]),
        None,
        None,
    ]);

    process_chat_loop(ChatLoopContext {
        sink: sink.clone(),
        queue,
        input_events: EmptyInputEvents,
        client: Arc::new(provider::api::LlmClient::from_provider(Arc::new(
            TwoTurnProvider,
        ))),
        registry: Arc::new(ToolRegistry::new()),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        messages: vec![Message::user("hello")],
        context_size: 200_000,
        cwd: std::env::current_dir().unwrap(),
        workspace: project::api::WorkspaceService::new(std::env::current_dir().unwrap()),
        session_id: "test-session".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(share::tool::SessionReminders::new())),
        agent_runner: None,
        allow_all: false,
        cancel: Arc::new(Mutex::new(CancellationToken::new())),
        task_store: Arc::new(storage::api::TaskStore::new()),
        max_tool_concurrency: 1,
        max_agent_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: test_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        frozen_chats: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        active_summary: std::sync::Arc::new(std::sync::Mutex::new(None)),
        language: "en".to_string(),
    })
    .await;

    let events = sink.events();
    let queued_sync = events
        .iter()
        .position(|event| event == "MessagesSync:stop-hook input")
        .expect("queued input should be synced after Stop hook");
    let done = events
        .iter()
        .position(|event| event == "DoneWithDuration")
        .expect("loop should eventually finish");
    let handled_text = events
        .iter()
        .position(|event| event == "Text:handled queued input")
        .expect("queued input should trigger another LLM turn");

    assert!(queued_sync < handled_text);
    assert!(handled_text < done);
}

/// Hook 首次输出 `{"continue": false}` JSON (exit 0)，之后放行。
/// 用于验证 `continue:false` 被识别为阻断（#372 缺陷 1）。
fn continue_false_then_allow_hook_runner(flag_path: &std::path::Path) -> HookRunner {
    let flag_path_str = flag_path.to_string_lossy().to_string();
    let mut events = HashMap::new();
    events.insert(
        HookEvent::Stop,
        vec![HookEntry {
            matcher: String::new(),
            command: format!(
                "python3 -c 'import json,sys,pathlib; \
                 p=pathlib.Path(\"{flag_path}\"); \
                 sys.exit(0 if p.exists() else \
                 (p.parent.mkdir(parents=True, exist_ok=True), \
                 p.write_text(\"1\"), \
                 print(json.dumps({{\"continue\": False, \"stopReason\": \"must keep working\"}})), 0)[3])'",
                flag_path = flag_path_str,
            ),
            timeout: 5,
        }],
    );
    HookRunner::new(HooksConfig { events })
}

/// Hook 前 `n` 次阻断 (exit 2)，之后放行。用计数器文件跟踪调用次数。
fn block_n_times_hook_runner(counter_path: &std::path::Path, n: usize) -> HookRunner {
    let counter_path_str = counter_path.to_string_lossy().to_string();
    let mut events = HashMap::new();
    events.insert(
        HookEvent::Stop,
        vec![HookEntry {
            matcher: String::new(),
            command: format!(
                "python3 -c 'import pathlib,sys; \
                 p=pathlib.Path(\"{path}\"); \
                 c=int(p.read_text()) if p.exists() else 0; \
                 p.parent.mkdir(parents=True, exist_ok=True); \
                 p.write_text(str(c+1)); \
                 sys.exit(2 if c < {n} else 0)'",
                path = counter_path_str,
                n = n,
            ),
            timeout: 5,
        }],
    );
    HookRunner::new(HooksConfig { events })
}

/// Hook 每次都阻断 (exit 2)。用于验证连续阻断超上限强制停止（#372 缺陷 3）。
fn always_blocking_hook_runner() -> HookRunner {
    let mut events = HashMap::new();
    events.insert(
        HookEvent::Stop,
        vec![HookEntry {
            matcher: String::new(),
            command: "echo always blocked; exit 2".to_string(),
            timeout: 5,
        }],
    );
    HookRunner::new(HooksConfig { events })
}

#[tokio::test]
async fn test_continue_false_json_treated_as_block() {
    // #372 缺陷 1：Stop hook 输出 {"continue": false} (exit 0) 应被识别为阻断
    let flag_path = std::env::temp_dir().join(format!(
        "aemeath_continue_false_{}.flag",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_file(&flag_path);
    let sink = RecordingSink::default();

    process_chat_loop(ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![None, None, None, None]),
        input_events: EmptyInputEvents,
        client: Arc::new(provider::api::LlmClient::from_provider(Arc::new(
            SequenceProvider::new(vec!["first response", "second response"]),
        ))),
        registry: Arc::new(ToolRegistry::new()),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        messages: vec![Message::user("hello")],
        context_size: 200_000,
        cwd: std::env::current_dir().unwrap(),
        workspace: project::api::WorkspaceService::new(std::env::current_dir().unwrap()),
        session_id: "test-continue-false".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(share::tool::SessionReminders::new())),
        agent_runner: None,
        allow_all: false,
        cancel: Arc::new(Mutex::new(CancellationToken::new())),
        task_store: Arc::new(storage::api::TaskStore::new()),
        max_tool_concurrency: 1,
        max_agent_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: continue_false_then_allow_hook_runner(&flag_path),
        memory_config: share::config::MemoryConfig::default(),
        frozen_chats: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        active_summary: std::sync::Arc::new(std::sync::Mutex::new(None)),
        language: "en".to_string(),
    })
    .await;
    let _ = std::fs::remove_file(&flag_path);

    let events = sink.events();
    // continue:false 应触发 HookEvent:Stop:Blocked
    assert!(
        events.iter().any(|e| e == "HookEvent:Stop:Blocked"),
        "continue:false JSON should be recognized as block: {:?}",
        events
    );
    // 应有反馈注入（stopReason 内容）
    assert!(
        events.iter().any(|e| e.contains("must keep working")),
        "stopReason should appear in feedback: {:?}",
        events
    );
    // 应有第 2 次 LLM 响应（说明阻断后 loop 继续）
    assert!(
        events.iter().any(|e| e == "Text:second response"),
        "loop should continue to second LLM turn: {:?}",
        events
    );
    // 最终应完成
    assert_eq!(
        events
            .iter()
            .filter(|e| e.as_str() == "DoneWithDuration")
            .count(),
        1,
        "loop should finish after hook allows: {:?}",
        events
    );
}

#[tokio::test]
async fn test_stall_triggers_stop_hook_check() {
    // #372 缺陷 2：stall 终止前应调用 Stop hook，阻断则重置 detector 并继续
    let counter_path = std::env::temp_dir().join(format!(
        "aemeath_stall_hook_{}.counter",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_file(&counter_path);
    let sink = RecordingSink::default();

    // LLM 前 3 次返回相同输出（触发 stall），第 4 次返回不同输出
    // Stop hook 前 3 次阻断，第 4 次放行
    process_chat_loop(ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![None, None, None, None, None, None]),
        input_events: EmptyInputEvents,
        client: Arc::new(provider::api::LlmClient::from_provider(Arc::new(
            SequenceProvider::new(vec![
                "same output",
                "same output",
                "same output",
                "final ok",
            ]),
        ))),
        registry: Arc::new(ToolRegistry::new()),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        messages: vec![Message::user("hello")],
        context_size: 200_000,
        cwd: std::env::current_dir().unwrap(),
        workspace: project::api::WorkspaceService::new(std::env::current_dir().unwrap()),
        session_id: "test-stall-hook".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(share::tool::SessionReminders::new())),
        agent_runner: None,
        allow_all: false,
        cancel: Arc::new(Mutex::new(CancellationToken::new())),
        task_store: Arc::new(storage::api::TaskStore::new()),
        max_tool_concurrency: 1,
        max_agent_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: block_n_times_hook_runner(&counter_path, 3),
        memory_config: share::config::MemoryConfig::default(),
        frozen_chats: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        active_summary: std::sync::Arc::new(std::sync::Mutex::new(None)),
        language: "en".to_string(),
    })
    .await;
    let _ = std::fs::remove_file(&counter_path);

    let events = sink.events();
    // stall 分支被触发（有 stall 的 SystemMessage）
    assert!(
        events.iter().any(|e| e.contains("repetitive output")),
        "stall should be detected: {:?}",
        events
    );
    // stall 后 Stop hook 阻断，应有第 4 次 LLM 响应（说明 detector 重置并继续了）
    assert!(
        events.iter().any(|e| e == "Text:final ok"),
        "loop should continue after stall + Stop hook block: {:?}",
        events
    );
    // 最终应完成
    assert_eq!(
        events
            .iter()
            .filter(|e| e.as_str() == "DoneWithDuration")
            .count(),
        1,
        "loop should finish: {:?}",
        events
    );
}

/// Channel-backed input port: 投递事件经 `recv_next_input` 阻塞返回，
/// drop 发送端关闭通道使 `recv_next_input` 返回 `None`（= shutdown）。
#[derive(Clone)]
struct ChannelInputEvents {
    rx: Arc<tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<sdk::ChatInputEvent>>>,
}

impl ChannelInputEvents {
    fn new() -> (
        tokio::sync::mpsc::UnboundedSender<sdk::ChatInputEvent>,
        Self,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (
            tx,
            Self {
                rx: Arc::new(tokio::sync::Mutex::new(rx)),
            },
        )
    }
}

impl InputEventDrainPort for ChannelInputEvents {
    fn drain_input_events<'a>(&'a self) -> crate::business::chat::looping::InputEventFuture<'a> {
        Box::pin(async move {
            let mut rx = self.rx.lock().await;
            let mut events = Vec::new();
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
            events
        })
    }

    fn recv_next_input<'a>(&'a self) -> crate::business::chat::looping::InputEventOptFuture<'a> {
        Box::pin(async move {
            let mut rx = self.rx.lock().await;
            rx.recv().await
        })
    }
}

#[tokio::test]
async fn test_loop_persists_across_turns_until_shutdown() {
    // 常驻 loop 跨回合：喂 "first" → 完成回合 1 → 喂 "second" → 完成回合 2
    // → drop 发送端关闭通道 → loop shutdown 退出（不 hang）。
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();

    // 首条输入（回合 1 的用户消息）在 loop 启动前投递。
    input_tx
        .send(sdk::ChatInputEvent::user_message("first", Vec::new()))
        .unwrap();

    // driver：轮询 sink 事件，见到第 1 个 DoneWithDuration 后投递 "second"，
    // 见到第 2 个 DoneWithDuration 后 drop 发送端关闭通道触发 shutdown。
    let driver_sink = sink.clone();
    let driver = tokio::spawn(async move {
        // 等回合 1 完成（第 1 个 DoneWithDuration）。
        loop {
            let done_count = driver_sink
                .events()
                .iter()
                .filter(|event| event.as_str() == "DoneWithDuration")
                .count();
            if done_count >= 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        input_tx
            .send(sdk::ChatInputEvent::user_message("second", Vec::new()))
            .unwrap();
        // 等回合 2 完成（第 2 个 DoneWithDuration），再关闭通道。
        loop {
            let done_count = driver_sink
                .events()
                .iter()
                .filter(|event| event.as_str() == "DoneWithDuration")
                .count();
            if done_count >= 2 {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx); // 关闭通道 → recv_next_input 返回 None → shutdown
    });

    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        client: Arc::new(provider::api::LlmClient::from_provider(Arc::new(
            SequenceProvider::new(vec!["turn one final", "turn two final"]),
        ))),
        registry: Arc::new(ToolRegistry::new()),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        messages: Vec::new(),
        context_size: 200_000,
        cwd: std::env::current_dir().unwrap(),
        workspace: project::api::WorkspaceService::new(std::env::current_dir().unwrap()),
        session_id: "test-persistent-loop".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(share::tool::SessionReminders::new())),
        agent_runner: None,
        allow_all: false,
        cancel: Arc::new(Mutex::new(CancellationToken::new())),
        task_store: Arc::new(storage::api::TaskStore::new()),
        max_tool_concurrency: 1,
        max_agent_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: test_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        frozen_chats: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        active_summary: std::sync::Arc::new(std::sync::Mutex::new(None)),
        language: "en".to_string(),
    };

    // timeout 包裹：若 loop 在 shutdown 后未返回（hang），测试失败而非永久阻塞。
    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop 应在 shutdown 后返回，而非 hang");
    driver.await.unwrap();

    let events = sink.events();
    // 两回合各产生一个 DoneWithDuration（常驻 loop 跨回合存活）。
    let done_count = events
        .iter()
        .filter(|event| event.as_str() == "DoneWithDuration")
        .count();
    assert_eq!(
        done_count, 2,
        "常驻 loop 应跨两回合产生 2 个 DoneWithDuration: {events:?}"
    );
    // 两回合的用户消息均被处理（first 在回合 1，second 在回合 2）。
    assert!(
        events.iter().any(|event| event == "Text:turn one final"),
        "回合 1 应调用 LLM: {events:?}"
    );
    assert!(
        events.iter().any(|event| event == "Text:turn two final"),
        "回合 2 应调用 LLM: {events:?}"
    );
}

/// 每次 LLM 调用固定返回同一条极短回复，并 sleep 固定时长。
/// 用于 #390 A1 跨回合泄漏回归：
/// - 相同回复 → 若 `stall_detector` 跨回合泄漏，第 3 回合会误判 stall 停机。
/// - 固定 sleep → 若 `turn_start` 跨回合泄漏，`DoneWithDuration` 的 duration
///   会随回合累加（第 N 回合 ≈ N*sleep）；重置后每回合 ≈ 单次 sleep。
#[derive(Clone)]
struct IdenticalReplyProvider {
    reply: String,
    per_turn_delay: std::time::Duration,
}

impl IdenticalReplyProvider {
    fn new(reply: &str, per_turn_delay: std::time::Duration) -> Self {
        Self {
            reply: reply.to_string(),
            per_turn_delay,
        }
    }
}

#[async_trait]
impl LlmProvider for IdenticalReplyProvider {
    async fn stream_message(
        &self,
        _system: &[SystemBlock],
        _messages: &[Message],
        _tool_schemas: &[serde_json::Value],
        handler: &mut dyn StreamHandler,
        _cancel: &CancellationToken,
    ) -> Result<StreamResponse, provider::LlmError> {
        tokio::time::sleep(self.per_turn_delay).await;
        handler.on_text(&self.reply);
        Ok(StreamResponse {
            assistant_message: Message {
                role: share::message::Role::Assistant,
                content: vec![share::message::ContentBlock::Text {
                    text: self.reply.clone(),
                }],
                metadata: None,
            },
            usage: Usage {
                input_tokens: 1,
                output_tokens: 1,
                cached_tokens: None,
                cache_creation_tokens: None,
                reasoning_tokens: None,
            },
            stop_reason: StopReason::EndTurn,
        })
    }

    fn model_name(&self) -> &str {
        "test-model"
    }

    fn provider_name(&self) -> &str {
        "test-provider"
    }

    fn set_reasoning(&self, _enabled: bool) {}

    fn is_reasoning(&self) -> bool {
        false
    }
}

#[tokio::test]
async fn test_stall_detector_resets_across_user_turns() {
    // #390 A1 回归：常驻 loop 跨 3 个独立 USER 回合，每回合 LLM 返回**完全相同**的
    // 极短回复（"Done."）。重构前每个 `chat()` 持有独立 StallDetector，跨回合不可能
    // 累积；A1 把 loop 改为常驻后 detector 在 loop 外只构造一次，3 个相同回复会在第 3
    // 回合触发 "[agent loop stopped: LLM is producing repetitive output]" 误报。
    //
    // 修复：每个新 USER 回合开始时重置 stall_detector（同时重置 turn_start）。
    // 期望：3 个回合都正常完成（3 个 DoneWithDuration），无 stall 停机 SystemMessage。
    //
    // 同时验证 turn_start 不跨回合泄漏（Finding 2）：每回合 LLM sleep 固定时长，
    // 若 turn_start 泄漏，第 3 回合 duration 会累积成 ~3*delay；重置后各回合 ~delay。
    let per_turn_delay = std::time::Duration::from_millis(40);
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();

    // 回合 1 的用户消息在 loop 启动前投递。
    input_tx
        .send(sdk::ChatInputEvent::user_message("turn-1", Vec::new()))
        .unwrap();

    // driver：每见到一个新的 DoneWithDuration 就投递下一回合输入；最后无条件关闭通道。
    //
    // **必须有界**：修复前第 3 回合会误触发 stall 停机，loop 直接 break（不产生第 3 个
    // DoneWithDuration）。若 driver 无界轮询「done_count>=3」会永久阻塞，掩盖失败为 hang。
    // 改为「有界等待 + stall 信号提前退出 + 无条件 drop 发送端」，使修复前以**断言失败**
    // （而非 hang）暴露 RED；修复后 3 回合正常完成 → GREEN。
    let driver_sink = sink.clone();
    let driver = tokio::spawn(async move {
        // 有界等待 done_count 达到 target 或观测到 stall 停机（提前退出）。
        async fn wait_for(sink: &RecordingSink, target: usize) {
            for _ in 0..400 {
                let events = sink.events();
                let done_count = events
                    .iter()
                    .filter(|event| event.as_str() == "DoneWithDuration")
                    .count();
                let stalled = events.iter().any(|e| e.contains("repetitive output"));
                if done_count >= target || stalled {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        }
        for (next, target) in [("turn-2", 1usize), ("turn-3", 2usize)] {
            wait_for(&driver_sink, target).await;
            let _ = input_tx.send(sdk::ChatInputEvent::user_message(next, Vec::new()));
        }
        wait_for(&driver_sink, 3).await;
        drop(input_tx); // 无条件关闭通道 → recv_next_input 返回 None → shutdown
    });

    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        client: Arc::new(provider::api::LlmClient::from_provider(Arc::new(
            IdenticalReplyProvider::new("Done.", per_turn_delay),
        ))),
        registry: Arc::new(ToolRegistry::new()),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        messages: Vec::new(),
        context_size: 200_000,
        cwd: std::env::current_dir().unwrap(),
        workspace: project::api::WorkspaceService::new(std::env::current_dir().unwrap()),
        session_id: "test-stall-reset-across-turns".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(share::tool::SessionReminders::new())),
        agent_runner: None,
        allow_all: false,
        cancel: Arc::new(Mutex::new(CancellationToken::new())),
        task_store: Arc::new(storage::api::TaskStore::new()),
        max_tool_concurrency: 1,
        max_agent_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: test_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        frozen_chats: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        active_summary: std::sync::Arc::new(std::sync::Mutex::new(None)),
        language: "en".to_string(),
    };

    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop 应在 shutdown 后返回，而非 hang");
    driver.await.unwrap();

    let events = sink.events();

    // Finding 1：3 个相同短回复回合均正常完成，无 stall 误报。
    assert!(
        !events.iter().any(|e| e.contains("repetitive output")),
        "相同短回复不应跨独立 USER 回合触发 stall 停机: {events:?}"
    );
    let done_count = events
        .iter()
        .filter(|event| event.as_str() == "DoneWithDuration")
        .count();
    assert_eq!(
        done_count, 3,
        "3 个独立 USER 回合应各产生 1 个 DoneWithDuration: {events:?}"
    );

    // Finding 2（轻量断言）：turn_start 每回合重置，duration 不随回合累积。
    // 若未重置，第 3 回合 duration ≈ 3*delay；重置后各回合 ≈ delay。
    // 取「第 3 回合 < 前两回合 duration 之和」作为非累积的稳健判据。
    let durations = sink.done_durations();
    assert_eq!(durations.len(), 3, "应有 3 个 duration: {durations:?}");
    assert!(
        durations[2] < durations[0] + durations[1],
        "turn_start 应每回合重置：第 3 回合 duration ({:?}) 不应累积到 >= 前两回合之和 ({:?} + {:?})",
        durations[2],
        durations[0],
        durations[1]
    );
}

#[test]
fn test_is_new_user_turn_message_genuine_user() {
    // 正常路径：真正的新用户输入 → true（应触发 per-turn 重置）。
    let msg = Message::user("turn-1");
    assert!(super::loop_runner::is_new_user_turn_message(Some(&msg)));
}

#[test]
fn test_is_new_user_turn_message_tool_result_is_not_new_turn() {
    // 边界：工具结果消息 role 虽为 User，但 has_tool_results() 为真 → false
    // （对应回合内工具轮次再迭代，NEVER 视为新回合，必须保留单回合 stall 检测）。
    let tool_msg = Message::tool_results_rich(vec![(
        "tool-id-1".to_string(),
        "ok".to_string(),
        serde_json::Value::String("ok".to_string()),
        false,
        Vec::new(),
    )]);
    assert!(tool_msg.has_tool_results());
    assert!(!super::loop_runner::is_new_user_turn_message(Some(
        &tool_msg
    )));
}

#[test]
fn test_is_new_user_turn_message_system_generated_is_not_new_turn() {
    // 边界：stop-hook 阻断注入的 system-generated 用户消息 → false（回合仍在继续）。
    let sys_msg = Message::system_generated_user("<system-reminder>keep working</system-reminder>");
    assert!(!super::loop_runner::is_new_user_turn_message(Some(
        &sys_msg
    )));
}

#[test]
fn test_is_new_user_turn_message_assistant_or_empty_is_not_new_turn() {
    // 错误/空路径：assistant 消息或空 messages → false。
    let assistant = Message {
        role: Role::Assistant,
        content: vec![share::message::ContentBlock::Text {
            text: "hi".to_string(),
        }],
        metadata: None,
    };
    assert!(!super::loop_runner::is_new_user_turn_message(Some(
        &assistant
    )));
    assert!(!super::loop_runner::is_new_user_turn_message(None));
}

/// 记录每次 LLM 调用时 messages 中最后一条用户消息的文本。
/// 用于确定性地检测「空闲期命令触发的陈旧历史空回合」：
/// 合法回合每次调用前都有新用户消息（"first" / "second"）；
/// bug 触发的空回合会在 "first" 之后、"second" 之前再次以 "first" 为末条用户消息调用 LLM。
#[derive(Clone)]
struct RecordingProvider {
    calls: Arc<Mutex<Vec<String>>>,
}

impl RecordingProvider {
    fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl LlmProvider for RecordingProvider {
    async fn stream_message(
        &self,
        _system: &[SystemBlock],
        messages: &[Message],
        _tool_schemas: &[serde_json::Value],
        handler: &mut dyn StreamHandler,
        _cancel: &CancellationToken,
    ) -> Result<StreamResponse, provider::LlmError> {
        let last_user = messages
            .iter()
            .rev()
            .find(|message| message.role == Role::User)
            .map(|message| message.text_content())
            .unwrap_or_default();
        self.calls.lock().unwrap().push(last_user.clone());
        let text = format!("response to {last_user}");
        handler.on_text(&text);
        Ok(StreamResponse {
            assistant_message: Message {
                role: Role::Assistant,
                content: vec![share::message::ContentBlock::Text { text }],
                metadata: None,
            },
            usage: Usage {
                input_tokens: 1,
                output_tokens: 1,
                cached_tokens: None,
                cache_creation_tokens: None,
                reasoning_tokens: None,
            },
            stop_reason: StopReason::EndTurn,
        })
    }

    fn model_name(&self) -> &str {
        "test-model"
    }

    fn provider_name(&self) -> &str {
        "test-provider"
    }

    fn set_reasoning(&self, _enabled: bool) {}

    fn is_reasoning(&self) -> bool {
        false
    }
}

#[tokio::test]
async fn test_idle_control_command_does_not_run_spurious_turn() {
    // #390 A1（Important）：空闲期收到一个不 append 任何用户消息的 ControlCommand
    // （如 /save / /model / /provider，apply_gate 返回 Proceed 且 appended_user_messages=0）
    // 时，loop 必须保持空闲，NEVER 在陈旧历史上跑空回合。随后投递真实 UserMessage
    // 才恢复运行并产出恰好一个新回合；drop 发送端关闭通道后 loop shutdown 退出。
    //
    // 确定性检测：RecordingProvider 记录每次 LLM 调用时末条用户消息文本。合法序列恰为
    // ["first", "second"]；bug 会插入一次陈旧 "first" 调用 → 序列变
    // ["first", "first", "second"]（断言失败）。
    //
    // 同步屏障：driver 等 DoneWithDuration（回合 1 完成、loop 已进入空闲态、下一处通道
    // 消费者必为 await_idle_input）后再投递 /save，确保命令落到 idle 臂而非被回合终结
    // 路径的 BeforeFinish gate 提前 drain 掉。
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();
    let provider = RecordingProvider::new();

    // 首条输入（回合 1 的用户消息）在 loop 启动前投递。
    input_tx
        .send(sdk::ChatInputEvent::user_message("first", Vec::new()))
        .unwrap();

    let driver_sink = sink.clone();
    let driver_provider = provider.clone();
    let driver = tokio::spawn(async move {
        // 等回合 1 完成（第 1 个 DoneWithDuration）→ loop 已进入空闲态阻塞于
        // await_idle_input；此时投递的 /save 必由 idle 臂消费。
        loop {
            let done_count = driver_sink
                .events()
                .iter()
                .filter(|event| event.as_str() == "DoneWithDuration")
                .count();
            if done_count >= 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        // 空闲期投递一个 ControlCommand（/save：SideEffect，append 0 条用户消息）。
        input_tx
            .send(sdk::ChatInputEvent::ControlCommand {
                raw: "/save".to_string(),
            })
            .unwrap();
        // 给 loop 充分调度机会去（错误地）消费命令、退出空闲、跑陈旧历史空回合。
        // 若 bug 存在，这会产生第 2 次 LLM 调用（末条用户消息仍为 "first"）。
        for _ in 0..200 {
            tokio::task::yield_now().await;
        }
        // 命令处理后 LLM 调用数仍应为 1（保持空闲，无空回合）。
        assert_eq!(
            driver_provider.calls(),
            vec!["first".to_string()],
            "空闲期单独 ControlCommand 不得触发 LLM 调用（应仍只有 first 一次）"
        );

        // 现在投递真实用户消息，应恢复运行并完成回合 2（第 2 次 LLM 调用）。
        input_tx
            .send(sdk::ChatInputEvent::user_message("second", Vec::new()))
            .unwrap();
        loop {
            if driver_provider.calls().len() >= 2 {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx); // 关闭通道 → recv_next_input 返回 None → shutdown
    });

    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        client: Arc::new(provider::api::LlmClient::from_provider(Arc::new(
            provider.clone(),
        ))),
        registry: Arc::new(ToolRegistry::new()),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        messages: Vec::new(),
        context_size: 200_000,
        cwd: std::env::current_dir().unwrap(),
        workspace: project::api::WorkspaceService::new(std::env::current_dir().unwrap()),
        session_id: "test-idle-control-command".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(share::tool::SessionReminders::new())),
        agent_runner: None,
        allow_all: false,
        cancel: Arc::new(Mutex::new(CancellationToken::new())),
        task_store: Arc::new(storage::api::TaskStore::new()),
        max_tool_concurrency: 1,
        max_agent_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: test_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        frozen_chats: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        active_summary: std::sync::Arc::new(std::sync::Mutex::new(None)),
        language: "en".to_string(),
    };

    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop 应在 shutdown 后返回，而非 hang");
    driver.await.unwrap();

    // 关键断言（确定性）：LLM 调用序列恰为 ["first", "second"]。
    // bug 会插入陈旧 "first" 调用 → ["first", "first", "second"]，断言失败。
    assert_eq!(
        provider.calls(),
        vec!["first".to_string(), "second".to_string()],
        "LLM 应恰好被两条真实用户消息触发；命令不得引发陈旧历史空回合"
    );
    // 命令的 raw 文本绝不应作为 user message 进入消息历史。
    assert!(
        sink.synced_messages()
            .into_iter()
            .flatten()
            .all(|message| message.text_content() != "/save"),
        "ControlCommand 永不作为 user message 进入历史: {:?}",
        sink.events()
    );
}

#[tokio::test]
async fn test_stop_hook_block_limit_stops_loop() {
    // #372 缺陷 3：Stop hook 连续阻断超过 MAX_STOP_HOOK_BLOCKS(5) 强制停止
    let sink = RecordingSink::default();

    // 每次返回不同输出避免 stall；Stop hook 每次阻断
    process_chat_loop(ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![None, None, None, None, None, None, None, None]),
        input_events: EmptyInputEvents,
        client: Arc::new(provider::api::LlmClient::from_provider(Arc::new(
            SequenceProvider::new(vec!["r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8"]),
        ))),
        registry: Arc::new(ToolRegistry::new()),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        messages: vec![Message::user("hello")],
        context_size: 200_000,
        cwd: std::env::current_dir().unwrap(),
        workspace: project::api::WorkspaceService::new(std::env::current_dir().unwrap()),
        session_id: "test-block-limit".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(share::tool::SessionReminders::new())),
        agent_runner: None,
        allow_all: false,
        cancel: Arc::new(Mutex::new(CancellationToken::new())),
        task_store: Arc::new(storage::api::TaskStore::new()),
        max_tool_concurrency: 1,
        max_agent_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: always_blocking_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        frozen_chats: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        active_summary: std::sync::Arc::new(std::sync::Mutex::new(None)),
        language: "en".to_string(),
    })
    .await;

    let events = sink.events();
    // 超过上限应发出 SystemMessage 提示
    assert!(
        events
            .iter()
            .any(|e| e.contains("stop hook blocked 5 times in a row")),
        "should emit block-limit SystemMessage: {:?}",
        events
    );
}

/// 第 1 次调用阻塞直到 cancel 被触发后返回 `LlmError::Cancelled`，
/// 第 2 次及以后调用立即返回正常响应。用于模拟「回合进行中被取消、
/// 随后新回合正常完成」的场景。
#[derive(Clone)]
struct CancellableThenNormalProvider {
    calls: Arc<Mutex<usize>>,
}

impl CancellableThenNormalProvider {
    fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(0)),
        }
    }
}

#[async_trait]
impl LlmProvider for CancellableThenNormalProvider {
    async fn stream_message(
        &self,
        _system: &[SystemBlock],
        _messages: &[Message],
        _tool_schemas: &[serde_json::Value],
        handler: &mut dyn StreamHandler,
        cancel: &CancellationToken,
    ) -> Result<StreamResponse, provider::LlmError> {
        let call_index = {
            let mut guard = self.calls.lock().unwrap();
            let idx = *guard;
            *guard += 1;
            idx
        };
        if call_index == 0 {
            // 回合 1：阻塞等待 cancel，被取消后返回 Cancelled（模拟 provider 侧取消）。
            cancel.cancelled().await;
            return Err(provider::api::LlmError::Cancelled);
        }
        // 回合 2+：正常完成（关键：此时若 token 未重置，会立刻 Cancelled）。
        if cancel.is_cancelled() {
            return Err(provider::api::LlmError::Cancelled);
        }
        let text = format!("turn {} final", call_index + 1);
        handler.on_text(&text);
        Ok(StreamResponse {
            assistant_message: Message {
                role: Role::Assistant,
                content: vec![share::message::ContentBlock::Text { text }],
                metadata: None,
            },
            usage: Usage {
                input_tokens: 1,
                output_tokens: 1,
                cached_tokens: None,
                cache_creation_tokens: None,
                reasoning_tokens: None,
            },
            stop_reason: StopReason::EndTurn,
        })
    }

    fn model_name(&self) -> &str {
        "test-model"
    }

    fn provider_name(&self) -> &str {
        "test-provider"
    }

    fn set_reasoning(&self, _enabled: bool) {}

    fn is_reasoning(&self) -> bool {
        false
    }
}

#[tokio::test]
async fn test_cancel_aborts_turn_then_returns_to_idle() {
    // #390 A1 Task 3：回合进行中 cancel → 发出 Cancelled、回滚本回合消息、
    // **回到空闲**（不退 loop）；随后投递新 UserMessage → 新回合正常完成；
    // 最后 drop 发送端关闭通道 → loop shutdown 退出。
    //
    // 取消令牌生命周期（并发关键）：cancel 槽改为 Arc<Mutex<CancellationToken>>，
    // 外部（模拟 cancel_impl）锁槽 .cancel() 当前 token；loop 处理 cancel 后
    // 将槽重置为新 token 供下回合。若未重置，回合 2 的 LLM 调用会立即 Cancelled。
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();
    // 共享 cancel 槽：loop 与「外部取消者」共用，模拟 RuntimeHandle.current_cancel。
    let cancel_slot = Arc::new(Mutex::new(CancellationToken::new()));
    let provider = CancellableThenNormalProvider::new();

    // 首条输入（回合 1 的用户消息）在 loop 启动前投递。
    input_tx
        .send(sdk::ChatInputEvent::user_message("first", Vec::new()))
        .unwrap();

    let driver_sink = sink.clone();
    let driver_provider = provider.clone();
    let driver_slot = cancel_slot.clone();
    let driver = tokio::spawn(async move {
        // 等回合 1 的 LLM 调用真正开始（call count >= 1），此时 provider 正阻塞于
        // cancel.cancelled()。再触发取消，确保取消落在「回合进行中」。
        loop {
            if *driver_provider.calls.lock().unwrap() >= 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        // 外部取消（模拟 cancel_impl）：锁槽，取消当前 live token。
        driver_slot.lock().unwrap().cancel();

        // 等回合 1 被取消（出现 Cancelled 事件）。
        loop {
            if driver_sink.events().iter().any(|e| e == "Cancelled") {
                break;
            }
            tokio::task::yield_now().await;
        }

        // 投递「陈旧的第二次 cancel」：若 loop 在重置后又把这个取消错误地
        // 应用到下回合，会污染回合 2。这里用 input 通道的 Cancel 事件模拟
        // 空闲期的 stale cancel（应被 idle 臂吞掉、保持空闲）。
        input_tx.send(sdk::ChatInputEvent::Cancel).unwrap();
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }

        // 投递真实用户消息：应恢复运行并完成回合 2（新 token 未被污染）。
        input_tx
            .send(sdk::ChatInputEvent::user_message("second", Vec::new()))
            .unwrap();
        loop {
            let done_count = driver_sink
                .events()
                .iter()
                .filter(|e| e.as_str() == "DoneWithDuration")
                .count();
            if done_count >= 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx); // 关闭通道 → recv_next_input 返回 None → shutdown
    });

    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        client: Arc::new(provider::api::LlmClient::from_provider(Arc::new(
            provider.clone(),
        ))),
        registry: Arc::new(ToolRegistry::new()),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        messages: Vec::new(),
        context_size: 200_000,
        cwd: std::env::current_dir().unwrap(),
        workspace: project::api::WorkspaceService::new(std::env::current_dir().unwrap()),
        session_id: "test-cancel-then-idle".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(share::tool::SessionReminders::new())),
        agent_runner: None,
        allow_all: false,
        cancel: cancel_slot.clone(),
        task_store: Arc::new(storage::api::TaskStore::new()),
        max_tool_concurrency: 1,
        max_agent_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: test_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        frozen_chats: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        active_summary: std::sync::Arc::new(std::sync::Mutex::new(None)),
        language: "en".to_string(),
    };

    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop 应在 cancel→idle→新回合→shutdown 后返回，而非 hang");
    driver.await.unwrap();

    let events = sink.events();
    // 回合 1 被取消：发出 Cancelled。
    assert!(
        events.iter().any(|e| e == "Cancelled"),
        "回合 1 进行中 cancel 应发出 Cancelled 事件: {events:?}"
    );
    // cancel 后未退 loop：回合 2 正常完成，恰好一个 DoneWithDuration。
    let done_count = events
        .iter()
        .filter(|e| e.as_str() == "DoneWithDuration")
        .count();
    assert_eq!(
        done_count, 1,
        "cancel 应回空闲、不退 loop；新回合应正常完成产出 1 个 DoneWithDuration: {events:?}"
    );
    // 回合 2 的 LLM 响应文本出现（说明新 token 未被陈旧 cancel 污染）。
    assert!(
        events.iter().any(|e| e == "Text:turn 2 final"),
        "重置 token 后回合 2 应正常调用 LLM 并完成: {events:?}"
    );
    // 回滚断言（per-turn 基线语义，与重构前 per-`chat()` 一致）：cancel 把基线设在
    // 「本回合用户消息已入、assistant 未产生」处，故取消只回滚本回合的 partial
    // assistant/tool 输出，**保留本回合用户消息 "first"**。检查取消回滚那次
    // MessagesSync（Cancelled 之前最近一次）应含 "first" 但不含任何 assistant 文本。
    let cancelled_idx = events
        .iter()
        .position(|e| e == "Cancelled")
        .expect("应有 Cancelled 事件");
    let syncs_before_cancel = events[..cancelled_idx]
        .iter()
        .filter(|e| e.starts_with("MessagesSync"))
        .count();
    assert!(
        syncs_before_cancel >= 1,
        "Cancelled 前应至少有一次 MessagesSync（回滚同步）: {events:?}"
    );
    let rollback_snapshot = &sink.synced_messages()[syncs_before_cancel - 1];
    let rollback_texts: Vec<String> = rollback_snapshot.iter().map(|m| m.text_content()).collect();
    assert!(
        rollback_texts.iter().any(|t| t == "first"),
        "per-turn 基线设在用户消息之后：回合 1 取消应保留本回合用户消息 'first': {rollback_texts:?}"
    );
    assert!(
        rollback_texts.iter().all(|t| t != "turn 2 final"),
        "回合 1 取消的回滚快照不应含回合 2 的 assistant 输出: {rollback_texts:?}"
    );
}

/// 回合 1 正常完成、回合 2 进行中阻塞等待 cancel、回合 3 正常完成。
/// 用于验证「取消晚于首回合的回合时，先前已完成回合的消息必须存活」。
#[derive(Clone)]
struct CompleteThenCancellableProvider {
    calls: Arc<Mutex<usize>>,
}

impl CompleteThenCancellableProvider {
    fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(0)),
        }
    }
}

#[async_trait]
impl LlmProvider for CompleteThenCancellableProvider {
    async fn stream_message(
        &self,
        _system: &[SystemBlock],
        _messages: &[Message],
        _tool_schemas: &[serde_json::Value],
        handler: &mut dyn StreamHandler,
        cancel: &CancellationToken,
    ) -> Result<StreamResponse, provider::LlmError> {
        let call_index = {
            let mut guard = self.calls.lock().unwrap();
            let idx = *guard;
            *guard += 1;
            idx
        };
        // 回合 2（call_index == 1）：阻塞等 cancel，被取消后返回 Cancelled。
        // 回合 1 / 回合 3：正常完成（token 已重置，不应被陈旧 cancel 污染）。
        if call_index == 1 {
            cancel.cancelled().await;
            return Err(provider::api::LlmError::Cancelled);
        }
        if cancel.is_cancelled() {
            return Err(provider::api::LlmError::Cancelled);
        }
        let text = format!("turn {} assistant", call_index + 1);
        handler.on_text(&text);
        Ok(StreamResponse {
            assistant_message: Message {
                role: Role::Assistant,
                content: vec![share::message::ContentBlock::Text { text }],
                metadata: None,
            },
            usage: Usage {
                input_tokens: 1,
                output_tokens: 1,
                cached_tokens: None,
                cache_creation_tokens: None,
                reasoning_tokens: None,
            },
            stop_reason: StopReason::EndTurn,
        })
    }

    fn model_name(&self) -> &str {
        "test-model"
    }

    fn provider_name(&self) -> &str {
        "test-provider"
    }

    fn set_reasoning(&self, _enabled: bool) {}

    fn is_reasoning(&self) -> bool {
        false
    }
}

#[tokio::test]
async fn test_cancel_later_turn_preserves_completed_prior_turns() {
    // #390 A1（Important，data-loss）：常驻 loop 中先前回合已完成的消息累积在同一个
    // `messages` Vec。若 cancel 回滚用「loop 启动时的固定基线」，取消任何「非首回合」会把
    // 先前已完成回合一并截掉（整段对话坍缩到首条）。修复后 cancel 改用 per-turn 基线，
    // 只回滚当前回合的 partial 输出，先前已完成回合的 user+assistant 必须存活。
    //
    // 时序：
    //   回合 1：投递 "turn1-user" → LLM 正常返回 "turn 1 assistant" → 完成、进入空闲。
    //   回合 2：投递 "turn2-user" → LLM 阻塞等 cancel → 外部 cancel → 回滚回空闲。
    //   回合 3：投递 "turn3-user" → LLM 正常完成（新 token 未被污染）→ shutdown。
    //
    // 关键断言：回合 2 被取消后的 MessagesSync 中，回合 1 的 "turn1-user" 与
    // "turn 1 assistant" 必须仍存在（pre-fix `truncate(loop_start_baseline=0)` 会删除它们）。
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();
    // 共享 cancel 槽：模拟外部 cancel_impl。
    let cancel_slot = Arc::new(Mutex::new(CancellationToken::new()));
    let provider = CompleteThenCancellableProvider::new();

    // 回合 1 的用户消息在 loop 启动前投递。
    input_tx
        .send(sdk::ChatInputEvent::user_message("turn1-user", Vec::new()))
        .unwrap();

    let driver_sink = sink.clone();
    let driver_provider = provider.clone();
    let driver_slot = cancel_slot.clone();
    let driver = tokio::spawn(async move {
        // 等回合 1 完成（第 1 个 DoneWithDuration），loop 进入空闲。
        loop {
            let done_count = driver_sink
                .events()
                .iter()
                .filter(|e| e.as_str() == "DoneWithDuration")
                .count();
            if done_count >= 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        // 投递回合 2 的用户消息：恢复运行，LLM（第 2 次调用）阻塞等 cancel。
        input_tx
            .send(sdk::ChatInputEvent::user_message("turn2-user", Vec::new()))
            .unwrap();
        // 等回合 2 的 LLM 调用真正开始（call count >= 2），确保 cancel 落在「回合进行中」。
        loop {
            if *driver_provider.calls.lock().unwrap() >= 2 {
                break;
            }
            tokio::task::yield_now().await;
        }
        // 外部 cancel：锁槽取消当前 live token。
        driver_slot.lock().unwrap().cancel();
        // 等回合 2 被取消（出现 Cancelled 事件）。
        loop {
            if driver_sink.events().iter().any(|e| e == "Cancelled") {
                break;
            }
            tokio::task::yield_now().await;
        }
        // 投递回合 3 的用户消息：恢复运行并完成回合 3。
        input_tx
            .send(sdk::ChatInputEvent::user_message("turn3-user", Vec::new()))
            .unwrap();
        loop {
            let done_count = driver_sink
                .events()
                .iter()
                .filter(|e| e.as_str() == "DoneWithDuration")
                .count();
            if done_count >= 2 {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx); // 关闭通道 → shutdown
    });

    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        client: Arc::new(provider::api::LlmClient::from_provider(Arc::new(
            provider.clone(),
        ))),
        registry: Arc::new(ToolRegistry::new()),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        messages: Vec::new(),
        context_size: 200_000,
        cwd: std::env::current_dir().unwrap(),
        workspace: project::api::WorkspaceService::new(std::env::current_dir().unwrap()),
        session_id: "test-cancel-preserves-prior-turns".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(share::tool::SessionReminders::new())),
        agent_runner: None,
        allow_all: false,
        cancel: cancel_slot.clone(),
        task_store: Arc::new(storage::api::TaskStore::new()),
        max_tool_concurrency: 1,
        max_agent_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: test_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        frozen_chats: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        active_summary: std::sync::Arc::new(std::sync::Mutex::new(None)),
        language: "en".to_string(),
    };

    // timeout 包裹：未 shutdown（hang）则测试失败而非永久阻塞。
    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop 应在 回合1→回合2取消→回合3→shutdown 后返回，而非 hang");
    driver.await.unwrap();

    let events = sink.events();
    // 回合 2 被取消：发出 Cancelled。
    assert!(
        events.iter().any(|e| e == "Cancelled"),
        "回合 2 进行中 cancel 应发出 Cancelled 事件: {events:?}"
    );

    // 关键回归断言：回合 2 取消后第一次 MessagesSync（cancel_to_idle 内的回滚同步）
    // 必须仍包含回合 1 的 user+assistant。pre-fix 用 loop_start_baseline=0 回滚 →
    // 这两条被删除 → 断言失败；修复后 per-turn 基线保留它们 → 通过。
    let cancelled_idx = events
        .iter()
        .position(|e| e == "Cancelled")
        .expect("应有 Cancelled 事件");
    // cancel_to_idle 先发 MessagesSync（回滚后）再发 Cancelled；取 Cancelled 之前最近一次
    // MessagesSync 对应的快照即「取消回滚后的 messages」。
    let syncs = sink.synced_messages();
    // 找到「取消回滚」那次 sync：它是 events 中 Cancelled 之前最后一个 MessagesSync。
    let messages_sync_count_before_cancel = events[..cancelled_idx]
        .iter()
        .filter(|e| e.starts_with("MessagesSync"))
        .count();
    assert!(
        messages_sync_count_before_cancel >= 1,
        "Cancelled 前应至少有一次 MessagesSync（回滚同步）: {events:?}"
    );
    let rollback_snapshot = &syncs[messages_sync_count_before_cancel - 1];
    let texts: Vec<String> = rollback_snapshot.iter().map(|m| m.text_content()).collect();
    assert!(
        texts.iter().any(|t| t == "turn1-user"),
        "回合 2 取消不得删除回合 1 的用户消息 'turn1-user': {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "turn 1 assistant"),
        "回合 2 取消不得删除回合 1 的 assistant 响应 'turn 1 assistant': {texts:?}"
    );
    // 回合 2 的 partial 输出（用户消息 'turn2-user' 之后无 assistant，因 LLM 被取消）：
    // 'turn2-user' 应被回滚（与重构前语义一致：保留用户消息这一点见下），实际本回合
    // 用户消息也属当前回合内容、应回滚到 per-turn 基线之内。本回合用户消息保留与否取决于
    // 捕获点：本实现把基线设在「用户消息已入、assistant 未产生」处，故回合 2 用户消息保留。
    assert!(
        texts.iter().any(|t| t == "turn2-user"),
        "per-turn 基线设在用户消息之后：回合 2 取消应保留本回合用户消息 'turn2-user': {texts:?}"
    );

    // cancel 后未退 loop：回合 3 正常完成，总计 2 个 DoneWithDuration。
    let done_count = events
        .iter()
        .filter(|e| e.as_str() == "DoneWithDuration")
        .count();
    assert_eq!(
        done_count, 2,
        "回合 1 与回合 3 各产出一个 DoneWithDuration（cancel 不退 loop）: {events:?}"
    );
    // 回合 3 的 assistant 响应出现（新 token 未被陈旧 cancel 污染）。
    assert!(
        events.iter().any(|e| e == "Text:turn 3 assistant"),
        "重置 token 后回合 3 应正常调用 LLM 并完成: {events:?}"
    );
}

/// Task 4：loop 顶部无待答回合时必须先 idle-wait，收到 UserMessage 后才调 LLM。
///
/// 用一个计数 provider 追踪 LLM 调用次数。在投递任何输入前，先给 loop 充分调度
/// 机会（yield 若干轮），断言此时 LLM 调用数为 0（loop 正处于 loop-top idle-wait）。
/// 随后投递一条 UserMessage，等 DoneWithDuration 出现，断言恰好一次 LLM 调用。
/// 最后 drop 发送端，loop shutdown 退出（无 hang）。
///
/// RED 阶段：当前 loop 在 `process_chat_loop` 顶部直接进入 BeforeLlm gate，
/// 即使 messages 为空也不会 idle-wait，调用 LLM 时 messages_for_api 为空
/// 导致 LLM provider 被调用（或回合逻辑异常）。实现 `has_pending_user_turn` +
/// 顶部 idle 门后，测试应变绿。
#[tokio::test]
async fn test_chat_impl_idle_until_first_input_event() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    // === 计数 provider：记录 LLM 调用次数 ===
    #[derive(Clone)]
    struct CountingProvider {
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }
    #[async_trait]
    impl LlmProvider for CountingProvider {
        async fn stream_message(
            &self,
            _system: &[SystemBlock],
            _messages: &[Message],
            _tool_schemas: &[serde_json::Value],
            handler: &mut dyn StreamHandler,
            _cancel: &CancellationToken,
        ) -> Result<StreamResponse, provider::LlmError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let text = "hi response";
            handler.on_text(text);
            Ok(StreamResponse {
                assistant_message: Message {
                    role: Role::Assistant,
                    content: vec![share::message::ContentBlock::Text {
                        text: text.to_string(),
                    }],
                    metadata: None,
                },
                usage: Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                    cached_tokens: None,
                    cache_creation_tokens: None,
                    reasoning_tokens: None,
                },
                stop_reason: StopReason::EndTurn,
            })
        }

        fn model_name(&self) -> &str {
            "test-model"
        }

        fn provider_name(&self) -> &str {
            "test-provider"
        }

        fn set_reasoning(&self, _enabled: bool) {}

        fn is_reasoning(&self) -> bool {
            false
        }
    }

    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();
    let call_counter = Arc::new(AtomicUsize::new(0));
    let provider = CountingProvider {
        calls: call_counter.clone(),
    };

    // driver：先给 loop 充分调度机会（不投递任何输入），断言 LLM 未被调用；
    // 再投递 UserMessage("hi")，等 DoneWithDuration；最后关闭通道触发 shutdown。
    let driver_sink = sink.clone();
    let driver_counter = call_counter.clone();
    let driver = tokio::spawn(async move {
        // 给 loop 充分调度机会（200 次 yield）——若 loop 不 idle-wait，
        // 会直接进入 BeforeLlm gate 并调用 LLM。
        for _ in 0..200 {
            tokio::task::yield_now().await;
        }

        // 关键断言 RED：当前 loop 无 loop-top idle，此时 LLM 已被调用（test 失败）。
        // 实现后 loop 在 loop-top idle-wait 阻塞，LLM 调用数应为 0。
        assert_eq!(
            driver_counter.load(Ordering::SeqCst),
            0,
            "无待答用户回合时 loop 必须 idle-wait，不得立即调用 LLM"
        );
        // 此时也不应有 Done（无 LLM 调用必然无完成）。
        assert!(
            driver_sink.events().iter().all(|e| e != "DoneWithDuration"),
            "未投递输入前不得出现 DoneWithDuration: {:?}",
            driver_sink.events()
        );
        // Finding 2：idle gate 已前置到回合头之前，空 seed 启动在收到首条输入前
        // 不得发出任何 TurnChanged（否则是「回合 1 / 处理中」假信号）。
        assert!(
            driver_sink
                .events()
                .iter()
                .all(|e| !e.starts_with("TurnChanged")),
            "未投递输入前不得发出 TurnChanged（前置 idle gate 避免假回合）: {:?}",
            driver_sink.events()
        );

        // 投递首条 UserMessage，loop 应从 idle 恢复、运行一个回合、发出 DoneWithDuration。
        input_tx
            .send(sdk::ChatInputEvent::user_message("hi", Vec::new()))
            .unwrap();

        // 等 DoneWithDuration（最多 10s）。
        loop {
            if driver_sink.events().iter().any(|e| e == "DoneWithDuration") {
                break;
            }
            tokio::task::yield_now().await;
        }

        // 恰好一次 LLM 调用。
        assert_eq!(
            driver_counter.load(Ordering::SeqCst),
            1,
            "投递 UserMessage 后应恰好调用一次 LLM"
        );

        drop(input_tx); // 关闭通道 → recv_next_input 返回 None → shutdown
    });

    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        client: Arc::new(provider::api::LlmClient::from_provider(Arc::new(provider))),
        registry: Arc::new(ToolRegistry::new()),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        messages: Vec::new(), // 空 messages：无待答回合，loop 必须先 idle-wait
        context_size: 200_000,
        cwd: std::env::current_dir().unwrap(),
        workspace: project::api::WorkspaceService::new(std::env::current_dir().unwrap()),
        session_id: "test-idle-until-first-input".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(share::tool::SessionReminders::new())),
        agent_runner: None,
        allow_all: false,
        cancel: Arc::new(Mutex::new(CancellationToken::new())),
        task_store: Arc::new(storage::api::TaskStore::new()),
        max_tool_concurrency: 1,
        max_agent_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: test_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        frozen_chats: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        active_summary: std::sync::Arc::new(std::sync::Mutex::new(None)),
        language: "en".to_string(),
    };

    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop 应在 shutdown 后返回，而非 hang");
    driver.await.unwrap();

    let events = sink.events();
    // 整个生命周期内恰好一次 LLM 调用（"hi" 回合）。
    assert_eq!(
        call_counter.load(Ordering::SeqCst),
        1,
        "全程应恰好一次 LLM 调用: {events:?}"
    );
    // 产出一个 DoneWithDuration（一个完整回合）。
    assert_eq!(
        events
            .iter()
            .filter(|e| e.as_str() == "DoneWithDuration")
            .count(),
        1,
        "应产出恰好一个 DoneWithDuration: {events:?}"
    );
    // Finding 2：全程恰好一次 TurnChanged，且回合编号为 1（首个真实回合 = 1，
    // 前置 idle gate 不会消耗回合号）。空 seed 启动不产生假回合。
    let turn_changes: Vec<&String> = events
        .iter()
        .filter(|e| e.starts_with("TurnChanged"))
        .collect();
    assert_eq!(
        turn_changes,
        vec![&"TurnChanged:1".to_string()],
        "空 seed 启动应恰好发出一次 TurnChanged:1（首个真实回合编号为 1）: {events:?}"
    );
}

/// Finding 2 专项：空 seed 启动时，loop-top idle gate 位于回合头之前，
/// 收到首条真实输入前 NEVER 发出任何回合信号（`TurnChanged`）或 turn 边界副作用。
///
/// 与 `test_chat_impl_idle_until_first_input_event` 的区别：本测试以 `RecordingSink`
/// 捕获「投递首条输入的那一刻」的事件快照，**确定性**断言该快照内不含 `TurnChanged`
/// （前置 idle gate 的直接观测）。若 gate 仍位于 `TurnChanged` 之后（回归），快照会
/// 含 `TurnChanged:1` 假信号 → 断言失败。随后真实输入触发恰好一个回合（`TurnChanged:1`
/// 在输入之后出现），drop 发送端关闭通道使 loop shutdown 退出。
#[tokio::test]
async fn test_empty_seed_start_emits_no_turn_signal_before_first_input() {
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();
    let provider = RecordingProvider::new();

    // 注意：loop 启动前 NEVER 投递任何输入（空 seed + 无 pending）→ loop 必先 idle-wait。
    let driver_sink = sink.clone();
    let driver_provider = provider.clone();
    let driver = tokio::spawn(async move {
        // 给 loop 充分调度机会去（错误地）跑回合头、发 TurnChanged。
        for _ in 0..200 {
            tokio::task::yield_now().await;
        }
        // 捕获「投递首条输入前」的事件快照。
        let snapshot_before_input = driver_sink.events();
        assert!(
            snapshot_before_input
                .iter()
                .all(|e| !e.starts_with("TurnChanged")),
            "空 seed 启动在收到首条输入前不得发出 TurnChanged（前置 idle gate 避免假回合）: {snapshot_before_input:?}"
        );
        assert_eq!(
            driver_provider.calls().len(),
            0,
            "收到首条输入前不得调用 LLM: {snapshot_before_input:?}"
        );

        // 投递首条真实用户消息 → 恢复运行、产出恰好一个回合。
        input_tx
            .send(sdk::ChatInputEvent::user_message("hello", Vec::new()))
            .unwrap();
        loop {
            if driver_sink.events().iter().any(|e| e == "DoneWithDuration") {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx); // 关闭通道 → shutdown
    });

    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        client: Arc::new(provider::api::LlmClient::from_provider(Arc::new(
            provider.clone(),
        ))),
        registry: Arc::new(ToolRegistry::new()),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        messages: Vec::new(), // 空 seed：无待答回合，loop 必须先 idle-wait
        context_size: 200_000,
        cwd: std::env::current_dir().unwrap(),
        workspace: project::api::WorkspaceService::new(std::env::current_dir().unwrap()),
        session_id: "test-no-turn-signal-before-first-input".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(share::tool::SessionReminders::new())),
        agent_runner: None,
        allow_all: false,
        cancel: Arc::new(Mutex::new(CancellationToken::new())),
        task_store: Arc::new(storage::api::TaskStore::new()),
        max_tool_concurrency: 1,
        max_agent_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: test_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        frozen_chats: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        active_summary: std::sync::Arc::new(std::sync::Mutex::new(None)),
        language: "en".to_string(),
    };

    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop 应在 shutdown 后返回，而非 hang");
    driver.await.unwrap();

    let events = sink.events();
    // 首条输入触发后：恰好一次 TurnChanged，编号 1（首个真实回合 = 1）。
    let turn_changes: Vec<&String> = events
        .iter()
        .filter(|e| e.starts_with("TurnChanged"))
        .collect();
    assert_eq!(
        turn_changes,
        vec![&"TurnChanged:1".to_string()],
        "真实输入后应恰好发出一次 TurnChanged:1: {events:?}"
    );
    // TurnChanged 必在首次 LLM 调用（输入处理后）之前的同一回合内；整体恰好一次 LLM 调用。
    assert_eq!(
        provider.calls(),
        vec!["hello".to_string()],
        "全程应恰好被首条真实用户消息触发一次 LLM 调用: {events:?}"
    );
}
