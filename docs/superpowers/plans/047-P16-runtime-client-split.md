# Feature 47 P16: runtime client.rs 拆分

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** 将 `agent/runtime/src/client.rs`（1266 行）按职责拆分为子模块，每个文件不超过 400 行。

## 背景

`client.rs` 当前包含：
- `from_args()` 编排（~200 行）— 配置加载→日志→模型→API key→LLM client→工具→hook→session→prompt→并发
- `AgentClientImpl` struct 定义 + `RuntimeHandle`（~80 行）
- `AgentClient` trait 实现（~500 行）— chat、session CRUD、command 执行、model switch、compact 等
- 类型映射函数（~200 行）— `message_to_sdk` / `message_from_sdk` / `session_summary_from_runtime` 等
- `SdkChatEventSink`（~80 行）
- 公共访问器（~80 行）— `tui_launch_context()`、`session_id()`、`cwd()` 等

## 目标结构

```
agent/runtime/src/client/
├── mod.rs            ← AgentClientImpl、RuntimeHandle 定义 + pub re-export
├── from_args.rs      ← from_args() 编排（步骤 1-19）
├── chat.rs           ← chat() 实现 + SdkChatEventSink + 事件映射
├── session.rs        ← load/list/save/delete_session 实现
├── command.rs        ← execute_command / switch_model / set_thinking 实现
├── mapping.rs        ← 类型映射函数（message_to_sdk 等）
└── accessors.rs      ← 公共访问器 + tui_launch_context（P15 后可大幅简化或删除）
```

## 步骤

- [ ] **1. 创建 `client/` 目录结构**
  - `mkdir agent/runtime/src/client/`
  - 从 `client.rs` 迁移内容到子模块

- [ ] **2. 拆 `mapping.rs`**
  - 提取 `message_to_sdk`、`message_from_sdk`、`session_summary_from_runtime`、`task_status_lines`、`format_task_status_line`、`model_display`、`memory_config_to_sdk`、`skill_to_sdk`、`processed_image_to_sdk`、`reflection_output_to_sdk`、`workspace_context_to_sdk`
  - 这些是纯函数，无依赖冲突

- [ ] **3. 拆 `from_args.rs`**
  - 提取 `from_args()` 函数 + `load_configured_skills()`
  - 引用 `mod.rs` 中的 struct 定义

- [ ] **4. 拆 `chat.rs`**
  - 提取 `SdkChatEventSink`、`EmptyQueueDrainPort`、`runtime_event_to_sdk_event`、`agent_progress_event_to_sdk`
  - `AgentClient::chat()` impl 块移入

- [ ] **5. 拆 `session.rs`**
  - 提取 `load_session`、`list_sessions`、`delete_session`、`save_current_session`、`sync_current_messages` impl 块

- [ ] **6. 拆 `command.rs`**
  - 提取 `execute_command`、`switch_model`、`set_thinking`、`compact_messages`、`estimate_context` impl 块 + `map_command_result`/`map_command_action`/`map_confirm_action`

- [ ] **7. 拆 `accessors.rs`**
  - 提取 `session_id()`、`cwd()`、`resolved_model()`、`context()`、`max_tool_concurrency()`、`max_agent_concurrency()`、`tui_launch_context()` 等

- [ ] **8. 编写 `mod.rs`**
  - `AgentClientImpl` + `RuntimeHandle` 定义
  - `pub mod` 声明
  - `pub use` re-export 保持对外 API 不变

- [ ] **9. 更新 `lib.rs`**
  - `pub mod client;` 替代原 `pub use crate::client;`（如需要）

- [ ] **10. 验证**
  - `cargo build` 编译通过
  - `cargo test -p runtime` 通过
  - 每个文件 ≤ 400 行
