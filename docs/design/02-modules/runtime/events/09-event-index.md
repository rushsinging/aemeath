# 09 · Runtime 事件全量索引

> 本索引用于从 Runtime、SDK 或 TUI 名称反查同一事实。状态为文档迁移状态，不代表代码已完成重命名。

## 1. 字段说明

| 字段 | 含义 |
|---|---|
| Family | 唯一事实族 |
| Runtime | Runtime producer/container variant；`—` 表示 SDK/control-plane 直接来源 |
| SDK | `sdk::ChatEvent` 或 command outcome 名称 |
| TUI / Consumer | TUI-owned event 与主要动作 |
| Identity | 最小 correlation identity |
| Delivery | delta/full-state/snapshot/terminal/ACK |
| Authority | 是否能决定 Lifecycle terminal |
| 状态 | Current、Compatibility、Deprecated、Target Rename/Removal/Split |

## 2. Lifecycle

| Runtime | SDK | TUI / Consumer | Identity | Delivery | Authority | 状态 |
|---|---|---|---|---|---|---|
| `Started` | `RunStarted` | `Run::Started` | run | transition | Run 状态 | Current |
| `StepStarted` | `RunStepStarted` | `RunStep::Started` | run + step | transition | Step 状态 | Current |
| `StepCompleted` | `RunStepCompleted` | `RunStep::Completed` | run + step | terminal | Step terminal | Current |
| `StepCancellationRequested` | `RunStepCancellationRequested` | `RunStep::CancellationRequested` | run + step | transition | 非终态 | Current |
| `StepFinalizationStarted` | `RunStepFinalizationStarted` | `RunStep::FinalizationStarted` | run + step | transition | 非终态 | Current |
| `StepCancelled` | `RunStepCancelled` | `RunStep::Cancelled` → `PresentCancelledStep` | run + step | typed terminal | Step terminal | Current |
| `DrainingInput` | `RunDrainingInput` | `Run::DrainingInput` | run | transition | 非终态 | Current |
| `TerminationRequested` | `RunTerminationRequested` | `Run::TerminationRequested` | run | transition | 非终态 | Current |
| `Terminated` | `RunTerminated` | `Run::Terminated` | run | terminal | Run terminal | Current |
| `AwaitingUser` | `RunAwaitingUser` | `Run::AwaitingUser` | run | transition | 非终态 | Current |
| `Resumed` | `RunResumed` | `Run::Resumed` | run | transition | 非终态 | Current |
| `StuckDetected` | `RunStuckDetected` | `Run::Stuck` | run | diagnostic fact | 否 | Current |
| `Completed` | `RunCompleted` | `Run::Completed` | run | terminal | Run terminal | Current |
| `Failed` | `RunFailed` | `Run::Failed` | run | terminal | Run terminal | Current |
| `Transitioned` | `RunTransitioned` | `Nop` | run | observation | 否 | Compatibility |

## 3. Activity

| Runtime | SDK | TUI / Consumer | Identity | Delivery | Authority | 状态 |
|---|---|---|---|---|---|---|
| `RuntimeActivityEvent::Changed` | `ActivityChanged` | `ObserveActivityChange` | run + activity + revision | delta | 否 | Current |
| `RuntimeActivityEvent::Snapshot` | `ActivitySnapshot` | `ReplaceActivitySnapshot` | run + revision | snapshot | 否 | Current |

Activity detail 中的 `CompactOperation` 使用 typed stage/work，不建立顶层 `CompactProgress` 事件。

## 4. Published State

| Runtime | SDK | TUI / Consumer | Identity | Delivery | Authority | 状态 |
|---|---|---|---|---|---|---|
| `RuntimeStatusChanged` | 同名 | `ReplaceRuntimeStatus` | session + revision + heartbeat sequence | full-state | 状态交付；非 Lifecycle | Current |
| `TaskStateChanged` | 同名 | `ReplaceTaskState` | session + Task revision | full-state | 状态交付；非 Lifecycle | Current |
| — | — | — | session | retired text snapshot | 否 | Removed；由 `TaskStateChanged` 替代，NEVER 恢复 |

## 5. Content Stream：模型、Tool 与 Provider

| Runtime | SDK | TUI / Consumer | Identity | Delivery | Authority | 状态 / Target |
|---|---|---|---|---|---|---|
| `AssistantTextDelta` | 同名 | 同名 → `AssistantText` | block | delta | 否 | Current |
| `ThinkingDelta` | 同名 | 同名 → `ThinkingText` | block | delta | 否 | Current |
| — | `Token` | compatibility dual-read → `AssistantTextDelta` | block | legacy delta payload | 否 | Compatibility；Runtime producer removed |
| — | `Thinking` | compatibility dual-read → `ThinkingDelta` | block | legacy delta payload | 否 | Compatibility；Runtime producer removed |
| `BlockComplete` | 同名 | `CompleteBlock` | block | completion fact | 否 | Current |
| `ToolCallStarted` | 同名 | 同名 → 建立 Tool block | tool call | start fact | 否 | Current |
| — | `ToolCallStart` | compatibility dual-read → `ToolCallStarted` | tool call | legacy start fact | 否 | Compatibility；Runtime producer removed |
| `ToolCallArgumentsDelta` | 同名 | 同名 → 更新参数预览 | tool call + stream order | delta | 否 | Current |
| `ToolCallStateChanged` | 同名 | 同名 → 更新完整 args/state | tool call | full-state fact | 否 | Current |
| — | `ToolCallUpdate` | compatibility dual-read → 上述两类 TUI fact | tool call | mixed legacy payload | 否 | Compatibility；Runtime producer removed |
| `ToolResult` | 同名 | 更新 Tool result | tool call | result fact | 否 | Current |
| `ToolOutputDelta` | 同名 | 同名 → streaming output | tool call | delta | 否 | Current |
| — | `ToolProgress` | compatibility dual-read → `ToolOutputDelta` | tool call | legacy event wrapper | 否 | Compatibility；Runtime producer removed |
| — | `AgentProgress` | 第一 ACL 边界归一化为 `SubRunStarted` / `SubRunActivity` | sub/source + parent attachment | compatibility mixed input | 否 | Compatibility；Runtime producer removed |
| `SubRunStarted` | 同名 | `UpdateAgentMeta` | sub + parent + tool + sequence | fact | 否 | Current |
| `SubRunActivity` | 同名 | parent ToolCall attachment | sub + parent + tool + sequence | structured observation | 否 | Current |
| `ModelInvocationRetrying` | 同名 | retry notice | invocation + attempt | transition | 否 | Current |
| `Usage` | 同名 | usage reducer | invocation/run | 未明确 delta/cumulative | 否 | Target Clarify |
| `LiveTps` | 同名 | throughput presentation | invocation/run | observation | 否 | Target Rename |
| `ApiError` | 同名 | error presentation | run/step | error fact | 否 | Compatibility |

## 6. Content Stream：消息、Session 与 processing

| Runtime | SDK | TUI / Consumer | Identity | Delivery | Authority | 状态 / Target |
|---|---|---|---|---|---|---|
| `TurnStarted` | 同名 | message-state sync | session/run | transition | 否 | Compatibility |
| `MicrocompactCompleted` | 同名 | 同名 → message-state sync | session | completion fact | 否 | Current |
| — | `MicrocompactDone` | compatibility dual-read → `MicrocompactCompleted` | session | legacy completion fact | 否 | Compatibility；Runtime producer removed |
| `SessionMessageStateChanged` | 同名 | revisioned light state | session + revision | state change | 否 | Current |
| `UserMessagesAdopted` | 同名 | adopt echo + queue sync | session/input ids | full batch fact | 否 | Current |
| `UserMessagesQueued` | 同名 | replace queued submissions | session/input ids | full-state | 否 | Target Rename |
| `UserMessagesWithdrawn` | 同名 | clear/restore input | session/input ids | fact | 否 | Current |
| `SystemMessage` | 同名 | append system message | session/run | content fact | 否 | Target Clarify |
| `HookNotice` | 同名 | append typed notice | run/hook | notice fact | 否 | Current |
| `Done` | `Done` | `CompleteChat` | chat processing | terminal | processing only | Compatibility |
| `DoneWithDuration` | `DoneWithDurationMs` | `CompleteChat` | chat processing | terminal | processing only | Target Consolidation |
| `Cancelled` | `Cancelled` | cancelled presentation | chat processing | terminal | processing only | Compatibility |
| `CompactOperationRolledBack` | 同名 | 同名 → sync messages/clear presentation | session/operation | rollback fact | 否 | Current |
| `CompactOperationCompleted` | 同名 | 同名 → sync messages/notice | session/operation | completion fact | 否 | Current |
| — | `CompactRollback` | compatibility dual-read → `CompactOperationRolledBack` | session/operation | legacy rollback fact | 否 | Compatibility；Runtime producer removed |
| — | `CompactFinished` | compatibility dual-read → `CompactOperationCompleted` | session/operation | legacy completion fact | 否 | Compatibility；Runtime producer removed |
| `SessionResumed` | 同名 | restore conversation | session | snapshot/replay | 否 | Current |
| `SessionResumeFailed` | 同名 | diagnostic + empty recovery | session | failure fact | 否 | Current |
| `SessionReset` | 同名 | reset epoch | session | reset | 否 | Current |
| `RunChanged` | `CurrentRunChanged` | no-op observation | run | observation | 否 | Compatibility |

## 7. Interaction 与 Control

| Runtime / Command | SDK / Outcome | TUI / Consumer | Identity | Delivery | Authority | 状态 |
|---|---|---|---|---|---|---|
| `InteractionRequested` | 同名 | `ShowInteraction` | request + run | request fact | Interaction resource | Current |
| — | — | — | legacy sender | retired transport | 否 | Removed；由 `InteractionRequested` 替代，NEVER 恢复 |
| `ReplyInteraction` command | accepted/rejected/failed | resolve local resource | request | ACK | 非 Run terminal | Current；目标使用完整命名 |
| `CancelInteraction` command | accepted/rejected/failed | cancel local resource | request | ACK | 非 Run terminal | Current；目标使用完整命名 |
| `CancelCurrentRun` command | accepted/rejected/failed | cancelling presentation | run | ACK | 否 | Compatibility fallback |
| `CancelRunStep` command | accepted/rejected/failed | cancelling presentation | run + step | ACK | 否 | Current |
| `TerminateRun` command | accepted/rejected/failed | terminating presentation | run | ACK | 否 | Current |

## 8. Catalog、Config 与 Query Result

| Runtime / Source | SDK | TUI / Consumer | Identity | Delivery | Authority | 状态 / Target |
|---|---|---|---|---|---|---|
| config control-plane | `ConfigChanged` | replace config view | revision | full-state | Config | Current |
| `ConfigReloaded` | 同名 | replace config + notice | revision | full-state | Config | Current / Clarify |
| `WorkingDirectoryChanged` | 同名 | `WorkspaceSnapshot` | workspace revision | snapshot | Workspace | Target Rename：`WorkspaceChanged` |
| `SkillsUpdated` | 同名 | replace skill catalog | catalog revision | full-state | Skill Catalog | Target Rename：`SkillCatalogChanged` |
| `ModelSwitched` | 同名 | model presentation | command/current config | mixed | Config | Target Split/Rename |
| `ThinkingChanged` | 同名 | config + presentation | config revision | state fact | Config | Target Rename |
| `ContextEstimated` | 同名 | estimate presentation | model/config | observation | 否 | Target Clarify |
| `ReflectionHistory` | 同名 | safe history view | query | snapshot response | 否 | Target Rename |
| `ModelList` | 同名 | model catalog | query/revision | snapshot | Catalog | Target Rename |
| `ReminderList` | 同名 | reminder catalog | query/revision | snapshot | Catalog | Target Rename |
| `SessionList` | 同名 | session catalog | query/revision | snapshot | Catalog | Target Rename |
| `ProjectInfo` | 同名 | project projection | workspace/project | snapshot | Workspace | Target Rename/Merge |
| `CostUpdate` | 同名 | cost presentation | run/session | 未明确 delta/cumulative | 否 | Target Clarify/Rename |
| `CommandResultText` | 同名 | system/error message | command | string result | 否 | Compatibility |
| legacy ChatInput | `Result` | command result text | command | string result | 否 | Deprecated |

## 9. 已退役事件

| 名称 | 原问题 | 替代事实 | 状态 |
|---|---|---|---|
| stringly `CompactProgress { stage, current, total }` | stage 类型擦除；scheduled work 被误作 completed；TUI 镜像竞争 | Activity typed `CompactOperation` stage/work | Removed；NEVER 恢复 |

## 10. 命名迁移清单

以下仅冻结目标，不表示代码已改名：

| 当前名称 | 问题类别 | 目标方向 | 迁移等级 |
|---|---|---|---|
| `Token` | Subject/Delta 缺失 | 已由 `AssistantTextDelta` 替代生产发布；SDK 旧 variant 仅 compatibility dual-read | Public SDK compatibility；Runtime/TUI internal migration complete |
| `Thinking` | Delta 缺失 | 已由 `ThinkingDelta` 替代生产发布；SDK 旧 variant 仅 compatibility dual-read | Public SDK compatibility；Runtime/TUI internal migration complete |
| `ToolCallStart` | 非事实后缀 | 已由 `ToolCallStarted` 替代生产发布；SDK 旧 variant 仅 compatibility dual-read | Public SDK compatibility；Runtime/TUI internal migration complete |
| `ToolCallUpdate` | args delta 与 state fact 淡化 | 已拆为 `ToolCallArgumentsDelta` / `ToolCallStateChanged`；SDK 旧 variant 仅 compatibility dual-read | Public SDK compatibility；Runtime/TUI internal split complete |
| `ToolProgress` | Subject/Delta 不清 | 已由 `ToolOutputDelta { delta }` 替代生产发布；SDK 旧 variant 仅 compatibility dual-read | Public SDK compatibility；Runtime/TUI internal migration complete |
| `Done` / `DoneWithDurationMs` | 宽泛且重复 | 明确 Chat processing subject 后合并 | 语义冲突 |
| `MicrocompactDone` | 禁用 `Done` | 已由 `MicrocompactCompleted` 替代生产发布；SDK 旧 variant 仅 compatibility dual-read | Public SDK compatibility；Runtime/TUI internal migration complete |
| `CompactFinished` | Subject/Fact 不统一 | 已由 `CompactOperationCompleted` 替代生产发布；SDK 旧 variant 仅 compatibility dual-read | Public SDK compatibility；Runtime/TUI internal migration complete |
| `CompactRollback` | 缺 operation subject/过去式 | 已由 `CompactOperationRolledBack` 替代生产发布；SDK 旧 variant 仅 compatibility dual-read | Public SDK compatibility；Runtime/TUI internal migration complete |
| `TasksSnapshot` | 与 `TaskStateChanged` 双路径 | 已从 Runtime/SDK/TUI transport 物理退役；由 revisioned `TaskStateChanged` 取代 | Removed；Guard 禁止恢复 |
| `AskUserBatch` | sender 穿透 Published Language | 已从 Runtime/SDK/TUI transport 物理退役；由纯值 `InteractionRequested` 与 command reply 取代 | Removed；Guard 禁止恢复 |
| `SkillsUpdated` | plural + Updated | `SkillCatalogChanged` | 跨层 wire |
| `ModelList` | query response 与 event 混淆 | `ModelCatalogSnapshot/Changed` | 语义澄清 |
| `ReminderList` | 同上 | `ReminderCatalogSnapshot/Changed` | 语义澄清 |
| `SessionList` | 同上 | `SessionCatalogSnapshot/Changed` | 语义澄清 |
| `ModelSwitched` | ACK 与 state fact 混合风险 | 拆 command ACK 与 `ModelChanged` | 语义冲突 |
| `WorkingDirectoryChanged` | Subject 不稳定 | `WorkspaceChanged` | 跨层 wire |
| `Result` / `CommandResultText` | stringly 通用通道 | typed command outcome/fact | Public compatibility |
| `LiveTps` | UI 指标缩写 | `ModelThroughputChanged` 候选 | 跨层 wire |
| `CostUpdate` | Updated + delta 不明 | `CostChanged` 并声明 delivery | 语义澄清 |

## 11. 索引维护规则

1. 新事件不得只加到 Rust enum；MUST 同时登记本索引。
2. 一个 Runtime/SDK/TUI 名称只能映射到一个业务事实，除非状态明确为 Compatibility。
3. 迁移完成后更新状态并删除旧名称；不得永久保留“Target Rename”。
4. 删除事件前提供 producer、mapper、consumer、schema 和测试无引用证据。
5. 索引与源码不一致时，代码 review MUST 阻断并回到 owner/authority/identity/delivery 核验。
