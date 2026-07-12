# Task 端口与 Published Language

> 层级：02-modules / task（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#791（S2）

## 1. TaskPort

TaskPort 是 Task BC 唯一的出站端口，Agent Runtime 和 Context Management 通过它访问 Task 能力。Runtime 不接触 TaskStore、HashMap 或内部聚合方法。

### 1.1 端口定义

```rust
#[async_trait]
pub trait TaskPort: Send + Sync {
    // ── CRUD ──
    async fn create(&self, subject: String, description: String, active_form: Option<String>) -> Task;
    async fn create_with_priority(&self, subject: String, description: String, active_form: Option<String>, priority: TaskPriority) -> Task;
    async fn get(&self, id: &str) -> Option<Task>;
    async fn update(&self, id: &str, f: &dyn Fn(&mut Task)) -> Option<Task>;
    async fn delete(&self, id: &str) -> bool;

    // ── 依赖图 ──
    async fn is_blocked(&self, task: &Task) -> bool;
    async fn would_create_cycle(&self, task: &Task, blocked_by_id: &str) -> bool;

    // ── 批次 ──
    async fn create_list(&self, summary: String, description: String);
    async fn complete_list(&self);

    // ── 查询 ──
    async fn list(&self) -> Vec<Task>;
    async fn list_batches(&self) -> Vec<Batch>;
    async fn stats(&self) -> TaskStoreStats;

    // ── 快照 ──
    async fn collect_snapshot(&self) -> TaskSnapshot;
    async fn restore_snapshot(&self, snapshot: TaskSnapshot);
}
```

### 1.2 端口设计原则

| 原则 | 说明 |
|---|---|
| **返回值用 PL 类型** | 所有返回值是 `Task`、`TaskSnapshot` 等 Published Language 类型，不泄漏内部结构 |
| **不暴露 store** | 调用方拿不到 `&TaskStore` 或 `&HashMap` |
| **聚合方法内化** | `update` 接受闭包操作 `&mut Task`，但状态迁移不变量由 Task 聚合方法守护（闭包内调用 `transition_to` 等） |
| **快照经端口** | `collect_snapshot` / `restore_snapshot` 是跨 BC 快照组装的唯一入口 |

> **Decision**：`update` 的闭包模式当前允许调用方直接写字段。目标态应改为只能调用聚合方法（`transition_to`、`set_progress` 等），但这一收窄需要与 Runtime 消费方协调，在 S5 迁移时落地。当前设计文档锁定目标态为"聚合方法内化"。

### 1.3 消费方

| 消费方 | 使用方式 |
|---|---|
| **Agent Runtime** | 创建 / 推进 / 删除 Task，查询 is_blocked 决定是否可执行 |
| **Context Management** | Session 落盘时 `collect_snapshot`，恢复时 `restore_snapshot` |
| **TUI** | 经 SDK 事件投影 Task 状态，**NEVER** 直接调用 TaskPort |

## 2. Task Published Language

### 2.1 PL 类型清单

Context Map §7 和 §10 决策：Task 类型是 Task BC 的 Published Language（非 Shared Kernel），由 Task BC 独占不变量，其他 BC 引用其发布类型。

| PL 类型 | 说明 | 消费方 |
|---|---|---|
| `Task` | 任务聚合根的可序列化表示 | Runtime、Context Management、TUI（经事件） |
| `TaskStatus` | 状态枚举（Pending / InProgress / Completed / Deleted） | Runtime、TUI |
| `TaskPriority` | 优先级枚举（Low / Normal / High / Urgent） | Runtime、TUI |
| `TaskTimestamps` | 时间戳值对象 | 内部 |
| `Batch` | 批次领域服务类型 | Runtime、TUI |
| `BatchStatus` | 批次状态枚举（Active / Paused / Archived） | Runtime、TUI |
| `TaskSnapshot` | 可持久化快照 | Context Management |
| `TaskStoreStats` | 统计信息 | TUI 显示 |

### 2.2 PL 发布位置

| 位置 | 角色 | 说明 |
|---|---|---|
| **Task BC `published_language/`** | 所有权属 | 类型定义的权威来源 |
| **`packages/sdk`** | 契约 crate | SDK 中引用 Task PL 类型，供 TUI / Server 消费 |
| **`agent/shared`** | 过渡桥接 | 当前类型定义在 `shared/tool/types/task.rs`，目标迁移到 Task BC 所有权下 |

> **Decision**：当前 Task 类型定义在 `shared/tool/types/task.rs`，因为 `build.rs` 需要从该位置生成 JSON Schema 供工具调用使用。目标态将类型定义迁移到 Task BC 所有权下（`task/published_language/`），`build.rs` 改为从新位置读取。迁移在 S5 执行，不阻塞本设计定稿。

### 2.3 序列化约定

- 所有 PL 类型实现 `Serialize + Deserialize`（serde）。
- 枚举使用 `#[serde(rename_all = "snake_case")]`。
- 向前兼容：新增字段使用 `#[serde(default)]`。
- ID 格式变更（自增 → UUIDv7）需版本化处理，旧 session 数据可恢复。

## 3. 与 Storage 的集成

### 3.1 分层关系

```
TaskPort (trait)                    ← 出站端口，消费方依赖此 trait
  └── TaskStore (Repository adapter) ← 实现 TaskPort，持有内存状态
        └── Storage (机制)            ← 原子写、损坏兜底
```

### 3.2 TaskStore 定位

TaskStore 是 Task BC 的 Repository adapter：

- **持有内存状态**：`HashMap<String, Task>` + `next_id` + `current_batch` + `batches`。
- **实现 TaskPort**：所有端口方法在 TaskStore 上实现。
- **使用 Storage 机制**：通过 Storage 的文件 I/O 进行持久化（经 `collect_snapshot` / `restore_snapshot` 驱动）。
- **不拥有数据本体**：数据所有权属 Task BC，Storage 只提供机制。

### 3.3 持久化模型

Task 的持久化不独立落盘，而是通过 Session 快照内嵌：

1. Context Management 触发 Session 落盘。
2. 经 `TaskPort::collect_snapshot()` 获取 `TaskSnapshot`。
3. `TaskSnapshot` 嵌入 Session 持久化 DTO。
4. Storage 将 Session DTO 原子写入 `~/.agents/sessions/`。
5. 恢复时反向：读取 Session DTO → 提取 `TaskSnapshot` → `TaskPort::restore_snapshot()`。

> **Decision**：Task **NEVER** 独立驱动持久化。所有落盘经 Session 快照路径，确保 Task 与 Session 状态一致。

## 4. 与 Context Management 的集成

### 4.1 快照组装

Session 落盘时的快照组装是 Context Map §8 定义的关键 ACL 位置之一：

```
Context Management                Task BC
     │                               │
     │  collect_snapshot()           │
     │ ────────────────────────────▶ │
     │                               │ 返回 TaskSnapshot
     │ ◀──────────────────────────── │
     │                               │
     │  嵌入 Session DTO             │
     │  写入磁盘                     │
```

### 4.2 快照恢复

```
Context Management                Task BC
     │                               │
     │  读取 Session DTO              │
     │  提取 TaskSnapshot             │
     │                               │
     │  restore_snapshot(snapshot)   │
     │ ────────────────────────────▶ │
     │                               │ 全量替换内存状态
     │ ◀──────────────────────────── │
     │  Ok(())                       │
```

### 4.3 边界约束

- Context Management **NEVER** 直接操作 Task 内部状态。
- 快照组装经 `TaskPort`，不共享内部结构。
- 恢复是全量替换，不做增量合并。

## 5. 与 Agent Runtime 的集成

### 5.1 Runtime 消费模式

Agent Runtime 通过 `TaskPort` 管理 Task 作为自身执行规划的投影：

| Runtime 动作 | TaskPort 调用 |
|---|---|
| 开始多步工作 | `create_list` + `create` × N |
| 开始执行某任务 | `update` → `transition_to(InProgress)` |
| 任务完成 | `update` → `transition_to(Completed)` |
| 添加依赖 | `update` → `add_dependency` |
| 检查是否可执行 | `is_blocked` |
| 批次完成 | `complete_list` |

### 5.2 边界约束

- Runtime **NEVER** 守护 Task 状态机不变量——不变量由 Task BC 独占。
- Runtime **NEVER** 直接读写 Task 字段——经 `TaskPort::update` 闭包调用聚合方法。
- Runtime 通过领域事件投影 Task 状态到 TUI，TUI 不自行推导。

## 6. 相关文档

- Task 领域模型：[01-domain-model.md](01-domain-model.md)
- 模块入口：[README.md](README.md)
- Context Map：[../../01-system/03-context-map.md](../../01-system/03-context-map.md) §4 / §6 / §7 / §8 / §10
- Runtime 端口与装配：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Context Management Session：[../context-management/01-session.md](../context-management/01-session.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：TaskPort 定义、PL 清单与发布位置、Storage 集成、跨 BC 快照组装 | #791 |
