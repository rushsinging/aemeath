# Feature 47 P20: 最终守卫强化 + 归档

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** 强化架构守卫，确保 CLI↔runtime 边界不可回退；确认所有编译和测试通过；更新 active.md 归档 #47。

## 背景

P13-P19 完成后，CLI 只通过 `packages/sdk` 的 `AgentClient` trait 与 runtime 通信。唯一允许的 `runtime` 直接引用是 composition root 中的 `from_args()` 调用。需要通过守卫脚本固化这个边界。

## 步骤

- [ ] **1. 强化 `check-forbidden-imports.sh`**
  - 新增规则：`apps/cli/src/tui/**/*.rs` 中禁止 `use ::runtime::api`
  - 允许白名单：`apps/cli/src/run_orchestration.rs` 和 `apps/cli/src/main.rs`
  - 允许 `tokio::runtime`（不是 aemeath runtime）

- [ ] **2. 强化 `check-cli-thin-entry.sh`**
  - 确认 `apps/cli` 只直接依赖 `runtime` 和 `sdk`
  - 确认 `apps/cli/src/tui/` 下无 runtime import

- [ ] **3. 确认 runtime `client/` 拆分后每个文件 ≤ 400 行**
  - `wc -l agent/runtime/src/client/*.rs` 验证

- [ ] **4. 全量编译和测试**
  - `cargo build` — workspace 编译通过
  - `cargo test` — 全部测试通过
  - `cargo clippy` — 无新 warning
  - `.agents/hooks/check-architecture-guards.sh` 通过

- [ ] **5. 更新 `docs/feature/active.md` 中 #47 状态**
  - 将 #47 标记为"✅ 已完成"
  - 添加完成摘要：CLI↔runtime SDK 边界完成、TuiLaunchContext 删除、client.rs 拆分、架构守卫强化

- [ ] **6. 归档 P1-P20 计划文件**
  - 将 `docs/superpowers/plans/047-P*.md` 全部移动到 `docs/superpowers/plans/archived/047/`
  - 或保留在原位并标记全部 `[x]`（P1-P12 已打勾）

- [ ] **7. 更新 spec**
  - 在 `docs/feature/specs/047-ddd-redesign.md` 修订历史中追加"P13-P20 完成"记录
  - 更新 §11 当前推进状态

- [ ] **8. 验证**
  - 完整 smoke test：`cargo run -- <正常聊天流程>`
  - 守卫脚本通过
  - `docs/feature/active.md` #47 状态为已完成
