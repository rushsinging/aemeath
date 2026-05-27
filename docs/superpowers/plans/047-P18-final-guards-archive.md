# Feature 47 P18: 守卫强化 + active.md 更新 + 归档

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** 架构守卫固化所有 DDD 边界；确认全量编译测试通过；更新 active.md 标记 #47 完成；归档 P1-P18 计划文件。

## 步骤

### Part A：守卫强化

- [ ] **1. CLI import 守卫**
  - `apps/cli/src/tui/**/*.rs` 中禁止 `use ::runtime::`（`tokio::runtime` 除外）
  - 允许白名单：`run_orchestration.rs`、`main.rs`（composition root）

- [ ] **2. runtime COLA 层间守卫**
  - `domain/` 中禁止 `use provider::` / `use tools::` / `use storage::` / `use hook::`
  - `domain/` 只允许 `use share::`（core）和 `crate::domain::` 内部引用

- [ ] **3. supporting domain 反向依赖守卫**
  - supporting domain crate 不得依赖 `runtime`、`cli`
  - 通过 `cargo metadata` 检查 Cargo.toml 依赖图

- [ ] **4. 文件行数守卫**
  - 所有 `.rs` 文件 ≤ 400 行

### Part B：全量验证

- [ ] **5. `cargo build` workspace 编译通过**
- [ ] **6. `cargo test` 全部通过**
- [ ] **7. `cargo clippy` 无新 warning**
- [ ] **8. `.agents/hooks/check-architecture-guards.sh` 通过**

### Part C：文档归档

- [ ] **9. 更新 `docs/feature/active.md` 中 #47 状态**
  - 标记为"✅ 已完成"
  - 添加完成摘要

- [ ] **10. 归档 P1-P18 计划文件**
  - 移动到 `docs/superpowers/plans/archived/047/`

- [ ] **11. 更新 spec 修订历史**
  - 在 `docs/feature/specs/047-ddd-redesign.md` 追加 P13-P18 完成记录
  - 更新当前推进状态

- [ ] **12. Smoke test**
  - `cargo run -- <正常聊天流程>`
  - 守卫脚本全部通过
