# Feature 47: Core 模块按 DDD Bounded Context 拆分实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 `crates/core` 中残留的非内核职责逐步拆分到对应 support domain crate（policy、project、prompt、storage、tools、hook、runtime），使 core 收敛为最小共享内核（error、message、string_idx、ApiDriverKind、token_estimation）。

**Architecture:** 按 DDD Bounded Context 逐域拆分。每个 batch 内按依赖耦合度从低到高排序，优先处理无内部依赖的模块。storage crate 已依赖 core（`aemeath_core`），迁移时只改 `crate::*` → `aemeath_core::*`，不改变函数签名与行为。

**Tech Stack:** Rust workspace、Cargo path dependencies、cargo check/test。

---

## 当前状态

### 已完成（worktree feature/47-split-core-provider，待合并）

| 改动 | 说明 |
|---|---|
| `crates/provider/src/api.rs` 新建 | 为 provider crate 建立 DDD api 模块模式，re-export `ApiDriverKind` |
| `core::security` → `crates/policy` | `SecurityWarning` / `scan_content` / `format_warnings` 迁移到 policy crate |
| `core::permission.rs` 删除 | 死代码清理（`PermissionManager` 未在任何模块中被引用） |

### core 剩余模块清单

| 模块 | 行数 | 目标 crate | 内部依赖 | 外部引用 |
|---|---|---|---|---|
| `logging/` + `logging.rs` | ~350 | `storage` | `config::logging`, `config::paths` | CLI 通过 `runtime::api::core::logging` |
| `history.rs` | ~220 | `storage` | 待分析 | 待分析 |
| `tool_result_storage.rs` | ~160 | `storage` | 待分析 | 待分析 |
| `cost/` | ~100 | `storage` | 待分析 | CLI status_bar, slash |
| `session/` | ~550 | `storage` | `config`, `message`, `tool`, `state`, `worktree` | runtime, CLI |
| `memory/` | ~800 | `storage` | `config`, `session`, `message` | runtime, CLI |
| `state/` | ~250 | `storage` | `config` | runtime |
| `worktree.rs` | ~310 | `project` | `session`, `tool`(ToolContext) | tool.rs |
| `skill/` | ~200 | `prompt` | `config`, `message` | runtime |
| `hook/` | ~1400 | `hook` | `config`, `message`, `session`, `tool`, `logging` | runtime, CLI |
| `tool.rs` | ~310 | `tools` | `worktree`, `compact`, `session`, `agent` | 大量引用 |
| `mcp/` + `mcp.rs` | ~400 | `tools` | `config`, `tool` | runtime |
| `mcp_manager/` + `mcp_manager.rs` | ~300 | `tools` | `config`, `mcp`, `tool` | runtime |
| `agent.rs` + `agent_tests.rs` | ~400 | `runtime` | `config`, `tool`, `session` | runtime, CLI |
| `compact/` | ~500 | `runtime` | `message`, `config`, `token_estimation` | runtime hook |
| `reflection/` | ~700 | `runtime` | `config`, `message`, `session`, `memory`, `tool` | runtime |
| `scheduler/` | ~150 | `runtime` | `config` | runtime |
| `task/` | ~700 | `runtime` | `config`, `session` | runtime, CLI |
| `command/` | ~450 | `runtime` | `config`, `session`, `cost` | runtime, CLI |
| `token_estimation.rs` | ~350 | 保留在 core | 无 | compact/ |
| `config/` | ~2500 | 保留在 core | 大量 | 几乎所有 crate |

---

## 拆分 Batch 规划

### Batch 2: Storage Domain（按耦合度从低到高）

#### Task 2.1: `logging/` → `crates/storage`

**Files:**
- Create: `crates/storage/src/logging/mod.rs`
- Create: `crates/storage/src/logging/json.rs`
- Create: `crates/storage/src/logging/text.rs`
- Create: `crates/storage/src/logging/rotation.rs`
- Create: `crates/storage/src/logging/tests.rs`
- Modify: `crates/storage/Cargo.toml`
- Modify: `crates/storage/src/lib.rs`
- Modify: `crates/storage/src/api.rs`
- Modify: `crates/runtime/src/api.rs`
- Modify: `apps/cli/` 中所有引用 `runtime::api::core::logging` 的文件
- Modify: `crates/core/src/lib.rs`（移除 `pub mod logging;`）
- Modify: `crates/core/src/logging.rs`（删除）
- Delete: `crates/core/src/logging/`

**依赖分析:**
- `logging/json.rs` 依赖 `crate::config::logging::LoggingConfig` → 改为 `aemeath_core::config::logging::LoggingConfig`
- `logging/text.rs` 依赖 `crate::config::paths` → 改为 `aemeath_core::config::paths`
- `logging/rotation.rs` 依赖 `super::LOG_MAX_BYTES/LOG_MAX_BACKUPS/LOG_RETENTION_DAYS` → 路径不变
- `logging/tests.rs` 依赖 `crate::config::LoggingConfig` → 改为 `aemeath_core::config::LoggingConfig`

**storage/Cargo.toml 新增依赖:**
```toml
serde_json = { workspace = true }
chrono = "0.4"
```

**CLI 引用更新路径:**
- `apps/cli/src/tui/app/mod.rs:89`: `runtime::api::core::logging::JsonLogger` → `runtime::api::storage::logging::JsonLogger`
- `apps/cli/src/tui/app/processing.rs:205,229`:同上
- `apps/cli/src/repl/mod.rs:53`:同上
- `apps/cli/src/repl/streaming.rs:19`:同上

#### Task 2.2: `history.rs` → `crates/storage`

**Files:**
- Create: `crates/storage/src/history.rs`
- Modify: `crates/storage/src/lib.rs`
- Modify: `crates/storage/src/api.rs`
- Modify: `crates/core/src/lib.rs`（移除 `pub mod history;`）
- Delete: `crates/core/src/history.rs`

#### Task 2.3: `tool_result_storage.rs` → `crates/storage`

**Files:**
- Create: `crates/storage/src/tool_result_storage.rs`
- Modify: `crates/storage/src/lib.rs`
- Modify: `crates/storage/src/api.rs`
- Modify: `crates/core/src/lib.rs`
- Delete: `crates/core/src/tool_result_storage.rs`

### Batch 3: Project Domain

#### Task 3.1: `worktree.rs` → `crates/project`

**前置条件:** 需要先将 `WorkingContext` 类型与 `ToolContext` 解耦，或将 `enter_worktree`/`exit_worktree` 方法保留为 `ToolContext` 上的方法（委托到 project crate）。

**Files:**
- Create: `crates/project/src/worktree.rs`
- Modify: `crates/project/Cargo.toml`（新增 `serde` 等依赖）
- Modify: `crates/project/src/lib.rs`
- Modify: `crates/project/src/api.rs`
- Modify: `crates/core/src/tool.rs`（更新 `crate::worktree::*` → `project::worktree::*`）
- Modify: `crates/core/src/lib.rs`
- Delete: `crates/core/src/worktree.rs`

### Batch 4: Tools Domain

#### Task 4.1: `tool.rs` 中 worktree 无关部分 → `crates/tools`

注意：`tool.rs` 定义了 `ToolContext`，这是整个系统的核心类型。迁移需谨慎。

### Batch 5: Runtime Domain

将 `agent.rs`、`compact/`、`reflection/`、`scheduler/`、`task/`、`command/` 迁入 `crates/runtime`。

### Batch 6: Remaining

- `hook/` → `crates/hook`
- `skill/` → `crates/prompt`
- `cost/` → `crates/storage`
- `session/` + `memory/` + `state/` → `crates/storage`

---

## 实施约束

1. 每个 Task 完成后运行 `cargo check` 确保编译
2. 每个 Batch 完成后运行 `cargo test` 确保测试通过
3. 每个 Task 独立提交
4. CLI/TUI 行为不变
5. 不新增 crate，只往已有 skeleton crate 中填充
6. `core` 最终保留: `error.rs`, `message/`, `string_idx/`, `provider.rs`(ApiDriverKind), `token_estimation.rs`, `config/`

---

## 当前进展

- [x] **Batch 1**: provider api.rs 模式 + security → policy + 死代码清理（已完成，在 worktree feature/47-split-core-provider 中）
- [x] **Batch 2**: Storage domain (logging, history, tool_result_storage) — 已完成，logging/history/tool_result_storage 从 core 迁入 crates/storage
- [x] **Batch 3**: Project domain (worktree) — 已完成，worktree 从 core 迁入 services/project，WorkingContext 留在 core::tool 避免循环依赖；P11 新增 services/share 解决 tools→project 跨 service 依赖
- [ ] **Batch 4**: Tools domain (tool, mcp)
- [ ] **Batch 5**: Runtime domain (agent, compact, reflection, scheduler, task, command)
- [ ] **Batch 6**: Remaining (hook, skill, cost, session, memory, state)
