# Task Management（支撑域）

> 层级：02-modules / task（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#791（S2）
> 本模块拥有任务的状态机、依赖图不变量与批次生命周期。Task 类型是 Task BC 的 Published Language，其他 BC 引用其发布类型。

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

1. **Task 是聚合根 + 状态机所有者**：`Pending → InProgress → Completed` 是唯一正轨；`Deleted` 是软删除标记而非状态机正轨状态。状态迁移由聚合方法守护，不允许外部直接写字段。
2. **blocked_by 不成环**：依赖图是有向无环图（DAG）。添加依赖前必须 DFS 检测环，拒绝产生环的写入。`blocks` 是 `blocked_by` 的反向投影，由聚合维护，不可独立写入。
3. **Batch 是领域服务，非独立聚合根**：Batch 管理一组 Task 的分组与生命周期（Active / Paused / Archived），但不拥有独立状态机。Batch 归档由 lifecycle 检测函数驱动，检测逻辑是纯函数。
4. **Task 类型 = Task BC 的 Published Language**：`Task`、`TaskStatus`、`TaskPriority`、`Batch`、`TaskSnapshot` 的类型定义所有权属 Task BC。其他 BC（Runtime、Context Management、Storage）引用其发布类型，经端口翻译，**NEVER** 自行定义副本。
5. **TaskPort 统一出站端口**：Agent Runtime 经 `TaskPort` 读写 Task，不直接接触 TaskStore 或内部 HashMap。端口覆盖 CRUD、依赖图操作、快照收集与恢复。
6. **快照内嵌 Session 落盘**：Context Management 在 Session 落盘时经 `TaskPort` 收集 `TaskSnapshot`，恢复时分发回去。跨 BC 快照经端口组装，边界不破。

## 3. 模块内部结构

```text
task/
├── aggregate/              # Task 聚合根：字段、状态迁移方法、依赖图操作
│   ├── status.rs           # TaskStatus 状态机与迁移矩阵
│   ├── dependency.rs       # 依赖图不变量：环检测、is_blocked、反向投影
│   └── batch.rs            # Batch 领域服务：分组、lifecycle 检测
├── snapshot/               # TaskSnapshot：可持久化快照结构
├── port/                   # TaskPort trait（出站端口定义）
├── published_language/     # Task PL 类型发布（Task/TaskStatus/...）
└── api/                    # BC 对外 facade
```

目录表达业务能力而非 `contract / business / gateway / utils` 等横向技术层。Composition Root 是唯一生产装配入口。

## 4. 对外端口

| 端口 | 消费方 | 职责 |
|---|---|---|
| `TaskPort` | Agent Runtime | 创建、更新、删除 Task；查询依赖状态；收集 / 恢复快照 |

Task BC 只暴露一个出站端口。Runtime 不接触 TaskStore、HashMap 或内部聚合方法。

## 5. 与其他 BC 的关系

### Agent Runtime

Runtime 通过 `TaskPort` 创建和推进任务，作为自身执行规划的投影。Runtime 不守护 Task 状态机不变量——不变量由 Task BC 独占。

### Context Management

Context Management 在 Session 落盘时经 `TaskPort` 收集 `TaskSnapshot`，恢复时分发。快照组装经端口，不共享内部结构。

### Storage

Storage 提供原子写与损坏兜底**机制**，不拥有 Task 数据本体。TaskStore 是 Task BC 的 Repository adapter，实现 `TaskPort`，底层使用 Storage 的文件 I/O 能力。

### Config

Config 通过只读 ConfigSnapshot 提供 Task 相关配置（如批次静默阈值）。Task BC 不绕过快照读取裸配置。

## 6. 设计边界

- **NEVER** 让外部调用方直接修改 `Task.status`、`blocked_by` 或 `blocks` 字段。
- **NEVER** 允许 `blocked_by` 形成环。
- **NEVER** 允许 `blocks` 被独立写入——它只能由聚合从 `blocked_by` 反向投影维护。
- **NEVER** 将 Task 聚合内部结构直接暴露给 Runtime、TUI 或持久化 adapter。
- **MUST** 所有状态迁移经聚合方法，产生领域事件。
- **MUST** 添加依赖前进行环检测。

## 7. 文档导航

| 文档 | 内容 |
|---|---|
| [01-domain-model.md](01-domain-model.md) | Task 聚合根、状态机、依赖图不变量、Batch 领域服务、TaskSnapshot |
| [02-ports-and-published-language.md](02-ports-and-published-language.md) | TaskPort、Task Published Language、Storage 集成、快照组装 |

## 8. 相关文档

- 统一语言：[../../01-system/02-ubiquitous-language.md](../../01-system/02-ubiquitous-language.md) §6 Task Management
- Context Map：[../../01-system/03-context-map.md](../../01-system/03-context-map.md) §4 / §7 / §10
- Runtime 领域模型：[../runtime/01-domain-model.md](../runtime/01-domain-model.md)
- Context Management Session：[../context-management/01-session.md](../context-management/01-session.md)
- 迁移治理：[../../03-engineering/migration-governance.md](../../03-engineering/migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：Task 聚合、状态机、依赖图不变量、Batch、TaskPort、Published Language | #791 |
