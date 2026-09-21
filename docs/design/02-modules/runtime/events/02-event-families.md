# 02 · Runtime 事件族

## 1. 事件族不是 Rust 枚举

事件族按业务事实分类；Rust enum 只负责代码组织和传输。每个事件 MUST 归属一个且仅一个 Family。

| Family | Owner | Authority | 典型 Identity | Ordering | Delivery |
|---|---|---|---|---|---|
| Lifecycle | Run 聚合 | Run / Run Step terminal 唯一权威 | run_id、step_id | 聚合顺序、terminal exactly-once | transition / terminal fact |
| Activity | ActivityCoordinator | observational only | run_id、activity_id | revision delta + snapshot repair | delta / snapshot |
| Published State | Runtime Registry；事实来自对应 owner | 状态交付权威，不是 lifecycle authority | session_id、family revision | revision + heartbeat sequence | full-state replacement |
| Content Stream | Runtime loop / Provider / Tool orchestration | 内容与 Chat processing 边界 | run/step/tool/block | sequence 或 stream order | delta / content fact / compatibility terminal |
| Interaction | Runtime Interaction resource owner | request/reply 资源协议 | request_id、run_id | request identity + exactly-once reply | request fact / reply outcome |
| Control | Runtime control use case | command outcome；非 terminal | run_id、step_id、request_id | request/response | synchronous ACK |
| Catalog / Config | Config、Workspace、Catalog owner | 当前配置/目录事实 | revision、workspace identity | revision 或 atomic snapshot | full-state / command result |

`Compatibility` 不是 Family，只是某个事件的生命周期状态标签。

## 2. 归类决策树

```text
事件是否决定 Run / Run Step 状态或终态？
├─ 是 → Lifecycle
└─ 否
   ├─ 是否描述“当前在做什么”的观测事实？ → Activity
   ├─ 是否携带可原子替换的 revisioned 完整状态？ → Published State
   ├─ 是否是文本/Thinking/Tool/usage 等增量或 Chat processing 边界？ → Content Stream
   ├─ 是否创建或回答带 request identity 的交互资源？ → Interaction
   ├─ 是否只是 command 的 accepted/rejected/failed 返回？ → Control
   └─ 是否改变配置、workspace 或目录快照？ → Catalog / Config
```

若一个候选事件同时落入两类，说明它混合事实，MUST 先拆分，禁止任选一个类别。

## 3. Authority 矩阵

| 事实 | 唯一 authority | 明确非 authority |
|---|---|---|
| Run terminal | Lifecycle `RunCompleted/RunFailed/RunTerminated` | ACK、Activity、Done、ToolResult 文本 |
| Run Step cancellation terminal | Lifecycle `RunStepCancelled` typed terminal | cancel accepted、timeout 字符串、Activity closed |
| Activity 展示 | Activity revision/snapshot | TUI running-tool counter |
| Runtime Status | Published State revisioned full-state | TUI config reader、token heuristic |
| Task full state | Task committed fact → Published State | tool name、success text、round materialization |
| Compact operation progress | Context typed progress → Activity detail | Context utilization、auto-compact threshold |
| Interaction reply | request identity 对应的 reply outcome | UI block 是否消失 |
| Chat processing 结束 | 已登记 processing compatibility terminal | Run terminal 的名称替代或 ACK 合成 |

## 4. Identity 规则

1. `session_id` 建立 Published State revision epoch；跨 session 不比较 family revision。
2. `run_id` 标识一次 Run；`step_id` 只在所属 Run 内有意义。
3. `activity_id` 与 `run_id + revision` 共同支持增量接纳和 snapshot repair。
4. `operation_id` 标识一次 Compact 等长操作；stage/work revision 不能跨 operation 复用。
5. `request_id` 标识 Interaction resource，reply/cancel 必须原样回传。
6. Tool、content block 与 child run 的 identity 必须保留到 TUI ACL，不得靠显示顺序反推。

## 5. Ordering 与交付

### 5.1 Lifecycle

- terminal exactly-once；
- stale ACK 不得回滚 terminal；
- Activity 更新失败不影响 terminal commit。

### 5.2 Activity

- `Changed` 是 revisioned delta；
- duplicate revision 幂等；
- revision gap 隐藏不可信摘要并等待 `Snapshot`；
- Snapshot 原子替换同一 Run 的 Activity 集合。

### 5.3 Published State

- `Changed` 携 full-state replacement；
- 同一 session/family 只接受更高 revision；
- heartbeat 可在同 revision 增加 heartbeat sequence；
- heartbeat NEVER 增加业务 revision；
- consumer NEVER merge 缺失字段或跨 BC 临时拼装。

### 5.4 Content Stream

- `Delta` 必须保持可拼接顺序；
- Tool Result 必须关联 tool call identity；
- processing compatibility terminal 的作用域必须与 Run/Step terminal 分离。

### 5.5 Interaction / Control

- ACK 是 command 调用返回值，不进入 Lifecycle 事件流；
- accepted 只表示命令被接纳；
- terminal 必须等待 Lifecycle 或相应资源 owner 的后续事实。

## 6. Producer 与 Consumer 分工

```text
Domain / Runtime use case owns fact
  → Runtime event container
  → Runtime SDK mapper
  → SDK Published Language
  → TUI structural ACL（保留事实名）
  → TUI semantic ACL（转 consumer action Intent）
  → reducer / Model
  → ViewAssembler / Render
```

- Producer 决定事实、identity、ordering、terminal 与 full-state 内容。
- SDK 负责稳定 DTO 和 wire schema，不创造业务事实。
- TUI 第一层只做无损结构转换。
- TUI 第二层可将事实翻译为 `Observe*`、`Replace*`、`Append*` 等动作。
- Render 只展示，不推断业务状态。

## 7. Compatibility 标记

| 状态 | 含义 |
|---|---|
| Current | 当前推荐、名称与语义一致 |
| Compatibility | 仍有生产或消费依赖，但名称/作用域属于历史协议 |
| Deprecated | 已提供替代路径，禁止新增 consumer |
| Target Rename | 语义保留但名称需迁移 |
| Target Removal | 无独立语义或已被新事实族替代，计划删除 |

Compatibility event 仍必须归属一个真实 Family，并在 [09-event-index.md](09-event-index.md) 记录替代事实和退出条件。
