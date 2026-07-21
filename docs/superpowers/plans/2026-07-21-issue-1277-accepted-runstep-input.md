# #1277 Accepted RunStep Input Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development (recommended) or executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 #1282 的结构化 Session backing 上，使已绑定到 `RunStepId` 的 user input 在首次 Context build 前 durable，并在后续 finalized outcome 上补充同一 Step。

**Architecture:** Context 发布独立的 `AcceptedInputAppend/AcceptedInputReceipt`，其幂等键是 `(run_id, step_id)`、指纹只覆盖 accepted user messages。canonical backing 将 input 原子写到既有 `CommittedRunStep.accepted_input`；in-memory backing 以等价语义服务测试。共享 Loop 在 `freeze_step` 后调用新的异步 `accept_step_input` hook、在 `needs_compaction` 前等待完成；Main/Sub adapter 从各自已经 freeze 的 user-only 消息投影调用 ContextCoordinator。#1278 仍拥有 finalized outcome 的正式 payload 升级。

**Tech Stack:** Rust、async_trait、Context Port/Repository、Runtime shared Loop、Cargo tests。

---

## 文件结构

- Modify: `agent/features/context/src/domain.rs` — accepted input append/receipt 与独立 conflict 错误语义。
- Modify: `agent/features/context/src/ports/{context_port.rs,rs}` — ContextPort / SessionRepository accepted-input 方法。
- Modify: `agent/features/context/src/application/service.rs` — 端口委托。
- Modify: `agent/features/context/src/adapters/{canonical_session.rs,in_memory_session.rs}` — durable backing、CAS、幂等、publish 顺序。
- Modify: `agent/features/context/src/domain/session/envelope.rs` — 将 accepted input 写入已有 Step、marker 后可见性。
- Modify: `agent/features/runtime/src/application/context_coordination.rs` — Runtime façade、指纹计算。
- Modify: `agent/features/runtime/src/application/loop_engine/{engine.rs,tests.rs}` — `freeze_step → accept_step_input → needs_compaction` 共享时序。
- Modify: `agent/features/runtime/src/application/chat/looping/main_run_port.rs` — Main adapter 提取 user-only accepted facts。
- Modify: `agent/features/runtime/src/application/agent/runner/loop_run.rs` — Sub adapter 提取 user-only accepted facts。
- Modify: Context/Runtime 既有测试与 fake 实现 — 新端口契约。
- Modify: `docs/design/02-modules/{context-management/01-session.md,runtime/03-loop-and-state-machine.md}`、`docs/design/03-engineering/03-migration-governance.md` — 两阶段语义与迁移责任。

## Task 1: 建立 accepted-input Published Language 与 Context Port

**Files:**
- Modify: `agent/features/context/src/domain.rs`
- Modify: `agent/features/context/src/ports/{context_port.rs,rs}`
- Modify: `agent/features/context/src/application/service.rs`
- Test: `agent/features/context/tests/context_port_contract.rs`
- Test: `agent/features/context/tests/application_service_contract.rs`

- [ ] **Step 1: 写失败的 ContextPort 契约测试**

新增 accepted append fixture，断言 `ContextPort::append_accepted_input` 返回独立 receipt，包含 run、step、revision、input fingerprint；并断言 accepted append 不接收 `FinalizeCause`、receipts、usage 或 outcome messages。

- [ ] **Step 2: 运行契约测试确认失败**

Run: `cargo test -p context --test context_port_contract accepted_input_port_returns_typed_receipt -- --exact`

Expected: FAIL，因 ContextPort 尚无 accepted-input 方法。

- [ ] **Step 3: 实现最小 PL 与端口**

- 新增 `AcceptedInputAppend { session_id, run_id, step_id, source_request_id, messages, fingerprint }`；
- 新增 `AcceptedInputReceipt`；
- 新增 `AcceptedInputError`，独立表达 content conflict、session missing 与 storage failure；
- 在 `ContextPort` 与 `SessionRepository` 增加 `append_accepted_input`；
- `ContextApplicationService` 只委托 repository；更新所有 fake 以显式实现该方法。

- [ ] **Step 4: 运行 Context Port 契约测试确认通过**

Run: `cargo test -p context --test context_port_contract --test application_service_contract -- --nocapture`

Expected: PASS。

## Task 2: 实现 canonical / in-memory accepted-input durable backing

**Files:**
- Modify: `agent/features/context/src/domain/session/envelope.rs`
- Modify: `agent/features/context/src/adapters/canonical_session.rs`
- Modify: `agent/features/context/src/adapters/in_memory_session.rs`
- Test: `agent/features/context/tests/canonical_session_repository.rs`
- Test: `agent/features/context/tests/in_memory_session_backing.rs`
- Test: `agent/features/context/tests/session_envelope_codec.rs`

- [ ] **Step 1: 写 canonical backing 的失败测试**

测试 accepted input 在 writer 成功后才发布，并断言：
- `CommittedRunStep.accepted_input` 保存 user messages、fingerprint、committed revision；
- `outcome` 为 `None`；
- snapshot / ContextWindow 能立即看到 accepted message；
- 相同 key 与相同 fingerprint 返回原 receipt；不同 fingerprint 返回 typed conflict；
- failed writer 不发布候选 session。

- [ ] **Step 2: 写 in-memory 等价语义失败测试**

断言 in-memory backing 对 accepted append 提供同样的 idempotent/conflict/revision 行为，并在 snapshot 返回 accepted message。

- [ ] **Step 3: 运行 backing 测试确认失败**

Run: `cargo test -p context --test canonical_session_repository accepted_input_persists_before_publish -- --exact`

Expected: FAIL，因 repository 没有 accepted writer。

- [ ] **Step 4: 实现 backing 与 Session helper**

- 在 `CanonicalSession` 实现只写 accepted facts 的 helper：若同 Step 已有 outcome bridge，补入 `accepted_input` 不覆盖 outcome；若 Step 不存在，创建 accepted-only Step；若 marker 无 tail，设置首个可见 cursor；
- canonical repository 在 mutation gate 内检查 key/指纹、读取当前 revision、collect Task/Workspace snapshot、writer save、再 publish；
- in-memory backing 以独立 accepted ledger 保存同样语义；
- accepted input 的 fingerprint/revision 不能复用 finalized outcome ledger key，以便同 Step 后续补 outcome；
- v2 codec round-trip 保留 accepted-only Step。

- [ ] **Step 5: 运行 Context backing 契约确认通过**

Run: `cargo test -p context --test canonical_session_repository --test in_memory_session_backing --test session_envelope_codec -- --nocapture`

Expected: PASS。

## Task 3: 增加 Runtime accepted-input coordinator 与 shared loop handoff

**Files:**
- Modify: `agent/features/runtime/src/application/context_coordination.rs`
- Modify: `agent/features/runtime/src/application/loop_engine/{engine.rs,tests.rs}`
- Modify: `agent/features/runtime/src/ports.rs`
- Test: `agent/features/runtime/src/application/context_coordination_tests.rs`
- Test: `agent/features/runtime/src/application/loop_engine/tests.rs`

- [ ] **Step 1: 写 coordinator 的失败测试**

在 Recording ContextPort 上断言 `ContextCoordinator::append_accepted_input`：使用 frozen request 的 session/run/step、只传 accepted user messages、产生稳定 fingerprint、返回 typed accepted receipt。

- [ ] **Step 2: 写 shared loop 顺序失败测试**

扩展 `ScriptedPort`，新增异步 `accept_step_input` 记录。断言一轮调用顺序为：

```text
input → freeze_step → accept_step_input → needs_compaction → model
```

且 accepted handoff 失败时 loop 不调用 model。

- [ ] **Step 3: 运行失败测试确认红灯**

Run: `cargo test -p runtime application::loop_engine::tests::engine_accepts_input_before_building_context -- --exact`

Expected: FAIL，因 shared `RunLoopPort` 没有 accepted handoff。

- [ ] **Step 4: 实现 coordinator 与 shared hook**

- `ContextCoordinator` 增加 accepted append 与只覆盖 accepted fields 的 fingerprint helper；
- `RunLoopPort` 增加 async `accept_step_input` 默认 no-op；
- shared engine 在 `freeze_step` 后，使用同一 interrupt/deadline 机制 await accepted handoff；失败立即终止，不开始 `needs_compaction` / model；
- Runtime ports re-export 新 Context PL；更新测试 fake。

- [ ] **Step 5: 运行 Runtime unit tests 确认通过**

Run: `cargo test -p runtime --lib application::loop_engine application::context_coordination -- --nocapture`

Expected: PASS。

## Task 4: 接入 Main / Sub 已绑定 user input

**Files:**
- Modify: `agent/features/runtime/src/application/chat/looping/main_run_port.rs`
- Modify: `agent/features/runtime/src/application/agent/runner/loop_run.rs`
- Test: Main/Sub 相邻 loop tests

- [ ] **Step 1: 写 Main adapter 失败测试**

构造 Main frozen Step，断言 accepted handoff 调用 ContextCoordinator，messages 只含 `StepMessageOwnership::freeze` 接收的 user facts；不带 assistant/tool/stop-hook feedback outcome。

- [ ] **Step 2: 写 Sub adapter 失败测试**

构造 Sub frozen Step，断言 accepted handoff 使用 `committed_message_count` 后的当前 user input，且在 `needs_compaction` 前完成。

- [ ] **Step 3: 运行 Main/Sub 失败测试确认红灯**

Run: `cargo test -p runtime --lib accepted_input -- --nocapture`

Expected: FAIL，适配器未实现 `accept_step_input`。

- [ ] **Step 4: 实现 Main / Sub adapter**

- Main 保存 freeze 后的 accepted user-only vector，`accept_step_input` 使用 `context_request`、该 vector 与 ContextCoordinator；accepted 成功后不清除 StepMessageOwnership，因为 #1278 仍需它形成 outcome；
- Sub 在 freeze 时捕获本 Step 新输入，仅将 user facts 传入；
- 不改变 #1272 drain epoch/seal、Run FSM transition、finalize/cancel semantics；
- 不在 Runtime 引用 Context session internals。

- [ ] **Step 5: 运行 Main/Sub 相邻测试确认通过**

Run: `cargo test -p runtime --lib main_run_port loop_run accepted_input -- --nocapture`

Expected: PASS。

## Task 5: 端到端恢复、文档与验证

**Files:**
- Modify: `agent/features/context/tests/main_session_wiring.rs`
- Modify: `agent/features/runtime/tests/...`（仅新增可证明 Main/Sub handoff 的场景文件）
- Modify: `docs/design/02-modules/context-management/01-session.md`
- Modify: `docs/design/02-modules/runtime/03-loop-and-state-machine.md`
- Modify: `docs/design/03-engineering/03-migration-governance.md`

- [ ] **Step 1: 写 L4 failed-before-outcome 场景测试**

完成 accepted append 后令 model invocation 返回错误，重新构造/resume Session 并断言 accepted user input 可见、Step outcome 仍为 `None`、没有 active Run 恢复。

- [ ] **Step 2: 实现最小集成接线并验证通过**

Run: `cargo test -p context --test main_session_wiring --test canonical_session_repository -- --nocapture && cargo test -p runtime --lib accepted_input -- --nocapture`

Expected: PASS。

- [ ] **Step 3: 回写 Target 与 Migration Governance**

- Session 文档明确 accepted input 在 freeze/bind 后 durable、outcome 后补且不覆盖 input；
- Runtime 文档固定 `freeze_step → accepted append → build_window` 时序和 cancellation-shielded handoff；
- Migration Governance 记录 #1277 的 writer/adapter 状态及 #1278 outcome 收口责任；
- #1275/1277 checklist 回写 L1-L4 evidence 与剩余 owner。

- [ ] **Step 4: 完整验证**

Run:

```bash
cargo fmt --check
cargo test -p context --lib --tests
cargo test -p runtime --lib
cargo test --workspace
cargo check
cargo clippy --all-targets -- -D warnings
bash .agents/hooks/check-architecture-guards.sh
```

Expected: all exit 0. 首次失败必须保留；只允许定向重跑用于 flaky 分类。

- [ ] **Step 5: 提交**

```bash
git add agent/features/context agent/features/runtime docs/design
git commit -m "feat(context): #1277 持久化已接受 RunStep 输入"
```
