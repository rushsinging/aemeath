# Feature 47 P13: TUI 完全 SDK 化——消除所有 runtime::api 引用

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** `apps/cli/src/tui/` 目录下零 `runtime::api` 引用（`tokio::runtime` 除外），所有 runtime 能力走 `sdk::AgentClient`。`run_orchestration.rs` 瘦身到 ~40 行。删除 `runtime_adapter.rs`。

## 当前状态

TUI 中残留 `runtime::api` 引用的文件（除 `run_orchestration.rs` 外）：
- `session/session_lifecycle.rs` — resume、消息清理、WorkspaceContext
- `core/cmd_exec.rs` — HookRunner、ModelsConfig、LlmClient、SessionReminders
- `core/slash/` — 可能残留
- `core/mod.rs` — App::new() 中 runtime 类型构造

`run_orchestration.rs` 有 18+ 参数逐字段赋值给 App，`runtime_adapter.rs` 17 行桥接代码。

## 步骤

### Part A：SDK AgentClient 补齐缺失方法

- [ ] **1. `resume_session(&self, id: &str) -> Result<ResumeResult, SdkError>`**
  - runtime 侧内部调用 `session::load_session` → `sanitize_messages` → `check_message_integrity` → `deep_clean_messages`
  - `ResumeResult` 含 `messages: Vec<ChatMessage>`、`workspace: Option<WorkspaceContextView>`、`tasks: Option<TaskSnapshot>`、`created_at`、`trimmed`、`repaired`
  - 定义在 `packages/sdk/src/session.rs`

- [ ] **2. `run_hook_notification(&self, message: &str, kind: &str) -> Result<(), SdkError>`**
  - runtime 侧调用 `HookRunner::on_notification`

- [ ] **3. `set_current_turn(&self, turn: u64)`**
  - runtime 侧调用 `bootstrap::set_current_turn`

- [ ] **4. `is_reasoning(&self) -> bool` / `context_size(&self) -> usize` / `allow_all(&self) -> bool` / `model_display(&self) -> &str`**
  - runtime 侧从内部 state 读取

### Part B：改写 TUI 消费者

- [ ] **5. `session_lifecycle.rs`：resume 走 SDK**
  - 删除所有 `runtime::api` import
  - resume 分支改为 `agent_client.resume_session(id)`
  - Task restore 改为 `agent_client.restore_tasks(snapshot)`（如需）

- [ ] **6. `CmdExecutor` 去 runtime 字段**
  - 删除 `client: LlmClient`、`models_config: ModelsConfig`、`session_reminders: SessionReminders`、`hook_runner: HookRunner` 字段
  - 只保留 `agent_client: Arc<dyn AgentClient>`
  - `RunHookNotification` → `agent_client.run_hook_notification()`
  - `SetCurrentTurn` → `agent_client.set_current_turn()`

- [ ] **7. `App::run()` 签名简化**
  - 从 18 参数 → `(client: Arc<dyn AgentClient>, resume_id: Option<String>)`
  - 内部通过 `client.is_reasoning()` / `client.context_size()` 等获取值

- [ ] **8. `run_orchestration.rs` 极简化**
  - 删除逐字段赋值，改为：`from_args()` → `App::new()` → `app.run(client, resume_id)`
  - `init_all()` / `init_panic_hook()` 移入 `from_args()`

- [ ] **9. 删除 `runtime_adapter.rs`**

- [ ] **10. 删除 `TuiLaunchContext`**（如已无消费者）

### Part C：验证

- [ ] **11. `grep -rn 'runtime::api' apps/cli/src/tui/` 返回空**
- [ ] **12. `cargo build -p cli` + `cargo test -p cli` 通过**
- [ ] **13. 完整 TUI 流程 smoke test：启动 → 聊天 → 工具调用 → slash 命令 → 退出 → resume**
