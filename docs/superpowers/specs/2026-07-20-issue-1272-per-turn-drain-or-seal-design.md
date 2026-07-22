# 设计：Runtime Loop finalized Step 的 per-turn drain-or-seal

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/1272
> Parent: #649　前置：#1277（已完成）　关联：#1278、#1247、#1248、#1280、#879
> 日期: 2026-07-21　更新: 2026-07-22（反映已实现事实）
> 状态: 已实现

## 1. 背景与根问题

领域 `Run` 已具备 `DrainingInput`、`DrainDecision`、`StartDraining` 与 `DrainEmptyAndSealed` 骨架；#1277 已把 `freeze_step → append_accepted_input → build_window` 接入共享 Loop，使已绑定 user input 在模型执行前 durable。生产 Loop 仍使用旧路径：`Start → PreparingContext`，text-only finalized 后直达 `Completed`，Tool finalized 后直接回 PreparingContext，busy Main 输入写入 `deferred_user_inputs` 并在 Session 外层拆成新 Run。

这造成三个问题：

1. 正常 Step 没有统一的 finalized 后门禁，`Completed` 不是由原子空队列 seal 唯一产生；
2. busy 输入被分流到 `PendingInputBuffer` 与 `deferred_user_inputs`，失去统一顺序和原子判空边界；
3. Sub 虽调用共享 Loop，但初始 prompt 旁路 InputBuffer，`drain_input` 恒空，不能证明 Main/Sub 使用同一输入状态机。

## 2. 已完成前置与边界

#1277 已完成并保留为本 Issue 的 native blocked-by：共享 Loop 在 `freeze_step` 后调用 `accept_step_input`，Main/Sub 仅将已接受的 user facts 经 ContextPort durable；失败时不开始 Context 或模型调用。

#1278 与本 Issue可并行，但拥有 finalized outcome 的唯一 Context 所有权：

- `FinalizedOutcomeProjection`、`FinalizeCause`、receipt、usage、fingerprint、revision；
- ContextPort 的 finalized-outcome DTO、canonical/in-memory writer、envelope、legacy upgrade；
- ContextWindow/compact 对 structured outcome 的读取。

#1272 只调用稳定的 Runtime → Context finalized-outcome Port，**NEVER** 定义第二份 serialization、fingerprint、receipt 或 repository writer。

## 3. 目标状态机（已实现）

```text
Created
  → DrainingInput
  → drain_or_seal(epoch)
      ├─ Ready(batch)
      │     → bind/freeze → accepted-input append → PreparingContext
      ├─ InternalContinuation(kind, batch)
      │     → StopHookFeedback / ToolResults → PreparingContext
      ├─ NoInput
      │     → AwaitUser（buffer 不 seal、epoch 不推进）
      └─ EmptyAndSealed → Completed

正常 finalized Step
  → ContinueAfterResponse / ToolsCompleted → DrainingInput
  → 同一 drain_or_seal 门禁

AwaitingUser 恢复（同一 Run）：
  → Ready → UserResumed → PreparingContext
```

`DrainDecision` 是 Loop 对 `DrainOutcome` 的唯一归约：

| 决策 | 状态迁移 | 含义 |
|---|---|---|
| `Inputs` | `DrainingInput → PreparingContext` | batch 已绑定 RunStep，accepted input 已 durable |
| `InternalContinuation` | `DrainingInput → PreparingContext` | 引擎驱动的 continuation（StopHookFeedback / ToolResults），可携同期用户输入 |
| `EmptyAndSealed` | `DrainingInput → Completed` | 当前 epoch 原子 seal 成功，无遗漏 admission；`Completed` 唯一入口 |

`Completed` 只能由 `EmptyAndSealed` 产生。普通 append、Tool 收口、Stop Hook Continue 均不能直接完成 Run。

### 3.1 DrainOutcome 全量（已实现）

```rust
enum DrainOutcome {
    Ready { batch: Vec<LoopInput>, epoch: DrainEpoch },
    InternalContinuation { kind: InternalContinuationKind, batch: Vec<LoopInput>, epoch: DrainEpoch },
    EmptyAndSealed { epoch: DrainEpoch },
    NoInput { epoch: DrainEpoch },
}

enum InternalContinuationKind {
    StopHookFeedback { feedback: String },
    ToolResults,
}
```

- `Ready`：batch 非空（构造 `ready()` 强制 assert）。用于 Main 用户输入和 Sub 固定 prompt。
- `InternalContinuation`：引擎驱动 continuation。batch 可为空（纯 continuation），也可携带同期排队的用户追问。epoch 始终推进。
- `EmptyAndSealed`：唯一终端门禁。buffer seal 后 `push_or_reject` 拒绝新 `UserMessage`。
- `NoInput`：仅在 `AwaitingUser` 时经 `await_user_input` → `try_drain_unsealed` 产生。buffer 不 seal、epoch 不推进，caller 返回 `LoopDirective::AwaitUser` 并以相同 expected epoch 重入。

### 3.2 DrainEpoch（已实现）

```rust
struct DrainEpoch(u64);
```

单调递增，Run-owned（`next_drain_epoch` 字段），跨 `run_loop` 调用持久。每次成功 drain 后递增。AwaitUser → re-enter 不重置 epoch，保证同一 Run 内 per-turn 线性化不被恢复打断。

## 4. InputBuffer 原语（已实现）

Main adapter 的 `RunInputBuffer` 是 Run-owned 的原子协议：

```rust
impl RunInputBuffer {
    /// 原子 drain-or-seal：用户输入非空 → Ready；空 → EmptyAndSealed（seal）。
    fn drain_or_seal(&mut self, expected: DrainEpoch) -> BufferDrain;

    /// 内部 continuation（StopHookFeedback / ToolResults）：drain 但不 seal。
    fn take_internal_continuation(&mut self, expected: DrainEpoch) -> BufferDrain;

    /// AwaitingUser 用 drain：从不 seal。空 → Empty（epoch 不推进）；
    /// 有输入 → Ready（epoch 推进）。
    fn try_drain_unsealed(&mut self, expected: DrainEpoch) -> BufferDrain;

    /// 密封后 push 拒绝 UserMessage（non-UserMessage control 仍接受）。
    fn push_or_reject(&mut self, event: ChatInputEvent) -> Option<ChatInputEvent>;

    /// 撤回所有未绑定 UserMessage。
    fn withdraw_all_user_texts(&mut self) -> Vec<String>;

    /// Run 结束时排空所有剩余事件（回 Session）。
    fn drain_all(&mut self) -> Vec<ChatInputEvent>;
}
```

不变量证明：

- `drain_or_seal` 在同一个同步调用中完成 drain + seal 判空，不依赖外部"先 drain 再判空"；
- `Ready` batch 非空（构造期 assert），`EmptyAndSealed` 只在 batch 为空时返回且同步 seal；
- `try_drain_unsealed` 的 `Empty` 不推进 epoch、不 seal——caller 以相同 expected epoch 重试；
- `take_internal_continuation` 总是推进 epoch 但不 seal——仅 `drain_or_seal` 的 `EmptyAndSealed` 持有 seal 权；
- seal 后 `push_or_reject` 拒绝 `UserMessage`，control 事件仍可进入以便 Run 结束时 drain 回 Session；
- `deferred_user_inputs` 不得与新 buffer 双写或作为独立消费 owner。

Main adapter 连接 live `ChatInputEvent` 和迁移期 queue admission；Sub adapter 是构造时写入固定 prompt 的 FixedInputBuffer（见 §6）。两者复用同一 `RunLoopPort::drain_input` contract。

## 5. 正常完成路径（已实现）

Loop 将 text、Tool、Stop Hook 的正常收口统一为 DrainingInput 回流：

1. 当前 Step 完成（`CompleteStep`）；
2. Run 状态迁移：`ApplyingResponse → ContinueAfterResponse → DrainingInput`（text/StopHookBlocked）或 `ExecutingTools → ToolsCompleted → DrainingInput`（Tool）；
3. 进入 loop 顶部的 `drain_or_seal`；
4. Complete 无追问 → `EmptyAndSealed` → `Completed`；有追问 → `Ready` → 下一 Step。

Stop Hook 的已实现口径：

- `Continue`：当前 Step 提交 outcome 后进入 `ContinueAfterResponse → DrainingInput`；
- `Block`：**不回滚当前 assistant 或 Tool outcome**；先提交当前 Step，feedback 经 `InternalContinuation(StopHookFeedback)` 成为下一 Step 的系统前缀，并与同次 drain 的 FIFO 用户输入组装；
- Block 达上限：当前 Step 仍先提交，再 `Failed(StopHookRetryExhausted)`；
- Cancel/Terminate 优先于尚未绑定的 Stop continuation。

## 6. Main/Sub 与 Session 边界（已实现）

Session 外层只在 idle 时收集启动一个 Run 的初始输入。Run 启动后，模型、Tool、Hook、compact 边界收到的 Main 输入均进入当前 Run-owned `RunInputBuffer`；下一次 per-turn drain 在**同一 RunId** 下绑定新的 RunStep。

### Sub FixedInputBuffer（已实现）

Sub 的 `drain_input` 维护 `prompt_drained` 标志和独立 `next_epoch`：

- 首轮（epoch 0）：`Ready(prompt) → freeze_step → accepted-input append`；
- 次轮及以后（epoch 1+）：`EmptyAndSealed`。

Loop 不按角色分支——Sub 的 `EmptyAndSealed` 经历与 Main 完全相同的 `run_loop` → `apply_drain_decision(EmptyAndSealed)` → `Completed` 路径。

### AwaitUser 恢复（同一 Run）（已实现）

当 `run_loop` 检测到 `AwaitingUser` 状态时，调用 `port.await_user_input(epoch)` 而非 `port.drain_input(epoch)`：

- `await_user_input` 内部使用 `try_drain_unsealed`：从不 seal、空时不推进 epoch；
- 返回 `NoInput` → `LoopDirective::AwaitUser`，caller 等待用户输入后以相同 expected epoch 重入 `run_loop`；
- 用户输入到达后返回 `Ready` → `UserResumed` → `PreparingContext`，同一 Run 继续执行。

## 7. 实施范围与退役边界

本 Issue 已实现：

- `RunLoopPort` 的 drain-or-seal seam（`drain_input`、`await_user_input`）与 shared engine 状态流；
- `Run` 正常路径的 `StartDraining` / `DrainDecision` 应用、`DrainEpoch` 生命周期；
- Main `RunInputBuffer`（`drain_or_seal`、`take_internal_continuation`、`try_drain_unsealed`、`push_or_reject`、`withdraw_all_user_texts`、`drain_all`）；
- Sub 固定 prompt epoch（`Ready` at epoch 0 → `EmptyAndSealed` at epoch 1）；
- Loop engine 中 `DrainOutcome::InternalContinuation(StopHookFeedback/ToolResults)`、`DrainOutcome::NoInput` 与 `LoopDirective::AwaitUser`；
- 正常完成（Complete/Continue/StopHookBlocked/ToolsCompleted）→ `ContinueAfterResponse` / `ToolsCompleted` → `DrainingInput` → `drain_or_seal`；
- #1277 accepted input（`freeze_step → append_accepted_input → build_window`）；
- #1272 L1-L4 测试、Target 文档与 Migration Governance。

**Out of scope（不变）：**

- #1278 finalized outcome schema/writer —— 所有权不变，仍由 #1278 承接；
- #1247 `PendingInteraction` 生产接线 —— `PendingInteraction` / `InteractionContinuation` 数据结构已存在（#1245），但从 Run aggregate 到生产 Main/Sub adapter 的 bridge 尚未接线，仍由 #1247 承接；
- #1248 Sub interaction/Hook/Reasoning 行为；
- #1280 唯一 launcher；
- #879 对旧 `MainRunPort`、`SubAgentRun`、`queue`/compatibility、`deferred_user_inputs` 的最终物理删除。

## 8. 验证矩阵

- **L1**：`Created → DrainingInput → DrainDecision(Inputs/InternalContinuation/EmptyAndSealed)` 状态转换；`Completed` 仅由 `DrainEmptyAndSealed` 产生（无旁路）。✓
- **L2**：`drain_or_seal` 原子性（enqueue-vs-seal race、empty seal、AlreadySealed）、`try_drain_unsealed` 不 seal/不推进 epoch、`take_internal_continuation` 推进 epoch 不 seal、`push_or_reject` seal 后拒绝、`withdraw_all_user_texts`、epoch mismatch 检测。✓
- **L3**：Main `RunInputBuffer`、Sub FixedInputBuffer 复用同一 `RunLoopPort::drain_input` / `await_user_input` contract。✓
- **L4**：text Complete→DrainingInput→EmptyAndSealed→Completed；Tool→ToolsCompleted→DrainingInput；StopHookBlocked→InternalContinuation；busy follow-up→Ready 同 RunId 新 Step；Sub fixed prompt epoch 0 Ready → epoch 1 EmptyAndSealed；AwaitUser→NoInput→re-enter→Ready→UserResumed。✓
- **L0**：`cargo check -p runtime -p sdk`、`cargo clippy -p runtime -p sdk --all-targets -- -D warnings`、`cargo fmt --all -- --check`、完整 architecture guards。