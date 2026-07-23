# Issue #1247 Main Agent Run Control Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development (recommended) or executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 仅将 `CancelRunStep` / `TerminateRun` 原子切入 Main Agent 的生产控制链，使 Main shared Loop 能区分 Step cancel 与 Run terminate，并保留 #1272 drain-or-seal、#1277 accepted input、#1278 finalized outcome 的既有语义。

**Architecture:** 在同一变更中将新的纯值控制命令发布到 `AgentClient`，并将 #878 的迁移期守卫切换为目标 API/纯值边界守卫。`ActiveRunRegistry` 成为 Main Run 唯一控制 owner，保存 root token、当前 Step token 与 typed control；Main `RunLoopPort` 在每个 await 返回后经同一 barrier 消费控制。CancelStep 只收口当前 Step 后回到现有 `DrainingInput`，TerminateRun 收口后成为 `Terminated`。当前旧 AskUser oneshot 只由既有 cancellation scope 中断；`InteractionBridge::drain_run` 与 Main suspension 生产接线由 #1246 完成。Sub Agent、父子递归 terminate 与 shared deadline 传播明确不在本计划，留给后续 Main/Sub Loop 统一工作。

**Tech Stack:** Rust、Tokio、async_trait、SDK Published Language、Run aggregate、ContextPort、Cargo tests。

---

## 范围与承接

### 本 PR 包含

- Main `AgentClient::cancel_run_step` / `terminate_run` 的纯值同步入口。
- Main ActiveRun typed control registry、当前 Step scope 与 Run terminate deadline。
- Main shared Loop 在 drain / compact / model / tool / hook / AwaitingUser 边界的 control barrier。旧 AskUser oneshot 通过既有 cancellation scope 退出，waiter bridge drain 不在本 PR。
- Main CLI stop effect 迁移至 `cancel_run_step`。
- Main L1-L4 测试、目标守卫、Target/Migration 文档与 Issue 验证证据。

### 明确不包含

- Sub Agent control registry、parent-child recursive terminate、absolute deadline 向 child 传播；承接至 Main/Sub Loop 统一后续工作。
- #1246 的 Main Tool suspension 生产切线及 `InteractionBridge::drain_run` waiter 生产接线。
- #1248 的 Sub interaction、Hook directive、reasoning 装配。
- #879 的旧 `cancel_run`、`Cancelling/Cancelled`、旧事件/projection 物理退役。
- #943/#944/#947 的 TUI 生命周期投影重构。

## 文件结构

- Modify: `packages/sdk/src/client.rs` — 发布 Main Run control 纯值命令。
- Modify: `packages/sdk/src/client_tests.rs`, `packages/sdk/tests/run_control_contract.rs` — L3 trait 与 wire 契约。
- Modify: `.agents/hooks/{check-agent-client-trait-minimal.sh,check-run-control-boundary.sh}` — 从 #878 迁移期禁入切换到目标 API/纯值约束。
- Modify: `docs/design/03-engineering/01-architecture-guards.md` — 同步守卫语义与 #1247 Main-only 范围。
- Modify: `agent/features/runtime/src/domain/agent_run.rs` — Main control delivery seam；不增加 Sub 专用模型。
- Modify/Create: `agent/features/runtime/src/application/{active_run.rs,active_run_tests.rs}` — Main typed registry 与外置 L1/L2 测试。
- Modify: `agent/features/runtime/src/application/loop_engine/{engine.rs,tests.rs}` — 保持 #1272 contract 的 typed barrier。
- Modify: `agent/features/runtime/src/application/main_loop/looping/{loop_runner.rs,main_run_port.rs,loop_runner_tests.rs}` — Main registration、Step scope、Context finalization 与 L4 场景。
- Modify: `agent/features/runtime/src/application/client/{accessors.rs,trait_impl.rs}` — SDK 命令委托 registry。
- Modify/Test: `apps/cli/src/tui/effect/session/{processing.rs,processing/handle.rs}` 及相邻 mock — stop effect 切换。
- Modify: `docs/design/02-modules/runtime/{01-domain-model.md,03-loop-and-state-machine.md,06-ports-and-adapters.md}` — Main production 入口与 Sub 承接边界。
- Modify: `docs/design/03-engineering/03-migration-governance.md` — R1/R8/R10 Current→Target 进度与 #879/后续统一 Loop 的责任。

## Task 1: 原子发布 Main control SDK API 并翻转守卫（L3/L0）

**Files:**
- Modify: `packages/sdk/src/client.rs`
- Modify: `packages/sdk/src/client_tests.rs`
- Modify: `packages/sdk/tests/run_control_contract.rs`
- Modify: `.agents/hooks/check-agent-client-trait-minimal.sh`
- Modify: `.agents/hooks/check-run-control-boundary.sh`
- Modify: `docs/design/03-engineering/01-architecture-guards.md`

- [ ] **Step 1: 写失败的 trait 与守卫契约测试**

在 `client_tests.rs` 增加一个仅通过 `dyn AgentClient` 调用的编译级契约：

```rust
let _ = client.cancel_run_step(&run_id, Some(&step_id), deadline);
let _ = client.terminate_run(&run_id, RunTerminationReason::UserExit, deadline);
```

在 `run_control_contract.rs` 保留 DTO wire round-trip，并为 `AgentClient` 公开方法编写最小 fake。新增/更新守卫测试，验证新 API 缺失、或 `AgentClient` 出现 `CancellationToken` / channel / lock 时均被拒绝。

- [ ] **Step 2: 运行失败测试确认红灯**

Run:

```bash
cargo test -p sdk --lib client::tests::agent_client_publishes_main_run_control_commands -- --exact
bash .agents/hooks/check-run-control-boundary.sh
```

Expected: trait 测试因缺少 API 失败；守卫仍报告迁移期禁入。

- [ ] **Step 3: 在同一原子修改中添加 API 与目标守卫**

在 `AgentClient` 增加同步纯值命令：

```rust
fn cancel_run_step(
    &self,
    run_id: &RunId,
    step_id: Option<&RunStepId>,
    deadline: ControlDeadline,
) -> CancelRunStepOutcome;

fn terminate_run(
    &self,
    run_id: &RunId,
    reason: RunTerminationReason,
    deadline: ControlDeadline,
) -> TerminateRunOutcome;
```

旧 `cancel_run` 保留为 #879 兼容入口。同步更新两个 guard：允许且要求这两个 API；仍禁止控制对象泄露并拒绝额外 AgentClient RPC。文档记录 #1247 是 Main atomic cutover leaf，不将新 API 提前发布为未接线接口。

- [ ] **Step 4: 运行 L0/L3 验证确认通过**

Run:

```bash
cargo test -p sdk --lib client::tests::agent_client_publishes_main_run_control_commands -- --exact
cargo test -p sdk --test run_control_contract -- --nocapture
bash .agents/hooks/check-agent-client-trait-minimal.sh
bash .agents/hooks/check-run-control-boundary.sh
```

Expected: PASS。

## Task 2: 建立 Main typed ActiveRun control registry（L1/L2）

**Files:**
- Modify: `agent/features/runtime/src/domain/agent_run.rs`
- Modify: `agent/features/runtime/src/application/active_run.rs`
- Create: `agent/features/runtime/src/application/active_run_tests.rs`
- Modify: `agent/features/runtime/src/application.rs`

- [ ] **Step 1: 外置现有 registry tests 并写失败控制测试**

将 `active_run.rs` 的内联 tests 迁入 `active_run_tests.rs`，在生产文件以 `#[cfg(test)] #[path = "active_run_tests.rs"] mod tests;` 声明。

新增确定性测试：

```rust
#[test]
fn cancel_step_only_cancels_current_step_scope() { /* root remains live */ }

#[test]
fn terminate_preempts_cancel_step_and_cancels_root_scope() { /* ... */ }

#[test]
fn repeated_main_control_commands_are_idempotent() { /* ... */ }

#[tokio::test]
async fn accepted_main_control_drains_interaction_waiter() { /* ... */ }
```

deadline 使用固定 `ControlDeadline::from_unix_millis`，不得 sleep。

- [ ] **Step 2: 运行失败测试确认红灯**

Run: `cargo test -p runtime application::active_run_tests -- --nocapture`

Expected: FAIL，当前 registry 仅存 bool 与单一 token。

- [ ] **Step 3: 实现 Main-only typed registry**

为 Main Run 注册 root token 和当前 Step child token。每条控制命令在同一 mutex 临界区内校验 Run/Step、决定 outcome、记录 `RunControl::{CancelStep, Terminate { reason, deadline }}`、取消对应 token。Terminate 优先于 cancel；control 接受后通过 `InteractionBridge::drain_run` 解析当前 Main waiter。不要注册/遍历 Sub descendant，不在此任务改变 Sub `ActiveRunRegistration`。

- Main `ActiveRunPort` 增加仅供 Loop 消费的 `take_control(run_id)`；默认实现只允许测试 fake，生产 Main port 必须委托 registry。
- 当前旧 AskUser oneshot 继续由传入的 cancellation scope 中断；不得将 `InteractionBridge` 注入 RuntimeHandle 或 Main loop，也不得在本 PR 调用 `InteractionBridge::drain_run`。

- [ ] **Step 4: 运行 registry 测试确认通过**

Run: `cargo test -p runtime application::active_run_tests -- --nocapture`

Expected: PASS。

## Task 3: 在 #1272 shared Loop 上实现 Main typed barrier（L2）

**Files:**
- Modify: `agent/features/runtime/src/application/loop_engine/engine.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/tests.rs`

- [ ] **Step 1: 扩展 ScriptedPort 并写失败测试**

保留 `DrainOutcome` / `DrainEpoch` fixture。给 ScriptedPort 提供 `RunControl` 队列和可控的 await 边界。新增场景：

```rust
#[tokio::test]
async fn cancel_step_after_model_finalizes_then_drains_follow_up() { /* Ready → cancel → Ready → seal */ }

#[tokio::test]
async fn cancel_step_with_empty_sealed_input_completes_without_cancelled_event() { /* ... */ }

#[tokio::test]
async fn terminate_during_compact_finishes_terminated_without_drain() { /* ... */ }

#[tokio::test]
async fn terminate_while_awaiting_user_finishes_terminated() { /* ... */ }
```

另覆盖 tool 与 stop-hook await。每个测试断言控制后不会启动新 model/tool/compact/hook，且新路径没有 `RunDomainEvent::Cancelled`。

- [ ] **Step 2: 运行失败测试确认红灯**

Run: `cargo test -p runtime application::loop_engine::tests -- --nocapture`

Expected: FAIL，当前 token cancel 一律走旧 `cancel_run`。

- [ ] **Step 3: 实现不破坏 DrainOutcome 的统一 barrier**

扩展 `RunLoopPort` 的 `take_control` 与 Main adapter。保持每个 await 都通过现有 `await_interruptible`，但在 await cancel/返回、以及任何新工作前调用统一 helper：

- `CancelStep`：请求 Step cancellation、发布事件、进入 `FinalizingStep`、调用 cancellation-shielded `finalize_cancelled_step`、完成 `StepCancelled → DrainingInput`，然后回到当前 epoch 的 drain 分支；
- `Terminate`：请求 termination、复用 finalizer 收口当前事实、flush Main Context、发布 `Terminated`，返回 Terminal；
- 旧 token cancellation 保留旧 `cancel_run` 兼容路径，直到 #879 删除；新 typed control 不得落入它；
- 既有 `Ready`、`InternalContinuation`、`NoInput`、`EmptyAndSealed` 的 epoch 增长和 seal 语义不得改变。

- [ ] **Step 4: 运行 Loop 测试确认通过**

Run: `cargo test -p runtime application::loop_engine::tests -- --nocapture`

Expected: PASS。

## Task 4: 接通 Main adapter、Context 收口与 AgentClient 委托（L2/L4）

**Files:**
- Modify: `agent/features/runtime/src/application/main_loop/looping/loop_runner.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/main_run_port.rs`
- Modify: `agent/features/runtime/src/application/main_loop/looping/loop_runner_tests.rs`
- Modify: `agent/features/runtime/src/application/client/{accessors.rs,trait_impl.rs}`

- [ ] **Step 1: 写 Main 生产路径失败场景**

用现有 deterministic provider / hook / tool fixture，分别在 model、compact、tool、stop hook、AwaitingUser 中发出新命令。断言：

- `cancel_run_step` 返回 Accepted 前当前 Step token 已取消；
- `terminate_run` 返回 Accepted 前 root token 已取消，waiter 已 drain；
- accepted input 仍经当前 `freeze_step → accept_step_input` 提交；
- cancellation outcome 仍经当前 `finalize_cancelled_step` / #1278 Context writer；
- cancel 后有 input 继续同一 Run 下一 Step，sealed 空输入才 Completed；terminate 不进入 drain。

- [ ] **Step 2: 运行失败测试确认红灯**

Run: `cargo test -p runtime application::chat::looping::loop_runner_tests -- --nocapture`

Expected: FAIL，Main 只注册单 token 且 AgentClient 无新委托。

- [ ] **Step 3: 接入 Main root/step scopes 与 Context flush**

在 `loop_runner` 创建 Main root scope 并注册；让 MainRunPort 在 Step 开始/结束更新 registry 的 current Step scope。为 terminate 实现 Main-only Context flush seam；只丢弃尚未 `accept_step_input` 的 buffer 内容。`MainRunPort::take_control` 委托 registry。

在 `AgentClientImpl` 直接委托 registry 新命令。不得经输入队列，且不得改 Sub runner。

- [ ] **Step 4: 运行 Main L4 测试确认通过**

Run: `cargo test -p runtime application::chat::looping::loop_runner_tests -- --nocapture`

Expected: PASS。

## Task 5: 切换当前 CLI stop effect（L3/L4）

**Files:**
- Modify: `apps/cli/src/tui/effect/session/{processing.rs,processing/handle.rs}`
- Modify: 受 trait 扩容影响的相邻 AgentClient mock

- [ ] **Step 1: 写失败的 effect 调用测试**

使用 recording AgentClient，断言现有 stop effect 调用 `cancel_run_step`，传入当前 Main `RunId`、当前 Step（若可得）和固定/构造 deadline；不再调用旧 `cancel_run`。

- [ ] **Step 2: 运行失败测试确认红灯**

Run: `cargo test -p cli tui::effect::session::processing -- --nocapture`

Expected: FAIL，当前调用旧 cancel。

- [ ] **Step 3: 进行最小 effect 切换**

只替换当前 stop effect 的 SDK 调用和所需 mock；不重构 TUI lifecycle/projection，不删除旧 Event 映射。

- [ ] **Step 4: 运行 effect 测试确认通过**

Run: `cargo test -p cli tui::effect::session::processing -- --nocapture`

Expected: PASS。

## Task 6: 回写 Main-only 文档、验证和 Issue 证据

**Files:**
- Modify: `docs/design/02-modules/runtime/{01-domain-model.md,03-loop-and-state-machine.md,06-ports-and-adapters.md}`
- Modify: `docs/design/03-engineering/03-migration-governance.md`

- [ ] **Step 1: 回写实际完成范围**

记录 Main typed registry、barrier、drain、waiter drain 和 Context finalization 已接生产。明确 Sub control / parent-child deadline 未完成并承接 Main/Sub Loop 统一工作；#879 仍负责物理退役。

- [ ] **Step 2: 执行完整门禁**

Run:

```bash
cargo fmt --all -- --check
cargo check -p runtime -p sdk
cargo clippy -p runtime -p sdk --all-targets -- -D warnings
cargo test -p sdk --test run_control_contract -- --nocapture
cargo test -p runtime application::active_run_tests -- --nocapture
cargo test -p runtime application::loop_engine::tests -- --nocapture
cargo test -p runtime application::chat::looping::loop_runner_tests -- --nocapture
cargo test -p cli tui::effect::session::processing -- --nocapture
bash .agents/hooks/check-architecture-guards.sh
```

Expected: 全部 PASS。

- [ ] **Step 3: Issue 回填与 PR 前检查**

Run:

```bash
git pull origin main
git diff origin/main...HEAD --check
git status --short
```

在 #1247 逐项回填 Main L1-L4、持久化、fmt/check/clippy/guard 的真实证据，并标注 Sub/parent-child deadline 承接至后续 Main/Sub Loop 统一工作。