# Feature 47 P13: TUI 完全 SDK 化——消除所有 runtime::api 引用

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** `apps/cli/src/tui/` 目录下零 `runtime::api` 引用（`tokio::runtime` 除外），所有 runtime 能力走 `sdk::AgentClient`。`run_orchestration.rs` 吸收 `runtime_adapter.rs`，瘦身到 ~40 行。删除 `runtime_adapter.rs`。

## 当前状态

  - ✅ `core/cmd_exec.rs` — 已删除（上轮）
  - ✅ `core/run_loop.rs` — Cmd 副作用全内联通过 AgentClient（上轮）
  - ✅ `session/session_lifecycle.rs` — resume 走 SDK，18 参数 → 2 参数（本轮）
  - ✅ `core/mod.rs` — 本地 SessionReminders 替代 runtime 类型（本轮）
  - ✅ `core/slash.rs` — 无残留（仅注释中提到，无实际引用）
  - ✅ `run_orchestration.rs` + `runtime_adapter.rs` — 已合并，runtime_adapter 已删除（本轮）
  - ✅ SDK `SessionSnapshot` 新增 `tasks` 字段（本轮）

  ## 步骤

  ### Part A：SDK AgentClient 补齐缺失方法

  - [✓] **1. `resume_session`** — 调整：runtime `load_session` 内部清洗替代独立方法
  - [✓] **2. `notify_hook`** — ✅ 上轮已实现
  - [✓] **3. `restore_tasks`** — ✅ 上轮已实现
  - [✓] **4. `get_thinking`** — ✅ 上轮已实现

  > **调整**：不再新增 `resume_session` 独立方法。改为在 runtime `load_session` 内部完成消息清洗（sanitize + deep_clean），让 CLI 拿到的 SessionSnapshot 已清洗完成。

  ### Part B：改写 TUI 消费者

  - [✓] **5. Runtime `load_session` 内部清洗**
    - ✅ `load_session()` 内部调 `sanitize_messages` → `check_message_integrity` → `deep_clean_messages`
    - ✅ SessionSnapshot 的 `trimmed` / `repaired` 由 runtime 填充
    - ✅ SessionSnapshot 新增 `tasks: Option<serde_json::Value>` 字段

  - [✓] **6. `session_lifecycle.rs` 消除所有 runtime::api 引用**
    - ✅ resume 分支：`agent_client.load_session(id)`
    - ✅ 删除消息清洗逻辑（已移至 runtime）
    - ✅ 删除类型转换函数
    - ✅ `run()` 签名从 18 参数 → `(agent_client, resume_id)`

  - [✓] **7. `CmdExecutor` 已删除** ✅ 上轮完成

  - [✓] **8. `run_orchestration.rs` 吸收 `runtime_adapter.rs`**
    - ✅ `agent_client_from_args` 内联到 `run_orchestration.rs`
    - ✅ `set_current_turn` 内联到 `run_orchestration.rs`
    - ✅ 删除 `runtime_adapter.rs` 文件和 module 声明
    - ✅ main.rs 调用方更新

  - [✓] **9. `mod.rs` + `slash.rs` 残留清理**
    - ✅ `mod.rs` 用本地 `SessionReminders` 替代 `::runtime::api::core::tool::SessionReminders`
    - ✅ 新增 `tui/core/reminder.rs` 独立类型
    - ✅ `slash.rs` 无实际引用

  ### Part C：验证

  - [✓] **10. `grep -rn 'runtime::api' apps/cli/src/tui/` 返回空**
  - [✓] **11. `cargo build -p cli` + `cargo test -p cli` 通过**
  - [✓] **12. 全量 `cargo test` 通过（849 tests, 0 fail）**
