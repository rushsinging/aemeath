# Skill Load Dedup v2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 基于最新 `origin/main` 重新接入被 revert 的 Skill revision 去重能力，并解决主线后续变更带来的冲突。

**Architecture:** 以 `5d4a5bee` 的 revert commit 为恢复基线，在最新主线执行反向应用；冲突优先保留主线已有的 Runtime/Context/Tools 修复，再恢复 SkillLoadScope、Context 原子持久化和 schema 兼容能力。重新验证从 Domain、Context、Runtime、Tools 到 workspace/pre-push 的完整链路。

**Tech Stack:** Rust workspace、Tokio、Serde、Cargo test/clippy、GitHub CLI。

---

### Task 1: 重新应用被 revert 的实现并定位冲突

**Files:**
- Modify: 被 `5d4a5bee` 反向删除的 Skill load implementation 文件

- [ ] 在最新 `origin/main` 上执行 `git revert --no-commit 5d4a5bee`。
- [ ] 收集所有冲突文件，逐个对照 `5d4a5bee^`、当前主线和旧实现，保留主线后续改动并恢复 Skill 去重逻辑。
- [ ] 执行 `git diff --check`，确认不存在未解决冲突标记。

### Task 2: 补齐最新主线下的回归测试

**Files:**
- Test: `agent/features/context/tests/canonical_session_repository.rs`
- Test: `agent/features/context/tests/session_envelope_codec.rs`
- Test: `agent/features/tools/src/adapters/skill_tool_contract_tests.rs`
- Test: `agent/features/runtime/src/application/runtime_context_factory_tests.rs`

- [ ] 确认以下行为在当前主线通过测试：Main 跨 Run 去重、Sub-agent 实例隔离、revision 更新返回正文、状态持久化失败不泄漏正文、Resume/compact/clear、并发加载最多一次 Fresh。
- [ ] 若冲突删除了测试，恢复对应测试并调整为当前主线 API；不新增与本 Issue 无关的测试。

### Task 3: 验证跨层实现和架构守卫

**Files:**
- Modify: 仅在测试或编译暴露当前主线冲突时涉及的实现文件

- [ ] 运行 `cargo fmt --check`。
- [ ] 运行 `cargo clippy -p tools -p context -p runtime -p composition --all-targets --all-features -- -D warnings`。
- [ ] 运行 `.agents/hooks/check-architecture-guards.sh --full`。
- [ ] 运行 `cargo test --workspace --quiet` 和 `.agents/hooks/check-unit-tests.sh`。
- [ ] 检查 `git diff --check`、未解决冲突标记和 `run_id` 误用。

### Task 4: 提交、推送并创建新的 PR

**Files:**
- No additional source files

- [ ] 提交冲突解决后的实现，提交信息引用 Issue #1460。
- [ ] 推送新分支 `feature/1460-skill-load-dedup-v2`。
- [ ] 创建新的 PR 到 `main`，说明这是对已合入后被 revert 的 #1466 的重新实现，列出冲突解决方式和验证命令。
- [ ] 查询新 PR 的 base/head、状态、mergeable 状态和 CI 初始状态。
