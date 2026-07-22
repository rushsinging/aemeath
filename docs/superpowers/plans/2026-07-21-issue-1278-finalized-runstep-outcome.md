# #1278 Finalized RunStep Outcome Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development (recommended) or executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将同一 `(run_id, step_id)` 的 finalized assistant、原序 Tool results、receipts、`FinalizeCause`、usage、fingerprint 与 revision 以 `FinalizedOutcomeProjection` 持久化到既有 `CommittedRunStep.outcome`，替代仅保存 `Vec<Message>` 的 compatibility bridge。

**Architecture:** #1277 已把 accepted user facts 先写入 `CommittedRunStep.accepted_input`；本计划只为同一个 Step 补充 outcome，绝不重写或重复 accepted input。Context 负责 outcome Published Language、canonical/in-memory repository、v2 envelope 与 structured 读取；Runtime Main/Sub 只能经稳定 `ContextCoordinator` / `ContextPort` 构造和提交该语言。#1272 继续拥有 InputBuffer、drain-or-seal 与 Loop admission；#1247 复用本协议接入生产 control command；#879 负责旧路径物理退役。

**Tech Stack:** Rust、serde/serde_json、async_trait、Context Port/Repository、Runtime ContextCoordinator、Cargo tests。

---

## 文件结构

- Modify: `agent/features/context/src/domain.rs` — 提取正式 `FinalizedOutcomeProjection`，约束 outcome append 的数据、receipt、usage 与 fingerprint 语义。
- Modify: `agent/features/context/src/domain/session/envelope.rs` — v2 `CommittedRunStep.outcome` wire schema、projection、legacy compatibility upgrade。
- Modify: `agent/features/context/src/ports/{context_port.rs,rs}` — 维持唯一 finalized outcome Port/Repository 语言，不暴露 session internals。
- Modify: `agent/features/context/src/application/service.rs` — outcome append 的纯委托。
- Modify: `agent/features/context/src/adapters/{canonical_session.rs,in_memory_session.rs}` — outcome durable-before-publish、同键幂等/冲突、revision 与 snapshot 行为。
- Modify: `agent/features/runtime/src/application/context_coordination.rs` — 唯一 outcome append façade 与稳定 fingerprint 输入。
- Modify: `agent/features/runtime/src/application/chat/looping/main_run_port.rs` — Main finalized facts 只经 coordinator 提交。
- Modify: `agent/features/runtime/src/application/agent/runner/loop_run.rs` — Sub finalized facts 只经 coordinator 提交。
- Modify: Context/Runtime 的相邻测试 — L1–L4 证据。
- Modify: `docs/design/02-modules/context-management/{01-session.md,02-compact.md}`、`docs/design/02-modules/runtime/03-loop-and-state-machine.md`、`docs/design/03-engineering/03-migration-governance.md` — Target 和迁移责任回写。

## Task 1: 定义正式 outcome Published Language 与 Context Port 契约

**Files:**
- Modify: `agent/features/context/src/domain.rs`
- Modify: `agent/features/context/src/ports/{context_port.rs,rs}`
- Modify: `agent/features/context/src/application/service.rs`
- Test: `agent/features/context/tests/context_port_contract.rs`
- Test: `agent/features/context/tests/application_service_contract.rs`

- [ ] **Step 1: 写失败的 ContextPort 契约测试**

新增 fixture，调用 `ContextPort::append_and_persist` 后断言 append 和 receipt 表达完整 finalized outcome：`FinalizeCause`、finalized messages、原序 `StepReceipt`、`api_input_tokens`、outcome fingerprint、committed revision。断言该 API 不接收 accepted input，且同一 Step 的 accepted input 不是 outcome payload 的副本。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p context --test context_port_contract finalized_outcome_port_preserves_typed_projection -- --exact`

Expected: FAIL，因 outcome 仍仅以 compatibility message vector 表达，契约无法检查正式 projection。

- [ ] **Step 3: 实现最小 Published Language**

在 `domain.rs` 定义：

```rust
pub struct FinalizedOutcomeProjection {
    pub finalize_cause: FinalizeCause,
    pub messages: Vec<ContextMessage>,
    pub receipts: Vec<StepReceipt>,
    pub api_input_tokens: Option<u64>,
    pub fingerprint: ContentFingerprint,
    pub committed_revision: SessionRevision,
}
```

同时：
- 保留 `ContextAppend` 作为 Runtime → Context 的唯一 append DTO，但明确它是投影的未提交输入；
- `AppendReceipt` 仍只发布 `(run_id, step_id, committed_revision, fingerprint)`；
- `ContextAppendError::ContentConflict` 继续用于相同 key、不同 finalized payload；
- `ContextPort`、`SessionRepository` 与 `ContextApplicationService` 只委托该单一语言；更新 fake，禁止新增 parallel finalize API；
- 不将 `RunStatus`、future、cancellation scope、Sub 私有消息链或 raw stream delta 加入 projection。

- [ ] **Step 4: 运行 Context Port 契约测试确认通过**

Run: `cargo test -p context --test context_port_contract --test application_service_contract -- --nocapture`

Expected: PASS。

## Task 2: 将 v2 envelope outcome 从 compatibility vector 收口为正式 projection

**Files:**
- Modify: `agent/features/context/src/domain/session/envelope.rs`
- Test: `agent/features/context/tests/session_envelope_codec.rs`

- [ ] **Step 1: 写失败的 v2 codec 测试**

构造带 accepted input 与 finalized outcome 的 `CommittedRunStep`，round-trip 后断言：
- accepted input messages 与 outcome messages 分别保留且只投影一次；
- `FinalizeCause::UserCancelledStep`、Tool/Agent receipts（含 `CancellationUnconfirmed`）、`api_input_tokens`、fingerprint 和 committed revision 完整保留；
- structured projection 只展平 accepted input + `outcome.messages`，不将 receipt/usage 转为模型消息；
- `outcome == None` 的 accepted-only Step 仍合法。

- [ ] **Step 2: 写 legacy bridge compatibility 的失败测试**

使用当前 schema v2 的 `outcome: [Message, ...]` JSON fixture decode，断言 reader 升级为 synthetic `FinalizedOutcomeProjection`：保留 message 顺序，标记为 compatibility normal finalization，receipt 为空、usage 为 `None`；不可把一个 legacy vector 拆成多个 Step。

- [ ] **Step 3: 运行 codec 测试确认失败**

Run: `cargo test -p context --test session_envelope_codec 'finalized_outcome_round_trip_preserves_receipts|v2_compatibility_outcome_vector_upgrades_as_single_projection' -- --nocapture`

Expected: FAIL，因 `CommittedRunStep.outcome` 当前类型为 `Option<Vec<Message>>`。

- [ ] **Step 4: 实现 outcome wire schema 与 compatibility upgrade**

- 将 `CommittedRunStep.outcome` 改为 `Option<FinalizedOutcomeProjection>`；wire projection 使用 Context-owned DTO，不在 Runtime 定义重复 serde 类型；
- 在 `CanonicalSession::append_finalized_outcome` 接收完整 projection，定位或创建既有 Run slice / Step；若 Step 已含 accepted input，只设置 `outcome`，不覆盖该字段；
- 将旧 v2 compatibility vector 的 decode 放在 envelope ACL：它升级为同一 synthetic finalized projection，**NEVER** 基于 role / tool 顺序猜 Step 边界或伪造 receipt；
- `structured_messages()` / marker 后读取只取 `accepted_input.messages` 与 `outcome.messages`；
- 保持 v1 / 无版本 legacy 的既有 synthetic Step 升级逻辑；它们仅作为 legacy facts，不伪造正式 Tool/Agent receipt。

- [ ] **Step 5: 运行 codec 与 migration 测试确认通过**

Run: `cargo test -p context --test session_envelope_codec -- --nocapture`

Expected: PASS，覆盖正式 v2 round-trip、旧 v2 bridge upgrade、v1/legacy synthetic Step 与 future-version fail-closed。

## Task 3: 实现 canonical 与 in-memory outcome writer 的等价语义

**Files:**
- Modify: `agent/features/context/src/adapters/canonical_session.rs`
- Modify: `agent/features/context/src/adapters/in_memory_session.rs`
- Test: `agent/features/context/tests/canonical_session_repository.rs`
- Test: `agent/features/context/tests/in_memory_session_backing.rs`

- [ ] **Step 1: 写 canonical repository 的失败测试**

以已包含 accepted input 的 Session 调用 finalized append，并断言：
- candidate 将正式 projection 补到同一 `(run_id, step_id)`；accepted input 的 messages、fingerprint、committed revision 不变；
- candidate 在 writer 成功前不 publish；
- writer 成功后 outcome `committed_revision` 与 `AppendReceipt.committed_revision` 相同，Task/Workspace snapshot 同次提交；
- 相同 key + 相同 fingerprint 返回原 receipt，且不新增 revision；不同 finalized payload 返回 `ContentConflict`；
- 过期 `expected_revision` 返回 `RevisionConflict`，不覆盖刚写入的 accepted input。

- [ ] **Step 2: 写 in-memory backing 的失败测试**

断言 in-memory backing 对 outcome append 有同样的 CAS、idempotency、conflict、revision 和 accepted-input-preservation 语义，并让 snapshot 可见 accepted + outcome messages。

- [ ] **Step 3: 运行 backing 测试确认失败**

Run: `cargo test -p context --test canonical_session_repository finalized_outcome_persists_before_publish -- --exact`

Expected: FAIL，因 canonical writer 仍只写 `Vec<Message>`，in-memory 仍把结果直接 append 到扁平 messages。

- [ ] **Step 4: 实现 canonical/in-memory 正式 outcome commit**

- canonical repository 在既有 mutation gate 内先检查 finalized ledger 的 `(run_id, step_id, fingerprint)`；仅当无已提交 outcome 时检查 `expected_revision`；
- 用 candidate revision 创建 `FinalizedOutcomeProjection`，将 `ContextAppend` 的 cause/messages/receipts/usage/fingerprint 拷入，随后 collect snapshot、durable save、再 publish；
- outcome ledger 与 accepted-input ledger 继续独立，保证同一 Step 可先 accepted 再 finalized；
- in-memory `SessionState` 改为保存结构化 step fixture 或等价 outcome projection 状态，禁止仅通过 `messages.extend` 掩盖字段丢失；
- clear 同时清空 accepted/outcome idempotency 状态；
- 不改变 compact 算法，只让它经既有 structured messages 读取正式 projection 的 messages。

- [ ] **Step 5: 运行 Context backing 契约确认通过**

Run: `cargo test -p context --test canonical_session_repository --test in_memory_session_backing --test session_envelope_codec -- --nocapture`

Expected: PASS。

## Task 4: 收口 Runtime coordinator 的 fingerprint 与 finalized append 语义

**Files:**
- Modify: `agent/features/runtime/src/application/context_coordination.rs`
- Test: `agent/features/runtime/src/application/context_coordination_tests.rs`

- [ ] **Step 1: 写 coordinator 失败测试**

在 Recording ContextPort 上提交 completed、cancelled 和 terminated 的 finalized facts，断言：
- ContextAppend 完整传递 cause、messages、原序 receipts、usage 与 request/window revision；
- fingerprint 对 cause、messages、receipts、usage 变化敏感，相同语义输入稳定；
- accepted input 不参与 outcome fingerprint，也不由 coordinator 重新写入；
- ContextPort 返回冲突或 revision error 时原样传播。

- [ ] **Step 2: 运行失败测试确认红灯**

Run: `cargo test -p runtime application::context_coordination_tests::finalized_outcome_fingerprint_covers_cause_receipts_and_usage -- --exact`

Expected: FAIL，因 coordinator 的测试尚未锁定正式 outcome projection 语义。

- [ ] **Step 3: 实现唯一 finalized-outcome façade**

- `ContextCoordinator::append_finalized_outcome` 接收仅为构造 `ContextAppend` 所需的 finalized facts，并从 `ContextWindow.backing_revision` 设置 CAS；
- 使用确定性编码计算 fingerprint，字段顺序固定为 finalize cause、messages、receipts、usage；
- 保持 Main/Sub 都只消费 Runtime re-export 的 Context PL，Runtime 不读取 envelope 或 session internals；
- 不在此任务改变 #1272 的 drain/seal 或 #1247 的 control deadline。

- [ ] **Step 4: 运行 Runtime coordinator 测试确认通过**

Run: `cargo test -p runtime --lib application::context_coordination -- --nocapture`

Expected: PASS。

## Task 5: 接入 Main/Sub finalized facts，并锁定 Stop Hook 边界

**Files:**
- Modify: `agent/features/runtime/src/application/chat/looping/main_run_port.rs`
- Modify: `agent/features/runtime/src/application/agent/runner/loop_run.rs`
- Test: Main/Sub 相邻 loop tests

- [ ] **Step 1: 写 Main adapter 失败测试**

构造已 accepted 的 Main Step，并分别模拟 normal completion、取消 partial 与 Stop Hook Block：
- normal 将 assistant 和按原 ToolCall 顺序的 terminal results、receipt、usage 提交；
- cancelled partial 使用 `FinalizeCause::UserCancelledStep`，保留已确认成功和 `CancellationUnconfirmed`；
- Stop Hook Block 仍以 `FinalizeCause::Completed` 提交当前 assistant/Tool outcome；feedback 不混入本 Step append，而留给下一 Step 输入。

- [ ] **Step 2: 写 Sub adapter 失败测试**

构造 Sub completed 与 terminated partial Step，断言它们同样通过 ContextCoordinator 提交正式 outcome；父仅接收 child terminal receipt 对应的稳定 Tool result，不把 Sub 私有完整消息链写入父 Session。

- [ ] **Step 3: 运行 Main/Sub 测试确认失败**

Run: `cargo test -p runtime --lib finalized_outcome stop_hook_block -- --nocapture`

Expected: FAIL，因 adapter 相邻测试尚未覆盖正式 projection 或 Stop Hook 更正口径。

- [ ] **Step 4: 实现 Main/Sub adapter 收口**

- Main/Sub 的现有 finalized message ownership 只传入 `ContextCoordinator`，由它构造 outcome append；
- 保留工具 result 的 provider 协议原顺序；单个 Tool 业务失败不得抹除同批成功事实；
- normal、cancelled 和 terminated 一律走同一 Context schema；控制路径仅改变 `FinalizeCause`、partial/receipt 内容，不建立第二 writer；
- Stop Hook Block 先提交当前 outcome，再把 feedback 交给下一次 drain 的 pending input；
- 不改动 #1272 的 `DrainDecision` / epoch/seal 唯一 admission，也不提前实现 #1247 的生产控制入口。

- [ ] **Step 5: 运行 Main/Sub 相邻测试确认通过**

Run: `cargo test -p runtime --lib finalized_outcome -- --nocapture`

Expected: PASS。

## Task 6: 增加 L4 恢复场景、回写 Target，并执行完整验证

**Files:**
- Modify: `agent/features/context/tests/main_session_wiring.rs`
- Modify: `agent/features/runtime/tests/...`（仅新增证明 Main/Sub → Context → resume 的场景文件）
- Modify: `docs/design/02-modules/context-management/01-session.md`
- Modify: `docs/design/02-modules/context-management/02-compact.md`
- Modify: `docs/design/02-modules/runtime/03-loop-and-state-machine.md`
- Modify: `docs/design/03-engineering/03-migration-governance.md`
- Modify: GitHub Issue #1278 / #1275

- [ ] **Step 1: 写 L4 finalized-outcome recovery 失败测试**

构造 accepted input、提交 completed/cancelled partial/terminated 的三个 Session fixture，重新构造/resume 后断言：
- accepted input 和正式 outcome messages 均可被 ContextWindow 读取一次；
- cancelled/terminated 的 receipts、cause、usage 与 revision 仍可从 structured Step 读取；
- compact marker 后 projection 不丢失新 finalized Step；
- resume 不恢复 Run、RunStatus、future、interaction 或 cancellation scope；
- Stop Hook Block 已完成 assistant/Tool 可见，而 feedback 只属于后续 Step。

- [ ] **Step 2: 运行 L4 测试确认失败**

Run: `cargo test -p context --test main_session_wiring finalized_outcome_survives_resume_without_runtime_state -- --exact`

Expected: FAIL，因 recovery fixture 尚未断言正式 outcome metadata。

- [ ] **Step 3: 实现最小集成接线并确认通过**

Run:

```bash
cargo test -p context --test main_session_wiring --test canonical_session_repository -- --nocapture
cargo test -p runtime --lib finalized_outcome -- --nocapture
```

Expected: PASS。

- [ ] **Step 4: 回写设计与迁移治理**

- Session 文档将 `CompatibilityOutcome` 替换为正式 `FinalizedOutcomeProjection`，明确 accepted/outcome 的独立幂等和不覆写关系；
- Compact 文档明确 compact 只读 outcome messages，但保留 Step identity 和 projection metadata；
- Runtime 文档明确 normal/control 共用唯一 schema，Stop Hook Block 持久化当前 outcome、feedback 属下一 Step；
- Migration Governance R10 记录 #1278 已收口 compatibility bridge，#1247 负责生产 control command，#879 负责旧 runtime 路径退役；
- 回填 #1278 / #1275 的 L1–L4 证据、legacy/recovery/Stop Hook 更正验证和未完成 owner。

- [ ] **Step 5: 执行格式、测试、静态检查与架构守卫**

Run:

```bash
cargo fmt --check
cargo test -p context --lib --tests
cargo test -p runtime --lib
cargo test --workspace
cargo check
cargo clippy --all-targets -- -D warnings
bash .agents/hooks/check-architecture-guards.sh
git diff origin/main...HEAD --check
git grep -n 'CompatibilityOutcome\|outcome: Option<Vec<Message>>' -- agent/features/context
```

Expected: 所有命令 exit 0；最终 grep 不应在常规 v2 writer/reader 中命中 compatibility bridge，只允许明确 legacy decoder 或迁移注释。首次失败必须保留完整输出；定向重跑只能用于 flaky 分类。

- [ ] **Step 6: 按任务提交**

```bash
git add agent/features/context agent/features/runtime docs/design
git commit -m "feat(context): #1278 持久化 finalized RunStep outcome"
```
