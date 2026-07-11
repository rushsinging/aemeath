# Agent Runtime · 状态机与 Loop Engine

> 层级：02-modules / runtime（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#761（S2）
> 本文定义 Run 单一状态机、统一 Loop Engine 骨架，以及"Main 常驻多轮 vs Sub 单次"的输入模型统一。**只描述目标态**；现状（`ChatLoopState` FSM 仅在 Main、Sub 无 FSM）差距记入 `03-engineering/migration-governance`。

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
  │ resume                               ▼                       ▼
AwaitingUser ◀──── ask_user ──── ExecutingTools            Completed
  (暂停,内存存活)                        │
                                        └──▶(回 PreparingContext 下一步)

  终态旁路： Failed（错误/超时/StuckHardPause） · Cancelled（用户取消）
```

### 迁移表

| 当前态 | 事件/条件 | 下一态 |
|---|---|---|
| Created | Start | PreparingContext |
| PreparingContext | needs_compaction | Compacting |
| Compacting | 回收完成 | PreparingContext |
| PreparingContext | 上下文就绪 | InvokingModel |
| InvokingModel | LLM 响应 | ApplyingResponse |
| InvokingModel | 错误/超时 | Failed |
| ApplyingResponse | 有 tool_calls | AwaitingToolApproval |
| ApplyingResponse | 无 tool_calls / EndTurn | Finishing |
| AwaitingToolApproval | 全部放行 | ExecutingTools |
| AwaitingToolApproval | 需人工确认(ask_user/approval) | AwaitingUser |
| ExecutingTools | 结果回收完 | PreparingContext（下一步）|
| AwaitingUser | resume(用户答复) | PreparingContext |
| Finishing | 收尾完成 | Completed |
| 任意 | timeout>0 且超时 / StuckHardPause | Failed |
| 任意 | 用户取消 | Cancelled |

**AwaitingUser 关键语义**：这是 **Run 内 ask_user 暂停**（Run 未完成，等特定问题答复），内存存活、不落盘；崩溃则整个 Run 从头开始（见 `05-recovery-semantics`）。**区别于**"Run 完成后 Session 等下一条全新输入"（那是 Run 序列层，见 §3）。

## 2. Loop Engine 骨架（Main/Sub 共用，零分支）

```rust
/// 驱动单个 Run 从 Created 到终态。Main/Sub 完全一致。
fn run_loop(run: &mut Run, ctx: &RuntimeContext, guard: &mut StuckGuard) {
    run.start();                                             // → PreparingContext
    loop {
        if let Some(reason) = guard.check_timeout(run) {     // L3 时间兜底（timeout=0 跳过）
            run.fail(reason); break;
        }
        if run.needs_compaction() {
            ctx.context.compact(run);                        // context_coordination
        }

        let window = ctx.context.build_window(run);          // context_coordination
        let effort = ctx.reasoning.effort(run);              // model_invocation（Sub: EffortOnly）
        let step = run.begin_step();                         // → InvokingModel

        let inv = ctx.provider.invoke(window, effort);       // model_invocation
        run.apply_response(step, inv);                       // → ApplyingResponse
        ctx.events.emit(run.drain_events());                 // event_projection

        if guard.stall(inv.text()) {                         // L1 文本重复
            run.mark_stuck(); /* soft: 喂回提示; hard: break→Failed */
        }

        if step.has_tool_calls() {                           // → AwaitingToolApproval
            for tc in step.tool_calls() {
                match guard.fuse(tc) {                        // L2 工具循环熔断
                    HardPause => { run.await_user(); return; }
                    SoftBlock(r) => { run.block_tool(tc, r); continue; }
                    Allow => {}
                }
                match ctx.policy.check(tc) {                 // interaction/policy
                    Denied  => run.cancel_tool(tc),
                    NeedAsk => { run.await_user(); return; }  // → AwaitingUser（暂停返回）
                    Allowed => {}
                }
            }
            let results = ctx.tools.execute(step.ready_calls()); // → ExecutingTools
            run.apply_results(step, results);                    // → PreparingContext（下一步）
        } else {
            run.finish(); break;                             // → Finishing → Completed
        }
    }
}
```

**零分支保证**：`run_loop` 对 Main/Sub 完全相同——compact/policy/memory/effort/tools/stall/fuse 的行为差异全部封装在 `ctx`（装配的 RuntimeContext，Sub 用 NoOp/受限/独立实例）与 `run.spec` 里。

## 3. 输入模型统一：单 Run vs Session 多 Run 序列

关键区分——Loop Engine 只管**单个 Run** 的生命周期；"Main 常驻多轮对话"是**外层 Run 序列**：

| | 谁管 | 循环 |
|---|---|---|
| **单个 Run** | `loop_engine::run_loop` | Run 内 Run Step 循环，跑到 Completed/AwaitingUser/Failed/Cancelled |
| **Main 常驻多轮** | `agent_execution` 会话循环 | `等用户输入 → start_run → Run 完成 → 等下一输入 → 新 Run`（一个 Session 内 Run 序列）|
| **Sub 单次** | 父 Run 的 tool_coordination | 派生一个子 Run，跑完回传父，无后续 |

**统一点**：Sub = 单次输入的一个 Run；Main = Session 层多个 Run 的序列，每个 Run 就是"单次输入"的特例。**Loop Engine 不感知这个区别**——它只跑一个 Run。

- `AwaitingUser`（ask_user 暂停）：同一个 Run 内暂停/resume，Run 未完成
- `Completed` 后等下一输入：Run 完成，Session 层开新 Run（不是同一 Run）

## 4. 停止条件

| 条件 | 结果 |
|---|---|
| 无 tool_calls / stop_reason=EndTurn | Finishing → Completed |
| timeout>0 且墙钟超时 | Failed |
| StuckGuard HardPause | AwaitingUser（Main）/ Failed（Sub，无人应答）|
| 用户取消 | Cancelled |
| LLM 错误 | Failed |

> **去掉 max_turns**：不再用轮次上限，改由 `timeout`（0=无限，Main 默认 0）+ StuckGuard 双重兜底（见 `04-stuck-prevention`）。

## 5. 相关文档

- 领域模型：[01-domain-model.md](01-domain-model.md)
- 模块边界：[02-module-boundaries.md](02-module-boundaries.md)
- 防 stuck：[04-stuck-prevention.md](04-stuck-prevention.md)
- 恢复语义：[05-recovery-semantics.md](05-recovery-semantics.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-11 | 初稿：Run 单状态机 + 迁移表、Loop Engine 零分支骨架、单 Run vs Session 多 Run 序列、停止条件 | #761 |
