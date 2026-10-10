# Issue #1919 — TUI ApiError/SessionResumeFailed notice 双写

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 同一 runtime 错误事件在 TUI 只展示一行（Error 语义），消除 `update_runtime_event` 侧写与 `map_runtime_event` reducer 的双写。

**Architecture:** 展示只走 `map_runtime_event` → `reduce_agent_event`。`update_runtime_event` 仅保留非展示副作用（processing、日志、presentation 字段、mapping 为 default 的唯一展示路径）。

**Tech Stack:** Rust、cli TUI（ratatui TEA）、`cargo test -p cli`

**Issue:** https://github.com/rushsinging/aemeath/issues/1919

---

### Task 1: 复现测试 — ApiError 单写

**Files:**
- Modify: `apps/cli/src/tui/app/update/ui_event_tests.rs`

- [ ] **Step 1: 改写 `test_api_error_appends_notice_and_defers_processing_to_done`**

  断言：
  - timeline 中 `OutputTimelineItem::Error` 含错误文案恰好 1 次
  - `OutputTimelineItem::System` 含同文案恰好 0 次
  - `is_processing` 仍为 true

- [ ] **Step 2: 新增 helper `error_notice_texts`**

  与 `system_notice_texts` 对称，收集 Error timeline 文本。

- [ ] **Step 3: 跑测试确认失败（当前双写）**

  Run: `cargo test -p cli test_api_error_appends_notice_and_defers_processing_to_done -- --nocapture`
  Expected: FAIL（System 同文仍为 1，或 Error 断言与旧 System 断言冲突）

### Task 2: 复现测试 — SessionResumeFailed 单写

**Files:**
- Modify: `apps/cli/src/tui/app/update/ui_event_tests.rs`

- [ ] **Step 1: 新增测试 `test_session_resume_failed_appends_single_prefixed_error`**

  派发 `TuiRuntimeEvent::SessionResumeFailed { kind: NotFound, id, message }`。
  断言：
  - Error 文本恰好 1 条，且等于 `⚠️ 会话恢复失败（不存在）: {message}`
  - System 文本不含该 message

- [ ] **Step 2: 跑测试确认失败**

  Run: `cargo test -p cli test_session_resume_failed_appends_single_prefixed_error -- --nocapture`
  Expected: FAIL（当前为 System 带前缀 + Error 仅 message）

### Task 3: 删除 ApiError 侧写

**Files:**
- Modify: `apps/cli/src/tui/app/update.rs`

- [ ] **Step 1: 删除 `TuiRuntimeEvent::ApiError` 臂中的 `append_system_notice`**

  保留 `mark_output_dirty()` 或整臂可缩为空（mapping 已 AppendError；若 dirty 由 reducer 合并则可删整臂，优先最小：只删 notice 侧写）。

- [ ] **Step 2: 跑 Task 1 测试确认通过**

### Task 4: 迁 SessionResumeFailed 到 mapping

**Files:**
- Modify: `apps/cli/src/tui/adapter/agent_event.rs`
- Modify: `apps/cli/src/tui/app/update.rs`

- [ ] **Step 1: mapping 改为带前缀的单次 `AppendError`**

  前缀逻辑与现 `update.rs` 三分支一致（NotFound / Corrupt / Io）。

- [ ] **Step 2: `update.rs` 删除 `append_system_notice`，保留 `log::warn!`**

- [ ] **Step 3: 跑 Task 2 测试确认通过**

### Task 5: 对账扫描与回归

**Files:**
- Read: `apps/cli/src/tui/app/update.rs`（`update_runtime_event` match）
- Read: `apps/cli/src/tui/adapter/agent_event.rs`

- [ ] **Step 1: 对账表确认无新双写**

  凡 mapping 已 `AppendError` / `AppendSystemMessage` 的事件，update 不得再 `append_*_notice`。

- [ ] **Step 2: 跑相关测试**

  Run: `cargo test -p cli --lib api_error -- --nocapture` 与 `cargo test -p cli --lib session_resume_failed -- --nocapture`（或精确测试名）。
  Expected: PASS

### Task 6: Commit + PR

- [ ] **Step 1: commit**（用户要求时再用 /commit；本计划默认在开 PR 前 commit）
- [ ] **Step 2: push + `gh pr create`，Closes #1919**

**Out of scope:** 限流文案用户可读化；runtime/provider 重试策略。
