# #390 A2 — InputId + 批量归宿事件 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 给每条用户提交分配 `InputId`、runtime 单一入口 append 并回带**批量** `UserMessagesAdded` 归宿事件、移除内容去重；TUI 分配并携带 id 但显示仍走 `MessagesSync`（零回归）。

**Architecture:** 沿用既有 UUIDv7 newtype（`impl_id_type!` 宏）新增 `InputId`；事件链 `RuntimeStreamEvent` → `ChatEvent` → `UiEvent` 各加一个 `UserMessagesAdded` 变体；A2 中 TUI 端 handler 为 no-op（A3 才消费）。

**Tech Stack:** Rust workspace（sdk / runtime / cli），UUIDv7（uuid crate），tokio mpsc，ratatui TUI。

## Global Constraints
- **MUST** wire / 持久化 JSON 格式不变（`InputId` serde 沿用单字符串格式，与 `ChatId` 一致）。
- **MUST** A2 显示路径不变：回显仍由 `MessagesSync` 驱动；新 `UserMessagesAdded` 在 TUI 端 **no-op**。零回归。
- **MUST** 不去重：移除 `seen_user_messages`；重复文本两条都 append。
- **NEVER** 在 A2 让 TUI 消费归宿事件 / 让 `MessagesSync` 退出 display（A3）。
- `InputId` 语义为「一次输入」，**NEVER** 复用 `ChatTurnId`。
- 验证门禁：`cargo clippy --all-targets --all-features`（0/0）、`cargo test --workspace`、`bash .agents/hooks/check-architecture-guards.sh`。worktree 内先 `source .cargo/set-target.sh`。

---

### Task 1：`InputId` newtype

**Files:**
- Modify: `packages/sdk/src/ids.rs`（仿 `ChatTurnId` 块）
- Modify: `packages/sdk/src/lib.rs:42`（导出）
- Test: `packages/sdk/src/ids.rs`（`#[cfg(test)] mod tests`）

**Interfaces:**
- Produces: `sdk::InputId`，方法 `new_v7()`、`from_legacy_or_new(&str)`、`parse_uuid7(&str)`、`as_str()`、`as_uuid()`；derive `Debug, Clone` + 宏给的 `PartialEq/Eq/Hash/Display/AsRef<str>/Serialize/Deserialize`。

- [ ] **Step 1: 写失败测试**（ids.rs tests 末尾）
```rust
    #[test]
    fn test_input_id_new_v7_is_version_7() {
        let id = InputId::new_v7();
        assert_eq!(id.as_uuid().get_version_num(), 7);
    }

    #[test]
    fn test_input_id_serde_roundtrip_preserves_uuid() {
        let original = InputId::new_v7();
        let json = serde_json::to_string(&original).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_string(), "expected string, got: {parsed}");
        let restored: InputId = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, original);
    }
```
- [ ] **Step 2: 运行确认失败** — `cargo test -p sdk input_id` → FAIL（`InputId` 未定义）
- [ ] **Step 3: 实现** — 复制 `ChatTurnId` 整块（含 `new_v7/new/parse_uuid7/from_legacy_or_new/as_uuid/as_str`）改名 `InputId`，末尾 `impl_id_type!(InputId);`；`lib.rs:42` 改为 `pub use ids::{ChatId, ChatTurnId, IdParseError, InputId, ToolCallId};`
- [ ] **Step 4: 运行确认通过** — `cargo test -p sdk input_id` → PASS
- [ ] **Step 5: 提交** — `git commit -m "feat(sdk): 新增 InputId UUIDv7 newtype (#390 A2)"`

---

### Task 2：归宿事件类型（端到端 additive，无人 emit）

**Files:**
- Modify: `packages/sdk/src/chat_event.rs`（`AddedInput` + `ChatEvent::UserMessagesAdded`）
- Modify: `agent/features/runtime/src/business/chat/looping/events.rs`（`RuntimeStreamEvent::UserMessagesAdded`）
- Modify: `agent/features/runtime/src/core/client/event.rs:~254`（转换 arm）
- Modify: `apps/cli/src/tui/effect/session/processing/event_mapping.rs`（→ `UiEvent::UserMessagesAdded`）
- Modify: `UiEvent` 枚举（grep `UiEvent::MessagesSync` 定位）+ `apps/cli/src/tui/app/update/ui_event.rs`（no-op handler）
- Modify: `apps/cli/src/chat/no_tui.rs:87`（穷尽 match 加忽略 arm）

**Interfaces:**
- Produces: `sdk::AddedInput { id: sdk::InputId, text: String }`（`#[derive(Debug, Clone, PartialEq, Eq)]`）；`sdk::ChatEvent::UserMessagesAdded { items: Vec<AddedInput> }`；`RuntimeStreamEvent::UserMessagesAdded { items: Vec<sdk::AddedInput> }`；`UiEvent::UserMessagesAdded(Vec<sdk::AddedInput>)`。

- [ ] **Step 1: 加 SDK 类型** — `chat_event.rs`：
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedInput {
    pub id: crate::InputId,
    pub text: String,
}
```
并在 `ChatEvent` 枚举加 `UserMessagesAdded { items: Vec<AddedInput> },`
- [ ] **Step 2: 加 runtime 事件** — `events.rs` 的 `RuntimeStreamEvent` 加 `UserMessagesAdded { items: Vec<sdk::AddedInput> },`
- [ ] **Step 3: 转换 arm** — `event.rs` 在 `MessagesSync` arm 旁加：
```rust
        crate::business::chat::RuntimeStreamEvent::UserMessagesAdded { items } => {
            ChatEvent::UserMessagesAdded { items }
        }
```
- [ ] **Step 4: TUI 映射 + no-op handler** — `event_mapping.rs` 加 `sdk::ChatEvent::UserMessagesAdded { items } => UiEvent::UserMessagesAdded(items),`；`UiEvent` 枚举加 `UserMessagesAdded(Vec<sdk::AddedInput>)`；`ui_event.rs` 加：
```rust
            // A2：仅建立通道，TUI 消费（按 id 清占位 + 回显）留待 A3。
            UiEvent::UserMessagesAdded(_items) => UpdateResult::none(),
```
- [ ] **Step 5: no_tui 穷尽 arm** — `no_tui.rs:87` 的忽略组加 `| sdk::ChatEvent::UserMessagesAdded { .. }`
- [ ] **Step 6: 编译** — `cargo build --workspace` → PASS（additive、无行为变化）
- [ ] **Step 7: 提交** — `git commit -m "feat(sdk): 新增 UserMessagesAdded 归宿事件类型(端到端 additive) (#390 A2)"`

---

### Task 3：`ChatInputEvent::UserMessage` 加 `id`

**Files:**
- Modify: `packages/sdk/src/chat.rs`（变体 + `user_message`/`classify_text`）
- Modify: `apps/cli/src/tui/app/update/enter.rs`（生成并携带 id）
- Modify: `apps/cli/src/tui/app/run_loop.rs:212`、`apps/cli/src/tui/effect/session/processing.rs`(test :431)、`agent/.../input_gate.rs`(匹配臂)
- Test: `packages/sdk/src/chat.rs`

**Interfaces:**
- Consumes: `sdk::InputId`（Task 1）。
- Produces: `ChatInputEvent::UserMessage { id: InputId, text: String, images: Vec<ToolResultImage> }`；`user_message`/`classify_text` 内部 `InputId::new_v7()`。

- [ ] **Step 1: 写失败测试**（chat.rs tests）
```rust
    #[test]
    fn test_user_message_generates_v7_input_id() {
        match ChatInputEvent::user_message("x", vec![]) {
            ChatInputEvent::UserMessage { id, .. } => {
                assert_eq!(id.as_uuid().get_version_num(), 7);
            }
            other => panic!("expected UserMessage, got {other:?}"),
        }
    }
```
- [ ] **Step 2: 运行确认失败** — `cargo test -p sdk user_message_generates` → FAIL（无 `id` 字段）
- [ ] **Step 3: 实现** — `UserMessage` 加 `id: crate::InputId`；`user_message`/`classify_text` 内 `id: crate::InputId::new_v7()`；改构造/匹配点：
  - `enter.rs submit_user_input_event`：`let event = sdk::ChatInputEvent::UserMessage { id: sdk::InputId::new_v7(), text: submission.text.clone(), images };`
  - `run_loop.rs:212`、`processing.rs:431`(test)：加 `id: sdk::InputId::new_v7(),`
  - `input_gate.rs` 两处匹配臂 `UserMessage { text, images }` → `UserMessage { id, text, images }`（`id` 供 Task 4；本 Task 暂 `let _ = id;` 或直接在 Task 4 用）
- [ ] **Step 4: 运行确认通过** — `cargo test -p sdk user_message_generates` + `cargo build --workspace` → PASS
- [ ] **Step 5: 提交** — `git commit -m "feat(sdk): ChatInputEvent::UserMessage 携带 InputId (#390 A2)"`

---

### Task 4：runtime 单一 append + 批量归宿 + 移除去重（A2 核心）

**Files:**
- Modify: `agent/features/runtime/src/business/chat/looping/input_gate.rs`
- Test: 同文件 `mod tests`

**Interfaces:**
- Consumes: `ChatInputEvent::UserMessage { id, text, images }`（Task 3）、`RuntimeStreamEvent::UserMessagesAdded`（Task 2）、`sdk::AddedInput`。
- Produces: `fn append_user_message(messages: &mut Vec<Message>, id: sdk::InputId, text: String, images: Vec<sdk::ToolResultImage>) -> sdk::AddedInput`（push 消息并返回 `AddedInput { id, text }`）。`PendingInputBuffer` 不再含 `seen_user_messages`。

- [ ] **Step 1: 写失败测试**（input_gate.rs tests）
```rust
    #[tokio::test]
    async fn test_apply_gate_emits_user_messages_added_batch_no_dedup() {
        let mut buffer = PendingInputBuffer::default();
        // 含重复文本：验证不去重
        let input = TestInputEventPort::new(vec![
            ChatInputEvent::user_message("same", Vec::new()),
            ChatInputEvent::user_message("same", Vec::new()),
        ]);
        let sink = TestSink::default();
        let mut messages = Vec::new();

        let outcome = run_loop_gate(
            GateKind::BeforeLlm, &mut buffer, &EmptyQueueDrainPort, &input, &sink, &mut messages,
        ).await;

        assert_eq!(outcome.appended_user_messages, 2, "不去重：两条都 append");
        assert_eq!(messages.len(), 2);
        let added = sink.events.lock().unwrap().iter().find_map(|e| match e {
            RuntimeStreamEvent::UserMessagesAdded { items } => Some(items.clone()),
            _ => None,
        });
        let items = added.expect("应发出一个 UserMessagesAdded 批事件");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].text, "same");
        assert_eq!(items[1].text, "same");
        assert_ne!(items[0].id, items[1].id, "每条提交一个独立 id");
    }
```
- [ ] **Step 2: 运行确认失败** — `cargo test -p runtime emits_user_messages_added` → FAIL（无该事件 / 仍去重）
- [ ] **Step 3: 实现**
  - 删 `PendingInputBuffer.seen_user_messages` 字段 + `should_accept`；`push` 改为直接 `self.events.push_back(event)`。
  - 抽 `append_user_message`（取代 `apply_gate` 内 `messages.push(user_message_with_images(...))`），返回 `AddedInput`。
  - `apply_gate`：用 `let mut added: Vec<sdk::AddedInput> = Vec::new();` 收集；`UserMessage { id, text, images }` arm 调 `added.push(append_user_message(messages, id, text, images));`（保留 `appended_this_gate`/计数）。
  - 循环结束、`appended_user_messages > 0` 分支内（`MessagesSync` emit **保留不动**）追加：`sink.send_event(RuntimeStreamEvent::UserMessagesAdded { items: added }).await;`
- [ ] **Step 4: 运行确认通过** — `cargo test -p runtime` → PASS（删/改任何依赖去重的旧测试）
- [ ] **Step 5: 提交** — `git commit -m "feat(runtime): 单一 append_user_message + 批量 UserMessagesAdded + 移除去重 (#390 A2)"`

---

### Task 5：门禁 + PR

- [ ] **Step 1: clippy** — `cargo clippy --all-targets --all-features`（0 warning / 0 error）
- [ ] **Step 2: 全量测试** — `cargo test --workspace`（全绿）
- [ ] **Step 3: 架构守卫** — `bash .agents/hooks/check-architecture-guards.sh`（全过）
- [ ] **Step 4: 同步 main** — `git fetch origin main` → `git merge refs/remotes/origin/main`（冲突解决后重跑门禁）
- [ ] **Step 5: PR** — push + `gh pr create`（base main），正文含根因/改动/门禁/TDD/「关联 #390 A2」+ A3 衔接说明。**NEVER 自动合并**。

## Self-review
- 设计 §3.2/§3.3/§4 A2 行全覆盖：InputId ✓ / 统一事件加 id ✓ / 单一 append ✓ / 批量 `UserMessagesAdded` ✓ / 移除去重 ✓ / 显示不变（MessagesSync 保留、TUI no-op）✓。
- 类型一致：`AddedInput`/`UserMessagesAdded` 仅在 sdk 定义，runtime/TUI 复用同一 sdk 类型（DRY）。
- 穷尽 match 覆盖：`event_mapping.rs`、`no_tui.rs` 均无 catch-all，Task 2 已显式加 arm。
