# Memory（支撑域）

> 层级：02-modules / memory（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#789（S2）
> 本模块拥有跨会话记忆的存取、检索、去重、归档与反思（Reflection）产出。Memory 是独立 BC，不是 Context Window 的一部分——检索归 Memory，注入位置与预算归 Context Management。

## 1. 模块定位

Memory BC 管理 Agent 跨会话积累的**持久化记忆**：用户偏好、项目决策、技术模式、已知陷阱。它回答两个问题——"记得什么"和"反思后该记什么"。

| 能力 | 语义 | 主要下游 |
|---|---|---|
| **记忆存取** | MemoryEntry 的增删改查、去重、归档、淘汰 | Context Management（注入）、CLI/TUI（slash 命令）|
| **检索** | 按 query 相关性或 recency/pin 排序返回 top-N | Context Management（注入前检索）|
| **Reflection** | 跑独立 LLM 调用，产出 MemorySuggestion（候选记忆）| Runtime（触发与 LLM 编排）|

Memory BC **不拥有**：记忆在 Context Window 中的注入位置、token 预算、与 guidance/AGENTS.md/system prompt 的排序——这些归 Context Management。Memory BC 只输出检索结果或 PromptFragment 级内容，由 Context Management 决定如何放进窗口。

## 2. 核心决策

1. **Memory 是数据 BC，不是状态机**：Memory 没有执行生命周期状态机；它守护 MemoryEntry 聚合的局部不变量（id 唯一、layer 不可变、access_count 单调递增、outdated 不可逆）。
2. **双层记忆**：Global（跨项目通用偏好）+ Project（项目特定决策/模式/陷阱）。两层独立存储、独立检索、统一排序。
3. **检索与注入分离**：Memory BC 负责检索逻辑（scoring + relevance ranking）和输出格式；Context Management 负责注入时机、位置、预算、去重。
4. **Reflection 是领域服务，不是 Runtime 模块**：prompt 模板构建、output schema、parsing、apply 逻辑归 Memory BC；触发时机和 LLM 调用编排归 Runtime。Memory BC **不依赖** ProviderPort。
5. **去重基于 Jaccard 相似度**：写入时与同 layer 条目比较，超过 `similarity_threshold` 则合并 tags + touch，不新增。
6. **淘汰基于评分**：pinned 条目不可淘汰；其余按 `eviction_score`（recency + access_count）排序，取最低分候选归档。
7. **Archive 不删除**：归档条目移到 `_archive.json`，保留可审计性；search 可跨 active + archive 检索。
8. **Sub Run 不读写 Memory**：SubAgent 装配 `NoOpMemory`，不读不写不 reflection（可由 Main 显式开启注入）。
9. **检索分层升级**：Tier 0（子串匹配，现状）→ Tier 1（BM25 关键词相关性，v0.1.0 目标）→ Tier 2（embedding 语义检索，v0.2.0+）。
10. **Reflection 异步执行**：Interval 和 Pre-compact 触发的 Reflection 不阻塞主循环——Runtime `tokio::spawn` 后台任务，结果通过 channel 回传。Forced（`/reflection`）保持同步。单一后台 slot 并发控制，前一个未完成时跳过本次。Pre-compact 在 compact 前抓 messages 快照交给后台任务，compact 立即继续。

## 3. 模块内部结构

```text
memory/
├── entry/                  # MemoryEntry 聚合、Layer/Category/Source 枚举
├── scoring/                # injection_score / eviction_score 纯函数
├── dedup/                  # Jaccard 相似度 + tokenize
├── retrieval/              # 检索策略：top-N / query-aware / BM25
├── reflection/             # ReflectionEngine 领域服务
│   ├── prompt/             # prompt 模板构建（纯函数，i18n）
│   ├── output/             # ReflectionOutput schema + parsing
│   └── apply/              # 写入 suggestion + 标记 outdated
├── format/                 # 记忆列表格式化、短 ID、tag 格式
└── api/                    # BC 对外 facade；MemoryPort 实现
```

目录表达业务能力而非 `contract / business / gateway / utils` 等横向技术层。Composition Root 是唯一生产装配入口。

## 4. 对外端口

| 端口 | 消费方 | 职责 |
|---|---|---|
| `MemoryPort` | Runtime（context_coordination）/ CLI/TUI（slash 命令）| 检索、注入 top-N、CRUD、归档、compact、stats |
| `ReflectionPromptPort` | Runtime（reflection 编排）| 构建 prompt（纯函数）、parse output、apply suggestion |

`MemoryPort` 覆盖记忆的全部操作；`ReflectionPromptPort` 把 Reflection 的领域逻辑（prompt/output/apply）暴露给 Runtime 编排，Memory BC 自身不调 LLM。

## 5. 与其他 BC 的关系

### Agent Runtime

Runtime 通过 `MemoryPort` 在每轮 LLM 调用前检索 top-N 记忆注入 Context Window；通过 `ReflectionPromptPort` 在间隔/强制/pre-compact 时机构建反思 prompt，Runtime 负责调 ProviderPort 执行 LLM 调用并把结果交回 Memory BC parse + apply。

### Context Management

Context Management 通过 `MemoryPort` 获取检索结果，**独占**注入位置、token 预算、去重、与 guidance/AGENTS.md/skill 的排序。Memory BC 不决定记忆在窗口中的位置。

### Storage

Storage BC 提供**物理机制**（原子写、损坏兜底、路径管理），不拥有 Memory 数据本体。Memory 的领域逻辑（scoring、dedup、检索）归 Memory BC；文件 I/O 归 Storage adapter。

### Config

Config 通过只读 ConfigSnapshot 提供 MemoryConfig（enabled / max_entries / similarity_threshold / inject_count / reflection 配置）。Memory BC 不绕过快照读取裸配置。

### Tool & Skill & Command

Slash Command 的 `SnapshotQuery`（`/memory` 查看列表）和 `ApplicationControl`（`/memory add` / `/memory delete` / `/memory pin`）经 CommandRouter 路由到 Memory BC 的 MemoryPort。Memory BC 不直接处理 Slash 文本解析。

## 6. 设计边界

- **NEVER** 让 Memory BC 依赖 ProviderPort——Reflection 的 LLM 调用由 Runtime 编排。
- **NEVER** 让 Memory BC 决定注入位置或 token 预算——归 Context Management。
- **NEVER** 让 Memory BC 直接操作文件系统——经 Storage adapter。
- **NEVER** 把所有历史消息自动视为记忆——Memory Entry 是显式写入或 Reflection 产出的。
- **MUST** MemoryEntry 的 id 唯一、layer 创建后不可变、access_count 单调递增、outdated 标记不可逆。
- **MUST** Sub Run 装配 NoOpMemory，不读写不 reflection。
- **MUST** pinned 条目不可被 compact/evict 淘汰。
- **MUST** 去重和淘汰基于纯函数评分，不依赖外部状态。

## 7. 文档导航

| 文档 | 内容 |
|---|---|
| [01-domain-model.md](01-domain-model.md) | MemoryEntry 聚合、Layer/Category/Source、不变量、scoring/dedup/eviction/archive |
| [02-retrieval-and-injection.md](02-retrieval-and-injection.md) | 检索策略、BM25 升级路径(#551)、注入格式、similarity_threshold 双重用途 |
| [03-reflection.md](03-reflection.md) | ReflectionEngine、MemorySuggestion、触发条件、prompt/output/apply、与 Runtime 职责边界 |
| [04-ports-and-adapters.md](04-ports-and-adapters.md) | MemoryPort trait、NoOpMemory、Storage 边界、Composition Root、现状缺口 |

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
