# Memory 记忆架构增强变更设计（Hindsight 对照）

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/1764

**日期**：2026-09-28
**状态**：设计中（本设计只覆盖设计阶段；实现需按本设计的测试策略另行立项）
**证据基础**：[050-hindsight-memory-research.md](050-hindsight-memory-research.md)
**影响范围**：Memory BC（`agent/shared/memory/`）、Runtime（Reflection 编排与触发门控）、Tool 层（Memory Tool Published Language）、Context Management（注入 eligibility）

---

## 1. 目标与非目标

### 1.1 目标

本次变更解决 Memory BC 现状中三个**已被具体化**的缺陷，并为第四个方向留下前置条件：

| 编号 | 缺陷 | 现状后果 |
|---|---|---|
| 变更 A | 无"取代"关系 | 两条语义相反的记忆可同时被注入，模型收到互相矛盾的信息 |
| 变更 B | 合并是信息销毁式的 | 被合并条目的正文不再可查，无法回答"这条记忆吸收了哪些内容" |
| 变更 C | 无归纳进度与内容水位 | 后台反思按 Run 计数触发，无新内容时仍会空转 LLM；归纳层无增量基础 |
| 变更 D | 无归纳层 | 缺少"跨条目归纳信念"的能力（本设计只记录方向，不实施） |

变更 A、B、C **可独立交付与验证**，D 依赖三者就位。

### 1.2 非目标

- **不引入数值置信度**：矛盾表达交给取代关系与正文文本，不新增需要调参、难以审计的浮点分数。
- **不引入独立演变史表**：归档 member 已提供历史版本可查能力，重复引入会与其职责重叠。
- **不改变自动注入的 query-independent 语义**：`injection_score` 仍为纯函数优先级，不因本次变更获得"相关性"含义。
- **不改变 `similarity_threshold` 的单一用途**：它仍只用于写入去重；取代关系的判定不得复用量纲不同的该阈值。
- **不改动 `MemoryLayer` 语义**：`Global` / `Project` 仍是作用域维度，创建后不可变（不变量 M2）。
- **不引入跨进程写者**：本次不触碰 revision CAS 与 shared lease 协议。

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

| 建议项 | 本项目现状 | 差距 |
|---|---|---|
| 取代关系 | 相似（Jaccard ≥ 阈值）则合并；不相似则并存；`outdated` 为不可逆二分标记 | 语义相反的两条记忆判定为"不同记忆"而**并存**，注入时同时出现 |
| 证据指针 | `confirmation_count` 单调递增；合并行为是"tags 取并集 + touch" | 被合并条目正文不可查；无法回答证据来源；计数混合了两种来源 |
| 巩固水位 | mutation 即时写入；Reflection 按 `interval_runs` 计数触发，处理对象是消息快照 | 无"记忆条目是否已被处理"的进度；无"内容是否变化"的触发门控 |
| 归纳层 | Reflection 的 `MemorySuggestion` 直接成为普通 `MemoryEntry` | 无跨条目归纳；无证据支撑的信念类型 |

外部系统的对应做法与证据出处见 [050-hindsight-memory-research.md](050-hindsight-memory-research.md)（§2 优势、§3 记忆架构、§4 记忆链路）。

---

## 3. 变更依赖与实施顺序

```text
变更 A（取代关系）──┬──> 变更 B（证据指针）──> 变更 C（巩固水位）──> 变更 D（归纳层，暂缓）
                    │
              共用"记忆间关系"的字段与不变量
```

| 顺序 | 变更 | 前置 | 可独立交付 |
|---|---|---|---|
| 1 | A 取代关系 | 无 | 是 |
| 2 | B 证据指针 | A（共用关系字段的设计与不变量模式） | 是 |
| 3 | C 巩固水位 | 独立；作为 D 的前置 | 是 |
| 4 | D 归纳层 | A + B + C | 否（本设计只记录方向） |

**不建议并行推进 A/B/C**：三者都在扩展同一个聚合（`MemoryEntry`）的字段与不变量，并行会产生反复的序列化格式变更与测试返工。

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

写入去重命中时**不再丢弃被合并条目的信息**：被合并条目进入归档，活动条目持有指向它们的证据指针。由此可回答"这条记忆吸收了哪些内容"，并为后续归纳层提供"信念由哪些事实支撑"的表达能力。

与变更 A 的关系表达区分：

| 关系 | 字段 | 语义方向 |
|---|---|---|
| 替代 | `superseded_by`（变更 A） | 横向：A 不再适用，B 取代它 |
| 支撑 | `evidence`（变更 B） | 纵向：A 是 B 的来源，B 吸收了 A |

### 5.2 领域模型变更

```rust
struct MemoryEntry {
    // ...（既有字段与变更 A 的 superseded_by 不变）
    /// 合并来源：本条目由这些条目合并而来（指向同层 archive 中的条目）。
    #[serde(default)]
    evidence: Vec<MemoryId>,
}
```

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
| archive 持续增长（合并不再销毁信息） | 归档清理必须排除被引用条目；容量策略见 §13 开放问题 |
| 指针悬空（被引用条目被清理） | M11 守护 + 清理流程前置校验 |
| 恢复归档条目后与 active 内容重复 | `restore` 走既有容量与去重路径；重复时按 §5.3 规则再次合并 |

**回滚**：停止归档与指针写入（`evidence` 保持空）即退化为原行为；已产生的归档条目与指针不影响除展示与清理外的任何逻辑。

---

## 6. 变更 C：巩固水位

### 6.1 目标行为

1. **归纳进度**：记忆条目可记录"是否已被归纳"，作为归纳层（变更 D）的增量基础。
2. **内容水位门控**（独立价值）：Reflection 触发前先判断"自上次成功反思以来是否有新的可反思内容"，无新内容时不发起 LLM 调用。

两个水位**必须区分**，混用会漏算：

| 水位 | 粒度 | 用途 | 载体 |
|---|---|---|---|
| 归纳水位 `consolidated_at` | **记忆条目** | 归纳层增量输入 | `MemoryEntry` 新增字段 |
| 消息水位 | **会话** | Reflection 触发门控 | 复用既有 `ReflectionRecord` 历史，不新增存储 |

### 6.2 领域模型变更

```rust
struct MemoryEntry {
    // ...（既有字段与变更 A/B 的字段不变）
    /// 被归纳的时间（None = 未归纳）。
    #[serde(default)]
    consolidated_at: Option<u64>,
}
```

### 6.3 不变量变更

| # | 新增 | 内容 | 守护点 |
|---|---|---|---|
| **M12** | 新增 | **水位只前进**：`consolidated_at` 一旦设置，仅在该条目被替换（同 id 重新写入新内容）时重置为 `None`，**NEVER** 因查询、注入或无关变更被清除 | 写入路径唯一重置点 |

### 6.4 端口变更

```rust
pub trait MemoryPort {
    // ...（既有方法不变）
    /// 列出尚未被归纳的条目（归纳层增量输入；无归纳层时供诊断使用）。
    fn list_unconsolidated(&self, layer: Option<MemoryLayer>) -> Vec<MemoryEntry>;
}
```

`NoOpMemory` 需实现该方法并返回空集合。

### 6.5 触发门控变更（独立价值）

现状：Reflection 由 `interval_runs` 计数触发，不判断内容是否变化。

变更：Runtime 的触发判定增加一个前置条件——**自最后一条成功完成的 Reflection 记录以来，会话是否存在新的可反思消息**。判定复用既有 `ReflectionHistoryQuery`，不新增存储。

两者关系（**互补而非替代**）：

```text
触发 = interval_runs 计数到期  AND  存在新的可反思内容
```

未通过门控时，跳过本次触发并记录诊断（**NEVER** 静默消费计数），使计数在下次有内容时仍能正常触发。

### 6.6 持久化与兼容

- `#[serde(default)]`，旧文件得到 `None`（视为"未归纳"，对首次归纳运行是安全默认）。
- 门控复用既有历史查询，无格式变更。

### 6.7 测试策略

| 层 | 覆盖目标 |
|---|---|
| L0 | 编译期：`consolidated_at` 参与构造的所有点 |
| L1 | 水位设置/重置规则（M12）；`list_unconsolidated` 过滤逻辑；serde 往返与旧格式默认值 |
| L2 | `apply_reflection` 后水位正确推进；注入与检索**不**改动水位（R1 不变量的回归） |
| L3 | `MemoryPort` 契约含新方法；`NoOpMemory` 返回空 |
| L4 | 场景：Reflection 触发门控——无新内容时跳过且不消耗计数；有新内容时正常触发 |
| L5 | 不适用 |

### 6.8 风险与回滚

| 风险 | 缓解 |
|---|---|
| 水位语义边界混用（"最后成功归纳时间" vs "归纳到哪个版本"） | 本设计明确取"最后成功归纳时间"语义；实现时 **MUST** 在类型命名与文档中保持一致，**NEVER** 用同一字段表达两种含义 |
| 门控误判导致反思饥饿 | 门控仅在"确无新内容"时跳过；计数不被消费，内容出现后仍可触发 |

**回滚**：门控可通过配置关闭（回退为纯计数触发）；`consolidated_at` 不被其他逻辑消费，停止写入即退化为原行为。

---

## 7. 变更 D：归纳层（方向记录，本设计不实施）

**前置**：变更 A（取代关系）+ B（证据指针）+ C（巩固水位）全部就位。

**方向要点**（不在本设计细化）：

1. 归纳产物是与 `MemoryCategory` **正交的类型维度**（分类表达用途，类型表达"原始 / 归纳"），**NEVER** 作为第 6 个 `Category`。
2. 归纳产物持有证据指针（多条来源事实）、去重后的证据强度，以及精炼时的演变叙事（文本表达，不引入数值置信度）。
3. 由后台归纳流程产生：LLM 裁决"新建 / 更新 / 删除"，配合相似度去重护栏。
4. 待决问题：归纳产物在读取时与原始事实**同路径**（统一检索排序）还是**独立路径**（分层下钻）——见 §13。

**不实施的理由**：收益依赖记忆规模效应；需要后台任务执行能力（当前 Runtime 只有单槽 Reflection）；会显著提升 Memory BC 的复杂度。**在前置变更未就位时引入会产生无法维护的中间态**（归纳产物既无证据指针、也无进度水位）。

---

## 8. 不变量变更汇总

| # | 状态 | 内容 | 来源变更 |
|---|---|---|---|
| M1–M3 | 不变 | id 唯一 / layer 不可变 / content 非空 | — |
| M4 | 语义澄清 | `confirmation_count` 单调递增（含义按 §5.3 明确，行为不变） | B |
| M5 | 不变 | `outdated` 不可逆且不可注入 | — |
| M6–M8 | 不变 | pinned 不被淘汰 / active 容量上限 / TTL 过期不注入 | — |
| **M9** | 新增 | 取代关系无环 | A |
| **M10** | 新增 | 被取代条目不可注入（pinned 不可绕过） | A |
| **M11** | 新增 | 证据指针指向真实条目；清理不得删除被引用条目 | B |
| **M12** | 新增 | 归纳水位只前进 | C |

---

## 9. 端口与型号变更汇总

| 对象 | 变更 | 来源 |
|---|---|---|
| `MemoryEntry` | 新增 `superseded_by: Option<MemoryId>`、`evidence: Vec<MemoryId>`、`consolidated_at: Option<u64>` | A / B / C |
| `MemorySuggestion` | 新增 `supersedes: Vec<MemoryId>` | A |
| `ReflectionApplyResult` | 新增 `superseded: usize` | A |
| `MemorySearchResult` hit | 新增取代状态字段 | A |
| `MemoryPort` | 新增 `list_unconsolidated` | C |
| `NoOpMemory` | 实现 `list_unconsolidated` | C |
| `write` | 合并路径行为变更（归档 + 指针） | B |
| `compact` | 归档清理排除被引用条目 | B |

**全部字段新增均使用 `#[serde(default)]`**，不产生破坏性格式变更。

---

## 10. 测试策略总览（L0–L5）

| 层 | 本变更的覆盖目标 |
|---|---|
| L0 编译期 | 新字段参与 `MemoryEntry` 构造的所有点；架构守卫无新增违规 |
| L1 单元测试 | eligibility 三个新条件的纯函数；环检测；水位推进/重置；指针校验；全部字段的 serde 往返（含旧格式缺字段） |
| L2 模块协作 | `write` 合并路径、`apply_reflection` 取代路径、`retrieve_for_inject` × `search` 的状态分工、Reflection 触发门控 |
| L3 契约测试 | `MemoryPort` 新方法契约；`NoOpMemory` 行为；持久化格式向前兼容 |
| L4 场景测试 | 取代后不再注入但可检索；合并后证据可查；无新内容时反思跳过且计数不消费 |
| L5 系统 smoke | 不适用（无进程级、平台级行为变更） |

**跨层纪律**：每个相邻边界都必须有证据，**NEVER** 只测源头（写入字段）与末尾（注入结果）而遗漏中间层（apply 建立关系、eligibility 过滤、持久化往返）。

---

## 11. 验收标准

变更 A / B / C 各自独立验收：

- [ ] A：被取代条目不出现在自动注入中，即使 `pinned=true`；仍可由显式 `search` 检索并携带取代状态；成环的关系被拒绝且不产生半写入
- [ ] B：合并发生后，新旧内容均可查（active 与 archive），证据指针可追溯；归档清理不删除被引用条目；历史已合并条目**不**被伪造指针
- [ ] C：`consolidated_at` 按 M12 推进与重置；Reflection 在无新内容时跳过且不消费计数；`list_unconsolidated` 正确
- [ ] 全部新增字段通过旧格式读取测试（缺失字段得到安全默认值）
- [ ] 实现完成后，`docs/design/02-modules/memory/` 的 01（领域模型）、02（检索与注入）、03（Reflection）、04（端口）**MUST** 同步更新，使目标态设计与实现一致

---

## 12. 明确不采纳清单

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

---

## 13. 开放问题

1. **合并时保留哪条为 active**：本设计采用"保留已存在条目"（活跃集合稳定）；若实践中发现新内容经常是旧内容的完整升级版，需重新评估"保留内容更完整者"方案及其对 CAS 抖动的影响。
2. **archive 容量策略**：证据指针依赖归档条目的长期保留，而现有淘汰策略只面向 active。archive 的容量上限、清理规则与被引用条目的处理需要独立设计。
3. **取代判定的归属**：由 Reflection（LLM 判定）还是写入时确定性规则判定？前者灵活但有误判成本，后者可控但覆盖有限。
4. **门控与计数的关系**：内容水位门控与 `interval_runs` 计数是互补（本设计）还是应当最终替代计数？
5. **归纳产物的读取路径**：与原始事实同路径（统一检索）还是独立路径（分层下钻）？
6. **reader 兼容前置验证**：持久化 reader 是否配置 `deny_unknown_fields`，决定新字段是否会影响旧版本读取。
7. **总结所需的证据出处**：本设计的每条结论指向 [050-hindsight-memory-research.md](050-hindsight-memory-research.md)；该文标注的"未验证项"在实现前**MUST** 补验，**NEVER** 基于未验证结论做实现决策。

---

## 14. 相关文档

- 证据基础：[050-hindsight-memory-research.md](050-hindsight-memory-research.md)
- 目标态领域模型：[01-domain-model.md](../../design/02-modules/memory/01-domain-model.md)
- 目标态检索与注入：[02-retrieval-and-injection.md](../../design/02-modules/memory/02-retrieval-and-injection.md)
- 目标态 Reflection 引擎：[03-reflection.md](../../design/02-modules/memory/03-reflection.md)
- 目标态端口与适配器：[04-ports-and-adapters.md](../../design/02-modules/memory/04-ports-and-adapters.md)
- 测试分层规范：[04-testing-and-coverage.md](../../design/03-engineering/04-testing-and-coverage.md)

## 修订记录

| 日期 | 变更 |
|---|---|
| 2026-09-28 | 初稿：变更 A/B/C 的领域模型、不变量、端口与行为设计、测试策略与验收标准；变更 D 方向记录；不变量与端口变更汇总；不采纳清单与开放问题 |
