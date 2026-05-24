# Feature 47 Chat Bootstrapping Boundary Phase 4 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 `run_orchestration::run_chat` 中混杂的 Chat 启动准备逻辑整理成显式的 bootstrapping 边界对象，继续压薄入口和 runtime adapter，为 #47 DDD/COLA 后续把 Configuration、Model Gateway、Skill / Guidance、Hook / Automation 等上下文拆清楚做铺垫。

**Architecture:** Phase 1/2/3 已经让 CLI no-TUI 与 TUI 通过 `ChatApplicationService`、`ChatRuntimePort`、`ChatRuntimeContext`、`ChatLaunchOptions` 和 mode-specific launch DTO 分发到既有 runtime。Phase 4 不迁 crate、不重写 agent loop、不改变 CLI/TUI 行为；只在 `cli/src/run_orchestration/` 内部把“启动前准备出来的东西”收束为 `ChatBootstrap` 和 `ChatModeSelection`，让 `run_chat` 从长流程脚本变成薄编排层。

**Current Pain:** `cli/src/run_orchestration.rs` 当前同时承担入口参数修正、cwd/config/model/API key/client/logger/skills/tools/MCP/hooks/session/prompt/concurrency/runtime dispatch 等职责。按 #47 DDD/COLA 设计，这些职责分属 Configuration、Model Gateway、Skill / Guidance、Tool Execution、Hook / Automation、Agent Runtime、Interaction 等上下文。Phase 4 先建立边界对象，后续再逐步把 builder 移到对应上下文或 application service。

**Tech Stack:** Rust workspace、现有 `aemeath_core` / `aemeath_llm` / `aemeath_tools` crate、现有 CLI/TUI runtime、Tokio。

---

## File Structure

Modify:

- `cli/src/run_orchestration.rs`
  - 保留 `run_chat(args)` 对外入口。
  - 新增 `ChatModeSelection`，封装 no-TUI / TUI 选择。
  - 将 `run_chat` 拆为：命令初始化与参数预处理 → `bootstrap_chat` → 按 `ChatModeSelection` dispatch。
  - 保持原有错误消息、日志、环境变量读取顺序和行为不变。
- `cli/src/run_orchestration/setup.rs`
  - 新增 `ChatBootstrap`，承载 runtime dispatch 所需的共享启动产物。
  - 新增 `bootstrap_chat(args) -> ChatBootstrap` 或等价函数，把现有 `run_chat` 中从 cwd 到 concurrency 的准备逻辑迁入。
  - 保留并复用现有 `build_json_logger`、`build_agent_runner`。
- `cli/src/run_orchestration/prompt.rs`
  - 仅在需要时暴露或调整静态 prompt builder 的调用位置；不得改变 prompt 内容拼接顺序。
- `cli/src/run_orchestration/runtime.rs`
  - 如需，新增从 `ChatBootstrap` 构造 runtime context / launch 的辅助函数，避免 `run_chat` 继续逐字段传参。
- `docs/feature/active.md`
  - 更新 #47 状态，说明 Phase 4 计划已形成，目标是 Chat bootstrapping 边界对象化。

Do not modify unless required:

- `cli/src/repl.rs`
- `cli/src/tui/**`
- `packages/**`
- provider/tool/hook 核心逻辑

---

## Target Shape

目标结构示意：

```rust
pub(crate) async fn run_chat(mut args: Args) {
    aemeath_core::command::commands::init_all();
    apply_permission_env_override(&mut args);

    let bootstrap = setup::bootstrap_chat(args).await;
    match bootstrap.mode_selection {
        ChatModeSelection::NoTui => runtime::run_no_tui_from_bootstrap(bootstrap).await,
        ChatModeSelection::Tui => runtime::run_tui_from_bootstrap(bootstrap).await,
    }
}
```

`ChatBootstrap` 推荐字段：

```rust
pub(super) struct ChatBootstrap {
    pub args: Args,
    pub cwd: PathBuf,
    pub resolved_model: ResolvedModel, // 使用现有 model_selection 返回类型；不要复制字段。
    pub client: Arc<LlmClient>,
    pub registry: Arc<ToolRegistry>,
    pub system_blocks: Vec<SystemBlock>,
    pub system_prompt_text: String,
    pub user_context: String,
    pub session_id: String,
    pub agent_runner: Arc<dyn AgentRunner>,
    pub task_store: Arc<TaskStore>,
    pub skills_map: HashMap<String, Skill>,
    pub hook_runner: HookRunner,
    pub memory_config: MemoryConfig,
    pub json_logger: Option<Arc<Mutex<JsonLogger>>>,
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
    pub agent_semaphore: Arc<tokio::sync::Semaphore>,
    pub mode_selection: ChatModeSelection,
}
```

可根据真实类型可见性调整字段，但原则是：不要把同一组启动产物在 `run_chat`、`runtime::run_no_tui`、`runtime::run_tui` 三处重复组装。

---

### Task 1: Establish baseline and read current boundary

**Files:**
- Read: `docs/bug/active.md`
- Read: `docs/feature/active.md`
- Read: `docs/feature/specs/047-ddd-redesign.md`
- Read: `cli/src/run_orchestration.rs`
- Read: `cli/src/run_orchestration/setup.rs`
- Read: `cli/src/run_orchestration/runtime.rs`
- Read: `cli/src/run_orchestration/prompt.rs`

- [ ] **Step 1: Verify branch and workspace**

Run:

```bash
pwd
git status --short --branch
cargo check -p aemeath-cli
```

Expected:
- Working tree is clean before implementation, except this plan/doc update if already committed separately.
- `cargo check -p aemeath-cli` passes before changes.

- [ ] **Step 2: Confirm no behavior expansion**

Read the listed files and confirm:
- This phase only changes `run_orchestration` structure and docs.
- No change to LLM request semantics, prompt content, tool registry contents, hook order, session ID generation, config priority, or TUI/no-TUI runtime behavior.

---

### Task 2: Add explicit mode selection and permission override helpers

**Files:**
- Modify: `cli/src/run_orchestration.rs`

- [ ] **Step 1: Introduce `ChatModeSelection`**

Add a small enum near the top of `run_orchestration.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ChatModeSelection {
    NoTui,
    Tui,
}
```

Add helper:

```rust
fn chat_mode_selection(args: &Args) -> ChatModeSelection {
    if args.no_tui || !args.tui {
        ChatModeSelection::NoTui
    } else {
        ChatModeSelection::Tui
    }
}
```

- [ ] **Step 2: Extract env permission override**

Move the existing `AEMEATH_PERMISSION_MODE=allow_all` logic into:

```rust
fn apply_permission_env_override(args: &mut Args) {
    if !args.allow_all {
        if let Ok(mode) = std::env::var("AEMEATH_PERMISSION_MODE") {
            if mode == "allow_all" {
                args.allow_all = true;
            }
        }
    }
}
```

Do not change env var name or precedence.

- [ ] **Step 3: Add tests for pure helpers**

Add tests at the end of `run_orchestration.rs` or a new sibling test module if needed:

- `test_chat_mode_selection_prefers_no_tui_flag`
- `test_chat_mode_selection_uses_tui_when_enabled`
- `test_apply_permission_env_override_enables_allow_all`

If environment mutation is awkward in parallel tests, use a private helper that accepts `Option<&str>` and test that helper instead; keep `apply_permission_env_override` as the env-reading wrapper.

---

### Task 3: Introduce ChatBootstrap in setup module

**Files:**
- Modify: `cli/src/run_orchestration/setup.rs`
- Modify: `cli/src/run_orchestration.rs`

- [ ] **Step 1: Define `ChatBootstrap`**

In `setup.rs`, add a `pub(super) struct ChatBootstrap` that contains all shared runtime dispatch outputs currently produced in `run_chat`:

- `args`
- `cwd`
- resolved model information needed by runtime dispatch
- `client`
- `registry`
- `system_blocks`
- `system_prompt_text`
- `user_context`
- `session_id`
- `agent_runner`
- `task_store`
- `skills_map`
- `hook_runner`
- `memory_config`
- `json_logger`
- concurrency fields and semaphore
- `mode_selection`

Use existing types directly. Do not introduce duplicate DTOs for model/config unless type visibility forces it.

- [ ] **Step 2: Move preparation code into `bootstrap_chat`**

Create:

```rust
pub(super) async fn bootstrap_chat(args: Args) -> ChatBootstrap
```

Move the existing preparation logic from `run_chat` into this function, preserving order:

1. cwd resolution
2. config loading
3. logging initialization
4. config permission override
5. model selection
6. API key/base URL/max token/reasoning resolution
7. LLM client creation
8. task store creation
9. skill loading
10. tool registry registration
11. MCP spawn
12. hook runner creation
13. session ID selection and `set_session_id`
14. json logger and agent runner creation
15. prompt context / prompt parts / static prompt / system blocks
16. concurrency resolution
17. memory config
18. mode selection

Important: `_mcp_manager` currently lives only inside `run_chat`. If moving into `bootstrap_chat`, ensure the manager is kept alive for the same duration as before. If its type is hard to store, return it inside `ChatBootstrap` with a field name like `_mcp_manager`, or document why its current lifetime is preserved.

- [ ] **Step 3: Keep exits unchanged**

Any existing `eprintln!` + `std::process::exit(1)` behavior for model/API key validation must remain identical. Do not convert these to `Result` in this phase.

---

### Task 4: Dispatch runtime from ChatBootstrap

**Files:**
- Modify: `cli/src/run_orchestration.rs`
- Modify: `cli/src/run_orchestration/runtime.rs`

- [ ] **Step 1: Simplify `run_chat`**

After Task 3, `run_chat` should only:

1. initialize commands,
2. apply env permission override,
3. call `setup::bootstrap_chat(args).await`,
4. match `bootstrap.mode_selection`,
5. call runtime dispatch.

- [ ] **Step 2: Add bootstrap dispatch helpers**

Prefer adding in `runtime.rs`:

```rust
pub(super) async fn run_no_tui_from_bootstrap(bootstrap: ChatBootstrap) { ... }
pub(super) async fn run_tui_from_bootstrap(bootstrap: ChatBootstrap) { ... }
```

These helpers should construct `ChatLaunchOptions`, `NoTuiChatLaunch` / `TuiChatLaunch` and `ChatRuntimeContext` from the bootstrap fields, then call the existing application service adapter path.

After these helpers exist, consider making the old `run_no_tui` / `run_tui` private helper functions or removing them if they become simple duplicates.

- [ ] **Step 3: Preserve TUI model display**

TUI dispatch must continue using:

```rust
runtime::model_display(
    &resolved_model.source_key,
    &resolved_model.model.name,
    &resolved_model.model.id,
)
```

Do not change display format.

---

### Task 5: Update #47 docs status

**Files:**
- Modify: `docs/feature/active.md`

- [ ] **Step 1: Update table row #47**

Change #47 description to mention:

- Phase 1/2/3 completed Chat application boundary cleanup.
- Phase 4 plan focuses on Chat bootstrapping boundary objectification.
- This remains behavior-preserving DDD/COLA refactor, not distributed server work.

- [ ] **Step 2: Update #47 detail section**

Append a short “当前推进” paragraph under #47 detail section:

```markdown
**当前推进**：Phase 4 计划已新增 `docs/superpowers/plans/2026-05-24-feature-47-chat-bootstrapping-boundary-phase4.md`，下一步把 `run_orchestration::run_chat` 中的启动准备逻辑收束为 `ChatBootstrap` 与 `ChatModeSelection`，保持 CLI/TUI 行为不变。
```

Do not archive #47.

---

### Task 6: Verify

- [ ] **Step 1: Format**

```bash
cargo fmt --all -- --check
```

- [ ] **Step 2: Focused tests**

```bash
cargo test -p aemeath-cli run_orchestration
cargo test -p aemeath-cli application::chat
```

- [ ] **Step 3: Build/check**

```bash
cargo check -p aemeath-cli
```

If this phase changes only docs/plan and not code, still run `cargo check -p aemeath-cli` before commit.

---

### Task 7: Commit, merge, and clean up

- [ ] **Step 1: Inspect commit style**

Before committing, invoke the built-in `commit` skill and sample recent commit messages, including Co-Authored-By examples if present.

- [ ] **Step 2: Commit on feature branch**

Suggested title:

```text
docs: plan #47 chat bootstrapping boundary phase4
```

Include `refs #47` in the commit body or title, consistent with repository style.

- [ ] **Step 3: Merge to main**

From main worktree:

```bash
git merge --no-ff feature/47-ddd-redesign-plan
cargo check -p aemeath-cli
```

- [ ] **Step 4: Clean worktree**

Remove the completed worktree after main validation passes.

---

## Acceptance Criteria

- `docs/superpowers/plans/2026-05-24-feature-47-chat-bootstrapping-boundary-phase4.md` exists and is actionable.
- Plan explicitly preserves existing CLI/TUI behavior, prompt content, config priority, hook order, session ID generation and tool registration.
- `docs/feature/active.md` records #47 Phase 4 direction.
- `cargo check -p aemeath-cli` passes on branch and on main after merge.
