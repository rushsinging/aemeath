# Memory 记忆架构增强变更设计（Hindsight 对照）

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/1764

**日期**：2026-09-28
**状态**：设计中（本设计只覆盖设计阶段；实现需按本设计的测试策略另行立项）
**证据基础**：[05-hindsight-research.md](../../design/02-modules/memory/05-hindsight-research.md)
**影响范围**：Memory BC（`agent/shared/memory/`）、Runtime（Reflection 编排与触发门控）、Tool 层（Memory Tool Published Language）、Context Management（注入 eligibility）

---

## 1. 目标与非目标

### 1.1 目标

本次变更解决 Memory BC 现状中三个**已被具体化**的缺陷：

| 编号 | 缺陷 | 现状后果 |
|---|---|---|
| 变更 A | 无"取代"关系 | 两条语义相反的记忆可同时被注入，模型收到互相矛盾的信息 |
| 变更 B | 合并是信息销毁式的 | 被合并条目的正文不再可查，无法回答"这条记忆吸收了哪些内容" |
| 变更 C | 无跨条目归纳 | 多条相关记忆只能作为独立条目分别读取，无法形成结论；缺少带证据支撑的信念 |
| 变更 D | 注入预算与时机不受控、记忆变更不可感知 | 注入预算固定为 300 token 不随窗口缩放；memory block 每轮重算，破坏 prompt cache 稳定性；自动反思完成后既无 LLM 提示也无 TUI 提示 |

**关键取舍**：

- 变更 C 不引入独立的后台归纳机制，而是**扩展既有 Reflection 的职责**（§6）。这一选择连带消除了独立归纳层所需的全部配套（巩固水位、滞后判定、分层读取）—— 它们都服务于"第二个生命周期"，而扩展反思不产生第二个生命周期。
- 变更 D 把注入时机由"每轮重算"改为"首次注入 + compact 后刷新"（§7.2），与既有 guidance 部分的 Session 冻结语义对齐。

**依赖**：C 依赖 B（证据指针与 `kind` 是其读侧协调的基础）；D 独立于 A/B/C，仅 D3 的提示内容在 C 落地后才会涵盖归纳类变更。

### 1.2 非目标

- **不引入数值置信度**：矛盾表达交给取代关系与正文文本，不新增需要调参、难以审计的浮点分数。
- **不引入独立演变史表**：归档 member 已提供历史版本可查能力，重复引入会与其职责重叠。
- **不改变自动注入的 query-independent 语义**：`injection_score` 仍为纯函数优先级，不因本次变更获得"相关性"含义。
- **不改变 `similarity_threshold` 的单一用途**：它仍只用于写入去重；取代关系的判定不得复用量纲不同的该阈值。
- **不改动 `MemoryLayer` 语义**：`Global` / `Project` 仍是作用域维度，创建后不可变（不变量 M2）。
- **不引入跨进程写者**：本次不触碰 revision CAS 与 shared lease 协议。
- **不引入独立归纳层**：不新增产物类型与后台归纳流程；归纳由既有反思承担（§6）。
- **不引入滞后判定**：记忆在本项目是辅助信息源而非主要知识源，当前对话 context 优先级更高，记忆滞后多被当前 context 自然纠正（依据见 §13）。
- **不引入分层读取策略**：它依赖滞后判定与多轮 agent 决策，本场景不适用。
- **不引入巩固水位**：归纳改由全量反思承担，不存在"增量归纳进度"的需求。

---

## 2. 现状与差距

### 2.1 一项必须先澄清的术语错位

外部系统的"层"与本项目的 `MemoryLayer` **不是同一个维度**：

| 概念 | 外部系统 | aemeath |
|---|---|---|
| 层的含义 | 认知梯度（提炼深度：原始事实 → 观察 → 心智模型） | **作用域**（`Global` 跨项目 / `Project` 项目内） |
| 层是否可变 | 事实随归纳升级为观察 | 创建后不可变（不变量 M2） |

**结论**：`MemoryLayer` 与认知梯度正交。本设计的所有变更都不改变 `MemoryLayer` 语义，也不与 M2 冲突。

### 2.2 四项差距

| 项 | 本项目现状 | 差距 |
|---|---|---|
| 取代关系 | 相似（Jaccard ≥ 阈值）则合并；不相似则并存；`outdated` 为不可逆二分标记 | 语义相反的两条记忆判定为"不同记忆"而**并存**，注入时同时出现 |
| 证据指针 | `confirmation_count` 单调递增；合并行为是"tags 取并集 + touch" | 被合并条目正文不可查；无法回答证据来源；计数混合了两种来源 |
| 跨条目归纳 | Reflection 仅从对话提炼新记忆（`MemorySuggestion` → `MemoryEntry`）；已有记忆只被读取用于判断过时 | 多条相关记忆无法形成结论；无"某条结论由哪些记忆支撑"的表达 |
| 注入与提醒 | 注入预算为固定 300 token + 5 条双约束；memory block 每轮重算；自动反思完成无任何提示 | 预算不随窗口缩放；prompt cache 稳定性受损；用户与 LLM 均不知记忆已更新 |

外部系统的对应做法与证据出处见 [05-hindsight-research.md](../../design/02-modules/memory/05-hindsight-research.md)（§2 优势、§3 记忆架构、§4 记忆链路）。

---

## 3. 变更依赖与实施顺序

```text
变更 A（取代关系）──┬──> 变更 B（证据指针 + kind）──> 变更 C（反思承担归纳职责）
                    │                                      │
              共用"记忆间关系"的字段与不变量                 └── 读侧协调依赖 B 的证据指针与 kind

变更 D（注入与提醒机制）── 独立于 A/B/C
```

| 顺序 | 变更 | 前置 | 可独立交付 |
|---|---|---|---|
| 1 | A 取代关系 | 无 | 是 |
| 2 | B 证据指针（含 `kind`） | A（共用关系字段的设计与不变量模式） | 是 |
| 3 | C 反思承担归纳 | B（归纳产物以证据指针与 `kind` 标记来源，读侧据此协调） | 是（但读侧协调不完整） |
| 4 | D 注入与提醒机制 | 无（D3 的归纳类提示受益于 C） | 是 |

**不建议 A/B/C 并行推进**：三者都在扩展同一个聚合（`MemoryEntry`）的字段与语义，并行会产生反复的序列化格式变更与测试返工。**D 可与任一变更并行** —— 它改的是注入与提醒链路，不触碰 `MemoryEntry` 结构。

---

## 4. 变更 A：取代关系

### 4.1 目标行为

一条记忆可以被另一条记忆**明确取代**；被取代的条目在自动注入中被硬过滤，但仍可由显式检索查到并携带状态；取代关系可沿链上溯且保证无环。

与既有 `outdated` 的语义分工：

| 标记 | 语义 | 可逆性 | 注入 |
|---|---|---|---|
| `outdated` | 不再适用（原因未必是"被谁取代"） | 不可逆（M5 不变） | 硬过滤 |
| `superseded_by` | **明确被另一条记忆替代** | 可追溯、可人工解除 | 硬过滤 |

### 4.2 领域模型变更

`MemoryEntry` 新增一个字段：

```rust
struct MemoryEntry {
    // ...（既有字段不变）
    /// 被哪条记忆取代；None = 未被取代。
    #[serde(default)]
    superseded_by: Option<MemoryId>,
}
```

**只记录单向关系**，不新增 `supersedes: Vec<MemoryId>`：

- 反向关系（"我取代了谁"）可沿全量条目扫描得出，本项目数据规模（`max_entries` 量级）下成本可忽略。
- 双向记录在 JSON 单 blob 存储中会产生**不一致风险**（一侧写入失败即出现互相矛盾的记录），需要额外的一致性协议；单向记录不存在该问题。
- 取代链的上溯（无环校验）只需沿 `superseded_by` 单向遍历。

### 4.3 不变量变更

| # | 新增/变更 | 内容 | 守护点 |
|---|---|---|---|
| **M9** | 新增 | **取代关系无环**：沿 `superseded_by` 上溯不得回到自身 | 建立关系时校验；违反返回结构化错误，**NEVER** 静默写入 |
| **M10** | 新增 | **被取代条目不可注入**：`is_injection_eligible` 硬过滤 `superseded_by.is_some()` | 与 M5（outdated）、M8（TTL）同一层，pinned **NEVER** 绕过 |
| M5 | 不变 | `outdated` 不可逆且不可注入 | 语义不变，与 M10 并存 |

`is_injection_eligible` 变更后的语义（纯函数，保持无外部状态）：

```rust
fn is_injection_eligible(entry: &MemoryEntry, now: u64) -> bool {
    entry.superseded_by.is_none()
        && !entry.outdated
        && !entry.is_ttl_expired(now)
}
```

### 4.4 端口与消费方变更

`MemoryPort` **不新增**建立关系的方法 —— 取代关系由 Reflection apply 承载，避免开放一个可被任意调用方滥用的写入口：

```rust
struct MemorySuggestion {
    layer: MemoryLayer,
    category: MemoryCategory,
    content: String,
    tags: Vec<String>,
    reason: String,
    /// 本建议取代哪些已有记忆（apply 时建立 superseded_by = 新条目 id）。
    #[serde(default)]
    supersedes: Vec<MemoryId>,
}
```

`apply_reflection` 的行为扩展：

1. 逐条 `suggested_memories` 走既有路径（去重 → 容量 → 归档重试）得到新条目 id；
2. 对新条目 id 与 `supersedes` 列表中的每个旧 id 建立 `superseded_by` 关系；
3. **环检测**：任一关系会使链成环时，跳过该条关系并计入 `ReflectionApplyResult` 的失败计数，**NEVER** 部分写入半条链；
4. 扩展 apply 结果：

```rust
struct ReflectionApplyResult {
    suggestions_added: usize,
    outdated_marked: usize,
    superseded: usize,          // 新增：成功建立的取代关系数
}
```

消费方变更：

| 消费方 | 变更 |
|---|---|
| `retrieve_for_inject` | 被取代条目在 eligibility 阶段被过滤（M10） |
| `search` | 被取代条目**仍可检索**，状态经 hit metadata 无损表达（新增 `superseded_by` 字段到结果） |
| Memory Tool | `search` / `list` 的 text 投影需体现取代状态；tool description 增加"取代关系"的使用策略 |
| Context Management | 无需变更（注入仍消费 eligible 集合，过滤发生在 Memory 侧） |

### 4.5 持久化与兼容

- 新字段使用 `#[serde(default)]`，旧持久化文件反序列化得到 `None`，**无需数据迁移**。
- writer 统一输出新字段；reader 保持对旧格式的容忍（沿用既有 reader serde alias 的兼容模式）。
- 若持久化层配置了 `deny_unknown_fields`，需确认并调整——**这是实现前必须验证的前置条件**。

### 4.6 测试策略

| 层 | 覆盖目标 |
|---|---|
| L0 | 编译期：`superseded_by` 参与 `MemoryEntry` 构造的所有点，不留未初始化路径 |
| L1 | `is_injection_eligible` 对被取代条目返回 false；即使 `pinned=true` 仍被过滤；取代链环检测的纯函数；`superseded_by` 的 serde 往返（含缺失字段的旧格式） |
| L2 | `write` + `apply_reflection` 建立取代关系；部分失败时结果计数如实反映；`retrieve_for_inject` 过滤被取代条目；`search` 仍返回被取代条目并携带状态 |
| L3 | `MemoryPort` 契约：`NoOpMemory` 的 `apply_reflection` 返回计数为 0；持久化格式向前兼容（旧文件可读、新文件可被旧 reader 忽略新字段） |
| L4 | 场景：Reflection 产出取代建议 → 应用 → 被取代记忆不再注入 → 显式 `search` 仍可见其取代状态 |
| L5 | 不适用（无进程级、平台级行为） |

**TDD 顺序**：先写 L1 的 eligibility 与环检测测试（表达期望行为），再改领域函数；L2/L3 随后。

### 4.7 风险与回滚

| 风险 | 缓解 |
|---|---|
| LLM 误判取代，隐藏仍有价值的记忆 | 关系可追溯（`superseded_by` 保留）、`search` 可见、`restore` 可恢复；tool description 明确"仅在明确冲突时取代" |
| 取代链成环导致上溯死循环 | M9 保证写入时即拒绝成环；上溯实现仍设最大深度护栏作为纵深防御 |
| 旧版本读取新文件报错 | 实现前验证 reader 对未知字段的容忍度（§4.5 前置条件） |

**回滚**：停止写入 `superseded_by`（字段保持 `None`）即退化为原行为，无需数据回滚——该字段不参与除 eligibility 与展示外的任何逻辑。

---

## 5. 变更 B：证据指针

### 5.1 目标行为

写入去重命中时**不再丢弃被合并条目的信息**：被合并条目进入归档，活动条目持有指向它们的证据指针。由此可回答"这条记忆吸收了哪些内容"，并**同时为归纳产物提供"结论由哪些记忆支撑"的表达能力**（变更 C 复用同一字段）。

本变更同时引入 `kind` 字段（§5.2）：它是「来源仍是 active 还是已归档」的显式标记，也是读侧协调（§8.3）的判断依据。

与变更 A 的关系表达区分：

| 关系 | 字段 | 语义方向 |
|---|---|---|
| 替代 | `superseded_by`（变更 A） | 横向：A 不再适用，B 取代它 |
| 支撑 | `evidence`（变更 B） | 纵向：A 是 B 的来源，B 吸收了 A |

### 5.2 领域模型变更

```rust
struct MemoryEntry {
    // ...（既有字段与变更 A 的 superseded_by 不变）
    /// 记忆类型：原始记忆，或由多条记忆归纳而来的结论。
    #[serde(default)]
    kind: MemoryKind,
    /// 来源条目：本条目由这些条目**合并**（变更 B）或**归纳**（变更 C）而来。
    /// 可能指向同层 archive 条目，也可能指向 active 条目。
    #[serde(default)]
    evidence: Vec<MemoryId>,
}

/// 记忆类型维度。与 `MemoryCategory` **正交**：分类表达用途，类型表达来源。
enum MemoryKind {
    /// 原始记忆（默认值；合并产物亦属此类——其来源已归档，不参与注入）
    Raw,
    /// 由多条记忆归纳而来的结论（其来源仍可能在 active，注入时需让位，见 §8.3）
    Synthesized,
}
```

**为什么需要 `kind` 字段（而非用 `evidence.len() >= 2` 推导）**：合并与归纳**都会**产生多条 `evidence`，但两者的读侧行为相反 ——

| 来源 | `evidence` 指向 | 读侧行为 |
|---|---|---|
| 合并（变更 B） | 同层 archive 条目 | 无特殊处理（来源不在 active 池，本就不参与注入） |
| 归纳（变更 C） | 仍在 active 的原始事实 | **需要让位**（避免结论与来源重复占预算，§8.3） |

因此必须显式标记类型，不能从证据条数推导。

### 5.3 合并语义变更

**现状**：Jaccard ≥ `similarity_threshold` 时合并，行为是"tags 取并集 + touch"，被合并条目的正文不再出现在可检索集合中。

**变更后**：

```text
write(entry)
  ├─ 去重命中（Jaccard ≥ similarity_threshold）
  │    ├─ 保留**已存在**的条目为 active（tags 取并集 + confirmation_count 递增，行为不变）
  │    ├─ 新写入条目**归档**至同层 archive（不删除）
  │    └─ 新条目 id 追加进 active 条目的 evidence
  └─ 未命中 → 走既有容量检查路径（不变）
```

> 本节描述的是**合并**（写入去重命中）场景；**归纳**（反思产出结论）场景下 `evidence` 由 `synthesizes` 填充，见 §6.3。两个场景共用同一字段，但触发来源不同。

**为什么保留已存在的条目为 active**：活跃条目集合保持稳定。若改为"新条目顶替为 active"，会导致同一内容反复归档/恢复、`pinned` 语义复杂化、以及 revision 频繁变化带来的跨实例 CAS 抖动。新写入的内容仍然可查（在 archive 中且被指针引用）。

**`confirmation_count` 语义澄清**（不改变数值行为，只明确含义）：它表示"该内容被确认的次数"，包含重复写入与相似合并两种来源（维持既有 `saturating_add(1)` 行为，因此 `injection_score` 与 `eviction_score` 的标定不变）。"被合并了多少条"这一新信息由 `evidence.len()` 独立表达，**NEVER** 与 `confirmation_count` 混用。

### 5.4 不变量变更

| # | 新增/变更 | 内容 | 守护点 |
|---|---|---|---|
| **M11** | 新增 | **证据指针指向真实条目**：每个 `evidence` id 必须能在同层 active 或 archive 中找到 | 写入时校验；归档清理流程 **MUST** 保留被引用的条目，**NEVER** 静默删除被引用条目 |
| M4 | 不变（语义澄清） | `confirmation_count` 单调递增 | 递增规则不变，含义按 §5.3 澄清 |

### 5.5 端口与消费方变更

`MemoryPort` **不新增**方法：证据指针完全由 `write` 的内部合并路径建立。

消费方变更：

| 消费方 | 变更 |
|---|---|
| `write` | 命中去重时改为"归档新条目 + 记录指针"（行为变更） |
| `restore` | 允许恢复被引用的归档条目；恢复后指针保持有效 |
| `search` | 命中条目携带 `evidence` 计数与 id 列表（管理可见性） |
| Memory Tool | `list` 输出体现 evidence 数量；tool description 说明合并保留的来源 |
| `compact` | 归档清理 **MUST** 排除被 evidence 引用的条目 |

### 5.6 持久化与兼容

- `#[serde(default)]`，旧文件得到空 `Vec`，无需数据迁移。
- **历史数据无回溯**：本变更**不**追溯性地为既往已合并的条目重建证据链（信息在变更前已丢失）。实现时 **MUST** 在文档与 tool 说明中标注该边界，**NEVER** 伪造历史指针。

### 5.7 测试策略

| 层 | 覆盖目标 |
|---|---|
| L0 | 编译期：`evidence` 参与构造的所有点 |
| L1 | 合并后 `evidence` 内容正确；归档条目确实存在且内容完整；`evidence` 的 serde 往返；旧格式（无该字段）读取 |
| L2 | `write` 去重命中路径：active 集合不变、archive 新增一条、指针建立；未命中路径行为不变 |
| L3 | 持久化格式兼容；`NoOpMemory` 行为不变 |
| L4 | 场景：写入相似内容 → 发生合并 → 新旧内容均可查（分别位于 active 与 archive）→ 指针可追溯 |
| L5 | 不适用 |

### 5.8 风险与回滚

| 风险 | 缓解 |
|---|---|
| archive 持续增长（合并不再销毁信息） | 归档清理必须排除被引用条目；容量策略见 §14 开放问题 |
| 指针悬空（被引用条目被清理） | M11 守护 + 清理流程前置校验 |
| 恢复归档条目后与 active 内容重复 | `restore` 走既有容量与去重路径；重复时按 §5.3 规则再次合并 |

**回滚**：停止归档与指针写入（`evidence` 保持空）即退化为原行为；已产生的归档条目与指针不影响除展示与清理外的任何逻辑。

---

## 6. 变更 C：反思承担归纳职责

### 6.1 目标行为

在既有 Reflection 的职责（从对话提炼新记忆、标记过时记忆）之上增加一项：**从已有记忆中归纳跨条目的结论**，并以证据指针标记结论的来源。

**为什么由反思承担而非新建归纳机制**：独立归纳层会引入第二个生命周期（写入 → 待归纳 → 已归纳 → 滞后判定），其配套机制（巩固水位、滞后判定、分层读取）在本项目场景中收益不足（§13）。而反思**本来就在读取记忆**（用于判断哪些已过时），让它顺带回答"这些记忆能否归纳出一条结论"，边际成本接近零。

### 6.2 触发方式：沿用既有反思触发

不新增触发条件，沿用现有三种（Interval / PreCompact / Manual）。

| 取舍 | 说明 |
|---|---|
| **代价** | 归纳是"每 N 轮批量做"（`interval_runs` 默认 10），存在最多 N 轮的归纳延迟，不是"写入即归纳" |
| **接受理由** | 本项目记忆是辅助信息源，当前对话 context 优先级更高（§1.2）；归纳延迟在本场景不构成问题 |
| **后续空间** | 若未来需要更及时，可单独增加触发条件，不影响本设计的其余部分 |

### 6.3 领域模型变更

复用既有 `MemorySuggestion`，新增一个字段（与变更 A 的 `supersedes` 平行）：

```rust
struct MemorySuggestion {
    layer, category, content, tags, reason,
    supersedes: Vec<MemoryId>,     // 变更 A：取代的已有记忆
    /// 本建议归纳自哪些已有记忆（apply 后成为新条目的 evidence）。
    #[serde(default)]
    synthesizes: Vec<MemoryId>,
}
```

apply 流程：

1. 写入新条目（走既有路径：去重 → 容量 → 归档重试）
2. 置新条目的 `kind = Synthesized`（变更 B 引入的类型字段）
3. 把 `synthesizes` 写入新条目的 `evidence`（变更 B 的字段）

**"某条记忆已被归纳吸收"是推导出来的，不在原始记忆上加标记**：读取时反向查询"是否存在某条 active 条目的 `evidence` 包含它"。理由与变更 A 相同 —— 单向记录避免 JSON 单 blob 下的双向不一致风险；数据规模（`max_entries` 量级）下反向扫描成本可忽略。

### 6.4 归纳产物的约束

| 约束 | 内容 |
|---|---|
| 类型维度 | 归纳产物**仍是 `MemoryEntry`**，不新增产物类型；与 `MemoryCategory` 正交（分类表达用途，归纳表达来源） |
| 证据下限 | `evidence.len() >= 2` —— 少于两条来源的"归纳"等价于复制既有条目，**MUST NOT** 产出 |
| 类型标记 | 归纳产物置 `kind: Synthesized`（变更 B 引入的字段）—— 该字段是读侧协调（§8.3）的判断依据 |
| 演变叙事 | 结论被新证据修正时用正文文本表达（形如"曾…现…"），**NEVER** 引入数值置信度 |

### 6.5 反思 prompt 变更

反思 prompt 增加一个环节：在读取已有记忆时，除判断"哪些已过时"外，还要判断"**是否存在可归纳的组合**"，并在输出中给出候选结论与来源 id 列表。

**风险**：一个 prompt 承担两个任务（提炼新记忆 + 归纳已有记忆）可能互相干扰。缓解：实现时分别验证两类产出的质量（§11），必要时拆成两次调用 —— 这属于实现细节，不改变本设计的接口。

### 6.6 候选集问题（已知限制）

归纳质量取决于反思时能看到多少相关记忆。当前 Reflection 读取的记忆集合可能不足以支撑归纳。**这是本设计的已知限制**：

- 候选集过小 → 归纳机会被漏掉（**不会产出错误结论，只是少产出**）
- 候选集过大 → prompt 膨胀（既有约束，非本次引入）

**明确不设计**：本设计**不**引入"反思前的相关记忆检索"机制。理由：它属于读链路的独立问题（如何为反思选择输入），且现有实现已能工作。实现时若确认候选集不足，另行设计。

### 6.7 风险与回滚

| 风险 | 缓解 |
|---|---|
| LLM 过度概括，产出失真结论 | 证据指针可回溯核查；`evidence.len() >= 2` 门槛阻止"单条记忆的伪归纳" |
| 归纳结论与来源事实重复注入 | 由读链路协调（§8.3）：注入时结论优先、被引用事实降权；显式检索保持两者完整可见 |
| prompt 任务混杂导致两类产出质量下降 | 实现时分别验证；必要时拆调用（§6.5） |

**回滚**：停止输出 `synthesizes`（字段保持空）即退化为原行为 —— 反思仅提炼新记忆，不产出归纳结论。

---

## 7. 变更 D：注入与提醒机制

变更 A / B / C 处理"记忆如何被写入和维护"，本变更处理"记忆如何进入当前对话的上下文"与"记忆变化如何被感知"。

### 7.1 D1：注入预算按比例计算

**现状**：`inject_count`（默认 5 条）与 `inject_token_budget`（默认 300 token）**双约束**。

**问题**：

- 固定 token 预算不随上下文窗口缩放 —— 大窗口模型浪费空间，小窗口模型可能超支
- 条数与 token 双约束语义重叠：实际生效的是两者中更严格的那个，难以推理

**变更**：

| 项 | 变更 |
|---|---|
| `inject_count` | **移除**（配置项废弃） |
| `inject_token_budget` | 固定值 → **按比例计算**：`context_size / 50`（**2%**） |

**实现落点**：`context/src/domain/token_budget.rs` 已有同类比例函数（`summary_budget`、`compact_tail_token_cap`），新函数置于同处，与既有函数共享命名与文档风格。

**token 计数**：沿用现有 `estimate_tokens` 估算（模块注释明确 "uses estimation algorithms, not actual tokenizers"）与 runtime 维护的 EMA 校准系数（`estimate_tokens_with_ratio`）。

### 7.2 D2：注入时机改为「首次注入 + compact 后刷新」

**现状**：memory block 在**每次** `build_window` 时重新 materialize（`context/src/application/service.rs` 中 `self.memory.materialize(request)` 无条件调用）。虽然 SystemBlock 标记为 `cacheable: true`（"低频变化"假设），但每轮都会执行检索，且内容变化会破坏 provider 的 prompt cache 命中。

**变更**：

```text
Session 首次 build window  →  注入记忆，标记为已注入
后续 build window          →  复用已注入内容（冻结）
Compact 完成后             →  刷新注入（重新检索 + 替换）
```

**明确只有这两个时机**：Session 首次、Compact 后。**没有其他刷新点** —— reflect 完成不刷新注入，只发提示（§7.3）。

**为什么冻结**：system prompt 是 cacheable prefix，其稳定性直接决定 prompt cache 命中率；每轮重算使 cache 收益不可预测。这与既有 guidance 部分的 Session 冻结语义一致（现有 reminder 文案："This Session's frozen system prompt remains unchanged"）。

**为什么 compact 后必须刷新**：compact 会重写对话历史（压缩为摘要），上下文结构已变化；若记忆内容不同步，可能与压缩后的摘要不一致。同时 compact 是天然的"重新规划"时点。

### 7.3 D3：reflect 完成后双通道提示

**现状**：自动反思完成后**没有提示链路** —— SDK 层只有 `ReflectionHistory`（`/memory reflect` 的查询结果），没有"反思完成"的推送事件；`ReflectionOutput.user_alert` 字段与格式化逻辑存在，但未搜到流向 TUI 的路径。

**变更**：apply 产生变更时（`N > 0`）通过两条通道提示：

| 消费方 | 通道 | 内容 |
|---|---|---|
| **LLM** | `InvocationReminderData` 新增变体 → `<system-reminder>` | "记忆已更新 N 条，如需最新信息可用 memory tool 的 list / search 查看" |
| **TUI** | `RuntimeStreamEvent::SystemMessage` → `ChatEvent::SystemMessage` → `append_system_notice` | "记忆已更新 N 条" |

**LLM 侧的三条约束**：

1. **仅下一轮注入一次** —— 不重复占用 token。
2. **必须指路** —— 只说"更新了"而不告知如何查看，LLM 可能不知道 memory tool 支持 `list` / `search`（工具 action 清单：`add` / `delete` / `search` / `pin` / `list` / `archive` / `restore` / `add_reminder` / `complete_reminder`）。
3. **不给具体内容** —— 内容让 LLM 按需检索，避免把可能已过时的记忆塞进上下文。

**TUI 侧**：复用既有 system message 通道（`append_system_notice` 是"替代旧的命令式 `OutputArea::push_system` 的唯一入口"），**不新增渲染机制**。

**顺带接线 `user_alert`**：其字段与格式化逻辑已存在（渲染为"用户提醒：…"），但未接入消费路径；本次一并接上 TUI 通道。

`user_alert` 与本设计的系统通知**语义不同，并存不合并**：

| | `user_alert` | 系统通知 |
|---|---|---|
| 来源 | **LLM 主动产出**（反思输出字段） | **系统自动**（apply 有变更即发） |
| 内容 | 开放，由 LLM 决定 | 固定格式（含计数） |
| 触发 | LLM 认为必要时 | 有变更时（零变更不发） |

**零变更不提示**：`N == 0` 时两条通道都不发。

### 7.4 D4：reflect 模型可跨 provider 指定（既有能力）

`memory.reflection.model` 的配置值格式为 `provider/model`（`share/src/config/domain/memory.rs` 测试用例为 `Some("test/model")`），支持在**全部 provider 的模型**中指定。

本设计**记为既有能力，不做变更**。此处记录它是为明确"反思可使用与主会话不同的模型"这一前提。

### 7.5 附带修正：compact 相关比例

| 预算 | 现值 | 目标值 | 说明 |
|---|---|---|---|
| `summary_budget` | `context_size / 50`（2%） | `context_size / 20`（**5%**） | compact 摘要预算 |
| `compact_tail_token_cap` | `context_size / 20`（5%） | **3%** | compact 保留尾部消息预算 |

**实现注意**：3% 无法用整数除法简洁表达，实现时应写为 `context_size * 3 / 100`（或等价的命名常量），**NEVER** 使用 `context_size / 33` 这类魔法除数。

**调整理由**：原组合下摘要预算偏紧、尾部保留偏松。调整后摘要获得更大空间（承载历史信息），尾部收紧（只保留最近上下文）。

### 7.6 风险与回滚

| 风险 | 缓解 |
|---|---|
| 注入冻结后，会话中途的重要记忆变更不被注入 | 由 reflect 后的 reminder 提示 LLM 主动检索（§7.3）；compact 后自然刷新 |
| 比例预算在大窗口模型上绝对值变大 | 比例随 `context_size` 缩放是设计意图；若实测过量可下调分母 |
| 移除条数上限后注入大量极短条目 | token 预算仍是硬上限；极短条目的 token 成本本就很低 |
| compact 后刷新破坏 provider cache | 刷新仅发生在 compact 后（低频），单次 miss 可接受 |

**回滚**：注入时机与预算通过配置回退（提供开关或保留旧配置解析）；比例函数改回固定值即可。

---

## 8. 读链路设计

变更 A / B / C 都在扩展**写入侧**（字段、关系、产出）。本节补齐**读取侧**：记忆写入之后，如何在三种消费场景中被正确读取。

### 8.1 三条读路径与决策者

| 读路径 | 入口 | 当前决策者 |
|---|---|---|
| 自动注入 | `retrieve_for_inject` | **无**（query-independent 纯函数，不 touch、不落盘） |
| 显式检索 | `search` + Memory Tool | **无模型参与**（词法匹配排序） |
| 反思输入 | Reflection 的 prompt 构建 | 无读取期模型参与 |

注入的**时机**（首次注入 + compact 后刷新，冻结语义）见 §7.2；本节处理"读什么、怎么排序、什么可见"。

**读取期无模型参与这条约束决定了本节的全部设计**：三条路径在读取期都没有模型参与，因此

- 状态语义必须由**系统侧**处理（不能"暴露给模型自行判断"）
- 读取策略必须是**确定性规则**（不能用"够不够"这类语义判断）

### 8.2 可见性矩阵

| 条目状态 | 自动注入 | 显式检索（`search`） | 反思输入 |
|---|---|---|---|
| active | ✓ 参与 `injection_score` 排序 | ✓ 参与相关性排序 | ✓ |
| `pinned` | ✓ 最高优先 | ✓ | ✓ |
| `outdated` | ✗ 硬过滤（M5） | ✓ 携带状态 | ✗ 排除（M13） |
| 被取代（`superseded_by`） | ✗ 硬过滤（M10） | ✓ 携带取代状态 | ✗ 排除（M13） |
| TTL 过期 | ✗ 硬过滤（M8） | ✓ 携带状态 | ✗ 排除（M13） |
| 已合并归档（archive） | ✗ | ✓ 可显式检索 | ✗ |
| **归纳产物**（`kind == Synthesized`） | **优先填充**；其来源事实让位（§8.3） | ✓ 与来源事实平等可见 | ✓ |

**三条规则**：

1. **反思输入 MUST 排除失效条目**（`outdated` / 被取代 / TTL 过期）。若反思读入失效条目，LLM 会基于过时信息产出新建议，形成**污染循环**（建议 → 写入 → 下次反思基于它再产出建议）。
2. **显式检索 MUST 携带状态**。让调用方知道"这条已被取代 / 已过时"，否则会得到"看似有效实则失效"的记忆（对齐检索不变量 R4：状态由 metadata 无损表达）。
3. **已被归纳吸收的原始事实：注入降权、检索不降权**（§8.3）。

### 8.3 归纳产物与来源事实的读取协调

**方案**：**覆盖式让位**（overlay），依据是注入预算紧（`context_size / 50`，§7.1）而检索预算宽。

| 场景 | 规则 | 理由 |
|---|---|---|
| 自动注入 | **覆盖式让位**（算法见下） | 预算紧张，避免"结论 + 来源事实"重复占用 |
| 显式检索 | 两者**平等可见，都不让位** | 检索是"按需查细节"的路径，排除来源事实等于丢失细节可达性 |
| 反思输入 | 两者**都可见** | 反思需要看到来源事实，才能判断结论是否仍然成立 |

**填充算法**：

```text
1. 按 injection_score 排出全部候选
2. 先把 kind == Synthesized 的条目按序填入 token 预算，
   标记其 evidence 指向的条目为「已覆盖」
3. 剩余预算按序填充未被标记的条目（含未被覆盖的原始事实）
4. 未被选中的 Synthesized 条目不产生任何标记（其来源正常参与）
```

**为什么不用固定降权系数**：

- 固定系数（如 `injection_score × 0.5`）会在"结论未被选中"时**误伤**其来源事实 —— 来源被降权却没换来结论入注
- 固定系数引入魔数，且多出一个需要标定的参数
- 覆盖式让位**不改分数、只改填充顺序**，保持 `injection_score` 的 query-independent 语义纯净（§1.2 非目标）

**不设 Synthesized 配额上限**：若一个结论的 score 高到能挤占全部预算，说明它确实最重要；设配额反而会排除最相关的结论。

**"已覆盖"的判定时机是注入时计算**（而非写入时标记）：

- 注入前扫描 active 条目的 `evidence` 构建被引用集合
- `max_entries` 量级（100 条）下扫描成本是微秒级
- 避免在原始记忆上引入 `synthesized_into` 双向指针 —— JSON 单 blob 下存在不一致风险（与变更 A 的取舍一致）
- 若未来规模增长，可在此时机改为反向指针（本设计不预设）

### 8.4 证据在读取时的消费

| 消费方式 | 说明 |
|---|---|
| 检索结果显示证据数量 | `evidence.len()` 作为"该条结论由 N 条记忆支撑"的可信度信号，**不参与排序** |
| 按证据反查源条目 | 管理用途：由归纳产物查到其来源（archive 或 active） |

**明确不引入证据强度的排序加权**：`injection_score` 的既有语义是 query-independent 优先级，引入"证据强度"会使其获得相关性含义（违反 §1.2 非目标）。若未来引入语义排序能力，应在标定后另行决定。

---

## 9. 不变量变更汇总

| # | 状态 | 内容 | 来源变更 |
|---|---|---|---|
| M1–M3 | 不变 | id 唯一 / layer 不可变 / content 非空 | — |
| M4 | 语义澄清 | `confirmation_count` 单调递增（含义按 §5.3 明确，行为不变） | B |
| M5 | 不变 | `outdated` 不可逆且不可注入 | — |
| M6–M8 | 不变 | pinned 不被淘汰 / active 容量上限 / TTL 过期不注入 | — |
| **M9** | 新增 | 取代关系无环 | A |
| **M10** | 新增 | 被取代条目不可注入（pinned 不可绕过） | A |
| **M11** | 新增 | 证据指针指向真实条目；清理不得删除被引用条目 | B |
| **M12** | 新增 | **反思输入排除失效条目**：`outdated`、被取代、TTL 过期的条目 **NEVER** 进入反思上下文（§8.2） | A |
| **M13** | 新增 | **归纳产物证据下限**：`evidence.len() >= 2`，单条来源的"归纳" **MUST NOT** 产出（§6.4） | C |

---

## 10. 端口与型号变更汇总

| 对象 | 变更 | 来源 |
|---|---|---|
| `MemoryEntry` | 新增 `superseded_by: Option<MemoryId>`、`kind: MemoryKind`、`evidence: Vec<MemoryId>` | A / B |
| `MemorySuggestion` | 新增 `supersedes: Vec<MemoryId>`、`synthesizes: Vec<MemoryId>` | A / C |
| `ReflectionApplyResult` | 新增 `superseded: usize` | A |
| `MemorySearchResult` hit | 新增取代状态字段 | A |
| `apply_reflection` | 接受 `synthesizes`，置 `kind = Synthesized`，写入 `evidence` | C |
| `write` | 合并路径行为变更（归档 + 指针） | B |
| `compact` | 归档清理排除被引用条目 | B |
| `retrieve_for_inject` | 覆盖式让位：`kind == Synthesized` 优先填充，其来源事实让位（§8.3） | C |
| `MemoryConfig` | **移除** `inject_count`；`inject_token_budget` 改为比例 `context_size / 50` | D1 |
| 注入时机 | 由每轮重算改为「Session 首次 + compact 后刷新」（§7.2） | D2 |
| `InvocationReminderData` | 新增变体（记忆已更新，仅下一轮注入一次） | D3 |
| `RuntimeStreamEvent::SystemMessage` | 复用既有通道下发 TUI 提示；同时接线 `user_alert` | D3 |
| `token_budget` | `summary_budget` 2% → 5%；`compact_tail_token_cap` 5% → 3% | D5 |

**不新增端口方法**：归纳由既有 `apply_reflection` 承载（§6.3），注入降权在既有路径内实现，因此 `MemoryPort` 的**方法集合不变**。

**全部字段新增均使用 `#[serde(default)]`**，不产生破坏性格式变更。

---

## 11. 测试策略总览（L0–L5）

| 层 | 本变更的覆盖目标 |
|---|---|
| L0 编译期 | 新字段参与 `MemoryEntry` 构造的所有点；架构守卫无新增违规 |
| L1 单元测试 | eligibility 三个新条件的纯函数；取代链环检测；证据指针校验；归纳证据下限（`evidence.len() >= 2`）；**比例预算函数的边界值（含 3% 的整数实现）**；全部字段的 serde 往返（含旧格式缺字段、`inject_count` 残留配置被忽略） |
| L2 模块协作 | `write` 合并路径；`apply_reflection` 的取代路径与归纳路径（`synthesizes` → `kind` + `evidence`）；三消费场景的状态分工（§8.2 可见性矩阵逐格）；**覆盖式让位算法（§8.3）**；**注入时机：首次注入后冻结、compact 后刷新（§7.2）** |
| L3 契约测试 | `apply_reflection` 扩展行为契约；`NoOpMemory` 行为；持久化格式向前兼容；**系统提示 reminder 的新变体契约（仅下一轮一次）** |
| L4 场景测试 | 取代后不再注入但可检索；合并后证据可查；归纳结论可产出且可回溯到全部来源事实；**注入时结论优先、来源事实让位，同时显式检索中两者都完整可见**；反思不基于被取代条目产出新建议（污染循环回归）；**compact 后注入刷新**；**反思完成后 LLM 收到一次 reminder、TUI 收到一次提示，零变更时都不发** |
| L5 系统 smoke | 不适用（无进程级、平台级行为变更） |

**跨层纪律**：每个相邻边界都必须有证据，**NEVER** 只测源头（写入字段）与末尾（注入结果）而遗漏中间层（apply 建立关系、eligibility 过滤、持久化往返）。

---

## 12. 验收标准

变更 A / B / C 各自独立验收：

- [ ] A：被取代条目不出现在自动注入中，即使 `pinned=true`；仍可由显式 `search` 检索并携带取代状态；成环的关系被拒绝且不产生半写入
- [ ] B：合并发生后，新旧内容均可查（active 与 archive），证据指针可追溯；归档清理不删除被引用条目；历史已合并条目**不**被伪造指针
- [ ] C：反思能产出跨条目归纳结论，其 `evidence` 可回溯到全部来源记忆；`evidence.len() < 2` 的"归纳"不会产出（M13）；停止输出 `synthesizes` 后行为退化为原状
- [ ] 读链路（§8.2 / §8.3）：反思输入排除失效条目，**污染循环不可复现**；`search` 结果携带取代 / 过期状态；**注入时归纳结论优先、被引用事实降权，而显式检索中两者都完整可见**
- [ ] D：注入预算按 `context_size / 50` 计算且条数上限已移除；注入在 Session 首次后冻结、compact 后刷新；reflect 完成且有变更时，LLM 收到一次含「记忆已更新 N 条」与查看指引的 reminder，TUI 收到一次提示；零变更时两者都不发；`user_alert` 已接入 TUI 通道
- [ ] D5：`summary_budget` 为 5%、`compact_tail_token_cap` 为 3%（后者以命名常量或 `* 3 / 100` 实现，无魔法除数）
- [ ] 全部新增字段通过旧格式读取测试（缺失字段得到安全默认值）
- [ ] 实现完成后，`docs/design/02-modules/memory/` 的 01（领域模型）、02（检索与注入）、03（Reflection）、04（端口）**MUST** 同步更新，使目标态设计与实现一致

---

## 13. 明确不采纳清单

来自外部系统但因成本或约束不适用于本项目者，记录理由以避免重复评估：

| 设计 | 不采纳理由 |
|---|---|
| store-owned 存储抽象（流式写入会话协议） | 单机 JSON 存储无第三方后端替换需求；抽象成本高于收益 |
| 双数据库后端全功能对等 | 专用后端代码量已超过主后端；不适用于单机场景 |
| 四臂检索 + RRF + cross-encoder 重排 | 依赖 embedding 与重排模型，与"Tier 1 零外部依赖"的现行决策冲突；且本项目记忆规模无此需要 |
| 独立证据引文存储 | 指针引用源条目正文即可满足审计需求 |
| 数值置信度 | 需调参且难以审计；与计数语义冲突 |
| 多租户 schema 级隔离 | 单机工具场景不适用 |
| 数据模型的多后端谓词与索引分片 | 本项目无数据库层，相关优化不适用 |
| **独立归纳层**（后台归纳流程 + 独立产物类型） | 引入第二个生命周期（写入 → 待归纳 → 已归纳 → 滞后判定），其配套机制的成本远超收益；改由既有反思承担（§6） |
| **滞后判定**（staleness） | 记忆在本项目是**辅助信息源**而非主要知识源；当前对话 context 优先级更高，记忆滞后多被当前 context 自然纠正 |
| **分层读取策略**（认知阶梯下钻） | 依赖滞后判定与多轮 agent 决策，需要读取期的模型参与；本项目三条读路径在读取期均无模型参与（§8.1） |
| **巩固水位**（`consolidated_at`） | 归纳改由全量反思承担，不存在"增量归纳进度"的需求 |

---

## 14. 开放问题

1. **合并时保留哪条为 active**：本设计采用"保留已存在条目"（活跃集合稳定）；若实践中发现新内容经常是旧内容的完整升级版，需重新评估"保留内容更完整者"方案及其对 CAS 抖动的影响。
2. **archive 容量策略**：证据指针依赖归档条目的长期保留，而现有淘汰策略只面向 active。archive 的容量上限、清理规则与被引用条目的处理需要独立设计。
3. **取代判定的归属**：由 Reflection（LLM 判定）还是写入时确定性规则判定？前者灵活但有误判成本，后者可控但覆盖有限。
4. **反思的归纳候选集**：当前反思读取的记忆集合是否足以支撑归纳？若不足，需要设计"反思前的相关记忆检索"（§6.6 已明确不在本设计范围）。
5. **反思 prompt 任务拆分**：两类任务（从对话提炼新记忆 / 从记忆归纳结论）是否应拆成两次调用，避免互相干扰（§6.5）？
6. **反思触发的空转**：现有 `interval_runs` 计数触发在无新内容时仍会调用 LLM。本设计未处理该问题（原先设想的"内容水位门控"因归纳改由反思承担而失去归属）；若需解决应单独立项。
7. **reader 兼容前置验证**：持久化 reader 是否配置 `deny_unknown_fields`，决定新字段是否会影响旧版本读取。
9. **注入冻结的失效场景**：注入冻结后，会话中途的记忆变更不进入上下文，仅靠 reminder 提示 LLM 主动检索。若实测发现 LLM 不主动检索，需要考虑额外的刷新触发点（本设计明确只有首次与 compact 两个时机）。
8. **总结所需的证据出处**：本设计的每条结论指向 [05-hindsight-research.md](../../design/02-modules/memory/05-hindsight-research.md)；该文标注的"未验证项"在实现前**MUST** 补验，**NEVER** 基于未验证结论做实现决策。

---

## 15. 相关文档

- 证据基础：[05-hindsight-research.md](../../design/02-modules/memory/05-hindsight-research.md)
- 目标态领域模型：[01-domain-model.md](../../design/02-modules/memory/01-domain-model.md)
- 目标态检索与注入：[02-retrieval-and-injection.md](../../design/02-modules/memory/02-retrieval-and-injection.md)
- 目标态 Reflection 引擎：[03-reflection.md](../../design/02-modules/memory/03-reflection.md)
- 目标态端口与适配器：[04-ports-and-adapters.md](../../design/02-modules/memory/04-ports-and-adapters.md)
- 测试分层规范：[04-testing-and-coverage.md](../../design/03-engineering/04-testing-and-coverage.md)

## 修订记录

| 日期 | 变更 |
|---|---|
| 2026-09-28 | 初稿：变更 A/B/C 的领域模型、不变量、端口与行为设计、测试策略与验收标准；变更 D 方向记录；不变量与端口变更汇总；不采纳清单与开放问题 |
| 2026-09-28 | 新增读链路设计：三条读路径与决策者、可见性矩阵（含反思输入排除规则）、证据消费、滞后判定与分层读取策略 |
| 2026-09-28 | **按方案确认重构**：删除「巩固水位」与「独立归纳层」两个变更，改为「由反思承担归纳职责」（新变更 C，沿用既有触发）；读链路精简为可见性矩阵 + 归纳产物与来源事实的读取协调（注入降权 / 检索不降权）；滞后判定与分层读取策略移入不采纳清单并记录场景依据；不变量收敛为 M9–M13 |
| 2026-09-28 | 新增**变更 D（注入与提醒机制）**：注入预算改为按比例（`context_size / 50`）并移除条数上限；注入时机改为「Session 首次 + compact 后刷新」；reflect 完成后双通道提示（LLM 单次 reminder 含查看指引 + TUI system message）；`user_alert` 接线；附带修正 compact 比例（summary 5% / tail 3%）。变更 B 补入 `kind: MemoryKind` 字段；读链路降权实现定为「覆盖式让位」 |
