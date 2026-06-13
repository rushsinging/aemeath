# Runtime / TUI UUIDv7 内部 ID 设计

## 背景

049 timeline/render 边界收敛后，TUI 输出区依赖 `chat_id + turn_id + tool_call_id` 在 timeline 与 conversation domain 之间 join。当前这些 ID 仍主要是 `String`：

- TUI `ChatId` / `ChatTurnId` / `ToolCallId` 是薄 String newtype。
- SDK `ChatEventContext` 的 `chat_id` / `turn_id` 是 String。
- runtime `RuntimeTurnContext`、`RuntimeStreamEvent` 的 id 字段是 String。
- tool 流已有 `id` 与 `provider_id` 分离雏形，但内部 id 仍可能是 `tool-1`、provider id 或测试字符串。

这导致内部实体 ID 与 provider 协议 ID 混用，难以保证跨 chat/turn/tool join 的唯一性和可排序性。

## 目标

- `ChatId`、`ChatTurnId`、内部 `ToolCallId` MUST 是 UUIDv7 newtype。
- 新 runtime/TUI 会话中产生的内部 chat、turn、tool id MUST 全部由 UUIDv7 生成。
- provider 返回的 tool id MUST NOT 作为内部 `ToolCallId`。
- tool call MUST 额外保存 `provider_id`，用于 provider 协议回填。
- 回填给 LLM/provider 时 MUST 通过内部 `ToolCallId` 找到 `provider_id`，实时组装 `ToolResult` / tool message。
- 旧历史加载时遇到非 UUIDv7 旧 id MUST 临时重新生成 UUIDv7；不保留旧内部 id 的稳定映射。

## 非目标

- 不改变 provider 对外协议字段名。OpenAI/Claude/Ollama 仍接收各自要求的 provider tool id。
- 不要求旧历史中的调试日志和新 UUIDv7 稳定关联。
- 不把 `share::message::ContentBlock::ToolUse.id` 改为内部 id；它仍表达 provider 协议中的 tool_use id。

## ID 类型边界

### 共享 ID 模块

新增共享 ID 类型，建议放在 `packages/sdk` 或 `agent/shared` 中，并由 runtime/TUI 复用：

- `ChatId(Uuid)`
- `ChatTurnId(Uuid)`
- `ToolCallId(Uuid)`

每个类型提供：

- `new_v7()`：生成 UUIDv7。
- `parse_uuid7(str) -> Result<Self, IdParseError>`：只接受 version 7。
- `from_legacy_or_new(str) -> Self`：历史兼容入口；非 UUIDv7 直接生成新 UUIDv7。
- `as_uuid()` / `as_str()` / `Display`。
- serde 序列化为 UUID 字符串，反序列化时严格检查 UUIDv7；历史兼容必须显式走 migration 入口，不能混在普通 serde 中。

### Runtime / SDK 事件

- `RuntimeTurnContext` 使用 `ChatId` / `ChatTurnId`。
- `sdk::ChatEventContext` 使用同一组 ID 类型，或使用 SDK re-export 的 ID 类型。
- `RuntimeStreamEvent::ToolCallStart/Update/ToolResult.id` 使用 `ToolCallId`。
- `AgentProgress.tool_id` 使用 `ToolCallId`。
- TUI `UiTurnContext`、`RuntimeObservation`、`ConversationIntent` 使用强类型 ID，不再传裸 String。

### Provider 消息边界

- `share::message::ContentBlock::ToolUse { id, ... }` 中的 `id` 继续表示 provider id。
- `ContentBlock::ToolResult { tool_use_id, ... }` 中的 `tool_use_id` 继续表示 provider id。
- provider conversion 不感知内部 UUIDv7，只处理 provider id。

## Tool ID 与 provider_id 映射

### ToolCall 结构

runtime 内部 `ToolCall` 调整为：

- `id: ToolCallId`：内部 UUIDv7。
- `provider_id: String`：provider 返回的 tool_use / tool_call id。
- `name: String`
- `index: usize`
- `input: serde_json::Value`

### ToolIdentityRegistry

`ToolIdentityRegistry` 继续负责将 provider stream 信息映射到内部 id，但输出必须是 `ToolCallId`：

- `by_stream_index: HashMap<usize, ToolCallId>`
- `by_provider_id: HashMap<String, ToolCallId>`
- 新 id 由 `ToolCallId::new_v7()` 生成。
- 同一 provider id MUST 复用同一内部 id。
- provider id 缺失时，按 stream index 生成/复用内部 id；后续如果 provider id 出现，应补齐 provider 映射。

### 回填组装

工具执行结果内部保留：

- `runtime_id: ToolCallId`
- `provider_id: String`
- `output`
- `content`
- `is_error`
- `images`

回填给 provider 时，`tool_results_for_api()` / sub-agent `append_tool_results()` MUST 丢弃 runtime id，使用 provider id 构造 `Message::tool_results_rich()`。

当前代码已经有测试覆盖“回填使用 provider_id 而不是 runtime_id”，迁移后这些测试 MUST 保留并改为断言 `ToolCallId` 不会进入 `ContentBlock::ToolResult.tool_use_id`。

## 旧历史兼容

历史加载必须显式 migration：

1. 如果 chat/turn/tool 内部 id 是 UUIDv7，直接解析。
2. 如果不是 UUIDv7，生成新的 UUIDv7。
3. 单次加载过程中 MAY 维护临时 in-memory 映射，确保同一个旧 id 在同一份历史中引用一致。
4. 该映射 MUST NOT 持久化为“旧 id -> 新 id”的全局兼容层。
5. migration 后保存的新状态 MUST 只包含 UUIDv7。

Provider message 中的 `ToolUse.id` / `ToolResult.tool_use_id` 不走该 migration，因为它们是 provider 协议 id，不是内部 id。

## 数据流

1. 用户输入开始新 chat：TUI 或 runtime 生成 `ChatId::new_v7()` 与首个 `ChatTurnId::new_v7()`。
2. runtime provider stream 收到 tool_use start：
   - provider handler 提供 provider id、name、index。
   - `ToolIdentityRegistry` 分配/复用内部 `ToolCallId`。
   - runtime 发出 `ToolCallStart { id: ToolCallId, provider_id, ... }`。
3. TUI adapter 将 runtime event 投影成 `RuntimeObservation`。
4. conversation model 用 `chat_id + turn_id + tool_call_id` join timeline 与 tool payload。
5. 工具执行结束：runtime 发出 `ToolResult { id: ToolCallId, provider_id, ... }` 给 TUI。
6. 回填 LLM：runtime 使用 result 中的 `provider_id` 构造 provider tool result message。

## 错误处理

- 普通 serde 反序列化遇到非 UUIDv7 MUST 报错，防止新路径悄悄接受旧 id。
- 历史加载使用专门 migration 函数，遇到非 UUIDv7 记录 debug 日志并生成新 UUIDv7。
- provider id 缺失但需要回填时 MUST 返回错误或生成 provider 层可接受的 fallback，并记录 warning；不能用内部 UUIDv7 冒充 provider id，除非 provider 原始协议允许无 id 或 runtime 确认该 id 就是发送给 provider 的 id。
- 若 tool result 找不到 provider_id，MUST 在 runtime 层失败并生成可诊断错误，而不是在 TUI 层展示 orphan result 后继续回填错误协议。

## 实施分阶段

1. 新增共享 UUIDv7 ID 类型和测试。
2. 迁 SDK `ChatEventContext` 与 chat/tool event 字段。
3. 迁 runtime `RuntimeTurnContext`、`RuntimeStreamEvent`、`ToolIdentityRegistry`、`ToolCall`、`ToolResultTuple`。
4. 迁 TUI `UiTurnContext`、conversation ids、timeline refs、adapter/projector/root reducer。
5. 迁历史/session restore 入口，加入旧 id 临时再生成逻辑。
6. 更新测试 fixture，禁止新增 `chat-1` / `turn-1` / `tool-1` 作为内部 id。
7. 验证 provider 回填仍使用 provider id。

## 测试计划

- ID 类型单测：
  - `new_v7()` 生成 version 7。
  - `parse_uuid7()` 接受 UUIDv7。
  - `parse_uuid7()` 拒绝 UUIDv4、普通字符串、空字符串。
  - `from_legacy_or_new()` 对非 UUIDv7 生成新的 UUIDv7。
- ToolIdentityRegistry 单测：
  - 同 provider id 复用同内部 `ToolCallId`。
  - 不同 provider id 生成不同 UUIDv7。
  - stream index 与 provider id 后续绑定一致。
- Runtime 回填单测：
  - `tool_results_for_api()` 使用 provider_id 而不是内部 UUIDv7。
  - sub-agent `append_tool_results()` 使用 provider_id。
- TUI join 单测：
  - `OutputTimelineItem::ToolCall/ToolResult` 通过 UUIDv7 `chat_id + turn_id + tool_call_id` join。
  - 同 provider id 不同 chat/turn 不串线。
- 历史 migration 单测：
  - 旧 `chat-1` / `turn-1` / `tool-1` 被转换为 UUIDv7。
  - 同一历史内重复旧 id 引用保持一致。
  - provider `tool_use_id` 不被改写。
- 验证命令：
  - `cargo fmt --check`
  - `cargo test -p sdk`
  - `cargo test -p runtime`
  - `cargo test -p cli`
  - `cargo clippy -p cli --all-targets -- -D warnings`
  - `.agents/hooks/check-architecture-guards.sh`
  - `.agents/hooks/check-unit-tests.sh`
  - `.agents/hooks/check-render-pure.sh`
  - `git diff --check`

## 已定实现决策

- ID 类型放在 `packages/sdk` 并由 runtime/TUI 复用，避免 SDK 对外事件退化成裸 String。
- 本次不新增 `ProviderToolId` newtype；provider id 继续在 provider/share message 边界使用 String，后续如需加强协议 ID 类型再单独设计。
