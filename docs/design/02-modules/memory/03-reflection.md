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

- **deviations**：LLM 观察到 agent 在对话中有偏离预期行为的描述（如重复尝试失败方案、忽略用户指令）。纯文本，供 TUI 展示。
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

### 触发模式

```rust
enum ReflectionRunMode {
    Interval { turn_count: usize },    // 间隔触发：每 N 轮
    Forced,                             // 强制触发：用户 /reflection 或 pre-compact
}
```

### 判定逻辑（Runtime 侧）

```rust
fn should_run_reflection(mode: ReflectionRunMode, config: &MemoryConfig) -> bool {
    if !config.enabled || !config.reflection.enabled || config.reflection.interval_turns == 0 {
        return false;
    }
    match mode {
        Interval { turn_count } => turn_count.is_multiple_of(config.reflection.interval_turns),
        Forced => true,
    }
}
```

### 三种触发时机

| 时机 | 模式 | 执行方式 | 触发者 | 说明 |
|---|---|---|---|---|
| **轮次间隔** | `Interval` | **异步 spawn** | Runtime loop | 每 `interval_turns`（默认 10）轮结束时触发；有 tool_calls 且非 EndTurn 时跳过；不阻塞主循环 |
| **Pre-compact** | `Forced` | **异步 spawn** | Runtime compact 前 | compact 前抓 messages 快照交给后台 reflection，compact 立即继续不等待；reflection 用快照跑，结果通过 channel 回传 |
| **用户强制** | `Forced` | **同步 await** | 用户 `/reflection` | 用户主动请求反思，需等待结果展示 |

### 异步执行模型

Interval、Pre-compact 与用户手动请求全部进入 Runtime 单槽后台协议，调用方不等待完整 Reflection 结果。后台任务完成后写入 Memory-owned `ReflectionRecord` history；只发送内部完成/失败信号供 slot 释放和诊断，**NEVER** 主动把完整结果推送到 TUI。`/reflect` 是只读 history query，不触发 LLM，也不执行 apply。

```text
Interval / Pre-compact 触发:
  Runtime 判定 should_run → spawn 后台任务（携带 messages 快照）
    │ (主循环不等待，继续处理 outcome / compact / 下一轮)
    ▼
  后台任务: build_prompt → call_llm → parse → apply
    │
    ▼
  后台任务完成 → mpsc::Sender 发 ReflectionResult
    │
    ▼
  主循环 select! 分支收到结果 → emit SystemMessage / ReflectionResult
```

### 并发控制

- **单一后台 slot**：同一时间最多一个后台 Reflection 任务（Interval 或 Pre-compact 共享一个 slot）。
- **前一个未完成时跳过**：新触发时若 slot 被占用，跳过本次（log debug），不排队。
- **Forced 不受限**：`/reflection` 是同步执行，不经过后台 slot，可与后台任务并发（但实际场景几乎不会同时发生）。
- **Run 结束时 drain**：Run 结束时若后台任务仍在执行，等待其完成或超时后丢弃（避免孤儿任务）。

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

- **project_memory**：从 MemoryStore 读取 Project 层 active 条目，格式化为 `- [Category][tags] content` 列表。
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
- 解析失败返回 `ReflectionError::Unparseable`，附带前 200 字符供调试。

### ReflectionError

```rust
enum ReflectionError {
    Parse(serde_json::Error),              // JSON 解析失败
    Memory(MemoryError),                   // Memory 操作失败
    Apply(String),                         // apply 流程失败
    StoreInit(String),                     // MemoryStore 初始化失败
    LlmCall(String),                       // LLM 调用失败（由 Runtime 设置）
    EmptyResponse,                         // LLM 返回空响应
    Unparseable(String),                   // 响应无法解析为 JSON
}
```

**注意**：`LlmCall` 和 `EmptyResponse` 由 Runtime 侧设置——Memory BC 的 `parse_output` 和 `apply` 不会产生这些错误。错误类型统一在 `ReflectionError` 中，但产生源区分清楚。

## 7. Apply 流程

```rust
fn apply_output(
    output: &ReflectionOutput,
    store: &mut MemoryStore,
) -> Result<ReflectionApplyResult, ReflectionError>;
```

### 步骤

1. **apply_suggestions**：遍历 `suggested_memories`，每条转换为 MemoryEntry（UUIDv7 + now + source=Llm），调用 `add_with_eviction_retry` 写入。
2. **apply_outdated**：遍历 `outdated_memories`，调用 `store.mark_outdated(id)` 标记过期。

### add_with_eviction_retry

```rust
fn add_with_eviction_retry(store: &mut MemoryStore, entry: MemoryEntry) -> Result<bool, ReflectionError> {
    match store.add(entry.clone())? {
        AddResult::Added { .. } | AddResult::Merged { .. } => Ok(true),
        AddResult::NeedsEviction { candidates } => {
            let ids = candidates.iter().map(|e| e.id.clone()).collect();
            store.evict(&ids)?;
            match store.add(entry)? {
                Added | Merged => Ok(true),
                NeedsEviction { .. } => Err(Apply("still requires eviction after retry")),
            }
        }
    }
}
```

### auto_apply_suggestions

```rust
if config.reflection.auto_apply_suggestions {
    match apply_output(&output, &mut store) {
        Ok(result) => { /* 追加 auto-apply 摘要到 formatted_content */ }
        Err(e) => { /* warn log，不阻断 */ }
    }
}
```

- `auto_apply_suggestions = false`（默认）时，suggestion 只展示给用户，由用户决定是否手动写入。
- `auto_apply_suggestions = true` 时，自动写入并标记过期，在输出中追加摘要。

### ReflectionApplyResult

```rust
struct ReflectionApplyResult {
    suggestions_added: usize,     // 成功写入/合并的 suggestion 数
    outdated_marked: usize,       // 标记过期的记忆数
}
```

## 8. 完整编排流程（Runtime 侧）

Memory BC 提供纯领域逻辑，Runtime 负责编排。Interval 和 Pre-compact 走异步路径，Forced 走同步路径。

### 8.1 异步路径（Interval / Pre-compact）

```text
Runtime 判定 should_run_reflection
  │  └─ 检查后台 slot 是否空闲（非空闲 → skip + log debug）
  │
  ▼
Runtime: spawn 后台任务（携带 messages 快照 / clone）
  │ (主循环不等待，继续处理 outcome / compact / 下一轮)
  │
  ▼
后台任务:
  Memory BC: build_reflection_prompt(project_memory, recent_summary, lang)
    └─ Memory BC: memory_summary(store.list(Project))
    └─ Memory BC: recent_messages_summary(messages_snapshot)
  │
  ▼
  Runtime: call_llm(prompt, system_prompt)  ──→ ProviderPort
  │
  ▼
  Memory BC: parse_output(llm_response)
  │
  ▼
  Memory BC: format_output(output, lang)  ──→ formatted_content
  │
  ▼
  if auto_apply:
    Memory BC: apply_output(output, &mut store)
      └─ apply_suggestions → add_with_eviction_retry
      └─ apply_outdated → mark_outdated
  │
  ▼
  后台任务完成 → mpsc::Sender 发 ReflectionResult
  │
  ▼
主循环 select! 分支收到结果 → emit SystemMessage / ReflectionResult
```

### 8.2 同步路径（Forced / `/reflection`）

```text
Runtime 处理 /reflection 命令
  │
  ▼
Memory BC: build_reflection_prompt(project_memory, recent_summary, lang)
  │
  ▼
Runtime: call_llm(prompt, system_prompt)  ──→ ProviderPort
  │
  ▼
Memory BC: parse_output(llm_response)
  │
  ▼
Memory BC: format_output(output, lang)  ──→ formatted_content
  │
  ▼
if auto_apply:
  Memory BC: apply_output(output, &mut store)
  │
  ▼
Runtime: emit ReflectionResult{output, formatted_content, tokens, auto_applied}
```

### 8.3 Pre-compact 快照语义

Pre-compact 触发时，Runtime 在 compact 前把 `messages` clone 一份交给后台任务。后台任务用这份快照构建 `recent_messages_summary`。compact 正常执行不等待。

- **快照时机**：compact 函数入口处，`messages` 尚未被压缩时。
- **快照内容**：`messages_selected_for_precompact_memory(messages)` 的结果（与现状一致，只取 compact 会丢掉的消息）。
- **结果回传**：后台任务完成后通过 channel 回传，主循环在后续轮次 emit。用户可能在 compact 后才看到 reflection 结果——这是可接受的 trade-off。

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
| 2026-07-18 | #898 将 Reflection PL/prompt/schema/parse/format/apply 归回 Memory；发布 history query 契约。统一异步、静默完成与 `/reflect` 只读查询的执行 adapter 由 #899 承接 | #898/#899 |
| 2026-07-12 | 初稿：ReflectionEngine 领域服务、MemorySuggestion、触发条件、prompt/output/apply、职责边界 | #789 |
| 2026-07-12 | 补充：Interval 和 Pre-compact 改为异步 spawn，Forced 保持同步；并发控制和快照语义 | #789 |
