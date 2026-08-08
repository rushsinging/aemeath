# Agent Runtime · 状态机与 Loop Engine

> 层级：02-modules / runtime（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#761（S2）
> 本文定义 Run 单一状态机、统一 Loop Engine 骨架，以及"Main 常驻多轮 vs Sub 单次"的输入模型统一。实现差距与退役责任只在 [迁移治理](../../03-engineering/03-migration-governance.md) 维护。

## 1. Run 状态机（唯一，内存态）

完整 Mermaid 状态转换图及聚合不变量见 [领域模型 §2.1](01-domain-model.md#21-run-状态机转换图)。本章专注转换条件、Loop 编排顺序和异常收口，不维护第二份图形真相。

状态主干：

```text
Idle → DrainingInput ⇄ AwaitingInput
              │ Ready / InternalContinuation
              ▼
      PreparingContext ⇄ Compacting
              │
              ▼
        InvokingModel → ApplyingResponse
                              ├─ end turn ────────────────┐
                              ├─ tool calls → Approval → Tools
                              └─ interaction → AwaitingInteraction
                                                   │ typed continuation
                                                   └───────────────┐
                                                                   ▼
                                                            FinalizingStep
                                                                   │
                                                                   ▼
                                                             DrainingInput

CancelRunStep: active Step → CancellingStep → FinalizingStep → DrainingInput
TerminateRun: 任意非终态 → Terminating → Terminated
终态：Completed / Failed / Terminated
```

`AwaitingInput` 与 `AwaitingInteraction` 是正交状态：前者只等待普通 `UserMessage`，后者只等待匹配 interaction identity 的 reply/cancel。普通输入不得恢复 interaction continuation；interaction reply 不得进入 InputQueue。

正常 finalized Step 与 `CancelRunStep` 收口后均进入 `DrainingInput`：有输入（`Ready`/`InternalContinuation`）继续下一 Step；队列保持 Open 且暂无输入时进入 `AwaitingInput`；admission 已 seal 且为空时由 `EmptyAndSealed` 正常 `Completed`。`TerminateRun` 才终止整个 Run，并在退出前完成同等质量的 Step 收口和 Session flush。Run 级 cancellation 兼容路径不存在，也不得从 root cancellation token 推断第二个 Run 终态。

`DrainEpoch` 是 Run-owned 单调递增计数器，在同一 `run_loop` 生命周期内持续推进。每次成功 drain 后递增；`AwaitingInput` park 不重置 epoch。Engine 和 InputQueue 双向校验 epoch，不匹配返回 typed error。

### 状态转换矩阵

| 源状态 | 事件/条件 | 目标状态 |
|---|---|---|
| Idle | `RunLauncher::launch` | DrainingInput |
| DrainingInput | `drain` → `Ready`（非空 batch） | PreparingContext |
| DrainingInput | `drain` → `InternalContinuation`（StopHookFeedback 或 ToolResults，可携同期用户输入） | PreparingContext |
| DrainingInput | `drain` → `NoInput`（队列 Open，不 seal、epoch 不推进） | AwaitingInput |
| AwaitingInput | `await_input` → `Ready(UserMessage)` | DrainingInput → PreparingContext |
| DrainingInput | `drain` → `EmptyAndSealed`（admission 已 seal 且队列为空） | Completed(`InputDrained` 或 `StepCancelledAndInputDrained`) |
| PreparingContext | needs_compaction | Compacting |
| Compacting | 回收完成 | PreparingContext |
| PreparingContext | 上下文就绪 | InvokingModel |
| InvokingModel | LLM 响应 | ApplyingResponse |
| InvokingModel | Retryable 错误(超时/5xx/429) | InvokingModel（退避重试 ≤10 次，见 §5）|
| InvokingModel | context 超限 | Compacting（compact 后重跑，非重试）|
| InvokingModel | Fatal 错误(4xx) / 重试耗尽 | Failed |
| ApplyingResponse | 有 tool_calls | AwaitingToolApproval |
| ApplyingResponse | 需要 plan approval | AwaitingInteraction（`ContinuePlanApproval`） |
| ApplyingResponse | 无 tool_calls / EndTurn | FinalizingStep → DrainingInput |
| ApplyingResponse | Stop Hook Block（未超过上限） | FinalizingStep → DrainingInput（feedback 经 `InternalContinuation(StopHookFeedback)` 进入下一 Step） |
| AwaitingToolApproval | 全部放行 | ExecutingTools |
| AwaitingToolApproval | 需人工确认 | AwaitingInteraction（`ContinueToolApproval`） |
| ExecutingTools | Tool 返回 `Suspended(UserInteraction)` | AwaitingInteraction（`CompleteToolCall`） |
| ExecutingTools | StuckGuard `HardPause` | capability 可用：AwaitingInteraction（`ContinueAfterHardPause`）；unavailable：Failed |
| ExecutingTools | 结果回收完 | FinalizingStep → DrainingInput |
| AwaitingInteraction | 匹配 reply | 按 typed continuation 恢复到 ExecutingTools / AwaitingToolApproval / PreparingContext |
| AwaitingInteraction | completion=`Cancelled` + Tool continuation | ToolCall 得到 typed Cancelled，回原 Tool 状态继续 |
| AwaitingInteraction | completion=`Cancelled` + Plan/HardPause continuation | Failed（typed PlanApprovalCancelled / HardPauseCancelled） |
| 任意 active Step 态 | `CancelRunStep` 获胜 | CancellingStep |
| CancellingStep | StepFinalizer 完成或 10s deadline 到达 | FinalizingStep（持久化 deterministic receipts / partial step） |
| FinalizingStep | cancel 原因的 Step 已持久化（`StepCancelled → DrainingInput`） | DrainingInput |
| 任意非终态（除 Terminating） | `TerminateRun` 获胜 | Terminating |
| Terminating | 同一 StepFinalizer 完成或 5s deadline 到达，Session flush 完成 | Terminated |
| 任意非终态（除 AwaitingInput、AwaitingInteraction、Terminating） | timeout>0 且超时 | Failed |

> **等待态 timeout 语义**：`AwaitingInput` 与 `AwaitingInteraction` 均不计入 `RunSpec.timeout` 的活动执行时间。前者等待新的普通输入，后者等待特定结构化答复；进入等待态时暂停 activity timer，离开时恢复。`AwaitingToolApproval` 在全部自动放行时是瞬时态，仅需人工确认时才进入 `AwaitingInteraction`。

**控制优先级**：一旦接受 `CancelRunStep`，当前 Step 进入 `CancellingStep`；该 Step 后续普通完成、timeout 或错误只作为收口诊断，NEVER 把它伪装为普通 Completed。Step 收口并持久化后 Run 必须进入 `DrainingInput`。一旦接受 `TerminateRun`，Run 进入 `Terminating`；后续 Step 完成仅作为终止收口事实，Run 最终只能进入 `Terminated`。重复控制命令必须幂等。

**等待边界**：`AwaitingInput` 不保存 interaction continuation，只等待 InputQueue；`AwaitingInteraction` 必须与唯一 `PendingInteraction` 同时存活，等待 `run_id + request_id` 的答复。reply / interaction cancellation 只能恢复或终结该 typed continuation，NEVER 统一跳到 `PreparingContext`。四类 completion 的穷尽映射见 [端口与适配器](06-ports-and-adapters.md) §2。

## 2. Loop Engine 骨架（统一执行，零来源分支）

### 2.0 控制反转与能力注入

`run_loop` 直接接收 `Run`、`RunExecutionState` 与冻结的 `RuntimeContext`。Loop Engine 是完整执行流程的唯一 owner；它通过 Runtime-owned 窄 Port 调用外部能力，不通过 fat adapter 回调流程步骤。

```rust
async fn run_loop(
    run: &mut Run,
    execution: &mut RunExecutionState,
    context: &RuntimeContext,
) -> Result<AgentRunTerminal, RunExecutionError>
```

差异只存在于 `RuntimeContextFactory` 已绑定的 capability adapter：

| 能力面 | 可绑定实现示例 |
|---|---|
| Input | live session queue / parent-dispatched task queue / scheduler queue |
| Event | client event sink / parent diagnostic sink / no-op sink |
| Interaction | client / parent-mediated / unavailable |
| Hook | full / boundary-only / disabled |
| Context、Tool、Memory、Workspace | shared / isolated / restricted / no-op |

Engine 不知道某个 Run 来自 Main、Sub、Reflection 或 Scheduler，也不按来源选择流程。`MainRunPort`、`SubAgentRun`、fat `RunLoopPort`、`MainInputStrategy` / `SubInputStrategy`、`MainEventStrategy` / `SubEventStrategy` 均不属于终态。模型调用、Tool 编排、Stop Hook、Interaction、finalization 和 terminal mutation 必须只有一份 application 流程。

`RunExecutionState` 持有 messages、accepted inputs、context window、tool working data、continuation working data 与 stream progress；`RuntimeContext` 只持绑定好的外部能力；`Run` 只持领域状态。三者不得反向调用 Engine 或复制状态。`AcceptedUserInput` 是 Session gate 接纳后直到 Step freeze 的唯一 typed 输入事实；`UserMessage` 与 `SkillRequest` 只在模型消息 materialization 规则上分支，**NEVER** 形成 `adopted_messages` / `adopted_events` 双轨。

模型调用边界按职责拆为两面：`ModelInvocationContext` 提供已绑定的 RuntimeContext、日志上下文、Reducer、等待事件上下文与 ToolCall 提取等调用输入；`ModelInvocationLifecycle` 仅承载窗口就绪、输入泵、重试、响应和终态分类回调。两者共同服务唯一 model coordinator，不能成为可替换整个调用流程的 fat port。

跨边界值转换不使用 `Projection` 作为架构角色名。领域事件到 SDK 的单向转换由 `sdk_event_mapper` 表达；Hook outcome 到 Runtime typed dispatch 的单向转换由 `outcome_mapper` 表达；恢复结果和已提交 Step 数据分别使用 `View` 与 `Record` 命名。

Session 对外只有一个 `SessionIngress`。入口先把纯值消息分类为 `UserMessage`、`Command` 或 `InteractionCommand`，再分别投递到目标 Run 的 `InputQueue`、`CommandScheduler` 或 `InteractionInbox`。Loop 不读取 SDK channel，也不以 poll 顺序猜测消息类别。

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

### 2.1 Session 回放边界与 InputQueue

Session 是可回放数据的唯一真相源；"可回放"只承诺已经提交到 Session 的内容，**NEVER** 承诺重建 Runtime 内存态。

- Provider partial、Tool/Agent progress 或结果只有在成为 Session committed content 后，才属于 resume/replay 边界；TUI 的临时流式 view 本身不是 durable source。
- CancelRunStep 收口时，当前 Step 的已确认事实、partial assistant、Tool/Agent deterministic receipts 通过 StepFinalizer 写入 Session；下一 Step 从 Session committed content 与新 drain batch 构建 Context。
- InputQueue 中尚未被当前 Step 接纳的内容尚未进入 Session，因此 TerminateRun **MAY** 直接丢弃，不持久化、不恢复、不计入 Session 回放；只允许记录不含内容的 count/bytes 诊断。
- 已经绑定当前 Step 并由 `append_accepted_input` 成功写入 Session 的 user input 不再属于 InputQueue，必须随 Session 回放；该 handoff 发生在 `freeze_step` 后、首次 `build_window` / compact / model 前。handoff 只传 accepted user facts，**NEVER** 混入 system-generated Stop feedback、assistant、Tool result、RunStatus 或进行态。Context 在自己的 mutation gate 内取得提交 revision；后续 outcome 以 window 的 `backing_revision` CAS 补充同一 Step，**NEVER** 重复 user input。
- TerminateRun 完成前必须 flush Session 已有 committed content；不要求把未提交 buffer 内容提升为 Session 事实。

### 2.2 Stop Hook 持久化与 continuation 边界

Stop Hook 只裁决 Run 能否终止，**NEVER** 否决已完成 assistant / Tool 产出的历史事实。最终 assistant Step 的 `append_and_persist`、Stop 判定与 continuation **MUST** 按以下顺序执行：

1. `apply_response` + `apply_results` 形成不可变当前 Step 事实；
2. **Stop Hook dispatch**：
   - `Continue`：进入 `FinalizingStep`，以 `FinalizeCause::Completed` cancellation-shielded 提交当前 Step，随后 Run Completed；
   - `Block` 且未超过上限：当前 Step 同样进入 `FinalizingStep` 并提交；Runtime 将 Hook 的结构化 reason 转为 system-generated feedback，随后重新进入 `DrainingInput`；
   - `Block` 且超过当前 Run 冻结的上限：当前 Step 仍先经 `FinalizingStep` 提交，再进入 `Failed`，错误文本保留实际阻断次数；
3. Block 后的下一次 `drain_or_seal` 必须构造一个稳定 batch：**已提交的 assistant / Tool 历史**在 Context backing 中；新 batch 以 Stop feedback 为系统前缀，再追加该次 drain 收到的普通用户追问（FIFO）。三者在同一次下一 Step Context Window 中可见；
4. Stop feedback 仅是 Runtime 生成的系统输入，不是 Hook BC 对 Session 的直接写入；Hook BC 只返回结构化 directive / reason；
5. `CancelRunStep` 或 `TerminateRun` 一旦获胜，优先于尚未绑定的 Stop continuation：不得发起下一次模型调用。CancelRunStep 由 StepFinalizer 收口当前事实后进入 Drain；TerminateRun seal admission、丢弃未绑定 InputQueue 内容并 flush 已提交 Session；
6. commit handoff 后 owned commit 必须跑到明确成功或失败，caller cancellation **NEVER** 中断 durable commit；commit 成功后立即 `mark_step_persisted`，该 Step 从此不属于 partial、取消时 **NEVER** 回滚。

> Session committed content 是唯一历史真相。Stop Block 的语义是“继续同一个 Run”，不是撤销 assistant 已产生的内容；feedback 和用户追问只决定该 Run 的**下一 Step**。

### 2.3 HardPause Continuation

从 `ExecutingTools` 因 StuckGuard HardPause 进入 `AwaitingInteraction(ContinueAfterHardPause)` 时，continuation **MUST** 记录当前 step 和 tool phase：

- 若恢复（HardPauseContinue）：回到 `ExecutingTools` 继续未完成的 Tool 调用，**NEVER** 直接跳到 `PreparingContext`；
- 若取消：为当前 step 的全部未完成 ToolCall 生成 typed Cancelled results，按原顺序提交完整 step（保持 assistant/tool-result 邻接协议），**THEN** 进入 Failed。

### 2.4 领域事件发布不变量

`Run` 是生命周期事件的唯一生产者。**每一次** Run 聚合状态 mutation 返回后，调用方都必须在执行下一条业务语句或 `.await` 前立即执行 `run.drain_events()` 并把结果交给 `EventSink`；禁止只在 response 或 loop 末尾批量 drain。该规则覆盖 `RunStarted`、`RunAwaitingInput`、`RunInteractionRequested`、对应 resumed 事件、step/tool 状态以及全部 terminal 事件。

伪代码用 `mutate_and_publish(run, &ctx.events, |run| ...)` 表示这个原子编排约定：closure 内只做一次聚合 mutation，返回后 helper 立即 drain + emit。interaction coordinator 也必须逐次使用同一 helper，先发布 `RunInteractionRequested` 再 `.await` completion，恢复 continuation 后先发布 `RunInteractionResumed` 再继续。epilogue 只执行 `assert_terminal` / `assert_no_pending_events`；**NEVER** 在末尾补造终态或延迟发布事件。

### 2.5 Loop 内部逻辑伪代码

以下伪代码展示完整 application 流程，而不只是 mailbox drain 骨架。辅助函数都属于 Loop Engine 内部协作器，不是可替换整段流程的 Port。

```rust
async fn run_loop(
    run: &mut Run,
    execution: &mut RunExecutionState,
    context: &RuntimeContext,
) -> Result<AgentRunTerminal, RunExecutionError> {
    if run.status() == RunStatus::Idle {
        mutate_and_publish(run, context.events(), Run::start_draining)?;
    }

    loop {
        apply_immediate_control(run, execution, context).await?;
        if let Some(terminal) = run.terminal_result() {
            assert_no_pending_events(run)?;
            return Ok(terminal);
        }

        match run.status() {
            RunStatus::DrainingInput => {
                let epoch = run.next_drain_epoch();
                match context.input().drain(epoch).await? {
                    DrainOutcome::Ready { batch, epoch }
                    | DrainOutcome::InternalContinuation { batch, epoch, .. } => {
                        mutate_and_publish(run, context.events(), |run| {
                            run.accept_drain(epoch, &batch)
                        })?;
                        execution.accept_inputs(batch)?;
                    }
                    DrainOutcome::NoInput { epoch } => {
                        mutate_and_publish(run, context.events(), |run| {
                            run.await_input(epoch)
                        })?;
                    }
                    DrainOutcome::EmptyAndSealed { epoch } => {
                        mutate_and_publish(run, context.events(), |run| {
                            run.complete_after_drain(
                                epoch,
                                execution.terminal_projection(),
                            )
                        })?;
                    }
                }
            }

            RunStatus::AwaitingInput => {
                let epoch = run.next_drain_epoch();
                let ready = context.input().await_input(epoch).await?;
                // 这里只可能接受 UserMessage；command 由 scheduler 处理，
                // interaction reply 由 InteractionInbox 处理。
                mutate_and_publish(run, context.events(), |run| {
                    run.resume_input(epoch, &ready)
                })?;
                execution.accept_inputs(ready.batch)?;
            }

            RunStatus::PreparingContext => {
                freeze_and_persist_accepted_input(run, execution, context).await?;
                if context.context().needs_compaction(execution)? {
                    mutate_and_publish(run, context.events(), Run::start_compacting)?;
                } else {
                    build_invocation_window(run, execution, context).await?;
                    mutate_and_publish(run, context.events(), Run::start_invoking_model)?;
                }
            }

            RunStatus::Compacting => {
                compact_context(run, execution, context).await?;
                mutate_and_publish(run, context.events(), Run::finish_compacting)?;
            }

            RunStatus::InvokingModel => {
                let completion = invoke_with_retry_and_control(
                    run,
                    execution,
                    context,
                ).await?;
                execution.record_invocation(completion)?;
                mutate_and_publish(run, context.events(), Run::start_applying_response)?;
            }

            RunStatus::ApplyingResponse => {
                match apply_model_response(run, execution, context).await? {
                    ResponseDirective::ToolCalls => {
                        mutate_and_publish(
                            run,
                            context.events(),
                            Run::start_tool_approval,
                        )?;
                    }
                    ResponseDirective::Interaction(request, continuation) => {
                        begin_interaction(
                            run,
                            execution,
                            context,
                            request,
                            continuation,
                        ).await?;
                    }
                    ResponseDirective::Finalize(cause) => {
                        mutate_and_publish(run, context.events(), |run| {
                            run.start_finalizing(cause)
                        })?;
                    }
                }
            }

            RunStatus::AwaitingToolApproval => {
                match evaluate_tool_approval(run, execution, context).await? {
                    ApprovalDirective::Execute => {
                        mutate_and_publish(run, context.events(), Run::start_tools)?;
                    }
                    ApprovalDirective::Ask(request, continuation) => {
                        begin_interaction(
                            run,
                            execution,
                            context,
                            request,
                            continuation,
                        ).await?;
                    }
                    ApprovalDirective::Reject(results) => {
                        execution.apply_tool_results(results)?;
                        mutate_and_publish(run, context.events(), |run| {
                            run.start_finalizing(FinalizeCause::ToolsCompleted)
                        })?;
                    }
                }
            }

            RunStatus::ExecutingTools => {
                match execute_tool_round(run, execution, context).await? {
                    ToolDirective::Completed(results) => {
                        execution.apply_tool_results(results)?;
                        mutate_and_publish(run, context.events(), |run| {
                            run.start_finalizing(FinalizeCause::ToolsCompleted)
                        })?;
                    }
                    ToolDirective::Interaction(request, continuation) => {
                        begin_interaction(
                            run,
                            execution,
                            context,
                            request,
                            continuation,
                        ).await?;
                    }
                }
            }

            RunStatus::AwaitingInteraction { request_id } => {
                let completion = context.interactions()
                    .await_completion(run.id(), request_id)
                    .await?;
                // identity 必须同时匹配 run_id + request_id；普通输入仍留在 InputQueue。
                resume_typed_continuation(
                    run,
                    execution,
                    context,
                    completion,
                ).await?;
            }

            RunStatus::CancellingStep => {
                finalize_step(
                    run,
                    execution,
                    context,
                    FinalizeCause::StepCancelled,
                ).await?;
                mutate_and_publish(run, context.events(), Run::finish_cancelled_step)?;
            }

            RunStatus::FinalizingStep => {
                let directive = finalize_current_step(run, execution, context).await?;
                match directive {
                    FinalizeDirective::Continue(batch) => {
                        context.input().submit_internal(batch).await?;
                        mutate_and_publish(run, context.events(), Run::start_draining)?;
                    }
                    FinalizeDirective::Drain => {
                        mutate_and_publish(run, context.events(), Run::start_draining)?;
                    }
                    FinalizeDirective::Fail(error) => {
                        mutate_and_publish(run, context.events(), |run| run.fail(error))?;
                    }
                }
            }

            RunStatus::Terminating => {
                terminate_with_shared_finalizer(run, execution, context).await?;
                context.input().close_and_discard_unaccepted().await?;
                context.context().flush_session().await?;
                mutate_and_publish(run, context.events(), Run::finish_termination)?;
            }

            RunStatus::Completed
            | RunStatus::Failed
            | RunStatus::Terminated => {
                // 下一轮 loop 顶部返回唯一 terminal projection。
            }

            RunStatus::Idle => unreachable!("Idle 只允许在入口激活一次"),
        }

        run_at_boundary_commands(run, execution, context).await?;
    }
}
```

Loop 内部必须保持以下顺序约束：

1. 每轮先处理 ImmediateControl；每次 `.await` 返回后再次观察 Step/Run cancellation scope。
2. 所有状态转换必须经 Run 聚合方法；转换后立刻 drain domain events 到 `EventSink`。
3. 已接纳的 `UserMessage` 在首次 model/compact 前提交 Session；未接纳的 InputQueue 内容不是 durable history。
4. interaction 开始前先原子保存 `PendingInteraction + typed continuation` 并发布事件，再等待 reply；恢复时先验证 identity，再按 continuation 返回原工作阶段。
5. Model、Tool、Compact、Hook 都只调用 `RuntimeContext` 的窄 Port；不得由 adapter 决定下一状态。
6. 普通完成、取消和终止共用 deterministic StepFinalizer；区别只在 terminal intent、deadline 和收口后的目标状态。
7. `AtRunBoundary` command 只在当前原子阶段完成后执行；`SessionQuery` 由 Session Runtime 直接处理，不进入本函数。

**#1272 关键点：**
- 正常完成（Complete/Continue/StopHookBlocked/ToolsCompleted）均进入 `ContinueAfterResponse` / `ToolsCompleted` → `DrainingInput`，不加中间状态；
- `Completed` 只能由 `EmptyAndSealed` 产生；
- AwaitingInput 恢复：`NoInput` 时 `InputPort::await_input` 保持同一 future 异步等待且 epoch 不推进；用户输入到达产生 `Ready` 后执行 `UserResumed`；
- AwaitingInteraction 恢复：只等待匹配 `run_id + request_id` 的 reply/cancel，普通用户输入仍留在 InputQueue，不得误恢复 interaction continuation；
- 派生任务输入：创建 Idle child Run 后由 SessionIngress 投递 `UserMessage`；epoch 0 `Ready(task)`，后续是否 seal 由 Run admission policy 决定，不存在固定 prompt adapter。

### 2.6 控制协议：请求同步，完成异步

Runtime 入站命令区分两个 scope，均不经过 InputQueue：

1. `cancel_run_step(run_id, step_id?)`：同步原子迁移当前 Step 到 `CancellingStep`、触发 Step scope、返回 typed outcome；异步 StepFinalizer 最长 10s，完成后 Run 固定进入 `DrainingInput`。
2. `terminate_run(run_id, reason)`：同步迁移 Run 到 `Terminating`、seal input admission、触发 Run root scope、返回 typed outcome；异步复用同一 StepFinalizer（最长 5s）、丢弃未进入 Session 的 InputQueue 内容、flush Session，最终进入 `Terminated`。
3. CancelRunStep 后 Drain 有输入则 `PreparingContext` 开下一 Step；无输入且 drain epoch 原子 seal 则 `Completed(reason=StepCancelledAndInputDrained)`。
4. TerminateRun 不回到 Drain；resume 只回放 Session committed content，并创建新 Run。
5. 当前不定义 Force Terminate。

Step scope 是 Run root scope 的 child；CancelRunStep 不污染下一 Step token，TerminateRun 传播到全部 Step/Tool/SubRun scope。

### 2.7 Agent Tool / Sub Run 控制传播

父 Run 当前 Step 接受 `CancelRunStep` 后，对普通 Tool 取消该 Tool operation；对 Agent Tool **MUST** 向关联 child Run 发送 `TerminateRun(ParentStepCancelled)`，**NEVER** 向 child 发送 CancelRunStep 后让它回到 Drain 继续执行。child 再对其嵌套 Agent Tool 递归传播 TerminateRun。

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

- `AwaitingInteraction`（ask_user / approval / hard pause）：同一个 Run 内暂停/resume，Run 未完成；
- `AwaitingInput`：同一个 Run 等新的普通 `UserMessage`，不持有 interaction continuation；
- `Completed` 后等下一输入：Run 完成，Session 层开新 Run（不是同一 Run）

### InputQueue（Run 入站 mailbox）— 支撑首次输入与追问

Loop Engine 每轮在门禁点调用 `RuntimeContext.input().drain(epoch)` 或 `await_input(epoch)`。live session、parent-dispatched task 与未来 scheduler 来源都在 `SessionIngress` 分类后进入目标 Run 的同一种 InputQueue；Engine 不感知来源，也不按 Main/Sub 区分。不存在 `fixed initial input` adapter。

Run-owned atomic InputQueue 提供 drain、park 与 admission 生命周期：

| 方法 | 行为 | seal? | epoch 推进? |
|---|---|---|---|
| `drain(epoch)` | 用户输入非空 → `Ready`；队列 Open 且空 → `NoInput`；已 Sealed 且空 → `EmptyAndSealed` | 不隐式 seal | Ready / terminal drain 时 |
| `submit_internal(batch)` | engine-driven continuation（StopHookFeedback/ToolResults） | Never | 下次 drain 时 |
| `await_input(epoch)` | AwaitingInput 用；保持 future 直到 UserMessage 到达或 admission 关闭 | Never | 仅 Ready 时 |
| `submit_or_reject(message)` | Open 时入队；Sealed/Closed 后 typed reject | — | — |
| `seal()` | 禁止新 UserMessage；保留已排队输入供最终 drain | Yes | — |
| `close_and_discard_unaccepted()` | Run 终止时清理未接纳输入，仅记录非内容诊断 | Closed | — |

所有 Run 都绑定同一种语义的 InputQueue：

| Run 来源 | 投递方式 | 行为 |
|---|---|---|
| client-facing interactive Run | `SessionIngress::UserMessage` | 首次输入与忙期追问均 FIFO 入队；`AwaitingInput` 时新消息唤醒同一 Loop |
| parent-dispatched child Run | 父调度器提交 `SessionIngress::UserMessage` 并指定 child RunId | 任务文本不是构造参数，也不是 fixed adapter；是否允许后续输入由 admission/capability 决定 |
| scheduler/background Run | scheduler 提交同一 `UserMessage` Published Language | Engine 不感知来源，仍通过相同 drain/epoch 契约消费 |

- `input` 是 **RuntimeContext 的入站 Port**；InputQueue 的具体实现由 factory 绑定，但语义由 Runtime 拥有。
- `run_loop` 只在 `Completed / Failed / Terminated` 返回 typed terminal；等待输入或 interaction 均在同一调用内 park，不向 caller 返回等待 directive。
- `DrainEpoch` 是 Run-owned 单调计数器；Engine 和 InputQueue 双向校验，不匹配返回 typed error。

## 4. 停止条件与等待态语义

### 停止条件

| 条件 | 结果 |
|---|---|
| 无 tool_calls / stop_reason=EndTurn，ContinueAfterResponse → DrainingInput → EmptyAndSealed | Completed |
| Stop Hook Block（累计≤当前 Run 冻结上限） | 当前 Step 提交 → InternalContinuation(StopHookFeedback) + 同次 drain 用户追问 → PreparingContext，同一 Run 继续 |
| Stop Hook Block 累计>当前 Run 冻结上限 | 当前 Step 提交 → Failed，错误文本保留实际阻断次数 |
| timeout>0 且墙钟超时 | Failed |
| StuckGuard HardPause | interaction capability 可用 → AwaitingInteraction；Unavailable → Failed |
| CancelRunStep 且 Drain 无新输入、admission 保持 Open | StepFinalizer → DrainingInput → AwaitingInput |
| CancelRunStep 且 Drain 有输入 | StepFinalizer → DrainingInput → Ready → PreparingContext，继续下一 Step |
| CancelRunStep 且 admission 已 seal 且无输入 | StepFinalizer → DrainingInput → EmptyAndSealed → Completed(`StepCancelledAndInputDrained`) |
| TerminateRun | 同一 StepFinalizer（≤5s）+ 丢弃未入 Session 的 InputQueue 内容 + Session flush → Terminated |
| LLM Fatal 错误 / 重试耗尽 | Failed（Retryable 先退避重试；context 超限→compact 重跑）|

### AwaitingInput 语义

`AwaitingInput` 是 Run 内等待普通 `UserMessage` 的非终态。Loop Engine 在检测到该状态时调用绑定后的 `InputPort::await_input(epoch)`：

- `await_input` 从不 seal InputQueue；
- 暂无用户输入时保持同一可取消 future，epoch 不推进，Loop 不退出也不返回给 caller；
- 用户输入到达后返回 `Ready`，Engine 执行 `RunInputResumed` 并在同一 `run_loop` 调用内继续；
- `Command` 和 `InteractionCommand` 不由该 future 消费，分别由 CommandScheduler 和 InteractionInbox 处理。

### AwaitingInteraction 语义

`AwaitingInteraction { request_id }` 是 Run 内等待结构化 reply/cancel 的非终态：

- 进入前必须保存唯一 `PendingInteraction` 和 typed continuation，并发布 `RunInteractionRequested`；
- `InteractionInbox` 只接受匹配 `run_id + request_id` 的 completion，重复、陈旧、不匹配的 reply 返回 typed error；
- 普通 `UserMessage` 可以继续进入 InputQueue，但不得唤醒或完成 interaction continuation；
- completion 到达后先验证 identity，再发布 `RunInteractionResumed`，按 continuation 恢复 Tool、Approval 或 HardPause 工作；
- cancel、timeout、parent disconnect 和 session shutdown 都必须走 typed completion，不能伪造普通用户输入。

两种等待均不属于终态；只有 `Completed / Failed / Terminated` 才能结束 `run_loop`。

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
- `stop_block_count` 不超过当前 Run 冻结的 `StopHookPolicy` 上限时，将反馈作为 system-generated input 加入下一步并回 PreparingContext。
- 首个超限阻断进入 `Failed` 并发布 `RunFailed { error }`；错误文本保留实际阻断次数，且不得强制 Completed。
- 两个上限分别归 Hook 和 Runtime，静态默认值均由 ConfigSnapshot 提供；Dispatcher 与每个 Run 分别冻结 typed policy。

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
| 2026-07-11 | 补 InputQueue 入站 Port（Loop 门禁 drain 支撑追问）+ input/result 归属；agent_execution→agent_run | #761 |
| 2026-07-11 | result 统一经 EventSink + 终态族对称载荷（RunCompleted / RunFailed / RunCancelled）| #761 |
| 2026-07-11 | Model Invocation 补重试：Retryable(超时/5xx/429)退避重试、context 超限→compact、仅 Fatal/耗尽→Failed；emit ModelInvocationRetrying | #761 |
| 2026-07-11 | 重试升级为梯度重试 §5：T0 即时/T1 退避/T2 降级/T3 故障转移(pool)/T4 放弃 | #761 |
| 2026-07-11 | 重试收敛为 T0-T1 退避（≤10 次，单次退避封顶 5 分钟），去掉 T2 降级/T3 故障转移 | #761 |
| 2026-07-12 | 取消建模为 `InterruptRequested → Cancelling → Cancelled`；明确 per-Run scope、同步请求/异步收口与父子传播 | #700 |
| 2026-07-12 | 重试补可见输出门禁：已提交 delta 且无法回滚时禁止自动重试 | #788 |
| 2026-07-12 | Finishing 接入 Stop Hook：命令执行最多 3 次、Run 阻断上限 15，第 16 次 RunFailed | #790 |
| 2026-07-14 | Loop 直接落实 ContextPort 四方法、per-step append、reasoning/ToolCatalog invocation 冻结与 Tool suspension 原序串行交互 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-15 | 以 `CancelRunStep` 与 `TerminateRun` 取代 Run 级 Cancel：增加 `DrainingInput/CancellingStep/FinalizingStep/Terminating/Terminated`；Cancel 10s、Terminate 5s 共用 deterministic StepFinalizer，永不调用 LLM summary；Session 是唯一回放源，未入 Session 的 InputQueue 内容在 Terminate 时可丢弃 | [#700](https://github.com/rushsinging/aemeath/issues/700) |
| 2026-07-18 | #875 将重试口径明确为首次调用 + 最多 10 次重试（最多 11 attempts），单次退避封顶 60 秒 | [#875](https://github.com/rushsinging/aemeath/issues/875) |
| 2026-07-15 | 补充 Agent Tool 控制传播：Main CancelRunStep 对 child Run 递归发送 TerminateRun；全树共享父绝对 deadline；StepFinalizer 按 RunSpec 区分 Main deterministic summary+Full receipt 与 Sub None+Safety receipt | [#700](https://github.com/rushsinging/aemeath/issues/700) |
| 2026-07-19 | #876 落地共享 Loop 的 `freeze_step`/真实 RunStepId、Main/Sub ContextCoordinator、Provider ContextTooLong typed compact 回环、普通完成与当前兼容 cancel 的 finalized append。`TerminateRun → FinalizeCause::RunTerminated` 的生产 control 入口仍由 #879 原子切换承接，本文目标语义不变 | [#876](https://github.com/rushsinging/aemeath/issues/876) / [#879](https://github.com/rushsinging/aemeath/issues/879) |
| 2026-07-21 | #1278 将 Context durable schema 收口为 `FinalizedStepRecord`，并更正 Stop Hook Block：当前 assistant / Tool outcome 先持久化，feedback 仅进入下一 Step；#1247 继续承接生产控制命令与 deterministic receipt 的完整接线 | #1278 / #1247 |
| 2026-07-20 | 纠正 Stop Hook 的历史语义：Block 只阻断 Run 终止，已完成 assistant / Tool Step 必须先持久化；feedback 与同次 drain 的用户追问组成下一 Step，控制请求优先于 continuation | #743 |
| 2026-07-22 | #1272 落地 per-turn drain/admission：`DrainOutcome` 全量（`Ready`/`InternalContinuation(StopHookFeedback,ToolResults)`/`EmptyAndSealed`/`NoInput`），`DrainEpoch` 双向校验，统一 InputQueue 接受 live session、parent-dispatched task 与 scheduler UserMessage；AwaitingInput 同一 Run park，普通输入与 interaction reply mailbox 分离 | [#1272](https://github.com/rushsinging/aemeath/issues/1272) |
