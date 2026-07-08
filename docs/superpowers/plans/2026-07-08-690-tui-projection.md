#690 TUI 投影化 —— 删除 chat.messages 镜像 + ChatRequest 增量化

**日期**：2026-07-08
**对应 Issue**：#690
**前置**：#687 + #688 + #689 已合入

## 现状

### ChatRequest 调用方（4 个）

| 调用方 | messages 内容 | 用途 |
|---|---|---|
| `no_tui.rs` | `vec![user_text(text)]` | 非交互模式单条消息 |
| `sessions_command.rs` | `Vec::new()` | 空，走 input_events |
| `model_selection.rs` | `Vec::new()` | 空，走 input_events |
| `processing.rs` (TUI) | `ctx.messages`（全量历史） | TUI 启动/重建时 |

### chat.messages 的 14 个引用

| 类型 | 位置 | 数量 |
|---|---|---|
| 赋值（runtime → TUI 覆盖） | `ui_event.rs` | 7 |
| 赋值（resume） | `resume.rs:29` | 1 |
| clear | `resume.rs:19`, `slash.rs:288` | 2 |
| clone 喂 runtime | `spawn_context.rs:19` | 1 |
| len() 读 | `slash.rs:76,207` | 2 |
| test 断言 | `enter.rs:123` | 1 |

## 改动计划

### 1. SDK 层：ChatRequest 增量化

`packages/sdk/src/chat.rs`：

```rust
// 之前
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    pub queue_drain: Option<Arc<dyn QueueDrainPort>>,
    pub input_events: Option<Arc<dyn ChatInputEventPort>>,
}

// 之后
pub struct ChatRequest {
    /// 初始 user input（首次 chat 时传入，常驻 loop 后续走 input_events）
    pub user_input: Option<UserInput>,
    pub queue_drain: Option<Arc<dyn QueueDrainPort>>,
    pub input_events: Option<Arc<dyn ChatInputEventPort>>,
}

pub struct UserInput {
    pub text: String,
    pub images: Vec<ChatInputImage>,
}
```

### 2. Runtime 层：chat_impl 改造

`trait_chat.rs`：

- 从 `ChatRequest.user_input` 提取首条消息（而非全量 messages）
- 如果 `current_chain` 已有历史（resume 场景），保留它；否则初始化空 chain
- 图片校验从 `user_input.images` 检查

```rust
// 之前
let messages: Vec<_> = input.messages.into_iter().map(message_from_sdk).collect();
let chain = ChatChain::from_flat_messages(messages);

// 之后
let chain = {
    let existing = me.inner.current_chain.lock().unwrap().clone();
    if existing.is_empty() {
        // 首次启动：从 user_input 构造
        ChatChain::default()  // loop idle 会等第一条用户输入
    } else {
        existing  // resume：保留已有 chain
    }
};
```

### 3. TUI 层：删除 chat.messages

#### 3a. ChatState 删 messages 字段

`app/state/chat.rs`：删除 `pub messages: Vec<sdk::ChatMessage>` 字段和 Default 中的初始化。

#### 3b. SpawnContext 删 messages

`effect/session/processing.rs`：`SpawnContext` 删除 `messages` 字段。
`app/update/spawn_context.rs`：`build_spawn_context` 不再 clone messages。
`effect/session/processing.rs:655`：`ChatRequest` 构造改为 `user_input: None`（常驻 loop，输入走 input_events）。

#### 3c. ui_event.rs 删除 7 处赋值

`self.chat.messages = messages;` 全部删除。这些 UiEvent 变体仍携带 `messages: Vec<ChatMessage>`，但 TUI 不再消费它。

#### 3d. resume.rs 删除赋值

`self.chat.messages.clear()` 和 `self.chat.messages = messages.clone()` 删除。

但 resume 需要 runtime 恢复 chain——改为通过 `ChatInputEvent::ResumeSession` 或 `load_session` RPC 触发 runtime 恢复，TUI 只更新 `model.conversation.timeline`。

#### 3e. slash.rs 改造

- `/context` 和 `/stats`：`self.chat.messages.len()` 改为从 `model.conversation.timeline` 近似计算或删除该统计
- `clear_conversation`：`self.chat.messages.clear()` 删除（runtime Reset 事件清 chain）

#### 3f. no_tui.rs 改造

```rust
// 之前
.chat(sdk::ChatRequest {
    messages: vec![sdk::ChatMessage::user_text(text)],
    ...
})

// 之后
.chat(sdk::ChatRequest {
    user_input: Some(sdk::UserInput { text, images: Vec::new() }),
    ...
})
```

### 4. SessionRestore 已在 #688 改好

`trait_session.rs::load_session_impl` 已写 `restore.active_chain` 到 `current_chain`。resume 通过 `ChatInputEvent::ResumeSession` 触发，runtime 从 `current_chain` 恢复。

## 依赖顺序

1. SDK 层改 ChatRequest（编译断裂点）
2. Runtime 改 chat_impl
3. TUI 改 4 个调用方
4. TUI 删 chat.messages 字段 + 所有引用
5. 测试修复

## 风险

1. **resume 路径**：TUI resume 需确保 runtime 已 load chain。当前 `load_session_impl` RPC 写 `current_chain`，TUI resume 通过 `ResumeSession` 事件触发 runtime 内部 load。两条路径都需验证。
2. **no_tui 模式**：非交互模式单次 `chat()` 传 `user_input` 而非 messages，需确认 loop 正确处理。
3. **UiEvent 携带 messages 的变体**：7 个变体仍带 `Vec<ChatMessage>` payload。TUI 不再消费它，但删除会破坏 SDK 契约。保留 payload 不动，TUI 侧忽略即可。
