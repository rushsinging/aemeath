# 06 · Content Stream 事件

## 1. 定位

| 属性 | 契约 |
|---|---|
| Owner | Runtime loop、Provider stream 与 Tool orchestration |
| Authority | 内容与 Chat processing 边界；NEVER 代替 Run/Step Lifecycle |
| Identity | run/step、content block、tool call、child run |
| Ordering | stream order、typed sequence 或 identity correlation |
| Delivery | delta、content fact、usage fact、compatibility processing terminal |
| Runtime 容器 | 主要为 `RuntimeStreamEvent` |

## 2. 模型与 Tool 内容

| Runtime | SDK | TUI | Consumer | 状态 |
|---|---|---|---|---|
| `AssistantTextDelta` | 同名 | 同名 | `AssistantText` | Current |
| `ThinkingDelta` | 同名 | 同名 | `ThinkingText` | Current |
| — | `Token` | dual-read 后转为 `AssistantTextDelta` | SDK public wire compatibility | Compatibility；Runtime producer removed |
| — | `Thinking` | dual-read 后转为 `ThinkingDelta` | SDK public wire compatibility | Compatibility；Runtime producer removed |
| `BlockComplete` | `BlockComplete` | 同名 | `CompleteBlock` | Current |
| `ToolCallStarted` | 同名 | 同名 | 建立 Tool block | Current |
| — | `ToolCallStart` | dual-read 后转为 `ToolCallStarted` | SDK public wire compatibility | Compatibility；Runtime producer removed |
| `ToolCallArgumentsDelta` | `ToolCallArgumentsDelta` | 同名 | 按 stream order 追加参数预览 | Current |
| `ToolCallStateChanged` | `ToolCallStateChanged` | 同名 | 原子替换完整参数与状态 | Current |
| — | `ToolCallUpdate` | dual-read 后转为上述两类 TUI fact | SDK public wire compatibility | Compatibility；Runtime producer removed |
| `ToolResult` | `ToolResult` | 同名 | 关联 ToolCall，sanitize 展示 | Current；需保持 committed/result 边界 |
| `ToolProgress` | `ToolProgress` | 同名 | streaming stdout | Target Rename：`ToolOutputDelta` |
| `AgentProgress` | `AgentProgress` | 同名 | child/agent progress projection | Compatibility；需按 detail 复核 |
| `ChildRunActivity` | 同名 | 同名 | 挂到父 ToolCall | Current |

`AssistantTextDelta` 明确 Assistant Subject 与 Delta delivery，`ThinkingDelta` 明确 reasoning delta。SDK 旧 `Token` / `Thinking` 仅保留为 public wire compatibility input；Runtime production mapper 不再发布它们，TUI/CLI 在第一边界立即归一化为 typed fact。typed delta payload 统一使用 `delta` 字段。

## 3. Provider observation

| Runtime | SDK | TUI | 语义 | 状态 |
|---|---|---|---|---|
| `ModelInvocationRetrying` | 同名 | 同名 | 当前 attempt 失败，下一 attempt 已安排；非 terminal | Current |
| `Usage` | `Usage` | 同名 | 一次 invocation usage fact | Target Rename/Clarify：需声明 cumulative/delta |
| `LiveTps` | `LiveTps` | 同名 | presentation observation | Target Rename：`ModelThroughputChanged` 候选 |
| `ApiError` | `ApiError` | 同名 | Provider/API 错误展示与消息同步 | Compatibility；不得替代 `RunFailed` |

## 4. 消息与队列

| Runtime | SDK | TUI | Delivery | 状态 |
|---|---|---|---|---|
| `TurnStarted` | 同名 | 同名 | message count / legacy turn boundary | Compatibility |
| `MicrocompactDone` | 同名 | 同名 | message count | Target Rename：`MicrocompactCompleted` |
| `SessionMessageStateChanged` | 同名 | 同名 | revisioned light state | Current |
| `UserMessagesAdopted` | 同名 | 同名 | accepted inputs full batch | Current |
| `UserMessagesQueued` | 同名 | 同名 | queued submissions full replacement | Target Rename：`QueuedUserMessagesChanged` 候选 |
| `UserMessagesWithdrawn` | 同名 | 同名 | queued inputs withdrawn | Current |
| `SystemMessage` | 同名 | 同名 | system content fact | Target Rename：需区分 delta/append |
| `HookNotice` | 同名 | 同名 | typed visible notice | Current |

## 5. Chat processing compatibility terminal

| Runtime | SDK | TUI | 当前作用域 | 状态 |
|---|---|---|---|---|
| `Done` | `Done` | `Done` | Chat processing completed | Compatibility |
| `DoneWithDuration` | `DoneWithDurationMs` | `Done` | 同上并带 duration | Target Consolidation |
| `Cancelled` | `Cancelled` | `Cancelled` | Chat processing cancelled | Compatibility |

约束：

1. `Done` 的名称过宽，仅作为历史 compatibility 保留；
2. processing terminal 不覆盖 `RunCompleted/RunFailed/RunTerminated`；
3. `Cancelled` 不覆盖 typed `RunStepCancelled.terminal`；
4. ACK NEVER 合成 processing terminal；
5. 后续迁移需先明确 processing owner 与是否可由权威 Lifecycle/Session boundary 替代。

## 6. Compact 与 Session compatibility

| Runtime | SDK | TUI | 状态 |
|---|---|---|---|
| `CompactRollback` | 同名 | 同名 | Target Rename：`CompactOperationRolledBack` |
| `CompactFinished` | 同名 | 同名 | Target Rename：`CompactOperationCompleted`；当前含 message sync/presentation |
| `SessionResumed` | 同名 | 同名 | Current |
| `SessionResumeFailed` | 同名 | 同名 | Current |
| `SessionReset` | 同名 | 同名 | Current |

旧 stringly `CompactProgress` 已退役，不得重新加入 Content Stream；typed Compact progress 属 [Activity](04-activity.md)。

## 7. 不变量

1. Delta 必须保留 block/tool identity 与顺序；
2. sanitize 只改变展示 payload，不改变事实 identity；
3. Tool Result 与 typed committed side effect 是不同边界；
4. Content Stream 不决定 Run/Step terminal；
5. 空 payload 可在 TUI ACL 显式丢弃并留日志，但不能由 wildcard 静默吞掉；
6. compatibility terminal 必须有明确作用域和退出策略。

## 8. 变更门禁

修改 Content Stream 必须同步 Provider/Tool producer、SDK DTO、TUI structural/semantic ACL、content identity 测试，并在 [09-event-index.md](09-event-index.md) 标记名称与迁移状态。
