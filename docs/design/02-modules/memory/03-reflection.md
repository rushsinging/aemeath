# Memory · Reflection 引擎

> 层级：02-modules / memory（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#789（S2）
> 本文定义 Reflection 引擎的领域模型、触发条件、prompt 构建、output schema、apply 流程，以及与 Runtime 的职责边界。**只描述目标态**；实现差距见 [迁移治理](../../03-engineering/migration-governance.md)。

## 1. 定位

Reflection 是 Memory BC 内部的**领域服务**——它不调 LLM，不依赖 ProviderPort。它负责：

1. **构建 prompt**：把当前项目记忆 + 最近对话摘要组装为反思 prompt（纯函数，i18n）。
2. **解析 output**：把 LLM 返回的 JSON 解析为 `ReflectionOutput`（含 MemorySuggestion）。
3. **定义应用规则**：由当前 Run 绑定的 `MemoryPort::apply_reflection` 在同一 active Memory instance 上写入 suggestion、标记过期记忆。

Runtime 负责：
- **触发判断**：interval / forced / pre-compact 三种时机的判定。
- **LLM 调用**：经 ProviderPort 发起独立 LLM 调用，传入 Memory BC 构建的 prompt。
- **结果回传**：把 LLM 原始响应交回纯 Reflection port parse，再让同一 Run 的 MemoryPort apply。

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
    if !config.enabled || !config.reflection.enabled {
        return false;
    }
    match mode {
        Interval { turn_count } => {
            config.reflection.interval_turns > 0
                && turn_count.is_multiple_of(config.reflection.interval_turns)
        }
        Forced => true,           // Forced 永远运行，不受 interval_turns == 0 影响
    }
}
```

### 三种触发时机

| 时机 | 模式 | 执行方式 | 触发者 | 说明 |
|---|---|---|---|---|
| **轮次间隔** | `Interval` | **异步 spawn** | Runtime loop | 每 `interval_turns`（默认 10）轮结束时触发；有 tool_calls 且非 EndTurn 时跳过；不阻塞主循环 |
| **Pre-compact** | `Forced` | **同步 await** | Runtime compact 前 | compact 前同步等待 reflection 完成 + `apply_reflection` 写入；保证 compact 后第一轮 `retrieve_for_inject` 可检索到新记忆 |
| **用户强制** | `Forced` | **同步 await** | 用户 `/reflection` | 用户主动请求反思，需等待结果展示 |

### 异步执行模型

Interval 触发的 Reflection **不阻塞主循环**——Runtime `tokio::spawn` 后台任务，主循环继续执行。后台任务完成后通过 `mpsc::channel` 回传结果，主循环在下一轮 `select!` 分支接收并 emit。

Pre-compact 触发的 Reflection **MUST 同步完成**——compact 会改变对话历史，如果 reflection 未完成就 compact，新记忆无法在 compact 后的第一轮注入中检索到。

```text
Interval 触发:
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

Pre-compact 触发:
  Runtime compact 前 → 同步调用 reflection pipeline
    │ (主循环等待)
    ▼
  build_prompt → call_llm → parse → apply_reflection → 返回
    │
    ▼
  compact 继续（此时新记忆已写入，compact 后第一轮可检索）
```

### 并发控制

- **单一后台 slot**：同一时间最多一个 **Interval** 后台 Reflection 任务。
- **前一个未完成时跳过 Interval**：新 Interval 触发时若 slot 被占用，跳过本次（log debug），不排队。
- **Pre-compact 不受 slot 限制**：Pre-compact 是同步 await 执行，**NEVER** 走后台 slot。即使 Interval 后台任务正在运行，Pre-compact 也会同步完成。
- **Forced 不受限**：`/reflection` 是同步执行，不经过后台 slot。
- **Run 结束时 drain**：Run 结束时若 Interval 后台任务仍在执行，等待其完成或超时后丢弃（避免孤儿任务）。

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

- **project_memory**：Runtime 用当前 Run 的 `MemoryPort::list(Project)` 读取 active 条目，再交给 `ReflectionPromptPort::format_memory_summary` 纯格式化；ReflectionPromptPort **NEVER** 自行选择 store。
- **recent_summary**：从最近对话消息提取文本，按 `[User]/[Assistant]: text` 格式逆序拼接，截断到合理长度。
- **i18n**：prompt 模板支持中英文，按 `lang` 参数选择。

### format_memory_summary（纯函数）

```rust
fn format_memory_summary(entries: &[MemoryEntry]) -> String {
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
    Unparseable(String),                   // 响应无法解析为 JSON
}
```

LLM 调用 / 空响应属于 Runtime-owned invocation error；Memory 打开失败属于 `MemoryOpenError`。二者 **NEVER** 塞进 `ReflectionError`，避免纯 parse / apply 协议反向拥有外部生命周期错误。

## 7. Apply 流程

```rust
memory.apply_reflection(&output).await -> Result<ReflectionApplyResult, MemoryError>;
```

### 步骤

1. **apply_suggestions**：本 Run 捕获的 MemoryPort 实例遍历 `suggested_memories`，转换为 MemoryEntry（UUIDv7 + now + source=Llm），在其内部并发控制下执行 write / eviction retry。
2. **apply_outdated**：同一实例遍历 `outdated_memories` 并标记过期。

eviction retry 是 MemoryPort implementation 的内部用例，**NEVER** 接受或暴露 `&mut MemoryStore`。

### auto_apply_suggestions

```rust
if config.reflection.auto_apply_suggestions {
    match memory.apply_reflection(&output).await {
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

Memory BC 提供纯领域逻辑，Runtime 负责编排。Interval 走异步路径；Pre-compact 和 Forced 走同步路径。

### 8.1 异步路径（Interval）

```text
Runtime 判定 should_run_reflection
  │  └─ 检查后台 slot 是否空闲（非空闲 → skip + log debug）
  │
  ▼
Runtime: spawn 后台任务（携带 messages 快照）
  │ (主循环不等待，继续处理 outcome / compact / 下一轮)
  │
  ▼
后台任务:
  Memory BC: build_reflection_prompt(project_memory, recent_summary, lang)
    └─ Runtime: memory.list(Project) → ReflectionPromptPort.format_memory_summary(entries)
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
    本 Run 捕获的 MemoryPort Arc: await apply_reflection(output)
      └─ implementation 内 apply_suggestions → eviction retry
      └─ apply_outdated → mark_outdated
  │
  ▼
  后台任务完成 → mpsc::Sender 发 ReflectionResult
  │
  ▼
  主循环 select! 分支收到结果 → emit SystemMessage / ReflectionResult
```

### 8.2 同步路径（Pre-compact / Forced / `/reflection`）

```text
Runtime: 同步调用 reflection pipeline（主循环等待）
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
  Pre-compact: 始终 apply_reflection（NEVER 受 auto_apply=false 控制）
  Forced: if auto_apply → apply_reflection
  │
  ▼
  Runtime: emit ReflectionResult{output, formatted_content, tokens, auto_applied}
  │
  ▼
  返回调用方（Pre-compact 的调用方在收到返回后才继续 compact）
```

### 8.3 Pre-compact 快照语义

Pre-compact 触发时，Runtime 在 compact 前**同步**调用 reflection pipeline（使用当前 messages，不需要 clone 快照——因为主循环在等待，messages 不会被并发修改）。Reflection 完成后 `apply_reflection` 写入新记忆，compact 继续。

- **不需要快照**：Pre-compact 是同步路径，主循环等待，messages 不会被并发修改。
- **记忆即时可检索**：`apply_reflection` 完成后，compact 后第一轮 `retrieve_for_inject` 可检索到新写入的记忆。

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
| 2026-07-12 | 初稿：ReflectionEngine 领域服务、MemorySuggestion、触发条件、prompt/output/apply、职责边界 | #789 |
| 2026-07-12 | 补充：Interval 和 Pre-compact 改为异步 spawn，Forced 保持同步；并发控制和快照语义 | #789 |
