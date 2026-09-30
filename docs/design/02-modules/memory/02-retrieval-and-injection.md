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
- 在评分前硬过滤被取代（M10）、outdated 与 TTL-expired；pinned **NEVER** 绕过 eligibility。
- 对 eligible 集合**先分组后排序**（#1777 覆盖式让位）：`kind = Synthesized` 的归纳结论排在全部普通条目之前，组内仍按 `injection_score` 降序、完整 Memory ID 升序。**不改分数，只改顺序**。
- 先取 query.limit 个候选；Context 再按 token 预算做**两段填充**（见 §6.2），超预算时停止，不重排、不回填后项。
- **不 touch、不落盘**——避免每轮注入导致排序漂移。

**设计理由**：注入是每轮 LLM 调用都会发生的高频纯查询。它只读 open 时已验证的内存 state；访问统计若未来需要，必须另设显式、fallible mutation。

### 1.2 Query-aware 检索

```rust
fn search(&self, query: &MemorySearchQuery) -> MemorySearchResult;
```

- 可按 `include_archive` 跨 active + archive（Global + Project）检索。
- archived、被取代、outdated 与 TTL-expired 条目仍可由用户显式检索，并通过 hit metadata（含 `superseded_by`）无损表达状态。
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

- 对 query、content、tags、category、layer 做确定性混合分词；拉丁字母、数字与代码标识符沿用小写字母数字词项，连续 Han 字符生成相邻双字 bigram，单个 Han 字符保留为单字词项；空 query 返回空结果。
- Han bigram 在同一个 BM25 词项空间内参与字段权重、文档频率和长度归一化，不建立中文专用 fallback、第二索引 backing 或词典依赖；中英文混排分别生成 Latin token 与 Han bigram。
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
3. 自动注入仍保持独立的稳定优先级与默认 count=5/token budget=300；Tier 1 只升级显式 search，不把自动注入改为相关性排序。

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
- **不含** id / last_confirmed_at / confirmation_count / source 等元数据——这些是管理信息，不注入给 LLM。

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
- `MemoryUpdate` 的 `status`（`pin`/`unpin`/`archive`/`restore`）、`layer`（`global`/`project`）、`category`（`fact`/`decision`/`preference`/`pattern`/`pitfall`）由 build.rs 从 Rust 类型生成枚举约束，而不是无边界字符串。
- `search` 的 typed result 返回 id、content、layer、category、tags、pinned、location、outdated、ttl_expired、superseded_by、evidence、relevance；`list` 返回完整 entries。由于 Tool pipeline 对 LLM 使用 text-first 投影，search/list 的 text **MUST** 同样保留有序条目与可管理完整 ID；structured data 服务 TUI/server，不能替代 LLM 文本契约。
- Tool 同时发布 `archive` / `restore`。满容量 add/restore 返回 `action=needs_eviction` 与 typed candidates（完整 ID、正文、层/分类/状态、confirmation_count、last_confirmed_at、eviction score/reason），写入保持 NotCommitted；调用方只能显式 archive，禁止静默自动淘汰。
- Memory 以 5 个单一职责工具暴露：`MemoryAdd` / `MemorySearch` / `MemoryList` / `MemoryUpdate` / `MemoryDelete`。
- **参数契约走字段级 `description`**：哪个字段必填、默认值、长度与数量上限都写在字段的文档注释上，由 build.rs 注入 schema。这是模型实际会读的参数级通道。
- **工具级 `description` 只回答「何时该用我」**，每条 ≤200 字符。行为策略不再堆在工具描述里。
- `MemoryUpdate` 用 `status` 枚举表达状态迁移，取代原先多个 action 加 `pinned: Option<bool>` 的三态歧义。
- Reflection 写入的 `MemorySuggestion` 经同一个 `MemoryPort` 成为普通 `MemoryEntry`，因此无需修改 Reflection trigger/workflow 即可被 Tool search 检索。

## 6. 自动注入配置

```rust
struct MemoryConfig {
    /// None（默认）= 按窗口比例 `context_size / 50`（2%）；Some(0) 禁用；
    /// Some(n) 为固定预算覆盖（#1777）。
    inject_token_budget: Option<usize>,
}
```

- 条数上限 `inject_count` 已随比例化**移除**（#1777）：token 预算本身就是上限，再叠条数约束只会让「长条目被条数截断、短条目被预算截断」两种语义互相掩盖。旧配置中的 `inject_count` 被忽略而非报错。
- 自动注入保持 query-independent `InjectionPriority`，显式 BM25 search 不改变其排序。
- `enabled=false` 或预算为 0 都不读取 Memory。
- User Message / Step query-aware retrieval 属 v0.2.0，当前不进入自动注入。

## 6.2 覆盖式让位与注入冻结

**填充算法**（#1777，预算紧而检索宽）：

```text
1. Memory 侧已把归纳结论排在前面（见 §1.1）
2. Context 第一段填结论；超预算即停——被挤掉的结论**不产生任何覆盖标记**
3. 第二段用剩余预算填普通条目，跳过被**已选结论**的 evidence 覆盖的来源
```

- 「已覆盖」在填充时扫描已选结论的 `evidence` 计算（`max_entries` 量级为微秒级），不引入 `synthesized_into` 反向指针。
- **关键性质**：结论未入选时其来源照常参与——固定降权系数会在这个场景误伤来源（来源被降权却没换来结论入注），因此让位必须是条件性的。
- 显式 `search` 中结论与来源**平等可见，都不让位**；反思输入两者都可见。

**注入时机**（#1777）：

```text
Session 首次 build window  →  注入并冻结
后续 build window          →  复用冻结内容（不重新检索）
Compact 成功提交后         →  下一次 build window 重新检索并替换
```

- 记忆块属于可缓存 system prefix：每轮重新检索会让内容漂移，直接损害 provider 的 prompt cache 命中率。
- 刷新点**只有 compact 成功提交**；`Skipped` 不刷新，reflect 完成也不刷新（只发提示）。
- 冻结状态绑定 `session_id`：resume 切换到新 Session 视为该 Session 的首次注入；`clear_session` 释放冻结内容。

## 6.1 安全观测

- Memory diagnostic 记录 search 的 query 字符数、候选/命中数、filter presence、空结果和 relevance 范围；write 记录 added/merged/noop/needs_eviction 与候选数。
- Context diagnostic 记录注入候选/入选/丢弃数、估算 token、count/token budget 和 Global/Project 数量。
- 日志禁止记录 query、content、tag、完整 ID、source_ref 或路径；这些是运行诊断，不伪装成成功 Model Invocation 的 Audit Usage fact。

## 7. 检索不变量

| # | 不变量 | 说明 |
|---|---|---|
| R1 | retrieve_for_inject / search / list / stats **不确认、不落盘** | 查询只读已验证内存 state，避免反馈环、排序与 revision 漂移 |
| R2 | search **可跨 active + archive** | 归档条目仍可由显式 search 检索 |
| R3 | TTL-expired 条目 **不参与注入** | 在 injection_score 前由 eligibility 硬过滤 |
| R4 | outdated 条目 **不参与注入但可显式检索** | 状态通过 search hit metadata 表达，NEVER 静默丢失 |
| R5 | pinned 只在 eligible 集合中获得最高优先级 | pinned 不能绕过 superseded / outdated / TTL eligibility |
| R7 | 被取代条目不参与注入但可显式检索并携带取代者 | M10 硬过滤；`superseded_by` 经 hit metadata 表达，关系由 apply 建立且无环（M9） |
| R8 | 合并产物可追溯且指针不悬空 | M11：写入时校验 evidence 指向；合并 = 归档新条目 + 旧条目记指针；compact 不删除被引用条目 |
| R9 | 结论入选时来源让位，未入选时来源照常 | 覆盖式让位：结论段优先填充，已选结论的 evidence 在第二段让位；不设降权系数、不设结论配额 |
| R10 | 注入内容在 Session 内稳定 | 首次注入后冻结，仅 compact 成功提交后刷新；`injection_score` 的 query-independent 语义不被排序分组破坏 |
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
| 2026-09-30 | 删除「已知信息缺口」节：其中两条 Memory 使用策略（不得覆盖系统/安全/当前用户指令；`superseded_by` 非空仅可检索不注入）已下沉至系统提示 `i18n/prompt/system.rs` 的 `# Core contract`，缺口闭合 | #1805 |
| 2026-08-21 | 闭合 archive/restore 与 typed eviction；将 access 字段治理为 confirmation 语义；增加稳定 ID tie-break、注入 token budget、LLM 使用策略和无正文诊断指标 | Memory governance |
| 2026-08-11 | 在单一 Tier 1 BM25 tokenizer 中加入连续 Han 字符 bigram，补齐中文短语与中英代码混排召回，不引入词典或第二检索路径 | Chinese lexical retrieval |
| 2026-07-26 | 落地共享确定性 BM25 词法排序与 typed Memory Tool PL；明确 Reflection 无需修改、search relevance 不复用写入去重 threshold | Tier 1 retrieval |
| 2026-07-12 | 初稿：检索模式、BM25 分层、注入格式、similarity_threshold 双重用途、注入职责边界 | 初始设计 |
| 2026-09-29 | 注入预算比例化（移除 `inject_count`、`inject_token_budget` 改 `Option` 且默认按窗口 2%）、覆盖式让位（结论优先 + 来源条件让位）、注入时机冻结（Session 首次 + compact 刷新）；决策矩阵新增 R9/R10 |
| 2026-09-29 | 注入硬过滤新增被取代条目（M10，pinned 不绕过）；`search` 结果 metadata 新增 `superseded_by`；决策矩阵新增 R7 | #1774 |
| 2026-07-17 | 对齐 #895：旧 top query 统一为只读 `retrieve_for_inject`；outdated/TTL 改为 eligibility 硬过滤；显式 search 改用 relevance + 独立 tie-break，并由 Context 独占 render | #895 |
