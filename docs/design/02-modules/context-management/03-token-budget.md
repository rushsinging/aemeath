# Context Management · Token Budget

> 层级：02-modules / context-management（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#786（S2）
> 本文定义 token 预算计算的单一真相——估算策略、effective context window 公式、auto-compact 阈值、幂等决策保证（#550）。所有常量收口于此，禁止散落。

## 1. 定位

Token Budget 是 Compact 家族（[02-compact.md](02-compact.md)）和 ContextPort `needs_compaction` 的**计算基础**：

- 回答"当前上下文用了多少 token？还有多少空间？"
- 回答"是否需要 compact？紧迫程度？"
- 保证幂等性：相同输入 → 相同决策（#550）

**不在本文范围**：实际 API token 计数（由 Provider 返回）、tokenizer 集成（演进方向）。

## 2. TokenEstimation 策略

### 2.1 两层估算

aemeath 有两条 token 估算路径，**按优先级选择**：

| 路径 | 来源 | 精度 | 使用时机 |
|---|---|---|---|
| **Actual API** | `last_api_input_tokens`（provider 响应） | 精确 | 非首轮、API 正常返回 |
| **Heuristic** | `estimate_messages_tokens()` | 估算（偏保守） | 首轮、API 未返回 token、resume 后首轮 |

```rust
fn estimate_usage(req: &ContextRequest, candidate: &WindowCandidate) -> usize {
    match req.last_api_input_tokens {
        Some(actual) => actual as usize,
        None => estimate_messages_tokens(&candidate.messages)
              + estimate_system_tokens(&candidate.system_blocks)
              + req.tool_schema_tokens,
    }
}
```

`WindowCandidate.messages` 由 Context-owned 已提交 history + `req.pending_messages` 生成；Runtime **NEVER** 回传一份可变 history 副本。`candidate.system_blocks` 遵循 [Prompt & Guidance](04-prompt-guidance.md) 的唯一 block 顺序，`req.tool_schema_tokens` 则只计算 `req.tool_schemas` 这一份冻结投影。

### 2.2 启发式估算算法

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

### 2.3 设计决策

- **偏保守**：估算值 > 实际值是安全方向——compact 触发偏早比偏晚好
- **reasoning_tokens 不加**：已包含在标准化 `input_tokens` 中，单独相加会导致双重计数；估算 API **NEVER** 保留一个未消费的 reasoning 参数
- **cached_tokens 不减**：Anthropic prompt caching 的 cached tokens 仍计入 input_tokens，不减去避免低估
- **tool_schema_tokens 独立计算**：`estimate_tool_schemas_tokens()` 遍历 tool 定义 JSON

### 2.4 已知局限

| 局限 | 影响 | 演进方向 |
|---|---|---|
| 无 tokenizer 集成 | 估算偏差 1.3×–4×（CJK 尤甚） | 按模型能力接入 tokenizer |
| Image 固定 85 tokens | 无分辨率感知 | 按 image dimensions 估算 |
| JSON 估算按字符 | 未考虑 key 压缩 | 按 JSON 结构估算（key 重叠率高时实际 token 更少） |
| `bytes_per_token` 固定 4.0 | 模型差异未体现 | 按 model 调整 ratio |

## 3. TokenBudgetConfig — 常量统一来源

```rust
struct TokenBudgetConfig {
    /// summary LLM 调用的 max_tokens 上限（p99.99 LLM 摘要输出）
    max_summary_output_tokens: usize,       // 20_000

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

    /// microcompact segment 保护窗口
    microcompact_protect_main: usize,       // 3
    microcompact_protect_sub: usize,        // 2
}
```

> **`max_output_tokens` 不在此 struct**——它从 `ProviderPort` 获取模型真实值，不是全局常量。见 §4。

### 3.1 常量来源说明

| 常量 | 值 | 依据 |
|---|---|---|
| `max_summary_output_tokens` | 20,000 | p99.99 LLM 生成的摘要输出不会超过此值 |
| `autocompact_buffer_tokens` | 13,000 | 安全缓冲：compact LLM 调用本身的输入+输出+下一轮用户输入的预留 |
| `estimation_safety_factor` | 1.33 | 4/3 保守系数，覆盖 JSON 结构和估算不确定性 |
| `map_reduce_chunk_threshold` | 30,000 | 超过此值时分块 map-reduce，每块 ≤ 此值 |
| `microcompact_protect_main` | 3 | Main Run 保护最近 3 个 segment 不被 microcompact |
| `microcompact_protect_sub` | 2 | Sub Run 保护最近 2 个 user turn |

## 4. Effective Context Window

### 4.1 公式

```
resolved   = ProviderPort.resolve_invocation_options(model, requested)
max_output = resolved.max_output_tokens
effective  = context_size - min(max_output, max_summary_output_tokens)
threshold  = effective - autocompact_buffer_tokens
```

**示例**（context_size=200,000, max_output=16,000）：
```
effective  = 200,000 - min(16,000, 20,000) = 200,000 - 16,000 = 184,000
threshold  = 184,000 - 13,000 = 171,000
```

### 4.2 max_output_tokens 注入

- Runtime 在每次 PreparingContext、且在 `build_window` 前调用 `ProviderPort.resolve_invocation_options`，把返回的真实上限写入 `ContextRequest.max_output_tokens`；同一个 `ResolvedInvocationOptions` 随后进入 `InvocationRequest`。
- `needs_compaction` 和 `compaction_urgency` **MUST** 只使用 `req.max_output_tokens`，**NEVER** 以固定 `8192` 或另一个 provider lookup 形成第二真相。

### 4.3 compaction_urgency 分级

```rust
fn compaction_urgency(req: &ContextRequest, candidate: &WindowCandidate) -> Urgency {
    let effective = effective_context_window(req.context_size, req.max_output_tokens);
    let total = match req.last_api_input_tokens {
        Some(actual) => actual as usize,
        None => estimate_usage(req, candidate),
    };
    let pct = total * 100 / effective;

    match pct {
        0..=69 => Urgency::None,
        70..=79 => Urgency::Monitor,
        80..=89 => Urgency::Should,
        _ => Urgency::Must,
    }
}
```

### 4.4 needs_compaction 决策

```rust
fn needs_compaction(req: &ContextRequest, candidate: &WindowCandidate) -> CompactionDecision {
    let effective = effective_context_window(req.context_size, req.max_output_tokens);
    let threshold = autocompact_threshold(effective);
    let total = match req.last_api_input_tokens {
        Some(actual) if actual > 0 => {
            let t = actual as usize;
            CompactionDecision {
                needed: t > threshold,
                urgency: compaction_urgency(req, candidate),
                estimated_tokens: t,
                threshold,
                reason: DecisionReason::ActualApi,
            }
        }
        _ => {
            let t = estimate_usage(req, candidate);
            CompactionDecision {
                needed: t > threshold,
                urgency: compaction_urgency(req, candidate),
                estimated_tokens: t,
                threshold,
                reason: DecisionReason::Heuristic,
            }
        }
    }
}
```

**确定性保证**：相同 Context backing revision + 相同 `ContextRequest` → 相同 `WindowCandidate` 与 `CompactionDecision`。`ContextPort::needs_compaction` 在内部对稳定 backing + `pending_messages` 建 candidate，Runtime 不提供或修改历史。supplier revision 变化会形成新的 candidate/fingerprint，而不是在相同输入下产生漂移。

## 5. 幂等性设计（#550）

### 5.1 幂等性矩阵

| 层 | 必须满足的约束 | 守护方式 |
|---|---|---|
| `needs_compaction` | 相同输入 → 相同输出 | 纯函数 |
| `needs_compaction_actual` / `needs_compaction_full` | 相同输入 → 相同输出 | 纯函数 |
| `compaction_urgency` | 相同输入 → 相同分级 | 纯函数 |
| `auto_compact` 外层 | fingerprint 不变时不重复触发 Hook / 扫描 | `CompactionFingerprint` |
| `apply_compact_outcome` | 同一 source revision 最多提交一次 | expected revision + committed marker |

### 5.2 CompactionFingerprint

```rust
#[derive(PartialEq, Eq, Hash)]
struct CompactionFingerprint {
    messages_hash: u64,                // messages 内容 hash（to_llm_view 后）
    last_api_input_tokens: Option<u64>,
    context_size: usize,
    max_output_tokens: usize,
    tool_schema_count: usize,          // tool 定义变化时 fingerprint 变化
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
) -> Result<ContextWindow, ContextWindowError> {
    let fingerprint = CompactionFingerprint::from(req);

    // cache 是 Context implementation-owned backing，NEVER 作为第五个 OHS 参数暴露。
    // fingerprint 只缓存纯 L2-L4 投影，NEVER 缓存整个 ContextWindow。
    let projection = self.projection_cache
        .get_or_compute(fingerprint, || self.project_compaction(req))?;

    // 易变外部输入每轮都经各自 owner 物化；adapter 内部可按 revision 命中缓存。
    // 可观察顺序固定：Prompt/Skill → Memory → summary → final assembly。
    let prompt = self.prompt.build_system_prompt(req.prompt_request()).await?;
    let memory = self.memory.retrieve_for_inject(req.memory_query())?;
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

- PreCompact hook **MUST** 只在 `needs_compaction == true` 时触发
- microcompact 只在 `fingerprint.messages_hash` 变化时执行

### 5.5 apply_compact_outcome 重入安全

```rust
fn apply_compact_outcome(chain: &mut ChatChain, result: CompactResult) {
    // 幂等检查：如果最近一个 segment 已是 Compact 且 summary 相同，跳过
    if let Some(last) = chain.active_segments().last() {
        if last.kind == SegmentKind::Compact && last.summary.as_deref() == Some(&result.summary) {
            return; // 已应用，跳过
        }
    }

    // 正常流程
    chain.freeze_active();
    chain.compact(result.summary, result.recent_messages);
}
```

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

**唯一的主动管控**是 compact——当历史占比过高导致总 token 超 threshold 时触发 compact。

## 8. 相关文档

- Compact 家族：[02-compact.md](02-compact.md)
- Memory 注入：[05-memory-injection.md](05-memory-injection.md)
- Runtime 端口（ProviderPort capabilities）：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Current → Target 迁移责任：[../../03-engineering/migration-governance.md](../../03-engineering/migration-governance.md)
- Run 状态机（Compacting 状态触发）：[../runtime/03-loop-and-state-machine.md](../runtime/03-loop-and-state-machine.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：估算策略、effective window 公式、常量统一、幂等性设计、遗留清理 | #786 |
