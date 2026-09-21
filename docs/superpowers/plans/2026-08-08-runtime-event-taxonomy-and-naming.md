# Runtime 事件分类、命名与 Published Language 文档重构实施计划

> 日期：2026-08-08
> 状态：待实施
> 范围：仅设计文档；本阶段不重命名生产代码，不修改 Runtime、SDK 或 TUI 行为

## 1. 背景与根因

当前 `docs/design/02-modules/runtime/08-event-pipeline-and-published-language.md` 同时承担事件体系原则、Lifecycle 矩阵、Activity 语义、Published State、内容流、Interaction、命令结果、TUI 消费和源码索引。事件数量增长后出现以下结构性问题：

1. 事件按承载枚举或历史加入顺序平铺，事实 owner、authority、identity、ordering 和 delivery contract 不易定位；
2. command、ACK、Lifecycle terminal、Activity observation、Published State 和 compatibility event 容易被误认为同一类消息；
3. 同一事实可能跨 Runtime、SDK、TUI 发生无理由改名，事件名与 consumer Intent 名混淆；
4. `Changed`、`Snapshot`、`Completed`、`Accepted`、`Delta`、`Progress` 等后缀没有统一受控语义；
5. 单文件继续扩张会让新增事件难以机械审查，也无法形成稳定的迁移清单和 Guard 输入。

根因不是“目录不够细”，而是缺少统一的事件本体模型与命名语法。文档必须先把事件定义为“由明确 owner 发布、带 identity 与 ordering 的领域事实”，再按事实族组织 Published Language。

## 2. 目标

1. 保留 `08-event-pipeline-and-published-language.md` 作为稳定权威入口，避免现有交叉引用失效；
2. 建立编号化 `runtime/events/` 分片，按阅读依赖拆分命名、分类、事实族和全量索引；
3. 固化统一命名语法、Subject 词汇表、后缀语义和 Runtime → SDK → TUI 对应规则；
4. 为每个事件族明确 Owner、Authority、Identity、Ordering、Delivery、Producer 和 Consumer；
5. 建立现有事件全量索引与历史命名迁移表，区分 Current、Compatibility、Deprecated、Target Rename；
6. 同步 TUI ACL 文档，明确事件事实名与 consumer action/Intent 名的职责差异；
7. 为后续代码迁移与 Guard 落地提供可机械检查的权威输入。

## 3. 非目标

1. 本阶段不修改 Rust 枚举、DTO、mapper、schema、测试或架构 Guard；
2. 不因文档整理立即删除 compatibility event；
3. 不在没有 wire 兼容评估时重命名 SDK Published Language；
4. 不改变 Lifecycle terminal authority、Activity observational、Published State full-state replacement 等已冻结行为；
5. 不建立另一份与源码竞争的事件定义，文档只定义语义、分类、命名与映射契约。

## 4. 目标文档结构

```text
docs/design/02-modules/runtime/
├── 08-event-pipeline-and-published-language.md  # 稳定入口与总览
└── events/
    ├── README.md
    ├── 01-naming-conventions.md
    ├── 02-event-families.md
    ├── 03-lifecycle.md
    ├── 04-activity.md
    ├── 05-published-state.md
    ├── 06-content-stream.md
    ├── 07-interaction-and-control.md
    ├── 08-catalog-and-config.md
    └── 09-event-index.md
```

`README.md` 遵循目录入口惯例，不编号；其余文件必须使用两位数字序号。文件序号同时表达阅读依赖，禁止仅按创建时间排列。

## 5. 统一事件本体

每个事件或事件族必须回答以下字段：

| 字段 | 含义 |
|---|---|
| Family | Lifecycle、Activity、Published State、Content Stream、Interaction、Control、Catalog/Config、Compatibility |
| Owner | 拥有该事实与发布时机的领域或 Runtime 用例 |
| Subject | 事件描述的稳定领域对象 |
| Fact | 已发生的变化、状态或协议结果 |
| Authority | 是否能决定 Run、Run Step、Interaction 或 processing terminal |
| Identity | session/run/step/activity/operation/request 等相关 identity |
| Ordering | revision、sequence、exactly-once、幂等或无序约束 |
| Delivery | delta、full-state、snapshot、terminal fact、ACK 或 heartbeat |
| Producer | Runtime 内部权威产生边界 |
| SDK Name | Published Language 名称 |
| TUI Name | TUI-owned DTO/event 名称 |
| Consumer | Intent、reducer、Model 或 App-level consumer |
| Lifecycle | Current、Compatibility、Deprecated 或 Target Rename |

## 6. 命名规范目标

### 6.1 语法

事件名统一采用：

```text
<Subject><FactOrTransition>
```

事件表达已经发生的事实，禁止使用命令式 `Update*`、`Handle*`、`Process*`。命令表达意图，ACK 表达协议接纳结果，Lifecycle event 表达权威状态或终态，三者不得共用名称。

### 6.2 固定 Subject

首批受控词汇包括：

- `Session`
- `Run`
- `RunStep`
- `Activity`
- `RuntimeStatus`
- `TaskState`
- `CompactOperation`
- `ModelInvocation`
- `ToolCall`
- `InteractionRequest`
- `InteractionReply`
- `Config`
- `Workspace`
- `SkillCatalog`
- `ModelCatalog`

同一业务对象禁止并存 `Compaction`、`CompactStatus`、`SummarizationProgress` 等无迁移标记的别名。

### 6.3 固定后缀

| 后缀 | 统一语义 |
|---|---|
| `Started` | 已实际进入运行，不表示排队或 future 创建 |
| `Changed` | revisioned 事实变化；必须声明 delta 或 full-state |
| `Snapshot` | 用于初始化、恢复或 revision gap 修复的完整快照 |
| `Delta` | 可按 ordering contract 拼接的增量内容 |
| `Completed` | 业务操作成功完成且结果已接纳 |
| `Cancelled` | 已确认取消并进入终态 |
| `CancellationUnconfirmed` | 取消已尝试但缺失完整终态证据 |
| `Failed` | 业务执行进入失败终态 |
| `Accepted` / `Rejected` | 仅用于 command/request/reply 协议结果，不代表 Lifecycle terminal |
| `Reset` | 清除旧状态并建立新 revision epoch |
| `Expired` | 资源或租约因时间条件失效 |

默认禁止事件后缀 `Updated`。现有 `*Updated` 必须登记为 Compatibility 或 Target Rename，并给出替代名。

### 6.4 跨层名称

Runtime、SDK 与 TUI 对同一事实应保持 Subject 与 Fact 一致，只允许所有权前缀或容器差异：

```text
RuntimeStatusChanged
  → sdk::ChatEvent::RuntimeStatusChanged
  → TuiRuntimeEvent::RuntimeStatusChanged
  → ReplaceRuntimeStatus
```

`Replace*`、`Observe*`、`Append*` 是 consumer action/Intent，不是 Runtime event。禁止将 `RuntimeStatusChanged` 在 SDK 改成 `StatusUpdate`、在 TUI 改成 `ContextUsageEvent`。

## 7. 文档职责分配

### Task 1：建立稳定入口和导航

修改 `08-event-pipeline-and-published-language.md`：

1. 保留文档 URL 和权威入口身份；
2. 收敛为事件体系总览、不可违反的 authority 规则、统一生产链路和分片导航；
3. 将详细矩阵迁移到 `events/` 分片；
4. 明确新增事件必须先归属 Family，再进入 SDK/TUI mapper；
5. 删除入口中的重复逐事件说明，避免双份真相。

验收：入口可在两屏内说明事实族、权威边界、文档导航和变更门禁。

### Task 2：编写命名规范

创建 `events/01-naming-conventions.md`：

1. 定义事件、command、ACK、Intent、Change 的词法边界；
2. 固化 Subject 词汇表和后缀语义；
3. 定义 progress 的 `stage/work/operation_id/revision` 词汇，禁止 stringly stage、含混 `current` 和 producer percentage；
4. 定义 Runtime/SDK/TUI 名称保持规则；
5. 给出推荐、禁止和迁移中示例；
6. 定义 breaking 与 non-breaking 命名迁移流程。

验收：仅凭该文档可以判断一个候选事件名是否合规。

### Task 3：建立事件族模型

创建 `events/02-event-families.md`：

1. 定义各 Family 的 owner、authority、identity、ordering、delivery；
2. 给出“应归哪个事件族”的决策树；
3. 冻结 ACK 非 terminal、Activity observational、Published State full-state、Content Stream delta 等边界；
4. 明确 Compatibility 不是业务事实族，而是迁移状态标签。

验收：任一事件只能有一个事实族归属；同一 terminal 不得在多个族重复定义。

### Task 4：拆分各事实族文档

依次创建：

- `03-lifecycle.md`
- `04-activity.md`
- `05-published-state.md`
- `06-content-stream.md`
- `07-interaction-and-control.md`
- `08-catalog-and-config.md`

每份文档使用统一模板：定位、Owner、Authority、Identity、Ordering、Delivery、生产链路、跨层矩阵、不变量、明确非职责、变更门禁。

验收：原 08 文档中的每条有效语义均迁入唯一分片，无遗漏、无重复权威定义。

### Task 5：建立全量事件索引与迁移清单

创建 `events/09-event-index.md`：

1. 按 Family 分组列出 Runtime event、SDK name、TUI name、consumer、identity、delivery、authority；
2. 标注 Current、Compatibility、Deprecated、Target Rename；
3. 对不符合命名规范的现有项建立迁移表；
4. 区分内部可直接重命名、SDK breaking change、需 dual-read/dual-write 过渡和应直接退役的死事件；
5. 记录 `RuntimeStreamEvent` 与 `ChatEvent` 容器归属，但不把容器名当业务事实族。

验收：可从任一 Runtime、SDK 或 TUI 名称反查同一事实的全链路与迁移状态。

### Task 6：同步 TUI ACL 与模块导航

修改：

- `docs/design/02-modules/tui/03-event-flow-and-acl.md`
- `docs/design/02-modules/runtime/README.md`
- `docs/design/02-modules/tui/README.md`

要求：

1. TUI ACL 引用 Runtime 事件入口、命名规范和全量索引；
2. 明确第一层转换保持业务事实名，第二层 Intent 使用 consumer action 命名；
3. 修正陈旧的 `UiEvent` 唯一生产路径与已退役 `CompactProgress` 描述；
4. Runtime/TUI README 增加编号分片导航和权威边界；
5. 所有旧链接保持有效或提供稳定入口。

验收：Runtime 和 TUI 两侧不会各自定义一套事件词汇。

## 8. 历史命名迁移策略

文档阶段必须先登记、不立即改代码。迁移清单按以下等级分类：

| 等级 | 条件 | 后续策略 |
|---|---|---|
| A：内部非 wire | 仅 Runtime 内部使用 | 同一 PR 原子重命名并由编译器收口 |
| B：跨 Runtime/SDK/TUI 但未持久化 | 可协调升级 | 单 PR 全链路重命名，更新 schema/golden |
| C：SDK/public wire | 外部消费者可能依赖 | 先兼容新增，标记 deprecated，再按版本策略删除 |
| D：dead compatibility | 无生产 producer 或 consumer | 先以证据证明无引用，再直接退役 |
| E：语义冲突 | 名称掩盖 authority/delivery 差异 | 先拆分事实，再迁移名称，禁止只做机械 rename |

任何代码迁移必须另建实施计划，并遵循 Domain/Runtime → SDK → TUI → Guard 的依赖顺序。

## 9. 文档验证门禁

本阶段完成时执行：

1. `find docs/design/02-modules/runtime/events -maxdepth 1 -type f | sort`，确认编号连续且 `README.md` 不编号；
2. 检查 `08-event-pipeline-and-published-language.md`、Runtime README、TUI ACL、TUI README 的相对链接；
3. `grep` 确认 design 文档不再把已退役 stringly `CompactProgress` 描述为 Current；
4. 检查每个事件族文档都包含 Owner、Authority、Identity、Ordering、Delivery；
5. 检查 `09-event-index.md` 覆盖 Runtime lifecycle/activity/stream、SDK-only control-plane 和 compatibility 项；
6. `git diff --check`；
7. `git diff --name-only` 确认本阶段只修改 `docs/design/**` 与本计划文档。

## 10. 后续代码迁移门禁

文档合入后，代码迁移必须满足：

1. 新增事件先更新命名规范与 `09-event-index.md`；
2. 同一事实的 Runtime/SDK/TUI 名称无理由漂移必须被 Guard 阻断；
3. 禁止新增 `*Updated`、`*Notification`、`*Info`、`*Data` 等宽泛事件名，除非命名文档登记例外；
4. command ACK 禁止使用 Lifecycle terminal 名；
5. `Changed` 必须标注 delta/full-state，`Snapshot` 必须是完整替换；
6. progress 必须使用 typed stage/work，producer 不发布展示百分比；
7. 每次重命名必须更新 mapper、schema、ACL、测试、event index 和 migration status；
8. SDK breaking rename 必须显式记录兼容窗口和删除条件。

## 11. 完成定义

- [ ] 稳定入口完成收敛并链接全部编号分片；
- [ ] 命名规则可独立审查候选事件名；
- [ ] 事件族决策树与 authority 边界完整；
- [ ] 六份事实族文档采用统一模板；
- [ ] 全量事件索引可从三层名称双向检索；
- [ ] 历史不规范名称均有迁移状态，不静默遗留；
- [ ] TUI ACL 与 Runtime Published Language 使用同一词汇；
- [ ] Runtime/TUI README 导航已更新；
- [ ] 文档链接、编号、diff scope 与 Markdown 空白检查通过；
- [ ] 未修改生产代码或运行行为。
