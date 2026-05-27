# Feature 47 P13: SDK 补齐 Resume/Session 通道

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** 让 `session_lifecycle.rs` 中的 `runtime::api::session::load_session`、`runtime::api::core::message::{sanitize_messages, check_message_integrity, deep_clean_messages}` 全部通过 SDK `AgentClient` 暴露，TUI resume 流程不再直接引用 runtime 类型。

## 背景

当前 `apps/cli/src/tui/session/session_lifecycle.rs` 有 18 处 `runtime::api` 引用：
- `load_session(id)` → resume 时加载历史 session
- `sanitize_messages(&mut msgs)` → 清理不完整消息
- `check_message_integrity(&msgs)` → 检查消息完整性
- `deep_clean_messages(&mut msgs)` → 深度修复
- `Role::User` / `Role::Assistant` → 角色映射
- `WorkspaceContext` → 工作区上下文
- `TaskStore::restore` → 恢复任务快照

## 步骤

- [ ] **1. SDK AgentClient 新增 `resume_session` 方法**
  - 签名：`async fn resume_session(&self, id: &str) -> Result<ResumeResult, SdkError>`
  - `ResumeResult` 包含：`messages: Vec<ChatMessage>`、`workspace: Option<WorkspaceContextView>`、`tasks: Option<TaskSnapshot>`、`created_at: Option<String>`、`trimmed: usize`、`repaired: usize`
  - 在 `packages/sdk/src/session.rs` 定义 `ResumeResult`

- [ ] **2. runtime AgentClientImpl 实现 `resume_session`**
  - 内部调用 `session::load_session` → `sanitize_messages` → `check_message_integrity` → `deep_clean_messages`
  - 返回已映射为 SDK 类型的 `ResumeResult`
  - 位置：`agent/runtime/src/client.rs`（后续 P17 拆分）

- [ ] **3. SDK AgentClient 新增 `restore_tasks` 方法**
  - 签名：`async fn restore_tasks(&self, snapshot: TaskSnapshot) -> Result<(), SdkError>`
  - `TaskSnapshot` 已存在于 SDK types 中，或需新增

- [ ] **4. 改写 `session_lifecycle.rs` 的 resume 逻辑**
  - 删除所有 `runtime::api` import
  - `App::run` 中 resume 分支改为调用 `self.agent_client.resume_session(id)`
  - Task restore 改为调用 `self.agent_client.restore_tasks(snapshot)`

- [ ] **5. 删除 `session_lifecycle.rs` 中的 `message_to_sdk` 辅助函数**
  - 消息映射已在 runtime 侧完成，TUI 直接使用 SDK `ChatMessage`

- [ ] **6. 验证**
  - `cargo build -p cli` 编译通过
  - `cargo test -p cli` 通过
  - 手动 `--resume <id>` 功能正常
