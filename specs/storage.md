# 持久化（Storage）

**Scope**：`agent/features/storage/**`——memory、task、history、tool_result 的持久化。
**主触发**：改 `agent/features/storage/**`。
**次触发**：改会话 / 记忆 / 任务 / 历史的落盘格式或路径。

## 子域与落盘位置

- **Memory**：`agent/features/storage/src/business/memory/`（`store.rs`、`path.rs`）→ `~/.agents/memory/`。
- **Task**：`agent/features/storage/src/business/task/`（`store.rs`、`list.rs`、`batch.rs`、`types.rs`、`display.rs`）——任务追踪持久化。
- **History**：`agent/features/storage/src/business/history.rs` → `~/.agents/history.json`（用户输入历史）。
- **Tool result**：`agent/features/storage/src/business/tool_result_storage.rs`——大体积 tool 结果落盘（默认上限见 MCP/工具配置）。
- 会话持久化目录：`~/.agents/sessions/`。

改落盘格式或路径时，**MUST** 兼顾已有数据的可读性，避免破坏现有 `~/.agents/` 下的用户数据。
