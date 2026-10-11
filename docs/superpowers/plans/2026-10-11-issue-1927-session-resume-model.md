# #1927 Session Resume Model 写读闭环

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans。步骤用 `- [ ]`。Commit 仅在用户要求时执行。

**Goal:** `--resume` 无 CLI `--model` 时恢复 `session.metadata.model`；启动与 `/model` 成功后回写该字段。

**Architecture:** 复用已有 `SessionMetadata.model`。扩展 `SessionMetadataUpdateData` 写入；resume 视图透出 model；composition/runtime 选型优先级为 CLI > session > models.default；写失败与解析失败 fail-open。

**Tech Stack:** Rust / tokio / cargo test

---

### Task 1: SessionMetadataUpdateData.model

**Files:**
- Modify: `agent/features/context/src/domain/session/management.rs`
- Modify: `agent/features/context/src/domain/session/management_tests.rs`（或同文件 tests）
- Modify: `agent/features/context/src/integration_tests/session_management_port_contract.rs`

- [x] 写失败测试：`apply` 设置 model；`update_metadata_for_project` 读回
- [x] 跑测确认 RED
- [x] 实现 `model: Option<String>` + `apply`
- [x] 跑测确认 GREEN

### Task 2: Resume 选型读 session model

**Files:**
- Modify: `agent/features/context/src/domain/session/management.rs`（`SessionResumeViewData` 加 `model`）
- Modify: `agent/features/context/src/adapters/session_resume.rs`
- Modify: `agent/composition/src/runtime.rs` 或 `agent/features/runtime/src/application/client/startup_resume.rs` / `from_args`
- Test: composition 或 runtime resume 场景测试

- [x] 写失败测试：无 CLI model + session.metadata.model=X → resolve 用 X（`startup_model_override`）
- [x] 写失败测试：有 CLI model → 用 CLI，不受 session 影响
- [x] 写失败测试：空白 session model 忽略；无效 model fail-open 回退默认（composition 实现）
- [x] 实现：composition `load_resume_session_model` + `startup_model_override`（未改 SessionResumeViewData）
- [x] GREEN

### Task 3: 启动与 SwitchModel 回写

**Files:**
- Modify: `agent/features/runtime/src/application/loop_engine/chat/session_driver/run_launch.rs`（SwitchModel 成功后 update_metadata）
- Modify: composition/runtime bootstrap（最终 model 确定后回写）
- Tests: runtime SwitchModel 回写；bootstrap 回写

- [x] 写失败测试：`persist_session_model` 写入 metadata.model；空白跳过
- [x] 实现：启动 `from_args` 回写；SwitchModel 成功后回写 selection
- [x] GREEN（helper 单测；SwitchModel 走同一 helper）

### Task 4: 验证

- [x] `cargo test -p context/runtime/composition` 定向全绿（metadata apply、contract 落盘、persist helper、startup_model_override）
- [x] `cargo clippy -p context --lib -- -D warnings` 绿；runtime/composition 被 main 既有 dead_code/manual_contains 挡住（错误不在本分支改动文件）
- [ ] 对照 issue #1927 checklist（PR 前勾）
