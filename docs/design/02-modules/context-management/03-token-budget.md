# Context Management · Token Budget

> 层级：02-modules / context-management（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#786（S2）
> 本文定义 token 预算计算的单一真相——估算策略、effective context window 公式、auto-compact 阈值、幂等决策保证（#550）。所有常量收口于此，禁止散落。

## 1. 定位

Token Budget 是 Compact 家族（[02-compact.md](02-compact.md)）和 `build_window` 内 `compaction_decision` 计算的**基础**：

- 回答"当前上下文用了多少 token？还有多少空间？"
- 回答"是否需要 compact？紧迫程度？"
- 保证幂等性：相同输入 → 相同决策（#550）

**不在本文范围**：实际 API token 计数（由 Provider 返回）、tokenizer 集成（演进方向）。

## 2. TokenEstimation 策略

### 2.1 Provider 实际值与启发式估算

aemeath 保留两类 token 数据，但职责不同：

| 数据 | 来源 | 用途 |
|---|---|---|---|
| **Actual Provider usage** | `last_total_tokens`（上一次 Provider 响应） | 自动 compact 的唯一触发依据 |
| **Heuristic** | `estimate_messages_tokens()` 等 | UI/日志诊断、recent-tail 30% 预算、summary/map-reduce 分块；**NEVER** 单独触发自动 compact |

`last_total_tokens` 是最近一次调用的 context usage，不是 Session 累计成本。只有
Provider 成功返回新 usage 才更新；compact 成功后清为 `None`，防止旧值重复触发。

> **Current / Target 边界**：Provider usage 标准化与自动触发已落地；
> recent-tail 30% 是 RunStep backing 完成后的 Target。Current 仍按 message
> 数保留约 10%（至少 4 条），不按 Run / Step token 预算裁剪。

```rust
fn should_auto_compact(req: &ContextRequest, threshold: usize) -> bool {
    req.last_total_tokens
        .is_some_and(|total| total > threshold as u64)
}

fn estimate_candidate(req: &ContextRequest, candidate: &WindowCandidate) -> usize {
    estimate_messages_tokens(&candidate.messages)
        + estimate_system_tokens(&candidate.system_blocks)
        + req.tool_schema_tokens
}
```

`WindowCandidate.messages` 由 Context-owned 已提交 history + `req.pending_messages` 生成；Runtime **NEVER** 回传一份可变 history 副本。`candidate.system_blocks` 遵循 [Prompt & Guidance](04-prompt-guidance.md) 的唯一 block 顺序，`req.tool_schema_tokens` 则只计算 `req.tool_schemas` 这一份冻结投影。

### 2.2 Provider usage 标准化

Provider ACL **MUST** 输出 provider-neutral usage：

```rust
struct UsageSnapshot {
    input_tokens: u64,
    output_tokens: u64,
    cache_read_input_tokens: u64,
    cache_creation_input_tokens: u64,
    total_input_tokens: u64,
    total_tokens: u64,
}
```

| Provider 族 | `total_input_tokens` | `total_tokens` |
|---|---|---|
| Anthropic | `input_tokens + cache_read_input_tokens + cache_creation_input_tokens` | `total_input_tokens + output_tokens` |
| OpenAI-compatible | Provider 的 prompt/input token；cached tokens 已包含其中，**NEVER** 重复相加 | 优先 Provider `total_tokens`，缺失时 `input_tokens + output_tokens` |

Runtime / Context 只消费 `total_tokens`。Provider 原始字段 MAY 用于 cost / audit，
但不得泄漏为 compact 分支。

### 2.3 启发式估算算法

```rust
const BYTES_PER_TOKEN: f64 = 4.0;       // 英文 ≈ 4 bytes/token
const CJK_TOKEN_RATIO: f64 = 2.0;       // CJK 字符 ≈ 2 tokens/char（vs 英文 0.25）
const SAFETY_FACTOR: f64 = 1.33;        // 保守系数（JSON 结构 + 不确定性）
const IMAGE_TOKENS: usize = 85;         // 图像固定估算（无分辨率感知）
```

```rust
fn estimate_message_tokens(msg: &Message) -> usize {
    let text_tokens = msg.content.chars().map(|c| {
        if is_cjk(c) { CJK_TOKEN_RATIO } else { 1.0 / BYTES_PER_TOKEN }
    }).sum::<f64>();

    let tool_tokens = msg.tool_calls.iter()
        .map(|tc| estimate_json_tokens(&tc.args))
        .sum::<usize>();

    let image_tokens = msg.content_blocks.iter()
        .filter(|b| matches!(b, ContentBlock::Image(_)))
        .count() * IMAGE_TOKENS;

    ((text_tokens + tool_tokens as f64 + image_tokens as f64) * SAFETY_FACTOR) as usize
}
```

### 2.4 设计决策

- **偏保守**：估算值 > 实际值是安全方向——compact 触发偏早比偏晚好
- **自动触发只认 Actual**：没有新 Provider usage 时不进入 `Compacting`；heuristic 只做预算和诊断
- **reasoning_tokens 不额外相加**：若 Provider 的 `total_tokens` 已包含 output/reasoning，重复相加会双计
- **Anthropic cache tokens 必须相加**：其 `input_tokens` 不代表完整 context input；cache read / creation 均占 context window
- **OpenAI cached tokens 不重复相加**：其 prompt/input tokens 已包含 cached 部分
- **tool_schema_tokens 独立计算**：`estimate_tool_schemas_tokens()` 遍历 tool 定义 JSON

### 2.5 已知局限

| 局限 | 影响 | 演进方向 |
|---|---|---|
| 无 tokenizer 集成 | 估算偏差 1.3×–4×（CJK 尤甚） | 按模型能力接入 tokenizer |
| Image 固定 85 tokens | 无分辨率感知 | 按 image dimensions 估算 |
| JSON 估算按字符 | 未考虑 key 压缩 | 按 JSON 结构估算（key 重叠率高时实际 token 更少） |
| `bytes_per_token` 固定 4.0 | 模型差异未体现 | 按 model 调整 ratio |

## 3. TokenBudgetConfig — 常量统一来源

```rust
struct TokenBudgetConfig {
    // summary_budget 不再在此 struct——改为 token_budget::summary_budget(context_size) 动态计算（context_size * 2%）

    /// auto-compact 触发缓冲区
    autocompact_buffer_tokens: usize,       // 13_000

    /// 估算安全系数
    estimation_safety_factor: f64,          // 1.33

    /// CJK 字符 token 比
    cjk_token_ratio: f64,                   // 2.0

    /// 英文 bytes/token
    bytes_per_token: f64,                   // 4.0

    /// 图像固定 token 估算
    image_tokens: usize,                    // 85

    /// map-reduce 分块阈值
    map_reduce_chunk_threshold: usize,      // 30_000

    /// Snip / Microcompact 统一 Run 保护窗口
    compact_family_protect_recent_runs: usize, // 3
}
```

> **`max_output_tokens` 不在此 struct**——它从 `ProviderPort` 获取模型真实值，不是全局常量。见 §4。

### 3.1 常量来源说明

| 常量 | 值 | 依据 |
|---|---|---|
| `summary_budget` | `context_size * 2%`（动态） | 按比例缩放，100K→2000 / 272K→5440；summary 作为后续每轮固定前缀，按比例比写死常量更合理 |
| `autocompact_buffer_tokens` | 13,000 | 安全缓冲：compact LLM 调用本身的输入+输出+下一轮用户输入的预留 |
| `estimation_safety_factor` | 1.33 | 4/3 保守系数，覆盖 JSON 结构和估算不确定性 |
| `map_reduce_chunk_threshold` | 30,000 | 超过此值时分块 map-reduce，每块 ≤ 此值 |
| `compact_family_protect_recent_runs` | 3 | Main / Sub 统一保护最近 3 个完整 Run；RunStep 不推进窗口 |

## 4. Effective Context Window

### 4.1 公式

```
resolved       = ProviderPort.resolve_invocation_options(model, requested)
max_output     = resolved.max_output_tokens
summary_budget = context_size * 2%
effective      = context_size - min(max_output, summary_budget)
threshold      = (effective - autocompact_buffer_tokens) * 0.8
```

**示例**（context_size=200,000, max_output=16,000）：
```
summary_budget = 200,000 * 2% = 4,000
effective      = 200,000 - min(16,000, 4,000) = 200,000 - 4,000 = 196,000
threshold      = (196,000 - 13,000) * 0.8 = 146,400
```

### 4.2 max_output_tokens 注入

- Runtime 在每次 PreparingContext、且在 `build_window` 前调用 `ProviderPort.resolve_invocation_options`，把返回的真实上限写入 `ContextRequest.max_output_tokens`；同一个 `ResolvedInvocationOptions` 随后进入 `InvocationRequest`。
- `compaction_decision` 计算和 `compaction_urgency` **MUST** 只使用 `req.max_output_tokens`，**NEVER** 以固定 `8192` 或另一个 provider lookup 形成第二真相。

### 4.3 compaction_urgency 分级

```rust
fn compaction_urgency(req: &ContextRequest, candidate: &WindowCandidate) -> Urgency {
    let effective = effective_context_window(req.context_size, req.max_output_tokens);
    let total = req.last_total_tokens
        .map(|value| value as usize)
        .unwrap_or_else(|| estimate_candidate(req, candidate));
    let pct = total * 100 / effective;

    match pct {
        0..=69 => Urgency::None,
        70..=79 => Urgency::Monitor,
        80..=89 => Urgency::Should,
        _ => Urgency::Must,
    }
}
```

### 4.4 compaction_decision 决策（build_window 内部纯函数）

> 以下纯函数是 `build_window` 内部计算 `compaction_decision` 的 helper。`build_window` 从自身稳定 backing 构造 `WindowCandidate` 后调用此函数，结果写入 `ContextWindow.compaction_decision`。

```rust
fn needs_compaction(req: &ContextRequest, candidate: &WindowCandidate) -> CompactionDecision {
    let effective = effective_context_window(req.context_size, req.max_output_tokens);
    let threshold = autocompact_threshold(effective);
    let estimated = estimate_candidate(req, candidate);
    let (needed, reason, observed) = if let Some(total) = req.last_total_tokens {
        (
            total > threshold as u64,
            DecisionReason::ActualProviderUsage,
            total as usize,
        )
    } else {
        (false, DecisionReason::NoActualUsage, estimated)
    };
    CompactionDecision {
        needed,
        urgency: compaction_urgency(req, candidate),
        estimated_tokens: observed,
        threshold,
        reason,
    }
}
```

**确定性保证**：相同 Context backing revision + 相同 `ContextRequest` → 相同 `WindowCandidate` 与 `CompactionDecision`。`build_window` 在内部对稳定 backing + `pending_messages` 建 candidate，Runtime 不提供或修改历史。supplier revision 变化会形成新的 candidate/fingerprint，而不是在相同输入下产生漂移。

## 5. 幂等性设计（#550）

### 5.1 幂等性矩阵

| 层 | 必须满足的约束 | 守护方式 |
|---|---|---|
| `needs_compaction` | 相同输入 → 相同输出 | 纯函数 |
| `needs_compaction_actual` / `needs_compaction_full` | 相同输入 → 相同输出 | 纯函数 |
| `compaction_urgency` | 相同输入 → 相同分级 | 纯函数 |
| `auto_compact` 外层 | fingerprint 不变时不重复触发 Hook / 扫描 | `CompactionFingerprint` |
| `ChatChain::compact` | 同一 source revision 最多提交一次 | `compact_source_revision` + `compact_committed` marker |

### 5.2 CompactionFingerprint

```rust
#[derive(PartialEq, Eq, Hash)]
struct CompactionFingerprint {
    backing_revision: SessionRevision,     // Context backing 的稳定 revision（ChatChain 版本）
    pending_messages_hash: u64,            // req.pending_messages 内容 hash
    last_total_tokens: Option<u64>,
    context_size: usize,
    max_output_tokens: usize,
    tool_schema_hash: u64,                 // tool 定义内容 hash（schema 变化时 fingerprint 变化）
}
```

### 5.3 幂等保护机制

```rust
struct ProjectionCache {
    last_fingerprint: Option<CompactionFingerprint>,
    last_projection: Option<CompactionProjection>,
}

struct ContextImplementation {
    projection_cache: ProjectionCache, // implementation 内部同步；不跨 OHS 暴露
    autocompact_state: AutoCompactState,
}

async fn build_window(
    &self,
    req: &ContextRequest,
) -> Result<ContextWindow, ContextWindowError> {    // 1. 先读取 Context-owned 稳定 backing（ChatChain），获得当前 revision。
    let backing = self.session.read_backing().await?;
    let revision = backing.revision();

    // 2. 基于 backing + req 构造 candidate（已提交 history + pending_messages）。
    let candidate = self.build_candidate(&backing, req);

    // 3. fingerprint 在读取稳定 backing 后生成——backing_revision 纳入 key，
    //    确保 Session 历史变化时即使 pending input 相同也不会复用旧 projection。
    let fingerprint = CompactionFingerprint::from(revision, req, &candidate);

    // cache 是 Context implementation-owned backing，NEVER 作为第五个 OHS 参数暴露。
    // fingerprint 只缓存纯 L2-L4 投影，NEVER 缓存整个 ContextWindow。
    let projection = self.projection_cache
        .get_or_compute(fingerprint, || self.project_compaction(req))?;

    // 易变外部输入每轮都经各自 owner 物化；adapter 内部可按 revision 命中缓存。
    // 可观察顺序固定：Prompt/Skill → Memory → summary → final assembly。
    let prompt = self.prompt.build_system_prompt(req.prompt_request()).await?;
    let memory = self.memory.retrieve_for_inject(req.memory_query());
    let summary = self.active_summary(&projection);
    Ok(self.assemble_window(
        projection,
        prompt,
        memory,
        summary,
        req.task_reminder.clone(),
    ))
}
```

`CompactionFingerprint` 的幂等范围只是纯 compact 投影与对应 hook / scan 去重。每轮顺序固定为 L2-L4 投影 → await Prompt guidance / Skill → Memory 检索 → active summary → 最终 block 编排。最终 blocks 则严格采用 `system_prompt → execution_discipline → model_guidance → skills → agent_roles → user_guidance → memory_context → active_summary → breakpoint → current_date → git_context → task_reminder`；Prompt Guidance、Skill materialization、Memory、Task reminder 与 project snapshot 不得因 compact fingerprint 命中而跳过，否则外部 revision 已变时会返回陈旧窗口。

### 5.4 Hook 去重

- PreCompact hook **MUST** 只在 `compaction_decision.needed == true` 时触发
- microcompact 只在 `fingerprint.pending_messages_hash` 变化时执行

### 5.5 Compact 提交幂等性

Compact 提交的唯一入口是 `ChatChain::compact(summary, recent_runs, source_revision)`（三参数版，定义见 [01-session.md](01-session.md) §3.1，集成说明见 [02-compact.md](02-compact.md) §8.7）。

`ChatChain::compact` 内部幂等保证：

- 若最近 segment 已是 Compact 且 `compact_source_revision == source_revision` 且 `compact_committed == true`，则跳过（同一 source revision 最多提交一次）
- **NEVER** 用 summary 文本等值判断幂等：相同 summary、不同 recent runs 或不同 source revision 是不同的 compact 操作，跳过会导致数据丢失

## 6. Per-message Token cache 决策

Per-message cache 是 Future optimization candidate：

```rust
struct MessageWithTokens {
    message: Message,
    cached_tokens: Option<usize>,    // 首次估算后缓存
}
```

- message 内容不变时复用 `cached_tokens`
- message 被 microcompact/snip 修改后清除 cache
- 新增消息时增量计算

v0.1.0 **NEVER** 预建该 cache。原因：
1. 估算速度已足够（100 条消息 < 1ms）
2. 引入 cache invalidation 增加复杂度
3. 待 #550 幂等化后，fingerprint 可避免重复全量估算

## 7. 预算分配

aemeath **不主动分配** system / history / tool / response 的 token 预算——模型自行处理。

| 部分 | 占比 | 由谁决定 |
|---|---|---|
| system prompt | 5-15% | PromptPipeline async 组装（见 [04-prompt-guidance.md](04-prompt-guidance.md)） |
| tool schemas | 2-8% | ToolCatalogPort snapshot |
| 对话历史 | 60-85% | ChatChain（经 compact 管控） |
| memory 注入 | <1% | Context memory integration（Config 上限 ∩ 剩余 budget） |
| response | max_output_tokens | ProviderPort |

主动管控分两类：

- Snip / Microcompact（Target）：每次 `PreparingContext` 常驻执行，按完整 Run
  保护最近 3 个 Run；Current 仍在 compact 管线内调用既有 Microcompact，
  本次未迁移为常驻投影；
- Auto-compact：仅当最近 Provider 标准化 `last_total_tokens > threshold` 时进入
  `Compacting`。RunStep-aware recent tail Target 单独使用 heuristic 估算，并
  限制在 `context_size * 30%`，不包含 summary/system/tool schemas；Current
  recent tail 仍保持 message 10%。

## 8. 相关文档

- Compact 家族：[02-compact.md](02-compact.md)
- Memory 注入：[05-memory-injection.md](05-memory-injection.md)
- Runtime 端口（ProviderPort capabilities）：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Current → Target 迁移责任：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)
- Run 状态机（Compacting 状态触发）：[../runtime/03-loop-and-state-machine.md](../runtime/03-loop-and-state-machine.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：估算策略、effective window 公式、常量统一、幂等性设计、遗留清理 | #786 |
| 2026-07-16 | summary_budget 改为动态计算（context_size * 2%），替代写死的 max_summary_output_tokens=20000；threshold 公式加 *0.8 系数 | #1110 |
| 2026-07-17 | Provider ACL 标准化 total tokens（Anthropic 纳入 cache read/create）；自动 compact 只由 last_total_tokens 触发；明确 recent-tail 30% 与常驻 Snip/Microcompact 为 Deferred Target，Current tail 保持 message 10% | compact token reset design |
