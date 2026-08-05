# Runtime 事件管线与 Published Language

> 层级：02-modules / runtime（模块战术设计）
> 状态：Current + Target 契约｜Milestone：v0.1.0
> 本文是 Runtime 对外事件事实、SDK Published Language 与 TUI ACL 消费方式的权威目录。事件新增、重命名、字段变更、终态语义或消费方式变化时，MUST 同步更新本文。

## 1. 文档归属与边界

全链路事件契约归 Runtime 模块所有，因为事件表达的业务事实、identity、权威性和发布时机均由 Runtime 决定。SDK 负责稳定 Published Language，TUI 负责防腐转换、事实镜像与展示投影。

本文覆盖三条职责分离的生产发布管线；三者只在 SDK `ChatEvent` 传输语言汇合：

```text
Run 聚合
  → RuntimeLifecycleEvent
  → sdk_event_mapper::map_lifecycle_event
  → sdk::ChatEvent
  → TUI event_mapping
  → TuiRuntimeEvent
  → AgentEventMapping / App 级消费
  → reducer / Model / View

Runtime 应用编排、Provider、Tool、Session 服务
  → RuntimeStreamEvent
  → sdk_event_mapper::map_stream_event
  → sdk::ChatEvent
  → 同一 TUI 管线

ActivityCoordinator
  → RuntimeActivityEvent
  → sdk_event_mapper::map_activity_event
  → sdk::ChatEvent
  → 同一 TUI 管线
```

本文不把以下对象混作 Runtime event：

- `CancelCurrentRunOutcome`、`CancelRunStepOutcome`、`TerminateRunOutcome` 和 interaction command outcome 是同步 command ACK，只表达 accepted / rejected / failed；它们不是 `ChatEvent`，NEVER 成为 terminal source。
- Activity 是 Runtime lifecycle 的 observational projection；它可修复展示镜像，但 NEVER 决定 Run、Run Step 或 processing 终态。
- Context durable Step / ToolCall receipt 是恢复与 cancellation terminal 的事实输入，不是 UI event；Runtime 从 receipt 派生权威 lifecycle event 后才发布。

## 2. 事件源与权威性

| 事件源 | 代码类型 | 权威性 | SDK 出口 | TUI 责任 |
|---|---|---|---|---|
| Run 聚合生命周期 | `RuntimeLifecycleEvent` | Run / Run Step 状态与终态的唯一权威事件源 | `map_lifecycle_event` | 按 identity 投影；不得由 ACK、Activity 或 turn 兼容事件覆盖 |
| Chat/Provider/Tool/Session 流 | `RuntimeStreamEvent` | 内容、进度、命令结果和 session 投影的 Runtime-owned 流 | `map_stream_event` | ACL 转换并更新对应 Model；仅明确 terminal 事件可结束 processing |
| Activity 观察流 | `RuntimeActivityEvent::{Changed, Snapshot}` | observational | `map_activity_event` | 维护 revision 化事实镜像与摘要，不驱动 terminal |
| 控制 ACK | client / control trait 返回值 | 非事件、非终态 | 不进入 `ChatEvent` | 只更新 cancelling/rejected/failed presentation |
| Resume | `SessionResumed` + durable finalized Step | 已持久化事实的重建投影 | SDK `SessionResumed` | 通过同一 TUI Model/reducer 语义还原历史，不伪造 live terminal |

## 3. RuntimeLifecycleEvent 全链路矩阵

`RuntimeLifecycleEvent` 定义于 `agent/features/runtime/src/domain/agent_run/event.rs`，通过 `agent/features/runtime/src/adapters/sdk_event_mapper.rs::map_lifecycle_event` 一对一进入 SDK。该名称表达 Runtime 执行生命周期职责，NEVER 以宽泛的 `DomainEvent` 技术分类替代。除表中注明外，TUI 第一层 ACL 将 SDK 事件包装为带 `UiRunId` / `UiRunStepId` 的 `TuiRuntimeEvent::{Run, RunStep}`。

| Runtime lifecycle event | SDK `ChatEvent` | TUI 第一层 | TUI reducer / 展示 | 权威与终态规则 |
|---|---|---|---|---|
| `Started` | `RunStarted` | `Run { Started }` | lifecycle 观察；Main/Sub 按 `parent_run_id` 区分 | Run 已激活；非终态 |
| `StepStarted` | `RunStepStarted` | `RunStep { Started }` | Main Step 缓存 active `(run_id, step_id)`；其他投影 no-op | active Main identity 的唯一建立点 |
| `StepCompleted` | `RunStepCompleted` | `RunStep { Completed }` | 当前无可见 Intent | Step 正常完成事实；不等于 Chat processing terminal |
| `StepCancellationRequested` | `RunStepCancellationRequested` | `RunStep { CancellationRequested }` | 当前无可见 Intent | 请求已进入 Runtime；非终态 |
| `StepFinalizationStarted` | `RunStepFinalizationStarted` | `RunStep { FinalizationStarted }` | 当前无可见 Intent | cleanup/finalization 进行中；非终态 |
| `StepCancelled { terminal }` | `RunStepCancelled { terminal }` | `RunStep { Cancelled { terminal } }` | 仅 Main Step 产生 `PresentCancelledStep`；最后展示边界把 typed terminal 转换为 confirmed presentation | Step cancellation 唯一权威 terminal；`Cancelled` 与 `CancellationUnconfirmed` 由 durable receipts 决定，互斥且 exactly-once |
| `DrainingInput` | `RunDrainingInput` | `Run { DrainingInput }` | 当前 lifecycle mapper no-op | `CancelRunStep` 后 Run 可继续 drain/seal；非终态 |
| `TerminationRequested` | `RunTerminationRequested` | `Run { TerminationRequested }` | 当前 lifecycle mapper no-op | whole-Run termination 已接纳；非终态 |
| `Terminated` | `RunTerminated` | `Run { Terminated }` | 当前 lifecycle mapper no-op；derived-run observer 可转 `AgentRunTerminal::Cancelled` | Run termination 唯一权威 terminal |
| `AwaitingUser` | `RunAwaitingUser` | `Run { AwaitingUser }` | 当前 lifecycle mapper no-op | interaction wait 状态；SDK 不暴露 request id，request 由独立 `InteractionRequested` 携带 |
| `Resumed` | `RunResumed` | `Run { Resumed }` | 当前 lifecycle mapper no-op | interaction continuation 已恢复；非终态 |
| `StuckDetected` | `RunStuckDetected` | `Run { Stuck }` | 当前 lifecycle mapper no-op；诊断展示由 interaction/activity 补充 | 诊断事实，非 terminal |
| `Completed { result, user_cancelled_step }` | `RunCompleted { result }` | `Run { Completed }` | 当前 lifecycle mapper no-op | Run 正常 seal 终态；`user_cancelled_step` 当前不进入 SDK，用户可见 Step 取消依赖此前 typed `RunStepCancelled` |
| `Failed { error }` | `RunFailed { error }` | `Run { Failed }` | 当前 lifecycle mapper no-op | Run 失败权威 terminal |
| `Transitioned { to, timing, ... }` | `RunTransitioned { status, timing }` | `Noop` | 明确丢弃，不进入 Model | Activity 已承载当前状态摘要；domain transition 仍供 SDK/其他 consumer 观察，TUI 不以其建立第二状态源 |

### 3.1 Run 事件的并行观察路径

Runtime 在发布 lifecycle event 时还会由 `ActivityRunObserver` 更新 Activity 树；这是并行投影，不是串行 terminal authority：

```text
RuntimeLifecycleEvent ──▶ SDK ChatEvent ──▶ TUI lifecycle / terminal projection
             └────────▶ ActivityCoordinator ──▶ RuntimeActivityEvent ──▶ SDK/TUI activity mirror
```

Activity 更新失败或 revision gap 不得改变 domain terminal；TUI 应等待 snapshot 修复 activity mirror，而不是从 activity 反推出 terminal。

## 4. RuntimeActivityEvent 与 RuntimeStreamEvent 全链路矩阵

Activity 不属于内容流。`RuntimeActivityEvent` 与 `RuntimeStreamEvent` 均定义于 `agent/features/runtime/src/application/loop_engine/chat/events.rs`，但分别经 `map_activity_event` 与 `map_stream_event` 发布。`ChatEventSink::send_activity_event` 保持 Activity 在 Runtime 内部的独立类型边界；SDK 为传输便利才将三类事件汇合为 `ChatEvent`。

表中“直接”表示 SDK 与 TUI 仅做无损 DTO 转换；“App 级”表示 `App::update_runtime_event` 在 ACL/reducer 前后执行明确的交付层协调。

### 4.1 Activity、模型内容与工具流

| Runtime event | SDK `ChatEvent` | TUI event | TUI 处理 |
|---|---|---|---|
| `RuntimeActivityEvent::Changed` | `ActivityChanged` | `ActivityChanged` | `ObserveActivityChange`；按 revision 增量更新事实镜像 |
| `RuntimeActivityEvent::Snapshot` | `ActivitySnapshot` | `ActivitySnapshot` | `ReplaceActivitySnapshot`；修复 revision gap |
| `Text` | `Token` | `Text` | `AssistantText`，非空内容重置模型静默计时 |
| `Thinking` | `Thinking` | `Thinking` | `ThinkingText`，非空内容重置模型静默计时 |
| `BlockComplete` | `BlockComplete` | `BlockComplete` | `CompleteBlock` |
| `ToolCallStart` | `ToolCallStart` | `ToolCallStart` | 建立 tool block；算作模型活动 |
| `ToolCallUpdate` | `ToolCallUpdate` | `ToolCallUpdate` | 更新参数和 pending/ready/running 状态；展示前 sanitize |
| `ToolResult` | `ToolResult` | `ToolResult` | 更新 tool result、error 和 image count；输出/content 在 ACL sanitize |
| `ModelInvocationRetrying` | `ModelInvocationRetrying` | 同名 | 追加 retry 系统提示；非 terminal |
| `Usage` | `Usage` | `Usage` | 更新 token usage，并可派生 TPS |
| `LiveTps` | `LiveTps` | `LiveTps` | 更新实时 TPS |
| `AgentProgress` | `AgentProgress` | `AgentProgress` | 保留 source/attachment/tool identity；Started 更新 agent meta，其余形成进度文本；内部 `ToolOutput` 不重复进入 timeline |
| `ToolProgress` | `ToolProgress` | `ToolProgress` | 记录顶层工具 streaming stdout |
| `ChildRunActivity` | `ChildRunActivity` | `ChildRunActivity` | 按 child identity + sequence 挂到父 ToolCall；其 Terminal 只结束 child activity，不结束 Main Run |

### 4.2 Chat、消息同步与终止兼容事件

| Runtime stream event | SDK `ChatEvent` | TUI event | TUI 处理与约束 |
|---|---|---|---|
| `TurnStarted` | `TurnStarted` | `TurnStarted` | 同步 message count、标记输出 dirty；不建立 Run terminal |
| `MicrocompactDone` | `MicrocompactDone` | 同名 | 同步 message count；不停止 spinner |
| `SessionMessageStateChanged` | 同名 | 同名 | 按 revision 更新轻量消息状态 |
| `UserMessagesAdopted` | 同名 | 同名 | App 级清 queued echo、回显正式 user/skill/hook 内容，再同步队列 |
| `UserMessagesQueued` | 同名 | 同名 | 全量替换 queued submissions |
| `UserMessagesWithdrawn` | 同名 | 同名 | 清全部 queued submissions；App 恢复输入文本 |
| `SystemMessage` | `SystemMessage` | `SystemMessage` | ACL 剥离 reminder envelope 后丢弃空 payload，否则追加系统消息 |
| `HookNotice` | `HookNotice` | `HookNotice` | 追加 typed hook notice |
| `ApiError` | `ApiError` | `ApiError` | 同步 messages、追加错误；该错误流不代替 `RunFailed` domain terminal |
| `Done` | `Done` | `Done { duration_ms: None }` | `CompleteChat` + completed notice；App 清 active identity、停止 processing |
| `DoneWithDuration` | `DoneWithDurationMs` | `Done { duration_ms: Some }` | 同上并展示耗时 |
| `Cancelled` | `Cancelled` | `Cancelled` | compatibility turn terminal：`CompleteChat` + user-cancel notice，App 停止 processing；不得覆盖 typed Step terminal，也不得由 ACK 合成 |
| `RunStarted` | `RunStarted` | `Run { Started }` | compatibility stream route；与 domain Started 同 vocabulary，不应成为第二个独立状态模型 |
| `RunChanged` | `CurrentRunChanged` | `RunChanged` | 当前 reducer no-op；保留兼容观测 |

`Done` / `Cancelled` 是 Chat processing 边界，`RunCompleted` / `RunFailed` / `RunTerminated` 是 Run domain terminal，`RunStepCancelled` 是 Step terminal。三者作用域不同，NEVER 互相改名或由下游推导补发。

### 4.3 Compact 与恢复

| Runtime stream event | SDK `ChatEvent` | TUI event | TUI 处理 |
|---|---|---|---|
| `CompactRollback` | `CompactRollback` | 同名 | 同步 messages，清 compact runtime presentation |
| `CompactFinished` | `CompactFinished` | 同名 | 同步 messages、追加 Runtime-owned notice，清 compact presentation |
| `CompactProgress` | `CompactProgress` | 同名字符串 stage | App/ACL 当前不写业务 Model；可由交付层展示进度 |
| `SessionResumed` | `SessionResumed` | `SessionResumed` | App 级 `resume_session_messages` 重建 conversation/history；保留每 Step 的 `run_id`、`step_id`、`finalize_cause`、duration |
| `SessionResumeFailed` | 同名 | 同名 | 追加错误并恢复空 session 路径 |
| `SessionReset` | `SessionReset` | `SessionReset` | 发出 `ResetRuntimeState` effect；清 Runtime/TUI session 镜像 |

Live 与 Resume 的等价边界：Live 使用 typed `RunStepCancelled` 表达 cancellation terminal；Resume 使用 finalized Step 的 `finalize_cause` 与 durable tool results 重建历史。两者必须共同区分正常完成、用户取消 Step 与 Run termination，且不得把 `CancellationUnconfirmed` tool result 降级为 generic error。

### 4.4 Runtime 命令结果与目录快照

| Runtime stream event | SDK `ChatEvent` | TUI event | TUI 处理 |
|---|---|---|---|
| `SkillsUpdated` | `SkillsUpdated` | `SkillsUpdated` | App 级原子替换 skill completion catalog；mapper no-op，避免重复写 Model |
| `WorkingDirectoryChanged` | 同名 | `WorkspaceSnapshot` | `WorkspaceIntent::ApplySnapshot` |
| `ConfigReloaded` | `ConfigReloaded` | `ConfigReloaded` | App 更新 config view；追加 reload notice并更新 UI preference |
| `ModelSwitched` | 同名 | 同名 | App 级模型状态处理；通用 mapper no-op |
| `ThinkingChanged` | 同名 | 同名 | App 级 reasoning 状态处理；通用 mapper no-op |
| `ContextEstimated` | 同名 | 同名 | App 级估算展示；通用 mapper no-op |
| `CommandResultText` | 同名 | 同名 | 成功追加 system message，失败追加 error |
| `ReflectionHistory` | 同名 | 同名 | App 级 reflection history 展示；通用 mapper no-op |
| `ModelList` | 同名 | 同名 | App 级模型列表；通用 mapper no-op |
| `ReminderList` | 同名 | 同名 | App 级提醒列表；通用 mapper no-op |
| `SessionList` | 同名 | 同名 | App 级会话列表；通用 mapper no-op |
| `ProjectInfo` | 同名 | 同名 | App 级项目信息；通用 mapper no-op |
| `TasksSnapshot` | 同名 | 同名 | `UpdateTaskLines` |
| `CostUpdate` | 同名 | 同名 | App 级成本投影；通用 mapper no-op |

### 4.5 Interaction transport

| Runtime stream event | SDK `ChatEvent` | TUI event | TUI 处理 |
|---|---|---|---|
| `InteractionRequested` | `InteractionRequested` | TUI-owned typed request | App 构建 inline user-question UI；ACL 产生 `ShowInteraction`；reply/cancel 仅携 request identity 回 Runtime |
| `AskUserBatch` | `AskUserBatch` | `SdkEventMapping::Nop` | legacy sender bridge 已从 TUI production 消费路径退休；不得恢复 sender 进入 Model |

Interaction reply/cancel outcome 是 command ACK，不在本表中产生 terminal。只有 Runtime 后续 lifecycle、tool result 或 Run event 可结束对应等待与 processing。

## 5. SDK 中非 RuntimeStreamEvent 直接来源的事件

SDK `ChatEvent` 还包含由其他 Runtime-owned adapter/control-plane 路径发布或保留的变体：

| SDK event | Runtime 来源 | TUI 处理 | 说明 |
|---|---|---|---|
| `ConfigChanged` | config control-plane | `ConfigChanged`；更新 config view 与 markdown spacing | 不由 `RuntimeStreamEvent` mapper 产生 |
| `CurrentRunChanged` | `RuntimeStreamEvent::RunChanged` | `RunChanged` no-op | `ChatEvent::RunChanged` 变体也仍存在，TUI 合并消费两者；前者是当前生产映射 |
| `Result` | 旧 ChatInput 兼容结果 | `CommandResultText` | compatibility Published Language；新 Runtime command 应使用 typed event |
| `RunStep*`、`RunDrainingInput`、`RunTerminationRequested`、`RunTerminated`、`RunCompleted`、`RunFailed`、`RunStuckDetected`、`RunTransitioned`、`RunAwaitingUser`、`RunResumed` | `RuntimeLifecycleEvent` | 见 §3 | 不属于 `RuntimeStreamEvent` 主体 |
| `ActivityChanged`、`ActivitySnapshot` | `RuntimeActivityEvent` | Activity ACL/reducer | 不属于 `RuntimeStreamEvent` 主体 |

## 6. TUI 消费层级与明确丢弃

当前生产 TUI 链路是：

```text
sdk::ChatEvent
  → sdk_event_to_tui_event
  → SdkEventMapping::Runtime(TuiRuntimeEvent) / Nop
  → processing runtime channel
  → TuiMsg::Runtime / RuntimeBatch
  → App::update_runtime_event
  → map_runtime_event
  → AgentEventMapping
  → root_reducer
```

`UiEvent` / `map_agent_event` 仍服务本地 UI 事件与部分兼容测试路径，但 SDK Runtime stream 的生产入口已直接转换为 `TuiRuntimeEvent`。设计与代码评审不得把旧 `sdk::ChatEvent → UiEvent` 描述当作唯一现状。

明确 no-op / App 级消费不是遗漏：

| TUI event | 原因 |
|---|---|
| `Noop`（来自 `RunTransitioned`） | 防止 Activity 与 Run status 双写展示状态 |
| `SkillsUpdated` mapper no-op | App 级原子替换 catalog |
| `RunChanged` | compatibility observation，当前无 Model owner |
| `Run { ... }` | 当前 domain lifecycle 主要供 identity/诊断与其他 SDK consumer；TUI 不建立第二套 Run 状态机 |
| 非 Main `RunStepCancelled` | Main presentation 不能被 child terminal 覆盖；child 由 ChildRunActivity/父 ToolCall 投影 |
| `CompactProgress`、模型/列表/成本等 mapper no-op | 由 `update_runtime_event` 的专用 App 级分支消费或保留交付层 DTO |
| legacy `AskUserBatch` | sender transport 退休，第一层直接返回 `SdkEventMapping::Nop` |

## 7. Identity、终态和 exactly-once 规则

1. `RunId` 标识 Run，`RunStepId` 标识 Run 内 Step；TUI 仅以 `parent_run_id == None` 的 `RunStepStarted` 更新 active Main identity。
2. Main/Sub 事件共享同一 Published Language；不得通过不同 enum 或 adapter 改变终态词汇。
3. `RunStepCancelled.terminal` MUST 保持 typed：`Cancelled | CancellationUnconfirmed`；SDK/TUI transport NEVER 恢复 `confirmed: bool`。
4. Step cancellation terminal 只由 Runtime 从同一 durable receipt set 派生并发布一次；TUI 不从 ACK、Activity、tool result 文本或 timeout 字符串推断。
5. `Done` / `Cancelled` 只结束 Chat processing；Step / Run terminal 仍由各自 lifecycle event 表达。
6. Parent → child cancellation 单向传播；child terminal 不回写或终止 parent，除非 parent 自己的 Runtime state machine 基于 tool result 作出后续转移。
7. 同一 identity 的重复 observational event 可幂等；陈旧 identity、乱序 ACK 或旧 activity revision 不得回滚已接纳的权威 terminal。

## 8. 变更门禁

新增或修改 Runtime 对外事件时 MUST：

1. 在 `RuntimeLifecycleEvent`、`RuntimeActivityEvent` 或 `RuntimeStreamEvent` 中明确事件所属事实源，NEVER 在多个事件族重复定义同一 terminal；Activity NEVER 放回 `RuntimeStreamEvent`。
2. 同步 Runtime → SDK mapper，并由穷尽 match 保证编译期覆盖。
3. 同步 SDK schema / wire registry（若该 DTO 可出站）。
4. 同步 TUI 第一层结构转换、TUI-owned DTO、第二层语义映射或明确 App 级消费。
5. 为跨层链路的每一层补测试，至少覆盖 Runtime emit、SDK mapping、TUI ACL 和 reducer/model；NEVER 只测首尾。
6. 若事件被有意丢弃，必须在本文“明确丢弃”表登记原因，并使用 `Nop` 或显式 match，禁止 wildcard 静默吞掉。
7. 修改 terminal、identity、Live/Resume 或 ACK 边界时，必须运行完整架构 Guards，并确认 Runtime 仍是唯一 terminal authority。

## 9. 源码索引

| 层 | 权威文件 |
|---|---|
| Runtime lifecycle events | `agent/features/runtime/src/domain/agent_run/event.rs` |
| Runtime Activity / stream events | `agent/features/runtime/src/application/loop_engine/chat/events.rs` |
| Runtime → SDK mapping | `agent/features/runtime/src/adapters/sdk_event_mapper.rs` |
| SDK Published Language | `packages/sdk/src/chat_event.rs`、`packages/sdk/src/activity.rs`、`packages/sdk/src/run.rs` |
| SDK → TUI structural ACL | `apps/cli/src/tui/adapter/event_mapping.rs` |
| TUI-owned runtime DTO | `apps/cli/src/tui/adapter/tui_runtime_event.rs` |
| TUI semantic ACL | `apps/cli/src/tui/adapter/agent_event.rs` |
| App-level event consumption | `apps/cli/src/tui/app/update.rs` |
| SDK stream delivery | `apps/cli/src/tui/effect/session/processing.rs` |

## 修改历史

| 日期 | 变更 |
|---|---|
| 2026-08-05 | 首次建立 Runtime lifecycle/activity/stream → SDK → TUI 全链路事件目录，明确三类事件族、terminal authority、Activity observational、ACK 非 terminal 与 Live/Resume 边界。 |
