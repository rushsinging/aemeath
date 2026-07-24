# Agent Runtime · 状态机与 Loop Engine

> 层级：02-modules / runtime（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#761（S2）
> 本文定义 Run 单一状态机、统一 Loop Engine 骨架，以及"Main 常驻多轮 vs Sub 单次"的输入模型统一。实现差距与退役责任只在 [迁移治理](../../03-engineering/03-migration-governance.md) 维护。

## 1. Run 状态机（唯一，内存态）

```
DrainingInput
  ├── Ready ──▶ PreparingContext ──▶ InvokingModel ──▶ ApplyingResponse
  │              ▲                                     │
  │              │ needs_compact            ┌──────────┴──────────┐
  │              ▼                        有 ToolCall           无 ToolCall
  │          Compacting                     │                    │
  │              │                          ▼                    ▼
  │              └─────────────▶ AwaitingToolApproval    ContinueAfterResponse
  │                                         │                    │
  │                                         ▼                    ▼
  │                                  ExecutingTools          DrainingInput
  │                                     │       │
  │                              结果回收完   AwaitUser
  │                                     │       │
  │                                     ▼       ▼
  │                              ToolsCompleted  AwaitingUser
  │                                     │       │
  │                                     ▼       │ (reply→UserResumed)
  │                               DrainingInput ◀┘
  │
  ├── InternalContinuation(kind) ──▶ PreparingContext
  ├── NoInput ──▶ AwaitUser（epoch 不推进、buffer 不 seal）
  └── EmptyAndSealed ──▶ Completed

AwaitingUser 收到匹配 reply 后按 continuation 回到
AwaitingToolApproval / ExecutingTools / PreparingContext 之一。
但恢复后 Run 状态迁移是 UserResumed → DrainingInput → drain_or_seal → 下一 Step。

Step 打断旁路：任意 active Step 态 ── CancelRunStep ──▶ CancellingStep
               ── 确定性收口 + 持久化 ──▶ DrainingInput
Run 终止旁路：任意非终态 ── TerminateRun ──▶ Terminating
               ── 同一 StepFinalizer + Session flush ──▶ Terminated
失败终态：Failed
```

迁移期仍保留旧 `cancel_run → Cancelling → Cancelled` 兼容路径；#1272 不改变该控制协议。正常 finalized Step 与 CancelRunStep 收口后均进入 `DrainingInput`，经 `drain_or_seal` 决定：有输入（`Ready`/`InternalContinuation`）继续下一 Step，无输入且 epoch 原子 seal（`EmptyAndSealed`）正常 `Completed`，无输入且不 seal（`NoInput`）返回 `AwaitUser`。`TerminateRun` 才终止整个 Run，并在退出前完成同等质量的 Step 收口和 Session flush。#1247 负责新 Run root / per-Step 控制协议接线，#879 才退役旧 `Cancelled` 状态与兼容投影。

`DrainEpoch` 是 Run-owned 单调递增计数器，跨 `run_loop` 调用持久。每次成功 drain 后递增；AwaitUser → re-enter 不重置 epoch。engine 和 buffer 双向校验 epoch 匹配，不匹配返回错误。

### 状态转换矩阵

| 源状态 | 事件/条件 | 目标状态 |
|---|---|---|
| Created | Start | DrainingInput |
| DrainingInput | `drain_or_seal` → `Ready`（非空 batch） | PreparingContext |
| DrainingInput | `drain_or_seal` → `InternalContinuation`（StopHookFeedback 或 ToolResults，可携同期用户输入） | PreparingContext |
| DrainingInput | `drain_or_seal` → `EmptyAndSealed`（空队列且 epoch 原子 seal） | Completed(`InputDrained` 或 `StepCancelledAndInputDrained`) |
| DrainingInput | `await_user_input` → `NoInput`（AwaitingUser 时空队列，不 seal、epoch 不推进） | AwaitUser 返回（同一 expected epoch 重入） |
| PreparingContext | needs_compaction | Compacting |
| Compacting | 回收完成 | PreparingContext |
| PreparingContext | 上下文就绪 | InvokingModel |
| InvokingModel | LLM 响应 | ApplyingResponse |
| InvokingModel | Retryable 错误(超时/5xx/429) | InvokingModel（退避重试 ≤10 次，见 §5）|
| InvokingModel | context 超限 | Compacting（compact 后重跑，非重试）|
| InvokingModel | Fatal 错误(4xx) / 重试耗尽 | Failed |
| ApplyingResponse | 有 tool_calls | AwaitingToolApproval |
| ApplyingResponse | 需要 plan approval | AwaitingUser（`ContinuePlanApproval`） |
| ApplyingResponse | 无 tool_calls / EndTurn | ContinueAfterResponse → DrainingInput |
| ApplyingResponse | Stop Hook Block（text/Continue 场景进入 `ModelStep::StopHookBlocked`） | ContinueAfterResponse → DrainingInput（feedback 经 `InternalContinuation(StopHookFeedback)` 进入下一 Step） |
| AwaitingToolApproval | 全部放行 | ExecutingTools |
| AwaitingToolApproval | 需人工确认(approval) | AwaitingUser（`ContinueToolApproval`） |
| ExecutingTools | Tool 返回 `Suspended(UserInteraction)` | AwaitingUser（`CompleteToolCall`） |
| ExecutingTools | StuckGuard `HardPause` | Main：AwaitingUser（`ContinueAfterHardPause`）；Sub / unavailable：Failed |
| ExecutingTools | 结果回收完 | ToolsCompleted → DrainingInput |
| AwaitingUser | 匹配 reply | 经 `complete_interaction` 恢复到原工作状态（ExecutingTools / AwaitingToolApproval / PreparingContext），正常收口进入 `DrainingInput`，再经 `drain_or_seal` 获得下一 batch → PreparingContext |
| DrainingInput | `AwaitingUser` 恢复后 drain 到 `NoInput`（buffer 空、不 seal） | AwaitUser 返回，caller 等待用户输入后以同一 expected epoch 重入 |
| AwaitingUser | completion=`Cancelled` + Tool continuation | ToolCall 得到 typed Cancelled，回原 Tool 状态继续 |
| AwaitingUser | completion=`Cancelled` + Plan/HardPause continuation | Failed（typed PlanApprovalCancelled / HardPauseCancelled） |
| 任意 active Step 态 | `CancelRunStep` 获胜 | CancellingStep |
| CancellingStep | StepFinalizer 完成或 10s deadline 到达 | FinalizingStep（持久化 deterministic receipts / partial step） |
| FinalizingStep | cancel 原因的 Step 已持久化（`StepCancelled → DrainingInput`） | DrainingInput |
| 任意非终态（除 Terminating） | `TerminateRun` 获胜 | Terminating |
| Terminating | 同一 StepFinalizer 完成或 5s deadline 到达，Session flush 完成 | Terminated |
| 任意非终态（除 AwaitingUser、Terminating） | timeout>0 且超时 | Failed |

> **AwaitingUser timeout 豁免**：`AwaitingUser` 状态 **MUST NOT** 计入 RunSpec.timeout 的墙钟计时。用户交互等待时间不可预测，timeout 在进入 `AwaitingUser` 时暂停、离开时恢复。`AwaitingToolApproval` 在全部自动放行时是**瞬时态**（不停留），仅在需人工确认时才进入 `AwaitingUser(ContinueToolApproval)`；因此自动放行路径不受 timeout 影响。

**控制优先级**：一旦接受 `CancelRunStep`，当前 Step 进入 `CancellingStep`；该 Step 后续普通完成、timeout 或错误只作为收口诊断，NEVER 把它伪装为普通 Completed。Step 收口并持久化后 Run 必须进入 `DrainingInput`。一旦接受 `TerminateRun`，Run 进入 `Terminating`；后续 Step 完成仅作为终止收口事实，Run 最终只能进入 `Terminated`。重复控制命令必须幂等。

**AwaitingUser 关键语义**：这是 **Run 内交互暂停**（Run 未完成，等特定 request id 的答复），必须同时保存 typed continuation，内存存活、不落盘；崩溃则整个 Run 从头开始（见 `05-recovery-semantics`）。reply / interaction cancellation 只能恢复或终结该 continuation，**NEVER** 统一跳到 `PreparingContext`。四类 completion 的穷尽映射见 [端口与适配器](06-ports-and-adapters.md) §2。这**区别于**"Run 完成后 Session 等下一条全新输入"（那是 Run 序列层，见 §3）。

## 2. Loop Engine 骨架（Main/Sub 共用，零分支）

### 2.0 Adapter 内部策略注入（#1382）

`run_loop` 只依赖 `RunLoopPort`，**NEVER** 按 Main/Sub 分支。两个 adapter 内部进一步用四个静态分发策略面隔离真实差异：

| 策略 | Main | Sub |
|---|---|---|
| `InputStrategy` | `MainInputStrategy`：RunInputBuffer、输入 channel、StopHook/ToolResults continuation | `SubInputStrategy`：固定 prompt、epoch、ToolResults continuation |
| `EventStrategy` | `MainEventStrategy`：投影 RuntimeStreamEvent、完成收口 | `SubEventStrategy`：提取 typed `AgentRunTerminal`、报告 progress |
| `LlmStrategy` | 动态 reasoning、visible delta、retry event | 固定 reasoning、non-visible delta、retry log/cancellation propagation |
| `ToolStrategy` | Main tool result continuation | Sub tool result continuation |

策略使用具体类型和泛型静态分发，**NEVER** 用 trait object 组成超集 struct。共享纯流程位于 `loop_engine::{shared,llm_log,llm_strategy,tool_strategy}`；Main 独有的输入交错、reflection、Stop Hook、InteractionBridge、任务快照和 Sub 独有的 max-output-tokens 恢复仍留在各自 adapter。`RunLoopPort` impl 仅保留 engine contract 与薄委托，业务差异不进入 engine。

### 2.1 Step/Run 控制与持久化原则

Loop 在**每个** `.await` 返回后 **MUST** 检查当前 Step scope 与 Run root scope：

1. `CancelRunStep` 只取消当前 Step scope；`TerminateRun` 取消 Run root scope及其所有 child scope；
2. 任一控制请求获胜后立即禁止该 scope 启动新的 Model Invocation、Tool、Compact 或 Hook，并同步返回 `Accepted`；
3. Provider / Tool / Agent 子 Run 在异步边界协作退出，Loop 进入唯一 `StepFinalizer`；
4. `StepFinalizer` 收集完成/partial/unconfirmed receipts，补齐未完成 ToolCall 的 typed terminal result，保持原始 ToolCall 顺序，并进行 cancellation-shielded persist；
5. CancelRunStep 与 TerminateRun **MUST** 使用完全相同的 deterministic Tool/Agent summary schema 和价值门禁，**NEVER** 为摘要调用 LLM；
6. CancelRunStep 的总收口 hard deadline 为 10s；到期仍未停止的工作标为 `CancellationUnconfirmed` 后持久化并进入 Drain；
7. TerminateRun 的总收口 hard deadline 为 5s；到期采用相同 `CancellationUnconfirmed` 收口并继续 Session flush，暂不定义 Force Terminate；
8. 已成功提交并标记 persisted 的 Step **NEVER** 回滚；未完成 Step 尽可能落盘已确认事实、partial 输出、Tool/Agent receipts 和可能副作用；
9. 所有非控制出口也经过 epilogue 校验，Run 最终只允许 `Completed / Failed / Terminated`。

> 控制请求同步生效，收口异步完成。"马上"表示当前 scope 立即停止调度和唤醒在途 future，不表示跳过 Tool/Agent 收口、Step 持久化或 Session flush。

### 2.1 Session 回放边界与 InputBuffer

Session 是可回放数据的唯一真相源；"可回放"只承诺已经提交到 Session 的内容，**NEVER** 承诺重建 Runtime 内存态。

- Provider partial、Tool/Agent progress 或结果只有在成为 Session committed content 后，才属于 resume/replay 边界；TUI 的临时流式 projection 本身不是 durable source。
- CancelRunStep 收口时，当前 Step 的已确认事实、partial assistant、Tool/Agent deterministic receipts 通过 StepFinalizer 写入 Session；下一 Step 从 Session committed content 与新 drain batch 构建 Context。
- InputBuffer 中的内容尚未进入 Session，因此 TerminateRun **MAY** 直接丢弃，不持久化、不恢复、不计入 Session 回放；只允许记录不含内容的 count/bytes 诊断。
- 已经绑定当前 Step 并由 `append_accepted_input` 成功写入 Session 的 user input 不再属于 InputBuffer，必须随 Session 回放；该 handoff 发生在 `freeze_step` 后、首次 `build_window` / compact / model 前。handoff 只传 accepted user facts，**NEVER** 混入 system-generated Stop feedback、assistant、Tool result、RunStatus 或进行态。Context 在自己的 mutation gate 内取得提交 revision；后续 outcome 以 window 的 `backing_revision` CAS 补充同一 Step，**NEVER** 重复 user input。
- TerminateRun 完成前必须 flush Session 已有 committed content；不要求把未提交 buffer 内容提升为 Session 事实。

### 2.2 Stop Hook 持久化与 continuation 边界

Stop Hook 只裁决 Run 能否终止，**NEVER** 否决已完成 assistant / Tool 产出的历史事实。最终 assistant Step 的 `append_and_persist`、Stop 判定与 continuation **MUST** 按以下顺序执行：

1. `apply_response` + `apply_results` 形成不可变当前 Step 事实；
2. **Stop Hook dispatch**：
   - `Continue`：进入 `FinalizingStep`，以 `FinalizeCause::Completed` cancellation-shielded 提交当前 Step，随后 Run Completed；
   - `Block` 且未超过上限：当前 Step 同样进入 `FinalizingStep` 并提交；Runtime 将 Hook 的结构化 reason 转为 system-generated feedback，随后重新进入 `DrainingInput`；
   - `Block` 且超过上限：当前 Step 仍先经 `FinalizingStep` 提交，再进入 `Failed(StopHookRetryExhausted)`；
3. Block 后的下一次 `drain_or_seal` 必须构造一个稳定 batch：**已提交的 assistant / Tool 历史**在 Context backing 中；新 batch 以 Stop feedback 为系统前缀，再追加该次 drain 收到的普通用户追问（FIFO）。三者在同一次下一 Step Context Window 中可见；
4. Stop feedback 仅是 Runtime 生成的系统输入，不是 Hook BC 对 Session 的直接写入；Hook BC 只返回结构化 directive / reason；
5. `CancelRunStep` 或 `TerminateRun` 一旦获胜，优先于尚未绑定的 Stop continuation：不得发起下一次模型调用。CancelRunStep 由 StepFinalizer 收口当前事实后进入 Drain；TerminateRun seal admission、丢弃未绑定 InputBuffer 并 flush 已提交 Session；
6. commit handoff 后 owned commit 必须跑到明确成功或失败，caller cancellation **NEVER** 中断 durable commit；commit 成功后立即 `mark_step_persisted`，该 Step 从此不属于 partial、取消时 **NEVER** 回滚。

> Session committed content 是唯一历史真相。Stop Block 的语义是“继续同一个 Run”，不是撤销 assistant 已产生的内容；feedback 和用户追问只决定该 Run 的**下一 Step**。

### 2.3 HardPause Continuation

从 `ExecutingTools` 因 StuckGuard HardPause 进入 `AwaitingUser(ContinueAfterHardPause)` 时，continuation **MUST** 记录当前 step 和 tool phase：

- 若恢复（HardPauseContinue）：回到 `ExecutingTools` 继续未完成的 Tool 调用，**NEVER** 直接跳到 `PreparingContext`；
- 若取消：为当前 step 的全部未完成 ToolCall 生成 typed Cancelled results，按原顺序提交完整 step（保持 assistant/tool-result 邻接协议），**THEN** 进入 Failed。

### 2.4 领域事件发布不变量

`Run` 是生命周期事件的唯一生产者。**每一次** Run 聚合状态 mutation 返回后，调用方都必须在执行下一条业务语句或 `.await` 前立即执行 `run.drain_events()` 并把结果交给 `EventSink`；禁止只在 response 或 loop 末尾批量 drain。该规则覆盖 `RunStarted`、`RunAwaitingUser`、`RunResumed`、`RunCancellationRequested`、step/tool 状态以及全部 terminal 事件。

伪代码用 `mutate_and_publish(run, &ctx.events, |run| ...)` 表示这个原子编排约定：closure 内只做一次聚合 mutation，返回后 helper 立即 drain + emit。interaction coordinator 也必须逐次使用同一 helper，先发布 `RunAwaitingUser` 再 `.await` completion，恢复 continuation 后先发布 `RunResumed` 再继续。epilogue 只执行 `assert_terminal` / `assert_no_pending_events`；**NEVER** 在末尾补造终态或延迟发布事件。

### 2.5 骨架伪代码

```rust
async fn run_loop(
    run: &mut Run,
    cancel: &CancellationToken,
    port: &mut impl RunLoopPort,
) -> Result<LoopDirective, LoopEngineError> {
    if run.status() == RunStatus::Created {
        run.start_draining()?;
        emit_events(run, port).await?;
    }

    // #1272: engine-owned epoch。从 Run 持久 epoch 初始化，AwaitUser→re-enter 不重置。
    let mut expected_epoch = DrainEpoch(run.next_drain_epoch());
    let mut terminal_text: Option<String> = None;

    loop {
        if handle_interrupt(run, cancel, port).await? {
            return Ok(LoopDirective::Terminal);
        }
        if run.status().is_terminal() {
            return Ok(LoopDirective::Terminal);
        }

        // ---- drain 阶段 ----
        let awaiting_user = run.status() == RunStatus::AwaitingUser;
        let drain_future = if awaiting_user {
            port.await_user_input(expected_epoch)   // #1272: 从不 seal，空时 epoch 不推进
        } else {
            port.drain_input(expected_epoch)
        };
        let outcome = await_interruptible(run, cancel, drain_future).await?;

        // epoch 双向校验
        if outcome.epoch() != expected_epoch {
            return Err(LoopEngineError::Adapter("epoch mismatch"));
        }

        match outcome {
            DrainOutcome::Ready { batch, .. } => {
                run.advance_drain_epoch();
                expected_epoch = expected_epoch.next();
                if run.status() == RunStatus::AwaitingUser {
                    run.transition(RunTransition::UserResumed)?;  // #1272: 同一 Run 恢复
                }
                run.apply_drain_decision(DrainDecision::Inputs, None)?;
                execute_step(run, cancel, port, &mut guard, &batch, &mut terminal_text).await?;
            }
            DrainOutcome::InternalContinuation { kind: _, batch, .. } => {
                run.advance_drain_epoch();
                expected_epoch = expected_epoch.next();
                if run.status() == RunStatus::AwaitingUser && batch.is_empty() {
                    return Ok(LoopDirective::AwaitUser);  // 纯 continuation 不自动 resume
                }
                if run.status() == RunStatus::AwaitingUser {
                    run.transition(RunTransition::UserResumed)?;
                }
                run.apply_drain_decision(DrainDecision::InternalContinuation, None)?;
                execute_step(run, cancel, port, &mut guard, &batch, &mut terminal_text).await?;
            }
            DrainOutcome::NoInput { .. } => {
                // #1272: buffer 不 seal、epoch 不推进。caller 等待用户输入后
                // 以相同 expected epoch 重入 run_loop。
                return Ok(LoopDirective::AwaitUser);
            }
            DrainOutcome::EmptyAndSealed { .. } => {
                if run.status() == RunStatus::AwaitingUser {
                    return Ok(LoopDirective::AwaitUser);  // seal 前仍在等用户
                }
                run.advance_drain_epoch();
                expected_epoch = expected_epoch.next();
                if !port.claim_terminal(run.id()) {
                    cancel_run(run, port).await?;
                    return Ok(LoopDirective::Terminal);
                }
                let text = terminal_text.as_deref();
                run.apply_drain_decision(DrainDecision::EmptyAndSealed, text)?;
                emit_events(run, port).await?;
                return Ok(LoopDirective::Terminal);
            }
        }
    }

    // ... TerminateRun 收口 ...
}
```

`execute_step` 驱动一线 Step：`freeze_step` → `accept_step_input` → compact check → `invoke_model` → `record_model_invocation` → `apply_drain_decision` → `complete_step` → `finalize_step`。每一步后检查 control/cancel/timeout。

**#1272 关键点：**
- 正常完成（Complete/Continue/StopHookBlocked/ToolsCompleted）均进入 `ContinueAfterResponse` / `ToolsCompleted` → `DrainingInput`，不加中间状态；
- `Completed` 只能由 `EmptyAndSealed` 产生；
- AwaitUser 恢复：`NoInput` → `LoopDirective::AwaitUser`，caller 等待用户输入后以同一 epoch 重入，`Ready` 时执行 `UserResumed`；
- Sub 固定 prompt：epoch 0 `Ready(prompt)` → epoch 1 `EmptyAndSealed`，与 Main 走完全相同的 Loop 路径。

### 2.6 控制协议：请求同步，完成异步

Runtime 入站命令区分两个 scope，均不经过 InputBuffer：

1. `cancel_run_step(run_id, step_id?)`：同步原子迁移当前 Step 到 `CancellingStep`、触发 Step scope、返回 typed outcome；异步 StepFinalizer 最长 10s，完成后 Run 固定进入 `DrainingInput`。
2. `terminate_run(run_id, reason)`：同步迁移 Run 到 `Terminating`、seal input admission、触发 Run root scope、返回 typed outcome；异步复用同一 StepFinalizer（最长 5s）、丢弃未进入 Session 的 InputBuffer、flush Session，最终进入 `Terminated`。
3. CancelRunStep 后 Drain 有输入则 `PreparingContext` 开下一 Step；无输入且 drain epoch 原子 seal 则 `Completed(reason=StepCancelledAndInputDrained)`。
4. TerminateRun 不回到 Drain；resume 只回放 Session committed content，并创建新 Run。
5. 当前不定义 Force Terminate。

Step scope 是 Run root scope 的 child；CancelRunStep 不污染下一 Step token，TerminateRun 传播到全部 Step/Tool/SubRun scope。

### 2.7 Agent Tool / Sub Run 控制传播

Main 当前 Step 接受 `CancelRunStep` 后，对普通 Tool 取消该 Tool operation；对 Agent Tool **MUST** 向关联 child Run 发送 `TerminateRun(ParentStepCancelled)`，**NEVER** 向 child 发送 CancelRunStep 后让它回到 Drain 继续执行。child 再对其嵌套 Agent Tool 递归传播 TerminateRun。

所有层级共享父控制请求创建的**绝对 deadline**：

```text
main_cancel_deadline = accepted_at + 10s
child terminate       = main_cancel_deadline - now
nested child          = same absolute deadline - now
```

NEVER 为每层重新发放 5s/10s，否则嵌套深度会线性放大总收口时间。直接 TerminateRun 同理使用 `accepted_at + 5s` 的绝对 deadline。

StepFinalizer 读取 child `RunSpec.finalization`：

- Main 默认 `SummaryMode::Deterministic + ReceiptDetail::Full`；摘要用于同 Run 下一 Step 的 Context 投影。
- Sub 默认 `SummaryMode::None + ReceiptDetail::Safety`；不生成自身 Context summary，但仍必须返回 terminal receipt，至少包含 child/run/tool identity、artifact refs、可能副作用、未完成 ToolCall 与 `CancellationUnconfirmed`。
- 特殊需要自身 continuation 的 Sub 可显式声明 Deterministic + Full；父 Run 只能收缩预算，不能把 Safety receipt 降为空。
- 父 Agent Tool 用 Sub terminal receipt 形成协议完整的 typed Tool result；**NEVER** 为此额外调用 LLM，也不保存/注入 Sub 的完整消息链。


## 3. 输入模型统一：单 Run vs Session 多 Run 序列

关键区分——Loop Engine 只管**单个 Run** 的生命周期；"Main 常驻多轮对话"是**外层 Run 序列**：

| | 谁管 | 循环 |
|---|---|---|
| **单个 Run** | `loop_engine::run_loop` | Run 内 Run Step 循环；函数只在 `Completed / Failed / Terminated` 后返回 typed terminal result |
| **Main 常驻多轮** | `agent_run` 会话循环 | `等用户输入 → start_run → Run 完成 → 等下一输入 → 新 Run`（一个 Session 内 Run 序列）|
| **Sub 单次** | 父 Run 的 tool_coordination | 派生一个子 Run，跑完回传父，无后续 |

**统一点**：Sub = 单次输入的一个 Run；Main = Session 层多个 Run 的序列，每个 Run 就是"单次输入"的特例。**Loop Engine 不感知这个区别**——它只跑一个 Run。

- `AwaitingUser`（ask_user 暂停）：同一个 Run 内暂停/resume，Run 未完成
- `Completed` 后等下一输入：Run 完成，Session 层开新 Run（不是同一 Run）

### InputBuffer（入站端口）— 支撑追问

Loop Engine 每轮在门禁点调用 `port.drain_input(epoch)` 或 `port.await_user_input(epoch)`。Main/Sub 靠不同 `RunLoopPort` 实现区分，引擎零分支。

**#1272 RunInputBuffer（Main adapter）** 是 Run-owned 的原子 drain-or-seal 原语：

| 方法 | 行为 | seal? | epoch 推进? |
|---|---|---|---|
| `drain_or_seal(epoch)` | 用户输入非空 → `Ready`；空 → `EmptyAndSealed`（seal） | Yes（空时） | 总是 |
| `take_internal_continuation(epoch)` | engine-driven continuation（StopHookFeedback/ToolResults） | Never | 总是 |
| `try_drain_unsealed(epoch)` | AwaitingUser 用：非空 → `Ready`；空 → `Empty`（不 seal） | Never | 仅 Ready 时 |
| `push_or_reject(event)` | 密封后拒绝 `UserMessage`，control 仍接受 | — | — |
| `withdraw_all_user_texts()` | 撤回所有未绑定用户消息 | — | — |
| `drain_all()` | Run 结束时排空所有事件 | — | — |

| | InputBuffer 装配 | 行为 |
|---|---|---|
| Main | `RunInputBuffer`（Run-owned，live `ChatInputEvent` + 迁移期 queue） | 用户在 Run 执行中**追问** → `push_or_reject` → 下一轮 drain → append 进 Context Window 带上；`drain_or_seal` 原子判空。**AwaitingUser 追问经 `try_drain_unsealed` 在同一 Run 内绑定，不创建新 Run** |
| Sub | FixedInputBuffer（`prompt_drained` 标志 + 独立 epoch） | 首轮 drain（epoch 0）：`Ready(prompt)` → `freeze_step` → `accepted_input`；次轮及以后（epoch 1+）：`EmptyAndSealed`。与 Main 复用完全相同的 Loop 状态机 |

- `input` 是 **RuntimeContext 的入站端口**（与出站端口同层，装配时确定）
- `result` 是 `run_loop` 的 `LoopDirective`（`Terminal` / `AwaitUser`），供 call site 据此决定下一步；`EventSink` 只承载权威领域事实的外向投影。Main 把 terminal event 投影给 TUI；Sub 同样可投影诊断，但父 Run **NEVER** 订阅或反向消费 EventSink 来取得业务结果，也 **NEVER** 靠遍历 message 推断结果
- `DrainEpoch`：Run-owned 单调递增计数器。跨 `run_loop` 调用持久（AwaitUser→re-enter 不重置）。engine 和 buffer 双向校验 epoch 匹配，不匹配返回错误

## 4. 停止条件与 AwaitUser drain 语义

### 停止条件

| 条件 | 结果 |
|---|---|
| 无 tool_calls / stop_reason=EndTurn，ContinueAfterResponse → DrainingInput → EmptyAndSealed | Completed |
| Stop Hook Block（累计≤15） | 当前 Step 提交 → InternalContinuation(StopHookFeedback) + 同次 drain 用户追问 → PreparingContext，同一 Run 继续 |
| Stop Hook Block 累计>15 | 当前 Step 提交 → Failed(StopHookRetryExhausted) |
| timeout>0 且墙钟超时 | Failed |
| StuckGuard HardPause | AwaitingUser（Main）/ Failed（Sub，无人应答）|
| CancelRunStep 且 Drain 无新输入 | StepFinalizer → DrainingInput → EmptyAndSealed → Completed(`StepCancelledAndInputDrained`) |
| CancelRunStep 且 Drain 有输入 | StepFinalizer → DrainingInput → Ready → PreparingContext，继续下一 Step |
| TerminateRun | 同一 StepFinalizer（≤5s）+ 丢弃未入 Session 的 InputBuffer + Session flush → Terminated |
| LLM Fatal 错误 / 重试耗尽 | Failed（Retryable 先退避重试；context 超限→compact 重跑）|

### AwaitUser drain 语义（#1272）

`AwaitingUser` 是 Run 内暂停（非终态）。Loop engine 在检测到 `AwaitingUser` 时调用 `port.await_user_input(epoch)` 而非 `port.drain_input(epoch)`：

- `await_user_input` 内部使用 `try_drain_unsealed`：**从不 seal buffer**；
- 无用户输入时返回 `NoInput { epoch }`，epoch **不推进**，caller 返回 `LoopDirective::AwaitUser`；
- 外部等待用户输入后，以**相同 expected epoch** 重入 `run_loop`；
- 用户输入到达后返回 `Ready`，engine 执行 `UserResumed` 恢复到 `PreparingContext`；
- 这与"Run 完成后 Session 等下一条全新输入"（Run 序列层，见 §3）是不同语义。

> **去掉 max_turns**：不再用轮次上限，改由 `timeout`（0=无限，Main 默认 0）+ StuckGuard 双重兜底（见 `04-stuck-prevention`）。

## 5. 重试策略（LLM 错误）

`model_invocation` 对 Retryable 错误退避重试，Fatal 直接失败。**只做退避重试，不做降级 / 故障转移**（避免改变结果质量、引入 pool 依赖）。

| 层级 | 触发 | 应对 |
|---|---|---|
| **T0 即时** | 流开始前中断 / 连接瞬断，且本 attempt 无可见 delta 已提交 | 首次立即重试（瞬时抖动）|
| **T1 退避** | 超时 / 5xx / 429，且本 attempt 无可见 delta 已提交 | 指数退避 + jitter，**单次退避封顶 60 秒**；429 尊重 `Retry-After`，但合并后的最终 delay 仍受 60 秒上限约束 |
| **失败** | 已执行第 **11 个 attempt** 或 Fatal(4xx) | `RunFailed{ error }` |

- **上限**：首次调用后最多重试 **10 次**，共最多 **11 attempts**；单次退避封顶 **60s（1 分钟）**
- **Fatal(4xx) 不重试**，直接 RunFailed
- **context 超限**单独触发 compact 重跑（不计入重试次数）
- **可见输出门禁**：attempt 已向 EventSink 提交 delta 且无法原子回滚时，不得自动重试；保留部分输出并按失败策略终结
- 可配（config/RunSpec）：`max_retries`(默认 10)、退避基数、退避上限
- 可观测：`ModelInvocationRetrying{ attempt }`

## 6. Stop Hook 两层重试

- Hook BC 对单条 Stop command 的执行故障最多尝试 3 次；主动 Block 不重试。
- 三次执行都失败时，Hook 返回 `Block(StopHookExecutionFailed)`。
- Runtime 对同一个 Run 维护 `stop_block_count`，主动 Block 与执行失败 Block 都计数。
- `stop_block_count≤15` 时，将反馈作为 system-generated input 加入下一步并回 PreparingContext。
- 第 16 次阻断进入状态 `Failed(StopHookRetryExhausted)`，并发布 `RunFailed { error: StopHookRetryExhausted }`；不得强制 Completed。
- 两个上限分别归 Hook 和 Runtime，静态默认值均由 ConfigSnapshot 提供。

详见 [../hook/01-run-loop-integration.md](../hook/01-run-loop-integration.md)。

## 7. 相关文档

- 领域模型：[01-domain-model.md](01-domain-model.md)
- 模块边界：[02-module-boundaries.md](02-module-boundaries.md)
- 防 stuck：[04-stuck-prevention.md](04-stuck-prevention.md)
- 恢复语义：[05-recovery-semantics.md](05-recovery-semantics.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：Run 单状态机 + 迁移表、Loop Engine 零分支骨架、单 Run vs Session 多 Run 序列、停止条件 | #761 |
| 2026-07-11 | 补 InputBuffer 入站端口（Loop 门禁 drain 支撑追问）+ input/result 归属；agent_execution→agent_run | #761 |
| 2026-07-11 | result 统一经 EventSink + 终态族对称载荷（RunCompleted / RunFailed / RunCancelled）| #761 |
| 2026-07-11 | Model Invocation 补重试：Retryable(超时/5xx/429)退避重试、context 超限→compact、仅 Fatal/耗尽→Failed；emit ModelInvocationRetrying | #761 |
| 2026-07-11 | 重试升级为梯度重试 §5：T0 即时/T1 退避/T2 降级/T3 故障转移(pool)/T4 放弃 | #761 |
| 2026-07-11 | 重试收敛为 T0-T1 退避（≤10 次，单次退避封顶 5 分钟），去掉 T2 降级/T3 故障转移 | #761 |
| 2026-07-12 | 取消建模为 `InterruptRequested → Cancelling → Cancelled`；明确 per-Run scope、同步请求/异步收口与父子传播 | #700 |
| 2026-07-12 | 重试补可见输出门禁：已提交 delta 且无法回滚时禁止自动重试 | #788 |
| 2026-07-12 | Finishing 接入 Stop Hook：命令执行最多 3 次、Run 阻断上限 15，第 16 次 RunFailed | #790 |
| 2026-07-14 | Loop 直接落实 ContextPort 四方法、per-step append、reasoning/ToolCatalog invocation 冻结与 Tool suspension 原序串行交互 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-15 | 以 `CancelRunStep` 与 `TerminateRun` 取代 Run 级 Cancel：增加 `DrainingInput/CancellingStep/FinalizingStep/Terminating/Terminated`；Cancel 10s、Terminate 5s 共用 deterministic StepFinalizer，永不调用 LLM summary；Session 是唯一回放源，未入 Session 的 InputBuffer 在 Terminate 时可丢弃 | [#700](https://github.com/rushsinging/aemeath/issues/700) |
| 2026-07-18 | #875 将重试口径明确为首次调用 + 最多 10 次重试（最多 11 attempts），单次退避封顶 60 秒 | [#875](https://github.com/rushsinging/aemeath/issues/875) |
| 2026-07-15 | 补充 Agent Tool 控制传播：Main CancelRunStep 对 child Run 递归发送 TerminateRun；全树共享父绝对 deadline；StepFinalizer 按 RunSpec 区分 Main deterministic summary+Full receipt 与 Sub None+Safety receipt | [#700](https://github.com/rushsinging/aemeath/issues/700) |
| 2026-07-19 | #876 落地共享 Loop 的 `freeze_step`/真实 RunStepId、Main/Sub ContextCoordinator、Provider ContextTooLong typed compact 回环、普通完成与当前兼容 cancel 的 finalized append。`TerminateRun → FinalizeCause::RunTerminated` 的生产 control 入口仍由 #879 原子切换承接，本文目标语义不变 | [#876](https://github.com/rushsinging/aemeath/issues/876) / [#879](https://github.com/rushsinging/aemeath/issues/879) |
| 2026-07-21 | #1278 将 Context durable schema 收口为 `FinalizedOutcomeProjection`，并更正 Stop Hook Block：当前 assistant / Tool outcome 先持久化，feedback 仅进入下一 Step；#1247 继续承接生产控制命令与 deterministic receipt 的完整接线 | #1278 / #1247 |
| 2026-07-20 | 纠正 Stop Hook 的历史语义：Block 只阻断 Run 终止，已完成 assistant / Tool Step 必须先持久化；feedback 与同次 drain 的用户追问组成下一 Step，控制请求优先于 continuation | #743 |
| 2026-07-22 | #1272 落地 per-turn drain-or-seal：`DrainOutcome` 全量（`Ready`/`InternalContinuation(StopHookFeedback,ToolResults)`/`EmptyAndSealed`/`NoInput`）、`DrainEpoch` 双向校验、Main `RunInputBuffer`（`drain_or_seal`/`take_internal_continuation`/`try_drain_unsealed`/`push_or_reject`）、Sub FixedInputBuffer epoch 0/1、AwaitUser 同一 Run 恢复、正常完成路径移除 `Finishing` 直通 `DrainingInput`；#1277 accepted input 已接入、#1278 所有权不变、#1247 `PendingInteraction` 生产接线仍 out of scope、#879 物理退役仍后续 | [#1272](https://github.com/rushsinging/aemeath/issues/1272) / [#1277](https://github.com/rushsinging/aemeath/issues/1277)
