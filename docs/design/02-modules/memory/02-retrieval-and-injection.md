# Memory · 检索与注入

> 层级：02-modules / memory（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#789（S2）
> 本文定义 Memory BC 的检索策略、注入格式、显式检索相关性，以及 #551 的 Tier 1 词法检索。**只描述目标态**。

## 1. 检索模式

Memory BC 提供两种检索模式，分别服务不同消费场景：

| 模式 | 方法 | 场景 | 排序依据 |
|---|---|---|---|
| **自动注入** | `retrieve_for_inject(&MemoryQuery)` | 每轮 LLM 调用前自动注入 | eligibility 硬过滤 + injection_score |
| **Query-aware 检索** | `search(&MemorySearchQuery)` | 用户 `/memory search` 或管理查询 | relevance 主排序 + search_tie_break_score |

### 1.1 自动注入检索

```rust
fn retrieve_for_inject(&self, query: &MemoryQuery) -> MemorySearchResult;
```

- 跨 Global + Project 两层 active 条目合并。
- 在评分前硬过滤 outdated 与 TTL-expired；pinned **NEVER** 绕过 eligibility。
- 对 eligible 集合按 `injection_score` 降序排列。
- 取 query.limit 条，返回 `mode = InjectionPriority`，hit 的 relevance 为 `None`。
- **不 touch、不落盘**——避免每轮注入导致排序漂移。

**设计理由**：注入是每轮 LLM 调用都会发生的高频纯查询。它只读 open 时已验证的内存 state；访问统计若未来需要，必须另设显式、fallible mutation。

### 1.2 Query-aware 检索

```rust
fn search(&self, query: &MemorySearchQuery) -> MemorySearchResult;
```

- 可按 `include_archive` 跨 active + archive（Global + Project）检索。
- archived、outdated 与 TTL-expired 条目仍可由用户显式检索，并通过 hit metadata 无损表达状态。
- 先按 query relevance 降序排列；仅 relevance 平分时使用 `search_tie_break_score`，**NEVER** 调用要求 injection eligibility 的 `injection_score`。
- search 同样不 touch、不落盘；返回 `mode = ExplicitSearch` 且每个 hit 携 relevance。

## 2. 检索分层（#551）

### Tier 0 — 子串匹配（已退役）

```rust
fn entry_matches(entry: &MemoryEntry, query: &str) -> bool {
    entry.content.to_lowercase().contains(query)
        || entry.tags.iter().any(|tag| tag.to_lowercase().contains(query))
        || format!("{:?}", entry.category).to_lowercase().contains(query)
        || format!("{:?}", entry.layer).to_lowercase().contains(query)
}
```

- **成本**：零依赖。
- **问题**：无相关性排序（命中即返回）、无模糊匹配、`similarity_threshold` 配置项不生效。
- **适用**：条目数少（< 100）时够用。

### Tier 1 — 确定性 BM25 词法相关性（v0.1.0）

生产实现由 Memory domain 的单一 `rank_explicit_search` 路径承担，`MemoryService` 与 `InMemoryMemory` 两种 backing 均复用该函数：

- 对 query、content、tags、category、layer 做小写字母数字分词；空 query 返回空结果。
- 使用 BM25（`k1 = 1.2`、`b = 0.75`）计算词项相关性。
- content、tag、facet 分别采用 `3.0 / 2.0 / 1.0` 的字段权重，完整 content 精确匹配获得固定 boost。
- 只保留正相关结果；先按 relevance 降序，再按 `search_tie_break_score` 与 Memory id 稳定排序，因此同一 state/query 的结果确定。
- relevance 是显式检索的排序 metadata，不承诺跨不同 corpus 可直接比较，也不改变自动注入的 `InjectionPriority` 语义。
- active/archive、outdated、TTL-expired 状态不被静默过滤；它们随 structured hit 返回。
- 当前实现每次基于只读候选集构建轻量统计，不引入第二索引 backing、缓存失效协议或持久化格式变更。

### Tier 2 — Embedding 语义检索（v0.2.0+，方向预留）

- 需引入 embedding 模型（本地如 `all-MiniLM-L6-v2` 或远程 API）。
- 存储格式变更：MemoryEntry 需增加 `embedding: Option<Vec<f8>>` 字段。
- 写入时计算 embedding 并存储；检索时计算 query embedding 做 cosine similarity。
- **前置条件**：#549（Memory 注入）落地后验证实际收益，再决定是否推进（见 #551）。

### 升级路径

```text
Tier 0（已退役）         Tier 1（v0.1.0）              Tier 2（v0.2.0+）
子串匹配        ──→     BM25 词法相关性       ──→     Embedding 语义检索
无排序                   确定性分数排序                 cosine similarity
零依赖                   纯 Rust、无第二索引 backing    需模型服务
```

**v0.1.0 决策**：推进 Tier 1（BM25），暂不做 Tier 2。理由：
1. BM25 成本低（纯 Rust，无外部依赖），收益明显。
2. Embedding 需要模型服务 + 存储格式变更，投入大，需先验证 #549 落地后的实际收益。
3. `inject_count` 默认值（5）在 Tier 1 落地后可提高（从 recency 排序升级为相关性排序，注入质量提升）。

## 3. 注入格式

Memory BC 输出检索结果后，由 **Context Management** 决定注入位置和 token 预算。Memory BC 提供格式化辅助函数，但不决定注入策略。

### 注入内容格式

```text
<memory-context>
- ★ [Decision] 使用 JSON 文件存储 memory 配置
- [Pattern] compact 前触发 pre-compact reflection 保留记忆
- [Pitfall] 避免在 Sub Run 中读写 Memory（NoOpMemory）
</memory-context>
```

- `★` 前缀标记 pinned 条目。
- `[Category]` 标注记忆类型。
- content 为记忆内容正文。
- **不含** id / accessed_at / access_count / source 等元数据——这些是管理信息，不注入给 LLM。

### 注入职责边界

| 职责 | 归属 |
|---|---|
| 检索 top-N 条目 | Memory BC（`MemoryPort::retrieve_for_inject`）|
| 按条目顺序渲染 `<memory-context>` | Context Management |
| 决定注入位置（system block 顺序）| Context Management |
| Token 预算分配 | Context Management |
| 与 guidance / AGENTS.md / skill 的排序 | Context Management |
| 注入去重（跨轮避免重复注入相同条目）| Context Management |

Memory BC 只输出"这些条目值得注入，格式如下"；Context Management 决定"放哪、放多少、与什么排序"。

## 4. similarity_threshold 边界

`similarity_threshold` 继续只用于写入去重的 Jaccard 判断。Tier 1 BM25 relevance 未归一化，当前不复用该配置做检索过滤，避免把不同量纲强行绑定；若未来增加搜索 threshold，**MUST** 发布独立配置与分数语义，而不是复用写入去重阈值。

## 5. Memory Tool Published Language

`Memory` Tool 必须让模型明确区分两类状态：

- `global` / `project` 是持久化 Memory 层；分类固定为 `fact`、`decision`、`preference`、`pattern`、`pitfall`。
- `add_reminder` / `complete_reminder` 是当前 Session reminder，不写入持久化 Memory。
- input schema 对 action、layer、category、priority 发布枚举约束，而不是无边界字符串。
- `search` 的 typed result 返回 id、content、layer、category、tags、pinned、location、outdated、ttl_expired、relevance；`list` 返回完整 entries。由于 Tool pipeline 对 LLM 使用 text-first 投影，search/list 的 text **MUST** 同样保留有序条目与可管理完整 ID；structured data 服务 TUI/server，不能替代 LLM 文本契约。
- Reflection 写入的 `MemorySuggestion` 经同一个 `MemoryPort` 成为普通 `MemoryEntry`，因此无需修改 Reflection trigger/workflow 即可被 Tool search 检索。

## 6. inject_count 配置

```rust
struct MemoryConfig {
    inject_count: usize,    // 默认 5
}
```

- **Tier 0**：默认 5（recency/pin 排序，相关性不高，保守注入 ≈ 300 token）。
- **Tier 1 落地后**：可提高默认值（相关性排序后注入质量提升，可注入更多条目）或改为动态决定（按 token 预算反推条数）。
- **动态注入**（未来方向）：Context Management 根据 token budget 动态决定注入条数，Memory BC 只提供排序后的候选池。

## 7. 检索不变量

| # | 不变量 | 说明 |
|---|---|---|
| R1 | retrieve_for_inject / search / list / stats **不 touch、不落盘** | 查询只读已验证内存 state，避免排序与 revision 漂移 |
| R2 | search **可跨 active + archive** | 归档条目仍可由显式 search 检索 |
| R3 | TTL-expired 条目 **不参与注入** | 在 injection_score 前由 eligibility 硬过滤 |
| R4 | outdated 条目 **不参与注入但可显式检索** | 状态通过 search hit metadata 表达，NEVER 静默丢失 |
| R5 | pinned 只在 eligible 集合中获得最高优先级 | pinned 不能绕过 outdated / TTL eligibility |
| R6 | search 平分使用 search_tie_break_score | archived/outdated/TTL hit NEVER 调 injection_score |

## 8. 相关文档

- 模块入口：[README.md](README.md)
- 领域模型（scoring 函数）：[01-domain-model.md](01-domain-model.md) §4
- Reflection 引擎：[03-reflection.md](03-reflection.md)
- 端口与适配器（MemoryPort.search）：[04-ports-and-adapters.md](04-ports-and-adapters.md)
- Context Management（注入位置归 CM）：[../context-management/01-session.md](../context-management/01-session.md)
- #551 Memory search 升级：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-26 | 落地共享确定性 BM25 词法排序与 typed Memory Tool PL；明确 Reflection 无需修改、search relevance 不复用写入去重 threshold | Tier 1 retrieval |
| 2026-07-12 | 初稿：检索模式、BM25 分层、注入格式、similarity_threshold 双重用途、注入职责边界 | 初始设计 |
| 2026-07-17 | 对齐 #895：旧 top query 统一为只读 `retrieve_for_inject`；outdated/TTL 改为 eligibility 硬过滤；显式 search 改用 relevance + 独立 tie-break，并由 Context 独占 render | #895 |
