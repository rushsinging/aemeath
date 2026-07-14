# Memory · 检索与注入

> 层级：02-modules / memory（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#789（S2）
> 本文定义 Memory BC 的 v0.1.0 检索策略、注入格式、`similarity_threshold` 双重用途与 Future 语义检索演进边界。**只描述目标态**；实现差距见 [迁移治理](../../03-engineering/migration-governance.md)。

## 1. 检索模式

Memory BC 提供两种检索模式，分别服务不同消费场景：

| 模式 | 方法 | 场景 | 排序依据 |
|---|---|---|---|
| **Top-N 注入** | `retrieve_for_inject(MemoryQuery)` | 每轮 LLM 调用前自动注入 | query-independent eligibility + injection_score（pinned + recency + access_count）|
| **Query-aware 检索** | `search(MemorySearchQuery)` | 用户 `/memory search` | BM25 / fallback relevance，平分时使用独立 `search_tie_break_score` |

两种模式统一返回 `MemorySearchResult`，但评分语义**不相同**：自动注入的 hit 没有 relevance，显式 search 才携带 BM25 / fallback relevance。envelope 让调用方看见实际 retrieval mode，并让管理面无损展示 archive、outdated 与 TTL 状态；Context Management 只消费其中按序排列的 eligible entry。

```rust
struct MemorySearchResult {
    mode: MemoryRetrievalMode,
    hits: Vec<MemorySearchHit>,
}

struct MemorySearchHit {
    entry: MemoryEntry,
    relevance: Option<MemoryRelevance>,
    location: MemoryLocation,
    ttl: MemoryTtlState,
}

enum MemoryRetrievalMode {
    InjectionPriority,
    Bm25,
    SubstringFallback { reason: MemoryFallbackReason },
    Disabled,
}

enum MemoryRelevance {
    Bm25Normalized(f64),
    SubstringMatch,
}

enum MemoryLocation { Active, Archive }

enum MemoryTtlState {
    NotConfigured,
    Valid { expires_at: u64 },
    Expired { expired_at: u64 },
}
```

`entry.outdated` 与 hit 的 `ttl` 是独立维度：同一显式 search hit 可以同时位于 Archive、`entry.outdated=true` 且 TTL expired。`outdated` 只定义一次在 `MemoryEntry`；envelope 只补充 entry 本身没有的 `location` 与按本次 `query.now` 规范化的 TTL 状态，调用方 **NEVER** 从文件名或当前时钟重复推导。

### 1.1 Top-N 注入检索

```rust
fn retrieve_for_inject(&self, query: &MemoryQuery) -> MemorySearchResult;
```

- 跨 Global + Project 两层 active 条目合并。
- 先硬过滤 `outdated` 或 TTL-expired 条目；`pinned` **NEVER** 绕过该过滤。
- 按 `injection_score` 降序排列。
- 取 top `limit` 条（默认 `inject_count = 5`）。
- **不 touch**（不更新 accessed_at）——避免每轮注入导致排序漂移；Target **NEVER** 发布 mutating 注入方法。
- 返回 `mode=InjectionPriority`；每个 hit **MUST** 为 `location=Active / entry.outdated=false / ttl!=Expired`，且 `relevance=None`。

**设计理由**：注入是**每轮 LLM 调用**都发生的高频操作。如果每次查询都 touch 条目，access_count 会爆炸增长，且排序会因自身行为而漂移。retrieve / search / list 都只读 open 时建立并由成功 mutation 发布的 in-memory state；若未来需要访问统计，必须新增显式、fallible mutation，**NEVER** 在返回 `Vec` 的查询中偷偷落盘。

### 1.2 Query-aware 检索

```rust
fn search(&self, query: &MemorySearchQuery) -> MemorySearchResult;
```

- 跨 active + archive（Global + Project）四域合并。
- 先按 query 相关性过滤并按 relevance 降序；平分时使用不要求 injection eligibility 的 `search_tie_break_score`，**NEVER** 调用 `injection_score`。
- archive、outdated 与 TTL-expired 条目可被显式检索，并在结果 metadata 中标记其状态；这不表示它们可自动注入。
- 取 top `limit` 条。
- 正常路径返回 `mode=Bm25` 与 `MemoryRelevance::Bm25Normalized`；只有 index 明确不可用时 **MUST** 返回带原因的 `SubstringFallback`，**NEVER** 以空结果掩盖降级。

## 2. 检索分层（#551）

### Tier 0 — 子串 fallback

```rust
fn entry_matches(entry: &MemoryEntry, query: &str) -> bool {
    let q = query.to_lowercase();
    entry.content.to_lowercase().contains(&q)
        || entry.tags.iter().any(|tag| tag.to_lowercase().contains(&q))
        || format!("{:?}", entry.category).to_lowercase().contains(&q)
        || format!("{:?}", entry.layer).to_lowercase().contains(&q)
}
```

- **成本**：零依赖。
- **限制**：没有连续 relevance 分数、无模糊匹配；匹配集合只按 `search_tie_break_score` 给出确定次序，因此只作为索引不可用时的显式 fallback。
- **适用**：空 query 的管理过滤或 BM25 index 明确不可构建的降级；结果 **MUST** 使用 `MemoryRetrievalMode::SubstringFallback { reason }` 标记实际模式供诊断。

### Tier 1 — BM25 关键词相关性（v0.1.0 primary）

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
- **除零防护**：`avg_doc_len == 0` 时返回 0 分；BM25 score 归一化时 `max_score == 0` 时直接返回 0；Jaccard fallback 在双方 token 集均为空时返回 0。
- **query 规范化**：substring fallback 对 query 做 `to_lowercase()` 后再匹配，与 entry 一致。
- **构建时机**：首次检索时构建索引并缓存，写入/归档/更新/**删除**后失效。

### Tier 2 — Embedding 语义检索（v0.2.0+，方向预留）

- 需引入 embedding 模型（本地如 `all-MiniLM-L6-v2` 或远程 API）。
- 存储格式变更：MemoryEntry 需增加 `embedding: Option<Vec<f32>>` 字段。
- 写入时计算 embedding 并存储；检索时计算 query embedding 做 cosine similarity。
- **前置条件**：#549（Memory 注入）落地后验证实际收益，再决定是否推进（见 #551）。

### 能力分层

```text
Tier 0（fallback）       Tier 1（v0.1.0 primary）      Tier 2（Future）
子串匹配                 BM25 关键词相关性              Embedding 语义检索
无排序                   归一化分数排序                 cosine similarity
显式降级诊断             threshold 过滤                 threshold 过滤
零依赖                   纯 Rust                       需模型服务
```

**v0.1.0 决策**：推进 Tier 1（BM25），暂不做 Tier 2。理由：
1. BM25 成本低（纯 Rust，无外部依赖），收益明显。
2. Embedding 需要模型服务 + 存储格式变更，投入大，需先验证 #549 落地后的实际收益。
3. Tier 1 只提升显式 query-aware search；自动注入仍使用 query-independent `injection_score`，因此 BM25 上线**不是**提高 `inject_count` 的依据。

## 3. 注入格式

Memory BC 输出排序后的 `MemorySearchResult`；**Context Management** 独占注入格式、位置、token 预算与跨轮去重。自动注入只按顺序读取 `hits[*].entry`，不把 retrieval mode、relevance、archive/outdated/TTL metadata 渲染进 prompt；管理查询则可完整展示这些 metadata。

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
| 检索 / 排序 top-N 条目 | Memory BC（`MemoryPort.retrieve_for_inject`）|
| 格式化为 `<memory-context>` | Context Management |
| 决定注入位置（system block 顺序）| Context Management |
| Token 预算分配 | Context Management |
| 与 guidance / AGENTS.md / skill 的排序 | Context Management |
| 注入去重（跨轮避免重复注入相同条目）| Context Management |

Memory BC 输出已完成 filtering / ranking 的条目；Context Management 只决定“如何渲染、放哪、在本轮 token budget 下放多少”，**NEVER** 二次改写相关性顺序。

## 4. similarity_threshold 双重用途

```rust
struct MemoryConfig {
    similarity_threshold: f64,    // 默认 0.8，范围 [0, 1]
    inject_count: usize,          // 默认 5
}
```

| 用途 | 语义 | Tier 0 | Tier 1 | Tier 2 |
|---|---|---|---|---|
| **去重** | 写入时 Jaccard ≥ threshold → 合并 | 应用 | 应用 | 应用 |
| **检索过滤** | 检索相关性 < threshold → 排除 | 不适用（显式 fallback） | BM25 归一化分数 | cosine similarity |

Tier 1 的 BM25 分数归一化到 [0, 1]：
- 归一化方式：`score / max_score`（相对归一化）。
- threshold = 0.8 意味着只保留与最高分条目相似度 ≥ 80% 的结果。
- 可配置调整：降低 threshold → 更多结果但质量参差；提高 threshold → 更少但更精准。

## 5. inject_count 配置

`inject_count` 是上节同一 `MemoryConfig` 的字段，默认值为 5；本文 **NEVER** 定义第二份配置结构。

- **自动注入**：默认 5（query-independent access/recency/pin 排序，保守注入约 300 token）。
- **Tier 1 与注入条数正交**：BM25 只服务显式 search，落地后 **MUST NOT** 仅以“相关性更高”为由提高 `inject_count`。
- **动态注入**（未来方向）：Context Management 根据 token budget 动态决定注入条数，Memory BC 只提供排序后的候选池。

## 6. 检索不变量

| # | 不变量 | 说明 |
|---|---|---|
| R1 | retrieve_for_inject **不 touch** | 避免注入导致排序漂移 |
| R2 | search **跨 active + archive** | 归档条目仍可被 search 检索到 |
| R3 | TTL 过期条目 **不参与注入** | 在 scoring 前由 eligibility 硬过滤 |
| R4 | outdated 条目 **不参与注入** | 在 scoring 前由 eligibility 硬过滤；显式 search 仍可检索 |
| R5 | pinned 条目 **在 eligible 集合中优先** | +10000 bonus，但不绕过 TTL / outdated 过滤 |
| R6 | search **NEVER** 复用 injection_score | 显式检索允许 archive / outdated / TTL-expired；相关性平分时走独立 tie-break |
| R7 | result **MUST** 标记实际 retrieval mode | BM25 降级不能伪装成 BM25 或普通空结果 |
| R8 | hit 状态 **MUST** 无损 | search 可同时表达 archive、outdated 与 TTL-expired；注入只返回 active eligible hit |

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
| 2026-07-14 | 引入 MemorySearchHit / MemorySearchResult envelope，区分 query-independent 注入与 BM25 relevance，并显式携带 fallback 与条目状态 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
