# #1929 resume generation stale → 0 message 修复计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 #1927 合入后 resume 显示 0 message：同值 model 不 bump generation；需要写入时在 resume 视图固化前完成；stale display-history 跟随 latest。

**Architecture:** 保持 `update_metadata_for_project` 的 `revision += 1` 乐观并发语义不变。止血在 `persist_session_model` 同值跳过；时序在 `wire_agent_client_from_args` 把 persist 移到 `resolve_startup_session` 之前；韧性在 display-history 读路径对 `StaleDisplayHistory` 用 latest revision 重读，TUI `apply_window` 接受刷新后的 revision。

**Tech Stack:** Rust / context dataset reader / runtime client bootstrap / TUI resumed_history

---

### Task 1: 同值跳过（TDD）

**Files:**
- Modify: `agent/features/runtime/src/application/client/session_model_persist_tests.rs`
- Modify: `agent/features/runtime/src/application/client/session_model_persist.rs`

- [x] **Step 1: 写失败测试** `persist_session_model_skips_when_metadata_model_unchanged`
  - Recording port 的 `load_for_project` 返回 `metadata.model = Some("OmniRoute/cursor-auto")`
  - 调用 persist 同值后 `updates` 为空
- [x] **Step 2: 跑测试确认 RED**
- [x] **Step 3: 实现 load 后同值跳过**
- [x] **Step 4: 跑测试确认 GREEN**
- [x] **Step 5: 补充不同值仍写入的测试并 GREEN**

### Task 2: 启动时序前移

**Files:**
- Modify: `agent/features/runtime/src/application/client/from_args.rs`
- Modify: `agent/features/runtime/src/application/client/from_args_tests.rs`（如需锁定顺序）

- [x] **Step 1: 源码顺序测试** — persist 调用点位于 `resolve_startup_session` 之前（session_id 用 resume 参数或 committed id）
- [x] **Step 2: 跑测试 RED/调整断言**
- [x] **Step 3: 移动 persist；SessionStart 仍在 session 解析之后**
- [x] **Step 4: GREEN**

### Task 3: StaleDisplayHistory 跟随（TDD）

**Files:**
- Modify: `agent/features/context/src/integration_tests/dataset_session_reader.rs`
- Modify: `agent/features/context/src/adapters/dataset_session_reader.rs`
- Modify: `apps/cli/src/tui/model/conversation/resumed_history.rs`
- Modify: `apps/cli/src/tui/model/conversation/resumed_history_tests.rs`

- [x] **Step 1: 集成测试** — 保存 revision N 的 session+steps，metadata bump 到 N+1，用 expected=N 拉 window，期望成功且 `generation_revision == N+1`、step 正文可读
- [x] **Step 2: RED**
- [x] **Step 3: reader 在 session_id 匹配且 revision 落后时，用 latest manifest 继续读 members，window 带 actual revision**
- [x] **Step 4: TUI 测试** — pin revision=7 时 apply_window(revision=8) 应成功并更新 pin
- [x] **Step 5: GREEN**

### Task 4: 验证

- [x] `cargo test -p runtime --lib application::client::session_model_persist`
- [x] `cargo test -p context` 定向 reader 相关
- [x] `cargo test -p cli resumed_history_tests`
- [x] `cargo clippy -p context --lib -- -D warnings` 绿；`runtime` 被 main 既有 dead_code/manual_contains 挡住（非本分支）
- [ ] 门禁清单回写 issue #1929（开 PR 时勾选）
