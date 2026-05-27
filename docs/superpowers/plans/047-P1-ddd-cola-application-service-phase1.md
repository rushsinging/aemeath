# Feature 47 DDD/COLA Application Service Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 建立 Chat/Session application service 薄入口边界，并把 CLI no-TUI 与 TUI 主入口最外层接入该边界，同时保持现有 agent loop、TUI loop、Stop hook 行为不变。

**Architecture:** 本阶段采用 COLA 的保守落地：`cli/src/application` 作为 Application 层，`cli/src/run_orchestration/runtime.rs` 作为入口适配桥接层。新增的 application service 只封装入口无关的 Chat/Session launch request，不迁移领域逻辑、不重写 agent loop、不改变 Tool Execution pipeline。

**Tech Stack:** Rust workspace、Tokio、现有 `aemeath_core` / `aemeath_llm` / `aemeath_tools` crate、现有 `.agents/aemeath.json` Stop hooks。

---

## File Structure

Create:

- `cli/src/application/mod.rs`
  - Application 层入口模块，只导出 `chat` 子模块。
- `cli/src/application/chat.rs`
  - 定义 Chat/Session 主入口 application service 的 request DTO、mode 枚举、service struct 和单元测试。

Modify:

- `cli/src/main.rs`
  - 增加 `mod application;`，让 CLI crate 可使用 application 层。
- `cli/src/run_orchestration/runtime.rs`
  - 将 `run_no_tui` / `run_tui` 的参数先组装为 application service request，再由 service 分发到现有 `repl::run_repl` / `tui::App::run`。
  - 不移动现有内部 loop 逻辑。
- `docs/feature/active.md`
  - 更新 #47 状态，记录 Phase 1 已进入 application service 薄入口重构。

Verification:

- `cargo fmt --all -- --check`
- `cargo test -p aemeath-cli application::chat`
- `cargo build`
- `cargo test`
- `.agents/aemeath.json` Stop hooks：依次执行 `build_cli.sh`、`.agents/hooks/check-architecture-guards.sh`、`.agents/hooks/check-unit-tests.sh`，确认全部通过。

---

### Task 1: Add Chat Application Service Types

**Files:**
- Create: `cli/src/application/mod.rs`
- Create: `cli/src/application/chat.rs`
- Modify: `cli/src/main.rs`

- [x] **Step 1: Create `cli/src/application/mod.rs`**

Create file with exactly:

```rust
pub(crate) mod chat;
```

- [x] **Step 2: Create failing tests in `cli/src/application/chat.rs`**

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

pub(crate) struct ChatApplicationService;

impl ChatApplicationService {
    pub(crate) fn validate_request(request: &ChatLaunchRequest) -> Result<(), String> {
        if request.max_tool_concurrency == 0 {
            return Err("max_tool_concurrency 必须大于 0".to_string());
        }
        if request.max_agent_concurrency == 0 {
            return Err("max_agent_concurrency 必须大于 0".to_string());
        }
        match request.mode {
            ChatLaunchMode::NoTui => Ok(()),
            ChatLaunchMode::Tui => {
                if request.session_id.as_deref().unwrap_or_default().is_empty() {
                    return Err("TUI 启动必须提供 session_id".to_string());
                }
                if request.model_display.as_deref().unwrap_or_default().is_empty() {
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
    fn test_validate_request_accepts_no_tui_without_tui_fields() {
        let request = base_request(ChatLaunchMode::NoTui);

        let result = ChatApplicationService::validate_request(&request);

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn test_validate_request_accepts_tui_with_required_fields() {
        let mut request = base_request(ChatLaunchMode::Tui);
        request.session_id = Some("session-1".to_string());
        request.model_display = Some("provider/model".to_string());

        let result = ChatApplicationService::validate_request(&request);

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn test_validate_request_rejects_tui_missing_session_id() {
        let mut request = base_request(ChatLaunchMode::Tui);
        request.model_display = Some("provider/model".to_string());

        let result = ChatApplicationService::validate_request(&request);

        assert_eq!(result, Err("TUI 启动必须提供 session_id".to_string()));
    }

    #[test]
    fn test_validate_request_rejects_zero_concurrency() {
        let mut request = base_request(ChatLaunchMode::NoTui);
        request.max_tool_concurrency = 0;

        let result = ChatApplicationService::validate_request(&request);

        assert_eq!(result, Err("max_tool_concurrency 必须大于 0".to_string()));
    }
}
```

- [x] **Step 3: Register module in `cli/src/main.rs`**

Change the top module list from:

```rust
mod agent_runner;
mod cli;
```

to:

```rust
mod agent_runner;
mod application;
mod cli;
```

- [x] **Step 4: Run targeted test**

Run:

```bash
cargo test -p aemeath-cli application::chat
```

Expected: PASS. The output must include four passing tests from `application::chat`.

- [x] **Step 5: Run format check**

Run:

```bash
cargo fmt --all -- --check
```

Expected: PASS with no diff.

---

### Task 2: Route no-TUI Chat Through Application Service

**Files:**
- Modify: `cli/src/application/chat.rs`
- Modify: `cli/src/run_orchestration/runtime.rs`

- [x] **Step 1: Extend imports in `cli/src/application/chat.rs`**

Replace:

```rust
use std::path::PathBuf;
```

with:

```rust
use aemeath_core::config::MemoryConfig;
use aemeath_core::hook::HookRunner;
use aemeath_core::logging::JsonLogger;
use aemeath_core::skill::Skill;
use aemeath_core::task::TaskStore;
use aemeath_core::tool::{AgentRunner, ToolRegistry};
use aemeath_llm::client::LlmClient;
use aemeath_llm::types::SystemBlock;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
```

- [x] **Step 2: Add no-TUI dependency bundle and service method**

Append this code before `#[cfg(test)] mod tests`:

```rust
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

impl ChatApplicationService {
    pub(crate) async fn run_no_tui_chat(
        request: ChatLaunchRequest,
        dependencies: NoTuiChatDependencies,
    ) -> Result<(), String> {
        Self::validate_request(&request)?;
        crate::repl::run_repl(
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
}
```

- [x] **Step 3: Update `cli/src/run_orchestration/runtime.rs` imports**

Replace:

```rust
use crate::{repl, tui};
```

with:

```rust
use crate::application::chat::{ChatApplicationService, ChatLaunchMode, ChatLaunchRequest, NoTuiChatDependencies};
use crate::tui;
```

- [x] **Step 4: Replace `run_no_tui` body**

Replace the body of `run_no_tui` with:

```rust
{
    let request = ChatLaunchRequest {
        mode: ChatLaunchMode::NoTui,
        session_id: None,
        cwd,
        model_display: None,
        verbose: args.verbose,
        markdown: !args.no_markdown,
        context_size: args.context_size,
        resume: args.resume.clone(),
        allow_all: args.allow_all,
        max_tool_concurrency,
        max_agent_concurrency: agent_semaphore.available_permits(),
    };
    let dependencies = NoTuiChatDependencies {
        client,
        registry,
        system_blocks,
        system_prompt_text,
        user_context,
        agent_runner,
        task_store,
        agent_semaphore,
        skills_map,
        hook_runner,
        memory_config,
        json_logger,
    };
    if let Err(e) = ChatApplicationService::run_no_tui_chat(request, dependencies).await {
        log::error!("no-TUI chat application service error: {e}");
        std::process::exit(1);
    }
}
```

- [x] **Step 5: Run targeted tests**

Run:

```bash
cargo test -p aemeath-cli application::chat
```

Expected: PASS.

- [x] **Step 6: Run compile check**

Run:

```bash
cargo build
```

Expected: PASS.

---

### Task 3: Route TUI Chat Through Application Service

**Files:**
- Modify: `cli/src/application/chat.rs`
- Modify: `cli/src/run_orchestration/runtime.rs`

- [x] **Step 1: Add TUI dependency bundle and service method**

Append this code before `#[cfg(test)] mod tests` in `cli/src/application/chat.rs`:

```rust
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

impl ChatApplicationService {
    pub(crate) async fn run_tui_chat(
        request: ChatLaunchRequest,
        dependencies: TuiChatDependencies,
    ) -> Result<String, String> {
        Self::validate_request(&request)?;
        let session_id = request
            .session_id
            .clone()
            .ok_or_else(|| "TUI 启动必须提供 session_id".to_string())?;
        let model_display = request
            .model_display
            .clone()
            .ok_or_else(|| "TUI 启动必须提供 model_display".to_string())?;
        let mut app = crate::tui::App::new(session_id.clone(), request.cwd, model_display);
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
        Ok(session_id)
    }
}
```

- [x] **Step 2: Update runtime imports**

Replace the import from Task 2:

```rust
use crate::application::chat::{ChatApplicationService, ChatLaunchMode, ChatLaunchRequest, NoTuiChatDependencies};
use crate::tui;
```

with:

```rust
use crate::application::chat::{
    ChatApplicationService, ChatLaunchMode, ChatLaunchRequest, NoTuiChatDependencies,
    TuiChatDependencies,
};
```

- [x] **Step 3: Replace `run_tui` body**

Replace the body of `run_tui` with:

```rust
{
    let request = ChatLaunchRequest {
        mode: ChatLaunchMode::Tui,
        session_id: Some(session_id.clone()),
        cwd,
        model_display: Some(model_display),
        verbose: args.verbose,
        markdown: !args.no_markdown,
        context_size: args.context_size,
        resume: args.resume,
        allow_all: args.allow_all,
        max_tool_concurrency,
        max_agent_concurrency,
    };
    let dependencies = TuiChatDependencies {
        client,
        registry,
        system_blocks,
        system_prompt_text,
        user_context,
        agent_runner,
        task_store,
        skills_map,
        hook_runner,
        memory_config,
        json_logger,
        max_agent_concurrency,
        agent_semaphore,
    };
    match ChatApplicationService::run_tui_chat(request, dependencies).await {
        Ok(session_id) => println!("aemeath --resume {}", session_id),
        Err(e) => {
            log::error!("TUI chat application service error: {e}");
            std::process::exit(1);
        }
    }
}
```

- [x] **Step 4: Run targeted tests**

Run:

```bash
cargo test -p aemeath-cli application::chat
```

Expected: PASS.

- [x] **Step 5: Run compile check**

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

Change the #47 row status text to include Phase 1 implementation. The row should keep the existing design summary and add this sentence at the end of the Notes column:

```text
Phase 1 开始落地 COLA application service 薄入口边界：CLI no-TUI 与 TUI 主入口最外层将通过 ChatApplicationService 分发到现有 runtime，不重写 agent loop。
```

- [x] **Step 2: Run full format and tests**

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

Expected: PASS. This mirrors `.agents/aemeath.json` Stop hook line 6.

- [x] **Step 4: Run Stop hook command 2**

Run:

```bash
"${PWD}/.agents/hooks/check-architecture-guards.sh"
```

Expected: PASS. This mirrors `.agents/aemeath.json` Stop hook line 11.

- [x] **Step 5: Run Stop hook command 3**

Run:

```bash
"${PWD}/.agents/hooks/check-unit-tests.sh"
```

Expected: PASS. This mirrors `.agents/aemeath.json` Stop hook line 16.

- [x] **Step 6: Inspect git status**

Run:

```bash
git status --short --branch
```

Expected: branch is `feature/47-ddd-cola-refactor-phase1` with only intended files changed.

---

## Self-Review

Spec coverage:

- 薄入口：Task 2 和 Task 3 让 no-TUI/TUI 最外层通过 `ChatApplicationService`。
- HTTP/CLI/TUI 都能接的方向：本阶段不实现 HTTP，但 `ChatLaunchRequest` 与 service 命名为入口无关结构，为 HTTP/SDK 后续复用留出边界。
- 包分细：本阶段只新增 `cli/src/application`，避免大规模移动领域代码。
- 不重写 agent loop：Task 2/3 仍调用现有 `repl::run_repl` 与 `tui::App::run`。
- Stop hook 顺利执行：Task 4 明确逐条执行 `.agents/aemeath.json` Stop hooks。

Placeholder scan:

- 无 TBD / TODO / implement later。
- 所有代码步骤都给出完整代码片段或精确替换。

Type consistency:

- `ChatLaunchMode`、`ChatLaunchRequest`、`ChatApplicationService`、`NoTuiChatDependencies`、`TuiChatDependencies` 在 Task 1-3 中命名一致。
- `max_tool_concurrency` 和 `max_agent_concurrency` 在 request、runtime 和 service 中命名一致。
