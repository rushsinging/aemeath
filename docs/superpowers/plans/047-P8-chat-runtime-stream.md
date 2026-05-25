# Feature 47 Chat Runtime Stream Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 `apps/cli/src/tui/app/stream*` 中的后台 Chat stream loop 首轮迁移到 `crates/runtime`，让 TUI 保留渲染、输入与事件 adapter。

**Architecture:** 首轮采用低风险 adapter 迁移：`UiEvent` 暂时仍定义在 CLI，runtime 通过泛型 event sink/queue drain port 调用 TUI adapter，避免重写渲染和 AskUserQuestion。`runtime::api::chat` 承载 `SpawnContext`、tool context 构造、LLM stream、tool round、compact、finalize 等 turn 编排；`apps/cli` 只负责 spawn、event forwarding 和 UI state 更新。

**Tech Stack:** Rust workspace、Tokio、`runtime::api`、`aemeath_core` message/tool/hook/task/session 类型、现有 TUI `UiEvent` adapter。

---

## File Structure

- Create: `crates/runtime/src/chat/looping/mod.rs`
  - 新 runtime 模块入口，导出 `ChatLoopContext`、`ChatEventSink`、`QueueDrainPort`、`process_chat_loop`。
- Create: `crates/runtime/src/chat/looping/events.rs`
  - 定义 runtime 侧最小事件 trait：发送文本、工具调用、系统消息、usage、done、message sync 等。
- Create: `crates/runtime/src/chat/looping/queue.rs`
  - 保存 queue drain 与 append queued input 逻辑，使用 `QueueDrainPort` 而不是 CLI `UiEvent`。
- Create: `crates/runtime/src/chat/looping/stream_handler.rs`
  - 从 CLI `TuiStreamHandler` 抽为通用 `RuntimeStreamHandler<S>`，通过 event sink 转发 streaming event。
- Create: `crates/runtime/src/chat/looping/loop_runner.rs`
  - 保存从 `process_in_background` 搬来的主循环。
- Modify: `crates/runtime/src/lib.rs`
  - 增加 `pub mod chat;`。
- Modify: `crates/runtime/src/api.rs`
  - 增加 `pub use crate::chat;`。
- Modify: `apps/cli/src/tui/app/processing.rs`
  - `SpawnContext` 保留在 CLI 或薄包装 runtime context；`spawn_processing` 调用 `runtime::api::chat::process_chat_loop`。
- Modify: `apps/cli/src/tui/app/stream.rs`
  - 首轮变薄或移除主循环，只保留 CLI-only adapter re-export；最终不再定义 `process_in_background`。
- Modify: `apps/cli/src/tui/app/stream/queue.rs`
  - 保留测试可迁移到 runtime；CLI adapter 实现 `QueueDrainPort`。
- Modify: `docs/feature/active.md`
  - 更新 #47 当前推进说明，记录 Feature #47 Chat runtime stream loop 首轮迁移状态。
- Modify: `docs/feature/specs/047-ddd-redesign.md`
  - 更新 checkpoint 注释，说明 TUI adapter 仍留 CLI，turn loop 开始下沉 runtime。

---

### Task 1: 建立 runtime Chat loop 事件与 queue port

**Files:**
- Create: `crates/runtime/src/chat/looping/mod.rs`
- Create: `crates/runtime/src/chat/looping/events.rs`
- Create: `crates/runtime/src/chat/looping/queue.rs`
- Modify: `crates/runtime/src/lib.rs`
- Modify: `crates/runtime/src/api.rs`

- [ ] **Step 1: 创建 runtime 模块入口**

Create `crates/runtime/src/chat/looping/mod.rs`:

```rust
mod events;
mod queue;

pub use events::{RuntimeStreamEvent, ChatEventSink};
pub use queue::{append_queued_input, QueueDrainPort};
```

- [ ] **Step 2: 定义 runtime stream event sink trait**

Create `crates/runtime/src/chat/looping/events.rs`:

```rust
use crate::api::core::message::Message;
use crate::api::core::tool::ImageData;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

pub type EventFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

#[derive(Debug, Clone)]
pub enum RuntimeStreamEvent {
    Text(String),
    Thinking(String),
    TextBlockComplete(String),
    ToolCallStart { name: String, index: usize },
    ToolArgumentsDelta { index: usize, name: String, partial_args: String },
    ToolCall { id: String, name: String, summary: String },
    ToolResult { id: String, tool_name: String, output: String, is_error: bool, images: Vec<ImageData> },
    SystemMessage(String),
    Error(String),
    Usage { input: u32, output: u32, last_input: u32, elapsed_secs: f64 },
    MessagesSync(Vec<Message>),
    Done,
    DoneWithDuration(Duration),
    Cancelled,
    LiveTps(f64),
    StopFailureHook { system_message: Option<String>, additional_context: Option<String> },
}

pub trait ChatEventSink: Clone + Send + Sync + 'static {
    fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a>;
    fn try_send_event(&self, event: RuntimeStreamEvent) -> Result<(), String>;
}
```

- [ ] **Step 3: 定义 queue drain port 与 append 逻辑**

Create `crates/runtime/src/chat/looping/queue.rs`:

```rust
use super::{RuntimeStreamEvent, ChatEventSink};
use crate::api::core::message::Message;
use std::future::Future;
use std::pin::Pin;

pub type QueueFuture<'a> = Pin<Box<dyn Future<Output = Option<Vec<String>>> + Send + 'a>>;

pub trait QueueDrainPort: Clone + Send + Sync + 'static {
    fn drain_queued_input<'a>(&'a self) -> QueueFuture<'a>;
}

pub async fn append_queued_input<Q, S>(
    queue: &Q,
    sink: &S,
    messages: &mut Vec<Message>,
) -> bool
where
    Q: QueueDrainPort,
    S: ChatEventSink,
{
    let Some(queued) = queue.drain_queued_input().await else {
        return false;
    };
    for input in queued {
        messages.push(Message::user(input));
    }
    sink.send_event(RuntimeStreamEvent::MessagesSync(messages.clone())).await;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::core::message::Message;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct TestQueue(Arc<Mutex<Option<Vec<String>>>>);

    impl QueueDrainPort for TestQueue {
        fn drain_queued_input<'a>(&'a self) -> QueueFuture<'a> {
            Box::pin(async move { self.0.lock().unwrap().take().filter(|v| !v.is_empty()) })
        }
    }

    #[derive(Clone, Default)]
    struct TestSink(Arc<Mutex<Vec<RuntimeStreamEvent>>>);

    impl ChatEventSink for TestSink {
        fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
            Box::pin(async move { self.0.lock().unwrap().push(event); })
        }

        fn try_send_event(&self, event: RuntimeStreamEvent) -> Result<(), String> {
            self.0.lock().unwrap().push(event);
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_append_queued_input_appends_and_syncs() {
        let queue = TestQueue(Arc::new(Mutex::new(Some(vec!["queued".to_string()]))));
        let sink = TestSink::default();
        let mut messages = vec![Message::user("first")];

        let appended = append_queued_input(&queue, &sink, &mut messages).await;

        assert!(appended);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].text_content(), "queued");
        assert!(matches!(sink.0.lock().unwrap().last(), Some(RuntimeStreamEvent::MessagesSync(msgs)) if msgs.len() == 2));
    }

    #[tokio::test]
    async fn test_append_queued_input_empty_returns_false() {
        let queue = TestQueue(Arc::new(Mutex::new(Some(Vec::new()))));
        let sink = TestSink::default();
        let mut messages = vec![Message::user("first")];

        let appended = append_queued_input(&queue, &sink, &mut messages).await;

        assert!(!appended);
        assert_eq!(messages.len(), 1);
        assert!(sink.0.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_append_queued_input_none_returns_false() {
        let queue = TestQueue(Arc::new(Mutex::new(None)));
        let sink = TestSink::default();
        let mut messages = vec![Message::user("first")];

        let appended = append_queued_input(&queue, &sink, &mut messages).await;

        assert!(!appended);
        assert_eq!(messages.len(), 1);
        assert!(sink.0.lock().unwrap().is_empty());
    }
}
```

- [ ] **Step 4: 导出 runtime 模块**

Modify `crates/runtime/src/lib.rs`:

```rust
pub mod agent_runner;
pub mod api;
pub mod bootstrap;
pub mod chat;
pub mod chat;
```

Modify `crates/runtime/src/api.rs`:

```rust
pub use crate::agent_runner;
pub use crate::bootstrap;
pub use crate::chat;
pub use crate::chat;
pub use aemeath_core as core;
pub use audit;
pub use hook;
pub use policy;
pub use project;
pub use prompt;
pub use provider;
pub use storage;
pub use tools;
```

- [ ] **Step 5: 验证 Task 1**

Run:

```bash
cargo fmt --all -- --check
cargo test -p runtime chat/looping::queue
cargo check -p runtime
```

Expected: all pass.

---

### Task 2: 迁移 stream handler 到 runtime

**Files:**
- Create: `crates/runtime/src/chat/looping/stream_handler.rs`
- Modify: `crates/runtime/src/chat/looping/mod.rs`
- Modify: `apps/cli/src/tui/app/stream/handler.rs`

- [ ] **Step 1: 创建 runtime stream handler**

Create `crates/runtime/src/chat/looping/stream_handler.rs`:

```rust
use super::{RuntimeStreamEvent, ChatEventSink};
use crate::api::provider::StreamHandler;

pub struct RuntimeStreamHandler<S>
where
    S: ChatEventSink,
{
    pub sink: S,
    pub first_text_time: Option<std::time::Instant>,
    pub total_chars: usize,
    pub last_tps_update: std::time::Instant,
}

impl<S> RuntimeStreamHandler<S>
where
    S: ChatEventSink,
{
    pub fn new(sink: S) -> Self {
        let now = std::time::Instant::now();
        Self {
            sink,
            first_text_time: None,
            total_chars: 0,
            last_tps_update: now,
        }
    }
}

impl<S> StreamHandler for RuntimeStreamHandler<S>
where
    S: ChatEventSink,
{
    fn on_text(&mut self, text: &str) {
        if let Err(e) = self.sink.try_send_event(RuntimeStreamEvent::Text(text.to_string())) {
            log::warn!("UI channel full, dropped Text event ({} bytes): {e}", text.len());
        }
        let now = std::time::Instant::now();
        if self.first_text_time.is_none() {
            self.first_text_time = Some(now);
            self.last_tps_update = now;
        }
        self.total_chars += text.len();
        if now.duration_since(self.last_tps_update).as_millis() >= 200 {
            self.last_tps_update = now;
            if let Some(start) = self.first_text_time {
                let elapsed = now.duration_since(start).as_secs_f64();
                if elapsed > 0.0 {
                    let estimated_tokens = self.total_chars as f64 / 3.0;
                    let tps = estimated_tokens / elapsed;
                    let _ = self.sink.try_send_event(RuntimeStreamEvent::LiveTps(tps));
                }
            }
        }
    }

    fn on_tool_use_start(&mut self, name: &str, index: usize) {
        if let Err(e) = self.sink.try_send_event(RuntimeStreamEvent::ToolCallStart { name: name.to_string(), index }) {
            log::warn!("UI channel full, dropped ToolCallStart({name}[{index}]): {e}");
        }
    }

    fn on_error(&mut self, error: &str) {
        if let Err(e) = self.sink.try_send_event(RuntimeStreamEvent::SystemMessage(format!("[warn] {}", error))) {
            log::warn!("UI channel full, dropped SystemMessage: {e}");
        }
    }

    fn on_text_block_complete(&mut self, text: &str) {
        if let Err(e) = self.sink.try_send_event(RuntimeStreamEvent::TextBlockComplete(text.to_string())) {
            log::warn!("UI channel full, dropped TextBlockComplete ({} bytes): {e}", text.len());
        }
    }

    fn on_thinking(&mut self, text: &str) {
        if let Err(e) = self.sink.try_send_event(RuntimeStreamEvent::Thinking(text.to_string())) {
            log::warn!("UI channel full, dropped Thinking event ({} bytes): {e}", text.len());
        }
    }

    fn on_tool_arguments_delta(&mut self, index: usize, name: &str, partial_args: &str) {
        if let Err(e) = self.sink.try_send_event(RuntimeStreamEvent::ToolArgumentsDelta {
            index,
            name: name.to_string(),
            partial_args: partial_args.to_string(),
        }) {
            log::warn!("UI channel full, dropped ToolArgumentsDelta({name}[{index}]): {e}");
        }
    }
}
```

- [ ] **Step 2: 导出 handler**

Modify `crates/runtime/src/chat/looping/mod.rs`:

```rust
mod events;
mod queue;
mod stream_handler;

pub use events::{RuntimeStreamEvent, ChatEventSink};
pub use queue::{append_queued_input, QueueDrainPort};
pub use stream_handler::RuntimeStreamHandler;
```

- [ ] **Step 3: CLI handler 改为 runtime handler 类型别名**

Replace `apps/cli/src/tui/app/stream/handler.rs` with:

```rust
use crate::tui::app::processing::TuiEventSink;

pub(crate) type TuiStreamHandler = ::runtime::api::chat::RuntimeStreamHandler<TuiEventSink>;
```

- [ ] **Step 4: 验证 Task 2**

Run:

```bash
cargo fmt --all -- --check
cargo check -p runtime
cargo check -p cli
```

Expected: all pass.

---

### Task 3: 在 CLI 实现 runtime event sink 与 queue adapter

**Files:**
- Modify: `apps/cli/src/tui/app/processing.rs`

- [ ] **Step 1: 在 processing.rs 添加 adapter 类型**

Modify `apps/cli/src/tui/app/processing.rs` by adding after imports:

```rust
#[derive(Clone)]
pub(crate) struct TuiEventSink {
    tx: mpsc::Sender<UiEvent>,
}

impl TuiEventSink {
    pub(crate) fn new(tx: mpsc::Sender<UiEvent>) -> Self {
        Self { tx }
    }

    fn map_event(event: ::runtime::api::chat::RuntimeStreamEvent) -> UiEvent {
        match event {
            ::runtime::api::chat::RuntimeStreamEvent::Text(text) => UiEvent::Text(text),
            ::runtime::api::chat::RuntimeStreamEvent::Thinking(text) => UiEvent::Thinking(text),
            ::runtime::api::chat::RuntimeStreamEvent::TextBlockComplete(text) => UiEvent::TextBlockComplete(text),
            ::runtime::api::chat::RuntimeStreamEvent::ToolCallStart { name, index } => UiEvent::ToolCallStart { name, index },
            ::runtime::api::chat::RuntimeStreamEvent::ToolArgumentsDelta { index, name, partial_args } => UiEvent::ToolArgumentsDelta { index, name, partial_args },
            ::runtime::api::chat::RuntimeStreamEvent::ToolCall { id, name, summary } => UiEvent::ToolCall { id, name, summary },
            ::runtime::api::chat::RuntimeStreamEvent::ToolResult { id, tool_name, output, is_error, images } => UiEvent::ToolResult { id, tool_name, output, is_error, images },
            ::runtime::api::chat::RuntimeStreamEvent::SystemMessage(message) => UiEvent::SystemMessage(message),
            ::runtime::api::chat::RuntimeStreamEvent::Error(error) => UiEvent::Error(error),
            ::runtime::api::chat::RuntimeStreamEvent::Usage { input, output, last_input, elapsed_secs } => UiEvent::Usage { input, output, last_input, elapsed_secs },
            ::runtime::api::chat::RuntimeStreamEvent::MessagesSync(messages) => UiEvent::MessagesSync(messages),
            ::runtime::api::chat::RuntimeStreamEvent::Done => UiEvent::Done,
            ::runtime::api::chat::RuntimeStreamEvent::DoneWithDuration(duration) => UiEvent::DoneWithDuration(duration),
            ::runtime::api::chat::RuntimeStreamEvent::Cancelled => UiEvent::Cancelled,
            ::runtime::api::chat::RuntimeStreamEvent::LiveTps(tps) => UiEvent::LiveTps(tps),
            ::runtime::api::chat::RuntimeStreamEvent::StopFailureHook { system_message, additional_context } => UiEvent::StopFailureHook { system_message, additional_context },
        }
    }
}

impl ::runtime::api::chat::ChatEventSink for TuiEventSink {
    fn send_event<'a>(&'a self, event: ::runtime::api::chat::RuntimeStreamEvent) -> ::runtime::api::chat::EventFuture<'a> {
        Box::pin(async move {
            let _ = self.tx.send(Self::map_event(event)).await;
        })
    }

    fn try_send_event(&self, event: ::runtime::api::chat::RuntimeStreamEvent) -> Result<(), String> {
        self.tx.try_send(Self::map_event(event)).map_err(|e| e.to_string())
    }
}

#[derive(Clone)]
pub(crate) struct TuiQueueDrainPort {
    tx: mpsc::Sender<UiEvent>,
}

impl TuiQueueDrainPort {
    pub(crate) fn new(tx: mpsc::Sender<UiEvent>) -> Self {
        Self { tx }
    }
}

impl ::runtime::api::chat::QueueDrainPort for TuiQueueDrainPort {
    fn drain_queued_input<'a>(&'a self) -> ::runtime::api::chat::QueueFuture<'a> {
        Box::pin(async move {
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            if self.tx.send(UiEvent::DrainQueuedInput { reply_tx }).await.is_err() {
                return None;
            }
            match reply_rx.await {
                Ok(queued) if !queued.is_empty() => Some(queued),
                _ => None,
            }
        })
    }
}
```

- [ ] **Step 2: 验证 adapter 编译**

Run:

```bash
cargo fmt --all -- --check
cargo check -p cli
```

Expected: all pass. If `EventFuture` or `QueueFuture` is private, ensure Task 1 exports them from `crates/runtime/src/chat/looping/mod.rs`:

```rust
pub use events::{EventFuture, RuntimeStreamEvent, ChatEventSink};
pub use queue::{append_queued_input, QueueDrainPort, QueueFuture};
```

---

### Task 4: 首轮迁移 process_in_background 到 runtime

**Files:**
- Create: `crates/runtime/src/chat/looping/loop_runner.rs`
- Modify: `crates/runtime/src/chat/looping/mod.rs`
- Modify: `apps/cli/src/tui/app/processing.rs`
- Modify: `apps/cli/src/tui/app/stream.rs`

- [ ] **Step 1: 创建 runtime context 类型**

Create `crates/runtime/src/chat/looping/loop_runner.rs` with the context struct first:

```rust
use super::{append_queued_input, QueueDrainPort, RuntimeStreamEvent, RuntimeStreamHandler, ChatEventSink};
use crate::api::agent_runner::{AgentRunOutcome, AgentRunStatus};
use crate::api::core::agent::Agent;
use crate::api::core::message::Message;
use crate::api::core::tool::{ToolContext, ToolRegistry};
use crate::api::provider::types::StopReason;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

pub struct ChatLoopContext<S, Q>
where
    S: ChatEventSink,
    Q: QueueDrainPort,
{
    pub sink: S,
    pub queue: Q,
    pub client: Arc<crate::api::provider::client::LlmClient>,
    pub registry: Arc<ToolRegistry>,
    pub system_blocks: Vec<crate::api::provider::types::SystemBlock>,
    pub system_prompt_text: String,
    pub user_context: String,
    pub messages: Vec<Message>,
    pub context_size: usize,
    pub cwd: PathBuf,
    pub workspace_context: Option<crate::api::core::session::WorkspaceContext>,
    pub session_id: String,
    pub read_files: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    pub session_reminders: Arc<std::sync::Mutex<crate::api::core::memory::SessionReminders>>,
    pub agent_runner: Option<Arc<dyn crate::api::core::tool::AgentRunner>>,
    pub allow_all: bool,
    pub interrupted: Arc<AtomicBool>,
    pub cancel: CancellationToken,
    pub task_store: Arc<crate::api::core::task::TaskStore>,
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
    pub hook_runner: crate::api::core::hook::HookRunner,
    pub memory_config: crate::api::core::config::MemoryConfig,
    pub json_logger: Option<Arc<std::sync::Mutex<crate::api::core::logging::JsonLogger>>>,
}
```

- [ ] **Step 2: 搬迁主循环，暂时保留 CLI-only helper 回调**

Continue in `crates/runtime/src/chat/looping/loop_runner.rs` by adding a minimal compile target that constructs `ToolContext` and emits cancellation. Do not move every stream submodule in this step:

```rust
pub async fn process_chat_loop<S, Q>(ctx: ChatLoopContext<S, Q>)
where
    S: ChatEventSink,
    Q: QueueDrainPort,
{
    let ChatLoopContext {
        sink,
        queue,
        client,
        registry,
        system_blocks,
        system_prompt_text,
        user_context,
        mut messages,
        context_size,
        cwd,
        workspace_context,
        session_id,
        read_files,
        session_reminders,
        agent_runner,
        allow_all,
        interrupted,
        cancel,
        task_store,
        max_tool_concurrency,
        max_agent_concurrency,
        agent_semaphore,
        hook_runner,
        memory_config,
        json_logger,
    } = ctx;

    let (cwd, working_root, path_base, context_stack) = if let Some(workspace) = workspace_context {
        (
            PathBuf::from(&workspace.path_base),
            Arc::new(Mutex::new(PathBuf::from(&workspace.working_root))),
            Arc::new(Mutex::new(PathBuf::from(&workspace.path_base))),
            Arc::new(Mutex::new(
                workspace
                    .context_stack
                    .into_iter()
                    .map(|entry| crate::api::core::worktree::WorkingContext {
                        path_base: PathBuf::from(entry.path_base),
                        working_root: PathBuf::from(entry.working_root),
                    })
                    .collect(),
            )),
        )
    } else {
        let (cwd, working_root, path_base) = ToolContext::new_working_paths(cwd.clone());
        (cwd, working_root, path_base, Arc::new(Mutex::new(Vec::new())))
    };

    hook_runner.set_project_dir(cwd.display().to_string());
    let agent = Agent {
        registry: &registry,
        ctx: ToolContext {
            cwd: cwd.clone(),
            working_root,
            path_base,
            cancel: cancel.clone(),
            read_files: read_files.clone(),
            agent_runner: agent_runner.clone(),
            session_reminders: Some(session_reminders.clone()),
            memory_config: memory_config.clone(),
            plan_mode: None,
            allow_all,
            max_tool_concurrency,
            max_agent_concurrency,
            agent_semaphore,
            progress_tx: None,
            parent_session_id: Some(session_id.clone()),
            context_stack,
        },
    };

    let messages_at_start = messages.len();
    let mut last_api_input_tokens: u64 = 0;
    let turn_start = std::time::Instant::now();
    let mut turn_count: usize = 0;

    loop {
        turn_count += 1;
        let tool_schemas = registry.schemas();
        let tool_schema_tokens = crate::api::core::compact::estimate_tool_schemas_tokens(&tool_schemas);

        if interrupted.load(Ordering::Relaxed) {
            interrupted.store(false, Ordering::Relaxed);
            if append_queued_input(&queue, &sink, &mut messages).await {
                continue;
            }
            messages.truncate(messages_at_start);
            sink.send_event(RuntimeStreamEvent::MessagesSync(messages.clone())).await;
            sink.send_event(RuntimeStreamEvent::Cancelled).await;
            sink.send_event(RuntimeStreamEvent::Done).await;
            break;
        }

        let messages_for_api: Vec<Message> = {
            let mut api_msgs = Vec::new();
            if !user_context.is_empty() {
                api_msgs.push(Message::user(format!(
                    "<system-reminder>\nAs you answer the user's questions, you can use the following context:\n# claudeMd\n{user_context}\n\nIMPORTANT: this context may or may not be relevant to your tasks. You should not respond to this context unless it is highly relevant to your task.\n</system-reminder>"
                )));
            }
            api_msgs.extend(messages.iter().cloned());
            api_msgs
        };

        let mut handler = RuntimeStreamHandler::new(sink.clone());
        let api_start = std::time::Instant::now();
        let response = client
            .stream_message(&system_blocks, &messages_for_api, &tool_schemas, &mut handler, &cancel)
            .await;
        let api_elapsed = api_start.elapsed().as_secs_f64();

        match response {
            Ok(resp) => {
                last_api_input_tokens = resp.usage.input_tokens as u64;
                sink.send_event(RuntimeStreamEvent::Usage {
                    input: resp.usage.input_tokens,
                    output: resp.usage.output_tokens,
                    last_input: resp.usage.input_tokens,
                    elapsed_secs: api_elapsed,
                }).await;
                messages.push(resp.assistant_message.clone());
                sink.send_event(RuntimeStreamEvent::MessagesSync(messages.clone())).await;

                let tool_calls = Agent::extract_tool_calls(&resp.assistant_message);
                if tool_calls.is_empty() || resp.stop_reason == StopReason::EndTurn {
                    if append_queued_input(&queue, &sink, &mut messages).await {
                        continue;
                    }
                    sink.send_event(RuntimeStreamEvent::DoneWithDuration(turn_start.elapsed())).await;
                    break;
                }

                sink.send_event(RuntimeStreamEvent::SystemMessage(format!(
                    "[runtime migration checkpoint: {} tool calls detected; detailed tool execution still handled by CLI in next task]",
                    tool_calls.len()
                ))).await;
                let _ = (&agent, tool_schema_tokens, last_api_input_tokens, context_size, task_store, hook_runner, json_logger);
                sink.send_event(RuntimeStreamEvent::DoneWithDuration(turn_start.elapsed())).await;
                break;
            }
            Err(e) => {
                sink.send_event(RuntimeStreamEvent::Error(e.to_string())).await;
                if append_queued_input(&queue, &sink, &mut messages).await {
                    continue;
                }
                sink.send_event(RuntimeStreamEvent::Done).await;
                break;
            }
        }
    }
}
```

Important: This step is an intermediate checkpoint only if compile/testing shows no user-visible path regression. If this minimal loop changes tool execution behavior, do not commit; instead move all existing helper modules in Task 5 before enabling `spawn_processing` to call runtime.

- [ ] **Step 3: 导出 context 与 runner**

Modify `crates/runtime/src/chat/looping/mod.rs`:

```rust
mod events;
mod loop_runner;
mod queue;
mod stream_handler;

pub use events::{EventFuture, RuntimeStreamEvent, ChatEventSink};
pub use loop_runner::{process_chat_loop, ChatLoopContext};
pub use queue::{append_queued_input, QueueDrainPort, QueueFuture};
pub use stream_handler::RuntimeStreamHandler;
```

- [ ] **Step 4: 暂不切换 CLI，先验证 runtime 编译**

Run:

```bash
cargo fmt --all -- --check
cargo check -p runtime
```

Expected: pass.

---

### Task 5: 完整搬迁 stream helper 并切换 CLI spawn

**Files:**
- Move/Modify: `apps/cli/src/tui/app/stream/{agent_calls,ask_user,compact,finalize,hook_ui,input_log,llm_log,non_agent,permissions,post_batch,queue,stall,tools}.rs` to `crates/runtime/src/chat/looping/`
- Modify: `crates/runtime/src/chat/looping/loop_runner.rs`
- Modify: `apps/cli/src/tui/app/processing.rs`
- Modify: `apps/cli/src/tui/app/stream.rs`

- [ ] **Step 1: Move helper modules one by one**

Use `mv` for each file, then replace imports from `crate::tui::app::stream::*` and `crate::tui::app::UiEvent` with runtime event sink equivalents.

Run:

```bash
mv apps/cli/src/tui/app/stream/compact.rs crates/runtime/src/chat/looping/compact.rs
mv apps/cli/src/tui/app/stream/finalize.rs crates/runtime/src/chat/looping/finalize.rs
mv apps/cli/src/tui/app/stream/queue.rs crates/runtime/src/chat/looping/queue_legacy_tests_source.rs
```

Expected: files are moved. After moving queue tests, manually merge useful Bug #49 tests into `crates/runtime/src/chat/looping/queue.rs`, then delete `queue_legacy_tests_source.rs`.

- [ ] **Step 2: Convert helper modules to runtime events**

For every moved helper function that accepted `mpsc::Sender<UiEvent>`, change it to generic `S: ChatEventSink` and send `RuntimeStreamEvent`.

Example conversion for tool result send:

```rust
pub(crate) async fn send_tool_result<S>(
    sink: &S,
    call: &ToolCall,
    result: &UiToolResult,
) where
    S: ChatEventSink,
{
    sink.send_event(RuntimeStreamEvent::ToolResult {
        id: result.0.clone(),
        tool_name: call.name.clone(),
        output: result.1.clone(),
        is_error: result.2,
        images: result.3.clone(),
    })
    .await;
}
```

- [ ] **Step 3: Replace loop_runner placeholder with original logic**

In `crates/runtime/src/chat/looping/loop_runner.rs`, replace the checkpoint message branch with the original logic from `apps/cli/src/tui/app/stream.rs` lines 171-347:

```rust
// Use migrated auto_compact, task reminder, log_llm_input/output, execute_tool_round,
// tool_results_for_api, run_post_tool_batch, finalize_main_loop.
```

Concrete rule: no behavior marker message such as `runtime migration checkpoint` may remain after this task.

- [ ] **Step 4: Switch CLI spawn_processing to runtime**

Modify `apps/cli/src/tui/app/processing.rs` `spawn_processing` body:

```rust
pub(super) fn spawn_processing(ctx: SpawnContext) {
    tokio::spawn(async move {
        let sink = TuiEventSink::new(ctx.tx);
        let queue = TuiQueueDrainPort::new(ctx.queue_request_tx);
        ::runtime::api::chat::process_chat_loop(::runtime::api::chat::ChatLoopContext {
            sink,
            queue,
            client: ctx.client,
            registry: ctx.registry,
            system_blocks: ctx.system_blocks,
            system_prompt_text: ctx.system_prompt_text,
            user_context: ctx.user_context,
            messages: ctx.messages,
            context_size: ctx.context_size,
            cwd: ctx.cwd,
            workspace_context: ctx.workspace_context,
            session_id: ctx.session_id,
            read_files: ctx.read_files,
            session_reminders: ctx.session_reminders,
            agent_runner: ctx.agent_runner,
            allow_all: ctx.allow_all,
            interrupted: ctx.interrupted,
            cancel: ctx.cancel,
            task_store: ctx.task_store,
            max_tool_concurrency: ctx.max_tool_concurrency,
            max_agent_concurrency: ctx.max_agent_concurrency,
            agent_semaphore: ctx.agent_semaphore,
            hook_runner: ctx.hook_runner,
            memory_config: ctx.memory_config,
            json_logger: ctx.json_logger,
        })
        .await;
    });
}
```

- [ ] **Step 5: Remove old CLI stream main loop**

Modify `apps/cli/src/tui/app/stream.rs` to only keep modules still required by CLI tests or delete the file if no longer referenced. If deleted, update `apps/cli/src/tui/app/mod.rs` module declarations accordingly.

- [ ] **Step 6: Verify Task 5**

Run:

```bash
cargo fmt --all -- --check
cargo check -p runtime
cargo check -p cli
cargo test -p runtime chat/looping
cargo test -p cli tui
```

Expected: all pass.

---

### Task 6: 更新 #47 文档并完整验证

**Files:**
- Modify: `docs/feature/active.md`
- Modify: `docs/feature/specs/047-ddd-redesign.md`

- [ ] **Step 1: 更新 active.md #47 当前推进**

In `docs/feature/active.md`, update #47 line and detail paragraph to include:

```text
TUI stream loop 首轮已开始下沉 runtime：后台 processing 入口通过 runtime::api::chat 调用，LLM turn loop、ToolContext 构造、queue drain、stream handler 与 tool batch 编排从 apps/cli/src/tui/app/stream* 迁入 crates/runtime；apps/cli 继续保留 UiEvent、渲染、输入、AskUserQuestion 交互和 event adapter。
```

- [ ] **Step 2: 更新 spec checkpoint**

In `docs/feature/specs/047-ddd-redesign.md` around checkpoint 5, include:

```text
TUI stream loop 首轮迁移后，runtime 开始接管 TUI 后台 turn loop；TUI adapter 仍保留 UiEvent、渲染、输入队列 UI 和 AskUserQuestion 交互，不在本 checkpoint 重写 projection event。
```

- [ ] **Step 3: Full verification**

Run:

```bash
cargo fmt --all -- --check
cargo check
cargo test -p runtime chat/looping
cargo test -p cli tui
cargo test
./build_cli.sh
.agents/hooks/check-architecture-guards.sh
.agents/hooks/check-unit-tests.sh
```

Expected: all pass.

- [ ] **Step 4: Commit implementation**

Before committing, invoke the `commit` skill and follow repository commit style. Suggested title:

```bash
git add -A
git commit -m "refactor: 将 TUI stream loop 迁移到 runtime (refs #47)"
```

---

## Self-Review

- Spec coverage: 计划覆盖路线 1 的首轮目标：runtime 接管 TUI 后台 turn loop，TUI 保留渲染、输入和 event adapter。
- Scope check: 不拆 slash command、不拆 App state、不改 AskUserQuestion UI、不改 session 格式，符合低风险范围。
- Placeholder scan: 无 TBD/TODO；Task 5 的 helper 模块迁移必须在实施时依据现有代码逐文件转换，不能提交 checkpoint marker。
- Type consistency: `RuntimeStreamEvent`、`ChatEventSink`、`QueueDrainPort`、`ChatLoopContext` 在前置任务定义并在后续使用。
