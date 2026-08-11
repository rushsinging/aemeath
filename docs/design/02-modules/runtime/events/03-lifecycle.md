# 03 · Lifecycle 事件

## 1. 定位

| 属性 | 契约 |
|---|---|
| Owner | Run 聚合 |
| Authority | Run / Run Step 状态与终态唯一权威 |
| Identity | `run_id`、`step_id`、`parent_run_id` |
| Ordering | 聚合事件顺序；terminal exactly-once |
| Delivery | transition fact / terminal fact |
| Runtime 容器 | `RuntimeLifecycleEvent` |
| SDK mapper | `map_lifecycle_event` |

Lifecycle 事实不由 ACK、Activity、Tool Result 文本或 TUI presentation 推断。

## 2. 全链路矩阵

| Runtime variant | SDK 名称 | TUI 结构事件 | Authority / Consumer | 状态 |
|---|---|---|---|---|
| `Started` | `RunStarted` | `Run::Started` | Run 已激活；非终态 | Current |
| `StepStarted` | `RunStepStarted` | `RunStep::Started` | 建立 active Main `(run_id, step_id)` | Current |
| `StepCompleted` | `RunStepCompleted` | `RunStep::Completed` | Step 正常完成；不等于 Chat processing terminal | Current |
| `StepCancellationRequested` | `RunStepCancellationRequested` | `RunStep::CancellationRequested` | cancel 已进入 Runtime；非终态 | Current |
| `StepFinalizationStarted` | `RunStepFinalizationStarted` | `RunStep::FinalizationStarted` | cleanup/finalization 进行中 | Current |
| `StepCancelled { terminal }` | `RunStepCancelled` | `RunStep::Cancelled` | Step cancellation 唯一 terminal；Main 可 `PresentCancelledStep` | Current |
| `DrainingInput` | `RunDrainingInput` | `Run::DrainingInput` | Run drain/seal；非终态 | Current |
| `TerminationRequested` | `RunTerminationRequested` | `Run::TerminationRequested` | whole-Run termination 已接纳；非终态 | Current |
| `Terminated` | `RunTerminated` | `Run::Terminated` | Run termination terminal | Current |
| `AwaitingUser` | `RunAwaitingUser` | `Run::AwaitingUser` | interaction wait；非终态 | Current |
| `Resumed` | `RunResumed` | `Run::Resumed` | interaction continuation；非终态 | Current |
| `StuckDetected` | `RunStuckDetected` | `Run::Stuck` | 诊断事实；非终态 | Current |
| `Completed` | `RunCompleted` | `Run::Completed` | Run 正常 terminal | Current |
| `Failed` | `RunFailed` | `Run::Failed` | Run 失败 terminal | Current |
| `Transitioned` | `RunTransitioned` | `Noop` | SDK 观察；TUI 不建立第二状态源 | Compatibility |

Runtime 内部 variant 可因 enum 上下文省略 `Run`，SDK Published Language MUST 使用完整 Subject。

## 3. Cancellation terminal

`RunStepCancelled.terminal` MUST 为封闭类型：

```text
Cancelled
CancellationUnconfirmed
```

规则：

1. 两者由同一 durable Step receipt set 决定，互斥且 exactly-once；
2. NEVER 恢复 `confirmed: bool`；
3. `CancelRunStepAccepted` 不是 `RunStepCancelled`；
4. completed tool results 保留，缺失结果使用 typed cancellation result；
5. Live 由该事件投影，Resume 由 finalized Step `finalize_cause` 重建，两者语义必须等价。

## 4. Lifecycle 与 Activity

```text
RuntimeLifecycleEvent ──▶ SDK/TUI lifecycle projection
              └────────▶ ActivityRunObserver ──▶ Activity facts
```

第二条路径是并行 observation。Activity 更新失败、乱序或 gap NEVER 改变 Lifecycle terminal。

## 5. Lifecycle 与 processing

- `RunCompleted/RunFailed/RunTerminated`：Run domain terminal；
- `RunStepCompleted/RunStepCancelled`：Step terminal；
- `Done/Cancelled`：历史 Chat processing 边界。

三者作用域不同。后续迁移 MAY 退役 compatibility processing terminal，但 NEVER 通过重命名把它伪装成 Run/Step fact。

## 6. 不变量

1. Main 与 derived Run 使用同一 Lifecycle vocabulary；
2. child terminal 不回写 parent terminal；
3. stale identity、ACK 或 Activity revision 不回滚已接纳 terminal；
4. Interaction wait/resume 只改变 Run 状态，不转移 request identity owner；
5. TUI 不从 `RunTransitioned`、Activity 或 spinner 建立第二套 Run 状态机。

## 7. 变更门禁

修改 Lifecycle event 必须同步：domain event、Runtime SDK mapper、SDK DTO/schema、TUI typed mapping、Live/Resume 测试、terminal architecture guards 与 [09-event-index.md](09-event-index.md)。
