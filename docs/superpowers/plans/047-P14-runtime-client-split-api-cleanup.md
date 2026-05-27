# Feature 47 P14: runtime client.rs 拆分 + api.rs 收口

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** `client.rs`（1349 行）按职责拆分为子模块（每个 ≤400 行）；`api.rs` 从全量 re-export 收口为只暴露 SDK 编排需要的类型。

## 当前状态

- `client.rs` 1349 行：`from_args()` 编排 + `AgentClientImpl` + trait 实现 + 类型映射 + `SdkChatEventSink` + 公共访问器
- `api.rs` 全量 re-export 所有 runtime 内部模块 + 所有 supporting domain crate

## 步骤

### Part A：client.rs 拆分

- [ ] **1. 创建 `client/` 目录**
  ```
  agent/runtime/src/client/
  ├── mod.rs            ← AgentClientImpl + RuntimeHandle 定义 + pub re-export
  ├── from_args.rs      ← from_args() 编排
  ├── chat.rs           ← chat() + SdkChatEventSink + 事件映射
  ├── session.rs        ← load/list/save/delete_session
  ├── command.rs        ← execute_command / switch_model / set_thinking / compact
  ├── mapping.rs        ← message_to_sdk / message_from_sdk 等类型映射纯函数
  └── accessors.rs      ← session_id / cwd / resolved_model / tui_launch_context 等
  ```

- [ ] **2. 提取 `mapping.rs`**（纯函数，无依赖冲突）
  - `message_to_sdk`、`message_from_sdk`、`session_summary_from_runtime`、`task_status_lines`、`model_display`、`memory_config_to_sdk`、`skill_to_sdk`、`processed_image_to_sdk`、`reflection_output_to_sdk`、`workspace_context_to_sdk`

- [ ] **3. 提取 `from_args.rs`**
  - `from_args()` + `load_configured_skills()`

- [ ] **4. 提取 `chat.rs`**
  - `SdkChatEventSink`、`EmptyQueueDrainPort`、`runtime_event_to_sdk_event`、`agent_progress_event_to_sdk`、`AgentClient::chat()` impl

- [ ] **5. 提取 `session.rs`**
  - `load_session`、`list_sessions`、`delete_session`、`save_current_session`、`sync_current_messages`

- [ ] **6. 提取 `command.rs`**
  - `execute_command`、`switch_model`、`set_thinking`、`compact_messages`、`estimate_context` + `map_command_result`/`map_command_action`/`map_command_confirm_action`

- [ ] **7. 提取 `accessors.rs`**
  - `session_id()`、`cwd()`、`resolved_model()`、`context()`、`max_tool_concurrency()`、`max_agent_concurrency()`、`tui_launch_context()` 等

- [ ] **8. 编写 `mod.rs`**
  - `AgentClientImpl` + `RuntimeHandle` 定义 + `pub mod` + `pub use` re-export

### Part B：api.rs 收口

- [ ] **9. `api.rs` 从全量 re-export 收口为按需导出**
  - 当前全量 re-export 了 16 个内部模块 + 11 个 supporting domain crate
  - 改为只 re-export composition root（CLI `run_orchestration.rs`）需要的：
    - `client`（`from_args` 入口）
    - `bootstrap`（如 `init_panic_hook` 未移入 `from_args`）
  - runtime 内部模块互相引用改用 `crate::` 路径，不依赖 `api.rs` re-export

- [ ] **10. 删除 `tui_launch.rs`**（P13 已删除 TuiLaunchContext 消费者）

### Part C：验证

- [ ] **11. 每个子模块 ≤ 400 行**
- [ ] **12. `cargo build` + `cargo test -p runtime` 通过**
- [ ] **13. `cargo test -p cli` 通过（确保 API 收口未破坏 CLI）**
