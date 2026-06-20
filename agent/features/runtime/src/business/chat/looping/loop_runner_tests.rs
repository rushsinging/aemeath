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
            RuntimeStreamEvent::DoneWithDuration { .. } => "DoneWithDuration".to_string(),
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
        };
        self.events.lock().unwrap().push(name);
    }

    fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }

    fn synced_messages(&self) -> Vec<Vec<Message>> {
        self.messages_syncs.lock().unwrap().clone()
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
    HookRunner::new(HooksConfig { events }, ".".to_string())
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
    HookRunner::new(HooksConfig { events }, ".".to_string())
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
async fn test_process_chat_loop_uses_workspace_working_root_for_stop_hook_env() {
    let sink = RecordingSink::default();
    let path_base = tempfile::tempdir().unwrap();
    let working_root = tempfile::tempdir().unwrap();
    let marker = path_base.path().join("stop-hook-env.txt");
    let marker_path = marker.display().to_string();
    let workspace_dto = crate::business::session::PersistedWorkspaceContext {
        path_base: path_base.path().display().to_string(),
        working_root: working_root.path().display().to_string(),
        context_stack: vec![crate::business::session::PersistedWorkspaceFrame {
            path_base: path_base.path().display().to_string(),
            working_root: path_base.path().display().to_string(),
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
        hook_runner: HookRunner::new(
            HooksConfig { events },
            path_base.path().display().to_string(),
        ),
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
    let expected = working_root.path().canonicalize().unwrap();
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
    HookRunner::new(HooksConfig { events }, ".".to_string())
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
    HookRunner::new(HooksConfig { events }, ".".to_string())
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
    HookRunner::new(HooksConfig { events }, ".".to_string())
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
            return Err(provider::LlmError::Cancelled);
        }
        // 回合 2+：正常完成（关键：此时若 token 未重置，会立刻 Cancelled）。
        if cancel.is_cancelled() {
            return Err(provider::LlmError::Cancelled);
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
    // 回滚断言：回合 1 取消后 "first" 用户消息被回滚，不应残留在最终历史里
    // 与回合 2 的 assistant 响应共存。检查最终一次 MessagesSync 不含 "first"。
    let last_sync = sink.synced_messages().into_iter().next_back();
    if let Some(messages) = last_sync {
        assert!(
            messages.iter().all(|m| m.text_content() != "first"),
            "回合 1 取消应回滚 'first' 用户消息: {:?}",
            messages
                .iter()
                .map(|m| m.text_content())
                .collect::<Vec<_>>()
        );
    }
}
