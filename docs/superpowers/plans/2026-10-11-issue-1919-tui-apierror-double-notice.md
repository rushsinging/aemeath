# Issue #1919 — TUI ApiError/SessionResumeFailed notice 双写

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 同一 runtime 错误事件在 TUI 只展示一行（Error 语义），消除 `update_runtime_event` 侧写与 `map_runtime_event` reducer 的双写。

**Architecture:** 展示只走 `map_runtime_event` → `reduce_agent_event`。`update_runtime_event` 仅保留非展示副作用（processing、日志、presentation 字段、mapping 为 default 的唯一展示路径）。

**Tech Stack:** Rust、cli TUI（ratatui TEA）、`cargo test -p cli`

**Issue:** https://github.com/rushsinging/aemeath/issues/1919

**PR:** https://github.com/rushsinging/aemeath/pull/1920

---

### Task 1: 复现测试 — ApiError 单写

**Files:**
- Modify: `apps/cli/src/tui/app/update/ui_event_tests.rs`

- [x] **Step 1: 改写 `test_api_error_appends_notice_and_defers_processing_to_done`**

  断言：
  - timeline 中 `OutputTimelineItem::Error` 含错误文案恰好 1 次
  - `OutputTimelineItem::System` 含同文案恰好 0 次
  - `is_processing` 仍为 true

- [x] **Step 2: 新增 helper `error_notice_texts`**

  与 `system_notice_texts` 对称，收集 Error timeline 文本。

- [x] **Step 3: 跑测试确认失败（当前双写）**

  Run: `cargo test -p cli --bin aemeath test_api_error_appends_notice_and_defers_processing_to_done -- --nocapture`
  Note: 修复与测试同批落地；最终态 PASS（Error=1, System=0）。

### Task 2: 复现测试 — SessionResumeFailed 单写

**Files:**
- Modify: `apps/cli/src/tui/app/update/ui_event_tests.rs`

- [x] **Step 1: 新增测试 `test_session_resume_failed_appends_single_prefixed_error`**

  派发 `TuiRuntimeEvent::SessionResumeFailed { kind: NotFound, id, message }`。
  断言：
  - Error 文本恰好 1 条，且等于 `⚠️ 会话恢复失败（不存在）: {message}`
  - System 文本不含该 message

- [x] **Step 2: 跑测试确认失败**

  Run: `cargo test -p cli --bin aemeath test_session_resume_failed_appends_single_prefixed_error -- --nocapture`
  Note: 修复与测试同批落地；最终态 PASS。

### Task 3: 删除 ApiError 侧写

**Files:**
- Modify: `apps/cli/src/tui/app/update.rs`

- [x] **Step 1: 删除 `TuiRuntimeEvent::ApiError` 臂中的 `append_system_notice`**

  保留 `mark_output_dirty()`；展示只走 mapping → AppendError。

- [x] **Step 2: 跑 Task 1 测试确认通过**

### Task 4: 迁 SessionResumeFailed 到 mapping

**Files:**
- Modify: `apps/cli/src/tui/adapter/agent_event.rs`
- Modify: `apps/cli/src/tui/app/update.rs`

- [x] **Step 1: mapping 改为带前缀的单次 `AppendError`**

  前缀逻辑与原 `update.rs` 三分支一致（NotFound / Corrupt / Io）。

- [x] **Step 2: `update.rs` 删除 `append_system_notice`，保留 `log::warn!`**

- [x] **Step 3: 跑 Task 2 测试确认通过**

### Task 5: 对账扫描与回归

**Files:**
- Read: `apps/cli/src/tui/app/update.rs`（`update_runtime_event` match）
- Read: `apps/cli/src/tui/adapter/agent_event.rs`

- [x] **Step 1: 对账表确认无新双写**

  凡 mapping 已 `AppendError` / `AppendSystemMessage` 的事件，update 不得再 `append_*_notice`。
  剩余 `append_system_notice` 均为 mapping=default 的唯一展示路径（ReflectionHistory / ModelSwitched / ContextEstimated 等）。

- [x] **Step 2: 跑相关测试**

  - `test_api_error*` PASS
  - `test_session_resume_failed_appends_single_prefixed_error` PASS
  - pre-push：cli 1377 全绿

### Task 6: Commit + PR

- [x] **Step 1: commit** `250bf887e`
- [x] **Step 2: push + `gh pr create`，Closes #1919** → https://github.com/rushsinging/aemeath/pull/1920

**Out of scope:** 限流文案用户可读化；runtime/provider 重试策略。
