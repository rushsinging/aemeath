# Agent Runtime · 状态机与 Loop Engine

> 层级：02-modules / runtime（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#761（S2）
> 本文定义 Run 单一状态机、统一 Loop Engine 骨架，以及"Main 常驻多轮 vs Sub 单次"的输入模型统一。实现差距与退役责任只在 [迁移治理](../../03-engineering/migration-governance.md) 维护。

## 1. Run 状态机（唯一，内存态）

```
Created
  │ Start
  ▼
PreparingContext ──────▶ InvokingModel ──────▶ ApplyingResponse
  ▲   │                                              │
  │   │ needs_compact                    ┌───────────┴───────────┐
  │   ▼                              有 ToolCall             无 ToolCall
  │ Compacting                           │                       │
  │   │                                  ▼                       ▼
  └───┘                          AwaitingToolApproval        Finishing
  │                                      │                       │
  │                                      ▼                       ▼
AwaitingUser ◀─ interaction ─── ExecutingTools            Completed
  (暂停 + typed continuation)                 │
                                        └──▶(回 PreparingContext 下一步)

AwaitingUser 收到匹配 reply 后按 continuation 回到
AwaitingToolApproval / ExecutingTools / PreparingContext 之一。

  终态旁路： Failed（错误/超时；Sub 无交互能力时的 HardPause）
  打断旁路：任意活跃态 ── InterruptRequested ──▶ Cancelling ── 收口完成 ──▶ Cancelled
```

`Cancelling` 是 Run 的一等过渡态，不是 UI 临时标记。进入后立即禁止启动新的 Model Invocation、Tool Call、Compaction 或 Hook，只允许等待在途工作响应 cancellation scope、回滚本 Run 的 partial assistant/tool 结果并发出终态事件。

### 状态转换矩阵

| 源状态 | 事件/条件 | 目标状态 |
|---|---|---|
| Created | Start | PreparingContext |
| PreparingContext | needs_compaction | Compacting |
| Compacting | 回收完成 | PreparingContext |
| PreparingContext | 上下文就绪 | InvokingModel |
| InvokingModel | LLM 响应 | ApplyingResponse |
| InvokingModel | Retryable 错误(超时/5xx/429) | InvokingModel（退避重试 ≤10 次，见 §5）|
| InvokingModel | context 超限 | Compacting（compact 后重跑，非重试）|
| InvokingModel | Fatal 错误(4xx) / 重试耗尽 | Failed |
| ApplyingResponse | 有 tool_calls | AwaitingToolApproval |
| ApplyingResponse | 需要 plan approval | AwaitingUser（`ContinuePlanApproval`） |
| ApplyingResponse | 无 tool_calls / EndTurn | Finishing |
| Finishing | Stop Hook Continue | Completed |
| Finishing | Stop Hook Block 且 stop_block_count≤15 | PreparingContext（反馈注入后继续同一 Run）|
| Finishing | Stop Hook Block 且 stop_block_count>15 | Failed(StopHookRetryExhausted) |
| AwaitingToolApproval | 全部放行 | ExecutingTools |
| AwaitingToolApproval | 需人工确认(approval) | AwaitingUser（`ContinueToolApproval`） |
| ExecutingTools | Tool 返回 `Suspended(UserInteraction)` | AwaitingUser（`CompleteToolCall`） |
| ExecutingTools | StuckGuard `HardPause` | Main：AwaitingUser（`ContinueAfterHardPause`）；Sub / unavailable：Failed |
| ExecutingTools | 结果回收完 | PreparingContext（下一步）|
| AwaitingUser | 匹配 reply + `ContinueToolApproval` | AwaitingToolApproval（应用决定后继续未决调用） |
| AwaitingUser | 匹配 reply + `CompleteToolCall` | ExecutingTools（完成原 ToolCall） |
| AwaitingUser | 匹配 reply + `ContinuePlanApproval` | PreparingContext（Approve / Deny 的 typed 结果先随当前 step 恰好一次提交，再进入下一 invocation；该 step 不得同时携 tool_calls） |
| AwaitingUser | 匹配 reply + `ContinueAfterHardPause` | PreparingContext |
| AwaitingUser | completion=`Cancelled` + Tool continuation | ToolCall 得到 typed Cancelled，回原 Tool 状态继续 |
| AwaitingUser | completion=`Cancelled` + Plan/HardPause continuation | Failed（typed PlanApprovalCancelled / HardPauseCancelled） |
| Finishing | 收尾完成 | Completed |
| 任意非终态（除 Cancelling） | InterruptRequested | Cancelling |
| Cancelling | Provider/Tool/Compact/Hook 已停止且回滚完成 | Cancelled |
| 任意非终态（除 Cancelling、AwaitingUser） | timeout>0 且超时 | Failed |

> **AwaitingUser timeout 豁免**：`AwaitingUser` 状态 **MUST NOT** 计入 RunSpec.timeout 的墙钟计时。用户交互等待时间不可预测，timeout 在进入 `AwaitingUser` 时暂停、离开时恢复。`AwaitingToolApproval` 在全部自动放行时是**瞬时态**（不停留），仅在需人工确认时才进入 `AwaitingUser(ContinueToolApproval)`；因此自动放行路径不受 timeout 影响。

**取消优先级**：一旦接受 `InterruptRequested`，Run 进入 `Cancelling`；后续到达的普通完成、timeout 或错误只能作为取消收口诊断，NEVER 把该 Run 改写为 `Completed`/`Failed`。重复取消必须幂等。

**AwaitingUser 关键语义**：这是 **Run 内交互暂停**（Run 未完成，等特定 request id 的答复），必须同时保存 typed continuation，内存存活、不落盘；崩溃则整个 Run 从头开始（见 `05-recovery-semantics`）。reply / interaction cancellation 只能恢复或终结该 continuation，**NEVER** 统一跳到 `PreparingContext`。四类 completion 的穷尽映射见 [端口与适配器](06-ports-and-adapters.md) §2。这**区别于**"Run 完成后 Session 等下一条全新输入"（那是 Run 序列层，见 §3）。

## 2. Loop Engine 骨架（Main/Sub 共用，零分支）

```rust
/// 驱动单个 Created Run 到终态；AwaitingUser 在本 future 内 await 后继续，NEVER 二次入栈。
async fn run_loop(
    run: &mut Run,
    ctx: &RuntimeContext,
    guard: &mut StuckGuard,
) -> Result<(), RunLoopError> {
    run.start_once();                                        // 仅 Created → PreparingContext
    'run: loop {
        if run.is_cancelling() {                             // 不再启动新工作
            ctx.cancel.await_quiesced();
            run.finish_cancellation(); break;                // → Cancelled
        }
        if let Some(reason) = guard.check_timeout(run) {     // L3 时间兜底（timeout=0 跳过）
            run.fail(reason); break;
        }
        for input in ctx.input.drain() {                     // 每个输入事实只 observe 一次
            ctx.reasoning.observe(run.classify_user_message(&input)); // UserMessage
            run.queue_context_input(input.into_message());  // append 成功前保持 pending
        }

        // 每次 PreparingContext 冻结一个 Catalog 与 Provider-resolved options。
        let catalog = ctx.tool_catalog.snapshot(
            run.spec.tools.scope(), run.spec.tools.profile(),
        )?;
        let tool_schemas = catalog.model_schemas();          // 本 invocation 唯一 schema 集
        let requested = ctx.reasoning.current_requested_level();
        let resolved = ctx.provider.resolve_invocation_options(
            &run.spec.model,
            RequestedInvocationOptions {
                requested_max_output_tokens: run.requested_output_limit(),
                reasoning: requested,
            },
        )?;                                                  // Provider-owned model clamp
        let request = context_coordination::freeze_request(
            run,
            &ctx.config,
            ContextRequestInputs {
                system_prompt: run.spec.system_prompt.clone(),
                pending_messages: run.pending_context_messages(),
                effective_reasoning: resolved.effective_reasoning,
                context_size: resolved.context_size,
                max_output_tokens: resolved.max_output_tokens,
                last_api_input_tokens: run.last_api_input_tokens(),
                tool_schema_tokens: estimate_tool_schemas_tokens(&tool_schemas),
                tool_schemas,
                task_reminder: ctx.task.reminder_snapshot().await,
                current_date: ctx.clock.calendar_date(),
                language: ctx.config.language(),
                agent_roles: ctx.config.agent_roles(),
                config_snapshot: ctx.config.clone(),
                // project_root / git_context 由 run-bound ContextPort 的 Project read view 冻结。
            },
        )?;

        let decision = ctx.context.needs_compaction(&request); // ContextPort ①
        if decision.needed {
            ctx.context.compact(&CompactRequest::automatic(request)).await?; // ②
            continue;
        }

        let window = ctx.context.build_window(&request).await?; // ③
        debug_assert_eq!(window.tool_schemas, request.tool_schemas);
        let invocation_request = InvocationRequest {
            model: run.spec.model.clone(),
            window,
            options: resolved,                                // 与 Prompt 共用 effective reasoning
        };
        let step = run.begin_step(request.request_id);        // → InvokingModel

        let inv = match model_invocation::invoke_with_retry(  // 普通 retry 复用冻结 request
            &ctx.provider, invocation_request, run.spec.retry, &ctx.cancel,
            |a| ctx.events.emit_retrying(a)).await {
            Ok(inv)               => inv,
            Err(ContextExceeded)  => {
                run.rollback_uncommitted_step(step);
                ctx.context.compact(&CompactRequest::context_exceeded(request)).await?; // ②
                continue;
            }
            Err(CapabilityChanged) => {
                run.rollback_uncommitted_step(step);
                continue;                                    // 丢弃旧 window，重新 resolve/build
            }
            Err(Fatal(e))         => { run.fail(e); break; }            // fatal/耗尽→Failed
        };
        debug_assert_eq!(inv.effective_reasoning, request.options.effective_reasoning);
        let response_text = inv.text().to_owned();
        run.apply_response(step, inv);                       // → ApplyingResponse
        ctx.events.emit(run.drain_events());                 // event_projection

        if guard.stall(&response_text) {                     // L1 文本重复
            run.mark_stuck(); /* soft: 喂回提示; hard: break→Failed */
        }

        if let Some(plan) = step.plan_approval_request() {
            debug_assert!(!step.has_tool_calls());             // plan decision 独占本 step
            let decision = interaction::await_plan_approval(
                run, plan, &ctx.interaction, &ctx.cancel,
            ).await?;                                         // Approved | Rejected { feedback }
            let append = context_coordination::completed_plan_step_append(
                run, step, request.request_id, decision,
            );                                                // assistant plan → typed user decision
            if let Err(error) = ctx.context.append_and_persist(append).await { // ContextPort ④，恰好一次
                run.fail(error.into());
                break;
            }
            run.mark_step_persisted(step);
            ctx.reasoning.observe(ReasoningSignal::TurnBoundary);
            run.resume_preparing_context();                   // 下一 invocation 消费决定
            continue;                                         // NEVER 执行同一 response 的 tool calls
        }

        let had_tool_calls = step.has_tool_calls();
        let final_results = if had_tool_calls {               // → AwaitingToolApproval
            tool_coordination::gate_calls_in_original_order(
                run, step, guard, &ctx.policy, &ctx.interaction, &ctx.cancel,
            ).await?;                                         // approval / HardPause typed continuations
            let outcomes = tool_coordination::execute_ready(
                &ctx.tool_execution, step.ready_calls(), &ctx.cancel,
            ).await;                                          // completion order is not protocol order
            let ordered = interaction::resolve_tool_suspensions_in_call_order(
                run, step.original_call_order(), outcomes,
                &ctx.interaction, &ctx.cancel,
            ).await?;                                         // one PendingInteraction at a time
            let ordered = tool_coordination::l1_reduce_in_original_order(ordered);
            for result in &ordered {
                ctx.reasoning.observe(ReasoningSignal::ToolCompleted {
                    declared_phase: result.declared_phase(),
                    is_error: result.is_error(),
                    tool_name: result.tool_name().to_owned(),
                });
            }
            run.apply_results(step, &ordered);
            ordered
        } else {
            ctx.reasoning.observe(ReasoningSignal::TextOnly);
            Vec::new()
        };

        let append = context_coordination::completed_step_append(
            run, step, request.request_id, &final_results,
        );                                                     // pending input → assistant → ordered results
        if let Err(error) = ctx.context.append_and_persist(append).await { // ContextPort ④，恰好一次
            run.fail(error.into());
            break;
        }
        run.mark_step_persisted(step);                         // 清空 pending input
        ctx.reasoning.observe(ReasoningSignal::TurnBoundary);  // commit 后才观察边界

        if had_tool_calls {
            run.resume_preparing_context();
            continue;
        }

        run.begin_finishing();
        let hook_outcome = ctx.hooks
            .dispatch(HookInvocation::Stop(run.stop_input()))
            .await;
        match hook_outcome.directive {
            HookDirective::Continue => { run.finish(); break; },
            HookDirective::Block { reason } if run.record_stop_block() <= 15 => {
                run.append_stop_feedback(reason);
                run.resume_from_finishing();
                continue;
            }
            HookDirective::Block { .. } => {
                run.fail(StopHookRetryExhausted);
                break;
            }
            _ => { run.fail(InvalidStopHookDirective); break; }
        }
    }
    Ok(())
}
```

`freeze_request` 只是字段映射 helper，**NEVER** 成为第五个 ContextPort 方法。其输出必须完整符合 [ContextRequest](../context-management/02-compact.md)：尤其是 `RunSpec.system_prompt`、Provider-resolved limits/effective reasoning、单次 Tool Catalog schema 集与 pending inputs 均按值进入 request。普通 retry 复用同一 `InvocationRequest`；只有 compact 或 capability/catalog/config 变化才回到 PreparingContext 重新冻结。

并发 Tool future 可以并发，interaction 不并发。`resolve_tool_suspensions_in_call_order` 先按 RunStep 原始 ToolCallId 序列稳定化，再逐个执行 `register PendingInteraction → await completion → clear PendingInteraction`。所有 suspension 均完成后才一次性构造 `ContextAppend`，从而保持 Provider 要求的 assistant/tool-result 邻接与原调用顺序。

**零分支保证**：`run_loop` 对 Main/Sub 完全相同——compact/policy/memory/effort/tools/stall/fuse/取消的行为差异全部封装在 `ctx`（装配的 RuntimeContext，Sub 用 NoOp/受限/独立/派生实例）与 `run.spec` 里。

### 2.1 打断协议：请求同步，完成异步

用户打断必须走单一 Runtime 入站命令；`InputBuffer` 只承载要加入 Context 的用户内容，不承载取消等控制命令：

1. TUI 调用 SDK `cancel_run(run_id)`；该调用为同步方法，不经输入队列、不等待 `.await`。
2. Runtime 在调用返回前原子地校验 active Run、迁移到 `Cancelling`、触发该 Run 的 cancellation scope，并产生 `RunCancellationRequested`。
3. Provider、Tool、Compact、Hook 及子 Run 在各自异步等待边界监听同一 scope 或其派生 scope，收到信号后立即停止继续工作。
4. Loop Engine 等待在途工作释放、回滚 partial 结果，再迁移为 `Cancelled` 并 emit `RunCancelled`。
5. TUI 发出请求后立即投影为 Cancelling；收到 `RunCancelled` 才投影为 Cancelled/Idle。

因此“马上”指**取消请求同步生效且在途 Future 立即被唤醒**；安全回滚和终态确认仍异步完成，TUI NEVER 阻塞事件循环等待取消完成。

每个 Run 独占 cancellation scope；Session NEVER 持有可替换 token 槽。父 Run 的 scope 是子 Run scope 的父级，父取消必须传播到全部活动子 Run；子 Run 自行取消不反向取消父 Run。

## 3. 输入模型统一：单 Run vs Session 多 Run 序列

关键区分——Loop Engine 只管**单个 Run** 的生命周期；"Main 常驻多轮对话"是**外层 Run 序列**：

| | 谁管 | 循环 |
|---|---|---|
| **单个 Run** | `loop_engine::run_loop` | Run 内 Run Step 循环，跑到 Completed/AwaitingUser/Failed/Cancelled |
| **Main 常驻多轮** | `agent_run` 会话循环 | `等用户输入 → start_run → Run 完成 → 等下一输入 → 新 Run`（一个 Session 内 Run 序列）|
| **Sub 单次** | 父 Run 的 tool_coordination | 派生一个子 Run，跑完回传父，无后续 |

**统一点**：Sub = 单次输入的一个 Run；Main = Session 层多个 Run 的序列，每个 Run 就是"单次输入"的特例。**Loop Engine 不感知这个区别**——它只跑一个 Run。

- `AwaitingUser`（ask_user 暂停）：同一个 Run 内暂停/resume，Run 未完成
- `Completed` 后等下一输入：Run 完成，Session 层开新 Run（不是同一 Run）

### InputBuffer（入站端口）— 支撑追问

Loop Engine 每轮在门禁点 `ctx.input.drain()` 纳入新输入，Main/Sub 靠装配的 `InputBuffer` 区分，引擎零分支：

| | InputBuffer 装配 | 行为 |
|---|---|---|
| Main | TUI 输入通道 + 忙期排队 buffer | 用户在 Run 执行中**追问** → 排队 → 下一轮门禁 drain → append 进 Context Window 带上 |
| Sub | 固定初始队列 | 首轮 drain 出 prompt，之后为空 → 自然收敛 |

- `input` 是 **RuntimeContext 的入站端口**（与出站端口同层，装配时确定）
- `result` 不是独立类型——**统一经 `EventSink`**：Run 到达终态时 agent_run 显式 emit 终态事件（`RunCompleted{ result }` / `RunFailed{ error }` / `RunCancelled`）。Main→TUI 通知完成；Sub→父 Run，父从终态事件统一提取（成功→result、失败→error）继续。**靠终态领域事件识别，不靠遍历 message**

## 4. 停止条件

| 条件 | 结果 |
|---|---|
| 无 tool_calls / stop_reason=EndTurn，Stop Hook 放行 | Finishing → Completed |
| Stop Hook 阻断（含执行失败 3 次耗尽），累计≤15 | feedback 注入 → PreparingContext，同一 Run 继续 |
| Stop Hook 阻断累计>15 | Failed(StopHookRetryExhausted) |
| timeout>0 且墙钟超时 | Failed |
| StuckGuard HardPause | AwaitingUser（Main）/ Failed（Sub，无人应答）|
| 用户打断 | Cancelling → Cancelled（同步触发 scope，异步收口确认） |
| LLM Fatal 错误 / 重试耗尽 | Failed（Retryable 先退避重试；context 超限→compact 重跑）|

> **去掉 max_turns**：不再用轮次上限，改由 `timeout`（0=无限，Main 默认 0）+ StuckGuard 双重兜底（见 `04-stuck-prevention`）。

## 5. 重试策略（LLM 错误）

`model_invocation` 对 Retryable 错误退避重试，Fatal 直接失败。**只做退避重试，不做降级 / 故障转移**（避免改变结果质量、引入 pool 依赖）。

| 层级 | 触发 | 应对 |
|---|---|---|
| **T0 即时** | 流开始前中断 / 连接瞬断，且本 attempt 无可见 delta 已提交 | 首次立即重试（瞬时抖动）|
| **T1 退避** | 超时 / 5xx / 429，且本 attempt 无可见 delta 已提交 | 指数退避 + jitter，**单次退避封顶 5 分钟**；429 尊重 `Retry-After` |
| **失败** | 重试达 **10 次** 或 Fatal(4xx) | `RunFailed{ error }` |

- **上限**：最多重试 **10 次**，单次退避封顶 **300s（5 分钟）**
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
