# Context Management · Memory 注入

> 层级：02-modules / context-management（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#786（S2）/ [#972](https://github.com/rushsinging/aemeath/issues/972)
> 本文定义 Context Management 对 Memory-owned `MemoryPort` OHS 的 integration：已排序条目的 render / placement、token budget、跨轮去重与 Reflection 时序。检索、评分、排序、mutation 与 persistence error 的唯一真相在 Memory BC。

## 1. 定位

Memory 注入是 ContextPort `build_window` 的内部步骤之一：

```
build_window
  ├─ L2-L4 compact 读模型投影（L1 已在 ToolResult 入链前完成）
  ├─ await prompt 组装（PromptPipeline）
  ├─ memory 注入（MemoryPort）  ← 本文
  ├─ active summary
  └─ → ContextWindow.system_blocks + messages
```

物化的可观察顺序以 [Compact](02-compact.md) §2.1 为唯一真相：Prompt（含 Skill）→ Memory → active summary → final assembly。Memory 的最终 placement 固定在 cacheable prefix 的 user guidance 之后、active summary 之前；它不因物化发生在 Prompt 后而移动到 uncached suffix。

- **MemoryPort 是独立端口**：被 ContextPort（注入）、Tool BC（Memory tool）、Reflection（写入）三方消费
- **Memory BC 属支撑域**：Memory 独占检索与排序；Context Management 独占 Context Window 中的 render / placement / budget / dedup
- **Memory capability 由 `RunSpec.memory` 装配**：Enabled 使用 shared lease 捕获的 active Arc；Disabled 使用 owner-owned `NoOpMemory`，统一 Loop 不按 Main / Sub 角色分支

## 2. MemoryPort 消费边界

`MemoryPort` 的完整方法、`MemoryQuery` / `WriteResult`、NoOp 行为与 `ProjectMemoryOpener` **MUST** 只在 [Memory 端口与适配器](../memory/04-ports-and-adapters.md) 定义；本文 **NEVER** 复制第二份 trait。Context Management 只消费当前 Run shared lease / `RunSpec` 装配绑定的 `Arc<dyn MemoryPort>`，调用 `retrieve_for_inject(&MemoryQuery)` 并把结果渲染为 SystemBlock。

- `retrieve_for_inject` **MUST** 只读且不 touch access count；
- Context Management **NEVER** 调用 Memory 写方法，也 **NEVER** 构造 / 打开 store；
- Runtime Reflection 与 MemoryTool 若写入，**MUST** 使用同一 Run 绑定的 Arc；
- `MemoryMode::Disabled` 使用 Memory 文档定义的 NoOp；显式 Enabled 的派生 Run clone 父 Run Arc，**NEVER** 重新 open。

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
  │   └─ 返回 MemorySearchResult { mode: InjectionPriority, hits, .. }（不 touch，不写盘）
  │
  ├─ 渲染为 SystemBlock
  │   ┌─────────────────────────────────────┐
  │   │ <memory-context>                    │
  │   │ - [Category] content      (★ if pinned) │
  │   │ - [Category] content                │
  │   │ </memory-context>                   │
  │   └─────────────────────────────────────┘
  │
  └─ 交给 Context assembler 放到 user_guidance 之后、active_summary 之前
      （属于 cacheable_prefix，通过 entry fingerprint 检测变化）
```

### 3.2 注入时机

- **Session 首次 `build_window`**：检索并注入，随后**冻结复用**（#1777）
- **compact 成功提交后**：下一次 `build_window` 重新检索并替换冻结内容；`Skipped` 不刷新
- **其他轮次**：直接复用冻结内容，不发起检索——memory 属于 cacheable prefix，每轮重取会让内容漂移、cache 命中不可预测（见 [04-prompt-guidance.md](04-prompt-guidance.md) §3.2）
- 冻结状态绑定 `session_id`：resume 切换 Session 视为该 Session 的首次注入；`clear_session` 释放

### 3.3 注入条件

```rust
// #1777：预算按窗口比例（None 默认 2%），条数上限已移除
let token_budget = match config.memory.inject_token_budget {
    Some(0) => return Ok(empty),
    Some(fixed) => fixed,
    None => token_budget::injection_token_budget(request.context_size),
};
if config.memory.enabled && token_budget > 0 {
    let query = MemoryQuery { limit: CANDIDATE_LIMIT, ..Default::default() };
    if let Some(block) = build_memory_block(port.retrieve_for_inject(&query), token_budget) {
        window.system_blocks.push(block);
    }
}
```

## 4. 已排序结果的消费规则

评分公式、BM25 / fallback 选择、filtering、ranking 与 `similarity_threshold` 的唯一真相见 [Memory 检索与注入](../memory/02-retrieval-and-injection.md)。Context Management **MUST** 验证 `MemorySearchResult.mode == InjectionPriority`，保持 `hits` 返回顺序，并且只消费 `hits[*].entry`；mode 不匹配返回 typed integration error，**NEVER** 复制 `injection_score` 或按 recency / pinned 二次排序。

- Context 取一个足够大的候选窗口（条数上限已移除，#1777），由 token 预算做最终截断。
- 返回后按**两段预算填充**（#1777 覆盖式让位）：先填 `kind = Synthesized` 的结论，再用剩余预算填普通条目并跳过被已选结论 `evidence` 覆盖的来源；段内超预算即停，**NEVER** 跳过高位条目后保留低位条目。
- 结论若在第一段就未入选，其来源在第二段照常参与——固定降权系数会误伤来源，让位必须是条件性的。
- render 只使用 `hits[*].entry` 的稳定 `MemoryEntry` Published Language 字段；result/hit 的 retrieval mode、relevance、location、outdated、TTL、internal score / index metadata **NEVER** 进入 prompt。
- 注入条数只由 token 预算决定（#1777 已移除 `inject_count`），Config 可用 `inject_token_budget` 覆盖为固定值。

## 5. Active Memory 生命周期

生命周期与物理格式的唯一真相见 [Memory 端口与适配器](../memory/04-ports-and-adapters.md)。Context Management 只遵守以下消费约束：

- Memory key **MUST** 使用完整、版本化 `ProjectIdentity`，**NEVER** 只取 cwd / basename；
- `ProjectMemoryOpener::open_for_project(identity, memory_config).await` **MUST** 在 resume prepare 中完成 dataset recovery、eager-read，并验证 active / archive、schema 与权限，无副作用地返回 candidate Arc；
- 每个 Run 在 shared session lease 下取得 active Arc；Context / Runtime / MemoryTool / Reflection 共享它；
- Memory 查询与 mutation **MUST** 受同一 lease 保护，后台 Reflection 在 exclusive resume 前 join / cancel；
- MemoryService 的 mutation **MUST** 由单一 async mutation permit 串行化：candidate state → Storage dataset CAS commit → 无失败 publish；query 只读已验证 in-memory state。同一 Composition / 进程的 active Main slot **NEVER** 为同一 identity 重复 open，而独立进程间的同 identity writer **MUST** 由 `DatasetRevision` CAS 检测冲突；任一实例都 **NEVER** 在未提交失败后暴露 candidate。

## 6. 只读注入约束

Target API 只使用 `retrieve_for_inject`。任何会 touch access count、改变 recency 或触发写盘的 `top_for_inject` 变体 **NEVER** 进入注入路径：相同 Memory state + query **MUST** 产生相同结果且不修改持久化状态。

## 7. Memory Tool（LLM 调用）

Memory tool 是 LLM 主动调用的 tool，与自动注入是**互补关系**：

| 维度 | 自动注入（MemoryPort） | Memory Tool（LLM 调用） |
|---|---|---|
| 触发 | Session 首次 + compact 后（其余轮次复用冻结内容） | LLM 决定调用 |
| 检索 | `retrieve_for_inject`（Memory-owned ranking） | `search`（BM25 primary / 显式 fallback） |
| 条数 | token 预算（窗口 2% 默认，可配置覆盖） | LLM 指定 limit |
| 写入 | 不写入（只读） | 可写入（`Memory.tool` write 操作） |
| 端口 | MemoryPort | MemoryPort（同一 trait） |

Memory tool 的 handler 也通过同一个 `MemoryPort` Arc 操作，不直接构造 store 或 service。

## 8. Budget-aware inject limit

Context 可在调用 MemoryPort 前按本轮剩余 token budget 收缩 Config 上限：

```rust
fn memory_query_limit(configured: usize, budget: TokenBudget) -> usize {
    configured.min(budget.memory_entry_capacity())
}
```

动态限额只表达“最多取多少条”，**NEVER** 在 Context 中读取 relevance score 或实现检索算法。Future embedding / semantic retrieval 的决策、模型与 index 仍完全归 Memory BC。

## 9. Reflection 集成

### 9.1 PreCompact Reflection

auto-compact 前触发——抢救关键信息到 Memory：

```
auto_compact
  ├─ run_precompact_reflection(messages)
  │   ├─ LLM 分析将 compact 的消息，提取值得记忆的信息
  │   ├─ 产出 MemorySuggestion
  │   └─ MemoryPort.write(entry).await  ← 写入 memory
  │
  ├─ compact_messages_with_llm(...)
  └─ apply_compact_outcome(...)
```

### 9.2 周期性 Reflection

```rust
struct ReflectionConfig {
    enabled: bool,                 // 默认 true
    interval_turns: usize,         // 默认 10
    auto_apply: bool,              // 默认 false（周期性 Reflection 需用户确认）
}
```

> **PreCompact 例外**：PreCompact Reflection **MUST** 同步完成并自动写入（`apply_reflection`），**NEVER** 受 `auto_apply = false` 控制——因为 compact 会改变对话历史，未写入的新记忆将永久丢失。

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

## 10. 相关文档

- Compact 家族：[02-compact.md](02-compact.md)
- Token Budget：[03-token-budget.md](03-token-budget.md)
- Prompt & Guidance：[04-prompt-guidance.md](04-prompt-guidance.md)
- Runtime 消费的 Memory OHS 与装配：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Project identity / WorkspaceRead：[../project/02-ports-and-adapters.md](../project/02-ports-and-adapters.md)
- 上下文地图（Memory BC = 支撑域）：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- Current → Target 迁移责任：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-18 | #897 提供只读 MemoryRetrieveAdapter：验证 InjectionPriority、保持 hit 顺序、仅渲染 entry 允许字段并处理 Disabled；生产自动注入切线仍由 #984 完成 | #897 |
| 2026-07-12 | 初稿：MemoryPort trait、注入管线、评分算法、top_for_inject 退役、semantic retrieval 演进、Reflection 集成 | #786 |
| 2026-07-14 | 将 Memory project root 统一为 Project-owned ProjectIdentity，避免恢复后继续使用旧 cwd | [#972](https://github.com/rushsinging/aemeath/issues/972) |
