# 统一输入缓冲区 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **取代**：`docs/superpowers/plans/2026-06-20-390-a3-tui-purify.md`（旧 A3 计划，已废弃）。

**Goal:** 废弃 TUI 并行文本队列作为 runtime 输入源，统一为单一带 `InputId` 事件流；占位按 id 清 + 回显只认 `UserMessagesAdded`；`MessagesSync` 退出 display（仅镜像 + 落盘）。

**Architecture:** 输入唯一经 `ChatInputEvent` 事件通道到达 runtime `PendingInputBuffer`。`submit` 不再 `push_queue`；`ChatRequest.queue_drain: None`（`RuntimeQueueDrainPort` 包 `Option`，None→drain 返回 None，断源）。占位块已携 `input_id`（commit `d193f3ed`），由 `UserMessagesAdded` 按 id 清。`MessagesSync` 仅更新 `chat.messages` 镜像 + `SaveSession`。

**Tech Stack:** Rust，ratatui TUI（TEA `ConversationModel`），tokio mpsc 事件通道。

## Global Constraints
- **MUST** 输入单一路径：`submit` 只 `SendChatInputEvent`，**NEVER** 再 `push_queue` 喂 runtime。
- **MUST** 回显只来自 `UserMessagesAdded`（按 id）；`MessagesSync` 退出 display，仅保留 `self.chat.messages = msgs` + `Effect::SaveSession { notify: false }`。
- **MUST** busy-slash（`key.rs:174`）不再建 `QueuedUserMessage` 占位。
- **MUST** resume 路径（`effect/session/resume.rs::render_history_message`）不得改动。
- **NEVER** 在本计划做撤回/召回/`/clear` 统一（归 #391）。
- 验证门禁：`cargo clippy --all-targets --all-features`(0/0)、`cargo test --workspace`、`bash .agents/hooks/check-architecture-guards.sh`。cargo 前先 `source .cargo/set-target.sh`。
- 排序约束：Task 1（断 push_queue）**MUST** 先于 Task 4（MessagesSync 退出），否则双 append 复活；Task 3（按 id 回显）**MUST** 先于 Task 4。

---

### Task 1：submit 断开文本队列（消除双 append 根因）

**Files:**
- `apps/cli/src/tui/app/update/enter.rs:44`（`submit_user_input_event` 删 `push_queue`）
- `apps/cli/src/tui/effect/session/processing.rs:383`（`queue_drain: None`）
- Test: `apps/cli/src/tui/app/update/enter.rs` tests

**Interfaces:**
- Produces: `submit_user_input_event` 提交后 `self.input.input_queue` 为空；仅产出 `Effect::SendChatInputEvent`。

- [ ] **Step 1: 失败测试** — submit 后 `app.input.input_queue.is_empty()` 为真，且 effects 含一个 `SendChatInputEvent::UserMessage`。
- [ ] **Step 2: 确认失败** — `cargo test -p cli submit_does_not_push_queue` → FAIL（当前仍 push_queue）。
- [ ] **Step 3: 实现** — `submit_user_input_event` 删除 `self.input.push_queue(submission.text);`（保留 `enqueue_submission_echo(input_id, …)` 与 `SendChatInputEvent`）；`spawn_processing` 的 `ChatRequest` 改 `queue_drain: None`（移除 `TuiQueueDrainPort::new(...)` 实参，构造改为 `None`）。
- [ ] **Step 4: 确认通过** — `cargo test -p cli submit_does_not_push_queue` + `cargo build --workspace` → PASS。
- [ ] **Step 5: 提交** — `git commit -m "feat(cli): submit 断开文本队列，统一走事件通道 (#390 A3)"`

---

### Task 2：占位按 InputId 精确清除

**Files:**
- `apps/cli/src/tui/model/conversation/intent.rs:72`（加 `ClearQueuedSubmissionById { input_id }`）
- `apps/cli/src/tui/model/conversation/model.rs`（`clear_queued_submission_by_id` + intent 路由）
- `apps/cli/src/tui/app/update/notice.rs`（包装 `clear_queued_submission_echo_by_id`）
- Test: `apps/cli/src/tui/model/conversation/model_tests.rs`

**Interfaces:**
- Consumes: 占位块 `input_id`（已落地）。
- Produces: `fn clear_queued_submission_by_id(&mut self, input_id: &sdk::InputId) -> Vec<ConversationChange>`；`App::clear_queued_submission_echo_by_id(&mut self, input_id: &sdk::InputId)`。

- [ ] **Step 1: 失败测试** — 入队 3 条占位（input_id A/B/C），`clear_queued_submission_by_id(&B)` 后 `queued_submissions`/`blocks`/`timeline` 各只剩 A、C。
- [ ] **Step 2: 确认失败** — `cargo test -p cli clear_queued_by_id` → FAIL。
- [ ] **Step 3: 实现**：
```rust
fn clear_queued_submission_by_id(&mut self, input_id: &sdk::InputId) -> Vec<ConversationChange> {
    let before = self.queued_submissions.len();
    self.queued_submissions.retain(|q| &q.input_id != input_id);
    self.blocks.retain(|b| !matches!(b,
        ConversationBlock::QueuedUserMessage { input_id: bid, .. } if bid == input_id));
    self.timeline.retain(|it| !matches!(it,
        OutputTimelineItem::QueuedUserMessage { input_id: tid, .. } if tid == input_id));
    let removed = before - self.queued_submissions.len();
    vec![
        ConversationChange::QueuedSubmissionsCleared { count: removed },
        ConversationChange::OutputDirty,
    ]
}
```
  intent.rs 加 `ClearQueuedSubmissionById { input_id: sdk::InputId }`；model.rs `apply` 加路由；notice.rs 加 `clear_queued_submission_echo_by_id` 包装（仿现有 `clear_queued_submission_echo`）。
- [ ] **Step 4: 确认通过** — `cargo test -p cli clear_queued_by_id` → PASS。
- [ ] **Step 5: 提交** — `git commit -m "feat(cli): 占位块支持按 InputId 精确清除 (#390 A3)"`

---

### Task 3：翻转 UserMessagesAdded handler（按 id 清 + 顺序回显）

**Files:**
- `apps/cli/src/tui/app/update/ui_event.rs:99`
- Test: `apps/cli/src/tui/app/update/ui_event_tests.rs`

**Interfaces:**
- Consumes: Task 2 `clear_queued_submission_echo_by_id`；既有 `append_user_echo(text)`；`UiEvent::UserMessagesAdded(Vec<sdk::AddedInput>)`（`AddedInput { id: InputId, text: String }`）。

- [ ] **Step 1: 失败测试** — 入队占位 A/B；handler 收 `UserMessagesAdded([{id:A,"hi"},{id:B,"yo"}])` → A/B 占位清、按序追加两个正式 `UserMessage` 回显（"hi"/"yo"），无残留占位。
- [ ] **Step 2: 确认失败** — `cargo test -p cli user_messages_added_consumes` → FAIL（当前 no-op）。
- [ ] **Step 3: 实现**：
```rust
            UiEvent::UserMessagesAdded(items) => {
                for item in items {
                    self.clear_queued_submission_echo_by_id(&item.id);
                    self.append_user_echo(item.text);
                }
                self.mark_output_dirty();
                return UpdateResult::one(Effect::SaveSession { notify: false });
            }
```
- [ ] **Step 4: 确认通过** — `cargo test -p cli user_messages_added_consumes` → PASS。
- [ ] **Step 5: 提交** — `git commit -m "feat(cli): UserMessagesAdded 驱动按 id 清占位+顺序回显 (#390 A3)"`

---

### Task 4：MessagesSync 退出 display（仅镜像 + 落盘）

**Files:**
- `apps/cli/src/tui/app/update/ui_event.rs:100`
- Test: `apps/cli/src/tui/app/update/ui_event_tests.rs`

- [ ] **Step 1: 失败测试** — 含 user 输入的 `MessagesSync(msgs)` → handler 后**不产生** `UserMessage` 回显块、**不清**占位；但 `self.chat.messages == msgs`。
- [ ] **Step 2: 确认失败** — `cargo test -p cli messages_sync_no_display` → FAIL。
- [ ] **Step 3: 实现** — handler 删 `old_len` diff / `new_user_texts` / `clear_queued_submission_echo()` / `append_user_echo` 循环 / `input.clear_queue()`，仅留：
```rust
            UiEvent::MessagesSync(msgs) => {
                // A3：MessagesSync 退出 display，仅作镜像 + 落盘；
                // 用户回显改由 UserMessagesAdded 归宿事件驱动。
                self.chat.messages = msgs;
                return UpdateResult::one(Effect::SaveSession { notify: false });
            }
```
- [ ] **Step 4: 确认通过** — `cargo test -p cli messages_sync_no_display` + 更新受影响旧测试 → PASS。
- [ ] **Step 5: 提交** — `git commit -m "feat(cli): MessagesSync 退出 display，降级为镜像+落盘 (#390 A3)"`

---

### Task 5：busy-slash 不建占位 + Up 键移除队列召回分支

**Files:**
- `apps/cli/src/tui/app/update/key.rs:174`（busy-slash 删 `enqueue_submission_echo`）
- `apps/cli/src/tui/app/update/key.rs:224`（Up 键删 `input_queue` 召回分支）
- Test: `apps/cli/src/tui/app/update/key_tests.rs`

- [ ] **Step 1: 失败测试** — (a) busy + 提交 `/foo` → 产出 `ControlCommand` 事件且 `conversation` 无新增 `QueuedUserMessage` 块；(b) `input_queue` 非空时按 Up → 不再清占位/不恢复文本（回归光标/历史导航）。
- [ ] **Step 2: 确认失败** — `cargo test -p cli busy_slash_no_placeholder` → FAIL。
- [ ] **Step 3: 实现** — key.rs:174 删 `self.enqueue_submission_echo(...)`（保留 `push_queue`? 否——busy-slash 的 control command 经事件通道，亦不入文本队列；删 `push_queue` 调用，仅 `SendChatInputEvent { ControlCommand }` + 状态通知）；key.rs:221-231 的 Up 分支删除「`input_queue` 非空 → 恢复 + 清占位」中间支，保留 completion-prev 与 `MoveCursorUp`。
- [ ] **Step 4: 确认通过** — `cargo test -p cli busy_slash_no_placeholder` → PASS。
- [ ] **Step 5: 提交** — `git commit -m "feat(cli): busy-slash 不建占位 + Up 键移除队列召回 (#390 A3)"`

---

### Task 6：删除文本队列死路径（清理）

**Files:**
- `apps/cli/src/tui/app/update/ui_event.rs:250`（删 `UiEvent::DrainQueuedInput` 分支）+ `UiEvent` 枚举该变体
- `apps/cli/src/tui/effect/session/processing.rs`（删 `TuiQueueDrainPort` 及其测试 `test_drain_queued_input_*`）
- `apps/cli/src/tui/app/state/input.rs`（评估 `input_queue`/`push_queue`/`drain_queue` 是否仍有非 runtime 消费者；无则删，有则保留并注释）
- Test: 现有套件

**Interfaces:**
- 说明：runtime 侧 `RuntimeQueueDrainPort`/`QueueDrainPort`/`drain_sources` 队列分支**本计划不删**（`queue_drain: None` 下恒为 no-op，无害）；彻底删除 runtime 队列 plumbing 作为**后续清理**（另开 issue，避免本 PR 触及 loop_runner 多签名）。

- [ ] **Step 1: 确认无残留消费者** — `grep -rn "DrainQueuedInput\|TuiQueueDrainPort\|push_queue\|drain_queue\|input_queue" apps/cli/src` 核对仅剩待删点（及 input_queue 若仍被 status/其它用则保留）。
- [ ] **Step 2: 删除** — 移除 `UiEvent::DrainQueuedInput` 变体 + handler + `TuiQueueDrainPort` struct/impl + 其单测；按 Step 1 结果处理 `input_queue`。
- [ ] **Step 3: 验证** — `cargo build --workspace` + `cargo test -p cli` → PASS（删测试后无悬挂引用）。
- [ ] **Step 4: 提交** — `git commit -m "chore(cli): 删除文本队列死路径（DrainQueuedInput/TuiQueueDrainPort）(#390 A3)"`

---

### Task 7：门禁 + PR

- [ ] **Step 1: clippy** — `cargo clippy --all-targets --all-features`（0/0）
- [ ] **Step 2: 全量测试** — `cargo test --workspace`（全绿）
- [ ] **Step 3: 架构守卫** — `bash .agents/hooks/check-architecture-guards.sh`（全过）
- [ ] **Step 4: 同步 main** — `git fetch origin main` → `git merge refs/remotes/origin/main`（冲突解决后重跑门禁）
- [ ] **Step 5: 删旧 A3 计划** — `git rm docs/superpowers/plans/2026-06-20-390-a3-tui-purify.md`（已被本计划取代）
- [ ] **Step 6: PR** — push + `gh pr create`（base main），正文含目标/改动/门禁/TDD/「关联 #390 A3」+ **MUST 提示用户做 TUI 验收**（首条/busy 插话回显、占位按 id 清、不重不漏不错位、resume 历史、busy 连发、busy-slash 行为）+ 「runtime 队列 plumbing 死路径留待后续清理」说明。**NEVER 自动合并**。

## Self-review
- spec §3.1 单一路径 → Task 1（断 push_queue + queue_drain None）+ Task 6（删死路径）✓
- spec §3.2 占位按 id 清 + busy-slash 不建占位 → Task 2/3 + Task 5 ✓
- spec §3.3 MessagesSync 退出 display → Task 4 ✓
- spec §3.4 Up 键 → Task 5 ✓
- spec §4 撤回/召回/clear 归 #391：本计划未涉及 ✓
- 排序：Task 1 先于 4、Task 3 先于 4（Global Constraints 已固化）✓
- 类型一致：`clear_queued_submission_by_id`/`clear_queued_submission_echo_by_id`/`ClearQueuedSubmissionById` 全程同名 ✓
