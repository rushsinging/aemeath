# 04 · Activity 事件

## 1. 定位

| 属性 | 契约 |
|---|---|
| Owner | `ActivityCoordinator` |
| Authority | observational only；NEVER 决定 Lifecycle 或 processing terminal |
| Identity | `run_id`、`activity_id` |
| Ordering | per-Run revision；delta + snapshot repair |
| Delivery | `Changed` delta、`Snapshot` full collection |
| Runtime 容器 | `RuntimeActivityEvent` |
| SDK mapper | `map_activity_event` |

Activity 回答“Runtime 当前在做什么”，不回答“Run 是否已经终结”。

## 2. Published Language

| Runtime | SDK | TUI event | Consumer | 状态 |
|---|---|---|---|---|
| `RuntimeActivityEvent::Changed` | `ActivityChanged` | `ActivityChanged` | `ObserveActivityChange` | Current |
| `RuntimeActivityEvent::Snapshot` | `ActivitySnapshot` | `ActivitySnapshot` | `ReplaceActivitySnapshot` | Current |

跨层 MUST 保持 `ActivityChanged` / `ActivitySnapshot` 名称；TUI-owned payload 可增加 `Tui` 前缀。

## 3. Revision contract

1. `Changed` 是 revisioned delta，不是 full-state；
2. duplicate revision 幂等；
3. lower revision stale event 丢弃；
4. revision gap 时保留最后可信事实，标记待修复并隐藏不可信摘要；
5. `Snapshot` 原子替换同一 Run 的 Activity 集合并修复 revision；
6. Snapshot 不跨 Run merge。

## 4. Activity detail

Activity detail MUST 使用封闭类型表达 Model invocation、Tool execution、Compact operation、child Run 等工作。新增 detail variant 不应膨胀成新的顶层事件，除非它拥有不同 identity、ordering 或 delivery contract。

### 4.1 Compact operation

Compact progress 通过 Activity detail 传输：

```text
stage:
  Preparing | Generating | Mapping | Reducing | Refreshing | Finalizing

work:
  Indeterminate
  Determinate { completed, total }
```

约束：

- map completed 只在 chunk 实际完成并被 operation 接纳后增加；
- future 创建、排队、开始或拿到 semaphore 不计完成；
- reduce/refresh 使用独立 stage；
- producer 不发布 TUI percentage；
- terminal Activity 不进入 LiveStatus progress selection；
- Context budget utilization 不是 operation progress。

## 5. Audience 与展示

Activity 的 `audience` 是 Runtime 发布的展示边界：

- User：可进入用户可见摘要；
- Operational：用于运维观察，不自动进入主状态行；
- Diagnostic：用于诊断，不自动进入主状态行。

TUI 只通过 ACL/reducer 建立事实镜像，再由 Activity summary assembler 派生 LiveStatus。NEVER 读取 running tool counter、旧 Run status 或 spinner phase 反推 Activity。

## 6. Lifecycle 并行投影

Lifecycle producer MAY 同时通知 Activity observer，但两条路径职责独立：

- Lifecycle commit 失败：不得发布虚假 Activity terminal；
- Activity publish 失败：不得撤销 Lifecycle terminal；
- TUI Activity gap：等待 Snapshot，不从 Lifecycle 补造 Activity detail。

## 7. 不变量

1. Activity 不拥有 Run、Step、Session、Interaction reply 或 Task commit；
2. Activity closed state 不是 Lifecycle terminal；
3. Activity detail 不包含 LLM/Session wire 的内部 committed metadata；
4. 同一 activity revision 只能表达一个确定事实版本；
5. TUI 可改变展示权重，但不得改变 producer stage/work 语义。

## 8. 变更门禁

修改 Activity 必须同步 Runtime model/coordinator、SDK typed DTO、SDK schema、TUI structural ACL、事实镜像、summary assembler 与 delta/gap/snapshot 每层测试，并更新 [09-event-index.md](09-event-index.md)。
