# Feature 47 Chat Runtime Port Phase 2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 `ChatApplicationService` 通过入口无关的 `ChatRuntimePort` 分发 no-TUI/TUI 启动请求，进一步把 CLI/TUI 适配器从 application service 中剥离。

**Architecture:** Phase 2 仍在 CLI crate 内保守落地 COLA 边界：`cli/src/application/chat` 定义 request、dependency DTO、port trait 和 service；`cli/src/run_orchestration/runtime.rs` 提供 no-TUI/TUI adapter，实现 port 并调用现有 `repl::run_repl` / `tui::App::run`。不重写 agent loop，不改变 Tool Execution pipeline，不引入 HTTP server。

**Tech Stack:** Rust workspace、Tokio、async-trait、现有 `aemeath_core` / `aemeath_llm` / `aemeath_tools` crate、现有 `.agents/aemeath.json` Stop hooks。

---

## File Structure

Create:

- `cli/src/application/chat/mod.rs`
  - Chat application 子模块出口，导出 request、port、service。
- `cli/src/application/chat/request.rs`
  - `ChatLaunchMode`、`ChatLaunchRequest` 与 validate tests。
- `cli/src/application/chat/port.rs`
  - `ChatRuntimePort`、`NoTuiChatDependencies`、`TuiChatDependencies`、`TuiChatOutcome`。
- `cli/src/application/chat/service.rs`
  - `ChatApplicationService`，只执行 validate + port dispatch。

Modify:

- `cli/src/application/mod.rs`
  - 保持 `pub(crate) mod chat;`，路径从 `chat.rs` 切换到 `chat/mod.rs`。
- `cli/src/application/chat.rs`
  - 删除，内容拆到 `chat/` 子模块。
- `cli/src/run_orchestration/runtime.rs`
  - 新增 `NoTuiChatRuntimeAdapter` 与 `TuiChatRuntimeAdapter`，实现 `ChatRuntimePort`。
  - `run_no_tui` / `run_tui` 改为调用 service + adapter。
- `cli/Cargo.toml`
  - 增加 `async-trait` 依赖，用于 async port trait。
- `docs/feature/active.md`
  - 更新 #47 Phase 2 状态。

Verification:

- `cargo test -p aemeath-cli application::chat`
- `cargo build`
- `cargo test`
- `cargo fmt --all -- --check`
- `.agents/aemeath.json` Stop hooks：`build_cli.sh`、`.agents/hooks/check-architecture-guards.sh`、`.agents/hooks/check-unit-tests.sh`

---

### Task 1: Split Chat Application Module Into Focused Files

**Files:**
- Create: `cli/src/application/chat/mod.rs`
- Create: `cli/src/application/chat/request.rs`
- Modify/Delete: `cli/src/application/chat.rs`

- [x] **Step 1: Read current source**

Run:

```bash
pwd
git status --short --branch
```

Expected: branch is `feature/47-chat-runtime-port-phase2` and working tree has no unrelated user changes.

- [x] **Step 2: Create `cli/src/application/chat/mod.rs`**

Create file with exactly:

```rust
pub(crate) mod request;

pub(crate) use request::{ChatLaunchMode, ChatLaunchRequest};
```

- [x] **Step 3: Move request code and tests to `cli/src/application/chat/request.rs`**

Create file with exactly:

```rust
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChatLaunchMode {
    NoTui,
    Tui,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChatLaunchRequest {
    pub mode: ChatLaunchMode,
    pub session_id: Option<String>,
    pub cwd: PathBuf,
    pub model_display: Option<String>,
    pub verbose: bool,
    pub markdown: bool,
    pub context_size: usize,
    pub resume: Option<String>,
    pub allow_all: bool,
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
}

impl ChatLaunchRequest {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.max_tool_concurrency == 0 {
            return Err("max_tool_concurrency 必须大于 0".to_string());
        }
        if self.max_agent_concurrency == 0 {
            return Err("max_agent_concurrency 必须大于 0".to_string());
        }
        match self.mode {
            ChatLaunchMode::NoTui => Ok(()),
            ChatLaunchMode::Tui => {
                if self.session_id.as_deref().unwrap_or_default().is_empty() {
                    return Err("TUI 启动必须提供 session_id".to_string());
                }
                if self.model_display.as_deref().unwrap_or_default().is_empty() {
                    return Err("TUI 启动必须提供 model_display".to_string());
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_request(mode: ChatLaunchMode) -> ChatLaunchRequest {
        ChatLaunchRequest {
            mode,
            session_id: None,
            cwd: PathBuf::from("/tmp/aemeath"),
            model_display: None,
            verbose: false,
            markdown: true,
            context_size: 200_000,
            resume: None,
            allow_all: false,
            max_tool_concurrency: 10,
            max_agent_concurrency: 4,
        }
    }

    #[test]
    fn test_validate_accepts_no_tui_without_tui_fields() {
        let request = base_request(ChatLaunchMode::NoTui);

        let result = request.validate();

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn test_validate_accepts_tui_with_required_fields() {
        let mut request = base_request(ChatLaunchMode::Tui);
        request.session_id = Some("session-1".to_string());
        request.model_display = Some("provider/model".to_string());

        let result = request.validate();

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn test_validate_rejects_tui_missing_session_id() {
        let mut request = base_request(ChatLaunchMode::Tui);
        request.model_display = Some("provider/model".to_string());

        let result = request.validate();

        assert_eq!(result, Err("TUI 启动必须提供 session_id".to_string()));
    }

    #[test]
    fn test_validate_rejects_zero_tool_concurrency() {
        let mut request = base_request(ChatLaunchMode::NoTui);
        request.max_tool_concurrency = 0;

        let result = request.validate();

        assert_eq!(result, Err("max_tool_concurrency 必须大于 0".to_string()));
    }

    #[test]
    fn test_validate_rejects_no_tui_zero_agent_concurrency() {
        let mut request = base_request(ChatLaunchMode::NoTui);
        request.max_agent_concurrency = 0;

        let result = request.validate();

        assert_eq!(result, Err("max_agent_concurrency 必须大于 0".to_string()));
    }
}
```

- [x] **Step 4: Temporarily leave old `cli/src/application/chat.rs` empty-deleted**

Delete `cli/src/application/chat.rs`. The module is now resolved through `cli/src/application/chat/mod.rs`.

- [x] **Step 5: Run targeted test and expect compile errors from missing service types**

Run:

```bash
cargo test -p aemeath-cli application::chat
```

Expected: FAIL because `ChatApplicationService`, `NoTuiChatDependencies`, and `TuiChatDependencies` are not yet reintroduced. This confirms the split removed the old monolithic file.

---

### Task 2: Add Chat Runtime Port and Service Dispatch

**Files:**
- Create: `cli/src/application/chat/port.rs`
- Create: `cli/src/application/chat/service.rs`
- Modify: `cli/src/application/chat/mod.rs`
- Modify: `cli/Cargo.toml`

- [x] **Step 1: Add async-trait dependency to `cli/Cargo.toml`**

In `[dependencies]`, add:

```toml
async-trait = { workspace = true }
```

- [x] **Step 2: Update `cli/src/application/chat/mod.rs`**

Replace contents with exactly:

```rust
pub(crate) mod port;
pub(crate) mod request;
pub(crate) mod service;

pub(crate) use port::{ChatRuntimePort, NoTuiChatDependencies, TuiChatDependencies, TuiChatOutcome};
pub(crate) use request::{ChatLaunchMode, ChatLaunchRequest};
pub(crate) use service::ChatApplicationService;
```

- [x] **Step 3: Create `cli/src/application/chat/port.rs`**

Create file with exactly:

```rust
use super::request::ChatLaunchRequest;
use aemeath_core::config::MemoryConfig;
use aemeath_core::hook::HookRunner;
use aemeath_core::logging::JsonLogger;
use aemeath_core::skill::Skill;
use aemeath_core::task::TaskStore;
use aemeath_core::tool::{AgentRunner, ToolRegistry};
use aemeath_llm::client::LlmClient;
use aemeath_llm::types::SystemBlock;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub(crate) struct NoTuiChatDependencies {
    pub client: Arc<LlmClient>,
    pub registry: Arc<ToolRegistry>,
    pub system_blocks: Vec<SystemBlock>,
    pub system_prompt_text: String,
    pub user_context: String,
    pub agent_runner: Arc<dyn AgentRunner>,
    pub task_store: Arc<TaskStore>,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
    pub skills_map: HashMap<String, Skill>,
    pub hook_runner: HookRunner,
    pub memory_config: MemoryConfig,
    pub json_logger: Option<Arc<Mutex<JsonLogger>>>,
}

pub(crate) struct TuiChatDependencies {
    pub client: Arc<LlmClient>,
    pub registry: Arc<ToolRegistry>,
    pub system_blocks: Vec<SystemBlock>,
    pub system_prompt_text: String,
    pub user_context: String,
    pub agent_runner: Arc<dyn AgentRunner>,
    pub task_store: Arc<TaskStore>,
    pub skills_map: HashMap<String, Skill>,
    pub hook_runner: HookRunner,
    pub memory_config: MemoryConfig,
    pub json_logger: Option<Arc<Mutex<JsonLogger>>>,
    pub max_agent_concurrency: usize,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TuiChatOutcome {
    pub session_id: String,
}

#[async_trait]
pub(crate) trait ChatRuntimePort {
    async fn run_no_tui_chat(
        &self,
        request: ChatLaunchRequest,
        dependencies: NoTuiChatDependencies,
    ) -> Result<(), String>;

    async fn run_tui_chat(
        &self,
        request: ChatLaunchRequest,
        dependencies: TuiChatDependencies,
    ) -> Result<TuiChatOutcome, String>;
}
```

- [x] **Step 4: Create `cli/src/application/chat/service.rs`**

Create file with exactly:

```rust
use super::port::{ChatRuntimePort, NoTuiChatDependencies, TuiChatDependencies, TuiChatOutcome};
use super::request::ChatLaunchRequest;

pub(crate) struct ChatApplicationService<P> {
    runtime: P,
}

impl<P> ChatApplicationService<P>
where
    P: ChatRuntimePort,
{
    pub(crate) fn new(runtime: P) -> Self {
        Self { runtime }
    }

    pub(crate) fn validate_request(request: &ChatLaunchRequest) -> Result<(), String> {
        request.validate()
    }

    pub(crate) async fn run_no_tui_chat(
        &self,
        request: ChatLaunchRequest,
        dependencies: NoTuiChatDependencies,
    ) -> Result<(), String> {
        Self::validate_request(&request)?;
        self.runtime.run_no_tui_chat(request, dependencies).await
    }

    pub(crate) async fn run_tui_chat(
        &self,
        request: ChatLaunchRequest,
        dependencies: TuiChatDependencies,
    ) -> Result<TuiChatOutcome, String> {
        Self::validate_request(&request)?;
        self.runtime.run_tui_chat(request, dependencies).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::chat::request::ChatLaunchMode;
    use async_trait::async_trait;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct RecordingRuntimePort {
        no_tui_calls: Arc<Mutex<usize>>,
        tui_calls: Arc<Mutex<usize>>,
    }

    #[async_trait]
    impl ChatRuntimePort for RecordingRuntimePort {
        async fn run_no_tui_chat(
            &self,
            _request: ChatLaunchRequest,
            _dependencies: NoTuiChatDependencies,
        ) -> Result<(), String> {
            *self.no_tui_calls.lock().unwrap() += 1;
            Ok(())
        }

        async fn run_tui_chat(
            &self,
            request: ChatLaunchRequest,
            _dependencies: TuiChatDependencies,
        ) -> Result<TuiChatOutcome, String> {
            *self.tui_calls.lock().unwrap() += 1;
            Ok(TuiChatOutcome {
                session_id: request.session_id.unwrap_or_default(),
            })
        }
    }

    fn base_request(mode: ChatLaunchMode) -> ChatLaunchRequest {
        ChatLaunchRequest {
            mode,
            session_id: None,
            cwd: PathBuf::from("/tmp/aemeath"),
            model_display: None,
            verbose: false,
            markdown: true,
            context_size: 200_000,
            resume: None,
            allow_all: false,
            max_tool_concurrency: 10,
            max_agent_concurrency: 4,
        }
    }

    #[test]
    fn test_validate_request_delegates_to_request_validation() {
        let mut request = base_request(ChatLaunchMode::NoTui);
        request.max_tool_concurrency = 0;

        let result = ChatApplicationService::<RecordingRuntimePort>::validate_request(&request);

        assert_eq!(result, Err("max_tool_concurrency 必须大于 0".to_string()));
    }
}
```

Note: This task intentionally only unit-tests validation because constructing real dependency bundles is expensive. Runtime dispatch is validated by compile-time wiring in Task 3 and full tests.

- [x] **Step 5: Run targeted test**

Run:

```bash
cargo test -p aemeath-cli application::chat
```

Expected: FAIL because `runtime.rs` still calls old static `ChatApplicationService::run_*` methods. Task 3 will wire adapters.

---

### Task 3: Implement no-TUI/TUI Runtime Adapters

**Files:**
- Modify: `cli/src/run_orchestration/runtime.rs`

- [x] **Step 1: Update imports in `runtime.rs`**

Replace the existing `crate::application::chat` import with:

```rust
use crate::application::chat::{
    ChatApplicationService, ChatLaunchMode, ChatLaunchRequest, ChatRuntimePort,
    NoTuiChatDependencies, TuiChatDependencies, TuiChatOutcome,
};
use async_trait::async_trait;
```

Also add direct imports for the existing adapters:

```rust
use crate::{repl, tui};
```

- [x] **Step 2: Add adapter structs near the top of `runtime.rs` after imports**

Add exactly:

```rust
struct NoTuiChatRuntimeAdapter;

#[async_trait]
impl ChatRuntimePort for NoTuiChatRuntimeAdapter {
    async fn run_no_tui_chat(
        &self,
        request: ChatLaunchRequest,
        dependencies: NoTuiChatDependencies,
    ) -> Result<(), String> {
        repl::run_repl(
            dependencies.client,
            dependencies.registry,
            dependencies.system_blocks,
            dependencies.system_prompt_text,
            dependencies.user_context,
            request.cwd,
            request.verbose,
            request.markdown,
            request.context_size,
            request.resume,
            Some(dependencies.agent_runner),
            request.allow_all,
            dependencies.task_store,
            request.max_tool_concurrency,
            dependencies.agent_semaphore,
            dependencies.skills_map,
            dependencies.hook_runner,
            dependencies.memory_config,
            dependencies.json_logger,
        )
        .await;
        Ok(())
    }

    async fn run_tui_chat(
        &self,
        _request: ChatLaunchRequest,
        _dependencies: TuiChatDependencies,
    ) -> Result<TuiChatOutcome, String> {
        Err("NoTuiChatRuntimeAdapter 不支持 TUI 启动".to_string())
    }
}

struct TuiChatRuntimeAdapter;

#[async_trait]
impl ChatRuntimePort for TuiChatRuntimeAdapter {
    async fn run_no_tui_chat(
        &self,
        _request: ChatLaunchRequest,
        _dependencies: NoTuiChatDependencies,
    ) -> Result<(), String> {
        Err("TuiChatRuntimeAdapter 不支持 no-TUI 启动".to_string())
    }

    async fn run_tui_chat(
        &self,
        request: ChatLaunchRequest,
        dependencies: TuiChatDependencies,
    ) -> Result<TuiChatOutcome, String> {
        let session_id = request
            .session_id
            .clone()
            .ok_or_else(|| "TUI 启动必须提供 session_id".to_string())?;
        let model_display = request
            .model_display
            .clone()
            .ok_or_else(|| "TUI 启动必须提供 model_display".to_string())?;
        let mut app = tui::App::new(session_id.clone(), request.cwd, model_display);
        app.memory_config = dependencies.memory_config;
        app.set_skills(dependencies.skills_map);
        app.hook_runner = dependencies.hook_runner;
        app.json_logger = dependencies.json_logger;
        app.run(
            dependencies.client,
            dependencies.registry,
            dependencies.system_blocks,
            dependencies.system_prompt_text,
            dependencies.user_context,
            request.context_size,
            request.verbose,
            request.markdown,
            Some(dependencies.agent_runner),
            request.allow_all,
            request.resume,
            dependencies.task_store,
            request.max_tool_concurrency,
            dependencies.max_agent_concurrency,
            dependencies.agent_semaphore,
        )
        .await
        .map_err(|e| e.to_string())?;
        Ok(TuiChatOutcome { session_id })
    }
}
```

- [x] **Step 3: Update `run_no_tui` service call**

Replace:

```rust
if let Err(e) = ChatApplicationService::run_no_tui_chat(request, dependencies).await {
```

with:

```rust
let service = ChatApplicationService::new(NoTuiChatRuntimeAdapter);
if let Err(e) = service.run_no_tui_chat(request, dependencies).await {
```

- [x] **Step 4: Update `run_tui` service call**

Replace:

```rust
match ChatApplicationService::run_tui_chat(request, dependencies).await {
    Ok(session_id) => println!("aemeath --resume {}", session_id),
```

with:

```rust
let service = ChatApplicationService::new(TuiChatRuntimeAdapter);
match service.run_tui_chat(request, dependencies).await {
    Ok(outcome) => println!("aemeath --resume {}", outcome.session_id),
```

- [x] **Step 5: Run targeted tests**

Run:

```bash
cargo test -p aemeath-cli application::chat
```

Expected: PASS.

- [x] **Step 6: Run build**

Run:

```bash
cargo build
```

Expected: PASS.

---

### Task 4: Update Feature Tracking and Verify Stop Hooks

**Files:**
- Modify: `docs/feature/active.md`

- [x] **Step 1: Update #47 row in `docs/feature/active.md`**

Find the #47 row and append this sentence to the Notes column:

```text
Phase 2 继续推进薄入口：ChatApplicationService 改为依赖 ChatRuntimePort，CLI no-TUI/TUI 通过 runtime adapter 实现 port，application service 不再直接调用 repl/tui。
```

- [x] **Step 2: Run full verification**

Run:

```bash
cargo fmt --all -- --check
cargo build
cargo test
```

Expected: all PASS.

- [x] **Step 3: Run Stop hook command 1**

Run:

```bash
"${PWD}/build_cli.sh"
```

Expected: PASS.

- [x] **Step 4: Run Stop hook command 2**

Run:

```bash
"${PWD}/.agents/hooks/check-architecture-guards.sh"
```

Expected: PASS.

- [x] **Step 5: Run Stop hook command 3**

Run:

```bash
"${PWD}/.agents/hooks/check-unit-tests.sh"
```

Expected: PASS.

- [x] **Step 6: Inspect git status**

Run:

```bash
git status --short --branch
```

Expected: branch is `feature/47-chat-runtime-port-phase2` with only intended files changed.

---

## Self-Review

Spec coverage:

- Phase 2 thin entry goal: Task 2 defines `ChatRuntimePort`; Task 3 implements no-TUI/TUI adapters outside the application service.
- Application no longer calls UI directly: Task 2 `ChatApplicationService` only validates and dispatches to `ChatRuntimePort`; Task 3 moves `repl`/`tui` calls into runtime adapters.
- No agent loop rewrite: Task 3 keeps existing calls to `repl::run_repl` and `tui::App::run` unchanged except for location.
- Stop hook safety: Task 4 runs all three hooks from `.agents/aemeath.json`.

Placeholder scan:

- No TBD/TODO/implement-later placeholders.
- Every code step includes exact content or exact replacement.

Type consistency:

- `ChatRuntimePort`, `NoTuiChatDependencies`, `TuiChatDependencies`, and `TuiChatOutcome` are defined in Task 2 and used consistently in Task 3.
- `ChatApplicationService<P>` method names match runtime call sites.
