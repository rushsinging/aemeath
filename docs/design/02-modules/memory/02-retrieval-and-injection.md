# Memory · 检索与注入

> 层级：02-modules / memory（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#789（S2）
> 本文定义 Memory BC 的检索策略、注入格式、`similarity_threshold` 双重用途，以及 #551 语义检索升级路径。**只描述目标态**；现状子串匹配的差距记入 `03-engineering/03-migration-governance.md`。

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

### Tier 0 — 子串匹配（现状）

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

### Tier 1 — BM25 关键词相关性（v0.1.0 目标）

```rust
struct BM25Index {
    docs: Vec<Vec<String>>,        // 分词后的文档
    avg_doc_len: f64,
    doc_freqs: HashMap<String, usize>,
    k1: f64,                       // 默认 1.2
    b: f64,                        // 默认 0.75
}

impl BM25Index {
    fn build(entries: &[MemoryEntry]) -> Self;
    fn score(&self, query: &str, doc_idx: usize) -> f64;
    fn search(&self, query: &str, entries: &[MemoryEntry], limit: usize) -> Vec<(usize, f64)>;
}
```

- **成本**：纯 Rust 实现，无外部依赖。
- **收益**：按相关性排序（TF-IDF + 文档长度归一化），比子串匹配显著提升检索质量。
- **`similarity_threshold` 接入**：BM25 分数归一化到 [0, 1] 后，低于 threshold 的结果排除。
- **中文支持**：分词需兼顾中文（按字符 bigram 或接入简易分词）。
- **构建时机**：首次检索时构建索引并缓存，写入/归档后失效。

### Tier 2 — Embedding 语义检索（v0.2.0+，方向预留）

- 需引入 embedding 模型（本地如 `all-MiniLM-L6-v2` 或远程 API）。
- 存储格式变更：MemoryEntry 需增加 `embedding: Option<Vec<f8>>` 字段。
- 写入时计算 embedding 并存储；检索时计算 query embedding 做 cosine similarity。
- **前置条件**：#549（Memory 注入）落地后验证实际收益，再决定是否推进（见 #551）。

### 升级路径

```text
Tier 0（现状）           Tier 1（v0.1.0 目标）         Tier 2（v0.2.0+）
子串匹配        ──→     BM25 关键词相关性     ──→     Embedding 语义检索
无排序                   归一化分数排序                cosine similarity
threshold 不生效         threshold 过滤                threshold 过滤
零依赖                   纯 Rust                      需模型服务
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

## 4. similarity_threshold 双重用途

```rust
struct MemoryConfig {
    similarity_threshold: f64,    // 默认 0.8，范围 [0, 1]
}
```

| 用途 | 语义 | Tier 0 | Tier 1 | Tier 2 |
|---|---|---|---|---|
| **去重** | 写入时 Jaccard ≥ threshold → 合并 | ✅ | ✅ | ✅ |
| **检索过滤** | 检索相关性 < threshold → 排除 | ❌ 不生效 | ✅ BM25 归一化分数 | ✅ cosine similarity |

Tier 1 落地时，BM25 分数归一化到 [0, 1]：
- 归一化方式：`score / max_score`（相对归一化）。
- threshold = 0.8 意味着只保留与最高分条目相似度 ≥ 80% 的结果。
- 可配置调整：降低 threshold → 更多结果但质量参差；提高 threshold → 更少但更精准。

## 5. inject_count 配置

```rust
struct MemoryConfig {
    inject_count: usize,    // 默认 5
}
```

- **Tier 0**：默认 5（recency/pin 排序，相关性不高，保守注入 ≈ 300 token）。
- **Tier 1 落地后**：可提高默认值（相关性排序后注入质量提升，可注入更多条目）或改为动态决定（按 token 预算反推条数）。
- **动态注入**（未来方向）：Context Management 根据 token budget 动态决定注入条数，Memory BC 只提供排序后的候选池。

## 6. 检索不变量

| # | 不变量 | 说明 |
|---|---|---|
| R1 | retrieve_for_inject / search / list / stats **不 touch、不落盘** | 查询只读已验证内存 state，避免排序与 revision 漂移 |
| R2 | search **可跨 active + archive** | 归档条目仍可由显式 search 检索 |
| R3 | TTL-expired 条目 **不参与注入** | 在 injection_score 前由 eligibility 硬过滤 |
| R4 | outdated 条目 **不参与注入但可显式检索** | 状态通过 search hit metadata 表达，NEVER 静默丢失 |
| R5 | pinned 只在 eligible 集合中获得最高优先级 | pinned 不能绕过 outdated / TTL eligibility |
| R6 | search 平分使用 search_tie_break_score | archived/outdated/TTL hit NEVER 调 injection_score |

## 7. 相关文档

- 模块入口：[README.md](README.md)
- 领域模型（scoring 函数）：[01-domain-model.md](01-domain-model.md) §4
- Reflection 引擎：[03-reflection.md](03-reflection.md)
- 端口与适配器（MemoryPort.search）：[04-ports-and-adapters.md](04-ports-and-adapters.md)
- Context Management（注入位置归 CM）：[../context-management/01-session.md](../context-management/01-session.md)
- #551 Memory search 升级：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：检索模式、BM25 分层(#551)、注入格式、similarity_threshold 双重用途、注入职责边界 | #789 |
| 2026-07-17 | 对齐 #895：旧 top query 统一为只读 `retrieve_for_inject`；outdated/TTL 改为 eligibility 硬过滤；显式 search 改用 relevance + 独立 tie-break，并由 Context 独占 render | #895 |
