# Context Management · Compact 家族
> 层级：02-modules / context-management（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#786（S2）
> 本文定义 Compact 家族——ContextPort 的压缩管线，五级策略从零成本规则到 LLM 摘要的完整分层。ContextPort 是 Context Management BC 对 Agent Runtime 的 OHS（见 [01-session.md](01-session.md) §7）。
## 1. 定位
Compact 家族是 Context Management 的**核心能力**：在 LLM context window 耗尽前，以最小代价回收 token 预算。
- **内聚于 ContextPort**：五级管线是 ContextPort 的实现细节，Runtime 只调用 §2 的 4 个稳定方法
- **策略分层**：从零成本（规则）到高成本（LLM），逐级升级
- **幂等性**：相同 Context backing revision + 相同 request → 相同压缩决策（#550）
- **非破坏优先**：L1 先限制尚未进入 `run_slices` 的单条 ToolResult；L2/L3/L4 只变换读模型；只有 L5 修改已持久化 Session backing。
## 1.1 #1282 Session backing cutover

L5 的持久化真相是 `run_slices + ActiveCompactMarker`，不是 `ChatChain` / `ChatSegment`：marker 保存累计 summary、首个可见完整 Step 与 source revision；完整历史只保留一份 `run_slices`。每次 compact 以完整 Step 为边界更新同一个 marker，**NEVER** 复制扁平 recent tail；新 accepted/finalized Step 只写 `run_slices`，自然位于 marker 后并在下一次 ContextWindow 可见。compact 读取每个 `FinalizedOutcomeProjection.messages` 作为模型历史，不把 receipts、usage、fingerprint 或 revision 扁平为消息；这些 metadata 继续与 Run/Step identity 一起留在 structured backing。本文其余出现的 ChatChain/Compact segment 描述均为 pre-#1282 迁移背景，不得作为新 writer 的实现依据。

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
    language: Language,
    agent_roles: HashMap<String, AgentRoleConfig>,
    config_snapshot: ConfigSnapshot,    // 本 Run shared lease 下的只读快照
    context_size: usize,                // 模型 context window
    max_output_tokens: usize,           // 与 InvocationRequest 相同的 resolved output limit
    last_api_total_tokens: Option<u64>, // 上一次 Provider 响应的标准化 total；None 时回退本轮启发式估算
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
    backing_revision: SessionRevision, // 本 window 读取的稳定 backing revision，供 append CAS
    system_blocks: Vec<SystemBlock>,    // 稳定系统+memory+summary；全部位于 cacheable prefix
    messages: Vec<Message>,             // canonical 原文窗口；包含已提交的普通 tool results
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
    source_revision: SessionRevision,   // 与 build_window 使用同一冻结 revision
    source: ContextRequest,
    trigger: CompactTrigger,            // Automatic | Manual
    cancellation: CancellationToken,    // Run cancellation 的合作式传播；取消后不得 fallback/commit
}
struct CompactionDecision {
    needed: bool,
    urgency: Urgency,                   // None / Monitor / Should / Must
    decision_token_count: usize,        // 本次 decision 实际采用的 token 数值
    threshold: usize,
    reason: DecisionReason,             // ActualProviderUsage / HeuristicFallback / Manual
}
enum DecisionReason {
    ActualProviderUsage,                // 直接采用上一次 Provider 标准化 total_tokens
    HeuristicFallback,                  // 首轮、resume、切换模型或 compact 后缺少可比 API baseline
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
    HookBlocked,                    // PreCompact Hook 阻断
    Cancelled,                      // Run 已取消；不得提交生成结果或本地 fallback
    CircuitBreakerOpen,             // 自动 compact 连续失败次数达配置上限
}
struct AppendReceipt {
    run_id: RunId,
    step_id: RunStepId,
    committed_revision: SessionRevision,
    fingerprint: ContentFingerprint,
}
```
`ContextRequest` 只承载一次 window build 的不可变输入。Task 进度通过 `TaskUpdate(status)` 的普通 tool result 进入 canonical history；Runtime/Context **NEVER** 额外查询 Task、构造 reminder 或装饰 Provider user message。PromptPipeline **NEVER** 读取 Task，Context Management 也不获得 Task mutation / restore authority。日期、工作区变化与 commit guidance 不进入 `ContextRequest` 或 LLM 上下文。
Runtime **NEVER** 把 Session 历史塞回 request：Context implementation 从自身稳定 backing 读取已提交历史，再在本次 candidate 尾部拼接 `pending_messages`。每个 finalized RunStep 恰好调用一次 `append_and_persist`；finalized projection 由 Runtime 唯一 `StepFinalizer` 在 `Completed | UserCancelledStep | RunTerminated` 三种原因下生成。实现以 `(run_id, step_id)` 幂等，重复相同 append 返回成功，内容冲突的重复键返回 typed error。
普通完成路径必须在 model response 与全部 Tool suspension/approval 收敛为 final result 后提交。控制路径可提交 finalizer 明确冻结的 partial assistant 与 deterministic Tool/Agent receipts，并为 deadline 内未确认停止的工作保存 `CancellationUnconfirmed`；这类内容已是协议完整的 finalized partial，而不是 Run checkpoint。`ContextAppend` **NEVER** 携带 RunStatus、RunStepStatus、活跃 future、Sub 完整消息链或 cancellation scope。
`ContextRequest → PromptRequest` 的映射是 Context-owned 纯函数，字段不得旁路重取：
| ContextRequest | PromptRequest |
|---|---|
| `system_prompt` | `system_prompt` |
| `model_id` | `model_id` |
| `effective_reasoning` | `effective_reasoning` |
| `language` / `agent_roles` / `config_snapshot` | `lang` / `agents_roles` / `config_snapshot` |
Git 首次快照不由 `ContextRequest` 或 `PromptRequest` 承载：Runtime session bootstrap 只采集一次并以普通系统生成消息投递首个 Run；后续状态由模型通过工具主动获取。
Tool schema 也只有一条数据流：`ToolCatalogSnapshot` → Runtime 稳定投影 → `ContextRequest.tool_schemas` → `ContextWindow.tool_schemas` → `InvocationRequest.window`。Context / Provider **NEVER** 重新查询 Catalog、重算 Profile 或改变顺序。
### 2.1 最终 system block 顺序（唯一真相）
无论各 supplier 的 I/O 实现如何，`build_window` 的可观察物化顺序固定为 **Prompt（含 Guidance + Skill）→ Memory → active summary → final assembly**；失败按该顺序返回第一个 typed error。最终 blocks 的位置则固定如下，物化先后与 placement **NEVER** 混为一谈：
```text
cacheable_prefix:
  1 system_prompt          2 execution_discipline  3 model_guidance
  4 skills                5 agent_roles           6 user_guidance
  7 memory_context        8 active_summary
cache breakpoint
ordinary messages:
  TaskUpdate(status) tool result → 按事件携带 Task 原子进度摘要
```
Git 首次快照不属于 `ContextWindow.system_blocks`：Runtime 仅在 session 首个 Run 作为普通系统生成消息投递一次。
## 3. 五级管线总览
| 级别 | 策略 | 触发时机 | 成本 | 破坏性 | 可逆 | 关联 |
|---|---|---|---|---|---|---|
| L1 | **Budget reduction** | tool 执行完成、结果入 ChatChain 前 | 零 | 有（超限尾部不进入 ChatChain） | 否 | Context baseline |
| L2 | **Snip** | `build_window` 扫描全历史 | 零 | 无（跳过 ContextWindow 中过时 content） | 是 | #552 |
| L3 | **Microcompact** | `build_window` 读模型变换 | 零 | 无（移除 ContextWindow 中的探索类 content） | 是 | #548 |
| L4 | **Context collapse** | `build_window` 投影折叠 | 零 | 无（投影层折叠） | 是 | #554 |
| L5 | **Auto-compact** | token 超阈值 | LLM 调用 | 有（摘要替换历史） | 否 | Context baseline / #671 |
`ActiveCompactMarker` 是唯一 L5 持久化边界：保存累计 summary、`start_at`（第一个可见完整 RunStep）和 source revision。完整 `run_slices` 不复制 tail；ContextWindow 从 marker 起投影，后续新 Step 继续追加到同一 `run_slices`，因此自动可见。
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
1. **计算 decision token count**：若存在与当前 session/model/compact generation 可比的 `last_api_total_tokens`，直接采用该标准化 total；否则使用本轮完整 candidate 的启发式估算
2. **Token 阈值**：`decision_token_count > threshold`
3. **PreCompact Hook（Future）**：当前生产 compact 管线尚未 emit `HookInvocation::PreCompact`，因此本项不参与 Current decision；接线后 `Block` 才能跳过 compact
4. **可压缩历史存在**：至少一个 finalized RunStep 可进入 summary 或 recent tail
`last_api_total_tokens` 是上一次 Provider 响应经 Provider ACL 标准化后的单次
context usage，不是 Session 累计成本。Anthropic 必须包含 cache read / cache
creation input；完整规则见 [03-token-budget.md](03-token-budget.md)。
**baseline 失效**：compact 成功、manual compact 成功、session resume 或模型切换后，
Runtime 必须把 `last_api_total_tokens` 清为 `None`；下一次 decision 回退本轮完整
candidate 启发式估算，直到当前模型成功返回新的标准化 usage。由此既不复用不可比的
旧 API 数值，也不会因缺少 usage 而禁用自动 compact。
### 8.2 阈值计算
见 [03-token-budget.md](03-token-budget.md)。核心公式：
```
reserved_context = context_size * 2%
effective = context_size - reserved_context - max_output_tokens
threshold = effective * 0.8
```
`reserved_context` 为 guidance 与 compact summary 预留，按 context window 比例动态缩放（如 100K context → 2000；272K → 5440），**NEVER** 写死常量。它与 `max_output_tokens` 是两个独立预算，必须同时从 context window 扣除，**NEVER** 取 `min`。0.8 safety ratio 已承担提前触发缓冲，因此不再叠加固定 `13_000`。`max_output_tokens` **MUST** 使用本 Run 的 Config / Provider capability 已解析真实值，**NEVER** 使用固定 `8192`。
### 8.3 Summary 生成
L5 的 Target 摘要生成已经演进为**持久化增量摘要树**：平时在
`append_and_persist` 后按 finalized RunStep 增量构建 Leaf / Branch，compact
时优先本地激活 warm projection。领域模型、16K / 24K 分块、recent 3 Run
 保护区、per-session 1 / global 5 scheduler、checkpoint 恢复、第一次与第二次
 compact 生命周期以及 usage 总账的唯一真相见
 [06-persistent-summary-tree.md](06-persistent-summary-tree.md)。这些仍属独立 Target；当前 L5 **NEVER** 引入 scheduler、第二 backing 或并行路径。
下面的同步单次 / map-reduce 流程描述当前唯一 L5 管线；持久化增量摘要树与
scheduler 不在本路径内，**NEVER** 作为第二套 compact backing 或并行 L5 路径接入：
```rust
async fn compact(&self, req: &CompactRequest) -> Result<CompactResult, CompactError> {
    // 1. 短暂持有 Session mutation gate，冻结 revision、visible steps、messages 与 previous summary
    let source = self.freeze_compact_source(&req.source.session_id, req.source_revision).await?;
    // 2. 释放 mutation gate 后才执行全部 CompactGenerator await；Context 不依赖具体 Provider/DTO
    let result = self.generate_compact(&source, req).await?;
    // 3. 取消在 fallback 前和 durable commit 前均重新检查；取消不得产生 fallback 或 commit
    req.cancellation.ensure_not_cancelled()?;
    // 4. 仅为 revision/CAS 校验与 durable commit/publish 重新取得 mutation gate
    //    revision 已变化时返回 typed CAS conflict，绝不覆盖 freeze 后新增的 Session history
    self.commit_generated_compact(&req.source.session_id, &source, result).await
}
```
**Typed map / Rust-owned reduce 策略**：
- `early_tokens > 30,000` 时分块（每块 ≤ 30,000 tokens）。
- 每块 LLM 只输出 `CompactFactBatch` typed JSON；Context 按 chunk index 与 fact sequence 合并 facts，并由 `reduce_compact_facts` 确定性构造权威 `ContinuationCheckpoint`。
- checkpoint 超预算时，LLM Refresh 只输出 `CheckpointCompressionPatch` typed JSON，且只能改写 committed facts、working set、risks、resume context、required revalidation 与 archived milestones；Context 应用 patch 并保留全部 protected semantics。
- LLM 摘要失败返回结构化 `CompactError`；若产品选择本地降级，结果 **MUST** 带显式 quality / fallback 标记，**NEVER** 静默伪装成 LLM 摘要成功。
- 同步路径保持 bounded map 并发；**NEVER** 引入 persistent-summary-tree scheduler、第二 compact backing 或并行 L5 路径。
**Summary 保真度不变量**：
- `early` **MUST** 覆盖所有将从 active messages 移除、且未进入 recent tail 的消息；**NEVER** 存在既不保留、也不进入 summary 的 head gap。
- Summary **MUST** 按时间顺序汇总影响当前工作的全部用户输入；相邻输入 **MAY** 合并表达，但后续修正 **MUST** 覆盖更早的冲突要求。
- Summary **MUST** 精确保留用户要求的动作层级，**NEVER** 把 inspect / diagnose / explain / review / design 升级为 implement / edit / commit / push / merge。
- Summary **MUST** 使用固定顺序的九分区 continuation checkpoint：`Immutable Constraints`、`Current Objective`、`Committed Facts`、`Uncommitted Working Set`、`Open Decisions / Risks`、`Resume Cursor`、`Required Revalidation`、`Archived Milestones`、`Continuation Status`。`Resume Cursor` **MUST** 恰好包含一个 `Next action`。
- LLM **MUST** 只通过 typed JSON 参与 compact：Map 输出带来源顺序、约束作用域、生命周期与 action 的 `CompactFactBatch`；Reduce 由 Context 在 Rust 中确定性完成；Refresh 只输出不包含受保护字段的 `CheckpointCompressionPatch`。Markdown **NEVER** 作为 Map / Reduce / Refresh 的阶段间协议；最终 `active_summary` 只能由 Context 的单一确定性 renderer 模板化生成。
- Map 或 Refresh 的输出若因 JSON 语法、字段类型、未知字段、缺失必填字段或领域 invariant 不合规，Context **MUST** 将原始非法输出与精确校验错误发送给 LLM 进行一次有界格式修复；修复请求 **MUST** 保留事实、来源、顺序、authority、scope、lifecycle、约束动作、目标和 resume intent，**NEVER** 借格式修复创造新事实或扩大权限。只有修复仍失败后才能按该阶段既有 fallback 语义降级；取消在修复前后均立即终止，**NEVER** 重试或 fallback。Map 只重试失败 chunk，不重跑已成功 chunk；Refresh 修复耗尽时保留当前有效 checkpoint。Rust-owned Reduce 不消费 LLM 输出，因此不存在 Reduce repair 路径。
- Repair/fallback 的质量目标 **MUST** 优先保留可执行连续性：`Current Objective` 保留最新主用户具体请求，`Committed Facts` 纳入 ToolResult 支持的证据，`Uncommitted Working Set` 保留明确的进行中/未提交工作，唯一 `Next action` 直接引用最新主用户的下一步要求。裸 ToolUse 不再逐项扩写为风险清单，因为它只证明调用意图且会显著降低 checkpoint 信息密度。
- 约束作用域 **MUST** 区分 `session`、`task`、`phase`、`tool_call` 与 `unknown`。只有明确的主 Session 用户输入才能建立或修订 `session` 约束；子代理 prompt、tool-call 参数、系统生成消息与来源不明文本中的限制 **NEVER** 提升为主 Session 长期权限边界。`grant`、`restrict`、`revoke` 与 `supersede` 按来源顺序和同一 scope 确定性归并，跨 scope 不得相互扩大权限。
- Refresh patch **MUST NOT** 包含 immutable constraints、当前目标、唯一 `Next action`、显式 prohibited actions、continuation status 或 waiting reason；这些受保护字段始终由 Context 从当前 checkpoint 原样保留。任何包含未知字段、字段类型错误或应用后违反领域 invariant 的 patch 必须拒绝，并保留上一版有效 checkpoint。previous checkpoint、typed facts、compression patch 与 local fallback **MUST** 汇入同一领域类型后再归一化和渲染。
- Summary **MUST** 输出 `Continue | Waiting for User | Completed` 三态 continuation。`Continue` 表示下一轮模型在一次简短动态状态重验证后直接执行 `Next action`；`Waiting for User` 只用于确实缺少批准、选择、输入或新权限；`Completed` 只用于用户请求已交付且没有剩余工作。
- `Committed Facts` **MUST** 只承载由 tool result、commit、测试或持久化状态支持的事实。assistant 文本和 ToolUse 本身 **NEVER** 直接成为 committed fact，只能进入风险区或带 `unverified` 标记的 working set。PR、CI、worktree、remote branch 等动态当前态 **MUST** 进入 `Required Revalidation`。
- 连续 compact 时，上一轮 active checkpoint **MUST** 作为 authoritative previous checkpoint 显式进入下一轮 compact 输入；Context **MUST** 按语义分区预算收敛，**NEVER** 对整份 previous checkpoint 做 authoritative head/tail 截断。Runtime **MAY** 替换 summary block，但 **NEVER** 在生成新 checkpoint 前丢弃旧 checkpoint。
- `Current Task State` 是九分区之外的 typed companion：它 **MUST** 在 checkpoint 定稿后由 Context canonical commit 路径追加；连续 compact **MUST** 丢弃旧 companion，只追加当前请求的 `task_context`，且 **NEVER** 将 companion 送入 LLM、混入 checkpoint 或在 Runtime 建立第二 owner。
- 单次 LLM compact 请求 **MUST** 只有一份带真实 history 的 compact prompt；**NEVER** 在尾部追加一条没有 history 的重复指令。
- 本地 fallback **MUST** 把 assistant 文本和 ToolUse 标为未验证报告 / 已观察调用，**NEVER** 直接据此声称工作完成；只有最新 unresolved user request 可输出 `Continue`，assistant 的等待、普通报告或完成报告都保守输出 `Waiting for User`。`Completed` 只允许语义摘要在确认交付事实后输出。
- Recent tail 的切分位置与 summary 覆盖范围是两个独立概念：调整 summary 输入 **NEVER** 隐式改变 tail 的预算、Run/Step 边界或 `split_point`。
### 8.4 compact_window 切分
Target 的 Leaf / Branch 与 recent raw 选择见
[06-persistent-summary-tree.md](06-persistent-summary-tree.md) §4–§6。下面的
`CompactWindow` 仍描述结构化 RunStep backing 的过渡目标；Current 的 message
10% 行为见 §8.9。
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
### 8.5 Pre/PostCompact Hook（Future production wiring）
- Hook Published Language 与 Config surface 已包含 `PreCompact` / `PostCompact`，但当前 Runtime compact 管线**尚未**构造或派发对应 `HookInvocation`；用户配置当前不会触发。
- Target `PreCompact`：compact 前触发，可用 `additional_context` 扩展摘要请求、用 `system_message` 通知 UI，并可 Block 阻止 compact。
- Target `PostCompact`：compact 成功提交后触发，可用 `additional_context` 作为 compact 后补充上下文。
- Future 接线必须覆盖 auto/manual compact、Block 后状态保持、context/message 消费、取消与 Resume 相邻边界；在这些证据完成前 **NEVER** 把 PL/Config 存在性描述为生产支持。
- **PreCompact Reflection** 是 Memory/Runtime 的独立现有机制，不等于 Hook `PreCompact`，其当前行为见 [05-memory-injection.md](05-memory-injection.md) §9。
### 8.6 Circuit Breaker
- `AutoCompactState` 是 session repository 拥有的自动 compact 运行态，记录 `compaction_count`、`consecutive_failures` 与 `circuit_broken`。
- 失败上限来自 `ContextConfig.auto_compact_failure_limit`，默认 `3`，Config snapshot 将非法 `0` 收敛为 `1`；Composition 只消费 `ConfigWiring` 发布的窄视图，不建立第二配置状态。
- 每次自动 compact 开始前申请 attempt permit；breaker 已打开时返回 `CompactSkipReason::CircuitBreakerOpen`，不进入生成。
- durable commit 成功后记录 success 并清零连续失败；返回 `ContextPortError` 或 attempt 在未完成时被丢弃记录 failure；typed skip（包括取消、resume protection、hook block、CAS 之前的中性跳过）不增加失败计数。
- LLM 失败后成功提交显式 `LocalFallback(failure_kind)` 属于本次 durable compact 成功；quality 元数据保留原始失败种类，不能伪装为 LLM summary。
- manual compact 复用同一个 freeze/generate/CAS/commit mechanics，但**必须绕过自动 breaker**；它仍受 resume protection、hook、durable-save-before-publish 与 revision/CAS 约束。

### 8.7 Compact 提交协议（统一入口）
`CanonicalSessionRepository` 是唯一提交 owner：
1. 在 mutation gate 内冻结 `CompactSource { revision, messages, visible_steps, previous_summary }`；
2. 释放 gate，经 `CompactGenerator` 执行 prompt、单次或 map/reduce、parse、取消与 fallback；
3. 再次取得 gate，校验当前 revision 等于 source revision，构造 `ActiveCompactMarker`；
4. 先 durable persist candidate，成功后才 publish 新 generation。

任一 Provider/LLM `await` **NEVER** 持有 Session mutation gate。CAS 冲突必须保留 freeze 后新增历史并返回 typed conflict，禁止 stale generation 覆盖当前 Session。durable save 失败时不得 publish；publish 只能发生在保存成功之后。
### 8.8 Manual Compact
用户 `/compact` 命令触发：
- **绕过 token 阈值检查**，但必须存在至少一个可进入 summary 的 finalized RunStep
- manual compact 不经过 `compaction_decision` 判定，直接进入 compact use case；内部 **NEVER** 重复检查自动阈值
- manual compact 与 automatic compact 共享 Context-owned generation/commit mechanics，但不读取或修改 automatic circuit breaker；manual 请求当前没有 Run cancellation 字段时使用独立未取消 token。
### 8.9 Current 落地边界与 Deferred 迁移
当前生产 `ChatChain` 只保留 Run/segment 边界，并在 compact 前调用
`messages_flat()`；因此现状**无法正确按 RunStep 裁 recent tail**。在
`CommittedRunStep` backing 落地前，Current 与 Target 必须明确区分：
**Current（本次已实现）**：
1. Context decision 优先采用 Provider ACL 标准化的 `last_api_total_tokens`；baseline 缺失时回退完整 candidate heuristic，并统一以 `decision_token_count > threshold` 进入 `Compacting`；
2. 删除 auto compact 各层重复阈值判断，`Compacting` 内只执行一次管线；
3. 连续 compact 把上一轮 active summary 显式并入下一轮 summary 输入；
4. recent tail **保持现有实现不变**：按 message 数保留约 10%（至少 4 条），
   并在 tail 内修复 ToolUse/ToolResult 配对、占位 ToolResult；这不是
   Run/Step-aware 算法；
5. 现有 Microcompact 仍在 compact 管线内执行；本次未把它迁移为
   `PreparingContext` 常驻投影，也未新增 micro 实验。
**Deferred / Target（本次不实现）**：
1. Snip / Microcompact 在每次 `PreparingContext` 常驻执行，并统一按完整 Run
   保护最近 3 个 Run；
2. recent tail 按完整 finalized RunStep 选择，使用 `context_size * 30%`
   token 预算并从最远 Step 开始移出；
3. Session schema 保存 `RunId → RunStepId → messages`，compact 提交前保持
   Run / Step 结构，只在 Provider 出站时扁平化。
在这些 Deferred 项落地前，文档中的 Run/Step-aware 30% 算法均为 Target，
**NEVER** 作为 Current 行为或“已完成的最小修复”验收。
## 9. 幂等性设计（#550）
### 9.1 Fingerprint 契约
字段、构造与缓存范围的唯一真相见 [Token Budget](03-token-budget.md) §5。本文只定义 Compact 对该契约的使用规则，**NEVER** 复制类型字段。
- **fingerprint 不变**时当前只跳过重复 microcompact 扫描；Future PreCompact Hook 接线后，还必须定义 hook 是否按 attempt/fingerprint 去重，接线前不得宣称已跳过该 Hook
- `compaction_decision` 计算对相同 backing revision + request 是确定性函数
- `compact` 的效果对相同 ChatChain + 相同 ContextRequest 是确定性的
### 9.2 生命周期
- `CompactionFingerprint` 存储在 Run 内存态（不落盘）
- 每轮 `build_window` 从纯 compact 输入计算 fingerprint
- 下一轮进入 `PreparingContext` 时比对：相同则跳过 L2/L3 的重复扫描
- fingerprint 命中只复用 L2-L4 投影，**NEVER** 跳过 Prompt / Skill / Memory 物化或复用整个 ContextWindow
## 10. 常量统一来源
全部预算输入只由 [03-token-budget.md](03-token-budget.md) 定义的 Context-owned 纯函数或本 Run 已解析 capability 提供：
| 输入 / 策略 | 默认值 / 来源 | 唯一所有者 |
|---|---|---|
| `context_size` | 本 Run 的 model capability / ConfigSnapshot | Run/Invocation binding |
| `max_output_tokens` | 本 Run 的 model capability / ConfigSnapshot | Run/Invocation binding |
| `reserved_context` | `context_size * 2%`（动态计算） | `token_budget::summary_budget(context_size)` |
| threshold safety ratio | 0.8 | `token_budget::autocompact_threshold` |
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
| #1162 持久化增量摘要树 | L5 增量摘要 / projection / usage | [06-persistent-summary-tree.md](06-persistent-summary-tree.md) |
| #554 Context collapse | L4 | §7 |
## 12. 相关文档
- Session 聚合（ChatChain/ChatSegment）：[01-session.md](01-session.md)
- Token Budget 详解：[03-token-budget.md](03-token-budget.md)
- 持久化增量摘要树：[06-persistent-summary-tree.md](06-persistent-summary-tree.md)
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
| 2026-07-17 | 自动触发落地 Provider 标准化 last_total_tokens；明确 Snip/Microcompact 常驻、Run/Step-aware 30% recent tail 为 Deferred Target，Current tail 仍保持 message 10% | compact token reset design |
| 2026-07-17 | 补充 L5 summary 保真度：所有被移除消息必须进入 summary；按序汇总用户输入且后续修正覆盖前述冲突要求；禁止动作层级升级；增加 continuation 三态 | [#671](https://github.com/rushsinging/aemeath/issues/671) |
| 2026-07-18 | L5 Target 改为持久化增量摘要树；同步 map-reduce 降为 legacy backfill，冻结 per-session 1 / global 5 与 compact usage 总账 | [#1162](https://github.com/rushsinging/aemeath/issues/1162) |
| 2026-07-19 | #876 回写实际四方法 ContextPort、`ContextRequest.step_id`、ContextWindow backing revision、Main/Sub execution 单向消费，以及 append/compact/resume 共用 mutation gate | [#876](https://github.com/rushsinging/aemeath/issues/876) |
| 2026-07-21 | L5 唯一生产管线改为短锁 freeze、无锁 `CompactGenerator` 生成、revision/CAS durable commit 后 publish；补 typed cancellation/fallback quality，并将可配置 session 级自动熔断与 manual bypass 纳入同一 Context-owned mechanics | L5 compact CAS/cancellation |
| 2026-08-10 | L5 compact 生成契约改为 typed JSON：map 局部事实、reduce/refresh typed checkpoint、scope/lifecycle 权限归并与单一 Markdown renderer，禁止阶段性只读约束升级为主 Session 长期边界 | [#1582](https://github.com/rushsinging/aemeath/issues/1582) |
