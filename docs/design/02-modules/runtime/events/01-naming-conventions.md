# 01 · Runtime 事件命名规范

> 本文是 Runtime → SDK Published Language → TUI ACL 的统一命名真相源。

## 1. 基本语法

事件名 MUST 使用：

```text
<Subject><FactOrTransition>
```

事件描述已经发生的事实；command 描述意图；ACK 描述协议接纳结果；Intent 描述 consumer 动作。四者 NEVER 混名。

| 类型 | 推荐 | 禁止混淆 |
|---|---|---|
| Command | `CancelRunStep` | `RunStepCancelled` |
| ACK | `CancelRunStepAccepted` | `RunStepCancelled` |
| Lifecycle fact | `RunStepCancelled` | `CancelRunStepAccepted` |
| Consumer Intent | `ReplaceRuntimeStatus` | `RuntimeStatusChanged` 作为写入动作 |

事件名 NEVER 使用 `Handle*`、`Process*`、`Send*`、`Update*` 等命令式动词。

## 2. 受控 Subject 词汇

| Subject | 单一含义 | 禁止的无迁移别名 |
|---|---|---|
| `Session` | 用户会话与恢复边界 | `ChatSession`、`ConversationSession` |
| `Run` | 一次 Agent 执行实例 | `AgentExecution` event subject |
| `RunStep` | Run 内一次可终结步骤 | `Turn` 作为同义 terminal subject |
| `Activity` | Runtime observational fact | `RuntimeWork` 对外事件名 |
| `RuntimeStatus` | Runtime 发布的状态 full-state family | `StatusInfo`、`RuntimeStateUpdate` |
| `TaskState` | Task 完整状态 family | `TasksSnapshot` 作为同义 Current 名 |
| `CompactOperation` | 一次 Context compact operation | `CompactionProgress`、`SummarizationProgress` |
| `ModelInvocation` | 一次 Provider 模型调用 | `LlmCall`、`ApiCall` |
| `ToolCall` | 一次工具调用 identity | `ToolExecution` 作为同义 wire subject |
| `InteractionRequest` | Runtime 创建的交互资源 | `PromptRequest` |
| `InteractionReply` | 对交互资源的回复 | `Answer` |
| `Config` | 生效配置事实 | `SettingsInfo` |
| `Workspace` | 当前 workspace/worktree 事实 | `WorkingDirectoryInfo` |
| `SkillCatalog` | Skill 全量目录 | `SkillsInfo` |
| `ModelCatalog` | Model 全量目录 | `ModelListInfo` |

新增 Subject MUST 先证明它拥有独立 identity、状态或变化原因；不得为单个 UI 文案创建 Subject。

## 3. 受控 Fact / 后缀

| 后缀 | 语义 | Delivery 约束 |
|---|---|---|
| `Started` | 已实际进入运行 | 排队、future 创建、拿 semaphore 不算 Started |
| `Changed` | revisioned 事实发生变化 | MUST 声明 delta 或 full-state |
| `Snapshot` | 完整快照 | 用于初始化、恢复或 revision gap repair |
| `Delta` | 可拼接增量 | MUST 有 identity 和 ordering contract |
| `Completed` | 工作完成且结果被当前操作接纳 | scheduled/started work 不算 completed |
| `Cancelled` | 已确认取消并进入终态 | ACK accepted 不等于 Cancelled |
| `CancellationUnconfirmed` | 已尝试取消但缺失完整终态证据 | NEVER 降级成 `confirmed: bool` |
| `Failed` | 业务执行失败终态 | command 拒绝使用 `Rejected` |
| `Accepted` | command/request/reply 已被协议接纳 | 非 Lifecycle terminal |
| `Rejected` | command/request/reply 被拒绝 | 非业务执行失败 |
| `Reset` | 清除旧状态并建立新 epoch | MUST 定义 identity/revision 重置规则 |
| `Expired` | 资源或租约因时间失效 | MUST 说明时钟 owner |
| `Retrying` | 当前 attempt 已失败且下一 attempt 已安排 | 非 terminal |

### 3.1 默认禁用后缀

以下后缀默认禁止用于新事件：

- `Updated`：与 `Changed`、`Snapshot` 边界不清；
- `Info`、`Data`、`Message`、`Notification`：不表达事实；
- `ProgressUpdated`：未说明 operation、stage、work 与 revision；
- `Done`：未说明作用域与 terminal owner；仅可作为已登记 compatibility event；
- `Result`：未说明是 command result、tool result 还是 lifecycle result。

已有名称不得静默视为合规，必须在 [09-event-index.md](09-event-index.md) 标记 Compatibility、Deprecated 或 Target Rename。

## 4. Changed 与 Snapshot

`Changed` 是否为 delta 不能靠名称猜测，MUST 在事件族契约中声明：

- `ActivityChanged`：revisioned delta；
- `RuntimeStatusChanged`：revisioned full-state replacement；
- `TaskStateChanged`：revisioned full-state replacement；
- `ActivitySnapshot`：用于 gap repair 的完整集合。

当 `Changed` 已携 full-state 时，不得再创建同义 `Updated`。只有存在初始化、恢复或 gap repair 的独立交付语义时才使用 `Snapshot`。

## 5. Progress 词汇

Operation progress MUST 使用 typed identity、stage 与 work：

```text
operation_id
revision
stage: <Operation>Stage
work: Indeterminate | Determinate { completed, total }
terminal
```

约束：

1. `completed` 只表示已完成且被当前 operation 接纳的工作；
2. `current` 禁止作为含混计数名；
3. producer NEVER 发布带 UI 权重的 percentage；
4. stage NEVER 压扁为自由字符串；
5. Context budget utilization 与 Compact operation progress 必须使用不同字段和事实族；
6. terminal 后的 progress 不得继续进入 live selection。

## 6. 跨层名称保持

同一事实跨层 MUST 保持 Subject + Fact：

| Runtime | SDK | TUI event | Consumer action |
|---|---|---|---|
| `RuntimeStatusChanged` | `RuntimeStatusChanged` | `RuntimeStatusChanged` | `ReplaceRuntimeStatus` |
| `TaskStateChanged` | `TaskStateChanged` | `TaskStateChanged` | `ReplaceTaskState` |
| `ActivityChanged` | `ActivityChanged` | `ActivityChanged` | `ObserveActivityChange` |
| `RunStepCancelled` | `RunStepCancelled` | `RunStep::Cancelled` | `PresentCancelledStep` |
| `ToolCallArgumentsDelta` | `ToolCallArgumentsDelta` | `ToolCallArgumentsDelta` | `AppendToolCallArguments` |
| `ToolCallStateChanged` | `ToolCallStateChanged` | `ToolCallStateChanged` | `ReplaceToolCallState` |

允许：

- Runtime 内部 enum variant 因容器上下文省略 Subject，如 `RuntimeLifecycleEvent::StepStarted`；SDK 对外名必须恢复完整 Subject；
- TUI-owned 类型增加 `Tui` / `Ui` 所有权前缀；
- Intent 使用 `Replace`、`Observe`、`Append`、`Present` 等 consumer action。

禁止：

- SDK 用 `StatusUpdate` 替代 `RuntimeStatusChanged`；
- TUI 用 `ContextUsageEvent` 替代同一 full-state fact；
- mapper 将封闭 enum 转为 `String` 或 `Debug` 文本。

## 7. 容器命名不等于事件族

以下名称是代码组织或传输容器：

- `RuntimeLifecycleEvent`
- `RuntimeActivityEvent`
- `RuntimeStreamEvent`
- `sdk::ChatEvent`
- `TuiRuntimeEvent`

业务分类必须使用 [02-event-families.md](02-event-families.md) 中的 Family。一个 `RuntimeStreamEvent` variant 可以属于 Content Stream、Published State、Interaction 或 Catalog/Config；不得把“Stream”当事实 owner。

## 8. 命名迁移

| 级别 | 范围 | 要求 |
|---|---|---|
| 内部非 wire | Runtime 私有 | 单 PR 原子 rename，编译器收口 |
| 跨 Runtime/SDK/TUI | 未对外持久化 | 全链路 rename，并更新 schema/golden/tests |
| Public SDK/wire | 外部可能消费 | 兼容新增 → deprecated → 到期删除 |
| Dead compatibility | 无 producer/consumer | 以引用证据确认后直接退役 |
| 语义冲突 | 一个名称承载多个 authority/delivery | 先拆事实，再迁移名称 |

每次迁移 MUST 更新 [09-event-index.md](09-event-index.md) 的状态和替代名。禁止只改 Rust symbol 而不改 Published Language，或只改文档隐藏实际不一致。

## 9. Review 清单

新增事件名必须全部回答“是”：

- [ ] Subject 是否来自受控词汇，或已证明新增理由？
- [ ] Fact 是否表达已发生事实？
- [ ] 是否与 command、ACK、Intent 清晰分离？
- [ ] 后缀是否具有本文定义的唯一语义？
- [ ] Runtime/SDK/TUI 是否保持同一 Subject + Fact？
- [ ] delta/full-state/snapshot 是否显式？
- [ ] identity 与 ordering 是否足以拒绝 stale/duplicate？
- [ ] 若为 progress，是否 typed 且不含展示百分比？
- [ ] 若不合规，是否在迁移表登记而非静默例外？
