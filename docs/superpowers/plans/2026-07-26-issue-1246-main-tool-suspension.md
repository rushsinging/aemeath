# Issue #1246 Main Tool Suspension to Shared Loop Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development (recommended) or executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Main `AskUserQuestion` 的 typed suspension 从 legacy `ask_user()` oneshot waiter 切换到 `InteractionBridge`，按原 ToolCall 顺序逐个注册、逐个 resolve，TUI 经 `AgentClient::reply_interaction` / `cancel_interaction` 回复纯值。

**Architecture:** `execute_tool_round` 中拦截 AskUserQuestion suspension 后，不再调用 `ask_user()`。改为通过 `InteractionBridge` 逐个注册 `InteractionRequest { body: UserQuestions }`，发布纯值 SDK event（不携 sender），等待 `InteractionCompletion`。`AgentClientImpl` 的 `reply_interaction` / `cancel_interaction` 委托 `InteractionBridge`，返回 typed outcome。reply 映射为 ToolSuccess，cancel 映射为 ToolCancelled。旧 `ask_user()` 和 `AskUserBatch { reply_tx }` 由 #879 物理退役。

**Tech Stack:** Rust、Tokio、async_trait、InteractionBridge、Run aggregate、Cargo tests。

---

## 范围与承接

### 本 PR 包含

- 废除 `execute_tool_round` 中对 `ask_user()` 的调用。
- `InteractionBridge` 注入 `RuntimeHandle` 和 Main adapter。
- AskUserQuestion suspension 通过 `InteractionBridge` 逐个注册/等待。
- 发纯值 `ChatEvent::InteractionRequested`（不携 sender）。
- `AgentClientImpl::reply_interaction` / `cancel_interaction` 委托 bridge。
- TUI `event_mapping` 将 `InteractionRequested` 映射到 TUI interaction model。
- TUI `AskUserBatch` 旧路径保留兼容但不走生产。
- Main 两个 suspension 稳定串行 L4 场景测试。
- 各层 L1-L4 测试、文档回写。

### 明确不包含

- TUI 全量 ACL、六 Context 私有化与完整 TEA 重构：#943/#944。
- `CancelRunStep` / `TerminateRun` 生产控制流：#1247（已完成）。
- Sub parent-mediated adapter、Hook 与 reasoning 装配：#1248。
- 旧 AskUser / TUI sender 的最终物理退役：#879/#947。

## 文件结构

- Modify: `agent/features/runtime/src/application/chat/looping/tools.rs` — 废除 `ask_user()` 调用，改为 InteractionBridge 逐个 resolve。
- Modify: `agent/features/runtime/src/application/chat/looping/events.rs` — 不再在生产路径发 `AskUserBatch { reply_tx }`。
- Modify: `agent/features/runtime/src/application/chat/looping/main_run_port.rs` — 接入 InteractionBridge 引用。
- Modify: `agent/features/runtime/src/application/client/{accessors.rs,trait_impl.rs}` — reply/cancel 委托 bridge。
- Modify: `agent/features/runtime/src/application/chat/looping/loop_runner.rs` — 传入 bridge 或通过 accessors 获取。
- Modify/Test: `agent/features/runtime/src/application/chat/looping/loop_runner_tests.rs` — L4 场景。
- Modify/Test: `agent/features/runtime/src/application/interaction.rs` — 如需适配 Main adapter 用法。
- Modify/Test: `apps/cli/src/tui/effect/session/processing/event_mapping.rs` — 映射 `InteractionRequested`。
- Modify: `docs/design/02-modules/runtime/{01-domain-model.md,03-loop-and-state-machine.md,06-ports-and-adapters.md}` — 回写。
- Modify: `docs/design/03-engineering/03-migration-governance.md` — R8/T5 进度。

## Task 1: 将 InteractionBridge 注入 RuntimeHandle（L2）

**Files:**
- Modify: `agent/features/runtime/src/application/client/accessors.rs`
- Modify: `agent/features/runtime/src/application/client/from_args.rs`
- Modify: `agent/features/runtime/src/application/client/trait_impl.rs`

- [ ] **Step 1: 写失败的 trait_impl 委托测试**

验证 `AgentClientImpl::reply_interaction` 和 `cancel_interaction` 返回非默认 `NotFound`，而是委托 `InteractionBridge`。

- [ ] **Step 2: 注入 bridge**

在 `RuntimeHandle` 增加 `Arc<InteractionBridge>`；`from_args` 构造默认实例；`trait_impl` 委托 bridge。

- [ ] **Step 3: 运行测试确认通过**

- [ ] **Step 4: 提交**

## Task 2: 废除 ask_user() oneshot，改用 InteractionBridge（L2/L4）

**Files:**
- Modify: `agent/features/runtime/src/application/chat/looping/tools.rs`
- Modify: `agent/features/runtime/src/application/chat/looping/main_run_port.rs`
- Modify: `agent/features/runtime/src/application/chat/looping/loop_runner.rs`
- Modify: `agent/features/runtime/src/application/chat/looping/events.rs`
- Modify: `agent/features/runtime/src/application/chat/looping/loop_runner_tests.rs`

- [ ] **Step 1: 写失败测试：InteractionBridge resolve 后返回正确 Tool result**

使用 deterministic bridge，验证 reply 映射为 ToolSuccess，cancel 映射为 ToolCancelled。

- [ ] **Step 2: 重构 execute_tool_round**

将 AskUserQuestion suspension 从 `ask_user()` 切换为：按 ToolCall 顺序逐个通过 bridge 注册 `InteractionRequest`、发纯值 event、等待 completion。

- [ ] **Step 3: 运行测试确认通过**

- [ ] **Step 4: 写失败测试：两个 suspension 稳定串行**

验证两个 AskUserQuestion 按 ToolCall 顺序逐个发布，Run 至多一个 PendingInteraction。

- [ ] **Step 5: 运行测试确认通过**

- [ ] **Step 6: 提交**

## Task 3: TUI event_mapping 映射 InteractionRequested（L3/L4）

**Files:**
- Modify: `apps/cli/src/tui/effect/session/processing/event_mapping.rs`
- Modify: 相关 TUI 测试

- [ ] **Step 1: 确认 TUI 已能消费 InteractionRequested**

#944/#943 已建立 TUI interaction model 与 Effect executor 的 `reply_interaction` / `cancel_interaction` 闭环。确认 `InteractionRequested` 被正确映射到 TUI `InteractionRequest`。

- [ ] **Step 2: 写相邻层测试**

验证 `ChatEvent::InteractionRequested` → TUI `InteractionRequest` → `AgentClient::reply_interaction` 的完整链路。

- [ ] **Step 3: 运行测试确认通过**

- [ ] **Step 4: 提交**

## Task 4: 生产可达测试与 reply/cancel race（L4）

**Files:**
- Modify: `agent/features/runtime/src/application/chat/looping/loop_runner_tests.rs`

- [ ] **Step 1: 写生产可达测试**

验证生产路径不再调用 legacy `ask_user()` / `AskUserBatch { reply_tx }`。

- [ ] **Step 2: 写 reply/cancel race 测试**

验证 reply 和 cancel 竞争时确定性结果；stream teardown 不悬挂。

- [ ] **Step 3: 运行测试确认通过**

- [ ] **Step 4: 提交**

## Task 5: 回写文档与完整门禁

**Files:**
- Modify: `docs/design/02-modules/runtime/{01-domain-model.md,03-loop-and-state-machine.md,06-ports-and-adapters.md}`
- Modify: `docs/design/03-engineering/03-migration-governance.md`

- [ ] **Step 1: 回写最终生产路径**

- [ ] **Step 2: 执行完整门禁**

Run:

```bash
cargo fmt --all -- --check
cargo check -p runtime -p sdk -p cli
cargo clippy -p runtime -p sdk -p cli --all-targets -- -D warnings
cargo test -p runtime --lib application::loop_engine application::active_run application::interaction
cargo test -p sdk --test run_control_contract
cargo test -p cli --bin aemeath -- test_processing_handle
bash .agents/hooks/check-architecture-guards.sh
```

- [ ] **Step 3: Issue 回填与 PR 前检查**
