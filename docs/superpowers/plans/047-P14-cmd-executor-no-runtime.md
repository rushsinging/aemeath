# Feature 47 P14: CmdExecutor 去 runtime 依赖

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** 让 `CmdExecutor` 不再直接引用 `runtime::api` 类型（`LlmClient`、`ModelsConfig`、`SessionReminders`、`HookRunner`），所有副作用通过 SDK `AgentClient` 走。

## 背景

当前 `apps/cli/src/tui/core/cmd_exec.rs` 有 6 处 `runtime::api` 引用：
- `runtime::api::core::config::ModelsConfig` — 字段类型
- `runtime::api::core::tool::SessionReminders` — 字段类型
- `runtime::api::hook::hook::HookRunner` — 字段类型
- `runtime::api::provider::client::LlmClient` — 字段类型

`CmdExecutor` 持有 4 个 runtime 类型字段 + 1 个 `Arc<dyn AgentClient>`，但实际只有 `hook_runner` 和 `agent_client` 被使用。

## 步骤

- [ ] **1. 识别 CmdExecutor 各字段的实际使用**
  - `client` — 未使用（`LlmClient` 直连已废弃）
  - `models_config` — 未使用（默认值占位）
  - `session_reminders` — 未使用
  - `hook_runner` — `RunHookNotification` Cmd 使用
  - `agent_client` — `ReadClipboardImage` / `ProcessImageFile` / `SetCurrentTurn` 使用

- [ ] **2. SDK AgentClient 新增 `run_hook_notification` 方法**
  - 签名：`async fn run_hook_notification(&self, message: &str, kind: &str) -> Result<(), SdkError>`
  - runtime 侧调用 `HookRunner::on_notification`

- [ ] **3. SDK AgentClient 新增 `set_current_turn` 方法**
  - 签名：`fn set_current_turn(&self, turn: u64)`
  - runtime 侧调用 `bootstrap::set_current_turn`

- [ ] **4. 改写 CmdExecutor**
  - 删除 `client`、`models_config`、`session_reminders`、`hook_runner` 字段
  - 只保留 `agent_client: Arc<dyn AgentClient>`
  - `RunHookNotification` → `agent_client.run_hook_notification()`
  - `ReadClipboardImage` → `agent_client.read_clipboard_image()`（已有）
  - `ProcessImageFile` → `agent_client.process_image_file()`（已有）
  - `SetCurrentTurn` → `agent_client.set_current_turn()`

- [ ] **5. 清理 App::new() 中的 runtime 类型初始化**
  - `mod.rs` 中 `CmdExecutor` 构造不再需要 `ModelsConfig::default()` / `HookRunner::empty()` / `SessionReminders::new()`
  - 删除 `run_orchestration.rs` 中对 `app.cmd_exec.hook_runner` 的赋值

- [ ] **6. 验证**
  - `cargo build -p cli` 编译通过
  - `cargo test -p cli` 通过
  - hook notification 功能正常
