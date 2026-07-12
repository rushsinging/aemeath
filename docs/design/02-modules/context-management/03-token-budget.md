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
fn estimate_usage(req: &ContextRequest) -> usize {
    match req.last_api_input_tokens {
        Some(actual) => actual as usize,
        None => estimate_messages_tokens(&req.messages)
              + estimate_system_tokens(&req.system_prompt)
              + req.tool_schema_tokens,
    }
}
```

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
- **reasoning_tokens 不加**：已包含在 API 返回的 `input_tokens` 中，单独加会导致双重计数。当前代码以 `_reasoning_tokens: Option<u64>` 参数保留但不使用（call-site stability）
- **cached_tokens 不减**：Anthropic prompt caching 的 cached tokens 仍计入 input_tokens，不减去避免低估
- **tool_schema_tokens 独立计算**：`estimate_tool_schemas_tokens()` 遍历 tool 定义 JSON

### 2.4 已知局限

| 局限 | 影响 | 演进方向 |
|---|---|---|
| 无 tokenizer 集成 | 估算偏差 1.3×–4×（CJK 尤甚） | 接入 tiktoken（注释中已标注 "consider integrating tiktoken"） |
| Image 固定 85 tokens | 无分辨率感知 | 按 image dimensions 估算 |
| JSON 估算按字符 | 未考虑 key 压缩 | 按 JSON 结构估算（key 重叠率高时实际 token 更少） |
| `bytes_per_token` 固定 4.0 | 模型差异未体现 | 按 model 调整 ratio（`.with_model_ratio()` builder 已存在但未用全） |

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
max_output = ProviderPort.max_output_tokens(model)    // 真实值，替代硬编码 8192
effective  = context_size - min(max_output, max_summary_output_tokens)
threshold  = effective - autocompact_buffer_tokens
```

**示例**（context_size=200,000, max_output=16,000）：
```
effective  = 200,000 - min(16,000, 20,000) = 200,000 - 16,000 = 184,000
threshold  = 184,000 - 13,000 = 171,000
```

### 4.2 max_output_tokens 注入

**当前问题**：`needs_compaction_actual` 和 `compaction_urgency` 硬编码 `max_output = 8192`（3 处），忽略模型真实 max_tokens。

**影响**：
- 真实 max_tokens = 16,000 的模型，阈值计算用 8,192 → threshold 偏低 → compact 触发偏早
- 真实 max_tokens = 4,096 的模型，阈值计算用 8,192 → threshold 偏高 → compact 触发偏晚

**目标**：
- `ContextRequest.max_output_tokens` 由 Runtime 从 `ProviderPort` 获取
- `needs_compaction` 和 `compaction_urgency` 使用 `req.max_output_tokens`，不硬编码
- ProviderPort 新增 `fn max_output_tokens(&self, model: &str) -> usize`

### 4.3 compaction_urgency 分级

```rust
fn compaction_urgency(req: &ContextRequest) -> Urgency {
    let effective = effective_context_window(req.context_size, req.max_output_tokens);
    let total = match req.last_api_input_tokens {
        Some(actual) => actual as usize,
        None => estimate_total_tokens(req),
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
fn needs_compaction(req: &ContextRequest) -> CompactionDecision {
    let effective = effective_context_window(req.context_size, req.max_output_tokens);
    let threshold = autocompact_threshold(effective);
    let total = match req.last_api_input_tokens {
        Some(actual) if actual > 0 => {
            let t = actual as usize;
            CompactionDecision {
                needed: t > threshold,
                urgency: compaction_urgency(req),
                estimated_tokens: t,
                threshold,
                reason: DecisionReason::ActualApi,
            }
        }
        _ => {
            let t = estimate_total_tokens(req);
            CompactionDecision {
                needed: t > threshold,
                urgency: compaction_urgency(req),
                estimated_tokens: t,
                threshold,
                reason: DecisionReason::Heuristic,
            }
        }
    }
}
```

**纯函数保证**：相同 `ContextRequest` → 相同 `CompactionDecision`。无外部状态依赖。

## 5. 幂等性设计（#550）

### 5.1 问题分析

| 层 | 幂等？ | 原因 |
|---|---|---|
| `needs_compaction` | ✅ | 纯函数，相同输入 → 相同输出 |
| `needs_compaction_actual` / `needs_compaction_full` | ✅ | 纯函数 |
| `compaction_urgency` | ✅ | 纯函数 |
| `auto_compact` 外层 | ❌ | 每轮无条件触发 PreCompact hook + microcompact 扫描 |
| `apply_compact_outcome` | ❌ | 重复调用会 double-freeze 旧链 |

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
struct ContextState {
    last_fingerprint: Option<CompactionFingerprint>,
    autocompact_state: AutoCompactState,
}

fn build_window(&self, req: &ContextRequest, state: &mut ContextState) -> ContextWindow {
    let fingerprint = CompactionFingerprint::from(req);

    if state.last_fingerprint.as_ref() == Some(&fingerprint) {
        // fingerprint 不变：跳过 L2/L3 重复扫描，直接复用上次 ContextWindow
        return self.last_window.clone();
    }

    // fingerprint 变化：执行完整 build_window
    let window = self.do_build_window(req);

    state.last_fingerprint = Some(fingerprint);
    window
}
```

### 5.4 Hook 去重

- PreCompact hook 只在 `needs_compaction == true` 时触发（当前是每轮无条件触发）
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

## 6. Per-message Token 标注（演进方向）

### 6.1 问题

当前每次 `estimate_messages_tokens` 全量遍历消息列表。对于长 session（100+ 消息），每轮 build_window 都重算。

### 6.2 目标

```rust
struct MessageWithTokens {
    message: Message,
    cached_tokens: Option<usize>,    // 首次估算后缓存
}
```

- message 内容不变时复用 `cached_tokens`
- message 被 microcompact/snip 修改后清除 cache
- 新增消息时增量计算

### 6.3 暂不实现

v0.1.0 不实现 per-message 标注。原因：
1. 估算速度已足够（100 条消息 < 1ms）
2. 引入 cache invalidation 增加复杂度
3. 待 #550 幂等化后，fingerprint 可避免重复全量估算

## 7. 预算分配

aemeath **不主动分配** system / history / tool / response 的 token 预算——模型自行处理。

| 部分 | 占比 | 由谁决定 |
|---|---|---|
| system prompt | 5-15% | PromptPort 组装（见 [04-prompt-guidance.md](04-prompt-guidance.md)） |
| tool schemas | 2-8% | ToolCatalogPort snapshot |
| 对话历史 | 60-85% | ChatChain（经 compact 管控） |
| memory 注入 | <1% | MemoryPort（默认 5 条 ≈ 300 tokens） |
| response | max_output_tokens | ProviderPort |

**唯一的主动管控**是 compact——当历史占比过高导致总 token 超 threshold 时触发 compact。

## 8. 遗留清理

### 8.1 退役项

| 遗留 | 位置 | 退役路径 |
|---|---|---|
| `TokenEstimation::warning_threshold` (80%) | `token_estimation.rs:14` | 删除——被 `autocompact_threshold` 替代，不再使用 |
| `TokenEstimation::is_near_limit` | `token_estimation.rs:63` | 删除——同上 |
| `needs_compaction_with_output` 参数 | `token_estimation.rs:293` | 删除参数——调用方硬编码 8192，应直接从 req 获取 |
| 硬编码 `8192` | `token_estimation.rs:289,329,356` | 替换为 `req.max_output_tokens` |
| `ContextUsage` struct | `token_estimation.rs:103` | 保留（TUI 展示用），但从 decision path 中移除 |
| `compact_messages`（无 LLM 版本） | `summary.rs` | 评估是否被外部调用——如否则删除 |
| `messages_selected_for_precompact_memory` | `summary.rs` | 删除——`auto_compact` 未调用它 |

### 8.2 保留项

| 项目 | 保留原因 |
|---|---|
| `_reasoning_tokens` / `_cached_tokens` 参数 | call-site stability，后续可能用于日志 |
| `format_tokens()` | TUI 展示用 |
| `estimate_tool_schemas_tokens()` | tool 定义 token 估算 |

## 9. 相关文档

- Compact 家族：[02-compact.md](02-compact.md)
- Memory 注入：[05-memory-injection.md](05-memory-injection.md)
- Runtime 端口（ProviderPort.max_output_tokens）：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Run 状态机（Compacting 状态触发）：[../runtime/03-loop-and-state-machine.md](../runtime/03-loop-and-state-machine.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：估算策略、effective window 公式、常量统一、幂等性设计、遗留清理 | #786 |
