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
        cancel: CancellationToken::new(),
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
        cancel: CancellationToken::new(),
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
        cancel: CancellationToken::new(),
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
        cancel: CancellationToken::new(),
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
        cancel: CancellationToken::new(),
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
        cancel: CancellationToken::new(),
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
        cancel: CancellationToken::new(),
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
