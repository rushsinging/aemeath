# Issue #943 + #944 5B 分阶段实施计划

> 对应：#943（主 PR）、#944（5B 退役并入）、#1246（Runtime Main suspension 生产切换依赖）。
> 基线：`origin/main` `e9b415f5`，已包含 #1359 AskUser 输入与恢复一致性修复。

**目标：** 在一个 PR 内建立唯一 `sdk::ChatEvent → TuiRuntimeEvent → AgentIntent → reducer` 链，同时完成 #944 5B 旧 TUI sender / SDK DTO 路径退役。

**关键约束：**

- `adapter/event_mapping.rs` 是唯一 SDK `ChatEvent` 穷尽转换点。
- Runtime stream 只发送纯值 `TuiRuntimeEvent`；本地 Effect 回灌只发送纯值 `LocalUiEvent`。
- `AskUserBatch.reply_tx` 不进入任何 TUI DTO、Intent、Model 或 View；#1246 生产切换完成前，删除旧 bridge 会破坏运行时交互，因此本 PR 必须以 Runtime 已发布的 `InteractionRequested` 为唯一展示路径，不能保留双发。
- 保留 #1359：Ask 自由输入留在 Ask block；已完成 Ask 批次保留；答案按 `tool_call_id + question_seq` 匹配和回传。
- 每阶段可单独提交，但不拆 Issue、不拆 PR。

---

## 阶段 1：纯值消息与队列边界

**目标：** TUI Conversation 只消费 `runtime_view::TuiChatMessage`、`TuiContentBlock` 与字符串 input identity，消除 SDK 消息 DTO 回流。

**范围：**

- `runtime_view.rs`：保留完整消息、内容块、Stop Hook metadata、图片和 input identity。
- `conversation/{intent,model,history_parse,stop_hook_notice,queued_submission}.rs`：改为纯值消息 / ID。
- `effect/session/resume.rs`：恢复历史只消费 `TuiChatMessage`。
- `app/event.rs` 与旧 mapper 的过渡消息快照先转换为 `TuiChatMessage`，不允许二次 SDK→TUI converter。
- 保留 #1359 的 completed Ask block 恢复逻辑与 `question_seq`。

**退役门槛：** `ConversationIntent`、timeline queue、history parser、Stop hook notice、resume 路径均不含 `sdk::ChatMessage` / `sdk::ContentBlock` / `sdk::InputId`。

**验证：**

```bash
cargo test -p cli tui::model::conversation -- --nocapture
cargo test -p cli tui::effect::session::resume -- --nocapture
cargo check -p cli
```

## 阶段 2：Runtime stream 与第二层 ACL

**目标：** processing mpsc 发送 `TuiRuntimeEvent`；`agent_event` 只匹配 TUI DTO 并产生 Intent。

**范围：**

- `effect/session/processing.rs` / `handle.rs`：Runtime sender 改为 `mpsc::Sender<TuiRuntimeEvent>`；网络错误以纯值 Runtime error DTO 回传。
- `update/msg.rs`：新增 `TuiMsg::Runtime(TuiRuntimeEvent)`。
- `adapter/agent_event.rs`：替换 `UiEvent` 输入；Run、RunStep、Interaction、Workspace、Hook、Progress、message、usage 等所有变体显式映射 Intent。
- `adapter/{hook_notice,agent_event/progress}.rs`：消费 TUI-owned Hook / Progress 值类型。
- 删除 `effect/session/processing/event_mapping.rs` 与其测试中的 SDK→UiEvent converter。

**退役门槛：** production `sdk::ChatEvent` match 仅在 `adapter/event_mapping.rs`；`agent_event.rs` 零 `sdk::`、零 Effect、零 I/O、零 wildcard 静默丢弃。

**验证：**

```bash
cargo test -p cli tui::adapter::event_mapping -- --nocapture
cargo test -p cli tui::adapter::agent_event -- --nocapture
cargo test -p cli tui::update::root_reducer -- --nocapture
cargo check -p cli
```

## 阶段 3：App 本地回灌与 Runtime 分发分离

**目标：** Runtime observation 不再进入 `UiEvent` / `update_ui`；后者仅处理本地 Effect 回灌。

**范围：**

- `app/run_loop.rs`：独立 Runtime / Local channel，在 `tokio::select!` 中分别转为 `TuiMsg::Runtime` / `TuiMsg::Ui`。
- `app/update.rs`：新增 `update_runtime_event`，先经第二层 ACL/reducer，再执行仅属 Runtime 的 UI shell 反馈；本地 `update_ui` 不再匹配 Runtime event。
- `effect/executor.rs`、slash、clipboard、update check：保留 / 迁移为 `LocalUiEvent` 纯值回灌。
- scenario harness：分别注入 Runtime 与 Local event。

**退役门槛：** `UiEvent` 不持 Runtime SDK DTO；`update_ui` 不处理 Runtime stream；Runtime 事件不走 `TuiMsg::Ui`。

**验证：**

```bash
cargo test -p cli tui::app -- --nocapture
cargo test -p cli tui::adapter::agent_event -- --nocapture
cargo check -p cli
```

## 阶段 4：#944 5B AskUser / sender / 旧状态路径退役

**前置：** 阶段 2、3 完成，Runtime `InteractionRequested` 已可经 ACL 映射 `ShowInteraction`。若 #1246 尚未发布 Main suspension 事件，必须停止在该依赖处，不能删除运行中 sender bridge。

**范围：**

- 用 `InteractionState` 与 `ReplyInteraction` / `CancelInteraction` Effect 取代 `AskUserBatch` 展示和 `reply_tx`。
- 删除 `AskUserState`、`InputState` sender、`UiEvent::AskUserBatch`、旧 AskUser key reply、旧 scenario bridge。
- 移除旧 ChatStatus/spinner 旁路，使 Run lifecycle 投影唯一决定执行状态。
- 保持 #1359 的 UI 语义：block 内自由输入、完成摘要稳定、question identity 对齐；其实现转为 Interaction body 的等价行为。

**退役门槛：** TUI 生产目录零 `AskUserBatch`、`AskUserReply`、`reply_tx`、`oneshot::Sender<sdk::...>`；不保留双交互链。

**验证：**

```bash
cargo test -p cli tui::model::conversation::interaction -- --nocapture
cargo test -p cli tui::app::scenario_tests::interaction -- --nocapture
cargo test -p cli tui::effect::executor_interaction_tests -- --nocapture
cargo check -p cli
```

## 阶段 5：Guard、文档与完整验收

**目标：** 固化唯一转换点、纯值边界与 #944 5B 退役证据。

**范围：**

- 增加 L0 guard：唯一 `sdk::ChatEvent` match、Tui DTO / mapper / Model / View / Render 零 SDK DTO、sender、waiter、registry。
- 增加 guard 负例：SDK DTO 泄漏、converter I/O、lifecycle 空折叠、mapper Effect、legacy sender 均必须失败。
- 回写 #943 / #944 / migration governance / TUI event-flow 文档；#944 仅记录交接与验收，用户确认后关闭。

**完整验证：**

```bash
cargo fmt --all -- --check
cargo test -p cli
cargo test -p sdk interaction -- --nocapture
cargo check -p cli -p sdk
cargo clippy -p cli -p sdk --all-targets -- -D warnings
PATH="/opt/homebrew/bin:$PATH" bash .agents/hooks/check-tui-model-view-boundaries.sh
PATH="/opt/homebrew/bin:$PATH" bash .agents/hooks/check-tui-effect-boundary.sh
PATH="/opt/homebrew/bin:$PATH" bash .agents/hooks/check-architecture-guards.sh --full
git diff --check
```
