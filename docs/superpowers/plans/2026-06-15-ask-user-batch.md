# AskUserQuestion 批量化重构 + 光标修复

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 AskUserQuestion 从单问模式重构为批量模式——多问一次性展示，用户依次作答，全部答完后显示确认页汇总，最终一次性回传所有答案。

**Architecture:** runtime 层收集同一 turn 内所有 AskUserQuestion tool calls，发单个 `AskUserBatch` 事件（携带 `Vec<AskUserQuestionItem>` + `oneshot::Sender<Vec<String>>`）。TUI 层用单个 `AskUserBatch` block 渲染多问 + 确认页状态机。用户确认后 TUI 通过 channel 一次性回传 `Vec<String>`，runtime 逐个 `send_tool_result`。

**Tech Stack:** Rust, tokio oneshot channel, ratatui, TEA architecture (Model-Update-View)

---

## 当前架构（要替换的）

### 事件流（单问、串行）
```
runtime ask_user():
  for call in ask_calls:
    send_event(AskUser { reply_tx })   // 发单个问题
    answer = reply_rx.await             // 阻塞等用户回答
    send_tool_result(call, answer)      // 回传结果
```

### 类型链
- `ChatEvent::AskUser { id, question, options, ..., reply_tx: Sender<String> }`
- `RuntimeStreamEvent::AskUser { ... }`
- `UiEvent::AskUser { ... }`
- `ConversationBlock::AskUser { question, options, cursor, selected, answer, ... }` (固定 id `"ask-user"`)
- `InputState { ask_user_state: Option<AskUserState>, ask_user_reply_tx: Option<Sender<String>> }`

### 问题
1. runtime 串行发问 → `show_ask_user` 先 `remove_ask_user_block` 再 push → 后一问覆盖前一问
2. Type something 子态用 `▏` 硬拼光标，与 input area 的块状光标不一致
3. 用户答完即回传，无确认页，无法回头修改

---

## 目标架构（批量化）

### 事件流（批量、一次性）
```
runtime ask_user():
  collect all ask_calls → items: Vec<AskUserQuestionItem>
  send_event(AskUserBatch { items, reply_tx: Sender<Vec<String>> })
  answers = reply_rx.await              // 等用户回答所有问题
  for (call, answer) in zip(ask_calls, answers):
    send_tool_result(call, answer)
```

### TUI 状态机
```
phase = Answering { active_index: 0 }
  用户作答 Q0 → answer Q0
  → Answering { active_index: 1 }
  ...
  用户作答 Q(N-1)
  → Confirming { confirm_cursor: N }  // 默认停在「全部确认提交」

Confirming:
  [↑↓] 移动 confirm_cursor (0..=N+1)
    cursor < N  → 选中某个 Q→A 项
    cursor == N → 选中「✓ 全部确认提交」
    cursor == N+1 → 选中「✗ 取消」
  [Enter]
    cursor < N  → Answering { active_index: cursor }  // 重新回答
      → 答完自动回 Confirming
    cursor == N → Confirmed → reply_tx.send(all_answers)
    cursor == N+1 → Cancelled → reply_tx.send(vec![""; N])
  [Esc] → Cancelled → reply_tx.send(vec![""; N])
```

### 渲染布局

**Answering 阶段**（显示当前问题 + 已答摘要）：
```
━━ 需要你的回答 (2/3) ━━

  ✓ 你喜欢什么语言？ → Rust

  你用什么编辑器？

    [↑↓] 选择  [Enter] 确认  [Esc] 取消

    ❯ 1. VS Code
      2. Vim
      3. Type something...
```

**Confirming 阶段**（汇总所有 Q→A + 内建操作项）：
```
━━ 请确认你的回答 ━━

  1. 你喜欢什么语言？ → Rust
  2. 你用什么编辑器？ → VS Code
  3. 你用什么 OS？ → macOS

  ❯ ✓ 全部确认提交
    ✗ 取消

  [↑↓] 导航（选中某项可重新回答）  [Enter] 确认  [Esc] 取消
```

**确认页交互规则**：
- 列表项 = N 个 Q→A 项 + 2 个内建 action（`✓ 全部确认提交` / `✗ 取消`）
- `confirm_cursor` 范围：`0..=N+1`，默认 `N`（停在「全部确认提交」）
- 光标在 Q→A 项 → Enter 切回 Answering 重答该项，答完自动回确认页
- 光标在「✓ 全部确认提交」→ Enter 提交，`reply_tx.send(all_answers)`
- 光标在「✗ 取消」（或 Esc）→ 回传空答案，取消整个 batch

**Type something 光标修复**：
- 移除 `Span::styled("▏", header_style)` 硬拼细竖线
- 改为在输入文本后追加 `Span::styled(" ", Style::default().bg(theme::ACCENT))` 块状光标（与 input area `set_cursor_style` 一致）

---

## 文件变更清单

### SDK 层
| 文件 | 操作 | 说明 |
|---|---|---|
| `packages/sdk/src/chat.rs` | 改 | 新增 `AskUserQuestionItem`；`ChatEvent::AskUser` → `ChatEvent::AskUserBatch` |

### Runtime 层
| 文件 | 操作 | 说明 |
|---|---|---|
| `agent/features/runtime/src/business/chat/looping/events.rs` | 改 | `RuntimeStreamEvent::AskUser` → `AskUserBatch` |
| `agent/features/runtime/src/business/chat/looping/ask_user.rs` | 改 | 重写为批量收集 + 单事件 + 批量回传 |

### TUI 事件映射层
| 文件 | 操作 | 说明 |
|---|---|---|
| `apps/cli/src/tui/app/event.rs` | 改 | `UiEvent::AskUser` → `UiEvent::AskUserBatch` |
| `apps/cli/src/tui/effect/session/processing.rs` | 改 | 映射 `ChatEvent::AskUserBatch` → `UiEvent::AskUserBatch` |

### TUI Model 层
| 文件 | 操作 | 说明 |
|---|---|---|
| `apps/cli/src/tui/model/conversation/block.rs` | 改 | `ConversationBlock::AskUser` → `AskUserBatch`（多问 + 状态机） |
| `apps/cli/src/tui/model/conversation/intent.rs` | 改 | 新增 batch intents |
| `apps/cli/src/tui/model/conversation/ask_user.rs` | 改 | model 层 batch 逻辑（show/answer/advance/confirm/navigate/cursor/toggle/chat_input） |
| `apps/cli/src/tui/model/conversation/ask_user_timeline.rs` | 改 | sync 函数改为 batch 版 |
| `apps/cli/src/tui/model/conversation/model.rs` | 改 | `apply()` intent 路由 |
| `apps/cli/src/tui/model/output_timeline/item.rs` | 改 | `OutputTimelineItem::AskUser` → batch 版 |

### TUI View 层
| 文件 | 操作 | 说明 |
|---|---|---|
| `apps/cli/src/tui/view_model/output.rs` | 改 | `AskUserBlockView` → `AskUserBatchBlockView` |
| `apps/cli/src/tui/view_assembler/output.rs` | 改 | timeline item → view 转换 |
| `apps/cli/src/tui/render/output/blocks/ask_user.rs` | 改 | 批量渲染 + 确认页 + 光标修复 |

### TUI App 层
| 文件 | 操作 | 说明 |
|---|---|---|
| `apps/cli/src/tui/app/state/ask_user.rs` | 改 | `AskUserState` → `AskUserBatchState` |
| `apps/cli/src/tui/app/state/input.rs` | 改 | `ask_user_state` / `ask_user_reply_tx` → `ask_user_batch_state` |
| `apps/cli/src/tui/app/update/ui_event.rs` | 改 | 处理 `UiEvent::AskUserBatch` |
| `apps/cli/src/tui/app/update/ask_user_key.rs` | 改 | batch 键盘交互（answering + confirming 两阶段） |
| `apps/cli/src/tui/app/update/notice.rs` | 改 | `show_ask_user_block` → batch 版 |
| `apps/cli/src/tui/app/update.rs` | 改 | 路由条件更新（Paste 分支） |
| `apps/cli/src/tui/app/runtime.rs` | 改 | 清理状态 |

---

## 任务分解

> **编译依赖说明**：Task 1-3 是跨 crate 类型变更，必须全部完成后才能编译通过。Task 4-8 是逐层实现，每个 task 做完后可以尝试编译。Task 9 是最终验证门禁。

### Task 1: SDK 类型层 — AskUserQuestionItem + ChatEvent::AskUserBatch

**Files:**
- Modify: `packages/sdk/src/chat.rs` (AskUser 变体 + 新增类型)

- [ ] **Step 1: 新增 `AskUserQuestionItem` 结构体**

在 `OptionItem` 定义之后插入：

```rust
/// AskUserQuestion 批量事件中的单个问题项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskUserQuestionItem {
    /// 对应的 tool_call_id（用于 TUI 关联 ToolCall 状态）。
    pub id: String,
    /// 问题文本。
    pub question: String,
    /// 预设选项（LLM 选项，不含内建选项）。
    pub options: Vec<OptionItem>,
    /// 是否多选。
    pub multi_select: bool,
    /// 默认值（用户跳过时使用）。
    pub default: Option<String>,
}
```

- [ ] **Step 2: 替换 `ChatEvent::AskUser` → `ChatEvent::AskUserBatch`**

在 `ChatEvent` enum 中，删除旧 `AskUser` 变体，替换为：

```rust
    /// AskUserQuestion 批量请求（一次携带多个问题）。
    AskUserBatch {
        items: Vec<AskUserQuestionItem>,
        /// 回传每个问题的答案（顺序与 items 一致）。
        reply_tx: tokio::sync::oneshot::Sender<Vec<String>>,
    },
```

- [ ] **Step 3: 更新 lib.rs re-export**

确认 `packages/sdk/src/lib.rs` 已 re-export `AskUserQuestionItem`。

- [ ] **Step 4: 更新 sdk 内测试**

删除引用 `ChatEvent::AskUser` 的旧测试（如有），新增 `AskUserQuestionItem` 字段验证测试。

- [ ] **Step 5: Commit**

```bash
git add packages/sdk/src/chat.rs packages/sdk/src/lib.rs
git commit -m "feat(sdk): AskUserQuestionItem + ChatEvent::AskUserBatch 替换单问 AskUser"
```

> ⚠️ 此 commit 后 runtime 和 TUI 编译会断。继续 Task 2-3 修复。

---

### Task 2: Runtime 事件层 — RuntimeStreamEvent::AskUserBatch

**Files:**
- Modify: `agent/features/runtime/src/business/chat/looping/events.rs:119-127`

- [ ] **Step 1: 替换 `RuntimeStreamEvent::AskUser` → `AskUserBatch`**

删除旧 `AskUser` 变体，替换为：

```rust
    AskUserBatch {
        items: Vec<sdk::AskUserQuestionItem>,
        reply_tx: tokio::sync::oneshot::Sender<Vec<String>>,
    },
```

- [ ] **Step 2: Commit**

```bash
git add agent/features/runtime/src/business/chat/looping/events.rs
git commit -m "feat(runtime): RuntimeStreamEvent::AskUserBatch 替换单问 AskUser"
```

---

### Task 3: Runtime 执行层 — ask_user() 批量化重写

**Files:**
- Modify: `agent/features/runtime/src/business/chat/looping/ask_user.rs` (全文重写)

- [ ] **Step 1: 重写 `ask_user()` 函数**

核心逻辑从"for 循环逐个发事件"改为"收集所有 calls → 发单个 batch 事件 → 收 Vec<String> → 逐个 send_tool_result"。

关键改动：
1. 过滤所有 `AskUserQuestion` calls
2. 对每个 call 运行 PermissionRequest hook
3. 收集所有问题为 `Vec<AskUserQuestionItem>`
4. 创建 `oneshot::channel::<Vec<String>>()`，发单个 `AskUserBatch` 事件
5. `reply_rx.await` 等待用户回答所有问题
6. 收到答案后，逐个 `send_tool_result` 回传

- [ ] **Step 2: Commit**

```bash
git add agent/features/runtime/src/business/chat/looping/ask_user.rs
git commit -m "feat(runtime): ask_user() 批量化——收集所有 calls 发单个 batch 事件"
```

> ⚠️ 此 commit 后 runtime 编译通过，但 TUI 仍编译失败。继续 Task 4。

---

### Task 4: TUI 事件映射层 — UiEvent + processing 映射

**Files:**
- Modify: `apps/cli/src/tui/app/event.rs:116-123`
- Modify: `apps/cli/src/tui/effect/session/processing.rs` (sdk_event_to_ui_event 映射)

- [ ] **Step 1: 替换 `UiEvent::AskUser` → `UiEvent::AskUserBatch`**

```rust
    /// AskUserQuestion 批量请求——一次携带多个问题。
    AskUserBatch {
        items: Vec<sdk::AskUserQuestionItem>,
        reply_tx: tokio::sync::oneshot::Sender<Vec<String>>,
    },
```

- [ ] **Step 2: 更新 `processing.rs` 映射**

```rust
        sdk::ChatEvent::AskUserBatch { items, reply_tx } => UiEvent::AskUserBatch {
            items,
            reply_tx,
        },
```

- [ ] **Step 3: Commit**

```bash
git add apps/cli/src/tui/app/event.rs apps/cli/src/tui/effect/session/processing.rs
git commit -m "feat(tui): UiEvent::AskUserBatch + 映射对齐"
```

---

### Task 5: TUI Model 类型层 — Block + Intent + Timeline + ViewModel

> ⚠️ 这是最大的类型变更 task，涉及 4 个文件，必须同时改完才能编译。

**Files:**
- Modify: `apps/cli/src/tui/model/conversation/block.rs` (AskUser 变体)
- Modify: `apps/cli/src/tui/model/conversation/intent.rs` (batch intents)
- Modify: `apps/cli/src/tui/model/output_timeline/item.rs` (AskUser 变体)
- Modify: `apps/cli/src/tui/view_model/output.rs` (AskUserBlockView)

- [ ] **Step 1: 新增辅助类型 `AskUserSlot` + `AskUserPhase`**

在 `block.rs` 中 `ConversationBlock` enum 之前新增：

```rust
/// AskUserQuestion 批量交互中的单个问题槽位。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AskUserSlot {
    pub id: String,
    pub question: String,
    /// 全部选项（LLM 选项 + 内建选项）。
    pub options: Vec<sdk::OptionItem>,
    /// LLM 选项数量（内建选项从该索引开始）。
    pub llm_option_count: usize,
    pub multi_select: bool,
    pub default: Option<String>,
    /// 用户回答。None=未答，Some=已答。
    pub answer: Option<String>,
}

/// AskUser 批量交互的阶段。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum AskUserPhase {
    /// 逐个回答中。
    Answering,
    /// 全部答完，等待确认。
    Confirming,
}
```

- [ ] **Step 2: `block.rs` — `ConversationBlock::AskUser` → `AskUserBatch`**

删除旧 `AskUser` 变体，替换为：

```rust
    /// AskUserQuestion 批量交互块（多问 + 确认页状态机）。
    AskUserBatch {
        id: String,
        /// 所有问题槽位。
        slots: Vec<AskUserSlot>,
        /// 当前激活的问题索引。
        active_index: usize,
        /// 交互阶段。
        phase: AskUserPhase,
        // ── 当前激活问题的选项导航状态 ──
        cursor: usize,
        selected: Vec<bool>,
        chat_input_active: bool,
        chat_input_text: String,
        /// 确认页导航光标。
        confirm_cursor: usize,
        /// 用户已确认提交。
        confirmed: bool,
    },
```

更新 `ConversationBlock::id()` 的 match 分支。

- [ ] **Step 3: `intent.rs` — 替换 batch intents**

删除旧 AskUser 相关 intents，替换为：

```rust
    ShowAskUserBatch { slots: Vec<super::block::AskUserSlot> },
    AnswerCurrentAskUser { answer: String },
    SetAskUserCursor { cursor: usize },
    ToggleAskUserSelected { index: usize },
    SetAskUserChatInput { active: bool },
    AppendAskUserChatChar { ch: char },
    DeleteAskUserChatChar,
    NavigateAskUserTo { index: usize },
    ConfirmAskUserBatch,
    DismissAskUserBatch,
```

- [ ] **Step 4: `output_timeline/item.rs` — `OutputTimelineItem::AskUser` → `AskUserBatch`**

```rust
    AskUserBatch {
        id: String,
        slots: Vec<crate::tui::model::conversation::block::AskUserSlot>,
        active_index: usize,
        phase: crate::tui::model::conversation::block::AskUserPhase,
        cursor: usize,
        selected: Vec<bool>,
        chat_input_active: bool,
        chat_input_text: String,
        confirm_cursor: usize,
        confirmed: bool,
    },
```

更新 `OutputTimelineItem::id()` 的 match 分支。

- [ ] **Step 5: `view_model/output.rs` — `AskUserBlockView` → `AskUserBatchBlockView`**

```rust
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AskUserBatchBlockView {
    pub key: String,
    pub slots: Vec<crate::tui::model::conversation::block::AskUserSlot>,
    pub active_index: usize,
    pub phase: crate::tui::model::conversation::block::AskUserPhase,
    pub cursor: usize,
    pub selected: Vec<bool>,
    pub chat_input_active: bool,
    pub chat_input_text: String,
    pub confirm_cursor: usize,
    pub confirmed: bool,
}
```

更新 `OutputBlockKind::AskUser(AskUserBlockView)` → `OutputBlockKind::AskUserBatch(AskUserBatchBlockView)`。

- [ ] **Step 6: Commit**

```bash
git add apps/cli/src/tui/model/conversation/block.rs \
       apps/cli/src/tui/model/conversation/intent.rs \
       apps/cli/src/tui/model/output_timeline/item.rs \
       apps/cli/src/tui/view_model/output.rs
git commit -m "feat(tui): AskUserBatch 类型定义——block/intent/timeline/viewmodel"
```

---

### Task 6: TUI Model 逻辑层 — ask_user.rs + timeline sync + model.rs apply

**Files:**
- Modify: `apps/cli/src/tui/model/conversation/ask_user.rs` (全文重写 model 逻辑)
- Modify: `apps/cli/src/tui/model/conversation/ask_user_timeline.rs` (sync 函数)
- Modify: `apps/cli/src/tui/model/conversation/model.rs` (apply intent 路由)

- [ ] **Step 1: 重写 `ask_user.rs` model 层逻辑**

核心方法：
- `show_ask_user_batch(slots)` — 初始化 block，active_index=0, phase=Answering
- `answer_current_ask_user(answer)` — 设 answer，自动前进或进入 Confirming
- `navigate_ask_user_to(index)` — 确认页导航回某项重新作答
- `set_ask_user_cursor(cursor)` / `toggle_ask_user_selected(index)` — 当前问题操作
- `set_ask_user_chat_input(active)` / `append_ask_user_chat_char(ch)` / `delete_ask_user_chat_char()` — Type something 子态
- `confirm_ask_user_batch()` — confirmed=true
- `dismiss_ask_user_batch()` — 移除块
- `ask_user_snapshot()` — 快照当前状态供 update 层读取

`AskUserSnapshot` 更新为 batch 版（包含 active_index, phase, cursor, selected, chat_input_active, confirm_cursor, llm_option_count, options_count, multi_select）。

- [ ] **Step 2: 重写 `ask_user_timeline.rs` sync 函数**

改为匹配 `ConversationBlock::AskUserBatch` → `OutputTimelineItem::AskUserBatch`，同步所有字段。

- [ ] **Step 3: 更新 `model.rs` `apply()` intent 路由**

将旧 AskUser intents 的 match 分支替换为 batch 版（10 个 intent 分支）。

- [ ] **Step 4: 添加 model 层单元测试**

```rust
    // 正常路径
    test_show_ask_user_batch_initializes_answering_phase
    test_answer_current_advances_to_next_question
    test_answer_last_question_enters_confirming_phase
    test_confirm_sets_confirmed_flag
    // 边界条件
    test_single_question_batch_answer_enters_confirming_immediately
    test_navigate_ask_user_to_resets_cursor_and_selected
    // 错误路径
    test_set_cursor_without_batch_is_noop
    test_toggle_builtin_option_is_noop
```

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/model/conversation/ask_user.rs \
       apps/cli/src/tui/model/conversation/ask_user_timeline.rs \
       apps/cli/src/tui/model/conversation/model.rs
git commit -m "feat(tui): AskUserBatch model 层逻辑——状态机 + timeline sync + 测试"
```

---

### Task 7: TUI View 层 — view_assembler + render + 光标修复

**Files:**
- Modify: `apps/cli/src/tui/view_assembler/output.rs:190-219`
- Modify: `apps/cli/src/tui/render/output/blocks/ask_user.rs` (全文重写)
- Modify: `apps/cli/src/tui/render/output/block_component.rs:50` (dispatch)

- [ ] **Step 1: 更新 `view_assembler/output.rs`**

将 `OutputTimelineItem::AskUser { ... }` 分支替换为 `OutputTimelineItem::AskUserBatch { ... }`，映射到 `OutputBlockKind::AskUserBatch(AskUserBatchBlockView { ... })`。

- [ ] **Step 2: 更新 `block_component.rs` dispatch**

```rust
            OutputBlockKind::AskUserBatch(ask) => blocks::ask_user::render_ask_user_batch(block_id, ask, ctx),
```

- [ ] **Step 3: 重写 `render/output/blocks/ask_user.rs`**

核心函数 `render_ask_user_batch`，按 `view.phase` 分支：

**Answering 阶段**：
- Header 显示进度 `(N/M)`
- 已答问题折叠为 `✓ Q → A` 摘要
- 当前激活问题 + 选项列表（复用现有 option_lines 逻辑）
- Type something 子态用块状光标（见下方光标修复）

**Confirming 阶段**：
- 列出所有 `Q → A` 项（`confirm_cursor < N` 时该项前加 `❯`）
- 2 个内建 action 项：
  - `✓ 全部确认提交`（`confirm_cursor == N` 时前加 `❯`，绿色）
  - `✗ 取消`（`confirm_cursor == N+1` 时前加 `❯`，红色）
- 底部 hint：`[↑↓] 导航（选中某项可重新回答）  [Enter] 确认  [Esc] 取消`

**光标修复**（核心）：
```rust
// 旧：Span::styled("▏", header_style)  ← 细竖线
// 新：与 input area 一致的块状光标
let cursor_style = Style::default().bg(theme::ACCENT).fg(theme::SURFACE);
Span::styled(" ", cursor_style)  // 空格 + ACCENT 背景色 = 块状光标
```

- [ ] **Step 4: 添加渲染单元测试**

```rust
    test_render_answering_shows_progress_header
    test_render_answering_shows_answered_summaries
    test_render_confirming_lists_all_qa
    test_render_chat_input_uses_block_cursor  // 验证不含 ▏，含 bg(ACCENT) span
```

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/tui/view_assembler/output.rs \
       apps/cli/src/tui/render/output/blocks/ask_user.rs \
       apps/cli/src/tui/render/output/block_component.rs
git commit -m "feat(tui): AskUserBatch 渲染——answering/confirming 双阶段 + 块状光标"
```

---

### Task 8: TUI App 层 — State + Update + 键盘交互

**Files:**
- Modify: `apps/cli/src/tui/app/state/ask_user.rs`
- Modify: `apps/cli/src/tui/app/state/input.rs`
- Modify: `apps/cli/src/tui/app/update/ui_event.rs`
- Modify: `apps/cli/src/tui/app/update/ask_user_key.rs`
- Modify: `apps/cli/src/tui/app/update/notice.rs`
- Modify: `apps/cli/src/tui/app/update.rs`
- Modify: `apps/cli/src/tui/app/runtime.rs`

- [ ] **Step 1: 重写 `state/ask_user.rs`**

```rust
pub(crate) const BUILTIN_OPTION_CHAT: &str = "Type something...";

pub(crate) struct AskUserBatchState {
    pub reply_tx: tokio::sync::oneshot::Sender<Vec<String>>,
    pub slots: Vec<crate::tui::model::conversation::block::AskUserSlot>,
    pub active_index: usize,
}
```

- [ ] **Step 2: 更新 `state/input.rs`**

删除 `ask_user_state` 和 `ask_user_reply_tx`，新增 `ask_user_batch_state: Option<AskUserBatchState>`。

- [ ] **Step 3: 重写 `update/ui_event.rs` — `UiEvent::AskUserBatch` 分支**

- 对每个 item `finish_tool_call`
- 构建 slots（追加 Type something 内建选项）
- `show_ask_user_batch(slots)`
- 存入 `ask_user_batch_state`

- [ ] **Step 4: 重写 `update/ask_user_key.rs` — batch 键盘交互**

按 `phase` 分路由：

**Answering 阶段**：Up/Down 移动 cursor，Space toggle（multi_select），Enter 确认（选项 → answer_current / Type something → 进入子态），Esc 取消整个 batch

**Confirming 阶段**：

```rust
fn update_ask_user_confirming_key(&mut self, key: KeyEvent) -> Option<UpdateResult> {
    let n = state.slots.len();
    match key.code {
        KeyCode::Up => { /* confirm_cursor = (confirm_cursor + N+1) % (N+2) */ }
        KeyCode::Down => { /* confirm_cursor = (confirm_cursor + 1) % (N+2) */ }
        KeyCode::Enter => {
            match confirm_cursor {
                i if i < n => {
                    // 切回 Answering 重答第 i 项
                    self.model.conversation.apply(ConversationIntent::NavigateAskUserTo { index: i });
                }
                n => {
                    // 全部确认提交
                    let state = self.input.ask_user_batch_state.take().unwrap();
                    let answers: Vec<String> = state.slots
                        .iter()
                        .map(|s| s.answer.clone().unwrap_or_default())
                        .collect();
                    self.model.conversation.apply(ConversationIntent::ConfirmAskUserBatch);
                    self.mark_output_dirty();
                    let _ = state.reply_tx.send(answers);
                    self.spinner_phase(SpinnerPhase::Generating);
                }
                n + 1 => {
                    // 取消
                    // 同 Esc 逻辑
                }
                _ => {}
            }
        }
        KeyCode::Esc => {
            // 取消整个 batch
            let state = self.input.ask_user_batch_state.take().unwrap();
            self.dismiss_ask_user_batch_block();
            let answers = vec![String::new(); state.slots.len()];
            let _ = state.reply_tx.send(answers);
            self.spinner_phase(SpinnerPhase::Generating);
        }
        _ => {}
    }
    Some(UpdateResult::none())
}
```

> 注意：从确认页切回 Answering 重答时，重答完毕（`answer_current_ask_user`）需要检查是否所有问题都已答——如果是，自动回到 Confirming；否则前进到下一个未答问题。NavigateAskUserTo 会重置 cursor/selected/chat_input，并保存当前已答值供用户参考。

- [ ] **Step 5: 更新 `update/notice.rs`**

`show_ask_user_block` → `show_ask_user_batch`，新增 `dismiss_ask_user_batch_block`。

- [ ] **Step 6: 更新 `update.rs` 路由条件**

`self.input.ask_user_state.is_some() || self.input.ask_user_reply_tx.is_some()` → `self.input.ask_user_batch_state.is_some()`。

- [ ] **Step 7: 更新 `runtime.rs` 清理状态**

`self.input.ask_user_batch_state = None;`

- [ ] **Step 8: Commit**

```bash
git add apps/cli/src/tui/app/
git commit -m "feat(tui): AskUserBatch app 层——state + update + 键盘交互状态机"
```

---

### Task 9: 编译 + clippy + 测试验证

- [ ] **Step 1: 全量编译**

```bash
cargo build
```

Expected: 0 errors。逐一修复类型不匹配的残留引用。

- [ ] **Step 2: clippy**

```bash
cargo clippy -- -D warnings
```

Expected: 0 warnings。

- [ ] **Step 3: 运行所有相关 crate 的测试**

```bash
cargo test -p aemeath-sdk
cargo test -p aemeath-runtime
cargo test -p aemeath-cli
```

Expected: 全部 PASS。

- [ ] **Step 4: 手动验证**

启动 TUI，触发包含 2+ AskUserQuestion 的场景：
- 两个问题依次出现，第一个答完后第二个自动出现
- 确认页展示所有 Q→A
- Enter 提交后 runtime 正常继续
- Esc 取消回传空答案
- Type something 光标为块状（与 input area 一致）

- [ ] **Step 5: 最终 Commit**

```bash
git add -A
git commit -m "test: AskUserBatch 全链路验证通过"
```

---

## 光标修复细节（Task 7 中实施）

### 问题
`render/output/blocks/ask_user.rs:228` 用 `Span::styled("▏", header_style)` 渲染细竖线光标。

### 修复
替换为与 input area 一致的块状光标：
```rust
let cursor_style = Style::default().bg(theme::ACCENT).fg(theme::SURFACE);
Span::styled(" ", cursor_style)
```

这与 `input_area/render.rs:116` 的 `set_cursor_style(Style::default().bg(theme::ACCENT).fg(theme::SURFACE))` 一致。

---

## Self-Review

### Spec coverage
- ✅ 多 AskUserQuestion 兼容：Task 1-3 runtime 批量化 + Task 5-8 TUI 批量化
- ✅ 确认页：Task 7 render confirming phase + Task 8 confirming key handler
- ✅ Type something 光标修复：Task 7 Step 3（块状光标替代 ▏）
- ✅ 向后兼容：单问 = batch size=1，统一走 batch 路径

### 风险点
1. **block.rs 需新增 `confirm_cursor` 字段**：Task 5 Step 2 的 block 定义中已包含
2. **旧 `ChatEvent::AskUser` 的所有引用**：grep 确认 `processing.rs` 是唯一映射点
3. **`OutputTimelineItem::AskUser` 的引用**：需检查 `output_timeline` 模块的其他 match 分支
4. **no_tui 模式**：`apps/cli/src/chat/no_tui.rs` 中可能有旧 AskUser 处理路径

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-06-15-ask-user-batch.md`. Two execution options:

**1. Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints

Which approach?
