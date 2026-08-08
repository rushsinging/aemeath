# Runtime 事件管线与 Published Language

> 层级：02-modules / runtime（模块战术设计）
> 状态：Current + Target 契约｜Milestone：v0.1.0
> 本文是 Runtime 对外事件体系的稳定权威入口。详细命名、事实族语义与逐事件映射位于 [`events/`](events/README.md)。

## 1. 所有权与边界

全链路事件契约归 Runtime 模块所有，因为 Runtime 决定业务事实、identity、authority、ordering 和发布时间。SDK 负责稳定 Published Language 与 wire DTO；TUI 负责无损防腐转换、事实镜像和展示投影。

```text
Domain / Runtime use case owns fact
  → Runtime event container
  → Runtime SDK mapper
  → sdk::ChatEvent / typed command outcome
  → TUI structural ACL
  → TUI semantic ACL
  → reducer / Model
  → ViewAssembler / Render
```

Runtime event container 是代码组织，不是业务事实族：

- `RuntimeLifecycleEvent`：主要承载 Lifecycle；
- `RuntimeActivityEvent`：承载 Activity delta/snapshot；
- `RuntimeStreamEvent`：可承载 Published State、Content Stream、Interaction、Catalog/Config；
- `sdk::ChatEvent`：多事件族汇合后的传输语言。

## 2. 统一事件本体

每个事件 MUST 明确：

| 字段 | 必答问题 |
|---|---|
| Family | 它属于哪个唯一事实族？ |
| Owner | 谁拥有事实与发布时间？ |
| Subject / Fact | 哪个稳定对象发生了什么？ |
| Authority | 它能否决定 Run/Step/processing terminal？ |
| Identity | 如何关联 session/run/step/activity/operation/request？ |
| Ordering | 如何处理 stale、duplicate、gap 与 exactly-once？ |
| Delivery | delta、full-state、snapshot、terminal fact、ACK 还是 heartbeat？ |
| Cross-layer names | Runtime、SDK、TUI 是否保持同一 Subject + Fact？ |

命名真相源见 [01-naming-conventions.md](events/01-naming-conventions.md)，全量映射见 [09-event-index.md](events/09-event-index.md)。

## 3. 事件族与 authority

| Family | Owner | Authority | Delivery | 详情 |
|---|---|---|---|---|
| Lifecycle | Run 聚合 | Run / Run Step terminal 唯一权威 | transition / terminal fact | [03-lifecycle.md](events/03-lifecycle.md) |
| Activity | ActivityCoordinator | observational only | revision delta / snapshot | [04-activity.md](events/04-activity.md) |
| Published State | Runtime Registry + 业务事实 owner | revisioned full-state delivery | full-state / heartbeat | [05-published-state.md](events/05-published-state.md) |
| Content Stream | Runtime loop / Provider / Tool | 内容与 Chat processing 边界 | delta / content fact / compatibility terminal | [06-content-stream.md](events/06-content-stream.md) |
| Interaction | Interaction resource owner | request/reply 协议 | request fact / reply outcome | [07-interaction-and-control.md](events/07-interaction-and-control.md) |
| Control | Runtime control use case | command outcome；非 terminal | synchronous ACK | [07-interaction-and-control.md](events/07-interaction-and-control.md) |
| Catalog / Config | 对应配置、Workspace、Catalog owner | 当前配置/目录事实 | full-state / snapshot / typed result | [08-catalog-and-config.md](events/08-catalog-and-config.md) |

归类决策树与 identity/delivery 细则见 [02-event-families.md](events/02-event-families.md)。

## 4. 不可违反的权威边界

1. `RunCompleted`、`RunFailed`、`RunTerminated` 是 Run terminal；`RunStepCancelled` 是 Step cancellation terminal。
2. `CancelRunStepAccepted`、`TerminateRunAccepted` 等 ACK 只表示 command 被接纳，NEVER 是 terminal。
3. Activity 是 Lifecycle 的并行 observation，失败、乱序或 gap 不改变 terminal。
4. Published State 是 revisioned full-state replacement，不从 TUI config reader、token heuristic 或旧字段 merge 重建。
5. Task State 只由 typed committed fact 驱动，不从 Tool 名、success text、result materialization 或 Activity 推断。
6. Compact operation progress 使用 Activity typed stage/work；Context budget utilization 是独立 Published State 事实。
7. `Done` / `Cancelled` 等 compatibility Chat processing terminal 与 Run/Step terminal 作用域不同。
8. Live 与 Resume 必须消费同一 durable terminal 语义，NEVER 把 cancellation 恢复为 Completed。

## 5. 命名规则摘要

事件名使用：

```text
<Subject><FactOrTransition>
```

- 事件表达已发生事实：`RunStepCancelled`；
- command 表达意图：`CancelRunStep`；
- ACK 表达协议结果：`CancelRunStepAccepted`；
- consumer Intent 表达动作：`PresentCancelledStep`、`ReplaceRuntimeStatus`。

默认禁止新增 `*Updated`、`*Info`、`*Data`、`*Notification`、宽泛 `Done` 或 stringly `ProgressUpdated`。现有不规范名称必须在全量索引登记 Compatibility / Deprecated / Target Rename，不得静默视为例外。

## 6. TUI 消费边界

当前 SDK Runtime stream 生产链路：

```text
sdk::ChatEvent
  → sdk_event_to_tui_event
  → SdkEventMapping::Runtime(TuiRuntimeEvent) / Nop
  → TuiMsg::Runtime / RuntimeBatch
  → App::update_runtime_event
  → map_runtime_event
  → AgentEventMapping
  → root_reducer
```

第一层 ACL MUST 保持业务事实名与 typed payload；第二层可转为 `Observe*`、`Replace*`、`Append*`、`Present*` Intent。TUI 不得通过无理由改名隐藏事实来源，也不得从 presentation 反推 Runtime 业务状态。

TUI 详细规则见 [TUI 事件流与 ACL](../tui/03-event-flow-and-acl.md)。

## 7. 文档导航

| 文档 | 内容 |
|---|---|
| [events/README.md](events/README.md) | 事件体系目录和阅读顺序 |
| [01-naming-conventions.md](events/01-naming-conventions.md) | 受控 Subject、后缀、跨层名称与迁移规范 |
| [02-event-families.md](events/02-event-families.md) | Family、authority、identity、ordering、delivery 与决策树 |
| [03-lifecycle.md](events/03-lifecycle.md) | Run / Run Step Lifecycle |
| [04-activity.md](events/04-activity.md) | Activity delta/snapshot 与 typed operation progress |
| [05-published-state.md](events/05-published-state.md) | Runtime Status、Task State、revision 与 heartbeat |
| [06-content-stream.md](events/06-content-stream.md) | 文本、Thinking、Tool、usage、Session 与 compatibility processing |
| [07-interaction-and-control.md](events/07-interaction-and-control.md) | Interaction resource、command 与 ACK |
| [08-catalog-and-config.md](events/08-catalog-and-config.md) | Config、Workspace、Catalog 与 query result |
| [09-event-index.md](events/09-event-index.md) | 全量 Runtime → SDK → TUI 索引和命名迁移清单 |

## 8. 变更门禁

新增、修改、删除或重命名 Runtime 对外事实时 MUST：

1. 在事件族决策树中确定唯一 Family；
2. 按命名规范核验 Subject + Fact；
3. 更新对应事实族文档和全量索引；
4. 同步 Runtime producer/container 与穷尽 SDK mapper；
5. 同步 SDK DTO、wire schema/golden；
6. 同步 TUI structural ACL、typed DTO、semantic ACL、reducer/model 或明确 `Nop`；
7. 为 Runtime emit、SDK mapping、TUI mapping、reducer/model 每一层提供相邻测试；
8. 修改 terminal、identity、Live/Resume、revision 或 ACK 边界时运行完整 architecture guards；
9. breaking rename 先登记兼容窗口和删除条件；
10. 禁止 wildcard 静默吞掉未登记事件。

## 9. 源码索引

| 层 | 权威文件 |
|---|---|
| Runtime Lifecycle | `agent/features/runtime/src/domain/agent_run/event.rs` |
| Runtime Activity / stream containers | `agent/features/runtime/src/application/loop_engine/chat/events.rs` |
| Runtime → SDK mapping | `agent/features/runtime/src/adapters/sdk_event_mapper.rs` |
| SDK Published Language | `packages/sdk/src/chat_event.rs`、`activity.rs`、`run.rs`、`runtime_status.rs` |
| SDK → TUI structural ACL | `apps/cli/src/tui/adapter/event_mapping.rs` |
| TUI-owned runtime DTO | `apps/cli/src/tui/adapter/tui_runtime_event.rs` |
| TUI semantic ACL | `apps/cli/src/tui/adapter/agent_event.rs` |
| App-level consumption | `apps/cli/src/tui/app/update.rs` |

## 修改历史

| 日期 | 变更 |
|---|---|
| 2026-08-05 | 建立 Runtime lifecycle/activity/stream → SDK → TUI 全链路目录。 |
| 2026-08-08 | 按事实 owner、authority、identity、ordering、delivery 重构为稳定入口 + 编号事件分片；建立统一命名规范、全量索引和迁移清单。 |
