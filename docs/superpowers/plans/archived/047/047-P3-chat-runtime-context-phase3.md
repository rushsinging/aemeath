# Feature 47 Chat Runtime Context Phase 3 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Chat 启动参数整理为 `ChatRuntimeContext`、`ChatLaunchOptions` 和 mode-specific launch DTO，降低 application port 参数重复和 runtime adapter 参数臃肿。

**Architecture:** Phase 3 延续 #47 DDD/COLA 薄入口重构：application 层继续定义 port 和 DTO，runtime adapter 继续调用现有 `repl::run_repl` / `tui::App::run`。本阶段只重塑 DTO 边界和参数映射，不重写 agent loop、不迁 crate、不改变 Tool Execution pipeline。

**Tech Stack:** Rust workspace、Tokio、async-trait、现有 `aemeath_core` / `aemeath_llm` / `aemeath_tools` crate、现有 `.agents/aemeath.json` Stop hooks。

---

## File Structure

Modify:

- `cli/src/application/main_loop/request.rs`
  - 用 `ChatLaunchOptions`、`NoTuiChatLaunch`、`TuiChatLaunch` 替代 `ChatLaunchMode` + `ChatLaunchRequest`。
  - 校验共同启动选项，TUI 专属必填项由非空 `String` 字段表达并校验非空。
- `cli/src/application/main_loop/port.rs`
  - 用 `ChatRuntimeContext` 替代 `NoTuiChatDependencies` / `TuiChatDependencies` 的重复字段。
  - port 方法改为接收 mode-specific launch DTO + shared context。
- `cli/src/application/main_loop/service.rs`
  - `ChatApplicationService` 分别 validate `NoTuiChatLaunch` / `TuiChatLaunch` 后分发到 port。
- `cli/src/application/main_loop/mod.rs`
  - 更新 re-export。
- `cli/src/run_orchestration/runtime.rs`
  - 构造 `ChatLaunchOptions`、`NoTuiChatLaunch`、`TuiChatLaunch` 和 `ChatRuntimeContext`。
  - runtime adapter 参数映射保持现有 `repl::run_repl` / `tui::App::run` 行为不变。
- `docs/feature/active.md`
  - 更新 #47 Phase 3 实现状态。

Verification:

- `cargo test -p aemeath-cli application::chat`
- `cargo build`
- `cargo test`
- `cargo fmt --all -- --check`
- `.agents/aemeath.json` Stop hooks：`build_cli.sh`、`.agents/hooks/check-architecture-guards.sh`、`.agents/hooks/check-unit-tests.sh`

---

### Task 1: Introduce Chat Launch DTOs

**Files:**
- Modify: `cli/src/application/main_loop/request.rs`
- Modify: `cli/src/application/main_loop/mod.rs`

- [x] **Step 1: Read current files**

Run:

```bash
pwd
git status --short --branch
```

Expected: branch is `feature/47-chat-runtime-context-phase3` and working tree has only planned docs changes if this plan has already been written.

- [x] **Step 2: Replace `cli/src/application/main_loop/request.rs`**

Replace the entire file with:

```rust
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChatLaunchOptions {
    pub cwd: PathBuf,
    pub verbose: bool,
    pub markdown: bool,
    pub context_size: usize,
    pub resume: Option<String>,
    pub allow_all: bool,
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
}

impl ChatLaunchOptions {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.max_tool_concurrency == 0 {
            return Err("max_tool_concurrency 必须大于 0".to_string());
        }
        if self.max_agent_concurrency == 0 {
            return Err("max_agent_concurrency 必须大于 0".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NoTuiChatLaunch {
    pub options: ChatLaunchOptions,
}

impl NoTuiChatLaunch {
    pub(crate) fn validate(&self) -> Result<(), String> {
        self.options.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TuiChatLaunch {
    pub options: ChatLaunchOptions,
    pub session_id: String,
    pub model_display: String,
}

impl TuiChatLaunch {
    pub(crate) fn validate(&self) -> Result<(), String> {
        self.options.validate()?;
        if self.session_id.is_empty() {
            return Err("TUI 启动必须提供 session_id".to_string());
        }
        if self.model_display.is_empty() {
            return Err("TUI 启动必须提供 model_display".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_options() -> ChatLaunchOptions {
        ChatLaunchOptions {
            cwd: PathBuf::from("/tmp/aemeath"),
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
    fn test_validate_accepts_no_tui_launch() {
        let launch = NoTuiChatLaunch {
            options: base_options(),
        };

        let result = launch.validate();

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn test_validate_accepts_tui_launch_with_required_fields() {
        let launch = TuiChatLaunch {
            options: base_options(),
            session_id: "session-1".to_string(),
            model_display: "provider/model".to_string(),
        };

        let result = launch.validate();

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn test_validate_rejects_tui_missing_session_id() {
        let launch = TuiChatLaunch {
            options: base_options(),
            session_id: String::new(),
            model_display: "provider/model".to_string(),
        };

        let result = launch.validate();

        assert_eq!(result, Err("TUI 启动必须提供 session_id".to_string()));
    }

    #[test]
    fn test_validate_rejects_tui_missing_model_display() {
        let launch = TuiChatLaunch {
            options: base_options(),
            session_id: "session-1".to_string(),
            model_display: String::new(),
        };

        let result = launch.validate();

        assert_eq!(result, Err("TUI 启动必须提供 model_display".to_string()));
    }

    #[test]
    fn test_validate_rejects_zero_tool_concurrency() {
        let mut options = base_options();
        options.max_tool_concurrency = 0;
        let launch = NoTuiChatLaunch { options };

        let result = launch.validate();

        assert_eq!(result, Err("max_tool_concurrency 必须大于 0".to_string()));
    }

    #[test]
    fn test_validate_rejects_zero_agent_concurrency() {
        let mut options = base_options();
        options.max_agent_concurrency = 0;
        let launch = NoTuiChatLaunch { options };

        let result = launch.validate();

        assert_eq!(result, Err("max_agent_concurrency 必须大于 0".to_string()));
    }
}
```

- [x] **Step 3: Update `cli/src/application/main_loop/mod.rs` re-export**

Replace the request re-export line with:

```rust
pub(crate) use request::{ChatLaunchOptions, NoTuiChatLaunch, TuiChatLaunch};
```

Leave port/service re-exports unchanged for now. The build is expected to fail until Task 2 updates port/service.

- [x] **Step 4: Run targeted test and expect compile failure**

Run:

```bash
cargo test -p aemeath-cli application::chat::request
```

Expected: FAIL because port/service/runtime still reference `ChatLaunchRequest` / `ChatLaunchMode`. This is acceptable for Task 1.

---

### Task 2: Replace Dependency Bundles With ChatRuntimeContext

**Files:**
- Modify: `cli/src/application/main_loop/port.rs`
- Modify: `cli/src/application/main_loop/mod.rs`

- [x] **Step 1: Replace `cli/src/application/main_loop/port.rs`**

Replace the entire file with:

```rust
use super::request::{NoTuiChatLaunch, TuiChatLaunch};
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

pub(crate) struct ChatRuntimeContext {
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
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TuiChatOutcome {
    pub session_id: String,
}

#[async_trait(?Send)]
pub(crate) trait ChatRuntimePort {
    async fn run_no_tui_chat(
        &self,
        launch: NoTuiChatLaunch,
        context: ChatRuntimeContext,
    ) -> Result<(), String>;

    async fn run_tui_chat(
        &self,
        launch: TuiChatLaunch,
        context: ChatRuntimeContext,
    ) -> Result<TuiChatOutcome, String>;
}
```

- [x] **Step 2: Update `cli/src/application/main_loop/mod.rs` port re-export**

Replace the port re-export block with:

```rust
pub(crate) use port::{ChatRuntimeContext, ChatRuntimePort, TuiChatOutcome};
```

- [x] **Step 3: Run targeted test and expect compile failure**

Run:

```bash
cargo test -p aemeath-cli application::chat
```

Expected: FAIL because service/runtime still reference old dependency bundle names. This is acceptable for Task 2.

---

### Task 3: Update ChatApplicationService

**Files:**
- Modify: `cli/src/application/main_loop/service.rs`

- [x] **Step 1: Replace `cli/src/application/main_loop/service.rs`**

Replace the entire file with:

```rust
use super::port::{ChatRuntimeContext, ChatRuntimePort, TuiChatOutcome};
use super::request::{NoTuiChatLaunch, TuiChatLaunch};

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

    pub(crate) fn validate_no_tui_launch(launch: &NoTuiChatLaunch) -> Result<(), String> {
        launch.validate()
    }

    pub(crate) fn validate_tui_launch(launch: &TuiChatLaunch) -> Result<(), String> {
        launch.validate()
    }

    pub(crate) async fn run_no_tui_chat(
        &self,
        launch: NoTuiChatLaunch,
        context: ChatRuntimeContext,
    ) -> Result<(), String> {
        Self::validate_no_tui_launch(&launch)?;
        self.runtime.run_no_tui_chat(launch, context).await
    }

    pub(crate) async fn run_tui_chat(
        &self,
        launch: TuiChatLaunch,
        context: ChatRuntimeContext,
    ) -> Result<TuiChatOutcome, String> {
        Self::validate_tui_launch(&launch)?;
        self.runtime.run_tui_chat(launch, context).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::chat::request::ChatLaunchOptions;
    use async_trait::async_trait;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct RecordingRuntimePort {
        no_tui_calls: Arc<Mutex<usize>>,
        tui_calls: Arc<Mutex<usize>>,
    }

    #[async_trait(?Send)]
    impl ChatRuntimePort for RecordingRuntimePort {
        async fn run_no_tui_chat(
            &self,
            _launch: NoTuiChatLaunch,
            _context: ChatRuntimeContext,
        ) -> Result<(), String> {
            *self.no_tui_calls.lock().unwrap() += 1;
            Ok(())
        }

        async fn run_tui_chat(
            &self,
            launch: TuiChatLaunch,
            _context: ChatRuntimeContext,
        ) -> Result<TuiChatOutcome, String> {
            *self.tui_calls.lock().unwrap() += 1;
            Ok(TuiChatOutcome {
                session_id: launch.session_id,
            })
        }
    }

    fn base_options() -> ChatLaunchOptions {
        ChatLaunchOptions {
            cwd: PathBuf::from("/tmp/aemeath"),
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
    fn test_validate_no_tui_launch_delegates_to_launch_validation() {
        let mut options = base_options();
        options.max_tool_concurrency = 0;
        let launch = NoTuiChatLaunch { options };

        let result = ChatApplicationService::<RecordingRuntimePort>::validate_no_tui_launch(&launch);

        assert_eq!(result, Err("max_tool_concurrency 必须大于 0".to_string()));
    }

    #[test]
    fn test_validate_tui_launch_delegates_to_launch_validation() {
        let launch = TuiChatLaunch {
            options: base_options(),
            session_id: String::new(),
            model_display: "provider/model".to_string(),
        };

        let result = ChatApplicationService::<RecordingRuntimePort>::validate_tui_launch(&launch);

        assert_eq!(result, Err("TUI 启动必须提供 session_id".to_string()));
    }
}
```

- [x] **Step 2: Run targeted test and expect compile failure from runtime**

Run:

```bash
cargo test -p aemeath-cli application::chat
```

Expected: FAIL because `cli/src/run_orchestration/runtime.rs` still constructs old DTOs. This is acceptable for Task 3.

---

### Task 4: Update Runtime Adapters and Orchestration Mapping

**Files:**
- Modify: `cli/src/run_orchestration/runtime.rs`

- [x] **Step 1: Update imports in `runtime.rs`**

Replace the existing `crate::application::chat` import block with:

```rust
use crate::application::chat::{
    ChatApplicationService, ChatLaunchOptions, ChatRuntimeContext, ChatRuntimePort,
    NoTuiChatLaunch, TuiChatLaunch, TuiChatOutcome,
};
```

Keep `use crate::{repl, tui};` and `use async_trait::async_trait;`.

- [x] **Step 2: Update `NoTuiChatRuntimeAdapter` method signature and body names**

Replace the `run_no_tui_chat` method in `impl ChatRuntimePort for NoTuiChatRuntimeAdapter` with:

```rust
    async fn run_no_tui_chat(
        &self,
        launch: NoTuiChatLaunch,
        context: ChatRuntimeContext,
    ) -> Result<(), String> {
        repl::run_repl(
            context.client,
            context.registry,
            context.system_blocks,
            context.system_prompt_text,
            context.user_context,
            launch.options.cwd,
            launch.options.verbose,
            launch.options.markdown,
            launch.options.context_size,
            launch.options.resume,
            Some(context.agent_runner),
            launch.options.allow_all,
            context.task_store,
            launch.options.max_tool_concurrency,
            context.agent_semaphore,
            context.skills_map,
            context.hook_runner,
            context.memory_config,
            context.json_logger,
        )
        .await;
        Ok(())
    }
```

Replace the unsupported TUI method signature with:

```rust
    async fn run_tui_chat(
        &self,
        _launch: TuiChatLaunch,
        _context: ChatRuntimeContext,
    ) -> Result<TuiChatOutcome, String> {
        Err("NoTuiChatRuntimeAdapter 不支持 TUI 启动".to_string())
    }
```

- [x] **Step 3: Update `TuiChatRuntimeAdapter` method signatures and body names**

Replace the unsupported no-TUI method with:

```rust
    async fn run_no_tui_chat(
        &self,
        _launch: NoTuiChatLaunch,
        _context: ChatRuntimeContext,
    ) -> Result<(), String> {
        Err("TuiChatRuntimeAdapter 不支持 no-TUI 启动".to_string())
    }
```

Replace the TUI method with:

```rust
    async fn run_tui_chat(
        &self,
        launch: TuiChatLaunch,
        context: ChatRuntimeContext,
    ) -> Result<TuiChatOutcome, String> {
        let session_id = launch.session_id;
        let mut app = tui::App::new(
            session_id.clone(),
            launch.options.cwd,
            launch.model_display,
        );
        app.memory_config = context.memory_config;
        app.set_skills(context.skills_map);
        app.hook_runner = context.hook_runner;
        app.json_logger = context.json_logger;
        app.run(
            context.client,
            context.registry,
            context.system_blocks,
            context.system_prompt_text,
            context.user_context,
            launch.options.context_size,
            launch.options.verbose,
            launch.options.markdown,
            Some(context.agent_runner),
            launch.options.allow_all,
            launch.options.resume,
            context.task_store,
            launch.options.max_tool_concurrency,
            launch.options.max_agent_concurrency,
            context.agent_semaphore,
        )
        .await
        .map_err(|e| e.to_string())?;
        Ok(TuiChatOutcome { session_id })
    }
```

- [x] **Step 4: Replace no-TUI DTO construction in `run_no_tui`**

Replace old `ChatLaunchRequest` and `NoTuiChatDependencies` construction with:

```rust
    let launch = NoTuiChatLaunch {
        options: ChatLaunchOptions {
            cwd,
            verbose: args.verbose,
            markdown: args.markdown,
            context_size: args.context_size,
            resume: args.resume.clone(),
            allow_all: args.allow_all,
            max_tool_concurrency: config.max_tool_concurrency,
            max_agent_concurrency,
        },
    };
    let context = ChatRuntimeContext {
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
    let service = ChatApplicationService::new(NoTuiChatRuntimeAdapter);
    if let Err(e) = service.run_no_tui_chat(launch, context).await {
```

Keep the existing error handling block after this line unchanged.

- [x] **Step 5: Replace TUI DTO construction in `run_tui`**

Replace old `ChatLaunchRequest` and `TuiChatDependencies` construction with:

```rust
    let launch = TuiChatLaunch {
        options: ChatLaunchOptions {
            cwd,
            verbose: args.verbose,
            markdown: args.markdown,
            context_size: args.context_size,
            resume: args.resume.clone(),
            allow_all: args.allow_all,
            max_tool_concurrency: config.max_tool_concurrency,
            max_agent_concurrency,
        },
        session_id: session_id.clone(),
        model_display,
    };
    let context = ChatRuntimeContext {
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
    let service = ChatApplicationService::new(TuiChatRuntimeAdapter);
    match service.run_tui_chat(launch, context).await {
```

Then fix the context construction: because `ChatRuntimeContext` does not contain `max_agent_concurrency`, remove the `max_agent_concurrency,` line. The TUI adapter reads max agent concurrency from `launch.options.max_agent_concurrency`.

Keep the existing success/error handling after this line, using `outcome.session_id`.

- [x] **Step 6: Run targeted tests and build**

Run:

```bash
cargo test -p aemeath-cli application::chat
cargo build
```

Expected: both PASS.

---

### Task 5: Update Tracking and Verify Stop Hooks

**Files:**
- Modify: `docs/feature/active.md`

- [x] **Step 1: Update #47 row in `docs/feature/active.md`**

In the #47 row, replace the Phase 3 design sentence with:

```text
Phase 3 继续整理 Chat 启动参数边界：已引入 ChatRuntimeContext、ChatLaunchOptions、NoTuiChatLaunch、TuiChatLaunch，拆分共享运行依赖、共同启动选项和入口模式专属字段，降低 application port 重复参数。
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

Expected: branch is `feature/47-chat-runtime-context-phase3` with only intended files changed.

---

## Self-Review

Spec coverage:

- `ChatRuntimeContext`: Task 2 defines shared context and Task 4 maps it to existing runtime calls.
- `ChatLaunchOptions`: Task 1 defines shared options and validation.
- mode-specific request: Task 1 defines `NoTuiChatLaunch` and `TuiChatLaunch`.
- application service still validates + dispatches: Task 3.
- adapter still calls existing runtime: Task 4.
- no agent loop rewrite: Task 4 only remaps parameters.
- Stop hook safety: Task 5 runs all three configured commands.

Placeholder scan:

- No TBD/TODO/implement-later placeholders.
- All changed code blocks define exact types and signatures.

Type consistency:

- `ChatRuntimeContext`, `ChatLaunchOptions`, `NoTuiChatLaunch`, `TuiChatLaunch`, and `TuiChatOutcome` names are consistent across tasks.
- TUI max agent concurrency is intentionally carried through `ChatLaunchOptions`, not `ChatRuntimeContext`.
