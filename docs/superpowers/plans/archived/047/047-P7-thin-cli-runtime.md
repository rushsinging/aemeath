# Feature 47 Thin CLI Runtime Phase 2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 优先瘦身 `apps/cli`，把当前 CLI 中的 agent runner 与 chat application 契约迁移到 `crates/runtime`，让 CLI 更接近只负责入口与 TUI/REPL adapter。

**Architecture:** 先迁移低 UI 耦合的运行时职责：`application/chat` 的 request/port/service 进入 `runtime::chat`，`agent_runner` 进入 `runtime::agent_runner`。CLI 保留真实 TUI/REPL 执行 adapter 与启动参数解析，继续通过 `runtime::api` 使用运行时 API；后续 checkpoint 再迁移 `run_orchestration` 中仍依赖 CLI UI 的部分。

**Tech Stack:** Rust workspace、Cargo path dependencies、async_trait、runtime facade、cargo check/test、architecture guards。

---

### Task 1: Move chat application contract to runtime

**Files:**
- Create: `crates/runtime/src/chat/mod.rs`
- Create: `crates/runtime/src/chat/request.rs`
- Create: `crates/runtime/src/chat/port.rs`
- Create: `crates/runtime/src/chat/service.rs`
- Modify: `crates/runtime/src/lib.rs`
- Modify: `crates/runtime/src/api.rs`
- Modify: `apps/cli/src/application/main_loop/mod.rs`
- Modify: `apps/cli/src/application/main_loop/request.rs`
- Modify: `apps/cli/src/application/main_loop/port.rs`
- Modify: `apps/cli/src/application/main_loop/service.rs`
- Modify: `apps/cli/src/run_orchestration/setup.rs`
- Modify: `apps/cli/src/run_orchestration/runtime.rs`

- [x] **Step 1: Move current source files**

Run:

```bash
mkdir -p crates/runtime/src/chat
mv apps/cli/src/application/main_loop/request.rs crates/runtime/src/chat/request.rs
mv apps/cli/src/application/main_loop/port.rs crates/runtime/src/chat/port.rs
mv apps/cli/src/application/main_loop/service.rs crates/runtime/src/chat/service.rs
```

Expected: files exist under `crates/runtime/src/chat`.

- [x] **Step 2: Create runtime chat module**

Write `crates/runtime/src/chat/mod.rs`:

```rust
pub mod port;
pub mod request;
pub mod service;

pub use port::{ChatRuntimeContext, ChatRuntimePort, TuiChatOutcome};
pub use request::{ChatLaunchOptions, NoTuiChatLaunch, TuiChatLaunch};
pub use service::ChatApplicationService;
```

- [x] **Step 3: Export runtime chat module**

Update `crates/runtime/src/lib.rs`:

```rust
pub mod agent_runner;
pub mod api;
pub mod chat;
```

If `agent_runner` has not been created yet, add only `pub mod api; pub mod chat;` in this task and add `agent_runner` in Task 2.

Update `crates/runtime/src/api.rs`:

```rust
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

- [x] **Step 4: Replace moved module imports**

In moved runtime files, replace:
- `use ::runtime::api::core::...` with `use crate::api::core::...`
- `use ::runtime::api::provider::...` with `use crate::api::provider::...`
- `use crate::application::chat::request::ChatLaunchOptions;` in service tests with `use crate::chat::request::ChatLaunchOptions;`

- [x] **Step 5: Turn CLI chat module into re-export shim**

Write `apps/cli/src/application/main_loop/mod.rs`:

```rust
pub(crate) use ::runtime::api::chat::{
    ChatApplicationService, ChatLaunchOptions, ChatRuntimeContext, ChatRuntimePort,
    NoTuiChatLaunch, TuiChatLaunch, TuiChatOutcome,
};
```

Delete empty `apps/cli/src/application/main_loop/request.rs`, `port.rs`, and `service.rs` from the CLI tree after updating `mod.rs`.

- [x] **Step 6: Verify chat contract moved**

Run:

```bash
cargo test -p runtime chat::request
cargo test -p runtime chat::service
cargo check -p cli
```

Expected: all pass.

### Task 2: Move agent runner to runtime

**Files:**
- Move: `apps/cli/src/agent_runner.rs` → `crates/runtime/src/agent_runner.rs`
- Move: `apps/cli/src/agent_runner/*` → `crates/runtime/src/agent_runner/*`
- Modify: `crates/runtime/src/lib.rs`
- Modify: `crates/runtime/src/api.rs`
- Modify: `apps/cli/src/main.rs`
- Modify: `apps/cli/src/run_orchestration/setup/runtime_support.rs`
- Modify: any CLI reference to `crate::agent_runner`

- [x] **Step 1: Move agent_runner module**

Run:

```bash
mkdir -p crates/runtime/src/agent_runner
mv apps/cli/src/agent_runner.rs crates/runtime/src/agent_runner.rs
mv apps/cli/src/agent_runner/* crates/runtime/src/agent_runner/
rmdir apps/cli/src/agent_runner
```

Expected: CLI no longer has `apps/cli/src/agent_runner.rs` or `apps/cli/src/agent_runner/`.

- [x] **Step 2: Export runtime agent runner**

Update `crates/runtime/src/lib.rs`:

```rust
pub mod agent_runner;
pub mod api;
pub mod chat;
```

Update `crates/runtime/src/api.rs` to include:

```rust
pub use crate::agent_runner;
pub use crate::chat;
```

- [x] **Step 3: Replace imports in moved agent_runner**

In `crates/runtime/src/agent_runner/**/*.rs`, replace:
- `use ::runtime::api::core::...` with `use crate::api::core::...`
- `use ::runtime::api::provider::...` with `use crate::api::provider::...`
- `::runtime::api::tools::` with `crate::api::tools::`
- any `crate::agent_runner` path remains valid because it is now inside runtime crate.

- [x] **Step 4: Update CLI module declarations**

Remove `mod agent_runner;` from `apps/cli/src/main.rs`.

- [x] **Step 5: Update CLI construction references**

In `apps/cli/src/run_orchestration/setup/runtime_support.rs`, replace `crate::agent_runner::CliAgentRunner` with `::runtime::api::agent_runner::CliAgentRunner`.

If any other CLI file imports `crate::agent_runner`, replace with `::runtime::api::agent_runner`.

- [x] **Step 6: Verify agent runner moved**

Run:

```bash
cargo test -p runtime agent_runner
cargo check -p cli
```

Expected: all pass.

### Task 3: Tighten CLI thinness tests and architecture guard evidence

**Files:**
- Modify: `.agents/hooks/check-forbidden-imports.sh` if runtime module names create false positives.
- Modify: `docs/feature/active.md`
- Modify: `docs/feature/specs/047-ddd-redesign.md`

- [x] **Step 1: Verify CLI no longer declares agent runner**

Run:

```bash
python3 - <<'PY'
from pathlib import Path
main = Path('apps/cli/src/main.rs').read_text()
assert 'mod agent_runner;' not in main
assert Path('apps/cli/src/agent_runner.rs').exists() is False
assert Path('apps/cli/src/agent_runner').exists() is False
print('agent_runner moved out of cli')
PY
```

Expected: prints `agent_runner moved out of cli`.

- [x] **Step 2: Verify runtime exposes agent runner and chat API**

Run:

```bash
python3 - <<'PY'
from pathlib import Path
api = Path('crates/runtime/src/api.rs').read_text()
assert 'pub use crate::agent_runner;' in api
assert 'pub use crate::chat;' in api
print('runtime api exposes agent_runner and chat')
PY
```

Expected: prints `runtime api exposes agent_runner and chat`.

- [x] **Step 3: Update #47 active status**

In `docs/feature/active.md`, update #47 current progress to mention:

```text
Phase 2 已开始瘦身 apps/cli：chat application 契约与 sub-agent runner 已迁移到 crates/runtime；CLI 继续保留 TUI/REPL adapter 与启动参数解析，后续再迁移 run_orchestration 中剩余的 runtime bootstrap 逻辑。
```

- [x] **Step 4: Update #47 spec checkpoint note**

In `docs/feature/specs/047-ddd-redesign.md`, update checkpoint 5 or add a note after checkpoint list:

```text
Phase 2 checkpoint：先迁移低 UI 耦合的 chat application contract 与 agent_runner 到 runtime，保留 TUI/REPL adapter 在 apps/cli；run_orchestration 需在后续继续拆成 runtime bootstrap API 与 CLI adapter。
```

- [x] **Step 5: Run architecture guards**

Run:

```bash
.agents/hooks/check-architecture-guards.sh
```

Expected: pass.

### Task 4: Full verification, commit, merge

**Files:**
- All changed files

- [x] **Step 1: Run full verification in worktree**

Run:

```bash
cargo fmt --all -- --check
cargo check
cargo test -p runtime chat
cargo test -p runtime agent_runner
cargo test -p cli run_orchestration
cargo test
./build_cli.sh
.agents/hooks/check-architecture-guards.sh
.agents/hooks/check-unit-tests.sh
```

Expected: all pass.

- [x] **Step 2: Commit changes**

Commit message:

```text
refactor: 将运行时职责迁移到 runtime (refs #47)
```

Include repository-standard AI co-author trailer if recent commits use it.

- [x] **Step 3: Merge to main and verify**

Exit worktree, merge branch `feature/47-thin-cli-runtime` into `main`, rerun full verification on main, then remove `.worktrees/feature-47-thin-cli-runtime` and delete the branch.
