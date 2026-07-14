# Memory · 领域模型

> 层级：02-modules / memory（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#789（S2）
> 本文定义 Memory BC 的领域模型：MemoryEntry 聚合、Layer/Category/Source 枚举、不变量、评分函数、去重与淘汰策略。**只描述目标态**；实现差距记入 `03-engineering/migration-governance.md`。

## 1. MemoryEntry 聚合

```rust
struct MemoryEntry {                     // 聚合根（可序列化，持久化）
    id: MemoryId,                        // UUIDv7 newtype
    layer: MemoryLayer,                  // Global | Project（创建后不可变）
    category: MemoryCategory,            // Fact | Decision | Preference | Pattern | Pitfall
    content: String,                     // 记忆内容（非空）
    source: MemorySource,                // Llm | Hook | User
    source_ref: Option<String>,          // 可选来源引用（如 hook 名、issue 编号）
    tags: Vec<String>,                   // 用户/LLM 标注的标签
    pinned: bool,                        // 固定条目，不参与淘汰
    ttl: Option<Duration>,               // 过期时间（None=永不过期）
    created_at: u64,                     // Unix 秒
    accessed_at: u64,                    // 最后访问时间
    access_count: u32,                   // 访问次数（单调递增）
    outdated: bool,                      // Reflection 标记为过期（不可逆）
}
```

### 实体 vs 值对象

| 对象 | 类型 | 说明 |
|---|---|---|
| **MemoryEntry** | 实体（聚合根） | 有标识（id）、有生命周期、可序列化 |
| `MemoryLayer` / `MemoryCategory` / `MemorySource` | 值对象（枚举） | 按值比较、不可变 |
| `tags` / `content` / `source_ref` | 值对象 | String / Vec\<String\> |
| `injection_score` / `eviction_score` | 值对象（计算值） | 纯函数，不存储 |
| `MemorySearchHit` / `MemorySearchResult` | 值对象（查询 envelope） | 携带条目、检索模式、相关性与 active/archive/outdated/TTL 状态；不改变条目 |
| `WriteResult` / `CompactResult` / `MemoryStats` | 值对象（DTO） | 操作结果，无行为 |

## 2. 枚举定义

### MemoryLayer

```rust
enum MemoryLayer {
    Global,    // 跨项目通用（~/.agents/memory/_global.json）
    Project,   // 项目特定（~/.agents/memory/{project_file_name}.json）
}
```

- **创建后不可变**：一条记忆不能从 Global 迁移到 Project 或反之。
- **独立存储**：两层各自有 active + archive 文件。
- **检索统一排序**：注入时跨两层合并排序，不按 layer 分组。

### MemoryCategory

```rust
enum MemoryCategory {
    Fact,         // 事实：项目技术栈、依赖版本、目录结构
    Decision,     // 决策：架构选择、库选型、设计取舍
    Preference,   // 偏好：代码风格、命名约定、回复语言
    Pattern,      // 模式：可复用的解决方案模式
    Pitfall,      // 陷阱：已知问题、易错点、避坑指南
}
```

Category 是**分类标签**，不驱动行为策略——检索时作为 query 匹配维度之一，但不影响 scoring 权重。Category 由写入者（User/LLM/Hook）指定，Reflection 建议 MemorySuggestion 时也携带。

### MemorySource

```rust
enum MemorySource {
    Llm,    // Reflection 引擎产出并写入
    Hook,   // Hook 脚本产出
    User,   // 用户手动写入（slash 命令）
}
```

Source 是**溯源标记**，用于审计和信任度判断——不改变 scoring 权重，但 Reflection 可根据 source 决定是否覆盖/更新。

## 3. 不变量

Memory BC 守护以下局部不变量：

| # | 不变量 | 违反场景 | 守护点 |
|---|---|---|---|
| M1 | **id 唯一** | 两条 active 记忆有相同 id | `add` 时检查；id 为空时自动生成 UUIDv7 |
| M2 | **layer 不可变** | 修改已有记忆的 layer | 无 `set_layer` 方法；`update` 只改 content |
| M3 | **content 非空** | 写入空字符串 | `add` / `update` 前校验 `content.trim().is_empty()` |
| M4 | **access_count 单调递增** | 回退 access_count | `touch` 只做 `saturating_add(1)` |
| M5 | **outdated 不可逆且不可注入** | 从 outdated 回到 active，或将 outdated 记忆放入注入候选 | 无 `unmark_outdated` 方法；`is_injection_eligible` 硬过滤 |
| M6 | **pinned 不被淘汰** | compact/evict 淘汰了 pinned 条目 | `eviction_candidates` 过滤 `!entry.pinned` |
| M7 | **active 容量上限** | active 条目数超过 `max_entries` | `add` 时检查，返回 `NeedsEviction` |
| M8 | **TTL 过期不注入** | 注入了 TTL 已过期的记忆 | `is_injection_eligible` 硬过滤 |

## 4. 评分函数

注入先做 eligibility 硬过滤，再对合格条目评分。过滤与评分都是**纯函数**，不依赖外部状态，只接收 `&MemoryEntry` + `now: u64`。

### injection_score（注入优先级）

```rust
fn is_injection_eligible(entry: &MemoryEntry, now: u64) -> bool {
    !entry.outdated && !entry.is_ttl_expired(now)
}

fn injection_score(entry: &MemoryEntry, now: u64) -> i64 {
    debug_assert!(is_injection_eligible(entry, now));
    let pinned_bonus    = if entry.pinned { 10_000 } else { 0 };
    let access_score    = i64::from(entry.access_count.min(20)) * 100;
    pinned_bonus + access_score + recency_score(entry.accessed_at, now)
}
```

| 因子 | 权重 | 说明 |
|---|---|---|
| pinned | +10,000 | 在 eligible 条目中保持最高优先级 |
| access_count | +100/次（封顶 20 次 = +2,000）| 高频访问 = 高价值 |
| recency | +50 ~ +1,000 | 越近访问权重越高（0天=1000, 1-7天=800, 8-30天=500, 31-90天=200, >90天=50）|

**设计意图**：outdated 或 TTL-expired 条目在评分前就被排除，`pinned` 不能绕过 eligibility。`pinned_bonus` 大于 access 与 recency 两项的最大和，因此 eligible pinned 条目始终排在未 pinned 条目前；未 pinned 条目按 access 与 recency 的加和排序，二者之间**没有**固定优先级。

`injection_score` 是**query-independent** 的自动注入优先级：`MemoryQuery` 只有 limit / layer / category / now 等过滤输入，不携带搜索文本。它 **NEVER** 被描述成 BM25 relevance，也 **NEVER** 因显式搜索升级为 BM25 就自动获得“更相关”的收益；若未来自动注入要使用用户 query，**MUST** 另行版本化输入、评分与 cache/fingerprint 语义。

### search_tie_break_score（显式检索的稳定次序）

```rust
fn search_tie_break_score(entry: &MemoryEntry, now: u64) -> i64 {
    let pinned_bonus = if entry.pinned { 10_000 } else { 0 };
    let access_score = i64::from(entry.access_count.min(20)) * 100;
    pinned_bonus + access_score + recency_score(entry.accessed_at, now)
}
```

显式 `search` 先按 BM25 / fallback relevance 排序，仅在 relevance 相同时用该纯函数稳定排序。它故意**没有** `is_injection_eligible` 前置条件：archive、outdated 与 TTL-expired 条目仍可被用户明确检索；这些状态作为结果 metadata 展示，**NEVER** 因复用 `injection_score` 触发断言或被静默过滤。

### eviction_score（淘汰优先级，越低越先淘汰）

```rust
fn eviction_score(entry: &MemoryEntry, now: u64) -> i64 {
    if entry.pinned { return i64::MAX; }    // pinned 不可淘汰
    let age_days = now.saturating_sub(entry.accessed_at) / 86_400;
    let recency_weight = 100_i64.saturating_sub(age_days.min(100) as i64);
    i64::from(entry.access_count) * 10 + recency_weight
}
```

- **pinned = i64::MAX**：永不淘汰。
- **低 access_count + 高 age_days = 低分 = 优先淘汰**。
- recency_weight 在 100 天后归零，之后只靠 access_count 保命。

## 5. 去重（Dedup）

### Jaccard 相似度

```rust
fn jaccard_similarity(left: &str, right: &str) -> f64 {
    let left_tokens = tokenize(left);
    let right_tokens = tokenize(right);
    // 交集 / 并集
}
```

- **分词**：按非字母数字字符分割，转小写，过滤空 token。
- **阈值**：`similarity_threshold`（默认 0.8）。
- **写入时去重**：`write` 时遍历同 layer active 条目，若 Jaccard ≥ threshold 则合并（tags 取并集 + touch），返回 `WriteResult::Merged`。

### similarity_threshold 的双重用途

| 用途 | 语义 | 阈值含义 |
|---|---|---|
| **去重** | 写入时判断是否与已有记忆重复 | Jaccard ≥ threshold → 合并 |
| **检索过滤**（Tier 1+）| query-aware 检索时过滤低相关性结果 | 相关性分数 < threshold → 排除 |

v0.1.0 的 BM25 检索与写入去重 **MUST** 共用同一配置值，但分别按各自分数语义解释；调用点 **NEVER** 硬编码第二份阈值。

## 6. 淘汰与归档

### 触发条件

- `write` 时 active 条目数 ≥ `max_entries` → 返回 `WriteResult::NeedsEviction { candidates }`
- `compact()` 主动触发 → 对超容量的 layer 批量淘汰

### 淘汰流程

```text
write(entry)
  ├─ active.len() >= max_entries?
  │   ├─ Yes → 取 eviction_candidates(count=3)
  │   │         → 返回 NeedsEviction { candidates }
  │   │         → 调用方决定 evict 后重试 add
  │   └─ No  → 正常添加
  └─ 合并检查（Jaccard ≥ threshold → Merged）
```

### 归档语义

- **archive_entries(ids)**：从 active 移到 archive 文件。
- **不删除**：归档条目保留在 `_archive.json`，可供 `search` 跨域检索。
- **compact()**：对每个超容量 layer 取 10 个淘汰候选，批量归档。
- **evict(ids)**：等价于 `archive_entries`——Memory BC 不做物理删除。

## 7. WriteResult

```rust
enum WriteResult {
    Added { id: MemoryId },                  // 新增成功
    Merged { existing_id: MemoryId },        // 与已有记忆合并
    NeedsEviction { candidates: Vec<MemoryEntry> }, // 需先淘汰
    NoOp,                                    // NoOpMemory 显式不写
}
```

`NeedsEviction` 是**非错误**——它告诉调用方“容量已满，这是淘汰候选”，Memory application service 可归档后重试。Storage / serialization 失败则通过结构化 `MemoryError` 返回，**NEVER** 伪装成结果值。

## 8. SessionReminder 所有权边界

`SessionReminder` 是**会话级**提醒（非跨会话记忆），所有权属于 **Context Management** 的 Session 聚合，**NEVER** 进入 Memory BC 的模型或公开面。

- SessionReminder 的 `recap_line` 是 session 级上下文注入，不是跨会话记忆检索。
- Memory BC 只管跨会话的 MemoryEntry；SessionReminder 不归 Memory。

## 9. 聚合与服务边界

| 对象 | 类型 | 所有权 / 说明 |
|---|---|---|
| MemoryEntry | 聚合根 | 守护 M1-M8 不变量 |
| is_injection_eligible / injection_score / eviction_score | 纯函数（领域服务）| 无状态，接收 entry + now；先过滤再评分；injection_score 与 query relevance 正交 |
| jaccard_similarity / tokenize | 纯函数（领域服务）| 无状态，接收两个字符串 |
| MemoryService | 应用服务 | 实现 MemoryPort，编排领域规则与窄 Storage port；不直接做文件 I/O |
| MemoryStorageAdapter | 出站 adapter | active / archive 的读取、原子写、key 迁移；不拥有领域排序与 dedup |
| ReflectionEngine | 领域服务 | prompt 构建 + output parsing + apply（纯领域，不调 LLM）|

## 10. 相关文档

- 模块入口：[README.md](README.md)
- 检索与注入：[02-retrieval-and-injection.md](02-retrieval-and-injection.md)
- Reflection 引擎：[03-reflection.md](03-reflection.md)
- 端口与适配器：[04-ports-and-adapters.md](04-ports-and-adapters.md)
- 统一语言：[../../01-system/02-ubiquitous-language.md](../../01-system/02-ubiquitous-language.md) §5

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：MemoryEntry 聚合、枚举、不变量 M1-M8、评分函数、去重、淘汰归档 | #789 |
