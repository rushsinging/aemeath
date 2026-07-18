# Context Management · 持久化增量摘要树

> 层级：02-modules / context-management（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：[#1162](https://github.com/rushsinging/aemeath/issues/1162)
> 本文是 L5 Auto-compact 的持久化摘要、后台调度、projection、恢复与 usage 记账唯一真相。L1–L5 的职责边界、触发阈值和 Compact 提交入口仍以 [02-compact.md](02-compact.md) 为准。

## 1. 目标与非目标

### 1.1 目标

1. **保真优先**：用户指令、后续修正、关键决策、文件 / 符号、当前状态与下一动作必须可追溯到连续的 source RunStep 范围。
2. **低阻塞延迟**：平时后台增量建树；进入 L5 时优先本地激活已经准备好的 warm projection。
3. **跨退出复用**：每个完成 shard 立即持久化；正常退出、取消或崩溃后不重复已完成调用。
4. **原始历史可恢复**：Session 原始历史始终是唯一真相源，摘要树只是一份可重建的派生索引。
5. **完整记账**：leaf、backfill map、branch reduce 与 precompact reflection 的每次调用都记录标准化 usage，并能与 Compact Job 和 Session 总账对账。

### 1.2 非目标

- Retrieval-first Context Projection、语义索引和 Context Collapse 仍是 [#547](https://github.com/rushsinging/aemeath/issues/547) Future。
- 摘要树 **NEVER** 替代 Session 原始历史，也 **NEVER** 赋予 Runtime 直接访问 ChatChain、manifest 或 shard 的能力。
- 本设计 **NEVER** 在单一 session 内并发多个摘要请求。低延迟来自提前增量处理，不来自临界点 fan-out。
- 本设计不创建第二套 Provider token 标准化或定价算法。

## 2. 核心术语

| 术语 | 含义 |
|---|---|
| `CoverageRange` | 同一 Session 内连续、闭开区间的 finalized RunStep 范围 |
| `Leaf` | 直接由一段原始 RunStep 生成的结构化摘要 |
| `Branch` | 合并最多 4 个相邻、同层子 shard 的结构化摘要 |
| `SummaryDocument` | 带固定字段和 source evidence 的摘要文档 |
| `CoverageFrontier` | active generation 已连续完成的最大历史前缀 |
| `CompactProjection` | 覆盖连续历史前缀的一组不重叠节点，加 recent raw 与 uncovered raw |
| `CompactJob` | 一次 leaf、backfill、branch reduce 或前台缺口补齐工作 |
| `Warm activation` | 不调用 Provider，只验证并发布已持久化 projection |
| `Generation` | 同一 normalization / schema / prompt version 下的一棵摘要树 |

## 3. 领域模型与不变量

### 3.1 Published Language

```rust
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CoverageRange {
    pub first: RunStepRef,
    pub end_exclusive: RunStepRef,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceEvidence {
    pub range: CoverageRange,
    pub content_fingerprint: ContentFingerprint,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummarySection {
    pub text: String,
    pub sources: Vec<SourceEvidence>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummaryDocument {
    pub user_requests: Vec<SummarySection>,
    pub directives: Vec<SummarySection>,
    pub decisions: Vec<SummarySection>,
    pub completed_work: Vec<SummarySection>,
    pub problems: Vec<SummarySection>,
    pub files_and_symbols: Vec<SummarySection>,
    pub current_state: Vec<SummarySection>,
    pub next_action: Option<SummarySection>,
    pub continuation: ContinuationStatus,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SummaryShardKind {
    Leaf { source_steps: Vec<RunStepRef> },
    Branch { children: Vec<SummaryShardId> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummaryShard {
    pub id: SummaryShardId,
    pub generation: SummaryGeneration,
    pub coverage: CoverageRange,
    pub source_fingerprint: ContentFingerprint,
    pub kind: SummaryShardKind,
    pub document: SummaryDocument,
    pub invocation_id: ModelInvocationId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactProjection {
    pub generation: SummaryGeneration,
    pub nodes: Vec<SummaryShardId>,
    pub summarized: CoverageRange,
    pub recent_raw: Vec<RunId>,
    pub source_revision: SessionRevision,
}
```

`RunStepRef` 是 `(RunId, RunStepId)` 的稳定 Published Language 值对象。`SummaryDocument` 的每个语义字段都携带 source evidence；合并 Branch 时 **MUST** 合并来源，**NEVER** 只保留 prose 而丢失覆盖依据。

### 3.2 Coverage 不变量

1. shard 只覆盖 finalized RunStep，**NEVER** 覆盖正在执行的 Run。
2. 每个 shard 的 coverage 连续、非空；Branch children 连续、无重叠、同 generation、同层且数量为 `2..=4`。
3. projection nodes 按历史顺序排列，连续、无遗漏、无重叠。
4. projection 的 summarized prefix、recent raw 与 uncovered raw 联合起来必须完整覆盖本轮候选历史；**NEVER** 为满足 token budget 静默制造 head gap。
5. 最近 3 个完整 Run 是保护区，**NEVER** 进入新 Leaf。保护区向前移动后，离开保护区的 finalized RunStep 才可成为 leaf candidate。
6. active user directive 的后续修正覆盖旧冲突要求，但旧来源仍保留用于审计。
7. 同一 source fingerprint、normalization version、summary schema version 和 prompt version 产生同一个 shard identity。
8. Provider/model 是 invocation 元数据，不进入 shard identity；普通模型切换 **NEVER** 使已有 shard 失效。

## 4. 规范化输入与分块

摘要预算必须依据真正发送给 SummaryGenerator 的规范化输入计算，**NEVER** 再按未发送的完整 ToolResult 估算。

规范化规则：

- 保留全部 user 文本。
- 保留 assistant 的结论、文件 / 符号、状态变化和 ToolUse 语义。
- 跳过 Thinking / reasoning 内容。
- ToolUse 与对应 ToolResult 必须留在同一完整 RunStep。
- ToolResult 投影保留工具名、路径、成功 / 失败状态、关键结果和明确截断标记。
- 新 ToolResult 在进入 Session 前仍先受 L1 budget reduction；摘要规范化不是绕过 L1 的第二条写入路径。

分块规则：

```rust
const LEAF_TARGET_TOKENS: usize = 16_000;
const LEAF_SOFT_LIMIT_TOKENS: usize = 24_000;
const RECENT_RAW_RUNS: usize = 3;
const BRANCH_MAX_CHILDREN: usize = 4;
```

- 以完整 RunStep 为最小单位累加到 16K 目标。
- 加入下一 Step 后超过 24K 时，在当前 Step 前结束 Leaf。
- 单 Step 自身超过 24K 时独占 Leaf；若 Provider 返回 ContextTooLong，再对该 Step 使用更强的确定性 ToolResult 投影，**NEVER** 拆开 ToolUse / ToolResult。
- 空输入不创建 job；不足 16K 的增量保留到后续 append，除非前台进入 Must 并需要补齐缺口。

## 5. 多分辨率摘要树与 Projection

```text
Raw:     Step 1 ───────────────────────────────── Step N
          ├── Leaf A ─┐
          ├── Leaf B ─┼── Branch AB
          ├── Leaf C ─┘
          └──────────────────────── recent 3 Runs (raw)

Projection:
  大 summary budget → Leaf A + Leaf B + Leaf C + recent raw
  小 summary budget → Branch AB + Leaf C + recent raw
```

Root 不是必须反复重写的唯一 summary，而是 Context builder 在当前 summary budget 下选择的连续节点集合：

1. 从 active generation 的稳定 coverage prefix 开始。
2. 在预算允许时优先选择更细粒度节点；不足时选择覆盖相同范围的 Branch。
3. 拼接 recent 3 Runs 原文。
4. frontier 与保护区之间仍未摘要的范围只要能容纳就保留原文。
5. 缺口无法容纳且 urgency 为 Must 时，前台最多等待该 session 一个 Provider round trip。

Warm activation 只执行：加载 manifest → 校验 generation / revision / fingerprint → 选择节点 → CAS 发布 active projection。它 **MUST** 是零 Provider 调用。

## 6. 第一次与后续 Compact 生命周期

```text
第一次 compact:
  P1 = [summary Step 1..80] + [raw Step 81..100]

继续对话:
  append Step 101..160
  后台只为离开保护区的 Step 81..120 生成新 Leaf

第二次 compact:
  P2 = [P1 稳定前缀] + [new Leaf 81..120] + [raw Step 121..160]
```

- P1 的 shard 不可变且可复用，第二次 compact **NEVER** 重新调用 Provider 处理稳定前缀。
- 新消息始终先进入 Session 原始历史，再由 `append_and_persist` 的后置调度发现新 candidate。
- 多个 Leaf 只有在同层连续节点达到合并条件、或 projection budget 需要时才低优先级生成 Branch；**NEVER** 每次 compact 都 reduce 全部旧摘要。
- 原始历史、旧 projection 和新 projection 可并存；active 指针只在候选 projection 完整验证后原子切换。

## 7. Scheduler 与前台优先

### 7.1 唯一并发约束

```rust
pub const COMPACT_PER_SESSION_LIMIT: usize = 1;
pub const COMPACT_GLOBAL_SESSION_LIMIT: usize = 5;
```

- Composition Root 创建唯一进程级 `CompactScheduler`。
- 同一 session 最多一个 in-flight SummaryGenerator 请求。
- 全局最多 5 个不同 session 同时执行请求。
- 全局调度按 session 公平轮转，**NEVER** 让单个长 session 占据多个 permit。
- 队列不持久化；恢复时从 Session history 与 manifest frontier 的差值重建。

### 7.2 优先级

1. `MustGap`：Provider 硬限制前无法容纳的前台缺口。
2. `ShouldBackfill`：compaction urgency 已到 Should。
3. `BackgroundLeaf`：普通增量 Leaf。
4. `BackgroundBranch`：Branch 合并与新 generation 重建。

新 Run 开始后，该 session 不再派发新的后台 job；已经 in-flight 的 job允许完成并 checkpoint，以免主动取消已付费请求。Provider / 全局限流层必须优先前台模型调用，后台 scheduler 只使用剩余容量。

## 8. 失败、取消、重试与 Circuit Breaker

| 分类 | 行为 |
|---|---|
| 网络、限流、超时 | 指数退避，最多 3 个 attempts |
| ContextTooLong | 不重发相同输入；按完整 RunStep 边界缩小或增强确定性投影 |
| 认证、权限、无效请求 | 立即停止该 session 后台 compact |
| fingerprint / revision 冲突 | 丢弃候选发布，重新扫描 frontier；已完成、匹配 source 的 shard仍可收养 |
| 外部取消 / 退出 | 停止派发；未完整返回的请求不提交半成品 |

- 连续失败 3 次打开 session 级 circuit breaker。
- 已返回 usage 的失败 attempt 仍写入账本；Provider 未返回 usage 时记录 `Unknown`，**NEVER** 填零冒充实际值。
- 单 session 并发 1 意味着崩溃后最多重做 1 个未 checkpoint 的 in-flight 请求；任何 completed shard 恢复后零重复调用。
- 前台取消等待时不发布部分 projection；已经完成的独立 shard仍可 checkpoint。

## 9. 持久化协议

### 9.1 Sidecar 布局

```text
~/.agents/sessions/{session_id}.compact/
├── manifest/                              # 独立 AtomicDataset
│   └── manifest.json
├── checkpoints/
│   └── {summary_shard_id}/                # 每个 shard 一个 AtomicDataset
│       ├── shard.json
│       └── success-attempt.json
└── usage/
    └── {model_invocation_id}/
        └── {attempt}/                      # 每个无 shard / GC 后 attempt 一个 AtomicDataset
            └── attempt.json
```

这是 Context adapter 的逻辑布局；Storage adapter 的 generation / journal /
previous 物理文件仍由 `AtomicDatasetPort` 私有管理。

`manifest.json` 只保存 generation、coverage frontier、active / previous
projection、shard 引用、circuit breaker 和 schema version。每个 checkpoint
dataset 把 shard result 与成功 attempt record 作为两个成员原子提交；manifest
revision 是唯一可变发布指针。每个已返回的失败 / 取消 attempt 必须在 retry 或
返回调用方前，以 `(ModelInvocationId, attempt)` 为 dataset key 独立提交到
`usage/`，**NEVER** 等整个 job 结束后批量补记，也 **NEVER** 伪造空 shard。

### 9.2 原子提交

一次成功调用按以下顺序提交：

1. 冻结 source range、Session revision 与 content fingerprint。
2. 调用 SummaryGenerator。
3. 再次验证 source fingerprint。
4. 以同一 checkpoint `AtomicDatasetPort::commit_atomic` 写入不可变 shard result 与成功 attempt record；先前失败 attempt 已分别持久化。
5. 对独立 manifest dataset 执行 CAS，注册 checkpoint 并推进 frontier。
6. 只有 manifest CAS 成功或恢复流程确认 orphan 可收养后，shard 才进入可选 projection。

文件 adapter **MUST** 复用 Storage 的 atomic dataset / CAS / recovery 机制，**NEVER** 在 Context 内复制 `tmp → fsync → rename` 实现。checkpoint dataset 的 key 由 `SessionId + SummaryShardId` 确定；manifest dataset 的 key 只由 `SessionId` 确定。

若第 4 步完成、第 5 步前崩溃，恢复扫描必须校验 source fingerprint、invocation identity 和 generation 后收养 orphan，避免再次请求 Provider。

### 9.3 生命周期与 GC

- 保留 active projection、previous projection 及其所有可达依赖。
- schema / prompt / normalization 版本升级时，旧 generation 继续服务；新 generation 完整后原子切换。
- 只追加历史不会使旧前缀 shard 失效。
- 历史 fingerprint 改变时只失效受影响范围及其祖先，稳定前缀继续复用。
- 新 generation 发布后，未被 active / previous 引用的 shard延迟回收。
- GC checkpoint 时只移除不可达 shard result；成功 attempt record 先以同一
  `(ModelInvocationId, attempt)` 物化到 `usage/`，**NEVER** 因 shard GC
  丢失或重复。
- 删除 Session 时删除 compact sidecar；独立 Audit usage 与 legacy global cost
  history 是否保留仍各自遵循其现有策略，Context **NEVER** 级联管理。

## 10. Compact attempt 账本与 Audit 边界

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompactPhase {
    Leaf,
    BackfillMap,
    BranchReduce,
    PrecompactReflection,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsageSource {
    ProviderReported,
    Estimated,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactAttemptUsage {
    pub invocation_id: ModelInvocationId,
    pub session_id: SessionId,
    pub job_id: CompactJobId,
    pub shard_id: Option<SummaryShardId>,
    pub phase: CompactPhase,
    pub provider: String,
    pub model: String,
    pub attempt: u32,
    pub status: CompactInvocationStatus,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub normalized_total_tokens: Option<u64>,
    pub source: UsageSource,
    pub duration_ms: u64,
    pub recorded_at: Timestamp,
}
```

- `ModelInvocationId` 直接复用 `packages/sdk` 发布的唯一 UUIDv7 newtype，
  **NEVER** 在 Context 重复定义 invocation ID。一个 Compact Job 对应一个逻辑
  Model Invocation；retry 保持同一 invocation id 并递增 attempt ordinal。
- Provider ACL 是标准化 usage 的唯一所有者；Context 只保存标准化值和
  compact 恢复所需的 attempt/status 元数据。
- `(invocation_id, attempt)` 是账本幂等键；同一完成事件重复写入返回幂等成功，
  **NEVER** 重复计入 token。
- `CompactUsageSummary` 必须由 attempt records 纯投影产生；attempt 之和等于
  Model Invocation 汇总，invocation 之和等于 Compact Job 与 Session compact
  总账。Provider 未返回 usage 的 attempt 保持 Unknown，不参与已知 token 求和。
- Context sidecar 账本是 compact execution checkpoint，不是 Audit-owned
  `UsageRecord`，也不替代 Audit JSONL。它必须记录失败 / 取消 / retry attempt，
  而 Audit v0.1.0 仍只接收成功逻辑 Model Invocation 的聚合事实。
- Context **NEVER** 持有 `UsagePort` / `UsageSink`，resume **NEVER** 加载 Audit。
  若 Composition 将成功 compact invocation 投影到 Audit，必须在
  `SummaryGenerator` 外层 bridge 聚合 attempts 后使用同一
  `ModelInvocationId`，且不能让 Audit enqueue / flush 结果影响 checkpoint。
- Session 总量展示在 query / presentation 层把 Main、Sub-agent 的既有来源与
  `CompactUsageSummary` 合并；Context 只拥有 compact 分量。
- sidecar **NEVER** 持久化 Price 或 Cost。`/cost` 如展示 compact 金额，只能在
  读取时复用 Runtime 现有 pricing 由 token 派生，并明确标记 estimated；Audit
  v0.1.0 仍不计算 Cost。
- 复用 shard记录 `reused_shards`，但 **NEVER** 虚构精确“节省 token”。

## 11. Ports、所有权与装配

```rust
#[async_trait]
pub trait SummaryGenerator: Send + Sync {
    async fn generate(
        &self,
        request: &SummaryRequest,
        cancel: &CancellationToken,
    ) -> Result<SummaryCompletion, SummaryGenerationError>;
}

#[async_trait]
pub trait CompactIndex: Send + Sync {
    async fn load(&self, session: &SessionId) -> Result<CompactIndexSnapshot, CompactIndexError>;
    async fn commit_shard(
        &self,
        request: CommitShardRequest,
    ) -> Result<CommitShardReceipt, CompactIndexError>;
    async fn publish_projection(
        &self,
        request: PublishProjectionRequest,
    ) -> Result<PublishProjectionReceipt, CompactIndexError>;
    async fn collect_garbage(
        &self,
        request: CompactGcRequest,
    ) -> Result<CompactGcReceipt, CompactIndexError>;
}
```

| 组件 | 唯一职责 |
|---|---|
| Context domain | coverage、分块、树、projection、版本和 compact attempt 聚合纯策略 |
| Context application | append 后调度、warm activation、Must 缺口等待、commit 编排 |
| `SummaryGenerator` adapter | Provider 调用、结构化响应 ACL、标准化 attempt usage 透传 |
| `CompactIndex` adapter | Storage atomic dataset、恢复、orphan 收养、GC |
| Composition | 进程级 scheduler 与 ports 唯一装配；可在 SummaryGenerator 外层桥接成功逻辑 invocation 到 Audit |
| Runtime | 只通过 `ContextPort` build / compact / status，不持有内部对象 |
| SDK / TUI | 只消费只读 status、progress、usage Published Language |

`ContextPort` 增加只读 `compact_status`：

```rust
async fn compact_status(
    &self,
    session_id: &SessionId,
) -> Result<CompactStatus, ContextPortError>;
```

`CompactStatus` 只包含 coverage、pending tokens、reused shard 数、当前 phase、circuit breaker 和累计 compact usage，**NEVER** 暴露 manifest 路径、原始 shard 或 Provider 私有 usage。

## 12. TUI 与可观测性

- `/cost`：Session 总量中单列 Compact，并拆 leaf / backfill map / branch reduce / reflection；金额是读取时派生 estimate，**NEVER** 回写 sidecar 或 Audit。
- `/compact`：展示 coverage、pending tokens、reused shards、当前 phase 和累计 compact tokens。
- 后台成功不刷屏；session circuit breaker 打开或持久化损坏时只发一次明确通知。
- `info` 只记录 job / projection 生命周期；per-shard / per-attempt 使用 `debug`，token 累计和细粒度状态使用 `trace`。
- Context → Runtime → SDK → TUI 的 status / progress / usage 每一层都必须有相邻契约测试。

## 13. 测试与验收

| 层级 | 必须证据 |
|---|---|
| L0 | fmt、all-target clippy `-D warnings`、workspace tests、架构守卫、production reachability、changed-lines coverage |
| L1 | coverage、分块、projection、版本失效、usage 幂等 / 守恒、permit 不变量 |
| L2 Context | barrier / channel 证明 6 sessions 时 global≤5、per-session≤1；warm activation 零 Provider 调用 |
| L2 Persistence | atomic commit 各崩溃点、manifest CAS、orphan 收养、旧 generation GC |
| L2 Runtime | 前台优先、Must 最多等一个 round trip、取消不发布、outcome 只应用一次 |
| L3 | Context → Runtime → SDK → TUI 每个相邻边界字段不丢失 |
| L4 | 第一次 compact → 新增历史 → 后台建树 → 退出 → 恢复 → 第二次 compact |
| L5 | 真实 Provider 复测 7 / 9 / 22 chunk 历史，记录冷回填、warm activation、第二次 compact 的调用、token 和耗时 |

确定性测试 **NEVER** 用短 `sleep`、墙钟差或“重跑成功”证明并发。使用 barrier、channel、controlled future 或 fault injection。

性能验收：

- Warm activation：零网络调用，真实性能 p95 ≤ 500ms；CI 不用墙钟断言。
- Must 状态只有一个缺口：最多等待一个 Provider round trip。
- 后台建树不阻塞前台 Run。

数据验收：

- completed shard 在重启后零重复调用。
- 第二次 compact 只处理第一次后的新范围。
- 失败、取消、fingerprint 冲突不发布部分 projection。
- per-attempt usage 之和等于 Model Invocation、Compact Job 与 Session compact 总账。
- 原始 Session 历史始终可恢复。

## 14. Issue / PR 拆分

| 顺序 | Issue | 交付 |
|---|---|---|
| 1 | [#1163](https://github.com/rushsinging/aemeath/issues/1163) | 领域模型、Published Language、coverage invariant |
| 2a | [#1164](https://github.com/rushsinging/aemeath/issues/1164) | SummaryGenerator、规范化输入、结构化 schema |
| 2b | [#1165](https://github.com/rushsinging/aemeath/issues/1165) | Sidecar、恢复、orphan、GC |
| 3 | [#1168](https://github.com/rushsinging/aemeath/issues/1168) | per-session 1 / global 5 scheduler |
| 4 | [#1166](https://github.com/rushsinging/aemeath/issues/1166) | Projection、增量生命周期、Runtime 接入 |
| 5 | [#1167](https://github.com/rushsinging/aemeath/issues/1167) | Compact attempt usage、SDK / TUI 与读取时 cost estimate |
| 6 | [#1119](https://github.com/rushsinging/aemeath/issues/1119) | Legacy map-reduce 回填迁移、Guard / Verify 与退役 |

依赖方向：`#1163 → (#1164, #1165) → #1168 → #1166 → #1167 → #1119`。每个叶子 Issue 对应独立 PR；父 Issue #1162 **NEVER** 直接承载代码 PR。

## 15. Current → Target

Current 的 `compact_summary.rs` 仍：

- 按 message 数保留约 10% recent tail；
- 以原始 message token 做 30K chunk；
- 串行执行 map，再执行一次 reduce；
- 只取生成文本，丢弃 `ProviderCompletion.usage`；
- 在进程内存中保存 sub summaries，退出后从头重做。

迁移期间：

1. Current 行为必须明确标为 legacy，**NEVER** 反向覆盖本文 Target。
2. 新领域模型、port、adapter 和 scheduler 按 §14 分 PR 落地。
3. #1119 最后把旧 map-reduce 迁为 checkpoint backfill，并清理旧同步主路径。
4. 新 projection 完成前，现有 compact 仍可服务；切换必须由 feature path / wiring 一次完成，**NEVER** 形成 Runtime 与 Context 双状态源。

## 16. 相关文档

- Compact 家族总览：[02-compact.md](02-compact.md)
- Session / RunStep 持久化边界：[01-session.md](01-session.md)
- Token Budget：[03-token-budget.md](03-token-budget.md)
- Runtime 端口：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- 测试与覆盖：[../../03-engineering/04-testing-and-coverage.md](../../03-engineering/04-testing-and-coverage.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-18 | 初稿：持久化增量摘要树、per-session 1 / global 5、warm projection、checkpoint 恢复、compact usage 总账 | [#1162](https://github.com/rushsinging/aemeath/issues/1162) |
