# Bug 114 Stop Hook Chat Loop FSM Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Stop hook blocking semantics explicit: blocked prevents final stop, feeds mandatory requirements back to the LLM, and only Stop hook success reaches Done.

**Architecture:** Add a small hand-written chat-loop FSM in runtime business code and thread it through existing loop boundaries without introducing a framework. Keep HookRunner, provider streaming, and TUI event protocols unchanged; improve Stop hook feedback text and tests around the existing control flow.

**Tech Stack:** Rust, Tokio, existing runtime/chat loop modules, existing hook API, existing cargo test + project hook scripts.

---

## File Map

- Create: `agent/features/runtime/src/business/chat/looping/state.rs`
  - Defines `ChatLoopState`, `ChatLoopTransition`, and `ChatLoopFsm`.
  - Contains pure FSM transition tests.

- Modify: `agent/features/runtime/src/business/chat/looping.rs`
  - Adds `mod state;` and re-exports FSM types for local runtime use/tests.

- Modify: `agent/features/runtime/src/business/chat/looping/loop_runner.rs`
  - Instantiates `ChatLoopFsm`.
  - Records explicit state transitions at compact, tool, gate, stopping, stop-blocked, and done boundaries.
  - Preserves existing `continue` / `break` behavior.

- Modify: `agent/features/runtime/src/business/chat/looping/finalize.rs`
  - Strengthens Stop hook blocked feedback text.
  - Adds/updates unit tests for mandatory blocked semantics.

- Modify: `docs/bug/active.md`
  - Updates Bug #114 title/root cause from “blocking is meaningless” to “blocked semantics are implicit and lack FSM/strong feedback”.

- Already created spec: `docs/superpowers/specs/2026-06-03-bug114-stop-hook-chat-loop-fsm.md`

---

### Task 1: Add Pure Chat Loop FSM

**Files:**
- Create: `agent/features/runtime/src/business/chat/looping/state.rs`
- Modify: `agent/features/runtime/src/business/chat/looping.rs`

- [ ] **Step 1: Create failing FSM tests and type skeleton**

Create `agent/features/runtime/src/business/chat/looping/state.rs` with:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatLoopState {
    Running,
    AwaitingTool,
    AwaitingUser,
    Compacting,
    Stopping,
    StopHookBlocked,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatLoopTransition {
    StartTurn,
    AwaitTool,
    AwaitUser,
    Compact,
    TryStop,
    StopBlocked,
    StopSucceeded,
    ResumeRunning,
    AbortCurrentLoop,
    CancelCurrentLoop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChatLoopFsm {
    state: ChatLoopState,
}

impl Default for ChatLoopFsm {
    fn default() -> Self {
        Self {
            state: ChatLoopState::Running,
        }
    }
}

impl ChatLoopFsm {
    pub fn state(&self) -> ChatLoopState {
        self.state
    }

    pub fn transition(&mut self, _transition: ChatLoopTransition) -> ChatLoopState {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_loop_state_stop_hook_blocked_must_resume_before_done() {
        let mut fsm = ChatLoopFsm::default();

        assert_eq!(fsm.state(), ChatLoopState::Running);
        assert_eq!(
            fsm.transition(ChatLoopTransition::TryStop),
            ChatLoopState::Stopping
        );
        assert_eq!(
            fsm.transition(ChatLoopTransition::StopBlocked),
            ChatLoopState::StopHookBlocked
        );
        assert_eq!(
            fsm.transition(ChatLoopTransition::ResumeRunning),
            ChatLoopState::Running
        );
        assert_eq!(
            fsm.transition(ChatLoopTransition::TryStop),
            ChatLoopState::Stopping
        );
        assert_eq!(
            fsm.transition(ChatLoopTransition::StopSucceeded),
            ChatLoopState::Done
        );
    }

    #[test]
    fn test_chat_loop_state_tool_and_user_boundaries_resume_running() {
        let mut fsm = ChatLoopFsm::default();

        assert_eq!(
            fsm.transition(ChatLoopTransition::AwaitTool),
            ChatLoopState::AwaitingTool
        );
        assert_eq!(
            fsm.transition(ChatLoopTransition::AwaitUser),
            ChatLoopState::AwaitingUser
        );
        assert_eq!(
            fsm.transition(ChatLoopTransition::ResumeRunning),
            ChatLoopState::Running
        );
    }

    #[test]
    fn test_chat_loop_state_abort_and_cancel_enter_done_from_any_state() {
        let mut aborting = ChatLoopFsm::default();
        assert_eq!(
            aborting.transition(ChatLoopTransition::AwaitTool),
            ChatLoopState::AwaitingTool
        );
        assert_eq!(
            aborting.transition(ChatLoopTransition::AbortCurrentLoop),
            ChatLoopState::Done
        );

        let mut cancelling = ChatLoopFsm::default();
        assert_eq!(
            cancelling.transition(ChatLoopTransition::TryStop),
            ChatLoopState::Stopping
        );
        assert_eq!(
            cancelling.transition(ChatLoopTransition::CancelCurrentLoop),
            ChatLoopState::Done
        );
    }
}
```

- [ ] **Step 2: Export the module**

Modify `agent/features/runtime/src/business/chat/looping.rs`:

```rust
mod stall;
mod state;
mod stream_handler;
```

and add after the queue export:

```rust
pub use state::{ChatLoopFsm, ChatLoopState, ChatLoopTransition};
```

- [ ] **Step 3: Run failing test**

Run:

```bash
cargo test -p runtime chat_loop_state
```

Expected: FAIL because `transition()` returns the current state for every transition.

- [ ] **Step 4: Implement transitions**

Replace `transition()` and add an `apply()` method in `state.rs`:

```rust
impl ChatLoopState {
    fn apply(self, transition: ChatLoopTransition) -> Self {
        match (self, transition) {
            (_, ChatLoopTransition::StartTurn | ChatLoopTransition::ResumeRunning) => {
                Self::Running
            }
            (Self::Running, ChatLoopTransition::AwaitTool) => Self::AwaitingTool,
            (
                Self::Running
                | Self::AwaitingTool
                | Self::Stopping
                | Self::StopHookBlocked,
                ChatLoopTransition::AwaitUser,
            ) => Self::AwaitingUser,
            (Self::Running, ChatLoopTransition::Compact) => Self::Compacting,
            (Self::Running | Self::AwaitingUser, ChatLoopTransition::TryStop) => Self::Stopping,
            (Self::Stopping, ChatLoopTransition::StopBlocked) => Self::StopHookBlocked,
            (Self::Stopping, ChatLoopTransition::StopSucceeded) => Self::Done,
            (_, ChatLoopTransition::AbortCurrentLoop | ChatLoopTransition::CancelCurrentLoop) => {
                Self::Done
            }
            (state, _) => state,
        }
    }
}

impl ChatLoopFsm {
    pub fn state(&self) -> ChatLoopState {
        self.state
    }

    pub fn transition(&mut self, transition: ChatLoopTransition) -> ChatLoopState {
        let previous = self.state;
        self.state = self.state.apply(transition);
        log::debug!(
            "chat loop state transition: {:?} --{:?}--> {:?}",
            previous,
            transition,
            self.state
        );
        self.state
    }
}
```

Keep the existing `Default` impl and tests.

- [ ] **Step 5: Verify FSM tests pass**

Run:

```bash
cargo test -p runtime chat_loop_state
```

Expected: PASS.

---

### Task 2: Strengthen Stop Hook Blocked Feedback

**Files:**
- Modify: `agent/features/runtime/src/business/chat/looping/finalize.rs`

- [ ] **Step 1: Add failing feedback test**

In `finalize.rs` test module, add this test near existing Stop hook feedback tests:

```rust
#[tokio::test]
async fn test_stop_hook_feedback_tells_llm_it_must_not_finish() {
    let entry = share::config::hooks::HookEntry {
        matcher: String::new(),
        hooks: Vec::new(),
        command: "check-stop.sh".to_string(),
        timeout: None,
    };
    let result = HookResult {
        blocked: true,
        output: "fix the failing test".to_string(),
        error: None,
        exit_code: Some(2),
    };

    let feedback = stop_hook_feedback_for_test(&[(entry, result, None)], "session-114")
        .await
        .expect("blocked hook should produce feedback");

    assert!(
        feedback.contains("不能结束") || feedback.contains("MUST NOT finish"),
        "feedback must explicitly tell the LLM it cannot finish yet: {feedback}"
    );
    assert!(
        feedback.contains("MUST") || feedback.contains("必须"),
        "feedback must use mandatory language: {feedback}"
    );
    assert!(feedback.contains("check-stop.sh"));
    assert!(feedback.contains("fix the failing test"));
}
```

- [ ] **Step 2: Run failing test**

Run:

```bash
cargo test -p runtime test_stop_hook_feedback_tells_llm_it_must_not_finish
```

Expected: FAIL because current feedback says only “请先解决以下问题后再结束”.

- [ ] **Step 3: Strengthen feedback text**

In `stop_hook_feedback()`, replace the current `Some(format!(...))` body:

```rust
Some(format!(
    "Stop hook 阻止了停止，请先解决以下问题后再结束：\n命令：{}\n{}",
    entry.command, details
))
```

with:

```rust
Some(format!(
    "Stop hook 阻止了停止。你现在还不能结束本轮处理。\n\
你 MUST 先满足下面 Stop hook 的要求，然后才能再次尝试停止。\n\
命令：{}\n{}",
    entry.command, details
))
```

- [ ] **Step 4: Verify feedback test passes**

Run:

```bash
cargo test -p runtime test_stop_hook_feedback_tells_llm_it_must_not_finish
```

Expected: PASS.

---

### Task 3: Thread FSM Through `process_chat_loop`

**Files:**
- Modify: `agent/features/runtime/src/business/chat/looping/loop_runner.rs`

- [ ] **Step 1: Import FSM types**

In the existing `use crate::business::chat::looping::{ ... }` list, add:

```rust
ChatLoopFsm, ChatLoopTransition,
```

The import block should include these names with the existing chat-loop types.

- [ ] **Step 2: Instantiate FSM**

After:

```rust
let mut pending_input = PendingInputBuffer::default();
```

add:

```rust
let mut loop_fsm = ChatLoopFsm::default();
```

- [ ] **Step 3: Mark each turn as running**

At the top of the `loop`, immediately after `turn_count += 1;`, add:

```rust
loop_fsm.transition(ChatLoopTransition::StartTurn);
```

- [ ] **Step 4: Mark cancellation and abort exits**

Before each `break` caused by cancelled / abort decisions, add the matching transition:

For interrupted cancellation path before `break;`:

```rust
loop_fsm.transition(ChatLoopTransition::CancelCurrentLoop);
```

For gate decisions matching `AbortCurrentLoop | CancelCurrentLoop`, add:

```rust
let transition = match gate.decision {
    GateDecision::AbortCurrentLoop => ChatLoopTransition::AbortCurrentLoop,
    GateDecision::CancelCurrentLoop => ChatLoopTransition::CancelCurrentLoop,
    _ => unreachable!("only abort/cancel handled here"),
};
loop_fsm.transition(transition);
```

Use this exact pattern only inside branches already guarded by `matches!(..., AbortCurrentLoop | CancelCurrentLoop)` or explicit match arms.

- [ ] **Step 5: Mark compaction boundary**

Immediately before `auto_compact(...).await;`, add:

```rust
loop_fsm.transition(ChatLoopTransition::Compact);
```

Immediately after `auto_compact(...).await;`, add:

```rust
loop_fsm.transition(ChatLoopTransition::ResumeRunning);
```

- [ ] **Step 6: Mark BeforeLlm gate continuation**

After the BeforeLlm gate match, for `Proceed | ContinueNextTurn`, ensure the FSM is running:

```rust
match gate.decision {
    GateDecision::Proceed | GateDecision::ContinueNextTurn => {
        loop_fsm.transition(ChatLoopTransition::ResumeRunning);
    }
    GateDecision::AbortCurrentLoop | GateDecision::CancelCurrentLoop => {
        let transition = match gate.decision {
            GateDecision::AbortCurrentLoop => ChatLoopTransition::AbortCurrentLoop,
            GateDecision::CancelCurrentLoop => ChatLoopTransition::CancelCurrentLoop,
            _ => unreachable!("only abort/cancel handled here"),
        };
        loop_fsm.transition(transition);
        sink.send_event(RuntimeStreamEvent::Cancelled).await;
        break;
    }
}
```

Replace the existing BeforeLlm match with this version.

- [ ] **Step 7: Mark stop attempt and blocked/success transitions**

Before calling `run_stop_hook_before_finish(...)`, add:

```rust
loop_fsm.transition(ChatLoopTransition::TryStop);
```

Inside the `if let Some(outcome) = run_stop_hook_before_finish(...).await { ... }` blocked branch, before pushing the system reminder, add:

```rust
loop_fsm.transition(ChatLoopTransition::StopBlocked);
```

Immediately before `continue;` in that same blocked branch, add:

```rust
loop_fsm.transition(ChatLoopTransition::ResumeRunning);
```

After the blocked branch and before the final BeforeFinish gate, do not mark success yet; the final gate can still continue the loop.

Immediately before `finish_completed_loop(&outcome, &sink, &task_store).await;`, add:

```rust
loop_fsm.transition(ChatLoopTransition::StopSucceeded);
```

- [ ] **Step 8: Mark tool boundary**

Immediately before `execute_tool_round(...).await`, add:

```rust
loop_fsm.transition(ChatLoopTransition::AwaitTool);
```

Immediately after tool results are synced and before the AfterBlockingBoundary gate, add:

```rust
loop_fsm.transition(ChatLoopTransition::AwaitUser);
```

When AfterBlockingBoundary gate returns `Proceed` or `ContinueNextTurn`, add:

```rust
loop_fsm.transition(ChatLoopTransition::ResumeRunning);
```

If AfterBlockingBoundary aborts/cancels, transition to Done before `break` using the abort/cancel pattern from Step 4.

- [ ] **Step 9: Mark API error finalization path**

Before `finalize_main_loop(...)` for `AgentRunStatus::ApiError`, add:

```rust
loop_fsm.transition(ChatLoopTransition::TryStop);
```

If `finalize_main_loop(...)` returns `Some(outcome)` and the loop continues, add before `continue;`:

```rust
loop_fsm.transition(ChatLoopTransition::StopBlocked);
loop_fsm.transition(ChatLoopTransition::ResumeRunning);
```

If it returns `None` and the branch breaks, add before `break;`:

```rust
loop_fsm.transition(ChatLoopTransition::StopSucceeded);
```

- [ ] **Step 10: Run compile-focused tests**

Run:

```bash
cargo test -p runtime chat_loop_state
cargo test -p runtime stop_hook_feedback
```

Expected: PASS.

---

### Task 4: Add/Adjust Loop Behavior Test for Stop Hook Blocking

**Files:**
- Modify: `agent/features/runtime/src/business/chat/looping/loop_runner.rs`

- [ ] **Step 1: Inspect existing loop tests**

Read the `#[cfg(test)] mod tests` in `loop_runner.rs`. Reuse existing fake provider, fake sink, and hook helpers. Do not introduce a new external test harness.

- [ ] **Step 2: Add or update a test for blocked then success**

If an equivalent test already exists, update assertions. Otherwise add a test named:

```rust
#[tokio::test]
async fn test_stop_hook_blocked_continues_until_stop_hook_success() {
    // Use existing test fakes from this module.
    // Configure provider with two responses:
    // 1. assistant text that ends turn
    // 2. assistant text that ends turn after receiving the system reminder
    // Configure Stop hook with a command that blocks first and succeeds second.
    // Run process_chat_loop.
    // Assert:
    // - events contain a SystemMessage with "MUST" and "不能结束" or equivalent.
    // - DoneWithDuration occurs only after the second provider response.
    // - messages sync includes a user system-reminder containing Stop hook feedback.
}
```

Use the module's existing fake types rather than inventing new ones. If the existing fake provider does not support multiple responses, add the smallest queue-backed response list to the existing fake provider in this test module.

- [ ] **Step 3: Run the specific test**

Run:

```bash
cargo test -p runtime test_stop_hook_blocked_continues_until_stop_hook_success -- --nocapture
```

Expected: PASS after implementation. If it fails because the existing hook fake cannot express “block once then success”, document the limitation in the test comment and rely on Task 1 + Task 2 unit tests plus existing stop-hook behavior tests.

---

### Task 5: Update Bug Tracking

**Files:**
- Modify: `docs/bug/active.md`

- [ ] **Step 1: Update Bug #114 table row**

Replace the #114 row with:

```markdown
| 114 | Stop hook blocked 缺少显式 chat loop 停止状态表达 | 中 | 修复中 | 未确认 | 2026-06 | Stop hook blocked 的语义应是阻止 chat loop 真正停止，并把 hook 要求反馈给 LLM 继续处理；现有控制流依赖隐式 `continue/break`，缺少轻量 FSM 和强约束反馈文案，容易被误解为“LLM 已完成后无意义阻止” |
```

- [ ] **Step 2: Add or update Bug #114 detail section**

If no `### #114` detail section exists, add one before `### #110`:

```markdown
### #114 Stop hook blocked 缺少显式 chat loop 停止状态表达

**状态**：修复中

**症状**：Stop hook blocked 时，runtime 会追加 system reminder 并继续下一轮，但宏观状态依赖 `continue/break` 隐式表达。用户容易理解为“LLM 已经完成输出后，Stop hook 再阻止停止没有意义”。

**修正语义**：Stop hook blocked MUST 阻止 chat loop 真正停止。LLM 已经尝试结束不等于 runtime 已经 Done；blocked 时必须把 Stop hook 要求反馈给 LLM，回到 Running，直到后续 Stop hook success 才进入 Done。

**修复方向**：
1. 引入轻量手写 `ChatLoopFsm`，显式表达 `Running -> ... -> Stopping -> StopHookBlocked -> Running -> Stopping -> Done`。
2. 强化 Stop hook blocked system reminder，明确“不能结束 / MUST 先满足 Stop hook 要求”。
3. 保持现有 HookRunner/provider/TUI 事件架构，不引入 FSM 框架，不做 stream-time Stop hook。

**验证**：
- `cargo test -p runtime chat_loop_state`
- `cargo test -p runtime stop_hook_feedback`
- `cargo test -p runtime stop_hook`

**涉及路径**：
- `agent/features/runtime/src/business/chat/looping/state.rs`
- `agent/features/runtime/src/business/chat/looping.rs`
- `agent/features/runtime/src/business/chat/looping/loop_runner.rs`
- `agent/features/runtime/src/business/chat/looping/finalize.rs`
```

- [ ] **Step 3: Verify docs format**

Run:

```bash
git diff -- docs/bug/active.md
```

Expected: Bug #114 row and detail section reflect the new semantics.

---

### Task 6: Full Verification

**Files:**
- No source edits unless verification reveals failures.

- [ ] **Step 1: Format check**

Run:

```bash
cargo fmt --check
```

Expected: PASS.

- [ ] **Step 2: Runtime targeted tests**

Run:

```bash
cargo test -p runtime chat_loop_state
cargo test -p runtime stop_hook_feedback
cargo test -p runtime stop_hook
```

Expected: PASS.

- [ ] **Step 3: Architecture guard**

Run:

```bash
AEMEATH_PROJECT_DIR="$PWD" CLAUDE_PROJECT_DIR="$PWD" .agents/hooks/check-architecture-guards.sh
```

Expected: PASS.

- [ ] **Step 4: Unit hook script**

Run:

```bash
AEMEATH_PROJECT_DIR="$PWD" CLAUDE_PROJECT_DIR="$PWD" .agents/hooks/check-unit-tests.sh
```

Expected: PASS. If a known unrelated test fails, capture the exact failure and do not mark Bug #114 complete until either it is fixed or explicitly documented as unrelated by user decision.

- [ ] **Step 5: Final status check**

Run:

```bash
git status --short
```

Expected: only intended files are modified.

---

## Self-Review

Spec coverage:
- Stop hook blocked prevents final Done: Task 3 and Task 4.
- Strong feedback to LLM: Task 2.
- Lightweight hand-written FSM: Task 1 and Task 3.
- No framework / no stream-time hook: preserved by file map and implementation tasks.
- Loop Gate consistency: Task 3 marks transitions around existing gates without replacing GateDecision.
- Bug tracking: Task 5.
- Verification: Task 6.

Placeholder scan: no TBD/TODO/fill-in-later placeholders. Task 4 allows reusing existing test fakes and gives fallback only if harness limitations prevent exact fake behavior.

Type consistency:
- `ChatLoopState`, `ChatLoopTransition`, and `ChatLoopFsm` names are consistent across tasks.
- `CancelCurrentLoop` matches existing `GateDecision::CancelCurrentLoop` naming.
- Existing runtime test command prefixes use `-p runtime`.
