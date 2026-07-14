# Memory · 端口与适配器

> 层级：02-modules / memory（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#789（S2）
> 本文定义 Memory BC 的对外端口、NoOpMemory（Sub）、Storage 边界、Composition Root 装配与现状缺口。**只描述目标态**；现状缺口记入 `03-engineering/03-migration-governance.md`。

## 1. MemoryPort

`MemoryPort` 是 Memory BC 对外的唯一出站端口（OHS），覆盖记忆的全部操作。Runtime 和 CLI/TUI 经此端口消费 Memory 能力。

```rust
trait MemoryPort: Send + Sync {
    // —— 检索 ——
    fn top_for_inject(&self, limit: usize) -> Vec<MemoryEntry>;
    fn search(&self, query: &str, limit: usize) -> Vec<MemoryEntry>;

    // —— 写入 ——
    fn add(&self, entry: MemoryEntry) -> AddResult;
    fn update(&self, id: &str, content: &str);
    fn delete(&self, id: &str);
    fn pin(&self, id: &str, pinned: bool);
    fn mark_outdated(&self, id: &str);

    // —— 归档 / 淘汰 ——
    fn archive(&self, ids: &[String]);
    fn compact(&self) -> CompactResult;

    // —— 管理 / 查询 ——
    fn list(&self, layer: Option<MemoryLayer>) -> Vec<MemoryEntry>;
    fn stats(&self) -> MemoryStats;
}
```

### 设计约束

- **MUST NOT** 返回内部 `MemoryStore` 实例或文件路径。
- **MUST NOT** 暴露文件 I/O 细节（路径、序列化格式）。
- **MUST NOT** 依赖 ProviderPort（Reflection 的 LLM 调用由 Runtime 编排）。
- **MUST** `top_for_inject` 不 touch（只读，避免排序漂移）。
- **MUST** `search` 跨 active + archive 检索。
- **MUST** `compact` 跳过 pinned 条目。

## 2. ReflectionPromptPort

`ReflectionPromptPort` 把 Reflection 的领域逻辑暴露给 Runtime 编排。Memory BC 自身不调 LLM。

```rust
trait ReflectionPromptPort: Send + Sync {
    /// 构建反思 prompt（纯函数）
    fn build_prompt(
        &self,
        project_memory: &str,
        recent_summary: &str,
        lang: &str,
    ) -> String;

    /// 解析 LLM 返回的 JSON
    fn parse_output(&self, raw: &str) -> Result<ReflectionOutput, ReflectionError>;

    /// 应用反思结果（写入 suggestion + 标记过期）
    fn apply_output(
        &self,
        output: &ReflectionOutput,
    ) -> Result<ReflectionApplyResult, ReflectionError>;

    /// 格式化反思结果供 TUI 展示
    fn format_output(&self, output: &ReflectionOutput, lang: &str) -> String;

    /// 读取当前项目记忆摘要
    fn project_memory_summary(&self) -> String;

    /// 从消息列表构建对话摘要
    fn recent_messages_summary(&self, messages: &[Message], max_chars: usize) -> String;
}
```

### 为什么不合并到 MemoryPort

1. **职责分离**：MemoryPort 管记忆 CRUD + 检索；ReflectionPromptPort 管反思 prompt/output/apply。两者消费方不同（MemoryPort 被 context_coordination 和 slash 命令消费；ReflectionPromptPort 被 Runtime reflection 编排消费）。
2. **Sub 隔离**：Sub Run 装配 `NoOpMemory`（MemoryPort 的空实现），但 Reflection 在 Sub 中完全不触发——不需要 NoOpReflection。
3. **演进独立**：检索升级（BM25/embedding）和 Reflection prompt 优化可以独立演进。

## 3. NoOpMemory（Sub Run）

Sub Run 装配 `NoOpMemory`——所有方法返回空值/空集合，不读写不报错：

```rust
struct NoOpMemory;

impl MemoryPort for NoOpMemory {
    fn top_for_inject(&self, _: usize) -> Vec<MemoryEntry> { Vec::new() }
    fn search(&self, _: &str, _: usize) -> Vec<MemoryEntry> { Vec::new() }
    fn add(&self, entry: MemoryEntry) -> AddResult { AddResult::Added { id: entry.id } }
    fn update(&self, _: &str, _: &str) {}
    fn delete(&self, _: &str) {}
    fn pin(&self, _: &str, _: bool) {}
    fn mark_outdated(&self, _: &str) {}
    fn archive(&self, _: &[String]) {}
    fn compact(&self) -> CompactResult { CompactResult { archived: 0, remaining: 0 } }
    fn list(&self, _: Option<MemoryLayer>) -> Vec<MemoryEntry> { Vec::new() }
    fn stats(&self) -> MemoryStats { MemoryStats { global_count: 0, global_archive_count: 0, project_count: 0, project_archive_count: 0, reminders_count: 0 } }
}
```

- Sub 不读记忆（`top_for_inject` 返回空）。
- Sub 不写记忆（`add` 静默丢弃，返回 `Added` 不报错）。
- Sub 不触发 Reflection（Runtime 根据 `MemoryMode::Disabled` 跳过）。
- Main 可通过 `share_memory` 参数显式给 Sub 开启注入（此时 Sub 的 MemoryPort 装配真实实现而非 NoOp）。

## 4. Storage 边界

### 现状问题

当前 `MemoryStore`（在 `agent/features/storage` crate）混合了：
- **领域逻辑**：scoring、dedup（jaccard）、检索过滤、排序——这些是 Memory BC 的业务逻辑。
- **文件 I/O**：read_entries / write_entries / 路径管理——这些是 Storage BC 的物理机制。

### 目标态拆分

| 层 | 归属 | 职责 |
|---|---|---|
| 领域模型 | Memory BC（`share::memory`）| MemoryEntry、枚举、scoring、dedup、format——纯数据 + 纯函数 |
| 领域服务 | Memory BC（`memory::api`）| MemoryPort 实现：检索、去重判定、淘汰候选、apply |
| 文件 I/O | Storage adapter | read_entries / write_entries / 路径解析 / 原子写 / 损坏兜底 |

```text
Memory BC                        Storage BC
┌─────────────────┐              ┌──────────────────┐
│ MemoryPort impl │ ──读写委托──▶ │ MemoryStorageAdapter │
│ (领域逻辑)       │              │ (文件 I/O)        │
│ - scoring       │              │ - read_entries    │
│ - dedup         │              │ - write_entries   │
│ - retrieval     │              │ - path resolve    │
│ - apply         │              │ - atomic write    │
└─────────────────┘              └──────────────────┘
```

MemoryPort 实现持有 `MemoryStorageAdapter`（或 trait），领域逻辑在 Memory 侧，I/O 委托给 Storage 侧。

### 迁移路径

现状 `MemoryStore` 同时做领域逻辑和 I/O。迁移分两步：
1. **S5**：抽 `MemoryPort` trait，`MemoryStore` 实现之（领域逻辑 + I/O 混合暂留）。
2. **S7**：拆分 `MemoryStore` → Memory 侧的 `MemoryService`（领域逻辑）+ Storage 侧的 `MemoryStorageAdapter`（文件 I/O）。

## 5. Composition Root 装配

```rust
// Composition Root
fn assemble_memory(spec: &RunSpec, root: &CompositionRoot) -> Arc<dyn MemoryPort> {
    match spec.memory {
        MemoryMode::Enabled => {
            let storage = root.memory_storage();    // Storage adapter
            Arc::new(MemoryService::new(storage, root.config_snapshot()))
        }
        MemoryMode::Disabled => Arc::new(NoOpMemory),
    }
}

fn assemble_reflection(root: &CompositionRoot) -> Arc<dyn ReflectionPromptPort> {
    Arc::new(ReflectionEngine::new(root.config_snapshot()))
}
```

- **Main Run**：装配真实 `MemoryService` + `ReflectionEngine`。
- **Sub Run（Disabled）**：装配 `NoOpMemory`；Reflection 不触发（Runtime 按 `MemoryMode::Disabled` 跳过）。
- **Sub Run（Enabled，Main 显式开启）**：装配真实 `MemoryService`。

### 装配约束

- **MUST** MemoryPort 实例由 Composition Root 构造，不在核心或适配器内私自 `new`。
- **MUST** MemoryStorageAdapter 由 Composition Root 注入 MemoryService。
- **MUST NOT** MemoryService 直接 `std::fs::read` / `std::fs::write`——经 Storage adapter。

## 6. 持久化格式

### 文件布局

```text
~/.agents/memory/
├── _global.json              # Global 层 active 条目
├── _global_archive.json      # Global 层归档条目
├── {project_file_name}.json       # Project 层 active 条目
└── {project_file_name}_archive.json  # Project 层归档条目
```

### 序列化格式

- JSON 数组：`Vec<MemoryEntry>` 序列化为 `[...]`。
- 枚举使用 `snake_case`：`"global"` / `"project"` / `"fact"` / `"decision"` / `"llm"` 等。
- 可选字段使用 `#[serde(default, skip_serializing_if = "Option::is_none")]`。
- `tags` 使用 `#[serde(default)]`。

### project_file_name

从 cwd 计算：取项目根目录名（或 hash），确保同一项目的记忆跨会话一致。路径解析逻辑归 Storage adapter。

## 7. 现状缺口

| # | 缺口 | 现状 | 目标 | 迁移阶段 |
|---|---|---|---|---|
| M1 | **无 MemoryPort trait** | Runtime 直调 `MemoryStore` 具体类型 | 抽 `MemoryPort` trait，实现移到 adapter | S5 |
| M2 | **领域逻辑与 I/O 混合** | `MemoryStore` 同时做 scoring/dedup/retrieval 和文件读写 | 拆分 MemoryService（领域）+ MemoryStorageAdapter（I/O）| S7 |
| M3 | **检索为子串匹配** | `entry_matches` 朴素小写 contains | Tier 1 BM25 关键词相关性排序 | #551 |
| M4 | **similarity_threshold 仅用于去重** | 检索不接入 threshold | 检索也用 threshold 过滤低相关结果 | #551 |
| M5 | **Reflection 代码在 Runtime** | `runtime/business/reflection/` 含 prompt/output/apply | 领域逻辑迁回 Memory BC，Runtime 只编排 | S5 |
| M6 | **无 ReflectionPromptPort** | Runtime 直接调 reflection 模块函数 | 抽 trait，Memory BC 暴露领域服务 | S5 |
| M7 | **memory_inject 硬编码参数** | `open_memory_store` 硬编码 `max_entries=100, threshold=0.8` | 从 ConfigSnapshot 读取 | S5 |
| M8 | **SessionReminder 在 Memory** | `share::memory::session_reminder` | 迁移到 Context Management | S5/S7 |
| M9 | **MemoryStore 触摸在注入路径** | `top_for_inject` 会 touch（已修复为 `top_for_inject_readonly`）| 确保 `top_for_inject` 只读 | ✅ 已修复 |
| M10 | **无 NoOpMemory** | Sub 无 Memory 隔离 | Sub 装配 NoOpMemory | S3/S5 |

## 8. 守卫映射

以下规则由架构守卫脚本在 CI / Stop hook 拦截（守卫注册表见 [../../03-engineering/01-architecture-guards.md](../../03-engineering/01-architecture-guards.md)）：

```text
Rule: memory-port-owned-by-composition
Scan: production Rust AST/path references to MemoryService::new / MemoryStore::new
Allow: agent/composition/** only
Deny: agent/features/** domain/application modules and apps/**
```

- MemoryPort 实例构造只在 Composition Root。
- 领域/应用模块不得直接 `new` MemoryStore / MemoryService。

## 9. 相关文档

- 模块入口：[README.md](README.md)
- 领域模型：[01-domain-model.md](01-domain-model.md)
- 检索与注入：[02-retrieval-and-injection.md](02-retrieval-and-injection.md)
- Reflection 引擎：[03-reflection.md](03-reflection.md)
- Runtime 端口（MemoryPort 消费方）：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Context Map（Memory 集成关系）：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- 依赖规则：[../../01-system/05-dependency-rules.md](../../01-system/05-dependency-rules.md)
- 迁移治理：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：MemoryPort trait、ReflectionPromptPort、NoOpMemory、Storage 边界、Composition Root、现状缺口 M1-M10 | #789 |
