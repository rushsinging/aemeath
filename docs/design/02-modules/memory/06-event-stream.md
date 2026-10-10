# Memory · 生产事件流与 append-only 历史

> 层级：02-modules / memory（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.2.0
> 本文定义 Memory-owned **append-only 事件流**（含内容的决策现场）以及 `reflection-history` 对齐为真 append-only 的落盘与语义。运行诊断日志（`aemeath:agent:memory`）保持无内容原则，不在本文范围内改写。

## 1. 定位与问题

Memory 的 durable 真相源是 `AtomicDataset`：每层只保留当代 + 一代 `previous`。事后无法从存储回溯「某次 write/delete/compact/reflection 当时看到了什么、改成了什么」。

因此在**决策发生时**必须另开一条 memory-owned 事件流：

- **运行日志**：诊断定位，**MUST NOT** 携带记忆正文。
- **事件流（本文）**：append-only、含内容、按时间窗滚动，供后期 LLM/人直接读文件复盘。

`reflection-history` 现状是单一 member 整 `Vec` CAS 改写，与「真 append-only」不一致；本期一并改为与事件流同形态的 segment 追加，读侧折叠最新。

## 2. 已拍板决策

| 决策点 | 选择 |
|---|---|
| 查询 / 报告面 | **本期不做**（无 xtask/scripts 报告、无 Memory 查询 Port、无 LLM 工具）；只保证事件落盘 + 测试 helper 读回，供 LLM 事后分析文件 |
| 落点装配 | `MemoryService`（及 opener / ReflectionWorkflow 必要装配边界）**中心化 emit** |
| 读路径粒度 | **全量**：候选集含正文 |
| 内容敏感 | **明文**落本地 agents 根（与现有 memory / history 同信任边界） |
| 与 reflection-history | 事件流旁路记录反思组；history **改为真 append-only**，两套落盘形态一致 |
| 交付 | **两 PR**（见 §9） |
| 事件落盘形态 | 真 append-only 按日 segment（jsonl） |
| 保留 | 默认 **30 天**时间窗滚动，可配置覆盖 |

## 3. 非目标（本期）

- 生产查询 Port / timeline API / slash 报告面。
- 改写运行日志 schema 或为其建立消费链。
- 默认 redact、采样策略、跨机同步。
- 让 Storage 发布通用 append-log OHS（Storage 铁律：只提供 `AtomicBlob` / `AtomicDataset` 整值替换；append 由数据 BC adapter 自管，复用 `SafeStorageRoot`——先例：Audit `FileUsageAppendStore`）。

## 4. 落盘形态

### 4.1 事件流

- **格式**：按 UTC 日切分的 jsonl；每行一条完整 `MemoryEvent` JSON，行尾 `\n`；**NEVER** 改写已写入行。
- **路径**（相对 agents storage root，segment 均经 `SafePathSegment`）：

```text
memory/{project_key}/events/{yyyy-mm-dd}.jsonl
```

  `project_key` 与现有 Memory dataset / reflection-history 使用同一 `ProjectMemoryKey` 派生段。全局层操作仍挂在打开该 Memory 实例的 project key 下（事件内 `layer` 字段区分 global/project）。
- **写入**：Memory-owned `JsonlSegmentEventStore`（或等价名）经 `SafeStorageRoot::ensure_dir` + `create_or_open(append)` 追加；进程内按文件名互斥，保证同行完整。
- **保留**：默认保留最近 **30** 个日历日 segment；配置项（建议）`memory.event_retention_days`（`0` 表示禁用 GC，**NEVER** 表示关闭事件写入）。GC 在 open 或低频周期触发，删除过期文件名；GC 失败只记运行日志，不阻断。
- **失败语义**：`append` 失败 **MUST** fail-open——主流程（retrieve/write/apply/…）返回值不变；仅 `log::warn!`（无正文）到 `aemeath:agent:memory`。

### 4.2 reflection-history（改造）

- **实现**：`JsonlReflectionHistoryStore`（经 `wire_reflection_history_store`），路径：

```text
memory/{project_key}/reflection-history/{yyyy-mm-dd}.jsonl
```

  每行一条 `ReflectionRecord`；只 append。旧 `AtomicDataset` member `records`（整 `Vec` CAS）仅作一次性导出源。
- **`append`**：追加一行。
- **`upsert`**：取消原地替换；同 id 再写入 = **再 append 一条**（后写覆盖语义由读侧折叠实现）。
- **`list` / `list_with_content`**：扫描保留窗内 segment（新→旧），按 `id` **折叠为最新一条**后截断 `limit`，以保持 `/reflect` 历史「每 id 一条有效视图」；完整谱系只存在于文件，不经该 Port 暴露（本期无查询 Port）。
- **迁移**：首读若尚无 jsonl segment 且发现旧 AtomicDataset member `records`，一次性按 `record.timestamp` 分日导出；导出失败 fail-open（warn，不阻断打开）。
- **保留**：与事件流共用 `memory.event_retention_days`（默认 30；`0` 禁用 GC）；wire 时 fail-open 触发一次 GC。

## 5. 事件模型

### 5.1 Envelope

领域类型（示意；实现落在 `domain/event.rs` 一类模块）：

```rust
struct MemoryEvent {
    schema_version: u32,          // 初始 1
    event_id: String,             // typed id 或 UUIDv7
    ts_unix_ms: u64,
    op: MemoryEventOp,            // 对齐 §6 映射表
    outcome: EventOutcome,        // Succeeded | Failed { kind } | Skipped
    correlation_id: String,       // 同因果链（含 CAS 重试）共用
    // 谁/何时
    session_id: Option<String>,
    run_ordinal: Option<u32>,
    step_ordinal: Option<u32>,
    tool_call_id: Option<String>,
    actor: EventActor,            // Service | ReflectionWorkflow | Opener | RetentionGc
    // 坐标
    layer: Option<MemoryLayer>,
    corpus: Option<String>,
    commit_revision: Option<String>,
    // 变更 + 上下文（含内容）
    change: EventChange,          // before/after / candidates / affected
    context: EventContext,        // query、触发摘要、覆盖区间等
    config_fingerprint: ConfigFingerprint,
}
```

`ConfigFingerprint` **MUST** 至少包含：scoring 开关、reflection model 标识/revision（若有）、与本次决策相关的阈值（如 similarity、inject budget、event_retention_days）。缺省用显式 `None` / 默认哨兵，**NEVER** 静默省略关键开关。

### 5.2 决策现场四要素（强制）

1. **谁/何时**：时间戳 + 可得的 session/run/step/tool_call；装配点拿不到的字段保持 `None`，不阻断 emit。
2. **变更**：
   - 写/生命周期：`before` / `after` 为受影响 `MemoryEntry` 全文快照（或列表）。
   - 删除 / 归档 / compact / supersede / synthesis：**复制**被影响条目全文进事件，**NEVER** 仅存 id 引用（`/clear` 会清会话 `run_slices`）。
   - 读：候选集 **含正文**（Q3=A）；同时带命中数、limit、layer 过滤等指标。
3. **上下文**：触发输入短摘要、检索 query 文本、反思覆盖区间等。
4. **坐标**：layer、corpus、commit revision、`correlation_id`。

### 5.3 与运行日志分流

| | 运行日志 | 事件流 |
|---|---|---|
| 内容 | 无记忆正文 | 明文全文 / 候选正文 |
| 消费者 | 运维诊断 | LLM/人复盘文件 |
| 失败 | 不影响业务 | 不影响业务 |
| 保留 | 日志轮转策略 | 30 天 segment GC |

## 6. 监控操作面（19 项）与落点

全部经中心化 `emit`（`MemoryEventAppendPort`）。映射表 **MUST** 以测试常量固化，禁止口头约定。

| 组 | Op | 主要 emit 点 |
|---|---|---|
| 读 | `RetrieveForInject` | `MemoryService::retrieve_for_inject` 返回前 |
| 读 | `Search` | `MemoryService::search` 返回前 |
| 读 | `PerMessageRecall` | `application::recall::recall_relevant`（或其所在 Memory 边界）返回前；候选含正文 + probability |
| 读 | `ListStats` | `list` / `stats`：可合并为一次事件（op 区分或 payload 带 kind），仍计读组覆盖 |
| 写 | `WriteAdd` | `write` commit 成功后；失败记 `outcome=Failed` |
| 写 | `Update` | `update` |
| 写 | `Delete` | `delete`（before=被删全文） |
| 写 | `Pin` | `pin` |
| 写 | `MarkOutdated` | `mark_outdated` |
| 写 | `ArchiveRestore` | `archive` / `restore`（可同一 op + action 字段） |
| 写 | `Compact` | `compact`（受影响条目全文列表） |
| 写 | `SupersedeSynthesis` | write/apply 路径上的 supersede/synthesis 分支 |
| 反思 | `ReflectionTriggered` | ReflectionWorkflow 开始（覆盖区间 + 触发原因） |
| 反思 | `ReflectionApplied` | apply + history append 边界（建议/过期/落地结果） |
| 反思 | `ReflectionCost` | token / duration / 终态分类 |
| 生命周期 | `OpenLoad` | opener 打开两层 / legacy 迁移 |
| 生命周期 | `CommitCas` | 单层 commit（含 CAS 冲突重试，同 `correlation_id`） |
| 生命周期 | `AssemblyFingerprint` | 装配完成时记录 scoring/model/阈值指纹 |
| 生命周期 | `EvictionWatermark` | 淘汰/满容候选决策（候选全文或完整 EvictionCandidate 快照） |

**禁止**：业务调用方（Tools/Runtime/TUI）直接写事件文件。

## 7. 端口与分层

```text
domain/     MemoryEvent、MemoryEventOp、ConfigFingerprint、retention 纯函数
ports/      MemoryEventAppendPort { append(&MemoryEvent) -> Result<(), EventAppendError> }
            测试-only 或 cfg(test) 读回 helper 可挂同一 adapter，不进生产 api 发布面
            ReflectionHistoryStore：语义改为 append-only；upsert = append；list 折叠
adapters/   JsonlSegmentEventStore（SafeStorageRoot）
            JsonlReflectionHistoryStore（替换/退役 AtomicDataset 整 Vec 路径）
service/    中心化 build+emit；fail-open
application/ ReflectionWorkflow 边界 emit 反思组
api/        本期不新增对外查询 façade；wire 工厂注入 EventAppendPort
composition/ 打开 Memory 时注入 root + retention 配置
```

`NoOpMemory`：**MUST NOT** 要求事件流；无 store 时 emit 为空实现。

## 8. 配置

- `memory.event_retention_days`：默认 `30`；仅影响 GC。
- 事件写入本身随 Memory 启用而启用；Memory disabled / `NoOpMemory` 时无事件。
- 配置进 `ConfigFingerprint`，便于复盘时对照「当时开关」。

具体字段名以 `specs/3.9-config-compat.md` 实施时登记为准。

## 9. 交付拆分

### PR1 — schema + 写路径 + 保留 + history 改造

- 事件 schema 单测 + jsonl 落盘/读回契约（测试 helper）。
- `MemoryEventAppendPort` + 中心化 emit 骨架 + fail-open。
- 写组 8 项落点；生命周期中与 `OpenLoad` / `CommitCas` 强相关项。
- 30 天 retention/GC。
- reflection-history 真 append-only + legacy Vec 迁移 + list 折叠。

### PR2 — 读 / 反思 / 剩余生命周期

- 读组 4 项（候选全文）。
- 反思组 3 项（含 cost）。
- `AssemblyFingerprint` / `EvictionWatermark` 等剩余项。
- 19 项映射表契约测试闭环。
- **不含** 报告工具与生产查询 Port。

## 10. 验收

| 项 | 标准 |
|---|---|
| Schema | 单测覆盖序列化 / 缺省 / 版本字段 |
| 落盘读回 | adapter 契约：append → 文件行 → 反序列化相等 |
| 时间线 | 测试 helper 按 memory id 过滤事件可得完整变更链（含 before/after 正文） |
| 19 落点 | 映射表测试：每个 Op 至少一处生产路径 emit（两 PR 合入后齐） |
| 保留 | 过期 segment 被 GC；未过期保留 |
| 降级 | 强制 append 失败时 write/retrieve/apply 仍成功 |
| history | append-only 文件增长；upsert 同 id 两行；list 只见最新 |
| 报告 / 查询工具 | **明确不做**（follow-up） |

## 11. 风险

| 风险 | 缓解 |
|---|---|
| 读全量正文体积大 | 30 天 GC；后续可加采样（另决策） |
| history upsert 心智变化 | 读折叠；文件保留谱系 |
| SafeStorageRoot 装配遗漏 | composition 单测 / opener 契约 |
| 中心化 emit 漏点 | 19 项映射表强制测试 |
| domain-no-direct-io 守卫 | I/O 只在 adapters；domain/application 只依赖 Port |

## 12. 相关文档

- [README](README.md) — 模块定位
- [03-reflection.md](03-reflection.md) — Reflection 与 history 语义（实施时同步「append-only + 读折叠」）
- [04-ports-and-adapters.md](04-ports-and-adapters.md) — Port 装配（实施时补 EventAppendPort）
- Storage：`SafeStorageRoot` /「NEVER 发布 append-log OHS」—— [../storage/README.md](../storage/README.md)
- Audit append 先例：`agent/features/audit/src/adapters/append.rs`

## 修改历史

| 日期 | 变更 |
|---|---|
| 2026-10-10 | 初稿：事件流目标态、reflection-history 真 append-only、19 落点、两 PR、无报告面 |
