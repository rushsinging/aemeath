# #390 A1：持久化会话 actor + chat() 契约 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 runtime 对话循环从"每次 `chat()` spawn 一个跑完即死的 loop"改造为"启动时 spawn 一次、回合间阻塞等待输入、仅在会话关闭时退出"的常驻 actor。

**Architecture:** 当前 `process_chat_loop` 在完成回合（无 tool_call + EndTurn + 无排队输入）时 `break`（loop_runner.rs:574）。A1 把该 `break` 改为**阻塞等待下一条输入事件**再 `continue`；loop 仅在收到 **shutdown 信号**（输入通道关闭）时退出。为支持"空闲阻塞等待"，把 `input_events` 从「`Arc<Mutex<Vec<_>>>` buffer 轮询」升级为 **tokio mpsc 通道**（mid-turn drain = `try_recv` 全取；空闲 = `recv().await`）。`chat()` 契约从"每回合调一次、`ChatRequest.messages` 带本回合用户输入"改为"**启动时调一次**、`messages` 只带初始历史（resume）、首条及后续用户输入全部经 `input_events` 通道到达"。Cancel（Ctrl+C）语义从"break 退出 loop"改为"中止当前回合、回到空闲等待"。

**Tech Stack:** Rust, tokio (mpsc unbounded channel, CancellationToken), async-trait。涉及 crate：`runtime`（business/chat/looping + core/client）、`sdk`、`cli`（tui/effect/session）。

## Global Constraints

- **MUST** 全程在 worktree 实施，PR 回 main，NEVER 直接改 main（AGENTS.md Git 工作流）。
- **MUST** 每个 task 跑验证门禁：`cargo build -p <crate>` + 相关 `cargo test` + `cargo clippy`（rust-coding.md）。
- **MUST NOT** 手动调格式，用 `cargo fmt`。
- **MUST** 保持 wire / 持久化 JSON 格式不变（不在 A1 动序列化）。
- **行为等价边界**：A1 只改"输入到达机制 + loop 生命周期 + cancel 语义"。**不改** InputId / 批量归宿 / append_user_message 合并 / MessagesSync 退出 display（这些是 A2/A3）。A1 完成后回显仍走现有 MessagesSync 路径（保持可用），只是触发来源从"每回合 chat()"变为"常驻 loop 的回合"。
- **属于伞 issue #394**；设计源 `docs/superpowers/specs/2026-06-20-persistent-session-actor-design.md` §3.1。
- 终端验证：`echo '...' | AEMEATH_VERSION= RUST_LOG= cargo run -- -qv`（-qv 不覆盖 TUI 渲染，仅验非交互链路）。

---

## File Structure

| 文件 | 职责 / 改动 |
|---|---|
| `packages/sdk/src/tui.rs` | `ChatInputEventPort` trait 增加阻塞 `recv_next()`（返回 `Option<ChatInputEvent>`，`None`=通道关闭=shutdown）。 |
| `agent/features/runtime/src/business/chat/looping/input_gate.rs` | `InputEventDrainPort` 增加 `recv_next_input()`（阻塞）；`drain_input_events` 语义保持（非阻塞 try-drain）。新增空闲等待辅助 `await_idle_input`。 |
| `agent/features/runtime/src/core/client/event.rs` | `RuntimeInputEventDrainPort` 实现 `recv_next_input`（转发 sdk port）。 |
| `agent/features/runtime/src/business/chat/looping/loop_runner.rs` | 完成路径 `break`（:574）→ 空闲等待再 `continue`；cancel 路径 → 回空闲（不退 loop）；新增 shutdown 退出。 |
| `agent/features/runtime/src/core/client/trait_chat.rs` | `chat_impl`：loop 启动后若无 pending 用户回合则空闲等待首条；`messages` 仅作初始历史。 |
| `apps/cli/src/tui/effect/session/processing.rs` | `TuiInputEventPort` 改 mpsc 通道实现 `recv_next`；`spawn_processing` 改为"启动一次"语义。 |
| `apps/cli/src/tui/app/run_loop.rs` + `spawn_context.rs` | 启动时调一次 `chat()`；用户提交改为往 `input_events` 通道发事件（不再每次 spawn_processing）。 |
| `apps/cli/src/tui/app/state/chat.rs` | 持有 input_events 发送端 + processing handle 的单次生命周期。 |

> 测试基座：复用 loop_runner / queue.rs 现有 mock port（`MockQueue` / mock `InputEventDrainPort`），扩展支持 `recv_next` + shutdown。LLM 用现有测试 mock client（loop_runner 既有测试已具备）。

---

## Task 1：输入端口增加阻塞 `recv_next`（sdk + runtime trait）

**Files:**
- Modify: `packages/sdk/src/tui.rs`（`ChatInputEventPort` trait）
- Modify: `agent/features/runtime/src/business/chat/looping/input_gate.rs`（`InputEventDrainPort` trait + mock）
- Modify: `agent/features/runtime/src/core/client/event.rs`（`RuntimeInputEventDrainPort` impl）
- Test: `agent/features/runtime/src/business/chat/looping/input_gate.rs`（#[cfg(test)]）

**Interfaces:**
- Consumes: 现有 `ChatInputEvent`、`InputEventFuture<'a>`。
- Produces:
  - sdk `ChatInputEventPort::recv_next<'a>(&'a self) -> Pin<Box<dyn Future<Output = Option<ChatInputEvent>> + Send + 'a>>`（`None` = 通道关闭）。
  - runtime `InputEventDrainPort::recv_next_input<'a>(&'a self) -> InputEventOptFuture<'a>`（`type InputEventOptFuture<'a> = Pin<Box<dyn Future<Output = Option<ChatInputEvent>> + Send + 'a>>`）。

- [ ] **Step 1: 写失败测试** — mock port 的 `recv_next_input` 在有事件时返回 `Some`、通道空且关闭时返回 `None`。

```rust
// input_gate.rs #[cfg(test)] 内，新增：
#[tokio::test]
async fn test_recv_next_input_returns_event_then_none_on_close() {
    // MockInputPort: 用 tokio::sync::mpsc 支持 recv_next
    let (tx, port) = MockInputPort::new();
    tx.send(ChatInputEvent::UserMessage { text: "hi".into(), image_paths: vec![] }).unwrap();
    let first = port.recv_next_input().await;
    assert!(matches!(first, Some(ChatInputEvent::UserMessage { .. })));
    drop(tx); // 关闭通道
    let after_close = port.recv_next_input().await;
    assert!(after_close.is_none(), "通道关闭后返回 None=shutdown");
}
```

- [ ] **Step 2: 运行测试确认失败** — `cargo test -p runtime recv_next_input -- --nocapture` → FAIL（`recv_next_input` / `MockInputPort` 未定义）。

- [ ] **Step 3: 实现 trait 方法 + mock**

```rust
// input_gate.rs
pub type InputEventOptFuture<'a> = Pin<Box<dyn Future<Output = Option<ChatInputEvent>> + Send + 'a>>;

pub trait InputEventDrainPort: Clone + Send + Sync + 'static {
    fn drain_input_events<'a>(&'a self) -> InputEventFuture<'a>;
    /// 阻塞等待下一条输入；None = 通道关闭（shutdown）。
    fn recv_next_input<'a>(&'a self) -> InputEventOptFuture<'a>;
}

// #[cfg(test)] MockInputPort 用 Arc<tokio::Mutex<mpsc::UnboundedReceiver<ChatInputEvent>>>
```

```rust
// sdk/src/tui.rs — ChatInputEventPort 增加：
fn recv_next<'a>(&'a self) -> Pin<Box<dyn Future<Output = Option<ChatInputEvent>> + Send + 'a>>;
```

```rust
// core/client/event.rs — RuntimeInputEventDrainPort::recv_next_input：
fn recv_next_input<'a>(&'a self) -> InputEventOptFuture<'a> {
    Box::pin(async move {
        match &self.0 {
            Some(port) => port.recv_next().await,
            None => None, // 无 port = 立即 shutdown
        }
    })
}
```

- [ ] **Step 4: 运行测试确认通过** — `cargo test -p runtime recv_next_input` → PASS。

- [ ] **Step 5: 全量编译占位实现** — 其余 `ChatInputEventPort` 实现（TUI）先加 `recv_next` 临时 `unimplemented!()` 桩或最小实现，确保 `cargo build --workspace` 过（Task 5 替换真实现）。运行 `cargo build --workspace` → PASS。

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat(runtime): 输入端口增加阻塞 recv_next（持久化 actor 基建, #390 A1)"
```

---

## Task 2：loop 完成路径 break → 空闲等待 + shutdown 退出

**Files:**
- Modify: `agent/features/runtime/src/business/chat/looping/loop_runner.rs`（完成路径 :574 附近 + 新增空闲等待）
- Test: `agent/features/runtime/src/business/chat/looping/loop_runner.rs`（#[cfg(test)]，跨回合测试）

**Interfaces:**
- Consumes: Task 1 的 `recv_next_input`；现有 `drain_and_apply_gate`、`GateDecision`、`pending_input`、`messages`。
- Produces: 一个内部辅助 `async fn await_idle_input<I: InputEventDrainPort>(input_events: &I, pending: &mut PendingInputBuffer) -> IdleResult`，`enum IdleResult { Resumed, Shutdown }`：阻塞 `recv_next_input`，收到事件 push 进 `pending` 返回 `Resumed`，`None` 返回 `Shutdown`。

- [ ] **Step 1: 写失败测试** — 常驻 loop 跨回合：喂输入 A → 完成一回合 → 喂输入 B → 再完成一回合 → 关闭输入通道 → loop 退出。断言两回合都产生了 LLM 调用 / Done 事件。

```rust
#[tokio::test]
async fn test_loop_persists_across_turns_until_shutdown() {
    // 用 mock LLM（每次返回 EndTurn 无 tool）、mock input port（可投递 + 关闭）
    let (input_tx, ctx) = build_persistent_test_ctx(/* mock llm: 2 次 EndTurn */);
    input_tx.send(user("first")).unwrap();
    // 在另一 task 里：等首个 Done 后投递 second，再 drop 关闭
    let driver = tokio::spawn(async move {
        // 简化：用 sink 收集事件，见到第 1 个 Done 后 send(second)，第 2 个 Done 后 drop(input_tx)
    });
    process_chat_loop(ctx).await; // 应在 shutdown 后返回，不 hang
    // 断言：collected events 含 2 个 Done（两回合）
}
```

- [ ] **Step 2: 运行测试确认失败** — `cargo test -p runtime test_loop_persists_across_turns` → FAIL（当前 loop 第 1 回合后 break，只有 1 个 Done，且 mock input 的第 2 条永不被消费）。

- [ ] **Step 3: 实现空闲等待 + 改完成路径**

```rust
// loop_runner.rs — 新增辅助：
enum IdleResult { Resumed, Shutdown }

async fn await_idle_input<I: InputEventDrainPort>(
    input_events: &I,
    pending: &mut PendingInputBuffer,
) -> IdleResult {
    match input_events.recv_next_input().await {
        Some(event) => { pending.push(event); IdleResult::Resumed }
        None => IdleResult::Shutdown,
    }
}
```

```rust
// 完成路径（原 loop_runner.rs:574 的 `finish_completed_loop(...); break;`）改为：
finish_completed_loop(&outcome, &sink, &turn_context, &task_store).await;
// 不再 break：进入空闲，阻塞等待下一条输入
loop_fsm.transition(ChatLoopTransition::Idle); // 若 FSM 无 Idle 态，新增之
match await_idle_input(&input_events, &mut pending_input).await {
    IdleResult::Resumed => {
        // drain 该 idle 收到的事件并 append（复用 apply_gate 的 append 逻辑）
        let gate = apply_gate(GateKind::BeforeLlm, &mut pending_input, &sink, &mut messages).await;
        if matches!(gate.decision, GateDecision::AbortCurrentLoop | GateDecision::CancelCurrentLoop) {
            // idle 期间收到 Cancel/abort：保持空闲，继续等下一条
            continue;
        }
        loop_fsm.transition(ChatLoopTransition::ResumeRunning);
        continue;
    }
    IdleResult::Shutdown => break, // 通道关闭 → 退出常驻 loop
}
```

> 注意：原"答完有排队输入 → ContinueNextTurn → continue"分支（:546-552）保留不变（非阻塞 drain 已拿到输入时直接继续，不进 idle 等待）。只有"无排队输入"才进 `await_idle_input`。

- [ ] **Step 4: 运行测试确认通过** — `cargo test -p runtime test_loop_persists_across_turns` → PASS（2 个 Done，shutdown 后返回）。

- [ ] **Step 5: 回归既有 loop 测试** — `cargo test -p runtime` → 既有 315 测试全过（确认未破坏单回合行为；单回合测试需在末尾 drop input 触发 shutdown，必要时调整测试夹具）。

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat(runtime): loop 完成后空闲等待输入、shutdown 退出（常驻 actor 核心, #390 A1)"
```

---

## Task 3：cancel 语义改为"中止当前回合 → 回空闲"（不退 loop）

**Files:**
- Modify: `agent/features/runtime/src/business/chat/looping/loop_runner.rs`（cancel 检查点 :185-226 / :296 / :618 / :659）
- Test: `loop_runner.rs` #[cfg(test)]

**Interfaces:**
- Consumes: `cancel: CancellationToken`、`await_idle_input`（Task 2）。
- Produces: cancel 处理后**重置 token**（`cancel` 需可重建——见下）并回空闲，而非 break。

- [ ] **Step 1: 写失败测试** — 回合进行中 cancel → 发出 `Cancelled` 事件、回滚本回合消息、**回到空闲**（不退 loop）；之后再喂新输入 → 能正常处理新回合；最后 shutdown → 退出。

```rust
#[tokio::test]
async fn test_cancel_aborts_turn_then_returns_to_idle() {
    // mock llm 第1回合可被 cancel；token.cancel() 触发后断言 Cancelled 事件
    // 再投递新输入，断言第2回合 Done；最后 drop input -> 退出
}
```

- [ ] **Step 2: 运行测试确认失败** — 当前 cancel 路径 `break`（:226/:296/:659），loop 直接退出，第 2 回合不会发生 → FAIL。

- [ ] **Step 3: 实现 cancel → 回空闲**

```rust
// cancel 检查点（原 break 处）改为：发出 Cancelled、回滚消息、重置 token、await_idle_input。
if cancel.is_cancelled() {
    messages.truncate(messages_at_start);
    sink.send_event(RuntimeStreamEvent::Cancelled { context }).await;
    // 重置取消令牌：A1 引入"会话级"可重建 token —— ctx.cancel 改为 Arc<Mutex<CancellationToken>>，
    // 此处 reset_cancel(&ctx_cancel) 生成新 token 供下回合；cancel_impl 也读该 Arc。
    reset_cancel(&shared_cancel);
    match await_idle_input(&input_events, &mut pending_input).await {
        IdleResult::Resumed => { /* apply_gate + continue，同 Task 2 */ continue; }
        IdleResult::Shutdown => break,
    }
}
```

> **取消令牌生命周期变更**：原 `current_cancel: Arc<Mutex<Option<CancellationToken>>>` 每回合在 chat_impl 重建。常驻后改为：loop 内部持有"当前回合 token"，cancel 后 reset 为新 token；`current_cancel`（供 `cancel_impl` 外部触发）指向同一可重建槽。`trait_accessor.rs::cancel_impl` 行为不变（仍 `token.cancel()`），但 token 由 loop 在每次 cancel/新回合时刷新。

- [ ] **Step 4: 运行测试确认通过** — `cargo test -p runtime test_cancel_aborts_turn_then_returns_to_idle` → PASS。

- [ ] **Step 5: 回归** — `cargo test -p runtime` 全过 + `cargo clippy -p runtime` 无 error。

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat(runtime): cancel 改为中止回合回空闲、令牌可重建（#390 A1)"
```

---

## Task 4：chat_impl 契约 —— 启动一次、首条经事件通道

**Files:**
- Modify: `agent/features/runtime/src/core/client/trait_chat.rs`（chat_impl）
- Test: `trait_chat.rs` #[cfg(test)] 或 `core/client/trait_reflection.rs` 同款集成测试

**Interfaces:**
- Consumes: Task 2/3 的常驻 loop；`ChatRequest { messages, queue_drain, input_events }`。
- Produces: chat_impl 语义 = "messages 作初始历史载入工作集；spawn 常驻 loop；loop 顶部若**无待响应的用户回合**（最后一条非待答 user 消息）则先 `await_idle_input`"。返回长生命周期 `ChatStream`。

- [ ] **Step 1: 写失败测试** — chat_impl 以空 messages 启动后 loop 不立即调 LLM（空闲等待）；经 input_events 投递首条后才产生回合。

```rust
#[tokio::test]
async fn test_chat_impl_idle_until_first_input_event() {
    // 构造空 messages + mock input port；chat()->stream
    // 断言：未投递前无 Token/Done；投递 user("hi") 后出现 Done
    // drop input port -> stream 结束
}
```

- [ ] **Step 2: 运行测试确认失败** — 当前 loop 以 messages 立即进入 LLM 调用；空 messages 会异常或空转 → FAIL。

- [ ] **Step 3: 实现 loop 顶部"无待答回合则空闲等待"**

```rust
// loop_runner.rs：在进入回合主体前（第一次 LLM 调用之前）判断：
fn has_pending_user_turn(messages: &[Message]) -> bool {
    // 最后一条是 user（待 assistant 响应）即视为有待答回合
    matches!(messages.last(), Some(m) if m.role == Role::User)
}
// loop 顶部（turn 初始化前）：
if !has_pending_user_turn(&messages) && pending_input.is_empty() {
    match await_idle_input(&input_events, &mut pending_input).await {
        IdleResult::Resumed => { apply_gate(BeforeLlm, ...).await; /* 落 messages */ }
        IdleResult::Shutdown => break,
    }
}
```

> chat_impl 不再要求 `ChatRequest.messages` 末尾是新用户输入；resume 时 messages=历史（末尾可能是 assistant，`has_pending_user_turn`=false → 空闲等首条）。

- [ ] **Step 4: 运行测试确认通过** — `cargo test -p runtime test_chat_impl_idle_until_first_input_event` → PASS。

- [ ] **Step 5: 回归 + -qv 冒烟** — `cargo test -p runtime` 全过；`echo 'say hi' | AEMEATH_VERSION= RUST_LOG= cargo run -- -qv` 能正常完成一轮（注：-qv 非交互模式如何投递首条见 Task 6 备注；若 -qv 仍走旧一次性路径，保留 `chat_text` 兼容，见下）。

> **`chat_text` 兼容**：非 TUI 一次性入口（`chat_text`）内部构造一个"投递单条 UserMessage 后立即关闭通道"的 input port，复用常驻 loop：投递→处理一回合→通道关闭→shutdown 退出。保证 `-qv` / SDK 一次性调用不破。

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat(runtime): chat() 契约改为启动一次+首条经事件通道，chat_text 兼容（#390 A1)"
```

---

## Task 5：TuiInputEventPort 改 mpsc 通道（支持 recv_next）

**Files:**
- Modify: `apps/cli/src/tui/effect/session/processing.rs`（`TuiInputEventPort`）
- Modify: `apps/cli/src/tui/app/state/chat.rs`（input_events 发送端持有）
- Test: `processing.rs` #[cfg(test)]

**Interfaces:**
- Consumes: sdk `ChatInputEventPort`（含 Task 1 的 `recv_next`）。
- Produces: `TuiInputEventPort` 内部 `Arc<tokio::Mutex<mpsc::UnboundedReceiver<ChatInputEvent>>>`；`recv_next` = `rx.recv().await`；`drain_input_events` = `try_recv` 全取。TUI 侧持有 `mpsc::UnboundedSender<ChatInputEvent>`。

- [ ] **Step 1: 写失败测试** — `TuiInputEventPort` 收到 sender 投递的事件，`recv_next` 返回之；sender drop 后 `recv_next` 返回 `None`。

```rust
#[tokio::test]
async fn test_tui_input_port_recv_next_and_close() {
    let (tx, port) = TuiInputEventPort::channel();
    tx.send(sdk::ChatInputEvent::UserMessage { text: "x".into(), image_paths: vec![] }).unwrap();
    assert!(port.recv_next().await.is_some());
    drop(tx);
    assert!(port.recv_next().await.is_none());
}
```

- [ ] **Step 2: 运行测试确认失败** — 当前 `TuiInputEventPort` 是 `Arc<Mutex<Vec<_>>>` buffer，无 `channel()` / `recv_next` → FAIL。

- [ ] **Step 3: 实现 mpsc 版 TuiInputEventPort**

```rust
pub struct TuiInputEventPort {
    rx: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<sdk::ChatInputEvent>>>,
}
impl TuiInputEventPort {
    pub fn channel() -> (mpsc::UnboundedSender<sdk::ChatInputEvent>, Self) {
        let (tx, rx) = mpsc::unbounded_channel();
        (tx, Self { rx: Arc::new(tokio::sync::Mutex::new(rx)) })
    }
}
impl sdk::ChatInputEventPort for TuiInputEventPort {
    fn recv_next<'a>(&'a self) -> Pin<Box<dyn Future<Output = Option<sdk::ChatInputEvent>> + Send + 'a>> {
        Box::pin(async move { self.rx.lock().await.recv().await })
    }
    fn drain_input_events<'a>(&'a self) -> ... { /* try_recv 循环全取 */ }
}
```

- [ ] **Step 4: 运行测试确认通过** — `cargo test -p cli test_tui_input_port_recv_next_and_close` → PASS。

- [ ] **Step 5: 编译** — `cargo build -p cli` → PASS（替换 Task 1 的桩实现）。

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat(cli): TuiInputEventPort 改 mpsc 通道支持 recv_next（#390 A1)"
```

---

## Task 6：TUI 启动调一次 chat()、提交改为发事件

**Files:**
- Modify: `apps/cli/src/tui/app/run_loop.rs`（启动处调一次 chat）
- Modify: `apps/cli/src/tui/app/update/spawn_context.rs` + `apps/cli/src/tui/app/update/key.rs`（提交 → 往 input_events 通道发，不再每次 spawn_processing）
- Modify: `apps/cli/src/tui/effect/session/processing.rs`（spawn_processing = 启动一次 + 持有 sender）
- Test: 纯逻辑可测部分加单测；TUI 交互留人工验

**Interfaces:**
- Consumes: Task 5 的 `TuiInputEventPort::channel()`、长生命周期 `ChatStream` 消费循环。
- Produces: App 持有 `input_tx: mpsc::UnboundedSender<ChatInputEvent>`；提交（非忙 & 忙）统一 `input_tx.send(ChatInputEvent::UserMessage{..})`；启动时 spawn 一次消费 + 一次 chat()。

- [ ] **Step 1: 写测试**（可测部分）— 提交动作产生一个发往 input_tx 的 `UserMessage` 事件（用 fake sender 断言）。首条与"忙时"提交走同一路径。

```rust
#[test]
fn test_submit_routes_user_message_to_input_channel() {
    // 注入 fake input_tx，模拟 update_enter / busy-enter，断言都 send UserMessage
}
```

- [ ] **Step 2: 运行确认失败** — 当前非忙提交走 `update_enter` → spawn_processing（新 chat），忙时才 send event → FAIL（两路不一致）。

- [ ] **Step 3: 实现统一提交 + 启动一次**

```rust
// run_loop.rs 启动：建 channel，spawn 一次 chat 消费循环
let (input_tx, input_port) = TuiInputEventPort::channel();
self.chat.input_tx = Some(input_tx);
let handle = processing::spawn_processing(SpawnContext {
    messages: self.chat.messages.clone(), // resume 历史；新会话为空
    input_port, queue_drain, ...
});
self.chat.set_processing_handle(handle);

// key.rs：非忙 & 忙提交统一：
if let Some(tx) = &self.chat.input_tx {
    let _ = tx.send(sdk::ChatInputEvent::UserMessage { text, image_paths });
}
// 删除"非忙时调 update_enter→spawn_processing 新 chat"分支。
```

> 退出 / `/clear` 等清理：会话结束时 drop `input_tx` → loop 收 shutdown 退出（与 Task 2 对齐）。

- [ ] **Step 4: 运行确认通过** — `cargo test -p cli test_submit_routes_user_message_to_input_channel` → PASS。

- [ ] **Step 5: 全量门禁** — `cargo build --workspace` + `cargo test -p sdk -p runtime -p cli` + `cargo clippy --workspace --all-targets` 全过。

- [ ] **Step 6: TUI 人工验（必须，标注给用户）**

```
手动跑 cargo run，逐项核对：
1. 启动后输入首条消息 → 正常回显 + 得到回复（首条经事件通道生效）。
2. agent 回复中插话（忙时提交）→ 排队 → 正常处理（mid-turn 不变）。
3. Ctrl+C 取消当前回合 → 显示 Cancelled、回到可输入状态，再输入新消息能继续（cancel 回空闲）。
4. resume 一个旧会话 → 历史正常显示，输入新消息能接续。
5. 退出（/exit）干净，无 hang。
```

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "feat(cli): TUI 启动调一次 chat、提交统一经事件通道（#390 A1)"
```

---

## 验收（A1 整体）

- **纯逻辑单测**：Task 1-5 的单测全过；核心是 Task 2「跨回合 + shutdown 退出」、Task 3「cancel 回空闲」、Task 4「空闲等首条」。
- **门禁**：`cargo build --workspace`、`cargo test -p sdk -p runtime -p cli`、`cargo clippy --workspace --all-targets` 全过。
- **`-qv` 冒烟**：`chat_text` 兼容路径一次性问答正常。
- **TUI 人工验**：Task 6 Step 6 五项（需用户在场）。
- **行为边界自检**：回显仍走现有 MessagesSync（A1 不动），首条/插话/取消/resume 行为与改造前等价（仅生命周期机制变）。

## 风险与回滚

- **风险**：常驻 loop 的空闲 await 若 shutdown 信号漏发会 hang（退出时未 drop input_tx）。Task 2/6 显式覆盖 shutdown=通道关闭。
- **风险**：cancel 令牌重建若与 `cancel_impl` 竞态可能漏取消。Task 3 用单一 `Arc<Mutex<CancellationToken>>` 槽消除竞态。
- **回滚**：A1 各 task 独立 commit；若 TUI 人工验暴露交互问题，可只回退 Task 6（TUI 接线），保留 runtime 常驻能力待修。

## 后续（非本计划）

- A2：InputId + 批量 `UserMessagesAdded` 归宿 + 单一 `append_user_message` + 移除内容去重。
- A3：MessagesSync 退出 display、回显只认归宿事件、占位按 id 清。
- A4：timeline 单一真相，删 legacy blocks。
- #391：`/clear` / abort 语义在常驻模型下统一（依赖 A1-A3）。
