# ChatChain 聚合化收口 + Segment-aware Microcompact + TUI 投影化

**日期**：2026-07-08
**对应 Issue**：[#680](https://github.com/rushsinging/aemeath/issues/680)（伞 issue）
**关联**：[会话历史单一真相设计稿](../specs/2026-07-08-session-history-single-source-design.md)、[PR #681](https://github.com/rushsinging/aemeath/pull/681)（临时止血）、[PR #682](https://github.com/rushsinging/aemeath/pull/682)（设计文档）
**状态**：草案 —— 待用户确认后创建子 issue 并执行

## 目标

把 `ChatChain` 从「load/save 瞬间存在的贫血结构」提升为 **runtime 唯一活跃可变真相**，按真实 user turn 维护 segment，使 microcompact 能按 segment 边界（最近 3 个大 loop）保护探索类 ToolResult；同时完成 TUI 投影化（删除 `chat.messages` 镜像、`ChatRequest` 增量化），最终恢复 PR #681 禁用的两处 microcompact 调用。

## 现状诊断摘要

### 真相散落 ≥5 处

| # | 位置 | 形态 | 问题 |
|---|---|---|---|
| 1 | 磁盘 `Session.chats` | `Vec<ChatSegment>` | 保存时全部对话压进单个 segment |
| 2 | `RuntimeHandle.current_messages` | `Arc<Mutex<Vec<Message>>>` | 扁平数组，非聚合 |
| 3 | `ChatLoopContext.messages` | `Vec<Message>` (loop 局部) | `start_new_segment()` 零调用（死代码） |
| 4 | `SdkChatEventSink.current_messages` | `Arc<Mutex<Vec<Message>>>` | 每 sync event 覆盖 #2 |
| 5 | TUI `chat.messages` | `Vec<ChatMessage>` | runtime 每 turn 整块覆盖（7+1 处） |

### 关键问题

- `microcompact_messages` 按 `Role::User` 数消息，但 `ToolResult` 也是 `Role::User` → 边界偏移。
- `sync_current_messages_impl` 已无活跃调用方（死代码）。
- `build_spawn_context` 每次 `chat()` 调用时 clone 全量 `chat.messages` 喂给 runtime（`ChatRequest.messages: Vec<ChatMessage>`），这是 TUI 投影化的硬依赖。
- `ChatRequest.messages` 是全量数组，runtime 收到后直接写 `current_messages`——TUI 本质在「管理上下文」而非「发送新输入」。

### Turn 边界检测策略

`Message::user()` 创建的消息 `metadata: None`；`system_generated_user()` 显式 `source: SystemGenerated`；`tool_results()` 也是 `metadata: None`。不能仅靠 `metadata.source`。正确启发式：

```
is_user_turn_boundary(msg) =
    msg.role == User
    && !msg.content.iter().all(|b| matches!(b, ToolResult { .. }))   // 非纯 ToolResult 批次
    && msg.metadata.map_or(true, |m| m.source != SystemGenerated)     // 排除系统注入
```

---

## 子 issue 拆分（5 个，严格串行）

### 子 issue A：ChatChain 聚合化为 runtime 唯一活跃真相

**依赖**：无（结构性基础）
**可并行**：否

**目标**：把 `RuntimeHandle.current_messages` → `current_chain: Arc<Mutex<ChatChain>>`，loop 局部也持有 `ChatChain`。行为与现状一致（纯结构重构，不调 `start_new_segment()`）。

**改动范围**：

1. `business/session/chat_chain.rs`：
   - 新增 `push_to_active(msg: Message)` — 等价于现有 `push()`，语义明确化
   - 新增 `replace_chain(new_chain: ChatChain)` — compact / resume 时整体替换
   - 新增 `messages_flat() -> Vec<Message>` — 等价于现有 `messages()`，语义明确化（派生读模型）

2. `core/client/accessors.rs`：
   - `RuntimeHandle.current_messages: Arc<Mutex<Vec<Message>>>` → `current_chain: Arc<Mutex<ChatChain>>`
   - 删除兼容访问器 `current_messages_flat()`（直接暴露 chain 即可）

3. `core/client/trait_chat.rs`：
   - `chat_impl`：`messages` 从 `ChatRequest` 提取后构造初始 `ChatChain`（单 segment 包裹），写入 `current_chain`
   - `ChatLoopContext.messages: Vec<Message>` → `chain: ChatChain`
   - `SdkChatEventSink`：删除 `current_messages` 字段（sink 不再持有消息副本）

4. `core/client/event.rs`：
   - `SdkChatEventSink` 只做事件转发（删除 7 处 `*guard = messages.clone()`）
   - loop 在 emit 前**直接写共享 chain slot**：`*ctx.current_chain.lock() = chain.clone()`

5. `core/client/trait_session.rs`：
   - `save_session_from_handle`：从 `current_chain` 直接取 `active_segments()`（不再从扁平 `current_messages` 反构造单 segment）
   - `load_session_impl`：写 chain 到 `current_chain`

6. `business/chat/looping/loop_runner.rs`：
   - `ChatLoopContext` 字段 `messages` → `chain: ChatChain`
   - 4 处 `messages.push(x)` → `chain.push(x)`
   - 事件 payload `messages.clone()` → `chain.messages_flat()`
   - `ResumeSession`：`messages = restore.active_messages` → `chain = ChatChain::from_flat_messages(restore.active_messages)`
   - compact 路径 `*messages = outcome.messages` → `chain.replace_chain(...)`

**关键设计**：
- 此阶段**不调 `start_new_segment()`**——所有消息仍在单 segment 中，行为不变
- 事件仍携带 `Vec<Message>`（扁平）给 TUI——TUI 不需要 segment
- `chat_impl` 中 `messages.clone()` 写共享 slot 改为 `chain.clone()`

**验收**：
- `cargo test` 通过（所有现有测试不变，行为一致）
- `cargo clippy` 无新 warning
- Save/load round-trip 保持一致（单 segment）

---

### 子 issue B：按真实 user turn 维护 segment + 旧 session 重分段

**依赖**：A
**可并行**：否

**目标**：让 `start_new_segment()` 真正生效，按真实 user turn 维护 segment。

**改动范围**：

1. `business/session/chat_chain.rs`：
   - 新增 `is_user_turn_boundary(msg: &Message) -> bool` 纯函数 + 单测
   - 新增 `ChatChain::from_flat_messages(messages: Vec<Message>) -> Self`（扫描 turn boundary 自动切分 segment）+ 单测

2. `business/chat/looping/loop_runner.rs`：
   - 在真实 user input 到达时（`drain_and_apply_gate` 或 idle input gate 处）调用 `chain.start_new_segment()` 再 `chain.push(user_msg)`
   - 非 user input（assistant、tool result、system reminder）直接 `chain.push()`
   - compact 时 `chain.compact(summary, recent_tail)` — 已有 API，确认调用

3. `business/session/restore.rs`：
   - `SessionRestore` 新增 `active_chain: ChatChain` 字段（自动按 `is_user_turn_boundary` 重分段）
   - `from_session`：用 `ChatChain::from_flat_messages(active_messages)` 构造 `active_chain`
   - 保留 `active_messages: Vec<Message>` 供过渡期消费方使用

**旧 session 兼容**：
- 磁盘上的旧 session（单 `Normal` segment）load 后经 `from_flat_messages` **重新切分**为多段
- `migrate_legacy_messages`（`storage.rs`）保持不动——它在更早的层把 `messages` 包成单段，`from_flat_messages` 在更高层负责重切分

**验收**：
- 新增测试：`is_user_turn_boundary` 正确区分 real user / tool result / system generated
- 新增测试：`from_flat_messages` 按 turn boundary 正确切分
- 新增测试：旧单段 session load 后变多段
- `cargo test` + `cargo clippy` 通过

---

### 子 issue C：Segment-aware microcompact + 恢复两处调用

**依赖**：A + B
**可并行**：否

**目标**：microcompact 按 segment 边界保护最近 3 个大 loop；恢复 PR #681 禁用的两处调用。

**改动范围**：

1. `business/compact/microcompact.rs`：
   - 新增 `microcompact_chain(chain: &mut ChatChain) -> usize`：保护最近 3 个 segment，折叠更早 segment 的探索类 ToolResult
   - `PROTECT_RECENT_SEGMENTS = 3`（新增常量）
   - 保留 `microcompact_messages(messages: &mut [Message]) -> usize`：**修复 turn 检测**，用 `is_user_turn_boundary` 替代裸 `Role::User` 计数（供 sub-agent 使用）
   - 现有 `PROTECT_RECENT_TURNS = 2` 保留（sub-agent 用）

2. `business/chat/looping/loop_runner.rs`：
   - 取消 `TODO(#680)` 注释，恢复 `microcompact_chain(&mut chain)` 调用 + `MicrocompactDone` 事件 emit
   - 事件 payload 改用 `chain.messages_flat()`

3. `business/agent/runner/loop_helpers.rs`：
   - 取消 `TODO(#680)` 注释，恢复 `microcompact_messages(&mut self.messages)` 调用（sub-agent 用修复后的 flat 版本）
   - 清理后重新判断是否跳过 LLM compact 的逻辑恢复

**Sub-agent 策略**：
- Sub-agent 是自包含单次任务，不持有 ChatChain
- 使用修复后的 `microcompact_messages`（source-based turn detection），保护最近 2 轮

**验收**：
- 新增测试：`microcompact_chain` 保护最近 3 segment 不折叠
- 新增测试：第 4 个及更早 segment 的探索类 ToolResult 被折叠
- 新增测试：非探索类 ToolResult 不受影响
- 新增测试：修复后的 `microcompact_messages` 不再把 ToolResult 当 turn 边界
- 恢复的两处调用在运行时实际生效
- `cargo test` + `cargo clippy` 通过

---

### 子 issue D：TUI 投影化 —— 删除 chat.messages 镜像 + ChatRequest 增量化

**依赖**：A + B + C
**可并行**：否

**目标**：TUI 不再持有 `chat.messages` 上下文镜像，`ChatRequest` 从全量消息改为只传新 user input。

**前置分析（TUI `chat.messages` 的 4 个消费方）**：

| # | 位置 | 用途 | 替代方案 |
|---|---|---|---|
| 1 | `app/slash.rs:76` `/context` | `messages.len()` | runtime chain 段数 / 消息数查询 |
| 2 | `app/slash.rs:207` `/stats` | `messages.len()` | 同上 |
| 3 | `effect/executor.rs:79` | `messages.is_empty()` 跳过空 save | runtime chain `is_empty()` 查询 |
| 4 | `app/update/spawn_context.rs:19` | `messages.clone()` 喂 runtime | **ChatRequest 增量化**（核心改动） |

**改动范围**：

1. `packages/sdk/src/chat.rs`：
   - `ChatRequest.messages: Vec<ChatMessage>` → `ChatRequest.user_input: Option<UserInput>`（只传本轮新输入）
   - `UserInput` 封装 user text + images + metadata（真实用户输入，非历史镜像）

2. `agent/features/runtime/src/core/client/trait_chat.rs`：
   - `chat_impl`：不再从 `ChatRequest.messages` 初始化 chain；runtime 用**已有 `current_chain`** + 新 `user_input` → `chain.start_new_segment()` + `chain.push(user_msg)`
   - 常驻 loop（resident actor）场景：首次 `chat()` 调用时初始化空 chain，后续调用追加

3. `apps/cli/src/tui/`：
   - `app/state/chat.rs`：删除 `ChatState.messages` 字段
   - `app/update/spawn_context.rs`：`SpawnContext.messages` 删除，改为只传 user input
   - `effect/session/processing.rs`：`ChatRequest` 构造从 `messages: ctx.messages` → `user_input: Some(ctx.user_input)`
   - `effect/session/resume.rs`：删除 `self.chat.messages = messages.clone()`
   - `app/update/ui_event.rs`：删除 7 处 `self.chat.messages = messages`
   - `app/slash.rs`：`/context` 和 `/stats` 的消息数改用 runtime 查询（或从 `model.conversation.timeline` 近似）
   - `effect/executor.rs`：`save_session_effect` 空会话判定改用 runtime chain 查询

4. `core/client/trait_session.rs`：
   - 删除死代码 `sync_current_messages_impl`

5. 事件流精简（**可选，视实际影响面**）：
   - `RuntimeStreamEvent` 的 7 个变体携带的 `messages: Vec<Message>` 是否可精简为增量信号——若 TUI 不再消费全量 `messages`，这些 payload 可以只传 count 或完全删除

**验收**：
- `ChatState.messages` 字段不再存在
- `ChatRequest` 不再携带全量消息历史
- TUI 通过事件流增量更新 `model.conversation.timeline`（只读渲染投影）
- resume 路径正常工作（runtime 从 `current_chain` 恢复）
- `cargo test` + `cargo clippy` 通过

---

### 子 issue E：收尾清理 + Guard + Verify

**依赖**：A + B + C + D
**可并行**：否

**改动范围**：
- 清理所有 `TODO(#680)` 残留注释
- 确认 `docs/design/01-outline.md` §会话历史单一真相 现状描述已更新
- 更新 `specs/runtime.md` 和 `specs/storage.md`（新增约束：runtime 持有 ChatChain 聚合、TUI 不持有消息镜像）
- 运行 `cargo clippy`，修复所有新增 warning
- 检查 `docs/design/02-architecture-guards.md` 白名单是否需要更新
- 端到端验收：microcompact 在真实多轮对话中按 segment 保护生效
- 端到端验收：resume 正常恢复多段会话
- Guard：故意制造违规（如在 TUI 重建消息镜像）验证守卫拦截（如适用）

**验收**：
- 无 `TODO(#680)` 残留注释
- `cargo test` + `cargo clippy` 通过
- specs / design 文档已同步
- 死代码已清除（`sync_current_messages_impl` 等）

---

## 依赖图

```
A (ChatChain 聚合化)
  └── B (segment 维护 + 旧 session 重分段)
        └── C (segment-aware microcompact)
              └── D (TUI 投影化)
                    └── E (收尾清理 + guard)
```

严格串行，无并行。每个子 issue 独立 PR、独立验证。依赖方向从内到外（domain → adapter），**NEVER** 反向。

## 不在本次范围（Out of Scope）

- `RuntimeStreamEvent` 变体精简为纯增量事件协议（子 issue D 标注为可选，视实际影响面决定是否纳入）
- TUI `conversation.timeline` 的增量维护机制重构（当前已有增量事件，只需确认不再被全量覆盖干扰）

## 风险

1. **`loop_runner.rs` 改动面大**（1900+ 行）：4 处 push + resume + compact 都需改为 chain 操作。建议在 worktree 中增量推进，每步 `cargo test` 验证。
2. **event.rs sink 变更**：从 7 处 `*guard = messages.clone()` 改为 loop 直接写 chain——需确保所有 sync 事件路径都覆盖。
3. **旧 session 兼容**：`from_flat_messages` 的 turn boundary 检测需覆盖 edge case（空消息、连续 user 消息、纯 ToolResult session）。
4. **Sub-agent microcompact 语义**：sub-agent 没有 ChatChain，用修复后的 flat 版本——需确保 source-based 检测在 sub-agent 上下文中正确工作。
5. **ChatRequest 增量化（子 issue D）**：`build_spawn_context` 改动影响 TUI → runtime 的核心通信契约，需确保 resume、reflection、multi-turn 等场景都正确。
