# #1272 Per-turn Drain-or-Seal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development (recommended) or executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 将共享 Loop 的所有正常 finalized Step 接入原子 per-turn drain-or-seal，使 busy 输入在同一 Run 的下一 Step 被消费，且 `Completed` 仅由空 epoch 成功 seal 产生。

**Architecture:** #1277 已提供 `freeze_step → accept_step_input → Context/model` handoff；本计划只在该 seam 前后接入 Run-owned InputBuffer 与 `DrainDecision`。#1278 独占 finalized outcome DTO、receipt、fingerprint、schema 与 writer；Loop 仅调用稳定 ContextPort append。Main live 输入与 Sub fixed 初始输入实现同一 InputBuffer contract，normal Step 和 #1247 后续 control Step 共用 drain-or-seal。

**Tech Stack:** Rust、Tokio、async_trait、Run aggregate、ContextPort、Cargo tests。

---

## 文件结构

- Modify: `agent/features/runtime/src/domain/agent_run/{state.rs,domain.rs}` — 正常 Step 的 DrainingInput 转换与 Completed 唯一门禁。
- Test: `agent/features/runtime/src/domain/agent_run/tests.rs` — L1 状态转换与非法旁路。
- Modify: `agent/features/runtime/src/ports/input_buffer.rs` — Runtime-owned drain-or-seal PL。
- Create: `agent/features/runtime/src/ports/input_buffer_tests.rs` — L2 原子 epoch/seal contract。
- Modify: `agent/features/runtime/src/application/loop_engine/{engine.rs,tests.rs}` — shared Loop 的唯一 drain epilogue。
- Modify: `agent/features/runtime/src/application/chat/looping/{input_gate.rs,main_run_port.rs,loop_runner.rs}` — Main live adapter、busy admission、Stop continuation 与 session boundary。
- Modify: `agent/features/runtime/src/application/agent/runner/{loop_run.rs,setup.rs}` — Sub fixed input adapter。
- Modify/Test: existing looping/runner test files — L3 adapter/L4 journey 证据。
- Modify: `docs/design/02-modules/runtime/03-loop-and-state-machine.md` — Target 状态机与 Stop Hook 语义。
- Modify: `docs/design/03-engineering/03-migration-governance.md` — #1272 current→target 与 #1278 ownership。

## Task 1: 锁定 Run 正常路径只能经 DrainingInput 完成

**Files:**
- Modify: `agent/features/runtime/src/domain/agent_run/state.rs`
- Modify: `agent/features/runtime/src/domain/agent_run/domain.rs`
- Test: `agent/features/runtime/src/domain/agent_run/tests.rs`

- [x] **Step 1: 写 normal finalized Step 的失败状态机测试**

新增测试覆盖：

```rust
#[test]
fn normal_finalized_step_enters_draining_before_completion() {
    // Created → DrainingInput → Inputs → PreparingContext → InvokingModel
    // → ApplyingResponse → Finishing → FinalizingStep
    // normal Step persisted 后必须为 DrainingInput。
    // 仅 apply_drain_decision(EmptyAndSealed) 可变为 Completed。
}
```

另写非法转换断言：`Finishing → Completed`、`ExecutingTools → PreparingContext` 与 `ApplyingResponse → PreparingContext` 的普通 response 路径必须失败。

- [x] **Step 2: 运行测试确认失败**

Run: `cargo test -p runtime domain::agent_run::tests::normal_finalized_step_enters_draining_before_completion -- --exact`

Expected: FAIL，当前 normal path 仍允许 `Finish` 或 `ToolsCompleted` 绕过 drain。

- [x] **Step 3: 以正常 finalization 领域转换替代旧旁路**

增加最小的 normal-finalization transition/API，使已完成 Step 只能：

```text
Finishing / ExecutingTools
  → FinalizingStep
  → DrainingInput
```

保留 #1247 的取消专用 transition；不在此任务实现 control command。`Run::complete` 只能从 `DrainingInput + EmptyAndSealed` 对应的 transition 产生终态事件。

- [x] **Step 4: 运行领域测试确认通过**

Run: `cargo test -p runtime domain::agent_run::tests -- --nocapture`

Expected: PASS。

- [x] **Step 5: 提交**

```bash
git add agent/features/runtime/src/domain/agent_run
git commit -m "refactor(runtime): #1272 收口 normal Step drain 状态"
```

## Task 2: 建立 epoch/seal InputBuffer 契约与确定性并发测试

**Files:**
- Modify: `agent/features/runtime/src/ports/input_buffer.rs`
- Create: `agent/features/runtime/src/ports/input_buffer_tests.rs`
- Modify: `agent/features/runtime/src/ports.rs`

- [x] **Step 1: 写 InputBuffer contract 失败测试**

创建 deterministic fake，提供 barrier 在“观测空”与“尝试 seal”之间挂起。测试：

```rust
#[tokio::test]
async fn enqueue_during_empty_drain_prevents_seal() {
    let buffer = BarrierInputBuffer::empty_and_paused_before_seal();
    let drain = buffer.begin_drain_or_seal(DrainEpoch::new(0));
    buffer.enqueue(LoopInput::from("follow-up"));
    buffer.release_seal();
    assert_eq!(drain.await, DrainOutcome::Ready {
        epoch: DrainEpoch::new(0),
        batch: vec![LoopInput::from("follow-up")],
    });
}

#[tokio::test]
async fn ready_batch_preserves_fifo_and_advances_epoch() {
    let buffer = TestInputBuffer::from_texts(["first", "second"]);
    assert_eq!(
        buffer.drain_or_seal(DrainEpoch::new(3)).await,
        DrainOutcome::Ready {
            epoch: DrainEpoch::new(3),
            batch: vec![LoopInput::from("first"), LoopInput::from("second")],
        }
    );
    assert_eq!(
        buffer.drain_or_seal(DrainEpoch::new(4)).await,
        DrainOutcome::EmptyAndSealed { epoch: DrainEpoch::new(4) },
    );
}

#[tokio::test]
async fn empty_and_sealed_has_no_admitted_input_outside_result() {
    let buffer = TestInputBuffer::default();
    assert_eq!(
        buffer.drain_or_seal(DrainEpoch::new(0)).await,
        DrainOutcome::EmptyAndSealed { epoch: DrainEpoch::new(0) },
    );
    assert!(buffer.try_enqueue(LoopInput::from("late")).is_err());
}
```

测试不得使用 `sleep`；必须直接控制 barrier 和 admission 顺序。

- [x] **Step 2: 运行测试确认失败**

Run: `cargo test -p runtime ports::input_buffer_tests -- --nocapture`

Expected: FAIL，现有 `InputBuffer` 仅有同步 `drain()`。

- [x] **Step 3: 定义最小 Published Language**

在 `ports/input_buffer.rs` 定义：

```rust
pub enum DrainOutcome {
    Ready { epoch: DrainEpoch, batch: Vec<LoopInput> },
    EmptyAndSealed { epoch: DrainEpoch },
}

#[async_trait]
pub trait InputBuffer: Send + Sync {
    async fn drain_or_seal(&self, epoch: DrainEpoch) -> DrainOutcome;
    fn discard_unbound(&self) -> DiscardedInputStats;
}
```

实现单一测试/生产 backing，使 enqueue 与 drain-or-seal 在同一线性化点处理。不要保留可独立消费的 `deferred_user_inputs`。

- [x] **Step 4: 运行 contract 测试确认通过**

Run: `cargo test -p runtime ports::input_buffer_tests -- --nocapture`

Expected: PASS。

- [x] **Step 5: 提交**

```bash
git add agent/features/runtime/src/ports/input_buffer.rs agent/features/runtime/src/ports/input_buffer_tests.rs agent/features/runtime/src/ports.rs
git commit -m "feat(runtime): #1272 建立 InputBuffer drain-or-seal 契约"
```

## Task 3: 让 shared Loop 以 drain decision 驱动 Start、Step 与终态

**Files:**
- Modify: `agent/features/runtime/src/application/loop_engine/engine.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/tests.rs`

- [x] **Step 1: 扩展 ScriptedPort 并写失败测试**

将 test port 改为返回 scripted `DrainOutcome`。新增：

```rust
#[tokio::test]
async fn text_step_finalizes_then_drains_follow_up_in_same_run() {
    let mut port = ScriptedPort::with_drains([
        DrainOutcome::Ready { epoch: DrainEpoch::new(0), batch: vec![LoopInput::from("first")] },
        DrainOutcome::Ready { epoch: DrainEpoch::new(1), batch: vec![LoopInput::from("follow-up")] },
        DrainOutcome::EmptyAndSealed { epoch: DrainEpoch::new(2) },
    ]);
    port.model_steps = VecDeque::from([
        ModelStep::Complete { text: "one".into() },
        ModelStep::Complete { text: "two".into() },
    ]);
    let mut run = new_run(Duration::ZERO);
    run_loop(&mut run, &CancellationToken::new(), &mut port).await.unwrap();
    assert_eq!(run.steps().len(), 2);
    assert_eq!(run.status(), RunStatus::Completed);
    assert_eq!(port.finalized_steps, port.frozen_steps);
}

#[tokio::test]
async fn tool_step_finalizes_then_empty_seal_completes() {
    let mut port = ScriptedPort::with_drains([
        DrainOutcome::Ready { epoch: DrainEpoch::new(0), batch: vec![LoopInput::from("tool request")] },
        DrainOutcome::EmptyAndSealed { epoch: DrainEpoch::new(1) },
    ]);
    port.model_steps = VecDeque::from([ModelStep::Tools {
        text: "calling".into(),
        calls: vec![call("Read", json!({"file_path": "a.rs"}))],
    }]);
    port.tool_steps = VecDeque::from([ToolStep::Continue]);
    let mut run = new_run(Duration::ZERO);
    run_loop(&mut run, &CancellationToken::new(), &mut port).await.unwrap();
    assert_eq!(run.status(), RunStatus::Completed);
    assert_eq!(port.finalized_steps.len(), 1);
}

#[tokio::test]
async fn empty_seal_is_the_only_terminal_completion_path() {
    let mut port = ScriptedPort::with_drains([
        DrainOutcome::Ready { epoch: DrainEpoch::new(0), batch: vec![LoopInput::from("first")] },
        DrainOutcome::EmptyAndSealed { epoch: DrainEpoch::new(1) },
    ]);
    port.model_steps = VecDeque::from([ModelStep::Complete { text: "done".into() }]);
    let mut run = new_run(Duration::ZERO);
    run_loop(&mut run, &CancellationToken::new(), &mut port).await.unwrap();
    assert_eq!(port.calls, vec![
        "emit", "drain_or_seal", "freeze_step", "accept_step_input",
        "needs_compaction", "emit", "model", "finalize_step", "start_draining",
        "drain_or_seal", "emit",
    ]);
}
```

断言每个 Step 有不同 `RunStepId`、相同 `RunId`，并验证调用序列含 `finalize_step → start_draining → drain_or_seal`。

- [x] **Step 2: 运行失败测试确认红灯**

Run: `cargo test -p runtime application::loop_engine::tests::text_step_finalizes_then_drains_follow_up_in_same_run -- --exact`

Expected: FAIL，当前 Loop 仍从 `Start` 直接进入 context，text-only 直接 complete。

- [x] **Step 3: 重构 Loop 的唯一门禁 helper**

新增局部 helper，例如 `drain_and_apply_decision`，职责：

1. 确保 Run 在 `DrainingInput`；
2. 调用 `InputBuffer::drain_or_seal(epoch)`；
3. `Ready` 时 apply `DrainDecision::Inputs`、freeze/bind input、等待已存在的 `accept_step_input` 成功；
4. `EmptyAndSealed` 时 apply `DrainDecision::EmptyAndSealed` 并发布 terminal event；
5. 所有正常 finalized 出口均回到该 helper。

`accept_step_input` 保持在 bind 后、Context/model 前。#1278 的 outcome append 继续只经现有 `finalize_step` / `finalize_cancelled_step` Port 调用，不修改 Context DTO。

- [x] **Step 4: 运行 Loop 单元测试确认通过**

Run: `cargo test -p runtime application::loop_engine::tests -- --nocapture`

Expected: PASS。

- [x] **Step 5: 提交**

```bash
git add agent/features/runtime/src/application/loop_engine
git commit -m "refactor(runtime): #1272 以 drain decision 驱动共享 Loop"
```

## Task 4: 将 Stop Hook continuation 收口到下一 Step drain batch

**Files:**
- Modify: `agent/features/runtime/src/application/loop_engine/engine.rs`
- Modify: `agent/features/runtime/src/application/chat/looping/main_run_port.rs`
- Test: `agent/features/runtime/src/application/loop_engine/tests.rs`
- Test: `agent/features/runtime/src/application/chat/looping/loop_runner_tests.rs`

- [x] **Step 1: 写 Stop Hook Block 的失败测试**

断言：

- Block 后当前 assistant/outcome 的 `finalize_step` 恰好一次；
- feedback 不回滚当前 outcome，也不作为 accepted user input；
- Loop 进入 `DrainingInput`；
- 下一 Step Context 输入以 feedback 前缀加 FIFO user follow-up；
- 无 follow-up 时 feedback 仍触发同 Run 下一 Step；
- 达到上限时当前 Step 提交后 `Failed`。

- [x] **Step 2: 运行失败测试确认红灯**

Run: `cargo test -p runtime test_continue_false_json_treated_as_block -- --exact`

Expected: FAIL 或断言显示当前 Loop 直接 `ContinueAfterResponse`，未走 finalized-then-drain。

- [x] **Step 3: 实现 internal continuation batch**

让 `ModelStep::StopHookBlocked` 在当前 Step finalized outcome append 后产生 Runtime-owned continuation。该 continuation 只在下一次 drain decision 中注入 system-generated feedback；普通用户输入仍由 InputBuffer FIFO 获取。不要把 feedback 写入 #1277 accepted-input projection，不要自行序列化 #1278 outcome。

- [x] **Step 4: 运行 Stop Hook 场景确认通过**

Run: `cargo test -p runtime test_continue_false_json_treated_as_block test_stall_triggers_stop_hook_check -- --nocapture`

Expected: PASS。

- [x] **Step 5: 提交**

```bash
git add agent/features/runtime/src/application/loop_engine agent/features/runtime/src/application/chat/looping
git commit -m "fix(runtime): #1272 经 drain 续跑 Stop Hook continuation"
```

## Task 5: 用单一 Main InputBuffer 替代 busy deferred queue

**Files:**
- Modify: `agent/features/runtime/src/application/chat/looping/{input_gate.rs,main_run_port.rs,loop_runner.rs}`
- Test: `agent/features/runtime/src/application/chat/looping/loop_runner_tests.rs`
- Test: `agent/features/runtime/src/application/chat/looping/input_gate_tests.rs`

- [x] **Step 1: 写 busy follow-up 失败场景**

使用 channel-backed input：第一模型调用阻塞期间依次发送两条 user message 与一个 control event。断言：

- 两条 user message 按 FIFO 进入同一 Run 的下一 Step；
- RunStarted 只有一个 RunId；
- control event 不改变 user 输入顺序；
- `deferred_user_inputs` 不再产生下一 Run；
- `WithdrawAll` 只影响尚未绑定的 user input。

- [x] **Step 2: 运行失败测试确认红灯**

Run: `cargo test -p runtime test_loop_persists_across_turns_until_shutdown -- --exact`

Expected: FAIL 或断言显示 busy message 被 Session 外层拆成新 Run。

- [x] **Step 3: 统一 Main admission**

- 删除 `deferred_user_inputs` 字段和 Session idle loop 的逐条 pop；
- `queue_busy_event` 仅向当前 Run-owned backing admission，不按事件类型分散到第二 queue；
- `drain_input` 不得同时返回 input 又重入 busy queue；
- legacy `QueueDrainPort` 只能作为同一 buffer 的 admission source；
- 保留现有 idle command routing，但它不成为 Run 输入的第二 owner。

- [x] **Step 4: 运行 Main adapter 与场景测试确认通过**

Run: `cargo test -p runtime application::chat::looping::loop_runner_tests application::chat::looping::input_gate_tests -- --nocapture`

Expected: PASS。

- [x] **Step 5: 提交**

```bash
git add agent/features/runtime/src/application/chat/looping
git commit -m "refactor(runtime): #1272 统一 Main busy 输入 admission"
```

## Task 6: 以 FixedInputBuffer 统一 Sub 首轮输入与收敛

**Files:**
- Modify: `agent/features/runtime/src/application/agent/runner/{setup.rs,loop_run.rs}`
- Test: `agent/features/runtime/src/application/agent/runner/tests.rs`
- Test: `agent/features/runtime/src/application/loop_engine/tests.rs`

- [x] **Step 1: 写 Sub fixed input 的失败测试**

断言：Sub 初始 prompt 由 FixedInputBuffer 首轮 `Ready` 返回；首轮 accepted-input handoff 后才 build window；下一 drain `EmptyAndSealed`；Sub 不直接以预置 `messages` 或恒空 `drain_input` 绕开 shared Loop。

- [x] **Step 2: 运行失败测试确认红灯**

Run: `cargo test -p runtime application::agent::runner::tests::sub_initial_prompt_drains_once_then_seals -- --exact`

Expected: FAIL，当前 Sub `drain_input()` 恒空，prompt 直接预置到 messages。

- [x] **Step 3: 实现 FixedInputBuffer adapter**

在 setup 将 prompt 放入 FixedInputBuffer；`SubAgentRun::freeze_step` 消费 Loop batch 并构造 accepted input / ContextRequest。删除 `committed_message_count` 对“首条 prompt 是 messages[0]”的归属推断；保留 #1277 accepted-input seam 和 #1278 outcome-only append 边界。

- [x] **Step 4: 运行 Sub 测试确认通过**

Run: `cargo test -p runtime application::agent::runner::tests application::loop_engine::tests -- --nocapture`

Expected: PASS。

- [x] **Step 5: 提交**

```bash
git add agent/features/runtime/src/application/agent/runner
git commit -m "refactor(runtime): #1272 统一 Sub fixed input drain"
```

## Task 7: 回写文档、守卫与全量验证

**Files:**
- Modify: `docs/superpowers/specs/2026-07-20-issue-1272-per-turn-drain-or-seal-design.md`
- Modify: `docs/design/02-modules/runtime/03-loop-and-state-machine.md`
- Modify: `docs/design/03-engineering/03-migration-governance.md`
- Test: architecture guards and workspace tests

- [x] **Step 1: 更新 Target 与 Migration Governance**

确认文档同时表达：

- #1277 accepted input handoff 已完成且 native dependency 保留；
- #1272 的 `Completed` 唯一由 `EmptyAndSealed` 产生；
- Stop Block 提交当前 outcome 后，通过下一 Step continuation/drain 继续；
- #1278 独占 finalized outcome schema/writer；
- #1247 只复用 drain-or-seal，#879 只做物理退役。

- [x] **Step 2: 逐项核对 #1272 checklist**

读取 Issue #1272，逐项回填 text/tool/hook/Main/Sub、enqueue-vs-seal、#1247 seam 与 L0-L4 证据。未完成项必须附 owner、影响和后续 Issue。

- [x] **Step 3: 运行完整验证**

Run:

```bash
cargo fmt --all -- --check
cargo check -p runtime -p sdk
cargo clippy -p runtime -p sdk --all-targets -- -D warnings
cargo test -p runtime --lib
cargo test -p context --lib --tests
bash .agents/hooks/check-architecture-guards.sh
```

Expected: all exit 0. 首次失败必须保留为证据；定向重跑只用于 flaky 分类。

- [x] **Step 4: 检查退役项并提交**

检查 `deferred_user_inputs`、旧 `drain_input`、Sub prompt 旁路、普通 `Finish`/`ToolsCompleted` 旁路是否仍有生产引用。仍需 #879 的物理残留必须记录原因和 owner。

```bash
git add agent/features/runtime docs
git commit -m "docs(runtime): #1272 回写 drain-or-seal 验收证据"
```
