# Runtime / TUI UUIDv7 内部 ID 设计终态

> 完整设计：[`docs/superpowers/specs/2026-06-13-runtime-tui-uuidv7-id-design.md`](superpowers/specs/2026-06-13-runtime-tui-uuidv7-id-design.md)

## 问题

内部实体 ID（`ChatId`/`ChatTurnId`/`ToolCallId`）与 provider 协议 ID 混用（均为 `String`），难以保证跨 chat/turn/tool join 的唯一性和可排序性。

## 终态设计

### 三层 ID 分离

| 层 | ID 类型 | 来源 | 用途 |
|---|---|---|---|
| 内部 | `ChatId`/`ChatTurnId`/`ToolCallId`（UUIDv7 newtype） | runtime/TUI 生成 | 跨 chat/turn/tool join，timeline 与 conversation domain 关联 |
| Provider 协议 | `provider_id: String` | provider stream 返回 | 回填给 LLM 时使用，不进内部 ID 体系 |
| 持久化 | 内部 UUIDv7 serde 为字符串 | 会话落盘 | 旧非 UUIDv7 id 加载时临时重新生成 |

### ToolCall 双 ID 结构

```rust
struct ToolCall {
    id: ToolCallId,        // 内部 UUIDv7
    provider_id: String,   // provider 返回的 tool_use id
    name: String,
    index: usize,
    input: Value,
}
```

### 核心规则

- 新会话中产生的所有内部 ID **MUST** 为 UUIDv7，由 `new_v7()` 生成。
- Provider 返回的 tool id **MUST NOT** 作为内部 `ToolCallId`。
- 回填给 LLM/provider 时 **MUST** 通过内部 `ToolCallId` 找到 `provider_id`，使用 provider id 构造 `ToolResult`。
- `ContentBlock::ToolUse.id` / `ToolResult.tool_use_id` 继续表达 provider 协议 ID，不改为内部 ID。
- 旧历史加载时非 UUIDv7 旧 id **MUST** 临时重新生成 UUIDv7（不持久化兼容映射）。

### ID 类型位置

`packages/sdk`，由 runtime/TUI 复用。每个类型提供 `new_v7()`、`parse_uuid7()`、`from_legacy_or_new()`；serde 严格检查 UUIDv7，历史兼容走显式 migration 入口。

## 数据流

1. 新 chat → `ChatId::new_v7()` + `ChatTurnId::new_v7()`
2. provider stream tool_use → `ToolIdentityRegistry` 分配/复用内部 `ToolCallId`，保存 `provider_id`
3. TUI 用 `chat_id + turn_id + tool_call_id` join timeline 与 tool payload
4. 回填 LLM → 使用 `provider_id`（丢弃内部 UUIDv7）
