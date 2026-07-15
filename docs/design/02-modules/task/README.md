# Task Management（支撑域）

> 层级：02-modules / task（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#791（S2）/ #890 / [#972](https://github.com/rushsinging/aemeath/issues/972)
> 本模块拥有 TaskStoreState 聚合、Task 局部生命周期、依赖图不变量与批次生命周期。Task 相关类型是 Task BC 的 Published Language；该局部状态机不是 Agent 执行状态机。

## 1. 模块定位

Task Management 让 Agent Runtime 在执行复杂多步工作时维护可观察的任务规划：创建、推进、标记依赖、归档批次。它是 Agent 自我组织的结构化投影，不是人的项目管理工具。

| 概念 | 回答 |
|---|---|
| **Task** | 一个可追踪的工作单元是什么 |
| **TaskStatus 状态机** | 它的生命周期怎么走 |
| **依赖图** | 哪些任务阻塞哪些任务 |
| **Batch** | 一轮对话产生的一组任务如何分组与归档 |
| **TaskSnapshot** | 跨 BC 落盘 / 恢复时携带什么 |

## 2. 核心决策

1. **TaskStoreState 是聚合根与一致性边界，Task 是聚合内实体**：`Pending → InProgress → Completed` 是 Task 实体的局部生命周期；`Deleted` 是软删除标记而非正轨状态。单 Task 字段迁移由实体方法守护，跨 Task 的 DAG、反向边、blocked admission、Batch/current_batch 与 ID 计数器由 `TaskStoreState` 在一次 mutation 内守护。该局部状态机 **NEVER** 驱动 Agent Run、checkpoint、resume 或崩溃恢复；唯一 Agent 执行生命周期状态机仍是 Runtime `AgentRun`。
2. **blocked_by 不成环**：依赖图是有向无环图（DAG）。添加依赖前必须 DFS 检测环，拒绝产生环的写入。`blocks` 是 `blocked_by` 的 live 反向投影，由聚合维护，不可独立写入，也不进入 `PersistedTask`。
3. **Batch 是 Task-owned 实体，lifecycle 检测是领域服务**：Batch 以 `BatchId`、状态与时间字段存在于唯一 `TaskStoreState` 权威状态槽，但不是独立聚合根；`TaskStoreState` 只承担跨 Task / Batch 一致性用例的单一状态持有，归档 / 中断 / stale 检测函数是只读快照的纯领域服务。
4. **Task 类型 = Task BC 的 Published Language**：`TaskId`、`BatchId`、`Task`、`TaskStatus`、`TaskPriority`、`Batch`、`TaskSnapshot` / `PersistedTask` 的类型定义所有权属 Task BC。其他 BC（Runtime、Context Management、Storage）引用其发布类型，经端口翻译，**NEVER** 自行定义副本。
5. **Task 发布两个窄 OHS**：Agent Runtime / TaskTool 只经 `TaskAccess` 执行 CRUD、依赖图与查询；Context Management 只经 `TaskPersist` 收集和恢复快照。两者来自同一 backing，**NEVER** 合并为公开 super-trait。
6. **快照内嵌 Session 落盘**：Context Management 经 `TaskPersist` 收集 `TaskSnapshot`；恢复时先取得 exclusive session-switch lease，在该同一 lease 内依次 prepare，待 Project / Config / Memory / Task 全部成功后无失败 commit。Runtime / Tool 编译期不可调用 restore。
7. **新建与 Batch lifecycle 全部 typed/fallible**：Task / Batch 经私有字段的 create spec 构造并返回结构化错误；Batch pause / resume / archive 接受明确 `BatchId`，与 `current_batch` 在一次 mutation 中更新。`Paused` 是可达、可恢复状态，Target **NEVER** 依赖 current-only shortcut。

## 3. Target 逻辑能力投影

```text
task.rs                    # 窄 façade：Task PL、TaskAccess / TaskPersist、composition-only wiring
task/
├── state.rs               # TaskStoreState + Task / Batch 权威字段
├── transition.rs          # Task 状态迁移用例
├── dependency.rs          # 依赖图不变量与删除边清理
├── batch.rs               # Batch lifecycle 用例
└── snapshot.rs            # codec + collect / prepare / commit restore
```

这是按 [代码组织规范](../../01-system/06-code-organization.md) 给出的非规范性能力投影，不是必须一次创建的目录清单。小实现可以从 `task.rs` 开始；只在某个用例已有独立共同变化与测试边界时才拆文件。`port/` / `api/` / `published_language/` **NEVER** 作为固定横向层；窄契约与所有者 façade 共置。Composition Root 仍是唯一生产装配入口。

## 4. 对外端口

| 端口 | 消费方 | 职责 |
|---|---|---|
| `TaskAccess` | Agent Runtime / TaskTool | typed/fallible 创建、更新、删除 Task；按 id 管理 Batch pause/resume/archive；查询依赖状态 |
| `TaskPersist` | Context Management | 收集快照与 prepare-commit 恢复 |

两个 OHS 由同一 TaskStore backing 实现并经 production wiring 分别分发。Runtime、Tool 与 Context Management 都不接触 TaskStore、HashMap 或内部聚合方法。

## 5. 与其他 BC 的关系

### Agent Runtime

Runtime 通过 `TaskAccess` 创建和推进任务，作为自身执行规划的投影。Runtime 不守护 Task 状态机不变量——不变量由 Task BC 独占。

### Context Management

Context Management 在 Session 落盘时经 `TaskPersist` 收集 `TaskSnapshot`；恢复时先取得联合恢复的 exclusive lease，再在 lease 内调用无副作用 prepare 与无失败 commit。prepare / commit 之间 **NEVER** 释放或降级该 lease。快照组装经窄端口，不共享内部结构。

### Storage

Storage 提供原子写与损坏兜底**机制**，不拥有 Task 数据本体。TaskStore 是纯内存 backing，实现 `TaskAccess` 与 `TaskPersist`，**NEVER** 直接做文件 I/O；Context Management 的 Session repository 把 TaskSnapshot 内嵌 Session 后才调用 Storage。

### Config

Config 通过只读 ConfigSnapshot 提供 Task 相关配置（如批次静默阈值）。Task BC 不绕过快照读取裸配置。

## 6. 设计边界

- **NEVER** 让外部调用方直接修改 `Task.status`、`blocked_by` 或 `blocks` 字段。
- **NEVER** 允许 `blocked_by` 形成环。
- **NEVER** 允许 `blocks` 被独立写入或持久化——它只能由聚合从 `blocked_by` 反向投影维护；snapshot writer 必须使用不含 `blocks` 的 `PersistedTask`。
- **NEVER** 将 Task 聚合内部结构直接暴露给 Runtime、TUI 或持久化 adapter。
- **MUST** 所有状态迁移经聚合方法，产生领域事件。
- **MUST** 添加依赖前进行环检测。
- **MUST** `current_batch` 精确指向唯一 Active Batch；pause / archive 清除匹配 current，resume 只有在无其他 Active 时才可设置 current。

## 7. 文档导航

| 文档 | 内容 |
|---|---|
| [01-domain-model.md](01-domain-model.md) | TaskStoreState 聚合根、Task 局部状态机、依赖图不变量、Batch 实体 / lifecycle 服务、TaskSnapshot |
| [02-ports-and-published-language.md](02-ports-and-published-language.md) | TaskAccess / TaskPersist、Task Published Language、Storage 集成、快照组装 |

## 8. 相关文档

- 统一语言：[../../01-system/02-ubiquitous-language.md](../../01-system/02-ubiquitous-language.md) §6 Task Management
- Context Map：[../../01-system/03-context-map.md](../../01-system/03-context-map.md) §4 / §7 / §10
- Runtime 领域模型：[../runtime/01-domain-model.md](../runtime/01-domain-model.md)
- Context Management Session：[../context-management/01-session.md](../context-management/01-session.md)
- 迁移治理：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：Task 聚合、状态机、依赖图不变量、Batch、访问契约、Published Language | #791 |
| 2026-07-14 | 拆分 TaskAccess / TaskPersist，并对齐单状态槽与联合 prepare-commit 恢复协议 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | 增加 typed/fallible create 与 Batch pause/resume/archive-by-id 契约，移除 Batch description 漂移 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
