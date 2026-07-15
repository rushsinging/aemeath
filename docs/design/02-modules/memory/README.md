# Memory（支撑域）

> 层级：02-modules / memory（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#789（S2）/ [#972](https://github.com/rushsinging/aemeath/issues/972)
> 本模块拥有跨会话记忆的存取、检索、去重、归档与反思（Reflection）产出。Memory 是独立 BC，不是 Context Window 的一部分——检索归 Memory，注入位置与预算归 Context Management。

## 1. 模块定位

Memory BC 管理 Agent 跨会话积累的**持久化记忆**：用户偏好、项目决策、技术模式、已知陷阱。它回答两个问题——"记得什么"和"反思后该记什么"。

| 能力 | 语义 | 主要下游 |
|---|---|---|
| **记忆存取** | MemoryEntry 的增删改查、去重、归档、淘汰 | Context Management（注入）、MemoryTool、CommandRouter |
| **检索** | 按 query 相关性或 recency/pin 排序返回 top-N | Context Management（注入前检索）|
| **Reflection** | 构建 / 解析 Reflection 语言并对当前 Memory 实例应用 suggestion | Runtime（触发与 LLM 调用编排）|

Memory BC **不拥有**：记忆在 Context Window 中的注入位置、token 预算、render 格式、与 guidance/AGENTS.md/system prompt 的排序——这些归 Context Management。Memory BC 返回已 filtering / ranking 的 `MemorySearchResult`（hits + retrieval mode + 状态 metadata）与纯 Reflection parse / format 结果；自动注入只消费其中 eligible entries，管理查询可展示完整 metadata。

## 2. 核心决策

1. **Memory 是数据 BC，不是状态机**：Memory 没有执行生命周期状态机；它守护 MemoryEntry 聚合的局部不变量（id 唯一、layer 不可变、access_count 单调递增、outdated 不可逆）。
2. **双层记忆**：Global（跨项目通用偏好）+ Project（项目特定决策/模式/陷阱）。两层独立存储、独立检索、统一排序。
3. **检索与注入分离**：Memory BC 负责 filtering、scoring 与 relevance ranking；Context Management 负责 render、注入时机、位置、预算与跨轮去重。
4. **Reflection 规则归 Memory，实例由 active Run 绑定**：prompt 模板、output schema 与 parsing 是纯 `ReflectionPromptPort`；apply 规则由当前 Run 的同一 `MemoryPort::apply_reflection` 执行。触发时机和 LLM 调用编排归 Runtime，Memory BC **不依赖** ProviderPort，也不隐式选择 store。
5. **去重基于 Jaccard 相似度**：写入时与同 layer 条目比较，超过 `similarity_threshold` 则合并 tags + touch，不新增。
6. **淘汰基于评分**：pinned 条目不可淘汰；其余按 `eviction_score`（recency + access_count）排序，取最低分候选归档。
7. **Archive 不删除**：归档条目移到 `_archive.json`，保留可审计性；search 可跨 active + archive 检索。
8. **Sub Run 默认不读写 Memory**：默认装配 `NoOpMemory`；Main 显式 share 时 clone 父 Run 当前 Arc 并继承 shared lease，**NEVER** 在同一 Composition / 进程的 active Main slot 为同 identity 新开第二个 service。独立进程 writer 则经 revision CAS 协调。
9. **检索能力分层**：Tier 1 BM25 是 v0.1.0 primary；Tier 0 子串只作显式 fallback；Tier 2 embedding 属 Future 且需真实收益证据。
10. **Reflection 异步执行**：Interval 和 Pre-compact 触发的 Reflection 不阻塞主循环——Runtime `tokio::spawn` 后台任务，结果通过 channel 回传。Forced（`/reflection`）保持同步。单一后台 slot 并发控制，前一个未完成时跳过本次。Pre-compact 在 compact 前抓 messages 快照交给后台任务，compact 立即继续。
11. **查询只读已验证内存态**：open 完成 dataset recovery 与 eager-read；retrieve / search / list / stats 不做 I/O 或 touch。mutation 先构造 candidate，再经 Storage dataset transaction durable commit，最后无失败发布。
12. **Active + archive 共同换代**：archive / compact 对受影响成员使用同一 `AtomicDatasetPort` journal / commit primitive；失败或 crash 后只能恢复完整旧代或新代，**NEVER** 暴露半迁移。
13. **跨实例用 revision CAS 防丢更新**：open 持有 Storage 返回的 opaque dataset revision；每次 mutation 以它作为 expected revision 提交。冲突时重新读取、验证并重算一次，**NEVER** 让跨进程锁掩盖 stale-writer overwrite。
14. **committed warning 仍发布**：Storage `Err` 只表示未提交；`Visible` / `RecoveryPending` receipt 都表示 durable truth 已是 candidate，Memory 必须发布新内存态，warning 只作诊断。
15. **查询 envelope 无损表达事实**：自动注入与显式 search 都返回 `MemorySearchResult`；BM25 / substring fallback 必须可诊断，archive / outdated / TTL 是独立状态维度。自动注入的 `injection_score` 不携 query relevance，**NEVER** 把 BM25 的收益套到 `inject_count`。

## 3. Target 物理目录

Memory 的 write、retrieve、compact 与 reflection 已有不同规则、变化原因和测试夹具，因此业务切片 **MUST** 统一进入私有 `capabilities/`；`MemoryEntry` 等跨切片共享不变量保留在最小 `model`，持久化 seam 靠近实际消费它的 mutation 切片，**NEVER** 建 crate-root 横向 adapter 层：

```text
lib.rs
model.rs                         # MemoryEntry 与跨切片共享不变量
capabilities.rs                  # 私有切片注册
capabilities/
├── write.rs                     # add / update / pin / outdated + CAS retry
├── retrieve.rs                  # eligibility / scoring / BM25 / search
├── compact.rs                   # eviction + archive 共同换代
├── reflection.rs                # prompt / parse / apply 协作入口
└── reflection/                  # 仅在内部用例已独立变化时展开
```

crate 根 façade 发布 `MemoryPort`、`ReflectionPromptPort` 与必要 PL；切片内部消费 Storage OHS 的 integration code 归对应 mutation 用例。`api/`、crate-root `port/`、crate-root `adapter/` **NEVER** 作为固定目录。若某切片退化为单一紧密行为，**MUST** 合并回相邻 owner，不能为了保留目录对称制造空壳。

## 4. 对外端口

| 端口 | 消费方 | 职责 |
|---|---|---|
| `MemoryPort` | Context Management / Runtime Reflection / MemoryTool / CommandRouter | 返回 typed search envelope、注入 top-N、CRUD、归档、compact、stats |
| `ReflectionPromptPort` | Runtime（reflection 编排）| 构建 prompt、格式化 memory / recent summary、parse / format output（全为纯逻辑） |

`MemoryPort` 覆盖记忆的全部操作（含对当前实例 apply reflection）；`ReflectionPromptPort` 只暴露纯 prompt / parse / format。Memory BC 自身不调 LLM。

## 5. 与其他 BC 的关系

### Agent Runtime

Context Management 通过 `MemoryPort` 检索 top-N 记忆并注入 Context Window；Runtime 通过纯 `ReflectionPromptPort` 构建 / 解析反思，并让 shared session lease 下绑定的同一 MemoryPort Arc 执行 apply。Runtime 负责调 ProviderPort，后台 Reflection job **MUST** 持 lease 到完成或取消收口。

### Context Management

Context Management 通过 `MemoryPort` 获取检索结果，**独占**注入位置、token 预算、去重、与 guidance/AGENTS.md/skill 的排序。Memory BC 不决定记忆在窗口中的位置。

### Storage

Storage BC 提供**物理机制**（单 blob 原子写、多 member dataset transaction、损坏兜底、路径管理），不拥有 Memory 数据本体。Memory 的领域逻辑（scoring、dedup、检索与 candidate 构造）归 Memory BC；文件 I/O、dataset lock 与 journal recovery 归 Storage adapter。

### Config

Config 通过只读 ConfigSnapshot 提供 MemoryConfig（enabled / max_entries / similarity_threshold / inject_count / reflection 配置）。Memory BC 不绕过快照读取裸配置。

### Tool & Skill & Command

Slash Command 的 `SnapshotQuery`（`/memory` 查看列表）和 `ApplicationControl`（`/memory add` / `/memory delete` / `/memory pin`）经 `AgentClient` 进入核心，再由 CommandRouter 路由到 Memory BC 的 `MemoryPort`。CLI / TUI 不直接持有 MemoryPort，Memory BC 也不处理 Slash 文本解析。

## 6. 设计边界

- **NEVER** 让 Memory BC 依赖 ProviderPort——Reflection 的 LLM 调用由 Runtime 编排。
- **NEVER** 让 Memory BC 决定注入位置或 token 预算——归 Context Management。
- **NEVER** 让 Memory BC 直接操作文件系统——经 Storage adapter。
- **NEVER** 把所有历史消息自动视为记忆——Memory Entry 是显式写入或 Reflection 产出的。
- **MUST** MemoryEntry 的 id 唯一、layer 创建后不可变、access_count 单调递增、outdated 标记不可逆。
- **MUST** Sub Run 默认装配 NoOpMemory；显式 share 时 clone 父 Run 当前 Arc 并由父 shared lease 覆盖其生命周期。
- **MUST** pinned 条目不可被 compact/evict 淘汰。
- **MUST** 去重和淘汰基于纯函数评分，不依赖外部状态。

## 7. 文档导航

| 文档 | 内容 |
|---|---|
| [01-domain-model.md](01-domain-model.md) | MemoryEntry 聚合、Layer/Category/Source、不变量、scoring/dedup/eviction/archive |
| [02-retrieval-and-injection.md](02-retrieval-and-injection.md) | 检索策略、BM25 升级路径(#551)、注入格式、similarity_threshold 双重用途 |
| [03-reflection.md](03-reflection.md) | ReflectionEngine、MemorySuggestion、触发条件、prompt/output/apply、与 Runtime 职责边界 |
| [04-ports-and-adapters.md](04-ports-and-adapters.md) | MemoryPort / ReflectionPromptPort、NoOpMemory、Storage 边界、project-aware Composition 装配 |

## 8. 相关文档

- Runtime 领域模型（MemoryPort 消费方）：[../runtime/01-domain-model.md](../runtime/01-domain-model.md)
- Runtime 端口与装配：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Context Map（Memory BC 集成关系）：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- BC 责任章程：[../../01-system/01-product-and-domain.md](../../01-system/01-product-and-domain.md) §4.1
- 统一语言（Memory 术语）：[../../01-system/02-ubiquitous-language.md](../../01-system/02-ubiquitous-language.md) §5
- 迁移治理：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：Memory BC 定位、核心决策、内部结构、端口、设计边界 | #789 |
| 2026-07-14 | 明确 MemoryPort 为 Memory-owned OHS，并禁止 CLI/TUI 绕过 AgentClient 直接持有 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | 统一自动注入与显式查询的 typed result envelope，纠正 BM25 与 query-independent injection_score 的收益边界 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-16 | 冻结 Memory Target 物理结构：write/retrieve/compact/reflection 统一进入私有 `capabilities/`，跨切片 MemoryEntry 不变量保留最小 model，Storage integration 就近归 mutation 切片 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
