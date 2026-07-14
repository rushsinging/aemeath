# Context Management · Memory 注入

> 层级：02-modules / context-management（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#786（S2）
> 本文定义 MemoryPort OHS——记忆检索与注入的单一入口、注入评分算法、semantic retrieval 演进路径、Reflection 集成与 `top_for_inject` 退役。

## 1. 定位

Memory 注入是 ContextPort `build_window` 的内部步骤之一：

```
build_window
  ├─ L1-L4 compact 管线
  ├─ memory 注入（MemoryPort）  ← 本文
  ├─ prompt 组装（PromptPort）
  └─ → ContextWindow.system_blocks + messages
```

- **MemoryPort 是独立端口**：被 ContextPort（注入）、Tool BC（Memory tool）、Reflection（写入）三方消费
- **Memory BC 属支撑域**：持久化在 Storage，但检索/注入逻辑归 Context Management
- **Sub Run 使用 `NoOpMemoryPort`**：Sub 不注入 memory，不写 reflection

## 2. MemoryPort trait

```rust
trait MemoryPort: Send + Sync {
    /// 检索注入用记忆（readonly，不 touch access count）
    fn retrieve_for_inject(&self, query: &MemoryQuery) -> Vec<MemoryEntry>;

    /// 写入记忆（reflection 产出 / Memory tool 写入）
    fn write(&self, entry: MemoryEntry) -> WriteResult;

    /// 搜索记忆（LLM tool 调用用，支持 substring 匹配）
    fn search(&self, query: &str, limit: usize) -> Vec<MemoryEntry>;

    /// 删除记忆
    fn delete(&self, id: &MemoryId) -> bool;

    /// 标记过期
    fn mark_outdated(&self, id: &MemoryId);
}

struct MemoryQuery {
    /// 注入条数上限
    limit: usize,                        // 默认 5
    /// 过滤 layer（Global / Project）
    layer: Option<MemoryLayer>,
    /// 过滤 category
    category: Option<String>,
}

enum WriteResult {
    Added,
    Merged,                              // Jaccard ≥ threshold，合并 tags
    NeedsEviction,                       // 达到 max_entries，需 eviction
}
```

### 2.1 替代关系

| 现状 | 目标 |
|---|---|
| runtime 直接 `MemoryStore::new(...)` × 5 处 | `MemoryPort` trait，由 Composition Root 注入 |
| `top_for_inject_readonly(limit)` | `MemoryPort.retrieve_for_inject(query)` |
| `top_for_inject(limit)`（mutating） | **删除**（见 §6） |
| `MemoryStore::add/delete/update/pin` | `MemoryPort.write/delete/mark_outdated` |

### 2.2 NoOpMemoryPort

```rust
struct NoOpMemoryPort;

impl MemoryPort for NoOpMemoryPort {
    fn retrieve_for_inject(&self, _: &MemoryQuery) -> Vec<MemoryEntry> { vec![] }
    fn write(&self, _: MemoryEntry) -> WriteResult { WriteResult::Added }
    fn search(&self, _: &str, _: usize) -> Vec<MemoryEntry> { vec![] }
    fn delete(&self, _: &MemoryId) -> bool { false }
    fn mark_outdated(&self, _: &MemoryId) {}
}
```

## 3. 注入管线

### 3.1 流程

```
ContextPort.build_window
  │
  ├─ MemoryPort.retrieve_for_inject(MemoryQuery { limit: 5, .. })
  │   ├─ 读取 active entries（Global + Project layer）
  │   ├─ 计算 injection_score
  │   ├─ 按 score 降序排序
  │   ├─ 截取 top N
  │   └─ 返回 Vec<MemoryEntry>（不 touch，不写盘）
  │
  ├─ 渲染为 SystemBlock
  │   ┌─────────────────────────────────────┐
  │   │ <memory-context>                    │
  │   │ - [Category] content      (★ if pinned) │
  │   │ - [Category] content                │
  │   │ </memory-context>                   │
  │   └─────────────────────────────────────┘
  │
  └─ push 到 ContextWindow.system_blocks（属于 cacheable_prefix，通过 entry fingerprint 检测变化）
```

### 3.2 注入时机

- **每轮 LLM 调用前**：`build_window` 时注入
- **属于 cacheable_prefix**：memory 内容不变时命中 prompt cache；reflection 写入新 memory 时 fingerprint 变化 → cache miss 一次 → 下一轮恢复命中（见 [04-prompt-guidance.md](04-prompt-guidance.md) §3.2）

### 3.3 注入条件

```rust
if config.memory.enabled && config.memory.inject_count > 0 {
    let query = MemoryQuery { limit: config.memory.inject_count, ..Default::default() };
    if let Some(block) = build_memory_block(port.retrieve_for_inject(&query)) {
        window.system_blocks.push(block);
    }
}
```

## 4. 注入评分算法

### 4.1 injection_score

```rust
fn injection_score(entry: &MemoryEntry) -> i64 {
    let mut score: i64 = 0;

    // 基础分
    score += entry.access_count as i64 * 100;

    // recency（越近越高）
    let age_secs = now() - entry.accessed_at;
    let recency = match age_secs {
        0..=3600 => 1000,           // 1 小时内
        3601..=86400 => 500,        // 1 天内
        86401..=604800 => 200,      // 1 周内
        _ => 50,                    // 更早
    };
    score += recency;

    // pinned 加权
    if entry.pinned { score += 10_000; }

    // 过期惩罚
    if entry.ttl_expired { score -= 5_000; }
    if entry.outdated { score -= 2_000; }

    score
}
```

### 4.2 排序与截取

```rust
fn retrieve_for_inject(&self, query: &MemoryQuery) -> Vec<MemoryEntry> {
    let mut entries: Vec<_> = self.read_active_entries()
        .into_iter()
        .filter(|e| match &query.layer {
            Some(l) => e.layer == *l,
            None => true,
        })
        .filter(|e| match &query.category {
            Some(c) => e.category == *c,
            None => true,
        })
        .collect();

    entries.sort_by(|a, b| injection_score(b).cmp(&injection_score(a)));
    entries.truncate(query.limit);
    entries
}
```

### 4.3 评分设计依据

| 因子 | 权重 | 理由 |
|---|---|---|
| pinned | +10,000 | 用户显式标记重要，绝对优先 |
| access_count × 100 | 100/次 | 高频访问的记忆更有用 |
| recency | 50–1,000 | 近期记忆更相关 |
| ttl_expired | -5,000 | 过期记忆大幅降权（但不删除） |
| outdated | -2,000 | 标记过期但不如 ttl 严重 |

**默认 inject_count = 5 的理由**：当前按 recency/pin 排序，相关性不高，5 条 ≈ 300 token。#551 落地语义检索后应提高此值或改为动态决定。

## 5. Memory Store 生命周期

### 5.1 持久化

| 维度 | 说明 |
|---|---|
| 格式 | `serde_json::to_string_pretty` 数组 |
| 布局 | `~/.agents/memory/{_global,_global_archive,<project>,<project>_archive}.json` |
| Project 身份 | `canonicalize(project_root)` → 去前导 `/` → `/` 替换为 `-` |
| 容量 | 100 entries/layer（`max_entries`） |
| 去重 | Jaccard ≥ 0.8 over alphanumeric tokens |
| 驱逐 | `eviction_score = access_count*10 + recency_weight`；pinned 不驱逐 |
| 写入 | 全文件重写（无 append，无 lock，无 atomic-rename） |

### 5.2 Factory 模式

**当前问题**：5 处重复 `MemoryStore::new(memory_base_dir(), project_file_name(project_root), 100, 0.8)`。

**目标**：

```rust
struct MemoryPortFactory {
    base_dir: PathBuf,
    config: MemoryConfig,
}

impl MemoryPortFactory {
    fn for_project(&self, project_root: &Path) -> Box<dyn MemoryPort> {
        let store = MemoryStore::new(
            self.base_dir.clone(),
            project_file_name(project_root),
            self.config.max_entries,
            self.config.similarity_threshold,
        );
        Box::new(store)
    }

    fn no_op() -> Box<dyn MemoryPort> {
        Box::new(NoOpMemoryPort)
    }
}
```

由 Composition Root 持有 `MemoryPortFactory`，各消费方通过工厂获取实例，不再自行构造。

### 5.3 project_root 一致性

**当前问题**：`memory_inject` 使用 `WorkspaceRead::initial_cwd`（stable project root），但 `MemoryTool` 使用 `ToolExecutionContext.cwd`（runtime CWD）。worktree 场景下两者可能不同，导致同一项目的 memory 分裂到两个文件。

**目标**：
- **所有 memory 操作统一使用 project_root**（`WorkspaceRead::initial_cwd`）
- `MemoryTool` 的 handler 从 `ToolExecutionContext` 获取 project_root 而非 runtime CWD
- `MemoryPortFactory.for_project(project_root)` 统一构造

### 5.4 并发安全

**当前问题**：`MemoryStore` 的 `add/delete/update` 都是 read-modify-write 全文件，无 lock，无 atomic-rename。Reflection（异步）和 LLM turn（同步）可能并发写同一文件。

**目标**（v0.1.0 不实现，记录为已知 gap）：
- 引入 `RwLock` 或 `Mutex` 保护文件操作
- 或改为 atomic write（write to temp + rename）

## 6. `top_for_inject` 退役

### 6.1 现状

| 函数 | 状态 | 调用方 |
|---|---|---|
| `top_for_inject(&mut self, limit)` | **生产死代码**——touches every entry（写盘），但无生产调用方 | 仅 2 个单元测试 |
| `top_for_inject_readonly(&self, limit)` | 生产路径 | `memory_inject::build_memory_block` |

### 6.2 退役路径

1. 将 `top_for_inject_readonly` 重命名为 `retrieve_for_inject`，纳入 `MemoryPort` trait
2. 删除 `top_for_inject`（mutating 版本）
3. 删除对应测试 `test_memory_store_top_for_inject_touches_entries`
4. 保留 `test_memory_store_search_returns_top_for_inject_results`，重命名引用

### 6.3 为什么 mutating 版本要删除

- **每轮 touch 导致所有 entry 立即变"fresh"**——injection_score 的 recency 权重失效
- **每轮写盘**——性能损耗
- **破坏幂等性**——相同输入的 retrieve 操作产生 side effect

## 7. Memory Tool（LLM 调用）

Memory tool 是 LLM 主动调用的 tool，与自动注入是**互补关系**：

| 维度 | 自动注入（MemoryPort） | Memory Tool（LLM 调用） |
|---|---|---|
| 触发 | 每轮 build_window | LLM 决定调用 |
| 检索 | `retrieve_for_inject`（score 排序） | `search`（substring 匹配） |
| 条数 | inject_count（默认 5） | LLM 指定 limit |
| 写入 | 不写入（只读） | 可写入（`Memory.tool` write 操作） |
| 端口 | MemoryPort | MemoryPort（同一 trait） |

**目标**：Memory tool 的 handler 也通过 `MemoryPort` trait 操作，不直接构造 `MemoryStore`。

## 8. Semantic Retrieval 演进（#551）

### 8.1 当前局限

- `search()` 是纯 substring 匹配（case-insensitive）
- `retrieve_for_inject` 是纯 recency/pin 排序，无相关性
- 无 embedding / BM25 / TF-IDF

### 8.2 演进路径

| 阶段 | 方案 | 成本 | 收益 |
|---|---|---|---|
| Phase 1 | TF-IDF + cosine similarity | 低（纯 Rust 实现） | 中（比 substring 好，比 embedding 差） |
| Phase 2 | BM25 | 低 | 中（比 TF-IDF 更适合短文本） |
| Phase 3 | Embedding（外部 API） | 高（需 embedding model + 向量存储） | 高（语义匹配） |

### 8.3 Phase 1 目标设计（v0.1.0 之后）

```rust
trait MemoryRetrieval {
    /// 语义检索（#551 目标）
    fn retrieve_semantic(&self, query: &str, limit: usize) -> Vec<MemoryEntry>;
}

impl MemoryRetrieval for MemoryStore {
    fn retrieve_semantic(&self, query: &str, limit: usize) -> Vec<MemoryEntry> {
        let query_vec = tfidf::vectorize(query, &self.idf_map);
        let mut scored: Vec<_> = self.read_active_entries()
            .into_iter()
            .map(|e| {
                let entry_vec = tfidf::vectorize(&e.content, &self.idf_map);
                let sim = cosine_similarity(&query_vec, &entry_vec);
                (e, sim)
            })
            .filter(|(_, sim)| *sim > self.similarity_threshold)
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
        scored.into_iter().map(|(e, _)| e).take(limit).collect()
    }
}
```

### 8.4 inject_count 动态化

#551 落地后，`inject_count` 应改为动态决定：

```rust
fn dynamic_inject_count(query: &MemoryQuery, entries: &[MemoryEntry]) -> usize {
    // 基于 token 预算：每条 entry ≈ 60 tokens，总预算 500 tokens
    let token_budget = 500;
    let avg_tokens_per_entry = 60;
    let max_by_budget = token_budget / avg_tokens_per_entry;

    // 基于 relevance：高相关性时多注入，低相关性时少注入
    let high_relevance_count = entries.iter()
        .filter(|e| e.relevance_score > 0.7)
        .count();

    max_by_budget.min(high_relevance_count.max(query.limit))
}
```

## 9. Reflection 集成

### 9.1 PreCompact Reflection

auto-compact 前触发——抢救关键信息到 Memory：

```
auto_compact
  ├─ run_precompact_reflection(messages)
  │   ├─ LLM 分析将 compact 的消息，提取值得记忆的信息
  │   ├─ 产出 MemorySuggestion
  │   └─ MemoryPort.write(entry)  ← 写入 memory
  │
  ├─ compact_messages_with_llm(...)
  └─ apply_compact_outcome(...)
```

### 9.2 周期性 Reflection

```rust
struct ReflectionConfig {
    enabled: bool,                 // 默认 true
    interval_turns: usize,         // 默认 10
    auto_apply: bool,              // 默认 false
}
```

- 每 N 轮触发一次 reflection
- LLM 分析近期对话，产出 MemorySuggestion
- `auto_apply = true` 时自动写入 Memory
- `auto_apply = false` 时需用户确认

### 9.3 Reflection → Memory → 注入闭环

```
Reflection 产出 → MemoryPort.write → 下轮 build_window → retrieve_for_inject → SystemBlock → LLM
```

- Reflection 写入的 memory 在**下一轮** build_window 时被检索注入
- PreCompact reflection 写入的 memory 在 **compact 后第一轮**被检索注入（因为 compact 改变了 messages，触发 fingerprint 变化）

## 10. 现状端口缺口

| 目标 | 现状 | 迁移动作 |
|---|---|---|
| `MemoryPort` trait | ❌ 无，runtime 直接调 `MemoryStore` | 抽 trait，实现移到 adapter |
| `MemoryPortFactory` | ❌ 5 处重复构造 | 工厂模式，Composition Root 持有 |
| `top_for_inject` 退役 | ⚠️ 生产死代码 | 删除 mutating 版本 |
| project_root 一致性 | ⚠️ inject 用 initial_cwd, tool 用 cwd | 统一使用 project_root |
| 语义检索 | ❌ 纯 substring | #551 演进路径 |
| 并发安全 | ❌ 无 lock | 已知 gap，v0.1.0 不修 |
| `trait_memory.rs` 误导命名 | ⚠️ 只含 `list_reminders` | 重命名或拆分 |

## 11. 相关文档

- Compact 家族：[02-compact.md](02-compact.md)
- Token Budget：[03-token-budget.md](03-token-budget.md)
- Prompt & Guidance：[04-prompt-guidance.md](04-prompt-guidance.md)
- Runtime 端口（MemoryPort = Runtime 出站端口）：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- 上下文地图（Memory BC = 支撑域）：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：MemoryPort trait、注入管线、评分算法、top_for_inject 退役、semantic retrieval 演进、Reflection 集成 | #786 |
