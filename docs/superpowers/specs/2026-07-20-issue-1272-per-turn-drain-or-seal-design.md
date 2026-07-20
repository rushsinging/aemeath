# 设计：Runtime Loop finalized Step 的 per-turn drain-or-seal

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/1272
> Parent: #649　关联：#875、#876、#877、#1247、#1248、#879
> 日期: 2026-07-20
> 状态: 设计待 review

## 1. 背景与根问题

目标 Runtime 状态机已经定义 `DrainingInput`、`DrainDecision` 与 drain epoch/seal；领域 `Run` 也已有相应骨架。但生产 Loop 仍以旧 `Start → PreparingContext`、`finish → Completed` 和 `ToolsCompleted → PreparingContext` 驱动，导致设计语言与实际编排分离。

当前 Main 输入还同时分布在事件通道、`PendingInputBuffer` 与 `deferred_user_inputs`。busy `UserMessage` 在接收时被分类写入 deferred queue，当前 Run 结束后由 Session 外层逐条新建 Run；control event 则走另一 buffer。该分流既丢失跨类别到达顺序，也使 enqueue 与 `Completed` 之间不存在可证明的线性化点。`MainRunPort::drain_input` 还会把作为当前 batch 返回的事件再次交给 busy 路由，形成重复归属风险。

Sub 虽调用共享 Loop，却把初始 prompt 直接写入 messages，`drain_input` 恒为空、`freeze_step` 忽略输入。这是共享函数而非共享输入状态机。

## 2. 目标与范围

本设计让每个正常 finalized Step 固定经过同一 per-turn drain-or-seal 门禁：

```text
append/persist 或 InternalContinuation
  → DrainingInput
  → drain_or_seal(epoch)
  → Inputs | InternalContinuation | EmptyAndSealed
  → PreparingContext | Completed
```

范围：

- text-only EndTurn、Tool Step、Stop Hook Proceed/Block 的统一收口；
- Main busy 输入进入当前 Run-owned InputBuffer，并由下一次 drain 纳入同一 Run；
- Main/Sub 复用一个 Loop 和一个 drain decision，仅 InputBuffer adapter 不同；
- epoch/seal 原子契约，避免“判空后、Completed 前”遗失输入；
- 为 #1247 提供 CancelRunStep 收口后复用的同一原语。

不在本 Issue 实现：

- #1247 的 CancelRunStep/TerminateRun 生产控制、deadline 与父子传播；
- #1246 的 Main Tool suspension/UserQuestions；#1248 的 Sub interaction、Hook adapter、ReasoningPort 完整装配；
- #879 的旧 `cancel_run`、`Cancelled`、`MainRunPort`、legacy queue 等最终物理退役；
- TUI RunProjection/TEA 收口。

## 3. 核心不变量

1. `Completed` 只能由 `DrainDecision::EmptyAndSealed` 产生；普通 Step 完成、Context append 成功或 Hook Proceed 都不能直达 Completed。
2. InputBuffer 是一个 Run 未绑定输入的唯一 owner。一个事件只能处于：尚未 admission、buffer 内、已绑定本 Step、被明确撤回/拒绝、或 Terminate 后被丢弃之一。
3. `enqueue` 与 `drain_or_seal` 必须线性化：成功 enqueue 的输入要么在当前 drain batch 中，要么阻止本 epoch seal；绝不能同时不在 batch 且允许 Completed。
4. 一个 finalized Step 只产生一次 `ContextAppend`。Tool Step 的 assistant 与恢复为原 ToolCall 顺序的全部 result 一起 append。
5. Stop Hook Block 不提交被阻断 assistant；feedback 作为 typed `InternalContinuation` 经过同一 drain decision，不伪装为用户输入。
6. Main/Sub 的差异只在 adapter：Main 连接 live input admission；Sub 在构造时装入固定初始输入，耗尽后 seal。Loop 不按角色分支。
7. #1247 的取消 Step 收口必须进入本设计的 `DrainingInput → drain_or_seal`，不得建立 control 专用 drain 或第二 InputBuffer。

## 4. InputBuffer 契约

Runtime-owned `InputBuffer` 取代仅能返回 `Vec<LoopInput>` 的 pull 端口，发布一个最小原子协议：

```text
InputBuffer
  enqueue(input) -> EnqueueOutcome
  drain_or_seal(epoch) -> DrainOutcome
  discard_unbound() -> DiscardedInputStats

DrainOutcome
  Ready { epoch, batch }
  EmptyAndSealed { epoch }

DrainDecision
  Inputs
  InternalContinuation
  EmptyAndSealed
```

`InternalContinuation` 不是外部输入，因此由 Loop 的 finalization epilogue 与 `DrainDecision` 结合表达；它不要求写入 InputBuffer，但必须先将 Run 迁移到 `DrainingInput`。

adapter 内部可用 mutex、actor mailbox 或等价同步机制；公开契约不规定实现。必须满足：

- `drain_or_seal(epoch)` 原子取得当前 epoch 所有待处理输入；非空返回 `Ready`，不可同时 seal；
- 空队列时只有在同一线性化操作中 seal 成功才返回 `EmptyAndSealed`；
- seal 后的 admission 不能静默丢失：若 Run 仍可继续，enqueue 必须使后续 epoch 可见；若 Run 已终止，则返回明确拒绝；
- `WithdrawAll` 与普通 enqueue/drain 在同一 owner 内处理，不能跨 `deferred_user_inputs` 与 `PendingInputBuffer` 做非原子拼接；
- InputBuffer 不持久化。TerminateRun 可经 `discard_unbound` 丢弃未绑定内容，只记录非内容诊断。

Main adapter 以单一 Run-owned buffer 消费 TUI `ChatInputEvent` 和迁移期 legacy queue adapter。legacy queue 只能作为 admission source，不能再成为独立 drain owner。`deferred_user_inputs` 被删除。Sub adapter 在创建时写入固定 prompt，首轮 Ready 后自然空并 seal。

## 5. Loop 与 finalization epilogue

Loop 从 `Created` 必须立即 `start_draining` 并发布事件。每次进入 `DrainingInput`，只允许以下决策：

| DrainDecision | 状态迁移 | 作用 |
|---|---|---|
| `Inputs` | `DrainingInput → PreparingContext` | 绑定本 batch，冻结为下一 Step 的 pending messages |
| `InternalContinuation` | `DrainingInput → PreparingContext` | 使用 Runtime 内部 feedback/continuation 继续，不创建用户 echo |
| `EmptyAndSealed` | `DrainingInput → Completed` | 产生 terminal result 和 RunCompleted |

正常路径通过唯一 `finalize_then_drain` 编排，顺序固定：

1. 处理当前 response 与 Tool result，保持仅内存态；
2. 若是 Stop Hook Block，回滚被阻断 assistant，保留 feedback 为 InternalContinuation，不 append；
3. 若可提交，执行一次 cancellation-shielded `append_and_persist`；成功后标记 Step persisted，失败则 Failed；
4. 聚合迁移到 `DrainingInput` 并立即发布事件；
5. 执行 `drain_or_seal`；
6. 以唯一 `DrainDecision` 迁移到 PreparingContext 或 Completed。

映射如下：

| 出口 | 持久化 | drain decision |
|---|---|---|
| text-only + Stop Hook Proceed | assistant 恰好一次 append | Inputs 或 EmptyAndSealed |
| Tool Step | assistant + 原序 Tool results 恰好一次 append | Inputs 或 EmptyAndSealed |
| Stop Hook Block | 不 append 被阻断 assistant | InternalContinuation |
| #1247 CancelRunStep | deterministic partial/receipt append | Inputs 或 EmptyAndSealed |

`run.finish()`、`ToolsCompleted → PreparingContext`、`ContinueAfterResponse → PreparingContext` 不能继续作为正常 finalization 的旁路；仅保留在与本设计不冲突的领域或迁移兼容语义中，并由 #879 清理最终死路径。

## 6. Main/Sub 与 Session 边界

Session 外层只在 idle 时取得启动一个新 Run 的初始输入。Run 一旦启动，busy 模型、Tool、Hook、compact 等边界到达的 Main 输入都 admission 到该 Run 的 buffer，由其下一次 per-turn drain 消费。

因此：

- follow-up 不创建第二 Run；同一 RunId 下创建新 RunStep；
- 多条 busy 输入保持单一 admission 顺序，并作为同一下一 batch 绑定；
- Sub 的初始 prompt 通过 FixedInputBuffer 首轮 drain 进入 StepMessage ownership，不再直接预置为 message；
- Sub 初始 batch 耗尽后返回 EmptyAndSealed；父 Run 消费 typed terminal result，不从 EventSink 或消息列表推断结果。

## 7. 迁移边界

本 Issue 必须提供 `InputBuffer` 的唯一 drain-or-seal 原语和正常 Step 的生产接线，但不与 #1247 重叠控制触发。#1247 只负责在 CancelRunStep finalizer 完成后调用本原语。#879 负责删除旧 cancel、Cancelled、MainRunPort 和迁移期 queue/compatibility 符号；本 Issue 不以双写、镜像或第二队列维持兼容。

Target 文档同步规则：状态矩阵、Stop Hook 边界、伪代码和停止条件一律以“Completed 仅由 EmptyAndSealed 产生”为准。Migration Governance 记录生产旧路径、#1272 接线范围、#1247 复用点和 #879 退役责任。

## 8. 验证策略

- **L1 状态机**：Created 进入 DrainingInput；normal finalized Step 只能进 DrainingInput；三种 DrainDecision 的可达/非法迁移；Completed 无其他入口。
- **L2 InputBuffer**：以 barrier/可控 fake 驱动 enqueue 与 seal 的交错，证明输入进入 batch 或阻止 seal；覆盖空、批量、WithdrawAll、Terminate discard，禁止 sleep。
- **L2 Loop**：text、Tool、Stop Hook Proceed/Block 的调用顺序；append 恰好一次；同一 RunId 下 follow-up 新 Step；compact/model/Tool/Hook 边界输入均在下一 drain 被采用。
- **L3 adapter contract**：Main live adapter 保序 admission；Sub fixed adapter 首轮返回 prompt、后续 EmptyAndSealed；两者复用同一 InputBuffer contract suite。
- **L4 场景**：busy follow-up 不新建 Run；多输入同 batch；Tool 期间输入不丢；Block assistant 不落 Session；Sub 固定输入自然收敛；#1247 相邻 seam 复用同一 drain 原语。
- **L0**：`cargo check -p runtime -p sdk`、`cargo clippy -p runtime -p sdk --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`bash .agents/hooks/check-architecture-guards.sh`。

## 9. 风险与非目标

- 原子语义依赖 adapter 的实际线性化实现；测试不得以定时或重跑代替竞态证明。
- InputBuffer 改动会牵涉 InputId、queued echo、WithdrawAll 与 idle command，必须保留事件顺序和 UI 回显的 adapter contract。
- 不在本 Issue 增加多 Run 持久化、恢复 active Run、远端输入协议或新的用户交互能力。
- 不为了迁移同时保留 `deferred_user_inputs` 与新 buffer 双写；双 owner 会重新引入本设计要消除的判空竞态。
