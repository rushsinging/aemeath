# Runtime 事件体系

> 层级：02-modules / runtime / events
> 状态：Current + Target 契约
> 权威入口：[Runtime 事件管线与 Published Language](../08-event-pipeline-and-published-language.md)

本目录按“事实 owner → authority → identity → ordering → delivery”组织 Runtime 对外事件。目录不是按 Rust 枚举、文件位置或历史加入顺序分类；`RuntimeLifecycleEvent`、`RuntimeActivityEvent`、`RuntimeStreamEvent` 与 SDK `ChatEvent` 是传输容器，不等同于业务事件族。

## 阅读顺序

| 编号 | 文档 | 职责 |
|---|---|---|
| 01 | [命名规范](01-naming-conventions.md) | 事件语法、受控 Subject、后缀语义、跨层名称与迁移规则 |
| 02 | [事件族](02-event-families.md) | 事实族模型、authority、identity、ordering、delivery 与归类决策树 |
| 03 | [Lifecycle](03-lifecycle.md) | Run / Run Step 生命周期与 terminal authority |
| 04 | [Activity](04-activity.md) | revisioned observational facts 与 snapshot repair |
| 05 | [Published State](05-published-state.md) | Runtime Status / Task full-state、revision 与 heartbeat |
| 06 | [Content Stream](06-content-stream.md) | 文本、Thinking、Tool、usage 与 processing compatibility 流 |
| 07 | [Interaction 与 Control](07-interaction-and-control.md) | request/reply identity、command、ACK 与 lifecycle 分离 |
| 08 | [Catalog 与 Config](08-catalog-and-config.md) | Config、Workspace、Skill/Model/Session catalog 与 command result |
| 09 | [全量事件索引](09-event-index.md) | Runtime → SDK → TUI 映射、delivery、authority 和迁移状态 |

## 不可违反的边界

1. Lifecycle 是 Run / Run Step terminal 的唯一权威来源。
2. Activity 只表达观察事实，NEVER 决定 terminal。
3. Published State 是 revisioned full-state replacement，NEVER 由消费者拼装业务真相。
4. Content Stream 的 delta 与 Chat processing terminal 不得冒充 Run terminal。
5. Control ACK 只说明 command outcome，NEVER 等价于 cancellation terminal。
6. Compatibility 是迁移状态标签，不是新的业务事件族。
7. 同一事实跨 Runtime、SDK、TUI 必须保持 Subject 与 Fact 一致；Intent 可按 consumer action 命名。

## 变更流程

新增、删除、重命名或改变事件字段时 MUST：

1. 先在 [事件族](02-event-families.md) 判定唯一 Family；
2. 按 [命名规范](01-naming-conventions.md) 审查 Subject 与 Fact；
3. 更新对应事实族文档；
4. 更新 [全量事件索引](09-event-index.md)；
5. 再修改 Runtime producer、SDK mapper/schema、TUI ACL/reducer 与逐层测试；
6. 若为 breaking rename，先登记兼容窗口与删除条件。
