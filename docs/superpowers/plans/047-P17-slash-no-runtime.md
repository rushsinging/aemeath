# Feature 47 P17: slash.rs 去 runtime 依赖

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** `tui/core/slash/` 目录下所有文件不再直接 `use runtime::api`，slash 命令执行和 model 列表全部走 SDK `AgentClient`。

## 背景

当前残留：
- `slash.rs` — 已大部分走 SDK，但仍有间接 runtime 依赖
- `slash/suggestions.rs` — `tokio::runtime::Handle::current()` 调用（可接受，是 tokio 非 aemeath runtime）
- `slash/dialog.rs` — `tokio::runtime::Handle::current()` 调用

需确认 slash 内部所有命令分发都已走 `agent_client.execute_command()` 而非直接调用 runtime。

## 步骤

- [ ] **1. 扫描 slash/ 下所有 `runtime::api` 引用**
  - 确认 `slash.rs`、`suggestions.rs`、`dialog.rs`、`help.rs`、`reflection.rs`、`memory.rs`、`save.rs` 中是否有残留

- [ ] **2. 逐个消除**
  - 若有直接 `runtime::api` 引用，替换为 `sdk::AgentClient` 方法调用
  - `tokio::runtime::Handle` 不是 aemeath runtime，可保留

- [ ] **3. 补齐 SDK 缺失方法（如有）**
  - 若 slash 命令需要 SDK 尚未暴露的能力，在 `AgentClient` trait 新增方法

- [ ] **4. 验证**
  - `grep -rn 'runtime::api' apps/cli/src/tui/core/slash/` 返回空
  - `cargo build -p cli` 编译通过
  - `/help`、`/model`、`/reflect`、`/memory`、`/save` 功能正常
