# Memory · Reflection 引擎

> 层级：02-modules / memory（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#789（S2）
> 本文定义 Reflection 引擎的领域模型、触发条件、prompt 构建、output schema、apply 流程，以及与 Runtime 的职责边界。**只描述目标态**；现状 Reflection 代码散落在 `runtime/business/reflection/` 的差距记入 `03-engineering/03-migration-governance.md`。

## 1. 定位

Reflection 是 Memory BC 内部的**领域服务**——它不调 LLM，不依赖 ProviderPort。它负责：

1. **构建 prompt**：把当前项目记忆 + 最近对话摘要组装为反思 prompt（纯函数，i18n）。
2. **解析 output**：把 LLM 返回的 JSON 解析为 `ReflectionOutput`（含 MemorySuggestion）。
3. **应用结果**：把 suggestion 转为 MemoryEntry，并通过当前 Run 的同一 `MemoryPort` 写入/合并、归档候选、标记过期记忆。
4. **历史事实**：定义 `ReflectionRecord` 与只读 `ReflectionHistoryQuery`；异步执行 adapter 完成后写入历史，`/reflect` 只查询这些记录。

Runtime 负责：
- **触发判断**：interval / manual request / pre-compact 三种来源的判定；三者都进入异步单槽协议。
- **LLM 调用**：经 ProviderPort 发起独立 LLM 调用，传入 Memory BC 构建的 prompt。
- **历史提交**：将完成结果交给 Memory-owned history adapter；完成后不主动向 TUI 投影完整结果。

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

MemorySuggestion 是 **Reflection 的产出物**，不是 MemoryEntry——它还没有 id、created_at、accessed_at。apply 时转换为 MemoryEntry（生成 UUIDv7 + 填充时间戳 + source = Llm）写入。

## 3. ReflectionOutput

```rust
struct ReflectionOutput {                // LLM 返回的完整反思结果
    deviations: Vec<String>,             // 偏差检测：对话中的偏离行为
    suggested_memories: Vec<MemorySuggestion>, // 建议新增的记忆
    outdated_memories: Vec<String>,      // 建议标记过期的记忆 id 列表
    user_alert: Option<String>,          // 需要提醒用户的事项
}
```

- **deviations**：LLM 观察到 agent 在对话中有偏离预期行为的描述（如重复尝试失败方案、忽略用户指令）。正文只保存在 Memory-owned history record 中；TUI 的只读查询只取得计数等安全摘要。
- **suggested_memories**：LLM 认为值得持久化的新记忆建议。
- **outdated_memories**：LLM 认为已过时的已有记忆 id 列表（apply 后标记 outdated）。
- **user_alert**：LLM 认为需要提醒用户的重要事项（可选）。

### 反序列化兼容

LLM 可能返回 `null` 而非空数组。使用 `null_as_empty_vec` 自定义反序列化器处理：

```rust
#[serde(default, deserialize_with = "null_as_empty_vec")]
pub deviations: Vec<String>,
```

## 4. 触发条件

Runtime 负责判定是否触发 Reflection，Memory BC 提供配置读取辅助：

### 触发来源

```rust
enum ReflectionTrigger {
    Interval,       // 每 N 轮
    PreCompact,     // compact 前快照
    Manual,         // Runtime 显式手动请求；不是 /reflect 查询命令
}
```

Runtime 对三种来源统一做 enable / interval 判定并构造拥有消息快照的后台请求。`Manual` 与 `PreCompact` 在启用 Reflection 时不受 interval 限制，但仍使用相同异步单槽，不存在特殊执行通道。

### 三种触发时机

| 时机 | Trigger | 执行方式 | 触发者 | 说明 |
|---|---|---|---|---|
| **轮次间隔** | `Interval` | Runtime 单槽异步 submit | Runtime loop | 每 `interval_turns`（默认 10）轮结束时提交；有 tool_calls 且非 EndTurn 时跳过；不阻塞主循环 |
| **Pre-compact** | `PreCompact` | Runtime 单槽异步 submit | Runtime compact 成功后 | compact 前冻结“将被丢弃”的 messages 快照；只有 compact 成功产生 outcome 后才 submit，不等待 Reflection |
| **手动请求** | `Manual` | Runtime 单槽异步 submit | Runtime 显式请求入口 | 与另两种 trigger 共用 slot；busy 时同样 skip；`/reflect [limit]` **NEVER** 进入此入口 |

### 异步执行模型

三种 trigger 全部进入 Runtime-owned 的同一个单槽后台 adapter，调用方只得到 `Accepted`、`BusySkipped` 或 disabled skip，不 await 完整结果。slot 接受任务后，先 append `Running` durable fact；执行成功、失败、partial apply、timeout 或 cancel 时再以同一 id `upsert` 终态。Runtime 仅保留不含正文的 completion metadata 用于 slot 释放、drain 与诊断，**NEVER** 主动发出完整 `ReflectionResult`、正文或“完成”系统消息到 TUI。`/reflect [limit]` 是只读 history query，不触发 LLM，也不执行 apply。

```text
Interval / PreCompact / Manual
  → Runtime submit(messages snapshot)
      ├─ slot busy → BusySkipped（不排队）
      └─ Accepted → spawn: build_prompt → call_llm → parse → optional apply
                       → append Running；upsert terminal record
                       → 仅安全 completion metadata，释放 slot

主 Run / compact 不等待结果，也不把结果主动投影到 TUI。
```

### 并发与结束控制

- **单一后台 slot**：Interval、PreCompact、Manual 同一时间合计最多一个 Reflection job。
- **busy skip**：slot 已占用或当前无法立即取得 slot 时，新触发返回 `BusySkipped`；不等待、不排队，也不另开并发路径。
- **任务超时**：adapter 对后台执行施加 timeout；超时/取消只形成安全终态 metadata，不泄漏 prompt、provider raw response 或 Reflection 正文。
- **Run 结束 drain/cancel**：先 drain 已完成/正在收口的 job；若在结束 deadline 内仍未完成，则 cancel，等待其释放 slot 后结束，避免孤儿任务及 Run lease 逃逸。

### 间隔触发的跳过条件

- `before_finish_gate_continue`（Run 还在门禁续行中）
- 有 tool_calls 且 `stop_reason != EndTurn`（工具调用中途不反思）
- `config.enabled = false` 或 `config.reflection.enabled = false`
- 后台 slot 被占用（前一个 Reflection 未完成）

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
4. user_alert: 需要提醒用户的事项

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
fn parse_output(raw: &str) -> Result<ReflectionOutput, serde_json::Error>;
```

- LLM 返回纯 JSON 文本。
- 使用 serde 反序列化为 `ReflectionOutput`。
- 解析失败返回 `ReflectionError::Unparseable`，只携带稳定的安全类别/固定描述；**NEVER** 附带 raw response、prompt、对话或 Reflection 正文。

### ReflectionError

```rust
enum ReflectionError {
    Parse(serde_json::Error),              // JSON 解析失败
    Memory(MemoryError),                   // MemoryPort 操作失败
    Apply(String),                         // apply 流程失败
    LlmCall(String),                       // LLM 调用失败（由 Runtime 设置）
    EmptyResponse,                         // LLM 返回空响应
    Unparseable(String),                   // 响应无法解析为 JSON
}
```

**注意**：`LlmCall` 和 `EmptyResponse` 由 Runtime 侧设置——Memory BC 的 `parse_output` 和 `apply` 不会产生这些错误。错误类型统一在 `ReflectionError` 中，但产生源区分清楚。

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
- `auto_apply_suggestions = true` 时，后台 job 自动写入 suggestion 并标记过期；apply 计数进入 record / safe summary，不触发 TUI 主动展示。

### ReflectionApplyResult

```rust
struct ReflectionApplyResult {
    suggestions_added: usize,     // 成功写入/合并的 suggestion 数
    outdated_marked: usize,       // 标记过期的记忆数
}
```

## 8. 完整编排流程（Runtime 侧）

Memory BC 提供 prompt / parse / apply 与 history 端口；Runtime 统一编排 Interval、PreCompact、Manual 三种 trigger。不存在同步执行路径。

```text
Runtime trigger
  ├─ capture owned messages snapshot
  └─ ReflectionTaskAdapter.submit
       ├─ BusySkipped → 安全日志（trigger/status 等 metadata），返回主流程
       └─ Accepted → background job
            Memory: format_memory_summary + recent_messages_summary + build_prompt
            Runtime: Provider invocation
            Memory: parse_output
            Runtime: optional MemoryPort.apply_reflection
            Memory: append Running → upsert terminal ReflectionRecord
            Runtime: 保存安全 completion metadata 并释放 slot

Run teardown
  └─ drain → deadline 到期仍 busy 时 cancel → 等待 slot 收口
```

### 8.1 Pre-compact 快照语义

PreCompact 在 compact 前把所选 `messages` clone 为 owned snapshot，但只有 compact 成功产生 outcome 后才尝试 submit。compact 失败、被 hook block、消息不足或取消时不 submit；`Accepted` / `BusySkipped` 都不影响已经成功的 compact outcome。后台 job 只使用冻结快照，因此不会观察 compact 后的消息变化。

- **快照时机**：compact 执行前冻结，`messages` 尚未被压缩；提交时机在 compact 成功 outcome 之后。
- **快照内容**：`messages_selected_for_precompact_memory(messages)` 的结果（只取 compact 会丢掉的消息）。
- **完成去向**：接受后 append `Running`，终态以同 id upsert；不通过结果通道回传完整结果，也不在后续轮次 emit 正文。

### 8.2 History 与安全查询

`ReflectionRecord` 是 Memory-owned 持久化事实，包含 trigger、状态、可选 parsed output / apply result、错误类别、token usage 与 duration。Runtime 接受任务后先通过 `ReflectionHistoryStore::append` 写入 `Running`，成功、失败、partial apply、timeout 或 cancel 后以同 id `upsert` 终态；adapter 使用 project-scoped durable dataset，append/upsert/query 均由 Memory 拥有。

`/reflect [limit]` 只调用 `ReflectionHistoryQuery::list(limit)`，按 newest-first 返回至多 `limit` 条，再投影为 `ReflectionSafeSummary`：id、时间、trigger、状态、deviation/suggestion/outdated 数量、apply 状态、错误类别、token 计数与耗时。该查询**不运行 Reflection、不 apply、也不返回 output 正文**。

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
- 端口与适配器（ReflectionPromptPort）：[04-ports-and-adapters.md](04-ports-and-adapters.md)
- Runtime 端口（ProviderPort）：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Context Map（Memory 不依赖 Provider）：[../../01-system/03-context-map.md](../../01-system/03-context-map.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-19 | #900 将旧 `MemoryStore` apply 示例更新为当前 Run shared lease 捕获的同一 `MemoryPort::apply_reflection`，保留 `ReflectionEngine` 作为无状态 prompt/parse 领域服务 | #900 |
| 2026-07-18 | #899 完成三 trigger Runtime 单槽异步、busy skip、静默完成、Memory-owned history append/query 持久化、`/reflect [limit]` 只读安全摘要、安全日志与 Run teardown drain/cancel timeout | #899 |
| 2026-07-12 | 初稿：ReflectionEngine 领域服务、MemorySuggestion、触发条件、prompt/output/apply、职责边界 | #789 |
| 2026-07-12 | 早期并发方案已由 #899 的三 trigger 统一异步单槽语义取代 | #789/#899 |
