# Feature 47 P19: 删除 TuiLaunchContext + CLI 完全 SDK 化

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** 删除 `TuiLaunchContext` 过渡结构，CLI 代码中不再有任何 `runtime::api` 引用（composition root 的 `from_args` 调用除外）。`packages/sdk` 成为 CLI 与 runtime 之间的唯一通信契约。

## 背景

P13-P18 完成后，CLI 对 runtime 的直接依赖应只剩：
- `main.rs`: `runtime::api::bootstrap::init_panic_hook()`
- `run_orchestration.rs`: `runtime::api::client::from_args()` + `runtime::api::command::commands::init_all()`

如果 P18 把 `init_all()` 和 `init_panic_hook()` 移入 `from_args()`，CLI 对 runtime 的直接调用只剩 `from_args()` 一处。

## 步骤

- [ ] **1. 确认 `TuiLaunchContext` 已无消费者**
  - P15 已简化 `App::run()` 签名
  - `grep -rn 'TuiLaunchContext' apps/cli/` 应返回空

- [ ] **2. 删除 `agent/runtime/src/tui_launch.rs`**
  - 删除文件，从 `lib.rs` 移除 `pub mod tui_launch`

- [ ] **3. 确认 `set_skills` 走 SDK**
  - `app.set_skills(launch.skills_map)` 应改为通过 AgentClient 获取或 SDK DTO

- [ ] **4. 全局扫描 CLI 中 `runtime::api` 残留**
  - `grep -rn 'runtime::api' apps/cli/src/` 预期只剩 `run_orchestration.rs` 中的 `from_args` 调用
  - 如果有其他残留，逐个消除

- [ ] **5. 确认 `packages/sdk` 公共 API 完整性**
  - 所有 CLI 用到的类型都可在 `sdk::` 下找到
  - 无裸 `runtime::` 类型泄露到 CLI

- [ ] **6. 验证**
  - `cargo build -p cli` 编译通过
  - `cargo test -p cli` 通过
  - 完整 TUI 流程：启动 → 聊天 → 工具调用 → slash 命令 → 退出 → resume
