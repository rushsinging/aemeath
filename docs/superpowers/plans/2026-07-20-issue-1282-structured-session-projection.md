# #1282 Session Structured Projection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development (recommended) or executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 v2 `CanonicalSession.run_slices` 与活动 compact marker 变为 Session、ContextWindow、resume、list 与 compact 的唯一常规读取 backing，为 #1277 accepted-input durable handoff 提供单一事实源。

**Architecture:** Context domain 新增 `CommittedRunSlice` / `CommittedRunStep` / `AcceptedInputProjection` 与单一 `CanonicalSession::structured_messages()`。所有读取方只通过 structured backing 取得 messages/active summary；v1 canonical 和无版本 legacy decode 时迁移为 synthetic slice/step。compact 使用唯一 `ActiveCompactMarker { summary, start_after, source_revision }`：它指向完整 RunStep 边界，保留完整 `run_slices` 作为唯一历史事实，ContextWindow 从 marker 后投影；每次 compact 原子合并旧 summary 并单调推进 marker。#1282 将当前 `append_finalized` 的 `ContextAppend.messages` 作为 compatibility bridge 写入同一结构化 Step 的 `outcome`。#1277 负责 accepted-input writer，#1278 把 outcome bridge 收口为带 FinalizeCause、receipt、usage 的正式协议。

**Tech Stack:** Rust、serde/serde_json、Context domain + ports + adapters、Cargo test。

---

## 文件结构

- Modify: `agent/features/context/src/domain/session/envelope.rs` — v2 envelope、RunSlice/RunStep、active compact marker、v1/legacy upgrade、唯一 projection。
- 修改：`agent/features/context/src/domain/session.rs` — 发布 v2 structured Session 类型。
- 修改：`agent/features/context/src/domain/session/restore.rs` — resume 使用结构化 projection。
- 修改：`agent/features/context/src/domain/session/management.rs` — session list 使用结构化 projection。
- 修改：`agent/features/context/src/adapters/canonical_session.rs` — snapshot 与 compact 输入使用结构化 projection。
- 修改：`agent/features/context/src/application/main_session.rs` — fresh Session 初始化 v2 字段。
- 修改：`agent/features/context/tests/session_envelope_codec.rs` — L1/L3 codec、migration、projection 测试。
- 修改：`agent/features/context/tests/canonical_session_repository.rs` — L2 snapshot/compact 读取契约。
- 修改：`agent/features/context/tests/main_session_wiring.rs` — L4 v2 resume 可见性。
- 修改：必要的现有 Context fixture 文件 — 初始化新增 `run_slices` 字段。

## Task 1: 建立 v2 structured Session schema 与唯一消息 projection

**Files:**
- Modify: `agent/features/context/src/domain/session/envelope.rs`
- Modify: `agent/features/context/src/domain/session.rs`
- Test: `agent/features/context/tests/session_envelope_codec.rs`

- [ ] **Step 1: 编写失败测试，定义 ordered structured projection**

新增测试构造两条 `CommittedRunSlice`，其中第一条 Step 有 accepted input 与 `outcome: None`，第二条 Step 有 accepted input 与 outcome message；断言 `CanonicalSession::structured_messages()` 只按 slice/step 顺序输出 accepted message，且 accepted input 不会因 outcome slot 重复。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p context --test session_envelope_codec structured_projection_flattens_steps_once -- --exact`

Expected: FAIL，因 `CommittedRunSlice` / `structured_messages` 尚不存在。

- [ ] **Step 3: 实现最小 v2 schema 与 projection**

在 envelope 中：
- 将 `CURRENT_SESSION_SCHEMA_VERSION` 改为 `2`；
- 定义 `AcceptedInputProjection { messages, fingerprint, committed_revision }`；
- 定义 `CommittedRunStep { step_id, accepted_input, outcome }`；outcome 以 `Option<Vec<Message>>` 预留，默认 `None`；
- 定义 `CommittedRunSlice { run_id, steps }`；
- 给 `CanonicalSession` 增加 `#[serde(default)] run_slices`；
- 实现 `structured_messages()`：依次展平每个 Step 的 accepted messages，若 outcome 非空再展平 outcome；
- 对空 `run_slices` 保持返回空消息，绝不回退读取 `chats`；
- 更新 fixture/debug/export。

- [ ] **Step 4: 运行单元测试确认通过**

Run: `cargo test -p context --test session_envelope_codec structured_projection_flattens_steps_once -- --exact`

Expected: PASS。

- [ ] **Step 5: 提交本任务**

```bash
git add agent/features/context/src/domain/session/envelope.rs agent/features/context/src/domain/session.rs agent/features/context/tests/session_envelope_codec.rs
git commit -m "feat(context): #1282 新增结构化 Session 投影"
```

## Task 2: 支持 v1 canonical 与 legacy 到 synthetic Step 的 v2 migration

**Files:**
- Modify: `agent/features/context/src/domain/session/envelope.rs`
- Test: `agent/features/context/tests/session_envelope_codec.rs`

- [ ] **Step 1: 编写失败测试，锁定 v1 canonical upgrade**

添加 `schema_version: 1` fixture，带一个 Normal `ChatSegment` 和一个 Compact segment。断言 decode 后：
- Normal segment 转为一个 `run_id = legacy:<segment-id>` slice；
- 该 slice 仅有一个 `step_id = synthetic:<segment-id>` Step；
- Step accepted messages 保持原有完整顺序，不按 role/tool 拆分；
- Compact segment 留在 legacy `chats` compatibility history，不能凭空写入 structured Step。

- [ ] **Step 2: 编写失败测试，锁定无版本 legacy migration**

使用 top-level `messages` fixture，断言 decode 后生成一个 slice/step，并且 `structured_messages()` 与原 messages 完全一致。

- [ ] **Step 3: 运行 migration 测试确认失败**

Run: `cargo test -p context --test session_envelope_codec 'v1_normal_segment_upgrades_to_single_synthetic_step|legacy_messages_upgrade_to_single_synthetic_step' -- --exact`

Expected: FAIL，因 v1 当前被拒绝或 legacy 未填充 run_slices。

- [ ] **Step 4: 实现 versioned v1 decoder 与 legacy upgrade**

- 添加只描述 schema v1 的 DTO，避免将 v1 直接反序列化为 v2；
- `decode_with_workspace_upgrade` 对 version 1 调用 upgrade helper，future version 继续 fail-closed 并保留原 bytes；
- 将每个 `SegmentKind::Normal` 映射为单 synthetic RunSlice/Step；
- 对无版本 legacy 先把 `messages` 正规化为单 Normal segment，再应用同一 synthetic conversion；
- Compact segments 保留在 `chats` 以保证历史 compact summary wire 兼容，但不构造 Step；
- 标记 v1/legacy decode 为升级路径。

- [ ] **Step 5: 运行 codec 兼容测试确认通过**

Run: `cargo test -p context --test session_envelope_codec -- --nocapture`

Expected: PASS，包含 current v2 roundtrip、v1 synthetic step、legacy synthetic step 与 future-version fail-closed。

- [ ] **Step 6: 提交本任务**

```bash
git add agent/features/context/src/domain/session/envelope.rs agent/features/context/tests/session_envelope_codec.rs
git commit -m "feat(context): #1282 迁移旧 Session 为 synthetic step"
```

## Task 3: 将所有常规读取方切换到 structured projection

**Files:**
- Modify: `agent/features/context/src/domain/session/restore.rs`
- Modify: `agent/features/context/src/domain/session/management.rs`
- Modify: `agent/features/context/src/adapters/canonical_session.rs`
- Modify: `agent/features/context/src/application/main_session.rs`
- Modify: `agent/features/context/tests/canonical_session_repository.rs`
- Modify: `agent/features/context/tests/main_session_wiring.rs`
- Modify: Context fixture tests containing `CanonicalSession { ... }`

- [ ] **Step 1: 编写失败测试，证明 finalized bridge 写入结构化 Step**

调用现有 `append_finalized` 后断言 canonical holder 的 `run_slices` 含对应 run、step 和 `outcome: Some(append.messages)`，同时 `structured_messages()` 返回该 outcome；重复同 fingerprint 不增加 Step，冲突仍保留 typed error。

- [ ] **Step 2: 编写失败测试，证明 snapshot 忽略 chats 而读取 run_slices**

构造 v2 session：`chats` 放入错误消息，`run_slices` 放入正确 accepted message。通过 `CanonicalSessionRepository::snapshot` 断言仅返回 structured message，active summary 不从 chats 恢复。

- [ ] **Step 3: 编写失败测试，证明 restore/list 忽略 chats**

构造同样的 v2 session，断言：
- `SessionRestore::from_canonical(...).active_messages` 只含 structured message；
- `SessionListEntry::from_canonical(...)` 的 preview 和 message_count 只来自 structured message。

- [ ] **Step 4: 运行读取链路测试确认失败**

Run: `cargo test -p context --test canonical_session_repository snapshot_reads_structured_projection_not_legacy_chats -- --exact`

Expected: FAIL，当前 snapshot 仍调用 `ChatChain::from_chats`。

- [ ] **Step 5: 用唯一 projection 替换读取链路并接入 bridge**

- `CanonicalSessionRepository::snapshot` 从 `structured_messages()` 构造 messages，active_summary 由活动 marker 提供；
- `CanonicalSessionRepository::append_finalized` 在同一 candidate 中创建或定位 `(run_id, step_id)` 的 `CommittedRunSlice/CommittedRunStep`，将既有 `ContextAppend.messages` 写为 `outcome: Some(messages)`，再按现有 CAS、ledger、durable-before-publish 规则提交；不写 accepted_input；
- `SessionRestore::from_canonical` 改从 `structured_messages()` 开始，保持 sanitize/deep-clean 行为；
- `SessionListEntry::from_canonical` 改从 `structured_messages()` 计算 preview/count；
- canonical automatic/manual compact 的输入从 `structured_messages()` 读取；
- fresh `CanonicalSession` 初始化 `run_slices: Vec::new()`；
- 修复所有 fixture 的新字段；
- 不在 v2 read path 添加 chats fallback 或双读。

- [ ] **Step 6: 运行相邻链路测试确认通过**

Run: `cargo test -p context --test canonical_session_repository --test main_session_wiring --test session_envelope_codec -- --nocapture`

Expected: PASS。

- [ ] **Step 7: 提交本任务**

```bash
git add agent/features/context/src/domain/session/restore.rs agent/features/context/src/domain/session/management.rs agent/features/context/src/adapters/canonical_session.rs agent/features/context/src/application/main_session.rs agent/features/context/tests
git commit -m "refactor(context): #1282 切换 Session 读取投影"
```

## Task 4: 将 compact 迁移为单一活动边界标记

**Files:**
- Modify: `agent/features/context/src/domain/session/envelope.rs`
- Modify: `agent/features/context/src/adapters/canonical_session.rs`
- Modify: `agent/features/context/tests/canonical_session_repository.rs`
- Modify: `agent/features/context/tests/session_envelope_codec.rs`
- Modify: `docs/design/02-modules/context-management/01-session.md`
- Modify: `docs/design/02-modules/context-management/02-compact.md`

- [ ] **Step 1: 编写失败测试，证明 marker 后的新 Step 仍可见**

准备含多个完整 RunStep 的 v2 session，compact 后再 append 一个新的 finalized Step。断言：
- marker 指向某个完整 `(run_id, step_id)` 的后继边界；
- ContextWindow 的 messages 是 marker 后的旧 Step 加上新 Step；
- 旧 chats 不影响结果；
- marker 的 summary 走 `active_summary`，不作为普通 message 复制。

- [ ] **Step 2: 编写失败测试，证明第二次 compact 合并摘要且只推进 marker**

对同一 session 执行两次 compact，断言第二次：
- 只保留一个 marker；
- marker 的 start_after 在 RunStep 全序上单调向后；
- summary 包含上次 summary 的内容；
- `run_slices` 没有被扁平 tail 覆写，第二次 compact 后的新 Step 仍从 marker 后可见。

- [ ] **Step 3: 运行 marker 测试确认失败**

Run: `cargo test -p context --test canonical_session_repository 'compact_marker_keeps_new_steps_visible|second_compact_advances_single_marker' -- --nocapture`

Expected: FAIL，当前 `StructuredCompactProjection.recent_messages` 覆盖 `run_slices` 读取。

- [ ] **Step 4: 实现 `ActiveCompactMarker`**

- 将临时 `StructuredCompactProjection { recent_messages }` 替换为 `ActiveCompactMarker { summary, start_at: Option<RunStepCursor>, source_revision }`；`RunStepCursor` 是 `(run_id, step_id)`，`None` 表示当前无可见 tail；
- `structured_messages()` 始终遍历完整 `run_slices`，只投影 marker 指向的第一个完整 Step 及之后内容；不复制 tail；
- `active_summary()` 只读取 marker summary；
- compact 按完整 Step 边界从当前 marker 后的 visible steps 选取 recent suffix，摘要输入为旧 summary 加本次被淘汰 steps；原子更新单个 marker；
- `append_finalized` / 后续 #1277 accepted input writer 只追加/补充 `run_slices`，天然落在 marker 之后；
- v1/legacy compact segment 升级为 marker：根据 synthetic Step 映射找到 compact 后第一个 Normal Step，若无法精确映射则 marker 从首 Step 开始并保留 summary，绝不从扁平消息猜测 Step 边界；
- clear 清除 marker、run_slices 和 legacy ledger；不实施 #1278 正式 outcome/receipt writer。

- [ ] **Step 5: 运行 Context 完整测试确认通过**

Run: `cargo test -p context --lib --tests`

Expected: PASS。

- [ ] **Step 6: 提交本任务**

```bash
git add agent/features/context/src/domain/session/envelope.rs agent/features/context/src/adapters/canonical_session.rs agent/features/context/tests/canonical_session_repository.rs agent/features/context/tests/session_envelope_codec.rs docs/design/02-modules/context-management/01-session.md docs/design/02-modules/context-management/02-compact.md
git commit -m "refactor(context): #1282 以 marker 收口 compact 投影"
```

## Task 5: 完整验证与 Issue 门禁回写

**Files:**
- Modify: `docs/design/02-modules/context-management/01-session.md`
- Modify: `docs/design/02-modules/context-management/02-compact.md`
- Modify: GitHub Issue #1282

- [ ] **Step 1: 运行格式与静态检查**

Run:

```bash
cargo fmt --check
cargo check
cargo clippy --all-targets -- -D warnings
```

Expected: 全部 exit 0。

- [ ] **Step 2: 运行工作区验证**

Run: `cargo test --workspace`

Expected: 全部通过；若发现首次失败，记录完整输出，仅定向重跑用于分类 flaky，不能用重跑覆盖失败。

- [ ] **Step 3: 运行完整架构守卫**

Run: `bash .agents/hooks/check-architecture-guards.sh`

Expected: exit 0。

- [ ] **Step 4: 审查退役路径**

Run: `git diff origin/main...HEAD --check && git grep -n 'ChatChain::from_chats' -- agent/features/context`

Expected: 常规 snapshot/restore/list/compact 消费点已移除；仅 chat-chain 自身测试或明确 compatibility 边界存在。

- [ ] **Step 5: 回写 #1282 checklist 与依赖状态**

使用 `gh issue edit 1282 --repo rushsinging/aemeath --body-file ...` 更新已完成 checklist，记录：v2 structured read projection 完成、v1/legacy synthetic migration、#1277 接管 accepted input writer、#1278 接管 outcome writer、完整验证证据。

- [ ] **Step 6: 最终提交**

```bash
git add docs/design/02-modules/context-management/01-session.md docs/design/02-modules/context-management/02-compact.md
git commit -m "docs(context): #1282 固化结构化投影边界"
```
