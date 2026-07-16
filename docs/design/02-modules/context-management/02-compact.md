# Context Management · Compact 家族

> 层级：02-modules / context-management（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#786（S2）
> 本文定义 Compact 家族——ContextPort 的压缩管线，五级策略从零成本规则到 LLM 摘要的完整分层。ContextPort 是 Context Management BC 对 Agent Runtime 的 OHS（见 [01-session.md](01-session.md) §7）。

## 1. 定位

Compact 家族是 Context Management 的**核心能力**：在 LLM context window 耗尽前，以最小代价回收 token 预算。

- **内聚于 ContextPort**：五级管线是 ContextPort 的实现细节，Runtime 只调用 §2 的 4 个稳定方法
- **策略分层**：从零成本（规则）到高成本（LLM），逐级升级
- **幂等性**：相同 Context backing revision + 相同 request → 相同压缩决策（#550）
- **非破坏优先**：L1 先限制尚未进入 ChatChain 的单条 ToolResult；L2/L3/L4 只变换读模型；只有 L5 修改已持久化对话链

## 2. ContextPort 签名

```rust
#[async_trait]
trait ContextPort: Send + Sync {
    /// 构建本轮 Context Window。
    /// 内部按序执行：L2 snip → L3 microcompact → L4 context collapse
    ///              → prompt/skill 物化 → memory 检索 → 最终 block 编排
    /// L1 budget reduction 在 tool 出站时已完成（不入 build_window）。
    /// L2/L3/L4 均为读模型变换——不修改 ChatChain，只影响 ContextWindow.messages。
    async fn build_window(
        &self,
        req: &ContextRequest,
    ) -> Result<ContextWindow, ContextWindowError>;

    /// 在与 build_window 相同的冻结输入上计算压缩决策。
    async fn needs_compaction(
        &self,
        req: &ContextRequest,
    ) -> Result<CompactionDecision, ContextPortError>;

    /// L5 执行 auto-compact（LLM 摘要）。实现只操作自身稳定 Session backing，
    /// NEVER 向调用方暴露 ChatChain 的可变引用。
    async fn compact(
        &self,
        req: &CompactRequest,
    ) -> Result<CompactOutcome, ContextPortError>;

    /// 追加当前 finalized RunStep 产出、收集跨 BC snapshot 并原子持久化。
    async fn append_and_persist(
        &self,
        append: &ContextAppend,
    ) -> Result<AppendReceipt, ContextAppendError>;
}
```

### 值对象

```rust
struct ContextRequest {
    request_id: ContextRequestId,       // 一次 PreparingContext 冻结输入的 identity
    run_id: RunId,
    step_id: RunStepId,                 // 当前待执行 RunStep identity
    pending_messages: Vec<Message>,     // 当前 RunStep 尚未提交的增量输入；历史仍由 Context backing 独占
    system_prompt: SystemPromptSpec,    // RunSpec.system_prompt 原值；不得在 Runtime 丢失
    model_id: String,                   // PromptPipeline 的 guidance 前缀选择
    effective_reasoning: ReasoningLevel,// Provider resolver 在 build 前冻结的最终纯值
    current_date: CalendarDate,         // 本轮冻结的日期；Prompt 不自行读时钟
    task_reminder: TaskReminderSnapshot, // Task query 经 context_coordination 原样传入；空态由 PL 表达
    language: Language,
    agent_roles: HashMap<String, AgentRoleConfig>,
    config_snapshot: ConfigSnapshot,    // 本 Run shared lease 下的只读快照
    context_size: usize,                // 模型 context window
    max_output_tokens: usize,           // 与 InvocationRequest 相同的 resolved output limit
    last_total_tokens: Option<u64>,     // 上一次 Provider 响应的标准化 total；None 时不自动 compact
    tool_schemas: Vec<ModelToolSchema>, // 本轮唯一 ToolCatalogSnapshot 的稳定投影
    tool_schema_tokens: usize,          // tool 定义占用
}

impl ContextRequest {
    /// 转换为 PromptPipeline 输入。
    fn prompt_request(&self) -> PromptRequest;
    /// 转换为 Memory 检索查询。
    fn memory_query(&self) -> MemoryQuery;
}

struct ContextWindow {
    system_blocks: Vec<SystemBlock>,    // 系统+memory+summary+reminder
    messages: Vec<Message>,             // 发给 LLM 的消息序列
    tool_schemas: Vec<ModelToolSchema>, // req.tool_schemas 原样透传；Context 不重拉 Catalog
    token_estimation: TokenBudget,      // 预算快照
    compaction_decision: CompactionDecision, // build_window 内计算，替代独立 needs_compaction
}

struct ContextAppend {
    session_id: SessionId,               // 当前稳定 backing identity
    expected_revision: SessionRevision,  // append CAS 前置条件
    run_id: RunId,
    step_id: RunStepId,                  // append 幂等键的一部分
    source_request_id: ContextRequestId,
    finalize_cause: FinalizeCause,       // Completed | UserCancelledStep | RunTerminated
    messages: Vec<Message>,              // finalized projection：inputs → assistant → 原序 terminal results
    receipts: Vec<StepReceipt>,          // deterministic Tool/Agent receipt；可含 CancellationUnconfirmed
    usage: Option<UsageSnapshot>,         // Provider ACL 标准化后的 usage；Context 不解释 provider 原始字段
    fingerprint: ContentFingerprint,     // 相同幂等键的内容一致性校验
}

struct CompactRequest {
    run_id: RunId,
    source: ContextRequest,             // 与 build_window 内的 compaction_decision 计算使用同一冻结输入
    trigger: CompactTrigger,            // Automatic | Manual
}

struct CompactionDecision {
    needed: bool,
    urgency: Urgency,                   // None / Monitor / Should / Must
    estimated_tokens: usize,
    threshold: usize,
    reason: DecisionReason,             // ActualProviderUsage / NoActualUsage / Manual
}

enum DecisionReason {
    ActualProviderUsage,                // 基于上一次 Provider 标准化 total_tokens
    NoActualUsage,                      // 首轮 / compact 后尚无新 Provider usage
    Manual,                             // 仅 manual compact 路径独立构造 Decision 时使用；
                                        // compaction_decision 计算永远不会产出此值
}

enum Urgency {
    None,                               // < 70% effective
    Monitor,                            // 70–80%
    Should,                             // 80–90%
    Must,                               // > 90%
}

struct CompactResult {
    summary: String,
    recent_runs: Vec<CommittedRunSlice>, // recent tail 保留 Run/Step 结构
    source_revision: SessionRevision,  // compact 基于的 backing revision（幂等键 + CAS 校验值）
}

/// compact 调用的完整 outcome——Runtime 据此区分"已提交"与"被跳过"。
enum CompactOutcome {
    Committed(CompactResult),       // compact 已提交
    Skipped(CompactSkipReason),     // compact 被跳过（Runtime 无需 continue 重试）
    Failed(CompactError),           // compact 失败
}

enum CompactSkipReason {
    ResumeProtection,               // resume 第一轮保护
    HookBlocked,                    // PreCompact hook 阻止
    CircuitBreakerOpen,             // 连续失败次数达上限
}

struct AppendReceipt {
    run_id: RunId,
    step_id: RunStepId,
    committed_revision: SessionRevision,
    fingerprint: ContentFingerprint,
}

struct CalendarDate(String);             // ISO-8601 calendar date；一次 build 内冻结
```

`ContextRequest` 只承载一次 window build 的不可变输入。Runtime 的 `context_coordination` 从 `TaskAccess::reminder_snapshot` 读取 Task-owned PL 后原样传入；Context Management 独占最终文本、位置与 token budget。PromptPipeline **NEVER** 读取 Task，Context Management 也 **NEVER** 因 reminder 获得 Task mutation / restore authority。`CalendarDate` 由 Runtime request builder 从注入的时钟取得并在本次 build 冻结，Prompt capability **NEVER** 读取进程全局时钟。

Runtime **NEVER** 把 Session 历史塞回 request：Context implementation 从自身稳定 backing 读取已提交历史，再在本次 candidate 尾部拼接 `pending_messages`。每个 finalized RunStep 恰好调用一次 `append_and_persist`；finalized projection 由 Runtime 唯一 `StepFinalizer` 在 `Completed | UserCancelledStep | RunTerminated` 三种原因下生成。实现以 `(run_id, step_id)` 幂等，重复相同 append 返回成功，内容冲突的重复键返回 typed error。

普通完成路径必须在 model response 与全部 Tool suspension/approval 收敛为 final result 后提交。控制路径可提交 finalizer 明确冻结的 partial assistant 与 deterministic Tool/Agent receipts，并为 deadline 内未确认停止的工作保存 `CancellationUnconfirmed`；这类内容已是协议完整的 finalized partial，而不是 Run checkpoint。`ContextAppend` **NEVER** 携带 RunStatus、RunStepStatus、活跃 future、Sub 完整消息链或 cancellation scope。

`ContextRequest → PromptRequest` 的映射是 Context-owned 纯函数，字段不得旁路重取：

| ContextRequest | PromptRequest |
|---|---|
| `system_prompt` | `system_prompt` |
| `model_id` | `model_id` |
| `effective_reasoning` | `effective_reasoning` |
| `current_date` | `current_date` |
| `language` / `agent_roles` / `config_snapshot` | `lang` / `agents_roles` / `config_snapshot` |

`PromptRequest.project_root / git_context` 不由 Runtime 伪造：run-bound Context implementation 在 `build_window` 开始时从 Composition 注入的同一 Project-owned read view 读取一次 snapshot，经 Context ACL 映射后同时填入两个字段；同一次 build **NEVER** 重探测。这样 `RuntimeContext` 仍不获得 Workspace / Project 能力。

Tool schema 也只有一条数据流：`ToolCatalogSnapshot` → Runtime 稳定投影 → `ContextRequest.tool_schemas` → `ContextWindow.tool_schemas` → `InvocationRequest.window`。Context / Provider **NEVER** 重新查询 Catalog、重算 Profile 或改变顺序。

### 2.1 最终 system block 顺序（唯一真相）

无论各 supplier 的 I/O 实现如何，`build_window` 的可观察物化顺序固定为 **Prompt（含 Guidance + Skill）→ Memory → active summary → final assembly**；失败按该顺序返回第一个 typed error。最终 blocks 的位置则固定如下，物化先后与 placement **NEVER** 混为一谈：

```text
cacheable_prefix:
  1 system_prompt          2 execution_discipline  3 model_guidance
  4 skills                5 agent_roles           6 user_guidance
  7 memory_context        8 active_summary
cache breakpoint
uncached_suffix:
  9 current_date         10 git_context           11 task_reminder
```

## 3. 五级管线总览

| 级别 | 策略 | 触发时机 | 成本 | 破坏性 | 可逆 | 关联 |
|---|---|---|---|---|---|---|
| L1 | **Budget reduction** | tool 执行完成、结果入 ChatChain 前 | 零 | 有（超限尾部不进入 ChatChain） | 否 | Context baseline |
| L2 | **Snip** | `build_window` 扫描全历史 | 零 | 无（跳过 ContextWindow 中过时 content） | 是 | #552 |
| L3 | **Microcompact** | `build_window` 读模型变换 | 零 | 无（移除 ContextWindow 中的探索类 content） | 是 | #548 |
| L4 | **Context collapse** | `build_window` 投影折叠 | 零 | 无（投影层折叠） | 是 | #554 |
| L5 | **Auto-compact** | token 超阈值 | LLM 调用 | 有（摘要替换历史） | 否 | Context baseline / #671 |

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
  ├─ await prompt 组装（PromptPipeline.build_system_prompt，含 Skill 物化）
  ├─ memory 注入（MemoryPort.retrieve_for_inject）
  ├─ active summary 读取
  ├─ 按 §2.1 唯一顺序编排 blocks，并原样携带 tool_schemas
  │
  ▼ ContextWindow 就绪（含 compaction_decision）
  │
  ├─ window.compaction_decision.needed 判定
  │   ├─ false → InvokingModel
  │   └─ true  → L5 compact → 重建 ContextWindow → InvokingModel
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

**幂等性**：对已截断的结果二次执行无效果（已短于上限）。

## 5. L2 Snip（#552）

**目标**：历史级扫描回收——遍历整个 ChatChain，隐藏已过期的探索类内容，不限于当前 tool batch。

### 5.1 与 L3 的职责边界

| 维度 | L3 Microcompact | L2 Snip |
|---|---|---|
| 扫描范围 | 最近 3 个完整 Run 之前 | 整个 ChatChain |
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
- **Run 边界**：保护窗口按 `CommittedRunSlice.run_id` 计算，Main / Sub 统一保护最近 3 个完整 Run；同一 Run 内新增 RunStep **NEVER** 推动保护窗口

### 5.3 Snip 幂等性

- 同一 ChatChain 状态 → 相同的跳过决策
- 每轮 `build_window` 重新计算，保护窗口滑动后可能展开之前跳过的 turn

## 6. L3 Microcompact

**目标**：规则驱动移除探索类工具结果 content，零 LLM 成本。读模型变换——**不修改 ChatChain**。

### 6.1 触发

- **时机**：`build_window` 内部，在 L2 snip 之后执行
- **条件**：结构化 history 中完整 Run 数 > 3；Main / Sub 统一

### 6.2 策略

```rust
const EXPLORATORY_TOOLS: &[&str] = &[
    "Read", "Glob", "Grep", "LS",
    // 不含 Edit/Write/Bash —— 修改类工具结果保留
];
```

- 从结构化 history 中扫描，保护最近 3 个完整 Run
- 在保护窗口外的 Run 中，移除 `EXPLORATORY_TOOLS` 对应的 tool result content
- 替换为 `[microcompacted: N tool results removed]` 标记
- **ChatChain 中的原始 message 不受影响**——下一轮 `build_window` 重新计算

### 6.3 读模型约束

- `microcompact_window(&mut structured_history, protect_last_runs=3)` **MUST** 只操作本次结构化 candidate。
- L3 **NEVER** 接收 `&mut ChatChain`，也 **NEVER** 通过另一条 helper 回写 Session backing。
- Main / Sub 的保护窗口统一为最近 3 个 Run。Run 内无论产生多少 Model Invocation、Tool batch 或 RunStep，均只算一个 Run。
- Run / Step 边界 **MUST** 来自 Context backing 保存的 identity，**NEVER** 通过 `Role::User`、ToolResult 或 message 顺序反推。

### 6.4 幂等性

- 对已移除 content 的消息二次执行无效果（EXPLORATORY_TOOLS 结果已不在 ContextWindow 中）
- 保护窗口随完整 Run 增长滑动——之前在保护窗口内的 Run 可能滑出窗口被修剪

## 7. L4 Context Collapse（#554）

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

1. **Provider usage 存在**：`last_total_tokens` 为 `Some`；首轮、resume 首轮和 compact 后尚未产生新 Provider 响应时均跳过自动 compact
2. **Token 阈值**：`last_total_tokens > threshold`
3. **PreCompact hook**：`result.blocked || decision == "block"` → 跳过
4. **可压缩历史存在**：至少一个 finalized RunStep 可进入 summary 或 recent tail

`last_total_tokens` 是上一次 Provider 响应经 Provider ACL 标准化后的单次
context usage，不是 Session 累计成本。Anthropic 必须包含 cache read / cache
creation input；完整规则见 [03-token-budget.md](03-token-budget.md)。

**compact 后 usage 重置**：compact 成功后把 `last_total_tokens` 清为 `None`。
只有下一次 Provider 调用成功返回新的 usage 后，自动 compact 才可再次触发。
因此一个长 Run 可以在不同 RunStep 后多次 compact，但 **NEVER** 在没有新
Provider usage 的情况下围绕同一个旧值重复进入 `Compacting`。目标态不再使用
“每 Run 最多 compact 一次”的粗粒度冷却。

### 8.2 阈值计算

见 [03-token-budget.md](03-token-budget.md)。核心公式：

```
summary_budget = context_size * 2%
effective = context_size - min(max_output_tokens, summary_budget)
threshold = (effective - autocompact_buffer_tokens) * 0.8
```

`summary_budget` 按 context window 比例动态缩放（如 100K context → 2000 token budget；272K → 5440 token），**NEVER** 写死常量。`max_output_tokens` **MUST** 使用本 Run 的 Config / Provider capability 已解析真实值，**NEVER** 使用固定 `8192`。

### 8.3 Summary 生成

```rust
async fn compact(&self, req: &CompactRequest) -> Result<CompactResult, CompactError> {
    // 1. 从自身稳定 Session backing 取得一致性快照并切分窗口
    let source = self.session.compaction_source()?;
    let window = compact_window(&source.structured_history, req.source.context_size);
    // early = 进入 summary 的完整 RunStep；tail = 不超过 window 30% 的近期完整 RunStep

    // 2. 选择策略
    let result = if early_tokens > 30_000 {
        // 大窗口：map-reduce 分块摘要
        compact_messages_map_reduce(&window.early, req).await?
    } else {
        // 小窗口：单次 LLM 调用
        llm_compact(&window.early, req).await?
    };

    // 3. tail 已按完整 Step 切分，不需要按 message 猜测 / 修复边界
    let recent = window.tail;

    // 4. CAS 校验：确认 backing revision 未变（compact 跨多个 LLM await，期间可能有并发写入）
    let current_revision = self.session.backing_revision();
    if current_revision != source.revision {
        return Err(CompactError::BackingChanged {
            expected: source.revision,
            actual: current_revision,
        });
    }

    // 5. ChatChain::compact 一次性提交（三参数版：summary, recent_runs, source_revision）
    //    内部完成 freeze_active → 创建 Compact segment → 记录 source_revision
    //    定义见 01-session.md §3.1
    self.session.compact(result.summary.clone(), recent.clone(), source.revision);

    Ok(CompactResult {
        summary: result.summary,
        recent_runs: recent,
        source_revision: source.revision,
    })
}
```

**Map-reduce 策略**：
- `early_tokens > 30,000` 时分块（每块 ≤ 30,000 tokens）
- 每块独立 LLM 摘要 → 合并后再 LLM 摘要
- LLM 摘要失败返回结构化 `CompactError`；若产品选择本地降级，结果 **MUST** 带显式 quality / fallback 标记，**NEVER** 静默伪装成 LLM 摘要成功。

**Summary 保真度不变量**：

- `early` **MUST** 覆盖所有将从 active messages 移除、且未进入 recent tail 的消息；**NEVER** 存在既不保留、也不进入 summary 的 head gap。
- Summary **MUST** 按时间顺序汇总影响当前工作的全部用户输入；相邻输入 **MAY** 合并表达，但后续修正 **MUST** 覆盖更早的冲突要求。
- Summary **MUST** 精确保留用户要求的动作层级，**NEVER** 把 inspect / diagnose / explain / review / design 升级为 implement / edit / commit / push / merge。
- Summary **MUST** 分开记录 `User Requests`、`Work Completed`、`Problems / Findings`、`Current State` 与单一 `Next Action`，并区分已确认事实、推断与未知项。
- Summary **MUST** 输出 `Continue | Waiting for User | Completed` 三态 continuation。`Continue` 表示下一轮模型直接执行 `Next Action`，不等待新用户输入；`Waiting for User` 只用于确实缺少批准、选择、输入或新权限；`Completed` 只用于用户请求已交付且没有剩余工作。
- Recent tail 的切分位置与 summary 覆盖范围是两个独立概念：调整 summary 输入 **NEVER** 隐式改变 tail 的预算、Run/Step 边界或 `split_point`。

### 8.4 compact_window 切分

```rust
struct CompactWindow {
    early: Vec<CommittedRunStep>,       // 待摘要部分
    tail: Vec<CommittedRunSlice>,       // 保留 Run/Step 结构的近期后缀
    tail_tokens: usize,
    tail_budget: usize,
}

fn compact_window(
    runs: &[CommittedRunSlice],
    context_size: usize,
) -> Option<CompactWindow> {
    let tail_budget = context_size * 30 / 100;
    let ordered_steps = runs.iter().flat_map(|run| run.steps.iter());

    // 从最新 Step 向前保留完整 Step；一旦加入更远 Step 会超预算，
    // 该 Step 及其之前的历史全部进入 early summary。
    let tail = take_newest_complete_steps_within_budget(ordered_steps, tail_budget);
    let early = steps_before_tail(runs, &tail);

    (!early.is_empty()).then(|| CompactWindow {
        tail_tokens: estimate_step_tokens(&tail),
        early,
        tail,
        tail_budget,
    })
}
```

recent tail 预算只估算 tail 自身 messages，**排除** compact summary、system
prompt、memory 和 tool schemas。选择单位是完整 finalized RunStep：

- 超过 `context_size * 30%` 时，从最远的 Step 开始逐个移出 tail，直到预算内；
- Step 内的 assistant ToolUse 与对应 ToolResult **NEVER** 被拆开；
- 不保留独立“前两条 head”；更早的目标、决策和初始输入统一由 summary 保存；
- 单个 Step 已超过预算时，该 Step 进入 early summary；L1 budget reduction 必须限制新 ToolResult，map-reduce 负责处理超大 early 输入；
- compact 提交与 Provider 出站前，才把 tail 按 Run / Step 顺序扁平化为 messages。

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

- `auto_compact` 调用前检查 `should_attempt()`
- LLM 失败后调 `record_failure()`
- 成功后调 `record_success()`
- Circuit breaker 触发后，跳过 compact，直接进入 InvokingModel（由 provider 报 context error 再触发）

### 8.7 Compact 提交协议（统一入口）

Compact 提交由 `ChatChain::compact(summary, recent_runs, source_revision)` 一次性完成（三参数版，定义见 [01-session.md](01-session.md) §3.1）：

```rust
// ChatChain 唯一提交入口——不再有 apply_compact_outcome 或 commit_compaction 独立函数
chain.compact(result.summary, result.recent_runs, source.revision);
// 内部等价于：freeze_active() → 创建 Compact segment → 记录 source_revision
// 幂等保护：若 compact_source_revision + compact_committed marker 匹配则跳过（见 03-token-budget.md §5.5）
```

- summary 作为 `CompactSegment.summary`（走 system 通道，不会被 future compact 二次损耗）
- recent_runs 保留在新 Compact segment 的结构化 `runs/steps` 中
- 旧 segment 冻结保留供审计
- `ChatChain::compact` 是唯一提交入口——`apply_compact_outcome`、`commit_compaction` 等独立函数皆已退役

### 8.8 Manual Compact

用户 `/compact` 命令触发：
- **绕过 token 阈值检查**，但必须存在至少一个可进入 summary 的 finalized RunStep
- manual compact 不经过 `compaction_decision` 判定，直接进入 compact use case；内部 **NEVER** 重复检查自动阈值

### 8.9 当前最小战术修复（不实现 Step schema）

当前生产 `ChatChain` 只保留 Run/segment 边界，并在 compact 前调用
`messages_flat()`；因此现状**无法正确按 RunStep 裁 recent tail**。在
`CommittedRunStep` backing 落地前，允许以下最小安全修复：

1. Provider ACL 先产出标准化 `last_total_tokens`，Runtime 仅以
   `last_total_tokens > threshold` 进入 `Compacting`，成功后清空该值；
2. 把现有 segment-aware Microcompact 从 `compact()` 内移到每次
   `PreparingContext` 的常驻投影阶段；保护最近 3 个 Run。Snip 复用同一
   Run traversal，但保持自身“被后续 Edit/Write 覆盖”的规则；
3. 删除 auto compact 各层重复阈值判断，`Compacting` 内只执行一次管线；
4. recent tail 暂时按完整 `ChatSegment` / Run 从最远端移除到 30% 以内，
   **NEVER** 根据 message role 或 ToolUse/ToolResult 猜 Step；
5. 该降级 MAY 多移除一些历史，但 summary 覆盖被移除的完整 Run，且不会破坏
   ToolUse/ToolResult 配对。Step-aware tail 必须等待 Session schema 保存
   `RunId → RunStepId → messages` 后再实现。

## 9. 幂等性设计（#550）

### 9.1 Fingerprint 契约

字段、构造与缓存范围的唯一真相见 [Token Budget](03-token-budget.md) §5。本文只定义 Compact 对该契约的使用规则，**NEVER** 复制类型字段。

- **fingerprint 不变**时跳过 PreCompact hook 和 microcompact 扫描
- `compaction_decision` 计算对相同 backing revision + request 是确定性函数
- `compact` 的效果对相同 ChatChain + 相同 ContextRequest 是确定性的

### 9.2 生命周期

- `CompactionFingerprint` 存储在 Run 内存态（不落盘）
- 每轮 `build_window` 从纯 compact 输入计算 fingerprint
- 下一轮进入 `PreparingContext` 时比对：相同则跳过 L2/L3 的重复扫描
- fingerprint 命中只复用 L2-L4 投影，**NEVER** 跳过 Prompt / Skill / Memory 物化或复用整个 ContextWindow

## 10. 常量统一来源

全部常量只由 [03-token-budget.md](03-token-budget.md) 定义的 `TokenBudgetConfig` 或本 Run 已解析 capability 提供：

| 常量 | 默认值 / 来源 | 唯一所有者 |
|---|---|---|
| `max_output_tokens` | 本 Run 的 model capability / ConfigSnapshot | Invocation / ContextRequest |
| `summary_budget` | `context_size * 2%`（动态计算） | `token_budget::summary_budget(context_size)` |
| `autocompact_buffer_tokens` | 13,000 | `TokenBudgetConfig.autocompact_buffer_tokens` |
| `estimation_safety_factor` | 1.33 | `TokenBudgetConfig.estimation_safety_factor` |

## 11. 与 #547 的映射

| #547 子 issue | 策略 | 目标契约位置 |
|---|---|---|
| #548 Microcompact | L3 | §6 |
| #546 Edit diff 分离 | L1 | §4 |
| #549 Memory injection | memory integration | [05-memory-injection.md](05-memory-injection.md) |
| #550 Tool result budget 幂等化 | 幂等性 | §9 |
| #551 Memory 语义检索 | Memory-owned retrieval | [../memory/02-retrieval-and-injection.md](../memory/02-retrieval-and-injection.md) |
| #552 Snip 历史级回收 | L2 | §5 |
| #553 Auto-compact 阈值优化 | L5 阈值 | [03-token-budget.md](03-token-budget.md) |
| #671 摘要失真 | L5 summary 质量 | §8.3 |
| #554 Context collapse | L4 | §7 |

## 12. 相关文档

- Session 聚合（ChatChain/ChatSegment）：[01-session.md](01-session.md)
- Token Budget 详解：[03-token-budget.md](03-token-budget.md)
- Memory 注入：[05-memory-injection.md](05-memory-injection.md)
- Runtime 端口：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Run 状态机（Compacting 状态）：[../runtime/03-loop-and-state-machine.md](../runtime/03-loop-and-state-machine.md)
- 上下文地图（ContextPort = OHS）：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)
- Current → Target 迁移责任：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：五级管线、ContextPort 签名、L1-L5 策略设计、幂等性、circuit breaker、常量统一 | #786 |
| 2026-07-15 | #868 实现回写：ContextPort 冻结四方法与 provider-neutral PL；append 使用 revision/fingerprint CAS 并返回 typed receipt，Runtime 只消费 Context-owned 契约 | [#868](https://github.com/rushsinging/aemeath/issues/868) |
| 2026-07-16 | compact 战术修改：Run 级冷却（每 Run 最多 compact 一次，防死循环）；compact 后重置 token 计数；recent tail 10%（从 30% 调低）；recent tail ToolResult 全部占位符替换；summary_budget 动态计算（context_size * 2%） | #1110 |
| 2026-07-17 | Snip/Microcompact 统一保护最近 3 个 Run；自动触发改为 Provider 标准化 last_total_tokens；recent tail 按完整 RunStep 且上限为 context window 30%；记录无 Step schema 时按完整 Run 降级的最小修复 | compact token reset design |
| 2026-07-17 | 补充 L5 summary 保真度：所有被移除消息必须进入 summary；按序汇总用户输入且后续修正覆盖前述冲突要求；禁止动作层级升级；增加 continuation 三态 | [#671](https://github.com/rushsinging/aemeath/issues/671) |
