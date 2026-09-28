# Memory · Reflection 引擎

> 层级：02-modules / memory（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#789（S2）
> 本文定义 Reflection 引擎的领域模型、触发条件、prompt 构建、output schema、Memory-owned workflow，以及与 Runtime 的职责边界。**只描述目标态**。

## 1. 定位

Reflection 是 Memory BC 内部的**领域服务**——它不调 LLM，不依赖 ProviderPort。它负责：

1. **构建 prompt**：把当前项目记忆 + 最近对话摘要组装为反思 prompt（纯函数，i18n）。
2. **解析 output**：把 LLM 返回的 JSON 解析为 `ReflectionOutput`（含 MemorySuggestion）。
3. **应用结果**：把 suggestion 转为 MemoryEntry，并通过当前 Run 的同一 `MemoryPort` 写入/合并、归档候选、标记过期记忆。
4. **历史事实**：定义 `ReflectionRecord` 与只读 `ReflectionHistoryQuery`；Runtime 执行通道完成后写入历史，`/reflect` 只查询这些记录。

Runtime 负责：
- **触发判断**：interval / manual request / pre-compact 三种来源的判定；三者都进入同一同步执行通道。
- **LLM 调用**：经 ProviderPort 发起独立 LLM 调用，传入 Memory BC 构建的 prompt。
- **历史提交**：将完成结果交给 Memory-owned history adapter；完成后只发布不含正文的计数事实。

### 职责边界

| 职责 | 归属 | 说明 |
|---|---|---|
| prompt 模板构建 | Memory BC | 纯函数，知道记忆格式与反思需求 |
| output schema / parsing | Memory BC | MemorySuggestion 类型归 Memory |
| apply 逻辑（写入 / 标记过期）| Memory BC | 经 MemoryPort 操作 |
| 触发时机判定 | Runtime | interval / forced / pre-compact |
| LLM 调用 | Runtime | 经 ProviderPort |
| Reflection 配置消费 | 双方 | Config 下发 MemoryConfig，Runtime 读触发条件，Memory 读 apply 策略 |

Memory BC **不依赖** ProviderPort——这保持 Context Map 一致（Memory 无 Memory→Provider 边）。

## 2. MemorySuggestion

```rust
struct MemorySuggestion {                // Reflection 产出的候选记忆（VO）
    layer: MemoryLayer,                  // 建议写入的层（默认 Project）
    category: MemoryCategory,            // 建议的分类
    content: String,                     // 建议内容
    tags: Vec<String>,                   // 建议标签
    reason: String,                      // LLM 给出的建议理由
}
```

MemorySuggestion 是 **Reflection 的产出物**，不是 MemoryEntry——它还没有 id、created_at、last_confirmed_at。apply 时转换为 MemoryEntry（生成 UUIDv7 + 填充时间戳 + source = Llm）写入。

## 3. ReflectionOutput

```rust
struct ReflectionOutput {                // LLM 返回的完整反思结果
    deviations: Vec<String>,             // 偏差检测：对话中的偏离行为
    suggested_memories: Vec<MemorySuggestion>, // 建议新增的记忆
    outdated_memories: Vec<String>,      // 建议标记过期的记忆 id 列表
}
```

- **deviations**：LLM 观察到 agent 在对话中有偏离预期行为的描述（如重复尝试失败方案、忽略用户指令）。正文只保存在 Memory-owned history record 中；TUI 的只读查询只取得计数等安全摘要。
- **suggested_memories**：LLM 认为值得持久化的新记忆建议。
- **outdated_memories**：LLM 认为已过时的已有记忆 id 列表（apply 后标记 outdated）。

### 反序列化兼容

LLM 可能返回 `null` 而非空数组。使用 `null_as_empty_vec` 自定义反序列化器处理：

```rust
#[serde(default, deserialize_with = "null_as_empty_vec")]
pub deviations: Vec<String>,
```

## 4. 触发条件

Runtime 负责判定是否触发 Reflection，Memory BC 提供配置读取辅助。当前已接通 Interval 与 PreCompact；Manual 显式入口由 #1289 承接。

### 触发来源

```rust
enum ReflectionTrigger {
    Interval,       // 每 N 轮
    PreCompact,     // compact 前快照
    Manual,         // Runtime 显式手动请求；不是 /reflect 查询命令
}
```

Runtime 对三种来源统一做 enable / interval 判定并构造拥有消息快照的请求。`Manual` 与 `PreCompact` 在启用 Reflection 时不受 interval 限制，但仍走同一个同步执行通道，不存在特殊路径。

### 三种触发时机

| 时机 | Trigger | 执行方式 | 触发者 | 说明 |
|---|---|---|---|---|
| **轮次间隔** | `Interval` | Runtime 同步 await | Runtime loop | 每 `interval_runs`（默认 10，旧键 `interval_run_steps` 仍可读取）个 Run 触发一次；计数 per-session，`/clear` 与 resume 切换后从 0 重数；有 tool_calls 且非 EndTurn 时跳过；轮末等待反思完成 |
| **Pre-compact** | `PreCompact` | Runtime 同步 await | Runtime compact 成功后 | compact 前冻结“将被丢弃”的 messages 快照；只有 compact 成功产生 outcome 后才执行 |
| **手动请求** | `Manual` | Runtime 同步 await | `/reflect-now` 命令（#1289） | 与另两种 trigger 共用通道；`/reflect [limit]` **NEVER** 进入此入口 |

### Manual 显式入口链路（#1289）

用户唯一可见入口是 slash 命令 `/reflect-now`（无参数）。链路与 `/compact` 同构：

```text
TUI "/reflect-now"
  → Tools Command Catalog: ApplicationControl / Memory target（补全与路由描述）
  → SDK ChatInputEvent::ReflectNow
  → Runtime input gate:
      idle → PendingCommand::ReflectNow（不启动新 Run）
      busy → CommandResultText("Reflection 正在运行…") 后丢弃，NEVER 排队
  → run_launch handler:
      memory/reflection 未启用 → CommandResultText(DisabledSkipped 文案)
      bind_main_run → session.structured_messages() 冻结 owned 快照
      run_manual_reflection(ReflectionTaskTrigger::Manual, snapshot)
        ├─ DisabledSkipped → 未启用提示
        └─ Completed       → CommandResultText（完成计数；Failed 时 is_error）
```

- **消息快照来源**：idle 时经 `MainSessionWiring::bind_main_run` 读取 committed CanonicalSession 的 `structured_messages()`（与 `/sessions` 列表同一投影），即当前可见 active 历史；不包含 system 注入。
- **等待终态再回显**：handler 同步 await 反思终态，只回显安全计数，不向 chat 投影反思正文。
- **配置门控**：input gate 的 busy 判定与配置门控是两层，前者在 Run 进行中直接丢弃，后者由 `DisabledSkipped` 表达。

### 同步执行模型

三种 trigger 全部进入 Runtime-owned 的同一执行通道，调用方 await 到终态：`Completed`（携带不含正文的状态与计数）或 `DisabledSkipped`。执行开始前 append `Running` durable fact；成功、失败、partial apply、timeout 或 cancel 时再以同一 id `upsert` 终态。Runtime **NEVER** 把反思正文投影到 TUI 或 chat，只发布计数事实。`/reflect [limit]` 是只读 history query，不触发 LLM，也不执行 apply。

```text
Interval / PreCompact / Manual
  → Runtime run(snapshot)
      ├─ 配置禁用 → DisabledSkipped（记录 [reflection_disabled]）
      └─ append Running → build_prompt → call_llm → parse → optional apply
                       → upsert terminal record
                       → 变更计数 > 0 时：TUI SystemMessage + 累积 LLM reminder
```

### 完成时的记忆变更提示

反思 apply 产生变更时走双通道，两侧都在「反思完成」这一时刻发生：

- **TUI**：立即 `RuntimeStreamEvent::SystemMessage`，渲染为 system notice，文案只含条数。
- **LLM**：变更计数累积在 adapter 内，由**下一次 Main Run 启动时**取走一次，注入 `InvocationReminderData::MemoryUpdated`。当前轮的 LLM 请求在反思完成前已发出，因此只能进下一轮；这与 system prompt 的 Session 冻结语义一致。

零变更、失败、取消、超时与配置禁用都不产生任何提示。

### 结束控制

- **执行期间可取消**：反思是 Run 内的协作式阶段，Run 的 cancellation token 直接传入执行通道；取消只形成安全终态 metadata，不泄漏 prompt、provider raw response 或 Reflection 正文。
- **任务超时**：执行通道对反思施加 timeout，超时形成安全终态。
- **无后台残留**：三种 trigger 都在所属调用点 await 完成，Session teardown 不需要 drain 或等待后台 job。

### 间隔触发的跳过条件

- `before_finish_gate_continue`（Run 还在门禁续行中）
- 有 tool_calls 且 `stop_reason != EndTurn`（工具调用中途不反思）
- `config.enabled = false` 或 `config.reflection.enabled = false`

## 5. Prompt 构建（纯函数）

```rust
fn build_reflection_prompt(
    project_memory: &str,     // 当前项目记忆摘要
    recent_summary: &str,     // 最近对话摘要
    lang: &str,               // "zh" | "en"
) -> String;
```

Prompt 结构（i18n）：

```text
# 当前项目记忆
{project_memory}

# 最近对话摘要
{recent_summary}

# 任务
分析以上对话，识别：
1. deviations: agent 是否有偏离行为
2. suggested_memories: 值得持久化的新记忆
3. outdated_memories: 已过时的记忆 id

只输出 JSON，格式如下：
{...}
```

- **project_memory**：从当前 Run 持有的同一 `MemoryPort` 读取 Project 层 active 条目，格式化为 `- [Category][tags] content` 列表。
- **recent_summary**：从最近对话消息提取文本，按 `[User]/[Assistant]: text` 格式逆序拼接，截断到合理长度。
- **i18n**：prompt 模板支持中英文，按 `lang` 参数选择。

### memory_summary（纯函数）

```rust
fn memory_summary(entries: &[MemoryEntry]) -> String {
    entries.iter()
        .map(|e| format!("- [{:?}][{}] {}", e.category, e.tags.join(","), e.content))
        .collect::<Vec<_>>()
        .join("\n")
}
```

### recent_messages_summary（纯函数）

```rust
fn recent_messages_summary(messages: &[Message], max_chars: usize) -> String;
```

- 逆序遍历消息，提取 Text content block。
- 格式：`[User]: text` / `[Assistant]: text`。
- 截断到 `max_chars`（`usize::MAX` 表示不截断）。

## 6. Output 解析

```rust
fn parse_output(raw: &str) -> Result<ReflectionOutput, ReflectionError>;
```

- LLM 返回纯 JSON 文本。
- 使用 serde 反序列化为 `ReflectionOutput`。
- JSON 结构错误返回 `ReflectionError::Parse`，非 JSON 或无可提取对象返回 `ReflectionError::Unparseable`；两者只携带稳定的安全类别/固定描述，**NEVER** 附带 raw response、prompt、对话或 Reflection 正文。

### ReflectionError

```rust
enum ReflectionError {
    Parse,                                // JSON 解析失败，不携带原文
    Memory(MemoryError),                  // MemoryPort 操作失败
    InvalidSuggestion(String),            // suggestion 违反领域约束
    Unparseable,                          // 响应无法解析为 JSON，不携带原文
}
```

**注意**：Memory 的 `ReflectionError` 只表达 parse、领域约束和 Memory 操作失败，且 Display **NEVER** 附带 raw response、prompt、对话或正文。LLM 调用、空响应与执行期 apply 失败由 Runtime 的执行错误与安全类别表达，不能回流为 Memory parse 错误。

## 7. Apply 流程

```rust
async fn apply_output(
    output: &ReflectionOutput,
    memory: &dyn MemoryPort,
) -> Result<ReflectionApplyResult, MemoryError> {
    memory.apply_reflection(output).await
}
```

### 步骤

1. **apply_suggestions**：`MemoryPort::apply_reflection` 遍历 `suggested_memories`，将每条转换为 MemoryEntry（UUIDv7 + now + source=Llm），执行去重、容量判断与必要的归档重试。
2. **apply_outdated**：同一 Port 实例遍历 `outdated_memories` 并标记过期；它就是当前 Run shared lease 捕获的 active Memory Arc。
3. **提交语义**：每个 layer 使用 MemoryService candidate/CAS/publish 协议；跨层部分完成返回结构化 `MemoryError::PartialApply`，不伪装成全成功。

### auto_apply_suggestions

```rust
if config.reflection.auto_apply_suggestions {
    memory.apply_reflection(&output).await
}
```

- `auto_apply_suggestions = false`（默认）时，不修改 active Memory；完整 output 只作为 Memory-owned `ReflectionRecord` 持久化，`/reflect` 仍只返回安全摘要。
- `auto_apply_suggestions = true` 时，执行通道写入 suggestion 并标记过期；apply 计数进入 record / safe summary，并触发完成时的双通道计数提示（不投影正文）。

### ReflectionApplyResult

```rust
struct ReflectionApplyResult {
    suggestions_added: usize,     // 成功写入/合并的 suggestion 数
    outdated_marked: usize,       // 标记过期的记忆数
}
```

## 8. 完整编排流程（Runtime 侧）

Memory BC 提供 prompt / parse / apply / history 的单一 `ReflectionWorkflow`；Runtime 统一编排 Interval、PreCompact、Manual 三种 trigger 与 Provider 调用。三种 trigger 走同一条同步执行路径，调用方 await 到终态。

```text
Runtime trigger
  ├─ capture owned messages snapshot
  └─ ReflectionTaskAdapter.run
       ├─ DisabledSkipped → [reflection_disabled] 安全日志，返回调用方
       └─ append Running → build_prompt → Provider invocation → parse_output
                       → optional MemoryPort.apply_reflection
                       → upsert terminal ReflectionRecord
                       → 返回 completion（状态 + 计数，不含正文）
                       → 计数 > 0 时发 TUI SystemMessage 并累积 LLM reminder

调用方（Run 轮末 / compact 完成 / /reflect-now）await 上述结果。
```

### 8.1 Pre-compact 快照语义

PreCompact 在 compact 前把所选 `messages` clone 为 owned snapshot，但只有 compact 成功产生 outcome 后才执行。compact 失败、被 hook block、消息不足或取消时不执行。执行只使用冻结快照，因此不会观察 compact 后的消息变化。

- **快照时机**：compact 执行前冻结，`messages` 尚未被压缩；执行时机在 compact 成功 outcome 之后。
- **快照内容**：`messages_selected_for_precompact_memory(messages)` 的结果（只取 compact 会丢掉的消息）。
- **完成去向**：执行前 append `Running`，终态以同 id upsert；不回传完整结果，也不在后续轮次 emit 正文。

### 8.2 History 与安全查询

`ReflectionRecord` 是 Memory-owned 持久化事实，包含 trigger、状态、可选 parsed output / apply result、错误类别、token usage 与 duration。Runtime 接受任务后先通过 `ReflectionHistoryStore::append` 写入 `Running`，成功、失败、partial apply、timeout 或 cancel 后以同 id `upsert` 终态；adapter 使用 project-scoped durable dataset，append/upsert/query 均由 Memory 拥有。

`/reflect [limit]` 只调用 `ReflectionHistoryQuery::list(limit)`，该 query 已按 newest-first 返回至多 `limit` 条 `ReflectionSafeSummary`；Runtime/SDK 仅映射其交付 DTO：id、时间、trigger、status、deviation/suggestion/outdated 数量、apply 状态、错误类别、token 计数与耗时。该查询**不运行 Reflection、不 apply、也不返回 output 正文**。

### 8.3 安全日志

Reflection 日志只能记录 event、trigger、status、error category、token/count、duration 与 record id 等 metadata。日志 **NEVER** 包含 prompt、对话消息、Memory content、provider raw response、parsed output、formatted content 或任何正文截断；解析失败也不得记录所谓“前 N 字符”。

## 9. model 覆盖

```rust
struct ReflectionConfig {
    model: Option<String>,    // None = 继承主对话模型
}
```

- `None`：Reflection 使用与主对话相同的 LLM 模型。
- `Some("model-id")`：使用指定模型（如更便宜的模型跑反思）。
- Runtime 读取此配置，经 ProviderPort 选择对应 client。

## 10. 相关文档

- 模块入口：[README.md](README.md)
- 领域模型（MemoryEntry / MemorySuggestion）：[01-domain-model.md](01-domain-model.md)
- 检索与注入：[02-retrieval-and-injection.md](02-retrieval-and-injection.md)
- 端口与适配器（ReflectionWorkflow / history）：[04-ports-and-adapters.md](04-ports-and-adapters.md)
- Runtime 端口（ProviderPort）：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Context Map（Memory 不依赖 Provider）：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-09-28 | 三种 trigger 由「单槽后台异步」改为「同一执行通道同步 await」：调用方 await 到终态，Run 的 cancellation token 传入执行通道，Session teardown 不再 drain；完成时按 apply 计数发 TUI SystemMessage 并累积下一轮 LLM reminder；`user_alert` 与无生产调用方的 `format_output` 随 i18n 死代码一并移除 | #1772 |
| 2026-09-25 | #1289 接通 Manual 显式入口：Tools catalog `/reflect-now` → SDK `ChatInputEvent::ReflectNow` → input gate（idle 受理 / busy 提示丢弃，NEVER 排队）→ run_launch handler 冻结 `structured_messages()` 快照 submit 单槽 | #1289 |
| 2026-07-20 | #1285 为 Run teardown 落地有界 drain→cancel→terminal 收口；Manual 显式入口由 #1289（归 #860）承接 | #1285/#1289/#860 |
| 2026-07-20 | #1284 接通 compact 成功后的 PreCompact 冻结快照提交；Manual 显式入口拆分至 #1289 | #1284/#1289 |
| 2026-07-20 | #1283 将 parse 错误收窄为不含模型原文的稳定类别，且 `ReflectionHistoryQuery` 仅发布安全摘要；完整 record 保持在 Memory adapter 内部 | #1283 |
| 2026-07-19 | #900 将旧 `MemoryStore` apply 示例更新为当前 Run shared lease 捕获的同一 `MemoryPort::apply_reflection`，保留 `ReflectionEngine` 作为无状态 prompt/parse 领域服务 | #900 |
| 2026-07-18 | #899 完成三 trigger Runtime 单槽异步、busy skip、静默完成、Memory-owned history append/query 持久化、`/reflect [limit]` 只读安全摘要、安全日志与 Run teardown drain/cancel timeout | #899 |
| 2026-07-12 | 初稿：ReflectionEngine 领域服务、MemorySuggestion、触发条件、prompt/output/apply、职责边界 | #789 |
| 2026-07-12 | 早期并发方案已由 #899 的三 trigger 统一异步单槽语义取代 | #789/#899 |
