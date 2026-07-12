# Context Management · Compact 家族

> 层级：02-modules / context-management（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#786（S2）
> 本文定义 Compact 家族——ContextPort 的压缩管线，五级策略从零成本规则到 LLM 摘要的完整分层。ContextPort 是 Context Management BC 对 Agent Runtime 的 OHS（见 [01-session.md](01-session.md) §6）。

## 1. 定位

Compact 家族是 Context Management 的**核心能力**：在 LLM context window 耗尽前，以最小代价回收 token 预算。

- **内聚于 ContextPort**：五级管线是 ContextPort 的实现细节，Runtime 只调 3 个方法（见 §2）
- **策略分层**：从零成本（规则）到高成本（LLM），逐级升级
- **幂等性**：相同输入状态 → 相同压缩决策（#550）
- **非破坏优先**：读模型变换策略（L1/L2/L3/L4）先于持久层策略（L5）

## 2. ContextPort 签名

```rust
trait ContextPort: Send + Sync {
    /// 构建本轮 Context Window。
    /// 内部按序执行：L2 snip → L3 microcompact → L4 context collapse
    ///              → memory 注入 → prompt 组装
    /// L1 budget reduction 在 tool 出站时已完成（不入 build_window）。
    /// L2/L3/L4 均为读模型变换——不修改 ChatChain，只影响 ContextWindow.messages。
    fn build_window(&self, req: &ContextRequest) -> ContextWindow;

    /// 判断是否需要 auto-compact（幂等：相同输入 → 相同决策）。
    fn needs_compaction(&self, req: &ContextRequest) -> CompactionDecision;

    /// L5 执行 auto-compact（LLM 摘要）。返回 summary + recent tail。
    fn compact(&self, chain: &mut ChatChain, req: &ContextRequest) -> CompactResult;
}
```

### 值对象

```rust
struct ContextRequest {
    run_id: RunId,
    messages: Vec<Message>,             // 当前扁平历史
    system_prompt: String,              // 已组装的静态 prompt
    context_size: usize,                // 模型 context window
    max_output_tokens: usize,           // 模型真实 max_tokens（修 8192 硬编码）
    last_api_input_tokens: Option<u64>, // API 上报（None=首轮/估算）
    tool_schema_tokens: usize,          // tool 定义占用
}

struct ContextWindow {
    system_blocks: Vec<SystemBlock>,    // 系统+memory+summary+reminder
    messages: Vec<Message>,             // 发给 LLM 的消息序列
    tool_schemas: Vec<Value>,           // tool 定义
    token_estimation: TokenBudget,      // 预算快照
}

struct CompactionDecision {
    needed: bool,
    urgency: Urgency,                   // None / Monitor / Should / Must
    estimated_tokens: usize,
    threshold: usize,
    reason: DecisionReason,             // ActualApi / Heuristic / Manual
}

enum Urgency {
    None,                               // < 70% effective
    Monitor,                            // 70–80%
    Should,                             // 80–90%
    Must,                               // > 90%
}

struct CompactResult {
    summary: String,
    recent_messages: Vec<Message>,
}
```

## 3. 五级管线总览

| 级别 | 策略 | 触发时机 | 成本 | 破坏性 | 可逆 | 现状 | 实现 Issue |
|---|---|---|---|---|---|---|---|
| L1 | **Budget reduction** | tool 执行完成、结果入 ChatChain 前 | 零 | 无 | 是 | ✅ 已实现 | — |
| L2 | **Snip** | `build_window` 扫描全历史 | 零 | 无（跳过 ContextWindow 中过时 content） | 是 | ❌ 未实现 | #552 |
| L3 | **Microcompact** | `build_window` 读模型变换 | 零 | 无（移除 ContextWindow 中的探索类 content） | 是 | ✅ 已实现（待迁移读模型层，见 §6.3） | #548 |
| L4 | **Context collapse** | `build_window` 投影折叠 | 零 | 无（投影层折叠） | 是 | ❌ 未实现 | #554 |
| L5 | **Auto-compact** | token 超阈值 | LLM 调用 | 有（摘要替换历史） | 否 | ✅ 已实现 | — |

### 执行序

```
ExecutingTools
  │
  ├─ 单个 tool 执行完成
  │   └─ L1 budget reduction（截断超长 tool result，在入 ChatChain 前）
  │
  ▼ PreparingContext / build_window
  │
  ├─ L2 snip（扫描全历史，标记隐藏陈旧段）
  ├─ L3 microcompact（移除 ContextWindow 中探索类 tool result content）
  ├─ L4 context collapse（投影折叠，生成压缩读模型）
  ├─ memory 注入（MemoryPort.retrieve_for_inject）
  ├─ prompt 组装（PromptPort.build_system_prompt）
  │
  ▼ ContextWindow 就绪
  │
  ├─ needs_compaction 判定
  │   ├─ 不需要 → InvokingModel
  │   └─ 需要 → L5 compact → 重建 ContextWindow → InvokingModel
  │
  ▼
```

> **L1 是唯一在 ChatChain 写入前执行的策略**。L2/L3/L4 都是 `build_window` 内部的读模型变换——不修改 ChatChain，只影响 `ContextWindow.messages`。只有 L5（auto-compact）会修改 ChatChain（创建 Compact segment）。

## 4. L1 Budget Reduction

**目标**：tool 执行完成后、结果写入 ChatChain 前，截断超长单条结果。

**触发时机**：ExecutingTools 状态下，每个 tool 执行完成时立即截断——**不等到 build_window**，在结果入 ChatChain 前就完成。

**策略**：
- 每条 tool result 有 `max_result_tokens` 上限（可配置，默认 10,000 tokens）
- 超限时截断尾部，替换为 `[truncated: original N tokens]` 标记
- 截断只作用于 tool result content，不影响 user/assistant message

**现状**：已实现。tool result 在写入 ChatChain 前经过 size 检查。

**幂等性**：对已截断的结果二次执行无效果（已短于上限）。

## 5. L2 Snip（#552 目标设计）

**目标**：历史级扫描回收——遍历整个 ChatChain，隐藏已过期的探索类内容，不限于当前 tool batch。

### 5.1 与 L3 的职责边界

| 维度 | L3 Microcompact | L2 Snip |
|---|---|---|
| 扫描范围 | 最近 N 个 segment | 整个 ChatChain |
| 触发时机 | `build_window` 时 | `build_window` 时 |
| 处理对象 | 探索类 tool result（Read/Glob/Grep） | 已被后续操作覆盖的探索结果 |
| 作用层 | 读模型层（不修改 ChatChain） | 读模型层（不修改 ChatChain） |
| 可逆性 | 是 | 是 |

**关键区别**：L3 移除保护窗口外的探索类 tool result content（因为后续不再需要）；L2 隐藏"探索后已被 Edit 覆盖"的 Read 结果（信息已过时）。两者都是读模型变换——只影响 `ContextWindow.messages`，ChatChain 原始数据不变。

### 5.2 Snip 规则

```rust
struct SnipRule {
    /// 探索类 tool 调用后，如果同一文件被 Edit/Write 修改，
    /// 该 tool result 标记为 hidden。
    /// 条件：tool = Read/Grep/Glob && 后续存在 Edit/Write 同路径
    covers: fn(tool_call: &ToolCall, later_calls: &[ToolCall]) -> bool,
}
```

- **不修改 ChatChain**：L2 在 `build_window` 时计算哪些 message 应跳过，直接在输出的 `ContextWindow.messages` 中省略——ChatChain 原始数据不变
- **保留 user/assistant 文本**：只跳过 tool result content，对应 assistant 的 tool_call 描述保留
- **跨 segment 生效**：扫描全链（已 compact 段内不操作，因为已摘要化）

### 5.3 Snip 幂等性

- 同一 ChatChain 状态 → 相同的跳过决策
- 每轮 `build_window` 重新计算，保护窗口滑动后可能展开之前跳过的 turn

## 6. L3 Microcompact

**目标**：规则驱动移除探索类工具结果 content，零 LLM 成本。读模型变换——**不修改 ChatChain**。

### 6.1 触发

- **时机**：`build_window` 内部，在 L2 snip 之后执行
- **条件**：`ContextWindow.messages` 对应的 segment 数 > 3（Main）/ > 2（Sub）

### 6.2 策略

```rust
const EXPLORATORY_TOOLS: &[&str] = &[
    "Read", "Glob", "Grep", "LS",
    // 不含 Edit/Write/Bash —— 修改类工具结果保留
];
```

- 从 `ContextWindow.messages` 中扫描，保护最近 N 个 segment（Main=3, Sub=2）
- 在保护窗口外的 segment 中，移除 `EXPLORATORY_TOOLS` 对应的 tool result content
- 替换为 `[microcompacted: N tool results removed]` 标记
- **ChatChain 中的原始 message 不受影响**——下一轮 `build_window` 重新计算

### 6.3 现状

已实现（#548, PR #568）。当前实现操作 `&mut ChatChain`——**目标态应改为操作 `ContextWindow.messages`**（读模型变换），迁移动作：
- `microcompact_chain(&mut chain, protect_last)` → `microcompact_window(&mut messages, protect_last)`
- `microcompact_messages(&mut messages, protect_last)` — 已存在，可直接复用
- 保护窗口：Main=3 segments, Sub=2 user turns

### 6.4 幂等性

- 对已移除 content 的消息二次执行无效果（EXPLORATORY_TOOLS 结果已不在 ContextWindow 中）
- 保护窗口随 segment 增长滑动——之前在保护窗口内的 segment 可能滑出窗口被移除

## 7. L4 Context Collapse（#554 目标设计）

**目标**：非破坏性投影折叠——将对话历史中的多轮交互"折叠"为压缩表示，在 build_window 时生成，不修改原始 ChatChain。

### 7.1 核心思路

Context Collapse 是**读模型变换**：ChatChain 中的原始消息不变，但 `build_window` 输出的 `ContextWindow.messages` 是折叠后的压缩表示。

```rust
struct CollapsePlan {
    /// 连续的 assistant+tool_result 序列折叠为单个 CollapseEntry
    entries: Vec<CollapseEntry>,
}

struct CollapseEntry {
    /// 折叠范围（原始 message index 区间）
    range: Range<usize>,
    /// 折叠后的压缩表示
    summary: CollapseSummary,
}

struct CollapseSummary {
    /// 一句话描述这组交互做了什么
    description: String,
    /// 关键产出（如文件路径、工具名）
    key_outputs: Vec<String>,
    /// 原始 message 数
    original_count: usize,
    /// 原始 token 估算
    original_tokens: usize,
}
```

### 7.2 折叠规则

1. **连续 tool batch 折叠**：一个 assistant turn + 其触发的所有 tool_call/tool_result 对，折叠为一个 `CollapseEntry`
2. **摘要来源**：
   - 优先复用 assistant turn 自身的文本（如果 assistant 已有总结性描述）
   - 否则从 tool_call name + args 提取关键信息（如 `Read("src/main.rs")` → `"读取了 src/main.rs"`）
3. **不折叠**：
   - user message（始终保留原文）
   - 最后 N 个 turn（保护窗口，与 microcompact 保护策略一致）
   - Compact segment 的 summary（已经是压缩态）
4. **可逆**：`CollapsePlan` 是 `build_window` 的临时产物，不写入 ChatChain。下一轮 build_window 可生成不同 plan（如保护窗口滑动后展开之前折叠的 turn）

### 7.3 折叠触发条件

```rust
fn should_collapse(req: &ContextRequest) -> bool {
    // 仅在 token 压力达到 Monitor 级别（70%+）时启用
    // 避免 token 充裕时的无谓处理
    let usage = estimate_usage(req);
    usage >= req.context_size * 70 / 100
}
```

### 7.4 与 L2/L3/L5 的关系

| 维度 | L2 Snip | L3 Microcompact | L4 Context Collapse | L5 Auto-compact |
|---|---|---|---|---|
| 修改 ChatChain | **否**（只影响 ContextWindow） | **否**（只影响 ContextWindow） | **否**（只影响 ContextWindow） | 是（创建 Compact segment） |
| 作用层 | 读模型层 | 读模型层 | 读模型层 | 持久层 |
| 可逆 | 是 | 是 | 是 | 否 |
| 信息损失 | 无（原文在 ChatChain 中） | 无（原文在 ChatChain 中） | 有（压缩为摘要） | 有（历史被摘要替换） |

**L2/L3/L4 都是读模型变换**：每轮 `build_window` 重新计算，ChatChain 原始数据始终不变。L5 是唯一修改 ChatChain 的压缩策略（创建 Compact segment 冻结旧链）。

**L4 是 L5 的前置减压层**：当 token 压力升高但还未到 auto-compact 阈值时，L4 先通过折叠释放空间，推迟 L5 触发时机。

### 7.5 实现路径

v0.1.0：**设计定稿，不实现**。实现条件：
1. L2 Snip (#552) 和 L3 Microcompact 已稳定
2. #550 幂等化完成（L4 增加 build_window 的复杂度，需要幂等基础）
3. #553 阈值优化完成（L4 影响 urgency 计算）

### 7.6 CollapseSummary 生成策略

**v0.1.0 目标设计**：规则驱动，不调 LLM。

```rust
fn generate_collapse_summary(messages: &[Message]) -> CollapseSummary {
    let tool_calls: Vec<_> = messages.iter()
        .filter_map(|m| m.tool_call.as_ref())
        .collect();

    let description = match tool_calls.as_slice() {
        [] => messages.first()
            .and_then(|m| m.content.as_str())
            .map(|s| s.chars().take(100).collect())
            .unwrap_or_default(),
        [single] => format!("{}({})", single.name, single.args_summary()),
        [first, .., last] => format!(
            "{} → ... → {}（共 {} 次工具调用）",
            first.name, last.name, tool_calls.len()
        ),
    };

    let key_outputs = tool_calls.iter()
        .filter_map(|tc| tc.args.get("file_path").or(tc.args.get("pattern")))
        .map(|v| v.as_str().to_string())
        .collect();

    CollapseSummary {
        description,
        key_outputs,
        original_count: messages.len(),
        original_tokens: estimate_messages_tokens(messages),
    }
}
```

## 8. L5 Auto-compact

**目标**：token 超阈值时，用 LLM 生成摘要替换历史。

### 8.1 触发条件

按优先级检查，任一失败即跳过：

1. **Resume 保护**：`turn_count == 1 && last_api_input_tokens == 0` → 跳过（resume 第一轮）
2. **PreCompact hook**：`result.blocked || decision == "block"` → 跳过
3. **Token 阈值**：
   - **Actual API**（`last_api_input_tokens > 0`）：`input_tokens > threshold`
   - **Heuristic fallback**（`last_api_input_tokens == 0`）：`estimated_tokens > threshold`
4. **消息数**：`messages.len() > 4`

### 8.2 阈值计算

见 [03-token-budget.md](03-token-budget.md)。核心公式：

```
effective = context_size - min(max_output_tokens, max_summary_output_tokens)
threshold = effective - autocompact_buffer_tokens
```

**关键修复**：`max_output_tokens` 从 ProviderPort 获取真实值，替代当前硬编码的 `8192`。

### 8.3 Summary 生成

```rust
fn compact(&self, chain: &mut ChatChain, req: &ContextRequest) -> CompactResult {
    // 1. 切分窗口
    let window = compact_window(&req.messages);
    // head = 前两条（system + 首条 user），tail = 最近 30%（max 4 条）

    // 2. 选择策略
    let result = if early_tokens > 30_000 {
        // 大窗口：map-reduce 分块摘要
        compact_messages_map_reduce(&window.early, req).unwrap_or_else(|_| {
            // fallback：本地文本摘要
            build_summary_text(&window.early)
        })
    } else {
        // 小窗口：单次 LLM 调用
        llm_compact(&window.early, req).unwrap_or_else(|_| {
            build_summary_text(&window.early)
        })
    };

    // 3. 修复 tail 中的 orphan tool pairs
    let recent = sanitize_tool_pairs(window.tail);

    CompactResult { summary: result.summary, recent_messages: recent }
}
```

**Map-reduce 策略**：
- `early_tokens > 30,000` 时分块（每块 ≤ 30,000 tokens）
- 每块独立 LLM 摘要 → 合并后再 LLM 摘要
- 失败时静默 fallback 到本地 `build_summary_text`——**调用方无法区分质量**（已知 gap，设计文档标注）

### 8.4 compact_window 切分

```rust
struct CompactWindow {
    head: Vec<Message>,     // 前两条（保留）
    early: Vec<Message>,    // 待摘要部分
    tail: Vec<Message>,     // 保留的近期消息
}

fn compact_window(messages: &[Message]) -> Option<CompactWindow> {
    if messages.len() <= 4 { return None; }

    let head: Vec<_> = messages[..2].to_vec();
    let tail_len = (messages.len() as f64 * 0.3) as usize;
    let tail_len = tail_len.min(4);
    let early = messages[2..messages.len() - tail_len].to_vec();
    let tail = messages[messages.len() - tail_len..].to_vec();

    Some(CompactWindow { head, early, tail })
}
```

### 8.5 Pre/PostCompact Hook

- **PreCompact**：compact 前触发。可注入 `additional_context`（追加到摘要请求）或 `system_message`（发给 UI）。可 block 阻止 compact。
- **PostCompact**：compact 后触发。可注入 `additional_context`（作为 compact 后的补充上下文）。
- **PreCompact Reflection**：compact 前抢救关键信息到 Memory（见 [05-memory-injection.md](05-memory-injection.md) §9）

### 8.6 Circuit Breaker

```rust
struct AutoCompactState {
    consecutive_failures: u32,
    max_failures: u32,                 // 默认 3
    compaction_count: u64,
}

impl AutoCompactState {
    fn should_attempt(&self) -> bool {
        self.consecutive_failures < self.max_failures
    }
    fn record_success(&mut self) { self.consecutive_failures = 0; self.compaction_count += 1; }
    fn record_failure(&mut self) { self.consecutive_failures += 1; }
}
```

**现状**：已实现但**未接入主循环**——`auto_compact` 函数从未引用 `AutoCompactState`。

**目标**：
- `auto_compact` 调用前检查 `should_attempt()`
- LLM 失败后调 `record_failure()`
- 成功后调 `record_success()`
- Circuit breaker 触发后，跳过 compact，直接进入 InvokingModel（由 provider 报 context error 再触发）

### 8.7 CompactResult → ChatChain 集成

```rust
fn apply_compact_outcome(chain: &mut ChatChain, result: CompactResult) {
    // 1. 冻结旧链
    let old_segments = chain.active_segments().to_vec();
    chain.frozen_chats.lock().unwrap().extend(old_segments);

    // 2. 创建新 Compact segment
    chain.compact(result.summary, result.recent_messages);

    // 3. 发出 CompactFinished 事件
    // events.emit(RuntimeStreamEvent::CompactFinished { messages: chain.messages_flat() });
}
```

- summary 作为 `CompactSegment.summary`（走 system 通道，不会被 future compact 二次损耗）
- recent_messages 保留在新 segment 的 `messages` 中
- 旧 segment 冻结保留供审计

### 8.8 Manual Compact

用户 `/compact` 命令触发：
- **绕过 token 阈值检查**（但保留 `messages.len() > 4` 检查）
- **当前问题**：`compact_messages_with_llm` 内部的 `needs_compaction` 会再次判断——双层检查导致语义模糊
- **目标**：manual compact 不经过 `needs_compaction`，直接调 `compact_messages_with_llm`，内层也不检查阈值

## 9. 幂等性设计（#550）

### 9.1 问题

当前 `auto_compact` 在每轮**无条件**进入 `Compact` 状态并执行：
- PreCompact hook 每轮触发（即使不需要 compact）
- microcompact 每轮扫描（即使无新 tool result）
- 这些副作用破坏"相同输入 → 相同输出"的幂等性

### 9.2 目标

```rust
struct CompactionFingerprint {
    messages_hash: u64,                // messages 内容 hash（to_llm_view 后）
    last_api_input_tokens: Option<u64>,
    context_size: usize,
    max_output_tokens: usize,
    tool_schema_count: usize,          // tool 定义变化时 fingerprint 变化
}
```

- **fingerprint 不变**时跳过 PreCompact hook 和 microcompact 扫描
- `needs_compaction` 是纯函数（已满足）
- `compact` 的效果对相同 ChatChain + 相同 ContextRequest 是确定性的

### 9.3 落地

- `CompactionFingerprint` 存储在 Run 内存态（不落盘）
- 每轮 `build_window` 后计算 fingerprint
- 下一轮进入 `PreparingContext` 时比对：相同则跳过 L2/L3 的重复扫描

## 10. 常量统一来源

当前散落的魔法常量收口到 [03-token-budget.md](03-token-budget.md) 定义的 `TokenBudgetConfig`：

| 常量 | 当前值 | 当前位置 | 目标 |
|---|---|---|---|
| `max_output_tokens` | 8192（硬编码） | `token_estimation.rs` ×3 | `ProviderPort.max_output_tokens()` |
| `max_summary_output_tokens` | 20,000 | `summary.rs` | `TokenBudgetConfig.max_summary_output_tokens` |
| `autocompact_buffer_tokens` | 13,000 | `token_estimation.rs` | `TokenBudgetConfig.autocompact_buffer_tokens` |
| `estimation_safety_factor` | 1.33 | `token_estimation.rs` | `TokenBudgetConfig.estimation_safety_factor` |

## 11. 与 #547 的映射

| #547 子 issue | 策略 | 本文档章节 | 状态 |
|---|---|---|---|
| #548 Microcompact | L3 | §6 | ✅ 已实现 |
| #546 Edit diff 分离 | L1 | §4 | ✅ 已实现 |
| #549 Memory `top_for_inject` 注入 | memory 注入 | [05-memory-injection.md](05-memory-injection.md) | ⏳ |
| #550 Tool result budget 幂等化 | 幂等性 | §9 | ⏳ |
| #551 Memory 语义检索 | memory 注入 | [05-memory-injection.md](05-memory-injection.md) §8 | ⏳ |
| #552 Snip 历史级回收 | L2 | §5 | ⏳ |
| #553 Auto-compact 阈值优化 | L5 阈值 | [03-token-budget.md](03-token-budget.md) | ⏳ |
| #671 摘要失真 | L5 summary 质量 | §8.3 | ⏳ |
| #554 Context collapse | L4 | §7 | 暂缓 |

## 12. 相关文档

- Session 聚合（ChatChain/ChatSegment）：[01-session.md](01-session.md)
- Token Budget 详解：[03-token-budget.md](03-token-budget.md)
- Memory 注入：[05-memory-injection.md](05-memory-injection.md)
- Runtime 端口：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Run 状态机（Compacting 状态）：[../runtime/03-loop-and-state-machine.md](../runtime/03-loop-and-state-machine.md)
- 上下文地图（ContextPort = OHS）：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：五级管线、ContextPort 签名、L1-L5 策略设计、幂等性、circuit breaker、常量统一 | #786 |
