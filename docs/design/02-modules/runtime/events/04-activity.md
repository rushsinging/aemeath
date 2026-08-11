# 04 · Activity 事件

## 1. 定位

| 属性 | 契约 |
|---|---|
| Owner | `ActivityCoordinator` |
| Authority | observational only；NEVER 决定 Lifecycle 或 processing terminal |
| Identity | `run_id`、`activity_id` |
| Ordering | per-Run business revision + heartbeat sequence |
| Delivery | logical-commit `Snapshot` full-state + fixed heartbeat resend |
| Runtime 容器 | `RuntimeActivityEvent` |
| SDK mapper | `map_activity_event` |

Activity 回答“Runtime 当前在做什么”，不回答“Run 是否已经终结”。

## 2. Published Language

| Runtime | SDK | TUI event | Consumer | 状态 |
|---|---|---|---|---|
| `RuntimeActivityEvent::Snapshot` | `ActivitySnapshot` | `ActivitySnapshot` | `ReplaceActivitySnapshot` | Current canonical |
| 无 production producer | `ActivityChanged` | `ActivityChanged` | `ObserveActivityChange` | SDK / first TUI ACL compatibility |

Runtime production MUST 只发布 `ActivitySnapshot`。`ActivityChanged` 仅保留 public compatibility ingress，NEVER 作为 canonical graph 拼装来源。

## 3. Revision contract

1. Snapshot `revision` 是 Activity graph business revision，只在 logical commit 后递增；
2. Snapshot `heartbeat_sequence` 在同 business revision 下递增并刷新权威 timing sample；
3. logical transition MUST 原子完成旧 primary terminal 与新 primary start 后只发布一个 Snapshot；
4. 更低 revision 丢弃；同 revision 仅接受更高 heartbeat sequence；
5. Snapshot 原子替换同一 Run 的 Activity 集合，NEVER 跨 Run merge；
6. Snapshot 内全部 timing MUST 使用同一 Runtime 单调时钟采样点；
7. SDK `ActivityChanged` compatibility 输入不得恢复 production delta/gap 拼装。

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
