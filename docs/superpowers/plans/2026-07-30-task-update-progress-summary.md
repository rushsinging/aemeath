# TaskUpdate 进度摘要与独立 Task Reminder 退役 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans) to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 `TaskUpdate(status)` 返回权威 task progress snapshot，自动管理 Batch 生命周期，并彻底移除 Context/Runtime 的独立 task reminder 注入。

**Architecture:** Task Domain 在状态迁移的同一原子提交中生成结构化进度摘要，并负责自动归档、重新激活和 active Batch 冲突校验。Task Tool Adapter 只把领域投影转换成 typed JSON 与双语文本；Context/Runtime/Provider 删除 reminder 字段、装配和消息装饰路径。

**Tech Stack:** Rust workspace、serde、async-trait、Tokio tests、Task Domain/Published Language、Context Port、Runtime loop、Tools TypedTool。

---

## 文件边界

- Modify `agent/features/task/src/domain/query.rs`: 新增 TaskProgressSnapshot 及 ready/recent/in-progress 投影算法，删除 TaskReminderSnapshot。
- Modify `agent/features/task/src/domain/state.rs`: 新增 status mutation 的 Batch 生命周期原子协调入口。
- Modify `agent/features/task/src/domain/task_access.rs`, `agent/features/task/src/domain.rs`, `agent/features/task/src/lib.rs`, `agent/features/task/src/adapters/store.rs`: 发布新的 TaskAccess 结果类型并移除 reminder query 端口。
- Test `agent/features/task/src/domain/query_tests.rs`, `agent/features/task/src/domain/state_tests.rs`, `agent/features/task/src/adapters/contract/task_access.rs`: 覆盖摘要、排序、截断、自动关闭、重新激活和冲突原子性。
- Modify `agent/features/tools/src/domain/types/task_update.rs`, `agent/features/tools/src/adapters/task_update.rs`: 扩展 typed result，status-only 返回领域摘要。
- Test `agent/features/tools/src/adapters/task_update_tests.rs`: 覆盖结构化返回、status-only 和错误无部分提交。
- Modify `agent/shared/src/i18n/tools/task.rs`: 删除完成后强制 TaskListGet 指引，改为摘要不足时才查询。
- Modify `agent/features/context/src/domain.rs`, `agent/features/context/src/application/service.rs`, `agent/features/context/src/domain/context_decision.rs`, context tests: 删除 TaskReminderSnapshot/InvocationReminder 和对应 token 估算参数。
- Modify `agent/features/runtime/src/ports.rs`, `agent/features/runtime/src/application/main_loop/looping/main_run_port.rs`, `loop_runner.rs`, `llm_strategy.rs`, subagent/context tests: 删除 Runtime task reminder 组装和 Provider 消息装饰。
- Delete `agent/features/runtime/src/application/main_loop/looping/task_reminder.rs` and `task_reminder_tests.rs`。
- Modify `docs/superpowers/specs/2026-07-30-task-update-progress-summary-design.md` only if implementation terminology changes; update relevant `specs/3.4-runtime.md`, `specs/3.7-prompt.md`, and task/context design docs when behavior is finalized.

## Task 1: 先建立 Task Domain 摘要失败测试

**Files:**
- Test `agent/features/task/src/domain/query_tests.rs`
- Test `agent/features/task/src/domain/state_tests.rs`

- [ ] **Step 1: 添加排序和截断的失败测试**

构造同一 Batch 中至少 3 个 Completed、3 个 InProgress、5 个 Pending，其中至少 2 个 Pending 具备未完成依赖；断言 `recently_completed` 按 `completed_at` 倒序只保留 2 个，`in_progress` 全量保留，`ready` 只保留前 2 个且 `ready_omitted == 1`、`blocked_count == 2`。

- [ ] **Step 2: 运行 Task Domain 测试确认失败**

Run: `cargo test -p task domain::query_tests -- --nocapture`
Expected: FAIL because the progress snapshot API does not exist.

- [ ] **Step 3: 添加自动关闭和重新激活失败测试**

断言完成当前 Batch 最后一个未完成任务后 Batch 自动变为 Archived、`current_batch()` 为 None，结果携带 `auto_closed`；随后无其他 active Batch 时将任务改回 Pending，断言 Batch 重新 Active、`current_batch()` 恢复且结果携带 `auto_reopened`。

- [ ] **Step 4: 添加 active Batch 冲突原子失败测试**

先自动关闭 Batch A，再创建并激活 Batch B；尝试将 A 的任务改回 Pending，断言返回 active conflict、任务仍为 Completed、A 仍 Archived、current 仍为 B、revision 不变。

- [ ] **Step 5: 运行新增测试确认失败原因正确**

Run: `cargo test -p task domain::query_tests domain::state_tests -- --nocapture`
Expected: FAIL only on missing progress/lifecycle behavior, not fixture or compile typos.

## Task 2: 实现 Task Domain 原子进度结果

**Files:**
- Modify `agent/features/task/src/domain/query.rs`
- Modify `agent/features/task/src/domain/state.rs`
- Modify `agent/features/task/src/domain/task_access.rs`
- Modify `agent/features/task/src/domain.rs`
- Modify `agent/features/task/src/lib.rs`
- Modify `agent/features/task/src/adapters/store.rs`

- [ ] **Step 1: 定义领域纯值类型**

定义任务条目、列表摘要、省略计数和生命周期事件；条目只暴露 task sequence、subject、status 必要值。使用固定 `const RECENT_LIMIT: usize = 2` 和 `const READY_LIMIT: usize = 2`，避免调用方重复实现限制。

- [ ] **Step 2: 实现 Batch snapshot 投影**

从变更后的 TaskStoreState 读取当前 Batch；完成项按 `completed_at.unwrap_or(0)` 倒序并以 task id 作为稳定 tie-breaker；进行中全量返回；ready 通过既有 `blocking_ids` 语义筛选 pending；分别计算 ready 和 blocked 的省略数量。

- [ ] **Step 3: 实现原子 status mutation**

新增 TaskAccess/TaskStoreState 方法，在 dry-run 校验依赖、Batch 状态和 active conflict 后，再一次性 reserve revision、迁移 Task、必要时迁移 Batch、更新 current_batch，并将 progress snapshot 绑定到同一个 command result。任何校验失败不得修改 task、batch、current_batch 或 revision。

- [ ] **Step 4: 处理自动关闭与重新激活**

状态变更后若当前 Batch 无 Pending/InProgress，归档并清除 current；若 Archived Batch 的任务变为 Pending/InProgress，仅在不存在其他 Active Batch 时重新激活，否则返回 typed conflict。保留 `completed_at` 已有清除/重写规则。

- [ ] **Step 5: 运行 Task Domain 测试确认通过**

Run: `cargo test -p task domain::query_tests domain::state_tests adapters::contract::task_access -- --nocapture`
Expected: PASS for ordering, limits, lifecycle and atomic conflict behavior.

## Task 3: 扩展 TaskUpdate Published Language 和 Adapter

**Files:**
- Modify `agent/features/tools/src/domain/types/task_update.rs`
- Modify `agent/features/tools/src/adapters/task_update.rs`
- Modify `agent/shared/src/i18n/tools/task.rs`
- Test `agent/features/tools/src/adapters/task_update_tests.rs`

- [ ] **Step 1: 添加 Tool Adapter 失败测试**

断言 status 更新结果包含 list、updated、recently_completed、in_progress、ready、omitted、lifecycle；非 status 更新不包含 progress 摘要；自动关闭文本包含“自动关闭/不会再提醒”；active conflict 返回错误且 store revision 不变。

- [ ] **Step 2: 运行 Tools 测试确认失败**

Run: `cargo test -p tools adapters::task_update_tests -- --nocapture`
Expected: FAIL because current result only contains task_id/status/subject/priority/blocked_by.

- [ ] **Step 3: 让 Adapter 消费 Task Domain 结果**

将 status 分支改为调用原子 progress mutation，subject/description/priority 分支保持现有 typed mutation；禁止 Adapter 自己调用 list、batch_snapshot 或推导 lifecycle。typed JSON 和人类可读文本均从同一领域结果生成。

- [ ] **Step 4: 更新中英文工具文案**

`TaskListGet` 明确写为：仅当最近一次 `TaskUpdate(status)` 摘要不足以决定下一步，或用户明确要求完整列表时调用；`TaskUpdate` 删除“完成后调用 TaskListGet”的强制指令，改为优先使用返回的 doing/ready 摘要。

- [ ] **Step 5: 运行 Tools 测试确认通过**

Run: `cargo test -p tools adapters::task_update_tests && cargo test -p share i18n::tools::task`
Expected: PASS and no tool description contains mandatory post-completion TaskListGet wording.

## Task 4: 删除 Context 的 reminder 数据流

**Files:**
- Modify `agent/features/context/src/domain.rs`
- Modify `agent/features/context/src/application/service.rs`
- Modify `agent/features/context/src/domain/context_decision.rs`
- Modify all context tests that construct `ContextRequest`/`ContextWindow`

- [ ] **Step 1: 添加 Context 契约失败测试**

更新窗口契约断言：存在任务状态不再影响 system blocks、message budget 或 invocation message；ContextRequest 不再需要 task reminder 字段，ContextWindow 不再拥有 invocation reminder。

- [ ] **Step 2: 运行 Context 测试确认失败**

Run: `cargo test -p context --tests -- --nocapture`
Expected: FAIL on the old reminder fields and old token-budget arguments.

- [ ] **Step 3: 删除 Context 类型和渲染逻辑**

移除 `TaskReminderSnapshot`、`InvocationReminder`、`ContextRequest.task_reminder`、`ContextWindow.invocation_reminder`、service 中的 reminder 构造，以及 context decision 中的 reminder token 参数；保留正常 system/message token 计算。

- [ ] **Step 4: 运行 Context 测试确认通过**

Run: `cargo test -p context --tests`
Expected: PASS and no `task_reminder`/`invocation_reminder` field remains in Context production code.

## Task 5: 删除 Runtime/Provider reminder 装饰

**Files:**
- Modify `agent/features/runtime/src/ports.rs`
- Modify `agent/features/runtime/src/application/main_loop/looping/main_run_port.rs`
- Modify `agent/features/runtime/src/application/main_loop/looping/loop_runner.rs`
- Modify `agent/features/runtime/src/application/loop_engine/llm_strategy.rs`
- Modify subagent and runtime tests
- Delete `agent/features/runtime/src/application/main_loop/looping/task_reminder.rs`
- Delete `agent/features/runtime/src/application/main_loop/looping/task_reminder_tests.rs`

- [ ] **Step 1: 添加 Runtime invocation 契约失败测试**

断言 `extract_invocation_context` 完全透传 canonical LLM messages，不追加 `<task-reminder>`；Main/Sub 的 ContextRequest 构造不再读取 TaskAccess reminder snapshot。

- [ ] **Step 2: 运行 Runtime 测试确认失败**

Run: `cargo test -p runtime application::loop_engine::llm_strategy_tests application::main_loop::looping::pre_compact_trigger_tests -- --nocapture`
Expected: FAIL on old reminder fixture and append behavior.

- [ ] **Step 3: 删除 Runtime reminder 状态和构造**

移除 `TaskReminderState` 字段、初始化、`freeze_request` 中的 TaskReminderSnapshot 构造、invoke_model 更新观察，以及相关 imports/ports。

- [ ] **Step 4: 删除 Provider message decoration**

让 `extract_invocation_context` 仅执行 `Message::to_llm_view`、tool schema 和 system block 映射；删除 `append_reminder_to_last_user_message` 及其测试。

- [ ] **Step 5: 运行 Runtime 测试确认通过**

Run: `cargo test -p runtime --lib`
Expected: PASS with no production reference to `task_reminder`, `InvocationReminder`, or `append_reminder_to_last_user_message`.

## Task 6: 补齐跨层场景与兼容验证

**Files:**
- Test `agent/features/tools/src/adapters/task_update_tests.rs`
- Test relevant `agent/features/task/src/adapters/contract/task_access.rs`
- Test relevant context/runtime contract tests
- Modify design/spec docs if terminology differs from implementation

- [ ] **Step 1: 添加 end-to-end-in-process scenario**

从 active Batch 创建多个任务、执行 status updates，断言最终 TypedToolResult 同时具备本次更新、全部 doing、前两个 ready、最近两个 completed、省略计数和 lifecycle 通知，并确认没有 `<task-reminder>` 出现在 Provider messages。

- [ ] **Step 2: 添加旧 snapshot compatibility test**

使用已有 snapshot fixture 读取带 `completed_at` 的任务，断言 resume 后排序和 status mutation 结果与实时路径一致；不得改旧 JSON 字段语义。

- [ ] **Step 3: 运行定向跨层测试**

Run: `cargo test -p task -p tools -p context -p runtime --tests`
Expected: PASS with no reminder injection regression.

- [ ] **Step 4: 搜索废弃代码**

Run: `rg -n 'TaskReminderSnapshot|InvocationReminder|invocation_reminder|task_reminder|TaskReminderState|append_reminder_to_last_user_message' agent packages apps`
Expected: no production references; only explicitly historical design records may remain.

## Task 7: 格式化、检查、架构守卫和 PR

**Files:**
- All files changed by Tasks 1-6

- [ ] **Step 1: 格式化并确认无格式差异**

Run: `cargo fmt --all && cargo fmt --all -- --check`
Expected: second command exits 0.

- [ ] **Step 2: 运行相关 clippy/check**

Run: `cargo check -p task -p tools -p context -p runtime && cargo clippy -p task -p tools -p context -p runtime --all-targets -- -D warnings`
Expected: exit 0 with no warnings.

- [ ] **Step 3: 运行架构守卫**

Run: `bash .agents/hooks/check-architecture-guards.sh`
Expected: all guards pass.

- [ ] **Step 4: 运行 workspace 测试**

Run: `cargo test --workspace`
Expected: all tests pass; first failures are recorded rather than hidden by reruns.

- [ ] **Step 5: 更新 Issue 验收清单并提交实现**

在 Issue #1456 勾选已完成门禁，记录定向测试、workspace test、clippy 和 guards 输出；提交实现 commit：

```bash
git add agent packages apps docs
git commit -m "feat(task): return progress summary from status updates"
```

- [ ] **Step 6: 拉取最新 main 并确认分支干净**

Run: `git pull origin main`
Expected: no unresolved conflicts; re-run relevant checks if merge changes code.

- [ ] **Step 7: 创建 PR 到 main**

```bash
gh pr create --repo rushsinging/aemeath --base main --head feat/1456-task-progress-summary --title "feat(task): return progress summary from status updates" --body-file /tmp/pr-1456.md
```

PR body 必须包含 `Closes #1456`、设计摘要、是否 breaking change、完整 Test plan，以及已核对的 specs/design 文档和架构守卫命令。Agent 不自动合并 PR，等待用户 review。
