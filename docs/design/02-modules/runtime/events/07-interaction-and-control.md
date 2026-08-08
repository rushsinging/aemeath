# 07 · Interaction 与 Control 事件

## 1. Interaction 定位

| 属性 | 契约 |
|---|---|
| Owner | Runtime Interaction resource owner |
| Authority | request/reply 资源协议；不直接决定 Run terminal |
| Identity | `request_id`、`run_id` |
| Ordering | request identity；reply/cancel exactly-once resolution |
| Delivery | request fact + command reply outcome |

## 2. Interaction request

| Runtime | SDK | TUI | Consumer | 状态 |
|---|---|---|---|---|
| `InteractionRequested` | 同名 | TUI-owned typed request | `ShowInteraction` | Current |
| `AskUserBatch` | 同名 | `Nop` | legacy sender bridge 已退役 | Target Removal |

`InteractionRequested` payload MUST 穷举 UserQuestions、ToolApproval、PlanApproval、HardPause 等 body，并保留 Runtime 生成的 `request_id` 与 `run_id`。TUI NEVER 自生成协议 identity，也 NEVER 持有 sender/waiter。

## 3. Reply 与 cancel command

Interaction reply/cancel 是 `AgentClient` command，不是 `ChatEvent`：

```text
ReplyInteraction { request_id, reply }
CancelInteraction { request_id }
  → Accepted | Rejected | Failed
```

推荐 Published Language：

- `InteractionReplyAccepted`
- `InteractionReplyRejected`
- `InteractionCancelAccepted`
- `InteractionCancelRejected`

这些结果只解析 Interaction resource，不等价于 `RunResumed`、`RunCancelled` 或 `RunFailed`。Run 后续状态由 Lifecycle 发布。

## 4. Run control

| Command | ACK | 后续权威事实 |
|---|---|---|
| `CancelCurrentRun` | accepted / already cancelling / rejected / failed | `RunStepCancellationRequested`、`RunStepCancelled` 或 Run terminal |
| `CancelRunStep` | accepted / already cancelling / rejected / failed | typed `RunStepCancelled` |
| `TerminateRun` | accepted / already terminated / rejected / failed | `RunTerminationRequested`、`RunTerminated` |

命名约束：

- Command 使用祈使语义：`CancelRunStep`；
- ACK 使用 `<Command>Accepted/Rejected`；
- Lifecycle 使用 `<Subject>Cancelled/Terminated`；
- NEVER 把 accepted 命名为 `RunStepCancelled`；
- NEVER 从 ACK、timeout 字符串或 TUI 状态补发 terminal。

## 5. Identity 与 stale handling

1. TUI 优先携 active Main `(run_id, step_id)` 发送 `CancelRunStep`；
2. current-run fallback 仅在 identity 不可用时使用；
3. stale request id / run id / step id 必须 rejected，不命中当前资源；
4. late ACK 不得覆盖已发布的 Lifecycle terminal；
5. Interaction reply/cancel 必须只解析对应 request；
6. child Run control 不得意外终止 parent。

## 6. Interaction 与 Lifecycle

```text
InteractionRequested
  → TUI renders resource
  → reply/cancel command ACK
  → Runtime resolves resource
  → RunResumed / RunCancelling / RunFailed ...
```

中间 ACK 不跨越最后一步。TUI 本地交互块消失也不是 Runtime Lifecycle 事实。

## 7. 不变量

1. command outcome 不进入 `ChatEvent` 作为 terminal；
2. request identity 贯穿 SDK/TUI/AgentClient，无字符串重建；
3. sender/waiter 不进入 SDK DTO、TUI Model 或 Session；
4. Interaction 与 control 失败使用 `Rejected/Failed`，不滥用 Lifecycle `Failed`；
5. cancel accepted 只进入 cancelling presentation；
6. terminal cleanup 继续由 durable receipt 和 Lifecycle authority 收敛。

## 8. 变更门禁

修改 Interaction/Control 必须覆盖 command trait、request/reply DTO、identity stale case、ACK 非 terminal、后续 Lifecycle 收敛、TUI block 资源协议与 [09-event-index.md](09-event-index.md)。
